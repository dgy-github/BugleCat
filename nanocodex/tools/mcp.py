"""MCP (Model Context Protocol) connector.

Connects to configured MCP servers over stdio, discovers their tools, and wraps
each remote tool as a nanocodex Tool named ``mcp__<server>__<tool>`` so the model
can call them like any built-in.

Testability vs. real I/O
------------------------
The wrapping + result extraction (``McpTool``, ``extract_text``,
``build_tools_for_session``) is pure and unit-tested with a fake session. The
actual stdio handshake (``McpManager.connect``) spawns real subprocesses and can
only be verified against a live MCP server; it is annotated as such and kept
separate from the tested core.

Security note
-------------
MCP tools run OUTSIDE nanocodex's sandbox — they are whatever the external
server chooses to expose (and a server may run arbitrary code). Connecting is
therefore opt-in (``--mcp``) and the CLI prints a clear warning. Do not enable it
for servers you don't trust.
"""

from __future__ import annotations

import json
import tomllib
from contextlib import AsyncExitStack
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Awaitable, Callable

from nanocodex.sandbox.approval import ApprovalRequest, Decision, step_decision
from nanocodex.tools.base import Tool, ToolContext

# nanocodex's OWN, isolated MCP config — deliberately NOT ~/.codex/config.toml
# (that belongs to the Codex client; nanocodex must not read another tool's
# private config). Define your MCP servers here under [mcp_servers.<name>].
NANOCODEX_MCP_CONFIG = Path.home() / ".nanocodex" / "mcp.toml"

# async (tool_name, arguments) -> CallToolResult-like
CallTool = Callable[[str, dict[str, Any]], Awaitable[Any]]


@dataclass
class McpServerConfig:
    name: str
    command: str
    args: list[str] = field(default_factory=list)
    env: dict[str, str] = field(default_factory=dict)
    # When False, the server stays in mcp.toml but is NOT connected at startup.
    # Lets the GUI's plugin manager disable a server without deleting its config.
    # Absent in TOML => True, so existing configs keep connecting unchanged.
    enabled: bool = True


def extract_text(result: Any) -> str:
    """Pull text out of an MCP CallToolResult; note non-text/error blocks."""
    content = getattr(result, "content", None)
    if content is None and isinstance(result, dict):
        content = result.get("content")

    parts: list[str] = []
    for block in content or []:
        btype = getattr(block, "type", None)
        if btype is None and isinstance(block, dict):
            btype = block.get("type")
        if btype == "text":
            text = getattr(block, "text", None)
            if text is None and isinstance(block, dict):
                text = block.get("text", "")
            if text:
                parts.append(text)
        else:
            parts.append(f"[{btype or 'non-text'} content omitted]")

    out = "\n".join(parts)
    is_error = getattr(result, "isError", False)
    if not is_error and isinstance(result, dict):
        is_error = result.get("isError", False)
    if is_error:
        return f"Error from MCP tool: {out}" if out else "Error from MCP tool (no detail)."
    return out or "(no content)"


def extract_structured(result: Any) -> dict[str, Any] | None:
    """Pull an MCP tool's ``structuredContent`` (the machine-readable second
    layer), tolerating both attribute- and dict-shaped results.

    Many servers return a short human ``text`` block PLUS a richer
    ``structuredContent`` payload (e.g. the windows executor returns
    ``"18 window(s) found."`` as text but the actual window list — titles,
    numeric handles, processes — only in structuredContent). If we drop the
    structured layer, the model never sees the data it needs to act (it can't
    learn a real window_id), so callers must surface it.
    """
    sc = getattr(result, "structuredContent", None)
    if sc is None and isinstance(result, dict):
        sc = result.get("structuredContent")
    return sc if isinstance(sc, dict) else None


def format_result(result: Any) -> str:
    """Render an MCP result for the model: the text layer, plus a compact JSON
    dump of any structuredContent so structured data (window lists, handles,
    geometry) actually reaches the model instead of being thrown away.

    The ``ok``/``error`` bookkeeping keys the server wraps around the payload
    are stripped — they duplicate the text layer and add noise.
    """
    text = extract_text(result)
    structured = extract_structured(result)
    if not structured:
        return text
    payload = {k: v for k, v in structured.items() if k not in ("ok", "error")}
    if not payload:
        return text
    try:
        dumped = json.dumps(payload, ensure_ascii=False, default=str)
    except (TypeError, ValueError):
        return text
    # Cap pathological payloads so one huge UI tree can't blow the context.
    if len(dumped) > 8000:
        dumped = dumped[:8000] + "…(truncated)"
    return f"{text}\n{dumped}"


# MCP tool-name prefixes that denote READ-ONLY / non-mutating actions. These
# never prompt (otherwise every screenshot or UI-tree poll during desktop
# automation would interrupt the user). Anything NOT matching is treated as a
# state-changing action and gated — the safe default for unknown tools.
_MCP_READONLY_PREFIXES = (
    "list", "get", "read", "capture", "screenshot", "screen", "search",
    "query", "find", "view", "describe", "inspect", "fetch", "status",
    "snapshot", "wait",
)


