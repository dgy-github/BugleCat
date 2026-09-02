"""Tests for the MCP connector's pure logic (offline, fake session)."""

from __future__ import annotations

import textwrap
from pathlib import Path

from nanocodex.sandbox.approval import NEVER, ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext
from nanocodex.tools.mcp import (
    McpTool,
    build_tools_for_session,
    discover_mcp_servers,
    extract_structured,
    extract_text,
    format_result,
    is_readonly_mcp_tool,
    parse_mcp_servers,
)


def _ctx(tmp_path: Path) -> ToolContext:
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def cb(_req):
        return True

    return ToolContext(workspace=tmp_path, policy=policy,
                       approver=Approver(ON_REQUEST, cb),
                       executor=make_executor(policy), plan=[])


# --- result extraction (handles object- and dict-shaped results) ----------


class _Block:
    def __init__(self, type, text=None):
        self.type = type
        self.text = text


class _Result:
    def __init__(self, content, is_error=False):
        self.content = content
        self.isError = is_error


def test_extract_text_object_shape():
    res = _Result([_Block("text", "hello"), _Block("text", "world")])
    assert extract_text(res) == "hello\nworld"


def test_extract_text_dict_shape():
    res = {"content": [{"type": "text", "text": "hi"}], "isError": False}
    assert extract_text(res) == "hi"


def test_extract_text_marks_non_text():
    res = _Result([_Block("image"), _Block("text", "caption")])
    out = extract_text(res)
    assert "image content omitted" in out
    assert "caption" in out


def test_extract_text_error_prefixed():
    res = _Result([_Block("text", "boom")], is_error=True)
    assert extract_text(res).startswith("Error from MCP tool")


def test_extract_text_empty():
    assert extract_text(_Result([])) == "(no content)"


# --- structuredContent extraction + formatting ----------------------------
# Regression: the windows executor returns "18 window(s) found." as TEXT but the
# actual window list (titles, numeric handles) ONLY in structuredContent. If we
# drop the structured layer the model can never learn a real window_id, so
# focus_window then gets a bogus id -> "unknown process". format_result must
# surface the structured payload alongside the text.


class _StructResult:
    def __init__(self, content, structured_content=None, is_error=False):
        self.content = content
        self.structuredContent = structured_content
        self.isError = is_error


def test_extract_structured_object_shape():
    res = _StructResult([_Block("text", "18 window(s) found.")],
                        structured_content={"ok": True, "windows": [{"window_id": "12345"}]})
    sc = extract_structured(res)
    assert sc == {"ok": True, "windows": [{"window_id": "12345"}]}


def test_extract_structured_dict_shape():
    res = {"content": [{"type": "text", "text": "hi"}],
           "structuredContent": {"windows": []}}
    assert extract_structured(res) == {"windows": []}


def test_extract_structured_absent_returns_none():
    assert extract_structured(_Result([_Block("text", "hi")])) is None


def test_format_result_surfaces_window_list():
    # The list_windows case: text says the count, structuredContent has the
    # actual handles. The model must SEE the handles to act.
    res = _StructResult(
        [_Block("text", "2 window(s) found.")],
        structured_content={"ok": True, "windows": [
            {"window_id": "65814", "title": "WeChat", "process": "Weixin.exe"},
            {"window_id": "131072", "title": "Code", "process": "Code.exe"},
        ]},
    )
    out = format_result(res)
    assert "2 window(s) found." in out      # text layer kept
    assert "65814" in out                   # the real handle reaches the model
    assert "Weixin.exe" in out


def test_format_result_strips_ok_and_error_keys():
    res = _StructResult([_Block("text", "done")],
                        structured_content={"ok": True, "error": None})
    # Only bookkeeping keys -> nothing useful to add, so just the text.
    assert format_result(res) == "done"


def test_format_result_no_structured_is_just_text():
    assert format_result(_Result([_Block("text", "plain")])) == "plain"


