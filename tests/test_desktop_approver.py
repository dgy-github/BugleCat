"""Tests for the scheduler's desktop-only approver (Direction B security core).

A scheduled task marked ``allow_desktop=True`` runs unattended under
``_desktop_only_approver`` instead of the default ``_auto_deny_approver``. The
WHOLE security claim rests on one invariant: that approver may grant ONLY
desktop (MCP) actions — never a shell command, never an out-of-sandbox file
write. These tests prove both halves END TO END through the real gates
(McpTool._gate_decision and ShellTool.execute), not just the callback in
isolation, so a future refactor that widens the hole trips a red test.

Offline: the remote MCP tool and the shell executor are faked; no desktop is
touched, no command actually runs.
"""

from __future__ import annotations

from nanocodex.cli import _auto_deny_approver, _desktop_only_approver
from nanocodex.sandbox import SandboxPolicy, make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE
from nanocodex.tools.base import ToolContext
from nanocodex.tools.mcp import McpTool
from nanocodex.tools.shell import ShellTool


class _Block:
    def __init__(self, type, text=None):
        self.type = type
        self.text = text


class _Result:
    def __init__(self, content, is_error=False):
        self.content = content
        self.isError = is_error


def _ctx(tmp_path, approver):
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)
    return ToolContext(
        workspace=tmp_path, policy=policy, approver=approver,
        executor=make_executor(policy), plan=[],
        # Unattended: per-step confirmation is OFF (no human to confirm).
        require_step_approval=False,
    )


def _mcp_write_tool(ctx, *, tool_name="send_wechat_message"):
    calls = []

    async def call_tool(name, arguments):
        calls.append((name, arguments))
        return _Result([_Block("text", "ok")])

    tool = McpTool(ctx, server="windows_computer_use", tool_name=tool_name,
                   description="d", input_schema=None, call_tool=call_tool)
    return tool, calls


# --- the ALLOW half: desktop MCP write runs under the desktop approver ------


async def test_desktop_approver_allows_mcp_write(tmp_path):
    # A scheduled WeChat send (escalated MCP write) must go through unattended.
    ctx = _ctx(tmp_path, _desktop_only_approver())
    tool, calls = _mcp_write_tool(ctx)
    out = await tool.execute(contact="x", message="hi")
    assert calls == [("send_wechat_message", {"contact": "x", "message": "hi"})]
    assert out == "ok"


async def test_desktop_approver_allows_click_and_type(tmp_path):
    for name in ("click_xy", "type_text", "press_keys", "focus_window"):
        ctx = _ctx(tmp_path, _desktop_only_approver())
        tool, calls = _mcp_write_tool(ctx, tool_name=name)
        out = await tool.execute(a=1)
        assert calls == [(name, {"a": 1})], name
        assert out == "ok", name


# --- the DENY half: shell stays denied (NOT widened) -----------------------


async def test_desktop_approver_denies_shell(tmp_path):
    # The shell string never starts with mcp__, so the desktop approver's
    # callback denies it — exactly matching `never` for shell.
    #
    # To exercise the DENY path we must force ESCALATION: a workdir OUTSIDE the
    # workspace makes _needs_escalation True (workspace-write can't write there),
    # so the command is classified ASK and reaches our callback — which refuses
    # it because "echo hi" doesn't start with mcp__. A harmless `echo` is used
    # (it would never run anyway: the gate denies it before the executor).
    outside = tmp_path.parent              # exists, but outside the workspace
    ctx = _ctx(tmp_path, _desktop_only_approver())
    tool = ShellTool(ctx)
    out = await tool.execute(command="echo hi", workdir=str(outside))
    assert "not approved" in out.lower() or "denied" in out.lower()


async def test_desktop_approver_callback_only_says_yes_to_mcp(tmp_path):
    # Direct proof of the callback rule the whole model rests on.
    from nanocodex.sandbox.approval import ApprovalRequest

    approver = _desktop_only_approver()
    assert await approver.request(ApprovalRequest(
        command="mcp__windows_computer_use__send_wechat_message",
        reason="r", cwd="."))
    for non_mcp in ("rm -rf /", "git push", "python evil.py", "send_wechat", ""):
        assert not await approver.request(ApprovalRequest(
            command=non_mcp, reason="r", cwd=".")), non_mcp


# --- parity with the default approver for everything non-desktop ------------


async def test_auto_deny_approver_denies_mcp_write(tmp_path):
    # The DEFAULT (allow_desktop unset) approver must still block desktop writes:
    # this is the safe baseline Direction B opts out of, per task.
    ctx = _ctx(tmp_path, _auto_deny_approver())
    tool, calls = _mcp_write_tool(ctx)
    out = await tool.execute(contact="x", message="hi")
    assert "denied" in out.lower()
    assert calls == []   # never reached the remote tool