def is_readonly_mcp_tool(tool_name: str) -> bool:
    """True if the remote tool name looks read-only (non-mutating)."""
    name = tool_name.lower()
    return any(name.startswith(p) for p in _MCP_READONLY_PREFIXES)


class McpTool(Tool):
    """Wraps one remote MCP tool, delegating execution to its session."""

    def __init__(
        self,
        ctx: ToolContext,
        *,
        server: str,
        tool_name: str,
        description: str,
        input_schema: dict[str, Any] | None,
        call_tool: CallTool,
    ) -> None:
        super().__init__(ctx)
        self._server = server
        self._tool_name = tool_name
        self._description = description or f"MCP tool '{tool_name}' from server '{server}'."
        self._input_schema = input_schema
        self._call_tool = call_tool

    @property
    def name(self) -> str:
        return f"mcp__{self._server}__{self._tool_name}"

    @property
    def description(self) -> str:
        return f"[external MCP] {self._description}"

    @property
    def parameters(self) -> dict[str, Any]:
        schema = self._input_schema
        if isinstance(schema, dict) and schema.get("type") == "object":
            return schema
        return {"type": "object", "properties": {}}

    async def execute(self, **kwargs: Any) -> str:
        gate = await self._gate_decision()
        if gate is not None:
            return gate
        try:
            result = await self._call_tool(self._tool_name, kwargs)
        except Exception as exc:  # noqa: BLE001 - surfaced to the model
            return f"Error calling MCP tool {self.name}: {type(exc).__name__}: {exc}"
        # Surface BOTH layers (text + structuredContent). Servers like the
        # desktop executor return the real payload (window list, numeric
        # handles, geometry) only in structuredContent; extract_text alone would
        # drop it and the model could never learn a real window_id.
        return format_result(result)

    async def _gate_decision(self) -> str | None:
        """Approval gate for MCP tools, mirroring ShellTool.

        MCP tools run OUTSIDE the sandbox (desktop/WeChat automation, etc.), so a
        write-class tool is treated as an escalated action: under the per-step
        confirmation mode (GUI auto-approve OFF) it must PROMPT, and under the
        ``never`` policy (unattended scheduled runs) it is auto-denied. Read-only
        tools (list/get/capture/wait …) never prompt — otherwise every screenshot
        during desktop automation would interrupt the user. Returns an error
        string to abort, or ``None`` to proceed.
        """
        if is_readonly_mcp_tool(self._tool_name):
            return None
        approver = getattr(self.ctx, "approver", None)
        if approver is None:
            return None
        # Acts outside the sandbox -> escalated. classify() turns that into
        # AUTO_DENY under `never`, ASK under on-request/untrusted.
        decision = approver.classify(self.name, needs_escalation=True)
        decision = step_decision(
            decision, is_write=True,
            require_step_approval=getattr(self.ctx, "require_step_approval", False),
        )
        if decision is Decision.AUTO_DENY:
            return (
                f"Error: MCP tool {self.name} denied by approval policy "
                f"'{approver.policy}' (it acts outside the sandbox). Ask the user "
                "to change the policy if this action is intended."
            )
        if decision is Decision.ASK:
            approved = await approver.request(
                ApprovalRequest(
                    command=self.name,
                    reason="External MCP tool (acts outside the sandbox); per-step confirmation is on.",
                    cwd=str(getattr(self.ctx, "workspace", "")),
                    escalated=True,
                )
            )
            if not approved:
                return f"Error: MCP tool {self.name} not approved by the user."
        return None


async def build_tools_for_session(
    ctx: ToolContext, server_name: str, session: Any
) -> list[McpTool]:
    """List an initialized session's tools and wrap each as an McpTool.

    *session* must expose ``async list_tools()`` (returning an object with a
    ``.tools`` list of descriptors with ``.name`` / ``.description`` /
    ``.inputSchema``) and ``async call_tool(name, arguments)``.
    """
    listed = await session.list_tools()
    descriptors = getattr(listed, "tools", None)
    if descriptors is None and isinstance(listed, dict):
        descriptors = listed.get("tools", [])
    tools: list[McpTool] = []
    for desc in descriptors or []:
        name = getattr(desc, "name", None) or (desc.get("name") if isinstance(desc, dict) else None)
        if not name:
            continue
        description = getattr(desc, "description", None)
        if description is None and isinstance(desc, dict):
            description = desc.get("description", "")
        schema = getattr(desc, "inputSchema", None)
        if schema is None and isinstance(desc, dict):
            schema = desc.get("inputSchema")
        tools.append(
            McpTool(
                ctx,
                server=server_name,
                tool_name=name,
                description=description or "",
                input_schema=schema,
                call_tool=session.call_tool,
            )
        )
    return tools