def test_format_result_truncates_huge_payload():
    big = {"blob": "x" * 20000}
    res = _StructResult([_Block("text", "ok")], structured_content=big)
    out = format_result(res)
    assert "(truncated)" in out
    assert len(out) < 9000                  # capped, not the full 20k


# --- config parsing -------------------------------------------------------


def test_parse_mcp_servers():
    cfg = {
        "mcp_servers": {
            "fs": {"command": "npx", "args": ["-y", "server-fs"], "env": {"K": "V"}},
            "bad": {"args": ["x"]},  # missing command -> skipped
        }
    }
    servers = parse_mcp_servers(cfg)
    assert len(servers) == 1
    s = servers[0]
    assert s.name == "fs" and s.command == "npx"
    assert s.args == ["-y", "server-fs"] and s.env == {"K": "V"}


def test_discover_mcp_servers_reads_toml(tmp_path):
    cfg = tmp_path / "config.toml"
    cfg.write_text(textwrap.dedent("""
        [mcp_servers.demo]
        command = "python"
        args = ["-m", "demo_server"]
    """))
    servers = discover_mcp_servers(cfg)
    assert len(servers) == 1
    assert servers[0].name == "demo" and servers[0].command == "python"


def test_discover_missing_file_returns_empty(tmp_path):
    assert discover_mcp_servers(tmp_path / "nope.toml") == []


def test_mcp_config_is_isolated_from_codex():
    # Regression: nanocodex must read its OWN config, not the Codex client's.
    from nanocodex.tools.mcp import NANOCODEX_MCP_CONFIG

    assert NANOCODEX_MCP_CONFIG.name == "mcp.toml"
    assert ".nanocodex" in NANOCODEX_MCP_CONFIG.parts
    # Must NOT point at the Codex client's private config.
    assert ".codex" not in NANOCODEX_MCP_CONFIG.parts


def test_discover_default_path_is_nanocodex(monkeypatch, tmp_path):
    # With no explicit path, discover should read ~/.nanocodex/mcp.toml.
    import nanocodex.tools.mcp as mcp_mod

    fake = tmp_path / "mcp.toml"
    fake.write_text('[mcp_servers.demo]\ncommand = "python"\n')
    monkeypatch.setattr(mcp_mod, "NANOCODEX_MCP_CONFIG", fake)
    servers = mcp_mod.discover_mcp_servers()
    assert len(servers) == 1 and servers[0].name == "demo"


# --- tool wrapping + execution -------------------------------------------


class _FakeDesc:
    def __init__(self, name, description, input_schema):
        self.name = name
        self.description = description
        self.inputSchema = input_schema


class _FakeListResult:
    def __init__(self, tools):
        self.tools = tools


class _FakeSession:
    def __init__(self):
        self.calls = []

    async def list_tools(self):
        return _FakeListResult([
            _FakeDesc("echo", "Echo text back",
                      {"type": "object", "properties": {"text": {"type": "string"}},
                       "required": ["text"]}),
        ])

    async def call_tool(self, name, arguments):
        self.calls.append((name, arguments))
        return _Result([_Block("text", f"echoed: {arguments.get('text')}")])


async def test_build_tools_and_naming(tmp_path):
    session = _FakeSession()
    tools = await build_tools_for_session(_ctx(tmp_path), "demo", session)
    assert len(tools) == 1
    tool = tools[0]
    assert tool.name == "mcp__demo__echo"
    assert "external MCP" in tool.description
    assert tool.parameters["required"] == ["text"]


async def test_mcp_tool_execute_delegates(tmp_path):
    session = _FakeSession()
    tools = await build_tools_for_session(_ctx(tmp_path), "demo", session)
    out = await tools[0].execute(text="hi")
    assert out == "echoed: hi"
    assert session.calls == [("echo", {"text": "hi"})]


