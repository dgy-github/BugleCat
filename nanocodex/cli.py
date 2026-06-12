"""nanocodex CLI: an interactive Codex-style coding REPL on a DeepSeek backend.

Usage:
    nanocodex                      # REPL in the current directory
    nanocodex --sandbox read-only  # override sandbox mode
    nanocodex "fix the bug in ..." # one-shot: run a single task and exit
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path
from typing import Optional

import typer
from typer.core import TyperGroup
from rich.console import Console
from rich.panel import Panel
from rich.prompt import Confirm
from rich.text import Text

# Windows consoles often default to a legacy code page (e.g. cp1252) that can't
# encode characters rich emits. Reconfigure to UTF-8 with replacement so the
# CLI degrades gracefully instead of crashing with UnicodeEncodeError.
for _stream in (sys.stdout, sys.stderr):
    reconfigure = getattr(_stream, "reconfigure", None)
    if callable(reconfigure):
        try:
            reconfigure(encoding="utf-8", errors="replace")
        except (ValueError, OSError):
            pass

from nanocodex.agent import AgentLoop, CompactionConfig, LoopHooks, Session, build_system_prompt
from nanocodex.agent.agents_md import discover_agents
from nanocodex.agent.memory_store import render_for_prompt as render_memory
from nanocodex.agent.skills_store import discover_skills
from nanocodex.agent.images import ImageError, build_user_content
from nanocodex.tools.mcp import McpManager, discover_mcp_servers
from nanocodex.config import VALID_APPROVAL_POLICIES, VALID_SANDBOX_MODES, ConfigError, load_config
from nanocodex.provider import ToolCall
from nanocodex.provider.deepseek import DeepSeekProvider, ProviderError
from nanocodex.sandbox import Approver, ApprovalRequest, SandboxPolicy, make_executor
from nanocodex.tools import ToolContext, ToolRegistry, render_plan

class _SubcommandFirstGroup(TyperGroup):
    """Route a leading subcommand name to the subcommand, not the ``task`` arg.

    The root callback has an optional positional ``task`` (so ``nanocodex "fix
    the bug"`` works as a one-shot). Click's group parser would otherwise greedily
    bind a bare ``storyboard`` / ``schedule`` token to that ``task`` argument and
    then fail to find the real subcommand (``No such command 'run'``). Here we
    peek at the args: if the FIRST non-option token is a registered subcommand,
    we let normal group dispatch handle it (task stays None); only a non-command
    first token falls through to the one-shot task path.
    """

    def parse_args(self, ctx, args):
        # Find the first non-option token. If it names a registered subcommand,
        # temporarily DROP the optional ``task`` positional from this group's
        # params so Click's parser can't swallow the subcommand name into it.
        # (Merely nulling ctx.params["task"] fails: Command.parse_args re-binds
        # the positional and overwrites it.) Restored in finally so help/other
        # parses are unaffected.
        first = next((t for t in args if not t.startswith("-")), None)
        if first is not None and first in self.commands:
            saved = self.params
            self.params = [p for p in saved if getattr(p, "name", None) != "task"]
            try:
                return super().parse_args(ctx, args)
            finally:
                self.params = saved
        return super().parse_args(ctx, args)


app = typer.Typer(
    add_completion=False,
    cls=_SubcommandFirstGroup,
    help="A minimal Codex-style coding agent (DeepSeek backend).",
)
console = Console()


def _build_console_approver(policy_name: str) -> Approver:
    """An Approver whose callback prompts the console with y/n."""

    async def _ask(req: ApprovalRequest) -> bool:
        body = Text()
        body.append("Command: ", style="bold")
        body.append(req.command + "\n")
        body.append("Dir:     ", style="bold")
        body.append(req.cwd + "\n")
        if req.reason:
            body.append("Reason:  ", style="bold")
            body.append(req.reason + "\n")
        if req.escalated:
            body.append("\nThis is an escalated retry (ran sandboxed and failed).", style="yellow")
        console.print(Panel(body, title="Approval required", border_style="yellow"))
        # Confirm.ask is blocking; run it off the event loop.
        return await asyncio.to_thread(Confirm.ask, "Allow this command?", default=False)

    return Approver(policy_name, _ask)


def _make_hooks() -> LoopHooks:
    # Tracks whether we're mid-"thinking" or mid-"answer" so we can insert
    # separators when the stream switches between reasoning and content.
    stream_state = {"mode": None}  # None | "reasoning" | "content"

    def _emit(text: str, *, dim: bool = False) -> None:
        # Text() avoids rich interpreting model output like "[red]" as markup;
        # flush forces token-by-token visibility despite stdout line buffering.
        console.print(Text(text, style="dim" if dim else ""), end="")
        try:
            console.file.flush()
        except (OSError, ValueError):
            pass

    async def on_reasoning_delta(delta: str) -> None:
        if stream_state["mode"] != "reasoning":
            if stream_state["mode"] is not None:
                console.print()
            console.print(Text("thinking…", style="dim italic"))
            stream_state["mode"] = "reasoning"
        _emit(delta, dim=True)

    async def on_content_delta(delta: str) -> None:
        if stream_state["mode"] != "content":
            if stream_state["mode"] is not None:
                console.print()
            stream_state["mode"] = "content"
        _emit(delta)

    async def on_stream_end() -> None:
        if stream_state["mode"] is not None:
            console.print()
        stream_state["mode"] = None

    async def on_assistant_text(text: str) -> None:
        # Non-streaming fallback only (the loop skips this when streaming).
        if text.strip():
            console.print(text)

    async def on_tool_start(tc: ToolCall) -> None:
        detail = _summarize_call(tc)
        console.print(Text(f"  -> {tc.name}", style="cyan") + Text(f"  {detail}", style="dim"))

    async def on_tool_result(name: str, result: str) -> None:
        first = result.strip().splitlines()[0] if result.strip() else "(no output)"
        style = "red" if first.startswith(("Error", "Sandbox denied")) else "green"
        console.print(Text(f"  <- {first[:140]}", style=style))

    return LoopHooks(
        on_assistant_text=on_assistant_text,
        on_tool_start=on_tool_start,
        on_tool_result=on_tool_result,
        on_content_delta=on_content_delta,
        on_reasoning_delta=on_reasoning_delta,
        on_stream_end=on_stream_end,
    )


def _summarize_call(tc: ToolCall) -> str:
    args = tc.arguments
    if tc.name == "shell":
        return str(args.get("command", ""))[:100]
    if tc.name == "apply_patch":
        patch = str(args.get("patch", ""))
        files = [ln.split(": ", 1)[-1] for ln in patch.splitlines() if ln.startswith("*** ") and "File:" in ln]
        return ", ".join(files)[:100]
    if tc.name == "read_file":
        return str(args.get("path", ""))
    if tc.name == "update_plan":
        return f"{len(args.get('plan', []))} steps"
    return ""


# Sentinel for _build_loop's log_path: distinguishes "not passed" (use the
# default session.jsonl) from an explicit None (ephemeral, no persistence).
_UNSET = object()


def _build_loop(
    overrides: dict,
    workspace: Path,
    *,
    resume: bool = False,
    approver_factory=_build_console_approver,
    log_path: "Path | None" = _UNSET,
    seed_messages: "list[dict] | None" = None,
) -> AgentLoop:
    """Build an AgentLoop.

    *log_path* controls where the session transcript is persisted:
    - ``_UNSET`` (default): the usual ``workspace/.nanocodex/session.jsonl``.
    - ``None``: do NOT persist (a fresh, ephemeral session). Used by the GUI's
      managed scheduler so unattended task runs never mix into — or pollute the
      session directory of — the user's interactive conversation.
    """
    cfg = load_config(workspace=workspace, overrides=overrides)
    cfg.validate()

    policy = SandboxPolicy.from_config(cfg)
    approver = approver_factory(cfg.approval_policy)
    executor = make_executor(policy)

    plan: list[dict[str, str]] = []
    tool_ctx = ToolContext(
        workspace=cfg.workspace,
        policy=policy,
        approver=approver,
        executor=executor,
        timeout_s=cfg.timeout_s,
        plan=plan,
    )
    tools = ToolRegistry(tool_ctx)
    provider = DeepSeekProvider(
        api_key=cfg.api_key, base_url=cfg.base_url, model=cfg.model, timeout_s=cfg.timeout_s
    )
    # Optional vision backend: when a VL model is configured, image-bearing turns
    # route to it (e.g. DashScope Qwen-VL) while text/coding stays on the main
    # model. base_url/api_key fall back to the main ones when only vl_model is set
    # (same vendor exposing a VL model on the same endpoint). All OpenAI-compatible.
    vision_provider = None
    if cfg.vl_model:
        vision_provider = DeepSeekProvider(
            api_key=cfg.vl_api_key or cfg.api_key,
            base_url=cfg.vl_base_url or cfg.base_url,
            model=cfg.vl_model,
            timeout_s=cfg.timeout_s,
        )
    agents = discover_agents(cfg.workspace)
    skills = discover_skills()
    memory = render_memory()
    system_prompt = build_system_prompt(policy, cfg.approval_policy, agents, skills, memory)
    # _UNSET -> default file; explicit None -> ephemeral (no persistence).
    if log_path is _UNSET:
        log_path = cfg.workspace / ".nanocodex" / "session.jsonl"
    if seed_messages is not None:
        # Fork: start a NEW conversation seeded from a prior snapshot's messages
        # (the GUI's "continue this conversation"). The original is untouched;
        # the system prompt is taken fresh (sandbox/AGENTS.md/skills may differ).
        session = Session.fork(system_prompt, seed_messages, log_path=log_path)
    elif resume:
        session = Session.resume(system_prompt, log_path=log_path)
    else:
        session = Session(system_prompt, log_path=log_path)

    reasoning = cfg.reasoning_effort if cfg.reasoning_effort != "auto" else None
    loop = AgentLoop(
        provider, tools, session,
        max_iterations=cfg.max_iterations,
        reasoning_effort=reasoning,
        compaction=CompactionConfig(token_budget=cfg.context_token_budget),
        vision_provider=vision_provider,
    )
    # Stash for the banner.
    loop._cfg = cfg  # type: ignore[attr-defined]
    loop._plan = plan  # type: ignore[attr-defined]
    return loop


def _print_banner(loop: AgentLoop) -> None:
    cfg = loop._cfg  # type: ignore[attr-defined]
    info = cfg.redacted()
    body = Text()
    body.append("model:    ", style="bold"); body.append(f"{info['model']}\n")
    body.append("endpoint: ", style="bold"); body.append(f"{info['base_url']}\n")
    body.append("sandbox:  ", style="bold"); body.append(f"{info['sandbox_mode']}\n")
    body.append("approval: ", style="bold"); body.append(f"{info['approval_policy']}\n")
    body.append("workspace:", style="bold"); body.append(f" {info['workspace']}")
    console.print(Panel(body, title="nanocodex", border_style="cyan"))


@app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    task: Optional[str] = typer.Argument(None, help="Run a single task and exit. Omit for an interactive REPL."),
    sandbox: Optional[str] = typer.Option(None, "--sandbox", "-s", help=f"Sandbox mode: {', '.join(VALID_SANDBOX_MODES)}."),
    approval: Optional[str] = typer.Option(None, "--approval", "-a", help=f"Approval policy: {', '.join(VALID_APPROVAL_POLICIES)}."),
    model: Optional[str] = typer.Option(None, "--model", "-m", help="Override the model name."),
    workdir: Optional[Path] = typer.Option(None, "--cd", help="Workspace directory (default: current)."),
    resume: bool = typer.Option(False, "--resume", "-r", help="Resume the previous session from this workspace's history."),
    context_budget: Optional[int] = typer.Option(None, "--context-budget", help="Approx token budget that triggers context compaction (0 = off)."),
    max_iterations: Optional[int] = typer.Option(None, "--max-iterations", help="Max tool-loop steps per turn before stopping (default 60)."),
    mcp: bool = typer.Option(False, "--mcp", help="Connect MCP servers from ~/.nanocodex/mcp.toml and expose their tools (runs OUTSIDE the sandbox)."),
    image: Optional[list[str]] = typer.Option(None, "--image", "-i", help="Attach an image to the task (repeatable). Requires a vision-capable model."),
    gui: bool = typer.Option(False, "--gui", "-g", help="Launch the desktop (Tkinter) window instead of the terminal REPL."),
    sandbox_tmp: bool = typer.Option(False, "--sandbox-tmp", help="Run in a fresh throwaway temp directory; your real project is never touched. Removed on exit."),
) -> None:
    # When a subcommand (e.g. `schedule`) was invoked, the callback must not
    # also start the REPL — let the subcommand handle it.
    if ctx.invoked_subcommand is not None:
        return
    overrides = {
        "sandbox_mode": sandbox,
        "approval_policy": approval,
        "model": model,
        "context_token_budget": context_budget,
        "max_iterations": max_iterations,
    }
    # One-shot isolated workspace: a fresh temp dir the agent is confined to.
    _tmp_workspace: Optional[Path] = None
    if sandbox_tmp:
        import tempfile
        _tmp_workspace = Path(tempfile.mkdtemp(prefix="nanocodex-sbx-")).resolve()
        workspace = _tmp_workspace
        console.print(f"[yellow]Sandbox: working in throwaway dir {workspace} (removed on exit).[/yellow]")
    else:
        workspace = (workdir or Path.cwd()).resolve()

    if gui:
        # The desktop window builds its own loop (with a GUI approver) and runs
        # its own mainloop; hand off and return.
        from nanocodex.gui import launch
        launch(overrides, workspace, resume=resume)
        return

    try:
        loop = _build_loop(overrides, workspace, resume=resume)
    except ConfigError as exc:
        console.print(f"[red]Config error:[/red] {exc}")
        raise typer.Exit(code=1)

    _print_banner(loop)
    restored = getattr(loop.session, "restored_count", 0)
    if resume:
        if restored:
            console.print(f"[dim]Resumed {restored} message(s) from previous session.[/dim]")
        else:
            console.print("[dim]No previous session found; starting fresh.[/dim]")

    if image:
        if not task:
            console.print("[red]--image requires a task argument (image input is one-shot).[/red]")
            raise typer.Exit(code=1)
        # Validate images up front so a bad path fails fast with a clear message.
        try:
            for p in image:
                from nanocodex.agent.images import encode_image_block
                encode_image_block(p)
        except ImageError as exc:
            console.print(f"[red]Image error:[/red] {exc}")
            raise typer.Exit(code=1)
        console.print(
            f"[yellow]Attaching {len(image)} image(s). Note: the model must be "
            f"vision-capable to see them; '{loop._cfg.model}' may be text-only and "  # type: ignore[attr-defined]
            "could ignore or reject image content.[/yellow]"
        )

    try:
        asyncio.run(_orchestrate(loop, task, use_mcp=mcp, images=image))
    finally:
        # Throwaway sandbox: remove the temp workspace on exit (any exit path).
        if _tmp_workspace is not None:
            import shutil
            shutil.rmtree(_tmp_workspace, ignore_errors=True)
            console.print(f"[dim]Removed sandbox dir {_tmp_workspace}.[/dim]")


async def _orchestrate(
    loop: AgentLoop,
    task: Optional[str],
    *,
    use_mcp: bool,
    images: Optional[list[str]] = None,
) -> None:
    """Connect MCP (if requested), run the task/REPL, then tear MCP down."""
    manager = None
    if use_mcp:
        manager = await _connect_mcp(loop)
    try:
        if task:
            content = build_user_content(task, images)
            await _run_once(loop, content)
        else:
            await _repl(loop)
    finally:
        if manager is not None:
            await manager.aclose()


async def _connect_mcp(loop: AgentLoop) -> Optional[McpManager]:
    """Discover + connect MCP servers, registering their tools onto the loop."""
    servers = [s for s in discover_mcp_servers() if s.enabled]
    if not servers:
        console.print("[dim]--mcp set but no enabled [mcp_servers] found in ~/.nanocodex/mcp.toml.[/dim]")
        return None
    console.print(
        "[yellow]Connecting MCP servers; their tools run OUTSIDE the sandbox. "
        "Only enable servers you trust.[/yellow]"
    )
    manager = McpManager(loop.tools.ctx, servers)
    tools = await manager.connect()
    for tool in tools:
        loop.tools.register(tool)
    for err in manager.errors:
        console.print(f"[red]MCP server failed:[/red] {err}")
    if tools:
        console.print(f"[dim]Registered {len(tools)} MCP tool(s): {', '.join(t.name for t in tools)}[/dim]")
    return manager


async def _run_once(loop: AgentLoop, task: "str | list") -> None:
    hooks = _make_hooks()
    try:
        result = await loop.run_turn(task, hooks)
    except ProviderError as exc:
        console.print(f"[red]Provider error:[/red] {exc}")
        return
    _print_plan(loop)
    console.print(Panel(result.final_text or "(done)", border_style="green", title=f"done ({result.stop_reason})"))


async def _repl(loop: AgentLoop) -> None:
    console.print("[dim]Type your task. Commands: /plan, /exit.[/dim]")
    hooks = _make_hooks()
    while True:
        try:
            user_input = await asyncio.to_thread(console.input, "[bold cyan]you ›[/bold cyan] ")
        except (EOFError, KeyboardInterrupt):
            console.print("\n[dim]bye[/dim]")
            return
        text = user_input.strip()
        if not text:
            continue
        if text in ("/exit", "/quit"):
            console.print("[dim]bye[/dim]")
            return
        if text == "/plan":
            _print_plan(loop)
            continue
        try:
            result = await loop.run_turn(text, hooks)
        except ProviderError as exc:
            console.print(f"[red]Provider error:[/red] {exc}")
            continue
        except KeyboardInterrupt:
            console.print("\n[yellow]interrupted[/yellow]")
            continue
        _print_plan(loop)
        if result.stop_reason != "completed":
            console.print(f"[yellow]({result.stop_reason})[/yellow]")


def _print_plan(loop: AgentLoop) -> None:
    plan = getattr(loop, "_plan", None)
    if plan:
        console.print(Panel(render_plan(plan), title="plan", border_style="blue"))


# --- scheduled tasks ------------------------------------------------------

schedule_app = typer.Typer(add_completion=False, help="Manage scheduled tasks (run a saved prompt at a future time).")
app.add_typer(schedule_app, name="schedule")


def _schedule_store():
    from nanocodex.agent.schedule import ScheduleStore
    return ScheduleStore()


@schedule_app.command("add")
def schedule_add(
    prompt: str = typer.Argument(..., help="The task prompt to run on schedule."),
    kind: str = typer.Option("once", "--kind", "-k", help="once | interval | daily."),
    at: Optional[str] = typer.Option(None, "--at", help="ISO time for 'once' (e.g. 2026-06-01T09:00:00)."),
    every: Optional[int] = typer.Option(None, "--every", help="Seconds between runs for 'interval'."),
    daily_at: Optional[str] = typer.Option(None, "--daily-at", help="HH:MM local time for 'daily'."),
    allow_desktop: bool = typer.Option(
        False, "--allow-desktop",
        help="DANGEROUS: let this task drive DESKTOP (MCP) actions unattended "
             "(click/type into real apps, e.g. send WeChat). Default off. Does "
             "NOT widen shell or out-of-sandbox file access.",
    ),
) -> None:
    """Add a scheduled task."""
    store = _schedule_store()
    hour, minute = 9, 0
    if daily_at:
        try:
            hh, mm = daily_at.split(":")
            hour, minute = int(hh), int(mm)
        except ValueError:
            console.print("[red]--daily-at must look like HH:MM[/red]")
            raise typer.Exit(code=1)
    try:
        task = store.add(
            prompt, kind=kind, run_at=at or "",
            every_seconds=every or 0, at_hour=hour, at_minute=minute,
            allow_desktop=allow_desktop,
        )
    except ValueError as exc:
        console.print(f"[red]{exc}[/red]")
        raise typer.Exit(code=1)
    console.print(f"[green]Added task {task.id}[/green] — next run: {task.next_run or '(immediate)'}")
    if allow_desktop:
        console.print(
            "[yellow][SECURITY] allow_desktop=ON — when this task fires it can "
            "drive the desktop with nobody watching. Only do this for a prompt "
            "you fully trust.[/yellow]"
        )


@schedule_app.command("list")
def schedule_list() -> None:
    """List all scheduled tasks."""
    store = _schedule_store()
    if not store.tasks:
        console.print("[dim]No scheduled tasks.[/dim]")
        return
    for t in store.tasks:
        state = "on" if t.enabled else "off"
        body = Text()
        body.append(f"{t.id}", style="bold")
        body.append(f"  [{state}] {t.kind}  next={t.next_run or '-'}  runs={t.runs}\n")
        body.append(f"  {t.prompt[:100]}", style="dim")
        console.print(body)


@schedule_app.command("remove")
def schedule_remove(task_id: str = typer.Argument(..., help="Task id to remove.")) -> None:
    """Remove a scheduled task."""
    store = _schedule_store()
    console.print("[green]removed[/green]" if store.remove(task_id) else f"[red]no task {task_id}[/red]")


@schedule_app.command("enable")
def schedule_enable(task_id: str = typer.Argument(...)) -> None:
    """Enable a scheduled task."""
    store = _schedule_store()
    console.print("[green]enabled[/green]" if store.set_enabled(task_id, True) else f"[red]no task {task_id}[/red]")


@schedule_app.command("disable")
def schedule_disable(task_id: str = typer.Argument(...)) -> None:
    """Disable a scheduled task."""
    store = _schedule_store()
    console.print("[green]disabled[/green]" if store.set_enabled(task_id, False) else f"[red]no task {task_id}[/red]")


@schedule_app.command("run")
def schedule_run(
    workdir: Optional[Path] = typer.Option(None, "--cd", help="Workspace directory (default: current)."),
    sandbox: Optional[str] = typer.Option(None, "--sandbox", "-s"),
    approval: Optional[str] = typer.Option(None, "--approval", "-a"),
    model: Optional[str] = typer.Option(None, "--model", "-m"),
    poll: float = typer.Option(30.0, "--poll", help="Seconds between checks for newly-due tasks."),
) -> None:
    """Run the scheduler: wait for tasks to come due and execute them.

    This is a long-running foreground process (Ctrl+C to stop). Each due task
    runs as one agent turn in the given workspace, through the normal sandbox
    and approval layers (approval defaults to 'never' for unattended runs unless
    you override it).
    """
    from nanocodex.agent.schedule_runner import run_forever

    workspace = (workdir or Path.cwd()).resolve()
    overrides = {
        "sandbox_mode": sandbox,
        "approval_policy": approval or "never",  # unattended: don't block on prompts
        "model": model,
    }
    store = _schedule_store()
    if not store.tasks:
        console.print("[yellow]No scheduled tasks. Add one with 'nanocodex schedule add'.[/yellow]")

    async def run_task(task) -> None:
        # A fresh loop per task keeps schedule runs isolated (no shared history).
        # Approver choice is per-task: a task explicitly marked allow_desktop may
        # drive DESKTOP (MCP) actions unattended; every other task keeps the safe
        # auto-deny behavior. Neither widens shell / out-of-sandbox file access.
        if getattr(task, "allow_desktop", False):
            factory = lambda _p: _desktop_only_approver()
        else:
            factory = lambda _p: _auto_deny_approver()
        loop = _build_loop(dict(overrides), workspace, resume=False,
                           approver_factory=factory)
        await _run_once(loop, task.prompt)

    console.print(Panel(
        Text(f"Scheduler running in {workspace}\nApproval: {overrides['approval_policy']}  "
             f"Poll: {poll}s\nCtrl+C to stop.", style=""),
        title="nanocodex schedule", border_style="cyan",
    ))

    def _on_event(msg: str) -> None:
        console.print(f"[dim]{datetime_now()} {msg}[/dim]")

    try:
        asyncio.run(run_forever(store, run_task, poll_interval=poll, on_event=_on_event))
    except KeyboardInterrupt:
        console.print("\n[dim]scheduler stopped[/dim]")


def datetime_now() -> str:
    from datetime import datetime
    return datetime.now().strftime("%H:%M:%S")


def _auto_deny_approver() -> Approver:
    """Approver for unattended runs: never grants escalation (no human present)."""
    async def _deny(_req: ApprovalRequest) -> bool:
        return False
    return Approver("never", _deny)


def _desktop_only_approver() -> Approver:
    """Unattended approver that grants ONLY desktop (MCP) actions, nothing else.

    Security model for allow_desktop tasks. The trick is the policy choice:

    * Under ``never`` the MCP gate auto-denies an escalated action BEFORE it ever
      consults the callback — so a never-approver can't selectively allow MCP.
    * Under ``on-request`` an escalated action becomes ASK and the gate calls our
      callback, where we can say yes to MCP and no to everything else.

    So we run ``on-request`` and gate in the callback: approve a request only
    when its command is an ``mcp__*`` tool name (the desktop executor). A shell
    command's request carries the shell string (never starts with ``mcp__``) and
    is denied — exactly matching the ``never`` behavior for shell / out-of-sandbox
    file writes. In-sandbox actions need no escalation and auto-approve under both
    policies, so this does NOT widen anything beyond desktop MCP calls.
    """
    async def _allow_mcp_only(req: ApprovalRequest) -> bool:
        return str(getattr(req, "command", "")).startswith("mcp__")
    return Approver("on-request", _allow_mcp_only)


def _auto_approve_approver() -> Approver:
    """Approver that grants every escalation without asking.

    Used for A/B comparison runs: each side runs inside its OWN throwaway git
    worktree, so file writes are already isolated from the real workspace and
    from each other. Auto-approving avoids a per-step prompt storm during the
    comparison. NOTE this does not disable SandboxPolicy — the run's writable
    root is still the worktree; this only skips the human prompt on escalation.
    Never wire this into the interactive conversation or an unattended scheduled
    task; it is scoped to the user-initiated, worktree-isolated A/B flow.
    """
    async def _allow_all(_req: ApprovalRequest) -> bool:
        return True
    return Approver("on-request", _allow_all)


# --- storyboard sub-command -------------------------------------------------

storyboard_app = typer.Typer(
    add_completion=False,
    help="Turn a story + reference images into a storyboard (and optionally video).",
)
app.add_typer(storyboard_app, name="storyboard")


@storyboard_app.command("run")
def storyboard_run(
    story: Path = typer.Option(..., "--story", help="Path to a story text file."),
    images: Optional[Path] = typer.Option(
        None, "--images", help="Directory of reference images (png/jpg/...)."),
    out: Path = typer.Option(Path("storyboard_out"), "--out", help="Output directory for the JSON files."),
    aspect_ratio: str = typer.Option("16:9", "--aspect-ratio", help="Video aspect ratio, e.g. 16:9 or 9:16."),
    render: bool = typer.Option(
        False, "--render",
        help="DANGEROUS/$$$: actually generate video via Seedance (needs ARK_API_KEY). "
             "Default off — without it the command only plans + exports JSON.",
    ),
    workdir: Optional[Path] = typer.Option(None, "--cd", help="Workspace directory (default: current)."),
) -> None:
    """Plan a storyboard from a story file + image directory, exporting JSON (and optionally video)."""
    import uuid

    from nanocodex.provider.deepseek import DeepSeekProvider
    from nanocodex.storyboard.clients import SeedanceClient, SeedanceError, TextPlanner, VisionAnalyzer
    from nanocodex.storyboard.models import StoryboardError
    from nanocodex.storyboard.pipeline import PipelineDeps, run_pipeline

    workspace = (workdir or Path.cwd()).resolve()

    if not story.is_file():
        console.print(f"[red]Story file not found: {story}[/red]")
        raise typer.Exit(code=1)
    story_text = story.read_text(encoding="utf-8", errors="replace").strip()
    if not story_text:
        console.print("[red]Story file is empty.[/red]")
        raise typer.Exit(code=1)

    # Collect images from the directory (by extension), if given.
    image_exts = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"}
    image_paths: list[Path] = []
    if images is not None:
        if not images.is_dir():
            console.print(f"[red]Images path is not a directory: {images}[/red]")
            raise typer.Exit(code=1)
        image_paths = sorted(
            p for p in images.iterdir()
            if p.is_file() and p.suffix.lower() in image_exts
        )

    cfg = load_config(workspace=workspace)
    if not cfg.api_key:
        console.print("[red]No API key configured for the text planner (set DEEPSEEK_API_KEY).[/red]")
        raise typer.Exit(code=1)

    planner = TextPlanner(DeepSeekProvider(
        api_key=cfg.api_key, base_url=cfg.base_url, model=cfg.model, timeout_s=cfg.timeout_s,
    ))

    vision = None
    if image_paths and cfg.vl_model:
        vision = VisionAnalyzer(DeepSeekProvider(
            api_key=cfg.vl_api_key or cfg.api_key,
            base_url=cfg.vl_base_url or cfg.base_url,
            model=cfg.vl_model,
            timeout_s=cfg.timeout_s,
        ))
    elif image_paths and not cfg.vl_model:
        console.print("[yellow]No VL backend configured (vl_model empty) — skipping image analysis.[/yellow]")

    seedance = None
    if render:
        # ARK key: config (GUI Settings / ~/.nanocodex/config.toml) or env.
        ark_key = cfg.ark_api_key
        try:
            seedance = SeedanceClient(ark_key)
        except SeedanceError as exc:
            console.print(f"[red]--render set but {exc}[/red]")
            raise typer.Exit(code=1)
        console.print(
            "[yellow][SECURITY/$$$] --render ON — Seedance bills real money per "
            "clip. Generating now.[/yellow]"
        )

    obj = {
        "project": {
            "id": f"sb_{uuid.uuid4().hex[:8]}",
            "title": story_text[:40] or "Untitled storyboard",
            "target_model": "seedance",
            "aspect_ratio": aspect_ratio,
            "language": "zh",
        },
        "inputs": {
            "story_text": story_text,
            "images": [
                {"image_id": f"img_{i:02d}", "path": str(p), "kind": "unknown"}
                for i, p in enumerate(image_paths, 1)
            ],
        },
    }

    def _progress(shot_id: str, i: int, status: str) -> None:
        console.print(f"[dim]{shot_id}: poll {i} — {status}[/dim]")

    deps = PipelineDeps(vision=vision, planner=planner, seedance=seedance)
    try:
        state, written = asyncio.run(run_pipeline(
            obj, deps, out_dir=out.resolve(), render_video=render,
            on_progress=_progress,
        ))
    except StoryboardError as exc:
        console.print(f"[red]{exc}[/red]")
        raise typer.Exit(code=1)

    console.print(
        f"[green]Storyboard built[/green]: {len(state.shots)} shots, "
        f"{len(state.asset_analysis)} images analysed."
    )
    for name, path in written.items():
        console.print(f"  wrote {name}: {path}")
    if render and state.video_urls:
        console.print("[cyan]Video URLs (signed, expire ~24h):[/cyan]")
        for sid, url in state.video_urls.items():
            console.print(f"  {sid}: {url}")


if __name__ == "__main__":
    app()