def parse_mcp_servers(config: dict[str, Any]) -> list[McpServerConfig]:
    """Parse ``[mcp_servers.<name>]`` tables from a loaded TOML dict."""
    servers: list[McpServerConfig] = []
    raw = config.get("mcp_servers")
    if not isinstance(raw, dict):
        return servers
    for name, spec in raw.items():
        if not isinstance(spec, dict):
            continue
        command = spec.get("command")
        if not command:
            continue
        args = spec.get("args") or []
        env = spec.get("env") or {}
        # `enabled` is optional and defaults to True so existing configs (which
        # have no such key) keep connecting exactly as before.
        enabled = spec.get("enabled", True)
        servers.append(
            McpServerConfig(
                name=name,
                command=str(command),
                args=[str(a) for a in args] if isinstance(args, list) else [],
                env={str(k): str(v) for k, v in env.items()} if isinstance(env, dict) else {},
                enabled=bool(enabled),
            )
        )
    return servers


def discover_mcp_servers(config_path: Path | None = None) -> list[McpServerConfig]:
    """Read MCP server definitions from nanocodex's own ~/.nanocodex/mcp.toml
    (best-effort). Deliberately isolated from ~/.codex/config.toml."""
    path = NANOCODEX_MCP_CONFIG if config_path is None else config_path
    if not path.is_file():
        return []
    try:
        with path.open("rb") as fh:
            data = tomllib.load(fh)
    except (OSError, tomllib.TOMLDecodeError):
        return []
    return parse_mcp_servers(data)


class McpManager:
    """Owns the lifecycle of one or more live MCP stdio connections.

    Real subprocess I/O — only verifiable against a live server. Tools whose
    server fails to start are skipped; the error is recorded in ``errors``.
    """

    def __init__(self, ctx: ToolContext, servers: list[McpServerConfig]) -> None:
        self.ctx = ctx
        self.servers = servers
        self.tools: list[McpTool] = []
        self.errors: list[str] = []
        self._stack: AsyncExitStack | None = None
        self._errlog = None  # UTF-8 file the server subprocess's stderr goes to
        # Live (server_name, session) pairs kept so a SECOND set of tools can be
        # built later against a DIFFERENT ToolContext (e.g. the scheduler's
        # desktop-only approver) without reconnecting. The sessions stay bound to
        # the event loop connect() ran on, so callers must bridge execution back
        # onto that loop. See build_tools_with_ctx.
        self._sessions: list[tuple[str, Any]] = []

    def _open_errlog(self):
        """Open a UTF-8 log for server stderr.

        Critical on Windows GUI launches: stdio_client defaults errlog to the
        parent's sys.stderr, which is None/invalid under pythonw (no console)
        and uses cp1252 (crashes on non-latin output). Both manifest as an
        opaque "Connection closed". Redirecting to a real UTF-8 file avoids the
        crash AND captures the real failure reason for debugging.
        """
        try:
            log_dir = Path.home() / ".nanocodex"
            log_dir.mkdir(parents=True, exist_ok=True)
            self._errlog = open(log_dir / "mcp-server.log", "a", encoding="utf-8")
            return self._errlog
        except OSError:
            return None

    async def connect(self) -> list[McpTool]:
        from mcp import ClientSession, StdioServerParameters
        from mcp.client.stdio import stdio_client

        errlog = self._open_errlog()
        self._stack = AsyncExitStack()
        for server in self.servers:
            try:
                params = StdioServerParameters(
                    command=server.command,
                    args=server.args,
                    env=server.env or None,
                )
                client = (stdio_client(params, errlog=errlog) if errlog is not None
                          else stdio_client(params))
                read, write = await self._stack.enter_async_context(client)
                session = await self._stack.enter_async_context(ClientSession(read, write))
                await session.initialize()
                self._sessions.append((server.name, session))
                self.tools.extend(await build_tools_for_session(self.ctx, server.name, session))
            except Exception as exc:  # noqa: BLE001 - one bad server shouldn't kill the rest
                self.errors.append(f"{server.name}: {type(exc).__name__}: {exc}")
        return self.tools

    async def build_tools_with_ctx(self, ctx: ToolContext) -> list[McpTool]:
        """Re-wrap every connected server's tools against a DIFFERENT ctx.

        The tools returned by :meth:`connect` are bound to ``self.ctx`` (the
        GUI's interactive context, whose approver prompts the user). The managed
        scheduler needs the SAME live sessions but a DIFFERENT approver — the
        unattended desktop-only one — so a scheduled desktop action is gated by
        ``_desktop_only_approver`` rather than popping a dialog nobody will
        answer. Each tool's ``call_tool`` is still the live session's (bound to
        the MCP event loop), so the caller must bridge execution onto that loop
        the same way :meth:`connect`'s tools are bridged.
        """
        tools: list[McpTool] = []
        for name, session in self._sessions:
            tools.extend(await build_tools_for_session(ctx, name, session))
        return tools

    async def aclose(self) -> None:
        if self._stack is not None:
            try:
                await self._stack.aclose()
            finally:
                self._stack = None
        if self._errlog is not None:
            try:
                self._errlog.close()
            finally:
                self._errlog = None