async def test_mcp_tool_handles_call_failure(tmp_path):
    async def boom(name, arguments):
        raise RuntimeError("server down")

    tool = McpTool(_ctx(tmp_path), server="s", tool_name="t",
                   description="d", input_schema=None, call_tool=boom)
    out = await tool.execute()
    assert out.startswith("Error calling MCP tool mcp__s__t")
    assert "server down" in out


# --- approval gating (desktop/WeChat actions run OUTSIDE the sandbox) ------


def _gated_ctx(tmp_path, *, answers, policy_name=ON_REQUEST, require_step=True):
    """ToolContext whose approver records prompts and returns scripted answers."""
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)
    asked = {"prompts": []}

    async def cb(req):
        asked["prompts"].append(req)
        return answers.pop(0) if answers else False

    ctx = ToolContext(
        workspace=tmp_path, policy=policy,
        approver=Approver(policy_name, cb),
        executor=make_executor(policy), plan=[],
        require_step_approval=require_step,
    )
    return ctx, asked


def _write_tool(ctx, *, tool_name="click_xy"):
    calls = []

    async def call_tool(name, arguments):
        calls.append((name, arguments))
        return _Result([_Block("text", "ok")])

    tool = McpTool(ctx, server="windows_computer_use", tool_name=tool_name,
                   description="d", input_schema=None, call_tool=call_tool)
    return tool, calls


def test_is_readonly_mcp_tool_classification():
    for name in ("list_windows", "get_ui_tree", "capture_screen", "wait",
                 "screenshot", "read_file", "query_state"):
        assert is_readonly_mcp_tool(name), name
    for name in ("click_xy", "type_text", "press_keys", "focus_window",
                 "send_wechat", "desktop_action", "drag"):
        assert not is_readonly_mcp_tool(name), name


async def test_write_mcp_tool_prompts_when_step_approval_on(tmp_path):
    ctx, asked = _gated_ctx(tmp_path, answers=[True])
    tool, calls = _write_tool(ctx)
    out = await tool.execute(x=10, y=20)
    assert asked["prompts"], "expected an approval prompt for the desktop action"
    assert calls == [("click_xy", {"x": 10, "y": 20})]  # ran after approval
    assert out == "ok"


async def test_write_mcp_tool_denied_blocks_call(tmp_path):
    ctx, asked = _gated_ctx(tmp_path, answers=[False])
    tool, calls = _write_tool(ctx)
    out = await tool.execute(x=10, y=20)
    assert "not approved" in out.lower()
    assert calls == []                       # never reached the remote tool
    assert len(asked["prompts"]) == 1


async def test_readonly_mcp_tool_never_prompts(tmp_path):
    ctx, asked = _gated_ctx(tmp_path, answers=[])   # no answers => would deny
    tool, calls = _write_tool(ctx, tool_name="capture_screen")
    out = await tool.execute()
    assert asked["prompts"] == []            # read-only: no prompt
    assert calls == [("capture_screen", {})]
    assert out == "ok"


async def test_write_mcp_tool_runs_when_callback_approves(tmp_path):
    # GUI "auto-approve ON" is implemented at the callback layer (_approve_via_ui
    # returns True), NOT via require_step_approval. Because a desktop action is
    # escalated (acts outside the sandbox), on-request classifies it as ASK, so
    # the gate still consults the approver — which approves and lets it run.
    ctx, asked = _gated_ctx(tmp_path, answers=[True], require_step=False)
    tool, calls = _write_tool(ctx)
    out = await tool.execute(x=1, y=2)
    assert calls == [("click_xy", {"x": 1, "y": 2})]
    assert out == "ok"


async def test_write_mcp_tool_denied_under_never_policy(tmp_path):
    # Unattended scheduled runs use policy `never`; desktop writes must be denied.
    ctx, asked = _gated_ctx(tmp_path, answers=[], policy_name=NEVER, require_step=False)
    tool, calls = _write_tool(ctx)
    out = await tool.execute(x=1, y=2)
    assert "denied" in out.lower()
    assert calls == []                       # never executed
    assert asked["prompts"] == []            # auto-denied, no human asked
