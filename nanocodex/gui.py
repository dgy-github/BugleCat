"""Tkinter desktop GUI for nanocodex (Windows-friendly entry point).

Threading model (the crux)
--------------------------
Tkinter must run on the main thread and is synchronous; the agent loop is
asyncio and long-running. So:

* A daemon thread owns an asyncio event loop and runs each turn there.
* The worker talks to the UI ONLY by pushing events onto a thread-safe
  ``queue.Queue``; the Tk main thread drains it via ``root.after()`` polling.
  (Tk widgets are never touched from the worker thread.)
* Approval is the tricky reverse direction: the approver runs on the worker
  thread but the dialog must appear on the main thread. We post an "approval"
  event to the queue carrying a ``threading.Event`` + result box; the worker
  blocks on that Event until the main thread shows the modal and sets the
  answer. ``asyncio.to_thread`` keeps the event loop unblocked while we wait.

This reuses the real AgentLoop / tools / sandbox unchanged — the GUI only
swaps the console approver and the streaming sinks for Tk equivalents.
"""

from __future__ import annotations

import asyncio
import queue
import threading
from pathlib import Path
from typing import Any, Optional

from nanocodex.agent import LoopHooks
from nanocodex.config import ConfigError
from nanocodex.provider.deepseek import ProviderError
from nanocodex.sandbox import Approver, ApprovalRequest

# Remembers GUI state across launches (last project, scheduler toggle, …).
_STATE_FILE = Path.home() / ".nanocodex" / "gui_state.json"


def _load_state() -> dict:
    """Read the whole gui_state.json dict (best-effort; {} on any problem)."""
    try:
        import json
        data = json.loads(_STATE_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def _save_state(updates: dict) -> None:
    """MERGE *updates* into gui_state.json (best-effort; never raises).

    Critical: read-modify-write so persisting one key (e.g. the scheduler
    toggle) never clobbers another (e.g. last_workspace). The old code wrote the
    whole file from a single key, which would erase the rest.
    """
    try:
        import json
        data = _load_state()
        data.update(updates)
        _STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
        _STATE_FILE.write_text(json.dumps(data), encoding="utf-8")
    except OSError:
        pass


def _load_last_workspace() -> Optional[Path]:
    """Return the last-opened project dir, if it was saved and still exists."""
    raw = _load_state().get("last_workspace")
    if not raw:
        return None
    p = Path(raw)
    return p if p.is_dir() else None


def _save_last_workspace(path: Path) -> None:
    """Persist the active project dir (best-effort; never raises)."""
    _save_state({"last_workspace": str(path)})


def _load_scheduler_enabled() -> bool:
    """Whether the managed scheduler should auto-start (default True).

    The user chose "GUI launches it automatically", so absence of the key means
    ON. Only an explicit stored ``false`` keeps it off across launches.
    """
    val = _load_state().get("scheduler_enabled", True)
    return bool(val)


def _save_scheduler_enabled(enabled: bool) -> None:
    """Persist the scheduler toggle (best-effort; never raises)."""
    _save_state({"scheduler_enabled": bool(enabled)})


# Theme palettes. Both are Codex-style (one accent, low-noise); "light" is the
# default (white canvas, soft-gray panels), "dark" preserves the original look.
# Every widget reads self._palette, so swapping the dict + rebuilding re-colors
# the whole UI. Keys are identical across themes so no call site needs to know
# which theme is active.
_PALETTES: dict[str, dict[str, str]] = {
    "light": {
        "bg":      "#ffffff",   # window / transcript background
        "panel":   "#f5f6f8",   # bars and inputs (soft gray)
        "border":  "#e2e5e9",   # hairline separators
        "fg":      "#1f2328",   # primary text (near-black)
        "muted":   "#6e7781",   # secondary text
        "accent":  "#2f6fed",   # single accent (links, send)
        "accent_fg": "#ffffff",
        "ok":      "#1a7f37",
        "err":     "#cf222e",
        "tool":    "#8250df",
        "reason":  "#8c959f",
        "diff_add_bg":    "#e6ffec",   # added-line row background
        "diff_del_bg":    "#ffebe9",   # removed-line row background
        "cat_messages":   "#2f6fed",   # context bar: Messages segment
        "cat_system":     "#8c959f",   # context bar: System prompt segment
        "cat_free":       "#d8dde3",   # context bar: Free space segment
    },
    "dark": {
        "bg":      "#0d1117",
        "panel":   "#161b22",
        "border":  "#30363d",
        "fg":      "#e6edf3",
        "muted":   "#7d8590",
        "accent":  "#2f81f7",
        "accent_fg": "#ffffff",
        "ok":      "#3fb950",
        "err":     "#f85149",
        "tool":    "#bc8cff",
        "reason":  "#6e7681",
        "diff_add_bg":    "#13351f",
        "diff_del_bg":    "#3a1620",
        "cat_messages":   "#4f8cff",
        "cat_system":     "#9aa0a6",
        "cat_free":       "#3a3f44",
    },
}

VALID_THEMES = tuple(_PALETTES)


def _palette_for(theme: str) -> dict[str, str]:
    """Return the palette dict for *theme*, falling back to light if unknown."""
    return dict(_PALETTES.get(theme, _PALETTES["light"]))


def _load_theme() -> str:
    """The persisted UI theme (default 'light'); only a known value is honored."""
    val = _load_state().get("theme", "light")
    return val if val in _PALETTES else "light"


def _save_theme(theme: str) -> None:
    """Persist the UI theme choice (best-effort; never raises)."""
    if theme in _PALETTES:
        _save_state({"theme": theme})


class _UiEvent:
    """A message from the worker thread to the Tk main thread."""

    __slots__ = ("kind", "payload")

    def __init__(self, kind: str, payload: Any = None) -> None:
        self.kind = kind          # "stream" | "reasoning" | "tool" | "result"
        self.payload = payload    # | "final" | "error" | "approval" | "done"


class _ApprovalRequestUI:
    """Carries an approval prompt across threads with a blocking handshake."""

    __slots__ = ("request", "event", "answer", "always", "session_all")

    def __init__(self, request: ApprovalRequest) -> None:
        self.request = request
        self.event = threading.Event()
        self.answer = False
        self.always = False  # set when the user clicks "Always allow this command"
        # set when the user clicks "Allow all desktop (session)" — Codex-style
        # approve-for-session for every mcp__* desktop action this session.
        self.session_all = False


class NanocodexGUI:
    def __init__(self, overrides: dict, workspace: Path, *, resume: bool) -> None:
        self._overrides = overrides
        self._workspace = workspace
        self._resume = resume

        self._ui_queue: "queue.Queue[_UiEvent]" = queue.Queue()
        self._loop = None          # AgentLoop, built lazily on the worker thread
        self._worker: Optional[threading.Thread] = None
        self._busy = False
        # Codex-style task queue: while a turn runs you can type the NEXT task
        # and it waits its turn instead of being rejected. Plain list, touched
        # ONLY on the main thread (enqueue in _on_send, dequeue in the "done"
        # handler), so it needs no lock. Stop cancels only the running turn, not
        # the queue (the user's chosen semantics: "stop current, queue goes on").
        self._pending_inputs: list[str] = []
        # Files attached to the NEXT send (📎 button). Collected on the main
        # thread, consumed when an idle turn starts. Image files become OpenAI
        # multimodal blocks (build_user_content); text-like files are read and
        # inlined into the prompt. Pending attachments ride the next idle send.
        self._attached_files: list[str] = []
        # Cooperative cancellation: set to request the running turn stop at its
        # next iteration boundary (a Python thread can't be force-killed).
        self._cancel_event = threading.Event()
        self._build_error: Optional[str] = None
        # Auto-approve state. The worker thread reads the plain bool (atomic in
        # CPython) rather than a Tk variable, which isn't safe off the main thread.
        self._auto_approve_on = False
        self._always_allow: set[str] = set()
        # Codex-style "approve for session": once the user OKs a desktop action
        # with "Allow all desktop (session)", every later MCP tool (mcp__*) runs
        # without prompting for the rest of this session. A plain bool the worker
        # thread reads (atomic in CPython), like _auto_approve_on. Sending one
        # WeChat message is several MCP steps (focus/click/type/press); this lets
        # the user approve the whole flow once instead of clicking per micro-step.
        self._allow_all_mcp = False
        # Right-side file-diff panel (manual): OFF by default. _last_file_edit
        # caches the most recent apply_patch payload so opening the panel after
        # an edit renders it immediately; shown state gates live rendering.
        self._file_panel_shown = False
        self._last_file_edit: Optional[dict] = None
        # Persistent MCP: a long-lived thread owns an event loop that the MCP
        # stdio connection stays bound to for the whole session. Tool calls from
        # per-turn loops are bridged onto it via run_coroutine_threadsafe.
        self._mcp_loop: Optional[Any] = None      # asyncio loop on the MCP thread
        self._mcp_thread: Optional[threading.Thread] = None
        self._mcp_manager: Any = None
        self._mcp_started = False

        # Managed scheduler (Direction A): a daemon thread owns its own event
        # loop and runs due scheduled tasks unattended, so GUI users never need
        # `nanocodex schedule run` in a terminal. SECURITY: when it fires a task
        # marked allow_desktop, that task drives the real desktop with nobody
        # watching — gated by _desktop_only_approver (see _scheduler_run_task).
        #   * _desktop_lock: the user's interactive turn and a scheduled task
        #     drive the SAME mouse/keyboard, so they must never run at once. The
        #     user's turn takes the lock (blocking); a scheduled task tries it
        #     non-blocking and skips its tick if the user is busy (user wins).
        #   * _scheduler_enabled: a plain bool (atomic in CPython) the scheduler
        #     loop reads as its stop_check. The toggle flips it; default ON
        #     (the user chose "GUI auto-hosts the scheduler").
        self._desktop_lock = threading.Lock()
        self._scheduler_thread: Optional[threading.Thread] = None
        self._scheduler_loop: Optional[Any] = None
        self._scheduler_started = False
        self._scheduler_enabled = _load_scheduler_enabled()
        # Set to ask the scheduler loop to stop polling (toggle off / GUI close).
        self._scheduler_stop = threading.Event()
        #   * _scheduler_running_id: the id of the task running RIGHT NOW, or None.
        #     Written by the scheduler thread (plain str assignment, atomic in
        #     CPython), read by the main thread's slow panel refresh. This is the
        #     only "live" bit the panel can't get from schedule.json — everything
        #     else (next_run/last_run/runs) the store persists.
        self._scheduler_running_id: Optional[str] = None
        # Guard so the slow Scheduled-panel refresh timer is armed only ONCE
        # (_init_loop re-runs on project/model switch; without this each re-run
        # would stack another after() loop and the panel would repaint N times).
        self._sched_panel_timer_on = False

        # Session directory: a global, browsable index of past conversations.
        # Keyed by session_id — each conversation (this launch, and each later
        # Open-project) is a SEPARATE history entry with its own frozen
        # full-transcript snapshot. Updated after each turn; shown in the left
        # sidebar. Best-effort — a failure here must never break a turn.
        from nanocodex.agent.session_index import SessionIndex, new_session_id
        # The id for THIS conversation; a fresh one is minted on Open project.
        self._session_id: str = new_session_id()
        try:
            self._session_index: Any = SessionIndex()
        except Exception:  # noqa: BLE001 - index is a convenience, not core
            self._session_index = None

        # Real-token cost accounting (in-memory, this session only — not
        # persisted). Each turn's TurnResult carries summed usage; we price it
        # via pricing.cost_usd and add to a running session total shown in the
        # status bar. Reset to zero when a new conversation starts
        # (_start_new_session). None means "no priced turn yet".
        self._session_cost_usd: float = 0.0
        self._last_turn_cost_usd: Optional[float] = None

        # UI theme: 'light' (Codex-style, default) or 'dark' (original). Read
        # here so _build_widgets can pick the palette; switched live in
        # Settings → General (persists to gui_state.json).
        self._theme: str = _load_theme()

        self._build_widgets()
        # Build the AgentLoop up front (cheap, no network) to surface config
        # errors before the user types anything.
        self._init_loop()
        self._poll_queue()

    # --- Tk widgets ------------------------------------------------------

    def _build_widgets(self) -> None:
        import tkinter as tk
        from tkinter import scrolledtext

        self._tk = tk
        self.root = tk.Tk()
        self.root.title("nanocodex")
        self.root.geometry("960x680")
        self.root.minsize(640, 480)

        # --- palette (Codex-style; light by default, dark optional) ------
        # Both palettes live in _palette_for(); the saved choice (Settings →
        # General) selects one. Every widget reads self._palette, so switching
        # the theme + rebuilding re-colors the whole UI.
        P = self._palette = _palette_for(self._theme)
        self.root.configure(bg=P["bg"])

        def flat_btn(parent, text, command, *, accent=False):
            return tk.Button(
                parent, text=text, command=command,
                bg=P["accent"] if accent else P["panel"],
                fg=P["accent_fg"] if accent else P["fg"],
                activebackground=P["accent"] if accent else P["border"],
                activeforeground=P["accent_fg"] if accent else P["fg"],
                relief="flat", bd=0, padx=14, pady=6,
                font=("Segoe UI", 9), cursor="hand2",
                highlightthickness=0,
            )

        def add_tooltip(widget, text):
            """Lightweight hover tooltip so each action button is self-explaining.

            A borderless Toplevel shown on <Enter> near the cursor and destroyed
            on <Leave>; best-effort (any Tk error is swallowed so a tooltip can
            never break a click).
            """
            tip = {"win": None}

            def show(event=None):
                if tip["win"] is not None:
                    return
                try:
                    tw = tk.Toplevel(widget)
                    tw.wm_overrideredirect(True)
                    x = widget.winfo_rootx() + 12
                    y = widget.winfo_rooty() - 30
                    tw.wm_geometry(f"+{x}+{y}")
                    tk.Label(
                        tw, text=text, justify="left",
                        bg=P["fg"], fg=P["bg"], relief="flat", bd=0,
                        font=("Segoe UI", 9), padx=8, pady=4,
                    ).pack()
                    tip["win"] = tw
                except Exception:
                    tip["win"] = None

            def hide(event=None):
                tw = tip["win"]
                tip["win"] = None
                if tw is not None:
                    try:
                        tw.destroy()
                    except Exception:
                        pass

            widget.bind("<Enter>", show)
            widget.bind("<Leave>", hide)
            widget.bind("<ButtonPress>", hide)
            return widget

        # --- top bar -----------------------------------------------------
        top = tk.Frame(self.root, bg=P["bg"])
        top.pack(fill=tk.X, padx=16, pady=(14, 8))
        self.open_btn = flat_btn(top, "Open project", self._on_open_project)
        self.open_btn.pack(side=tk.LEFT)
        self.new_session_btn = flat_btn(top, "New session", self._on_new_session)
        self.new_session_btn.pack(side=tk.LEFT, padx=(8, 0))
        # Single Settings entry: the former separate "Plugins" (MCP) button was
        # folded into the Settings window's "MCP servers" section.
        self.settings_btn = flat_btn(top, "Settings", self._open_settings)
        self.settings_btn.pack(side=tk.LEFT, padx=(8, 0))
        # A/B: run one task under two configs in isolated git worktrees, then
        # adopt the better side's changes. Requires a clean git workspace.
        self.ab_btn = flat_btn(top, "A/B", self._on_ab_compare)
        self.ab_btn.pack(side=tk.LEFT, padx=(8, 0))
        # 分镜: story -> chapters + shots preview -> (on command) render video.
        # A dedicated panel so the user reviews the breakdown BEFORE spending.
        self.storyboard_btn = flat_btn(top, "分镜", self._open_storyboard_panel)
        self.storyboard_btn.pack(side=tk.LEFT, padx=(8, 0))
        add_tooltip(self.storyboard_btn,
                    "分镜：把故事拆成章节+镜头先预览，\n满意后再出片（出片真实计费）")
        self.ws_label = tk.Label(top, text="", anchor="w", bg=P["bg"],
                                 fg=P["muted"], font=("Segoe UI", 9))
        self.ws_label.pack(side=tk.LEFT, padx=(12, 0), fill=tk.X, expand=True)
        self._auto_var = tk.BooleanVar(value=False)
        self.auto_chk = tk.Checkbutton(
            top, text="Auto-approve", variable=self._auto_var,
            command=self._on_toggle_auto, bg=P["bg"], fg=P["muted"],
            activebackground=P["bg"], activeforeground=P["fg"],
            selectcolor=P["panel"], relief="flat", bd=0,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
        )
        self.auto_chk.pack(side=tk.RIGHT)
        # Managed scheduler toggle. Default reflects the persisted choice (ON).
        # SECURITY: when ON, a due task marked allow_desktop drives the desktop
        # unattended — this switch is the user's always-available kill switch.
        self._sched_var = tk.BooleanVar(value=self._scheduler_enabled)
        self.sched_chk = tk.Checkbutton(
            top, text="Scheduler", variable=self._sched_var,
            command=self._on_toggle_scheduler, bg=P["bg"], fg=P["muted"],
            activebackground=P["bg"], activeforeground=P["fg"],
            selectcolor=P["panel"], relief="flat", bd=0,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
        )
        self.sched_chk.pack(side=tk.RIGHT, padx=(0, 12))
        # Manual file-diff panel toggle. OFF by default (the user opts in); when
        # ON, each apply_patch renders its diff in the right-side dock.
        self._files_var = tk.BooleanVar(value=False)
        self.files_chk = tk.Checkbutton(
            top, text="Files", variable=self._files_var,
            command=self._on_toggle_file_panel, bg=P["bg"], fg=P["muted"],
            activebackground=P["bg"], activeforeground=P["fg"],
            selectcolor=P["panel"], relief="flat", bd=0,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
        )
        self.files_chk.pack(side=tk.RIGHT, padx=(0, 12))

        # hairline separator under the top bar
        tk.Frame(self.root, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X)

        # IMPORTANT: pack the bottom bars FIRST (side=BOTTOM) so they always
        # reserve their space. The expanding transcript is packed LAST and only
        # takes what's left — otherwise shrinking the window clips the input and
        # status bars off the bottom.

        # --- status bar (very bottom): model switcher + clickable usage --
        status_bar = tk.Frame(self.root, bg=P["panel"])
        status_bar.pack(side=tk.BOTTOM, fill=tk.X)
        tk.Frame(self.root, bg=P["border"], height=1).pack(side=tk.BOTTOM, fill=tk.X)
        # Model switcher lives down here now (not the top bar).
        self.model_btn = tk.Button(
            status_bar, text="model: …", command=self._on_pick_model,
            bg=P["panel"], fg=P["fg"], activebackground=P["border"],
            activeforeground=P["fg"], relief="flat", bd=0, padx=12, pady=4,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
        )
        self.model_btn.pack(side=tk.LEFT)
        # Clickable context usage — click to expand a details popup.
        self.status = tk.Label(
            status_bar, text="starting…", anchor="w", bg=P["panel"], fg=P["fg"],
            font=("Segoe UI", 10), padx=8, pady=6, cursor="hand2",
        )
        self.status.pack(side=tk.LEFT, fill=tk.X, expand=True)
        self.status.bind("<Button-1>", lambda e: self._show_context_details())

        # --- input bar (above status) ------------------------------------
        bottom = tk.Frame(self.root, bg=P["bg"])
        bottom.pack(side=tk.BOTTOM, fill=tk.X, padx=16, pady=12)
        tk.Frame(self.root, bg=P["border"], height=1).pack(side=tk.BOTTOM, fill=tk.X)
        entry_wrap = tk.Frame(bottom, bg=P["panel"], highlightbackground=P["border"],
                              highlightthickness=1, bd=0)
        entry_wrap.pack(side=tk.LEFT, fill=tk.X, expand=True)
        # Starts at 2 rows and auto-grows with content up to 12 rows (then the
        # box scrolls internally) — pasting a whole story no longer hides most
        # of it behind a fixed 3-line window. See _autogrow_entry.
        self._entry_min_rows = 2
        self._entry_max_rows = 12
        self.entry = tk.Text(entry_wrap, height=self._entry_min_rows, wrap=tk.WORD,
                             font=("Cascadia Code", 11), bg=P["panel"], fg=P["fg"],
                             insertbackground=P["fg"], relief="flat", bd=0,
                             padx=12, pady=8, highlightthickness=0)
        self.entry.pack(fill=tk.BOTH, expand=True)
        # Enter sends; Shift+Enter inserts a newline (chat-box convention).
        self.entry.bind("<Return>", self._on_send)
        self.entry.bind("<Shift-Return>", lambda e: None)  # fall through -> newline
        # Auto-grow on any content change (typing, paste, programmatic prefill).
        self.entry.bind("<KeyRelease>", self._autogrow_entry)
        self.entry.bind("<<Modified>>", self._autogrow_entry)
        self.send_btn = flat_btn(bottom, "Send  ⏎", self._on_send, accent=True)
        self.send_btn.pack(side=tk.RIGHT, fill=tk.Y, padx=(10, 0))
        add_tooltip(self.send_btn, "发送 (Enter)。Shift+Enter 换行")
        # ✨ Enhance: rewrite the raw input into a clearer prompt, then PREVIEW
        # it before sending (never silently replaces the user's words).
        self.enhance_btn = flat_btn(bottom, "✨ 润色", self._on_enhance)
        self.enhance_btn.pack(side=tk.RIGHT, fill=tk.Y, padx=(10, 0))
        add_tooltip(self.enhance_btn,
                    "润色输入：把草稿改写成更清晰的 prompt，\n发送前先预览，不会替换你的原话")
        # 📎 Attach: pick local files for the NEXT send. Images become OpenAI
        # multimodal blocks; text-like files (.txt/.md/.json…) are read and
        # inlined into the prompt. The button label shows the pending count.
        self.attach_btn = flat_btn(bottom, "📎 附件", self._on_attach)
        self.attach_btn.pack(side=tk.RIGHT, fill=tk.Y, padx=(10, 0))
        add_tooltip(self.attach_btn,
                    "附件：给下一条消息带上本地文件\n图片作图像块，文本类文件读进 prompt")
        # Dedicated, always-visible Stop button (disabled until a turn runs).
        self.stop_btn = tk.Button(
            bottom, text="■ Stop", command=self._request_stop,
            bg=P["panel"], fg=P["err"], activebackground=P["border"],
            activeforeground=P["err"], relief="flat", bd=0, padx=14, pady=6,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
            state=tk.DISABLED,
        )
        self.stop_btn.pack(side=tk.RIGHT, fill=tk.Y, padx=(10, 0))
        add_tooltip(self.stop_btn,
                    "停止：取消正在跑的这一轮\n排队中的下一个任务照常继续")
        # Continue button: enabled only when a turn stopped with work left
        # (hit the step limit, or paused with unfinished plan steps). One click
        # resumes instead of making the user type "continue".
        self.continue_btn = tk.Button(
            bottom, text="▶ Continue", command=self._on_continue,
            bg=P["panel"], fg=P["accent"], activebackground=P["border"],
            activeforeground=P["accent"], relief="flat", bd=0, padx=14, pady=6,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
            state=tk.DISABLED,
        )
        self.continue_btn.pack(side=tk.RIGHT, fill=tk.Y, padx=(10, 0))
        add_tooltip(self.continue_btn,
                    "继续：上一轮没干完时点它接着干\n（撞步数上限或还有未完成步骤时才亮）")

        # --- body: session sidebar (left) + transcript (right) -----------
        # A horizontal container so the directory list sits beside the
        # transcript and both share the space left by the bars above/below.
        body = tk.Frame(self.root, bg=P["bg"])
        body.pack(side=tk.TOP, fill=tk.BOTH, expand=True)

        # Left sidebar: a scrollable directory of past conversations. Click a
        # row to view that session's summary. Width is fixed so the transcript
        # gets the rest; pack_propagate(False) keeps the frame from shrinking
        # to its contents.
        sidebar = tk.Frame(body, bg=P["panel"], width=240)
        sidebar.pack(side=tk.LEFT, fill=tk.Y)
        sidebar.pack_propagate(False)
        tk.Frame(body, bg=P["border"], width=1).pack(side=tk.LEFT, fill=tk.Y)
        tk.Label(
            sidebar, text="Sessions", anchor="w", bg=P["panel"], fg=P["muted"],
            font=("Segoe UI", 9, "bold"), padx=12, pady=8,
        ).pack(side=tk.TOP, fill=tk.X)
        self.session_list = tk.Listbox(
            sidebar, bg=P["panel"], fg=P["fg"], relief="flat", bd=0,
            font=("Segoe UI", 9), activestyle="none",
            selectbackground=P["accent"], selectforeground=P["accent_fg"],
            highlightthickness=0,
        )
        self.session_list.pack(side=tk.TOP, fill=tk.BOTH, expand=True,
                               padx=6, pady=(0, 8))
        self.session_list.bind("<<ListboxSelect>>", self._on_session_select)
        # Workspaces aligned with the listbox rows (index -> SessionSummary).
        self._session_entries: list[Any] = []

        # --- Scheduled panel (bottom of the sidebar) ---------------------
        # A live read-out of scheduled tasks: which is running NOW (the only bit
        # the schedule.json store can't give us — set by the scheduler thread),
        # plus next/last/×runs from the store. The user chose "task activity does
        # not enter the transcript", so this panel is how the conversation stays
        # uncluttered yet the automation stays visible. Fixed height so it never
        # crowds out the Sessions list above it.
        tk.Frame(sidebar, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X)
        # Read-only status read-out; the single on/off control is the top-bar
        # "Scheduler" toggle (no duplicate switch here, matching the design).
        tk.Label(
            sidebar, text="Scheduled", anchor="w", bg=P["panel"], fg=P["muted"],
            font=("Segoe UI", 9, "bold"), padx=12, pady=8,
        ).pack(side=tk.TOP, fill=tk.X)
        self.schedule_panel = tk.Text(
            sidebar, height=8, wrap="word", bg=P["panel"], fg=P["fg"],
            relief="flat", bd=0, font=("Segoe UI", 9), padx=12, pady=4,
            highlightthickness=0, state=tk.DISABLED, cursor="arrow",
        )
        self.schedule_panel.pack(side=tk.TOP, fill=tk.X, pady=(0, 8))
        self.schedule_panel.tag_config("running", foreground=P["ok"])
        self.schedule_panel.tag_config("idle", foreground=P["fg"])
        self.schedule_panel.tag_config("off", foreground=P["muted"])
        self.schedule_panel.tag_config("empty", foreground=P["muted"])

        # --- transcript (fills whatever space remains) -------------------
        self.output = scrolledtext.ScrolledText(
            body, wrap=tk.WORD, state=tk.DISABLED,
            font=("Cascadia Code", 11), bg=P["bg"], fg=P["fg"],
            insertbackground=P["fg"], relief="flat", bd=0,
            padx=18, pady=14, highlightthickness=0, spacing3=4,
        )
        self.output.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        self.output.tag_config("user", foreground=P["accent"], font=("Cascadia Code", 11, "bold"))
        self.output.tag_config("reasoning", foreground=P["reason"], font=("Cascadia Code", 10, "italic"))
        self.output.tag_config("content", foreground=P["fg"])
        self.output.tag_config("tool", foreground=P["tool"])
        self.output.tag_config("result_ok", foreground=P["ok"])
        self.output.tag_config("result_err", foreground=P["err"])
        self.output.tag_config("system", foreground=P["muted"])

        # --- right-side file-diff dock (created hidden; manual toggle) ----
        # Packed with side=RIGHT so it's order-independent of the LEFT-packed
        # expanding transcript. NOT packed here — _show_file_panel packs it
        # (and its 1px border) on demand; the user opens it via the top-bar
        # "Files" switch. width fixed like the sidebar via pack_propagate(False).
        self._file_panel_border = tk.Frame(body, bg=P["border"], width=1)
        self.file_panel = tk.Frame(body, bg=P["panel"], width=460)
        self.file_panel.pack_propagate(False)
        fp_head = tk.Frame(self.file_panel, bg=P["panel"])
        fp_head.pack(side=tk.TOP, fill=tk.X)
        self.file_panel_title = tk.Label(
            fp_head, text="(no edits yet)", anchor="w", bg=P["panel"],
            fg=P["muted"], font=("Segoe UI", 9, "bold"), padx=12, pady=8,
        )
        self.file_panel_title.pack(side=tk.LEFT, fill=tk.X, expand=True)
        flat_btn(fp_head, "×", self._hide_file_panel).pack(side=tk.RIGHT, padx=(0, 8))
        tk.Frame(self.file_panel, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X)
        fp_body = tk.Frame(self.file_panel, bg=P["bg"])
        fp_body.pack(side=tk.TOP, fill=tk.BOTH, expand=True)
        fp_vsb = tk.Scrollbar(fp_body, orient=tk.VERTICAL)
        fp_hsb = tk.Scrollbar(fp_body, orient=tk.HORIZONTAL)
        self.file_view = tk.Text(
            fp_body, wrap="none", state=tk.DISABLED,
            font=("Cascadia Code", 10), bg=P["bg"], fg=P["fg"],
            relief="flat", bd=0, padx=8, pady=8, highlightthickness=0,
            yscrollcommand=fp_vsb.set, xscrollcommand=fp_hsb.set, cursor="arrow",
        )
        fp_vsb.config(command=self.file_view.yview)
        fp_hsb.config(command=self.file_view.xview)
        fp_vsb.pack(side=tk.RIGHT, fill=tk.Y)
        fp_hsb.pack(side=tk.BOTTOM, fill=tk.X)
        self.file_view.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        self.file_view.tag_config("lineno", foreground=P["muted"])
        self.file_view.tag_config("added", background=P["diff_add_bg"], foreground=P["fg"])
        self.file_view.tag_config("removed", background=P["diff_del_bg"], foreground=P["fg"])
        self.file_view.tag_config("context", foreground=P["fg"])
        self.file_view.tag_config("meta", foreground=P["accent"], font=("Cascadia Code", 10, "bold"))
        self.file_view.tag_config("hunk", foreground=P["muted"], font=("Cascadia Code", 10, "italic"))

    # --- auto-approve + context usage ------------------------------------

    def _on_toggle_auto(self) -> None:
        """Mirror the Tk checkbox into the plain bool the worker thread reads.

        Auto-approve ON  -> full auto (no prompts for in-sandbox writes).
        Auto-approve OFF -> confirm each write step (the default), so the user
        gates the session. We set ctx.require_step_approval to the inverse.
        """
        self._auto_approve_on = bool(self._auto_var.get())
        self._sync_step_approval()
        if self._auto_approve_on:
            self._append(
                "[auto-approve ON — writes & commands run without asking. "
                "Uncheck to confirm each step.]\n",
                "result_err",
            )
        else:
            self._append("[auto-approve OFF — you'll confirm each write/command.]\n", "system")
        self._update_context_usage()

    def _sync_step_approval(self) -> None:
        """Push the toggle state onto the live ToolContext (worker reads it)."""
        loop = self._loop
        ctx = getattr(getattr(loop, "tools", None), "ctx", None)
        if ctx is not None:
            # OFF auto-approve => require per-step confirmation.
            ctx.require_step_approval = not self._auto_approve_on

    # --- right-side file-diff panel --------------------------------------

    def _on_toggle_file_panel(self) -> None:
        """Top-bar 'Files' switch: show/hide the right-side diff dock."""
        if bool(self._files_var.get()):
            self._show_file_panel()
        else:
            self._hide_file_panel()

    def _show_file_panel(self) -> None:
        """Pack the dock (and its border) on the right; render any cached edit."""
        if not self._file_panel_shown:
            self._file_panel_border.pack(side=self._tk.RIGHT, fill=self._tk.Y)
            self.file_panel.pack(side=self._tk.RIGHT, fill=self._tk.Y)
            self._file_panel_shown = True
        # Keep the top-bar checkbox in sync (also covers programmatic opens).
        self._files_var.set(True)
        if self._last_file_edit is not None:
            self._render_file_edit(self._last_file_edit)

    def _hide_file_panel(self) -> None:
        """Collapse the dock; the transcript reclaims the freed width."""
        if self._file_panel_shown:
            self.file_panel.pack_forget()
            self._file_panel_border.pack_forget()
            self._file_panel_shown = False
        self._files_var.set(False)

    def _on_file_edit(self, payload: "dict") -> None:
        """Worker reported an apply_patch; cache it, render only if panel is open.

        Manual mode: a hidden panel just stores the latest edit so flipping the
        switch on shows it — an edit never forces the dock open.
        """
        self._last_file_edit = payload
        if self._file_panel_shown:
            self._render_file_edit(payload)

    def _render_file_edit(self, payload: "dict") -> None:
        """Draw the classified diff rows into the read-only file view."""
        tk = self._tk
        files = payload.get("files") or []
        view = self.file_view
        view.config(state=tk.NORMAL)
        view.delete("1.0", "end")

        total = len(files)
        # Header: first file's path, plus a count when the patch spans several.
        first = files[0] if files else {"path": "(none)", "op": "?"}
        header = f"{first.get('op', '?')}  {first.get('path', '')}"
        mv = first.get("move_to")
        if mv:
            header += f"  →  {mv}"
        if total > 1:
            header += f"   (1 of {total})"
        self.file_panel_title.config(text=header)

        for fi, f in enumerate(files):
            path = f.get("path", "")
            op = f.get("op", "?")
            move_to = f.get("move_to")
            meta = f"── {op}  {path}"
            if move_to:
                meta += f"  →  {move_to}"
            if fi:
                view.insert("end", "\n", "context")
            view.insert("end", meta + "\n", "meta")
            for row in f.get("rows", []):
                kind = row.get("kind")
                text = row.get("text", "")
                if kind == "hunk_sep":
                    view.insert("end", f"    @@ {text}\n".rstrip() + "\n", "hunk")
                    continue
                gutter = _line_gutter(row.get("new_no") if kind != "removed"
                                      else row.get("old_no"))
                marker = {"added": "+", "removed": "-"}.get(kind, " ")
                view.insert("end", gutter, "lineno")
                view.insert("end", f"{marker}{text}\n", kind if kind in
                            ("added", "removed") else "context")
            if f.get("truncated"):
                view.insert("end", "… (truncated)\n", "hunk")

        view.config(state=tk.DISABLED)
        view.see("1.0")

    def _update_context_usage(self) -> None:
        """Status bar, Claude-Code style: state | model | used / window (%)."""
        loop = self._loop
        if loop is None:
            # No working loop (e.g. config error) — show the reason, not blank.
            self.status.config(text=_build_status(
                busy=self._busy, auto_on=self._auto_approve_on,
                error=self._build_error or "no model loaded",
            ))
            return
        cfg = getattr(loop, "_cfg", None)
        model = getattr(cfg, "model", None) if cfg else None
        tokens = window = budget = None
        try:
            from nanocodex.agent.compaction import estimate_tokens
            tokens = estimate_tokens(loop.session.for_model())
            window = int(getattr(cfg, "context_window", 0) or 0)
            budget = int(getattr(loop.compaction, "token_budget", 0) or 0)
        except Exception:
            pass
        # Seedance video spend (CNY) accrues on the shared ToolContext as the
        # StoryboardTool renders clips. Read it back here so the status bar
        # shows it alongside (but separate from) the USD turn cost.
        seedance_cny = 0.0
        try:
            seedance_cny = float(getattr(loop.tools.ctx, "seedance_cost_cny", 0.0) or 0.0)
        except Exception:
            pass
        self.status.config(text=_build_status(
            busy=self._busy, auto_on=self._auto_approve_on,
            model=model, tokens=tokens, window=window, budget=budget,
            session_cost=self._session_cost_usd, seedance_cny=seedance_cny,
        ) + "   ›")

    def _show_context_details(self) -> None:
        """Popup breaking down context usage, styled after Claude Code:
        a header (used / window, %), a colored progress bar, then categories
        sorted by size with a color swatch, token count, and percentage."""
        tk = self._tk
        P = self._palette
        loop = self._loop
        if loop is None:
            return

        # Single-instance: close any previous popup so repeated clicks don't
        # stack a pile of windows.
        prev = getattr(self, "_ctx_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:
                pass

        dlg = tk.Toplevel(self.root, bg=P["panel"])
        self._ctx_dlg = dlg
        dlg.title("Context window")
        dlg.resizable(True, True)
        dlg.geometry("440x340")

        # Colors for the categories (swatch + bar segments), theme-aware so the
        # bar reads cleanly on both the light and dark palettes.
        cat_colors = {
            "Messages": P["cat_messages"],
            "System prompt": P["cat_system"],
            "Free space": P["cat_free"],
        }

        txt = tk.Text(dlg, wrap="word", bg=P["panel"], fg=P["fg"],
                      font=("Segoe UI", 10), relief="flat", bd=0,
                      padx=16, pady=14, highlightthickness=0, spacing1=2, spacing3=2)
        txt.pack(fill="both", expand=True)

        try:
            from nanocodex.agent.compaction import estimate_tokens

            cfg = getattr(loop, "_cfg", None)
            msgs = loop.session.for_model()
            window = int(getattr(cfg, "context_window", 0) or 0)
            budget = int(getattr(loop.compaction, "token_budget", 0) or 0)

            # nanocodex's real categories (we don't fake Memory/Skills/MCP that
            # this app doesn't track): Messages = user+assistant+tool,
            # System prompt = system, Free space = window - used.
            messages_tok = sum(estimate_tokens([m]) for m in msgs if m.get("role") != "system")
            system_tok = sum(estimate_tokens([m]) for m in msgs if m.get("role") == "system")
            used = messages_tok + system_tok
            free = max(0, window - used) if window else 0
            denom = window if window else max(1, used)

            cats = [
                ("Messages", messages_tok),
                ("System prompt", system_tok),
                ("Free space", free),
            ]
            cats = [c for c in cats if c[1] > 0]
            cats.sort(key=lambda c: c[1], reverse=True)

            pct = int(used / window * 100) if window else 0
            head = f"{_fmt_tok(used)} / {_fmt_tok(window)} ({pct}%)" if window else f"{_fmt_tok(used)} tokens"

            # --- header ---
            txt.tag_config("title", font=("Segoe UI", 11), foreground=P["muted"])
            txt.tag_config("head", font=("Segoe UI", 12, "bold"), foreground=P["fg"])
            txt.insert("end", "Context window   ", "title")
            txt.insert("end", head + "\n\n", "head")

            # --- progress bar: a Canvas (not block chars, which wrap on a
            # proportional font) for a single clean proportional bar ---
            bar_w, bar_h = 396, 14
            bar = tk.Canvas(txt, width=bar_w, height=bar_h, bg=P["bg"],
                            highlightthickness=0, bd=0)
            x = 0
            for name, tok in cats:
                seg = int(round(tok / denom * bar_w)) if denom else 0
                if seg <= 0:
                    continue
                color = cat_colors.get(name, P["accent"])
                bar.create_rectangle(x, 0, min(x + seg, bar_w), bar_h,
                                     fill=color, width=0)
                x += seg
            txt.window_create("end", window=bar)
            txt.insert("end", "\n\n")

            # --- per-category rows (swatch + name + tokens + pct) ---
            txt.tag_config("muted", foreground=P["muted"])
            for name, tok in cats:
                color = cat_colors.get(name, P["accent"])
                sw = f"sw_{name}"
                txt.tag_config(sw, foreground=color)
                row_pct = (tok / denom * 100) if denom else 0
                txt.insert("end", "  ■ ", sw)
                txt.insert("end", f"{name:<16}", )
                txt.insert("end", f"{_fmt_tok(tok):>8}   {row_pct:4.1f}%\n", "muted")

            txt.insert("end", "\n")
            txt.insert("end", f"model:  {getattr(cfg, 'model', '?')}\n", "muted")
            if not window:
                txt.insert("end", "context window: not set (NANOCODEX_CONTEXT_WINDOW)\n", "muted")
            txt.insert("end",
                       (f"auto-compaction: ON @ {_fmt_tok(budget)}\n" if budget > 0
                        else "auto-compaction: OFF (--context-budget 0)\n"), "muted")
        except Exception as exc:  # noqa: BLE001 - show the reason, not a blank box
            txt.insert("end", f"(could not compute details)\n\n{type(exc).__name__}: {exc}")

        txt.config(state="disabled")
        dlg.lift()
        dlg.update()
        dlg.lift()
        dlg.update()                         # force a draw on this Tk build

    # --- loop construction ----------------------------------------------

    def _init_loop(self) -> None:
        from nanocodex.cli import _build_loop  # reuse the real builder

        def gui_approver_factory(policy_name: str) -> Approver:
            return Approver(policy_name, self._approve_via_ui)

        self._build_error = None
        # A one-shot seed: when set (by "Continue this conversation"), the loop
        # is forked from these messages instead of started fresh / resumed. Read
        # once then cleared so a later project/model switch starts clean.
        seed = getattr(self, "_pending_seed", None)
        self._pending_seed = None
        try:
            self._loop = _build_loop(
                self._overrides, self._workspace,
                resume=self._resume and seed is None,
                approver_factory=gui_approver_factory,
                seed_messages=seed,
            )
            cfg = self._loop._cfg  # type: ignore[attr-defined]
            info = cfg.redacted()
            self.ws_label.config(text=f"workspace: {info['workspace']}")
            self.model_btn.config(text=f"model: {info['model']} ▾")
            self._append(
                f"nanocodex  |  model={info['model']}  sandbox={info['sandbox_mode']}  "
                f"approval={info['approval_policy']}\nworkspace={info['workspace']}\n\n",
                "system",
            )
            if self._resume:
                restored = getattr(self._loop.session, "restored_count", 0)
                self._append(
                    f"Resumed {restored} message(s).\n\n" if restored
                    else "No previous session found; starting fresh.\n\n",
                    "system",
                )
            self.send_btn.config(state=self._tk.NORMAL)
            self._update_context_usage()  # refresh model + context display
            self._sync_step_approval()    # apply the current toggle to the new ctx
            # Auto-connect MCP servers from ~/.nanocodex/mcp.toml in the
            # background, so GUI users get desktop/tool capabilities without
            # ever touching the command line or the --mcp flag.
            self._autoconnect_mcp()
            # _autoconnect_mcp is idempotent (one MCP thread per session). On a
            # rebuild (New session / project / model switch) it returns early,
            # so the freshly built loop.tools has NO MCP tools. Re-register the
            # already-connected MCP tools onto the new loop so a new session
            # keeps desktop/tool capabilities instead of silently losing them.
            self._reattach_mcp_tools()
            # Populate the session directory now so the sidebar isn't empty
            # until the first turn ends (it lists prior workspaces too).
            self._refresh_session_list()
            # Start the managed scheduler once (survives project/model switches,
            # which re-run _init_loop). Fires due scheduled tasks unattended.
            self._autostart_scheduler()
            # Start the slow Scheduled-panel refresh loop (it paints immediately
            # on its first tick), so the sidebar shows tasks (and a live
            # "running" dot) without waiting.
            self._start_schedule_panel_refresh()
        except ConfigError as exc:
            self._loop = None
            self._build_error = str(exc)
            self.ws_label.config(text=f"workspace: {self._workspace}")
            self._append(f"Config error: {exc}\n", "result_err")
            self.send_btn.config(state=self._tk.DISABLED)

    # --- persistent MCP (auto-loaded for GUI users) ----------------------

    def _autoconnect_mcp(self) -> None:
        """Start a long-lived MCP thread (once) and connect servers on it.

        GUI users never run `--mcp`; this gives them MCP tools automatically
        from ~/.nanocodex/mcp.toml. The connection lives on a dedicated event
        loop for the whole session; per-turn tool calls are bridged onto it.
        """
        if self._mcp_started:
            return
        try:
            from nanocodex.tools.mcp import discover_mcp_servers
        except Exception:
            return
        servers = [s for s in discover_mcp_servers() if s.enabled]
        if not servers:
            return  # nothing configured (or all disabled) — stay silent, it's optional
        self._mcp_started = True
        self._append(
            f"Connecting {len(servers)} MCP server(s) from ~/.nanocodex/mcp.toml "
            "(tools run OUTSIDE the sandbox)…\n", "system",
        )
        self._mcp_thread = threading.Thread(
            target=self._mcp_thread_main, args=(servers,), daemon=True,
        )
        self._mcp_thread.start()

    def _mcp_thread_main(self, servers) -> None:
        """Owns a persistent event loop the MCP connection stays bound to."""
        import asyncio as _asyncio
        from nanocodex.tools.mcp import McpManager

        loop = _asyncio.new_event_loop()
        _asyncio.set_event_loop(loop)
        self._mcp_loop = loop
        try:
            manager = McpManager(self._loop.tools.ctx, servers)
            tools = loop.run_until_complete(manager.connect())
            self._mcp_manager = manager
            # Register each remote tool, but bridge its execution back onto THIS
            # loop (the call arrives from a different per-turn loop).
            for tool in tools:
                self._register_bridged_tool(tool)
            names = ", ".join(t.name for t in tools) if tools else "(none)"
            self._ui_queue.put(_UiEvent("system_text",
                                        f"MCP ready: {len(tools)} tool(s): {names}\n"))
            for err in manager.errors:
                self._ui_queue.put(_UiEvent("error", f"MCP server failed: {err}"))
            loop.run_forever()  # keep the connection alive for the session
        except Exception as exc:  # noqa: BLE001
            self._ui_queue.put(_UiEvent("error", f"MCP connect failed: {exc}"))
        finally:
            try:
                loop.close()
            except Exception:
                pass

    def _register_bridged_tool(self, tool) -> None:
        """Register an MCP tool whose execute() is dispatched to the MCP loop.

        The agent runs each turn on its own loop; an MCP tool's coroutine is
        bound to the MCP loop, so we hop threads via run_coroutine_threadsafe
        and await the result with wrap_future (loop-agnostic)."""
        import asyncio as _asyncio

        mcp_loop = self._mcp_loop
        original_execute = tool.execute

        async def bridged_execute(**kwargs):
            fut = _asyncio.run_coroutine_threadsafe(original_execute(**kwargs), mcp_loop)
            return await _asyncio.wrap_future(fut)

        tool.execute = bridged_execute  # type: ignore[method-assign]
        self._loop.tools.register(tool)

    # --- managed scheduler (auto-host the schedule runner for GUI users) -

    def _autostart_scheduler(self) -> None:
        """Start the in-GUI schedule runner once, on a dedicated thread.

        GUI users never run `nanocodex schedule run`; this hosts it for them so
        a due task fires with no manual step. Honors the user's on/off switch
        (default ON) and is idempotent (started once for the whole session;
        `_init_loop` may run again on project/model switch).

        SECURITY: a due task marked allow_desktop drives the desktop unattended.
        The risk is bounded by (a) allow_desktop being an explicit per-task opt
        in, (b) the desktop-only approver, and (c) the user being able to flip
        the switch off at any time.
        """
        if self._scheduler_started:
            return
        if not self._scheduler_enabled:
            return  # user turned it off; respect that (can re-enable later)
        self._scheduler_started = True
        self._scheduler_stop.clear()
        self._scheduler_thread = threading.Thread(
            target=self._scheduler_thread_main, daemon=True,
        )
        self._scheduler_thread.start()
        self._append(
            "[scheduler ON — due tasks run automatically in the background. "
            "Uncheck 'Scheduler' to stop. Output goes to ~/.nanocodex/scheduler.log, "
            "not here.]\n", "system",
        )

    def _scheduler_thread_main(self) -> None:
        """Own an event loop and poll the ScheduleStore until told to stop."""
        import asyncio as _asyncio
        from nanocodex.agent.schedule import ScheduleStore
        from nanocodex.agent.schedule_runner import run_forever

        loop = _asyncio.new_event_loop()
        _asyncio.set_event_loop(loop)
        try:
            store = ScheduleStore()
            loop.run_until_complete(run_forever(
                store, self._scheduler_run_task,
                poll_interval=30.0,
                on_event=lambda msg: self._scheduler_log(msg),
                stop_check=lambda: self._scheduler_stop.is_set()
                or not self._scheduler_enabled,
            ))
        except Exception as exc:  # noqa: BLE001 - never crash the GUI over scheduling
            self._scheduler_log(f"scheduler thread error: {type(exc).__name__}: {exc}")
        finally:
            try:
                loop.close()
            except Exception:  # noqa: BLE001
                pass

    async def _scheduler_run_task(self, task) -> None:
        """Run ONE due task unattended (on the scheduler loop).

        Concurrency: the GUI conversation and a scheduled task drive the SAME
        mouse/keyboard, so they must never overlap. The desktop lock is taken
        NON-blocking inside ``_run_scheduled_turn`` — if the user is mid-turn
        this firing is skipped (retried next poll) so the conversation always
        wins. The turn is also TIME-BOUNDED there: a stuck task can't pin the
        lock and freeze the GUI forever. All lock handling lives in that helper
        (never here too — a threading.Lock isn't reentrant).
        """
        from nanocodex.cli import (
            _build_loop, _desktop_only_approver, _auto_deny_approver,
        )

        allow_desktop = bool(getattr(task, "allow_desktop", False))
        mcp_connected = self._mcp_manager is not None and self._mcp_loop is not None
        approver_kind, attach_mcp = _scheduler_run_plan(
            allow_desktop=allow_desktop, mcp_connected=mcp_connected,
        )
        task_id = getattr(task, "id", "?")

        async def _run(cancel_check):
            # Mark this task as running NOW so the sidebar panel can show a live
            # "● running" dot. _run is only invoked AFTER the lock is acquired
            # (a skipped tick never calls it), so the flag tracks real execution.
            # try/finally clears it on every exit — normal return, timeout, or
            # error — so a finished/killed task never looks stuck-running.
            self._scheduler_running_id = task_id
            try:
                factory = (_desktop_only_approver if approver_kind == "desktop_only"
                           else _auto_deny_approver)
                # Ephemeral loop: no session.jsonl so it never mixes into /
                # pollutes the user's interactive history or session directory.
                loop = _build_loop(
                    dict(self._overrides), self._workspace, resume=False,
                    approver_factory=lambda _p: factory(), log_path=None,
                )
                if attach_mcp:
                    await self._attach_scheduler_mcp_tools(loop)
                # Hand the loop the cancel flag so a soft timeout stops it clean.
                return await loop.run_turn(task.prompt, cancel_check=cancel_check)
            finally:
                self._scheduler_running_id = None

        outcome = await _run_scheduled_turn(
            lock=self._desktop_lock, run=_run,
            timeout_s=_scheduler_turn_timeout(),
        )

        status = outcome.get("status")
        if status == "skipped":
            # GUI was busy: this is a normal back-off, NOT a task failure. Don't
            # raise (so it doesn't count toward auto-disable) — it retries next poll.
            self._scheduler_log(_format_scheduler_log_entry(
                now_iso=_now_iso(), task_id=task_id, allow_desktop=allow_desktop,
                error="skipped (GUI busy); will retry next poll",
            ))
        elif status == "timeout":
            how = "force-killed" if outcome.get("forced") else "cancelled cleanly"
            self._scheduler_log(_format_scheduler_log_entry(
                now_iso=_now_iso(), task_id=task_id, allow_desktop=allow_desktop,
                error=(f"timed out after {outcome.get('timeout_s')}s ({how}); "
                       "lock released. Raise NANOCODEX_SCHEDULER_TIMEOUT if the "
                       "task legitimately needs longer."),
            ))
            # Surface as a failure so run_due_once marks it (auto-disable after
            # repeated timeouts — a task that can't finish shouldn't spin forever).
            raise RuntimeError(f"scheduled task {task_id} timed out")
        elif status == "error":
            self._scheduler_log(_format_scheduler_log_entry(
                now_iso=_now_iso(), task_id=task_id, allow_desktop=allow_desktop,
                error=outcome.get("error", "unknown error"),
            ))
            raise RuntimeError(
                f"scheduled task {task_id}: {outcome.get('error', 'unknown error')}"
            )
        else:  # done
            self._scheduler_log(_format_scheduler_log_entry(
                now_iso=_now_iso(), task_id=task_id, allow_desktop=allow_desktop,
                stop_reason=outcome.get("stop_reason", ""),
                summary=outcome.get("summary", ""),
            ))

    async def _attach_scheduler_mcp_tools(self, loop) -> None:
        """Give the scheduled task's loop the MCP desktop tools, bridged.

        Rebuilds the tools against the TASK loop's ctx (desktop-only approver),
        not the GUI's, so the approval gate is the unattended one. Execution is
        still bridged onto the MCP event loop (the live sessions live there)."""
        import asyncio as _asyncio

        mcp_loop = self._mcp_loop
        manager = self._mcp_manager
        if manager is None or mcp_loop is None:
            return
        # Build the tools on the MCP loop (build_tools_with_ctx lists tools over
        # the live sessions, which are bound to that loop).
        fut = _asyncio.run_coroutine_threadsafe(
            manager.build_tools_with_ctx(loop.tools.ctx), mcp_loop,
        )
        tools = await _asyncio.wrap_future(fut)
        for tool in tools:
            original_execute = tool.execute

            def _bridge(orig):
                async def bridged_execute(**kwargs):
                    f = _asyncio.run_coroutine_threadsafe(orig(**kwargs), mcp_loop)
                    return await _asyncio.wrap_future(f)
                return bridged_execute

            tool.execute = _bridge(original_execute)  # type: ignore[method-assign]
            loop.tools.register(tool)

    def _reattach_mcp_tools(self) -> None:
        """Re-register the live MCP tools onto the current (rebuilt) loop.

        ``_autoconnect_mcp`` connects once per session on a long-lived MCP
        thread; it returns early on every later ``_init_loop`` (New session /
        project / model switch), so the freshly built ``self._loop.tools`` has
        no MCP tools. The MCP connection itself is still alive on its own event
        loop, so we just rebuild the tool objects against the NEW loop's ctx and
        bridge their execution back onto the MCP loop (same pattern as the
        scheduler attach). No reconnect, no touching the live sessions.

        No-op when MCP isn't connected yet: the first connection hasn't
        finished, and ``_mcp_thread_main`` will register onto whatever loop is
        current when it completes. Mirrors ``_attach_scheduler_mcp_tools`` but
        runs synchronously on the GUI thread against the GUI loop's ctx (the
        normal interactive approver, not the desktop-only one).
        """
        import asyncio as _asyncio

        loop = self._loop
        mcp_loop = self._mcp_loop
        manager = self._mcp_manager
        if loop is None or manager is None or mcp_loop is None:
            return
        try:
            fut = _asyncio.run_coroutine_threadsafe(
                manager.build_tools_with_ctx(loop.tools.ctx), mcp_loop,
            )
            tools = fut.result(timeout=10)
        except Exception as exc:  # noqa: BLE001 - never block a rebuild on this
            self._append(f"[MCP re-attach failed: {exc}]\n", "system")
            return
        names = []
        for tool in tools:
            original_execute = tool.execute

            def _bridge(orig):
                async def bridged_execute(**kwargs):
                    f = _asyncio.run_coroutine_threadsafe(orig(**kwargs), mcp_loop)
                    return await _asyncio.wrap_future(f)
                return bridged_execute

            tool.execute = _bridge(original_execute)  # type: ignore[method-assign]
            loop.tools.register(tool)
            names.append(tool.name)
        if names:
            self._append(
                f"MCP ready: {len(names)} tool(s): {', '.join(names)}\n", "system",
            )

    def _scheduler_log(self, msg: str) -> None:
        """Append a line to ~/.nanocodex/scheduler.log (best-effort, UTF-8).

        Unattended runs deliberately do NOT touch the transcript; this file is
        their only record."""
        try:
            log_dir = Path.home() / ".nanocodex"
            log_dir.mkdir(parents=True, exist_ok=True)
            with (log_dir / "scheduler.log").open("a", encoding="utf-8") as fh:
                fh.write(msg.rstrip("\n") + "\n")
        except OSError:
            pass

    def _on_toggle_scheduler(self) -> None:
        """Flip the managed scheduler on/off and persist the choice."""
        self._scheduler_enabled = bool(self._sched_var.get())
        _save_scheduler_enabled(self._scheduler_enabled)
        if self._scheduler_enabled:
            self._append("[scheduler ON — due tasks run automatically.]\n", "system")
            # Restart the thread if it had stopped.
            self._scheduler_started = False
            self._autostart_scheduler()
        else:
            self._scheduler_stop.set()
            self._scheduler_started = False
            self._append(
                "[scheduler OFF — no scheduled tasks will fire until re-enabled.]\n",
                "system",
            )
        # Reflect the new on/off state in the sidebar panel right away.
        self._refresh_schedule_panel()

    # --- switching the active project ------------------------------------

    def _on_open_project(self) -> None:
        """Pick a folder, rebuild the loop in it, and reset the transcript."""
        if self._busy:
            self._append("\n[busy — wait for the current turn to finish]\n", "system")
            return
        from tkinter import filedialog

        chosen = filedialog.askdirectory(
            title="Open project folder",
            initialdir=str(self._workspace),
            parent=self.root,
        )
        if not chosen:
            return  # user cancelled
        self._workspace = Path(chosen).resolve()
        _save_last_workspace(self._workspace)  # remember for next launch
        self._start_new_session(f"Switched project to: {self._workspace}\n\n")

    def _on_new_session(self) -> None:
        """Start a clean conversation in the current workspace."""
        if self._busy:
            self._append("\n[busy — wait for the current turn to finish]\n", "system")
            return
        self._start_new_session(f"New session in: {self._workspace}\n\n")

    def _start_new_session(self, banner: str = "") -> None:
        """Mint a fresh session_id, clear the transcript, and rebuild the loop."""
        # A new session gets a fresh id so its snapshot/history is separate from
        # the previous conversation. Switching MODEL keeps the same id: same chat.
        try:
            from nanocodex.agent.session_index import new_session_id
            self._session_id = new_session_id()
        except Exception:  # noqa: BLE001 - id is a convenience, never critical
            pass
        # Fresh transcript for the new project (the old session log stays on disk).
        self.output.config(state=self._tk.NORMAL)
        self.output.delete("1.0", "end")
        self.output.config(state=self._tk.DISABLED)
        self._pending_seed = None
        # A new conversation starts a fresh cost tally.
        self._session_cost_usd = 0.0
        self._last_turn_cost_usd = None
        if banner:
            self._append(banner, "system")
        self._init_loop()

    # --- switching the model ---------------------------------------------

    def _on_pick_model(self) -> None:
        """Open a menu of available models; switching rebuilds the loop."""
        if self._busy:
            self._append("\n[busy — wait for the current turn to finish]\n", "system")
            return
        loop = self._loop
        cfg = getattr(loop, "_cfg", None) if loop else None
        models = list(getattr(cfg, "available_models", []) or [])
        current = getattr(cfg, "model", None)
        if not models:
            self._append("[no models configured — set NANOCODEX_MODELS]\n", "system")
            return
        tk = self._tk
        menu = tk.Menu(self.root, tearoff=0, bg=self._palette["panel"],
                       fg=self._palette["fg"], activebackground=self._palette["accent"],
                       activeforeground=self._palette["accent_fg"])
        for name in models:
            label = ("● " if name == current else "   ") + name
            menu.add_command(label=label, command=lambda n=name: self._switch_model(n))
        # Drop the menu just under the model button.
        x = self.model_btn.winfo_rootx()
        y = self.model_btn.winfo_rooty() + self.model_btn.winfo_height()
        menu.tk_popup(x, y)

    def _switch_model(self, name: str) -> None:
        cfg = getattr(self._loop, "_cfg", None) if self._loop else None
        if cfg is not None and name == getattr(cfg, "model", None):
            return  # no change
        self._overrides["model"] = name
        self._append(f"Switched model to: {name}\n\n", "system")
        self._init_loop()  # rebuilds the loop with the new model

    # --- MCP plugin manager (CRUD over mcp.toml; takes effect next launch) ---

    def _refresh_plugin_list(self) -> None:
        """Redraw the server rows in the plugin manager from mcp.toml."""
        from nanocodex.tools.mcp_store import McpStore
        frame = getattr(self, "_plugin_list_frame", None)
        if frame is None:
            return
        tk = self._tk
        P = self._palette
        for child in frame.winfo_children():
            child.destroy()

        try:
            servers = McpStore().list()
        except Exception as exc:  # noqa: BLE001
            tk.Label(frame, text=f"Could not read mcp.toml: {exc}", anchor="w",
                     bg=P["bg"], fg=P["err"], font=("Segoe UI", 9)).pack(
                         side=tk.TOP, fill=tk.X)
            return
        if not servers:
            tk.Label(frame, text="No MCP servers configured yet.", anchor="w",
                     bg=P["bg"], fg=P["muted"], font=("Segoe UI", 10)).pack(
                         side=tk.TOP, fill=tk.X, pady=8)
            return

        for s in servers:
            row = tk.Frame(frame, bg=P["panel"])
            row.pack(side=tk.TOP, fill=tk.X, pady=3)
            state = "on" if s.enabled else "off"
            dot = P["ok"] if s.enabled else P["muted"]
            tk.Label(row, text="●", bg=P["panel"], fg=dot,
                     font=("Segoe UI", 10)).pack(side=tk.LEFT, padx=(8, 6), pady=6)
            desc = f"{s.name}  [{state}]\n{s.command} {' '.join(s.args)}".rstrip()
            tk.Label(row, text=desc, anchor="w", justify="left", bg=P["panel"],
                     fg=P["fg"], font=("Cascadia Code", 9)).pack(
                         side=tk.LEFT, fill=tk.X, expand=True, pady=4)

            def _remove(name=s.name) -> None:
                from nanocodex.tools.mcp_store import McpStore
                McpStore().remove(name)
                self._refresh_plugin_list()

            def _toggle(name=s.name, enabled=s.enabled) -> None:
                from nanocodex.tools.mcp_store import McpStore
                McpStore().set_enabled(name, not enabled)
                self._refresh_plugin_list()

            tk.Button(row, text="Remove", command=_remove, bg=P["panel"],
                      fg=P["err"], activebackground=P["border"],
                      activeforeground=P["err"], relief="flat", bd=0, padx=10,
                      pady=4, font=("Segoe UI", 9), cursor="hand2",
                      highlightthickness=0).pack(side=tk.RIGHT, padx=(0, 8))
            tk.Button(row, text=("Disable" if s.enabled else "Enable"),
                      command=_toggle, bg=P["panel"], fg=P["fg"],
                      activebackground=P["border"], activeforeground=P["fg"],
                      relief="flat", bd=0, padx=10, pady=4, font=("Segoe UI", 9),
                      cursor="hand2", highlightthickness=0).pack(
                          side=tk.RIGHT, padx=(0, 6))

    # --- settings window (Codex-style: left nav + right section) ---------

    def _open_settings(self) -> None:
        """Codex-style settings window: a left nav list switches right sections.

        Folds the old standalone Settings dialog AND the MCP plugin manager into
        one window with four sections (General / Config / MCP servers / Desktop),
        matching what nanocodex actually configures — no empty Codex stubs.

        Single-instance (reuses self._settings_dlg). NOTE on the Tk gotcha that
        bit us before: tuple pady/padx are geometry args for .pack()/.grid()
        ONLY — putting one in a widget constructor raises TclError and yields a
        blank window. Keep tuple pads on .pack()/.grid() throughout.
        """
        tk = self._tk
        P = self._palette

        prev = getattr(self, "_settings_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:  # noqa: BLE001
                pass

        dlg = tk.Toplevel(self.root, bg=P["bg"])
        self._settings_dlg = dlg
        dlg.title("Settings")
        dlg.resizable(True, True)
        dlg.geometry("720x540")
        dlg.minsize(560, 400)

        # Horizontal split: fixed-width nav on the left, content fills the rest.
        nav = tk.Frame(dlg, bg=P["panel"], width=168)
        nav.pack(side=tk.LEFT, fill=tk.Y)
        nav.pack_propagate(False)
        tk.Frame(dlg, bg=P["border"], width=1).pack(side=tk.LEFT, fill=tk.Y)
        content = tk.Frame(dlg, bg=P["bg"])
        content.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        self._settings_content = content

        tk.Label(nav, text="Settings", anchor="w", bg=P["panel"], fg=P["fg"],
                 font=("Segoe UI", 11, "bold"), padx=14).pack(
                     side=tk.TOP, fill=tk.X, pady=(14, 8))

        # One flat nav button per section; clicking re-renders the content frame
        # and re-highlights. Buttons are kept so we can repaint their colors.
        self._settings_nav_btns = {}
        for name in _settings_sections():
            b = tk.Button(
                nav, text=name, command=lambda n=name: self._settings_show_section(n),
                anchor="w", bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                activeforeground=P["fg"], relief="flat", bd=0, padx=14, pady=8,
                font=("Segoe UI", 10), cursor="hand2", highlightthickness=0,
            )
            b.pack(side=tk.TOP, fill=tk.X)
            self._settings_nav_btns[name] = b

        # Show the first section by default.
        self._settings_show_section(_settings_sections()[0])

    def _settings_show_section(self, name: str) -> None:
        """Repaint the content frame with *name*'s section; re-highlight nav."""
        P = self._palette
        content = getattr(self, "_settings_content", None)
        if content is None:
            return
        self._settings_section = name
        # Re-highlight nav: active row gets the accent, others the panel bg.
        for nm, btn in getattr(self, "_settings_nav_btns", {}).items():
            active = nm == name
            btn.config(
                bg=P["accent"] if active else P["panel"],
                fg=P["accent_fg"] if active else P["fg"],
                activebackground=P["accent"] if active else P["border"],
                activeforeground=P["accent_fg"] if active else P["fg"],
            )
        for child in content.winfo_children():
            child.destroy()
        builder = {
            "General": self._settings_section_general,
            "Config": self._settings_section_config,
            "MCP servers": self._settings_section_mcp,
            "Marketplace": self._settings_section_marketplace,
            "Scheduled tasks": self._settings_section_schedule,
            "Desktop": self._settings_section_desktop,
        }.get(name)
        if builder is not None:
            builder(content)

    def _settings_section_header(self, parent, title: str, sub: str) -> None:
        """Shared title + subtitle for a settings section (tuple pads on pack)."""
        tk = self._tk
        P = self._palette
        tk.Label(parent, text=title, anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 12, "bold"), padx=18).pack(
                     side=tk.TOP, fill=tk.X, pady=(16, 2))
        if sub:
            tk.Label(parent, text=sub, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9), padx=18, wraplength=480,
                     justify="left").pack(side=tk.TOP, fill=tk.X, pady=(0, 10))

    def _settings_section_general(self, parent) -> None:
        """Read-only workspace + model overview (model is changed in Config)."""
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "General",
            "Current workspace and model. Change the model or other settings "
            "in the Config section.")
        cfg = getattr(self._loop, "_cfg", None) if self._loop else None
        info = cfg.redacted() if cfg is not None else {}
        grid = tk.Frame(parent, bg=P["bg"])
        grid.pack(side=tk.TOP, fill=tk.X, padx=18)
        grid.columnconfigure(1, weight=1)
        rows = [
            ("workspace", info.get("workspace", "(unknown)")),
            ("model", info.get("model", "(unset)")),
            ("sandbox", info.get("sandbox_mode", "(unset)")),
            ("approval", info.get("approval_policy", "(unset)")),
            ("reasoning", info.get("reasoning_effort", "(unset)")),
        ]
        for r, (k, v) in enumerate(rows):
            tk.Label(grid, text=k, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=r, column=0, sticky="w",
                                                padx=(0, 12), pady=3)
            tk.Label(grid, text=str(v), anchor="w", bg=P["bg"], fg=P["fg"],
                     font=("Cascadia Code", 10)).grid(row=r, column=1, sticky="w",
                                                      pady=3)

        # --- Appearance: theme switcher -----------------------------------
        # The whole UI reads self._palette, so switching the theme + rebuilding
        # the widgets re-colors everything live. 'light' is the Codex-style
        # default (white canvas); 'dark' is the original GitHub-dark look.
        tk.Label(parent, text="Appearance", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 12, "bold"), padx=18).pack(
                     side=tk.TOP, fill=tk.X, pady=(20, 2))
        tk.Label(parent, text="Theme applies immediately (the transcript is kept).",
                 anchor="w", bg=P["bg"], fg=P["muted"], font=("Segoe UI", 9),
                 padx=18).pack(side=tk.TOP, fill=tk.X, pady=(0, 8))
        trow = tk.Frame(parent, bg=P["bg"])
        trow.pack(side=tk.TOP, fill=tk.X, padx=18)
        tk.Label(trow, text="theme", anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9)).pack(side=tk.LEFT, padx=(0, 12))
        theme_var = tk.StringVar(value=self._theme)
        om = tk.OptionMenu(trow, theme_var, *VALID_THEMES,
                           command=lambda t: self._on_pick_theme(t))
        om.config(bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                  activeforeground=P["fg"], relief="flat", bd=0,
                  highlightthickness=0, font=("Segoe UI", 9), anchor="w")
        om["menu"].config(bg=P["panel"], fg=P["fg"],
                          activebackground=P["accent"],
                          activeforeground=P["accent_fg"])
        om.pack(side=tk.LEFT)

    def _on_pick_theme(self, theme: str) -> None:
        """Switch the UI theme: persist it, rebuild every widget, restore the
        transcript text, and reopen Settings on this section.

        Rebuilding is how the ~300 ``P[...]`` lookups re-color at once — we tear
        down the root's children and re-run _build_widgets() with the new
        palette, then replay the saved transcript so the conversation isn't lost.
        """
        theme = theme if theme in VALID_THEMES else "light"
        if theme == self._theme:
            return
        tk = self._tk
        # Snapshot the transcript so the rebuild doesn't wipe the conversation.
        try:
            saved = self.output.get("1.0", "end-1c")
        except Exception:
            saved = ""
        self._theme = theme
        _save_theme(theme)

        # Close the Settings window (its widgets belong to the old palette).
        prev = getattr(self, "_settings_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:
                pass
            self._settings_dlg = None

        # Tear down and rebuild the main UI with the new palette.
        for child in list(self.root.winfo_children()):
            try:
                child.destroy()
            except Exception:
                pass
        self._build_widgets()

        # Replay the saved transcript text (plain; tag coloring resumes for new
        # output). Best-effort — a failure here must not break the session.
        if saved:
            try:
                self.output.config(state=tk.NORMAL)
                self.output.insert("end", saved)
                self.output.config(state=tk.DISABLED)
                self.output.see("end")
            except Exception:
                pass

        # Repaint dynamic bits that _build_widgets leaves blank (model label,
        # context usage, session list, scheduled panel).
        for fn in ("_refresh_session_list", "_update_context_usage",
                   "_refresh_model_label", "_refresh_schedule_panel"):
            m = getattr(self, fn, None)
            if callable(m):
                try:
                    m()
                except Exception:
                    pass

    def _settings_section_config(self, parent) -> None:
        """Editable config: API key / base URL / model / sandbox / approval /
        reasoning, persisted to ~/.nanocodex/config.toml and applied via rebuild.
        """
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "Config",
            "Saved to ~/.nanocodex/config.toml (separate from ~/.deepseek's). "
            "Environment variables and CLI flags still override these. Saving "
            "applies immediately by rebuilding the session.")

        cfg = getattr(self._loop, "_cfg", None) if self._loop else None
        info = cfg.redacted() if cfg is not None else {}
        cur_masked = info.get("api_key", "(unset)")
        cur_base = info.get("base_url", "")
        cur_model = info.get("model", "")
        cur_sandbox = info.get("sandbox_mode", "workspace-write")
        cur_approval = info.get("approval_policy", "on-request")
        cur_reasoning = info.get("reasoning_effort", "auto")
        cur_vl_base = info.get("vl_base_url", "")
        cur_vl_masked = info.get("vl_api_key", "(unset)")
        cur_vl_model = info.get("vl_model", "")
        cur_ark_masked = info.get("ark_api_key", "(unset)")

        form = tk.Frame(parent, bg=P["bg"])
        form.pack(side=tk.TOP, fill=tk.X, padx=18)
        form.columnconfigure(1, weight=1)
        row = [0]  # mutable counter so the helpers can share it

        def _label(text: str) -> None:
            tk.Label(form, text=text, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=row[0], column=0, sticky="w",
                                                padx=(0, 10), pady=3)

        def _entry(*, show: str | None = None, prefill: str = "") -> "Any":
            e = tk.Entry(form, bg=P["panel"], fg=P["fg"], relief="flat",
                         insertbackground=P["fg"], font=("Cascadia Code", 10))
            if show:
                e.config(show=show)
            if prefill:
                e.insert(0, prefill)
            e.grid(row=row[0], column=1, sticky="we", pady=3)
            return e

        def _option(choices: tuple, current: str) -> "Any":
            var = tk.StringVar(value=current if current in choices else choices[0])
            om = tk.OptionMenu(form, var, *choices)
            om.config(bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                      activeforeground=P["fg"], relief="flat", bd=0,
                      highlightthickness=0, font=("Segoe UI", 9), anchor="w")
            om["menu"].config(bg=P["panel"], fg=P["fg"],
                              activebackground=P["accent"],
                              activeforeground=P["accent_fg"])
            om.grid(row=row[0], column=1, sticky="we", pady=3)
            return var

        from nanocodex.config import (
            VALID_SANDBOX_MODES, VALID_APPROVAL_POLICIES,
        )

        # current key (masked, read-only) — never echo the full secret.
        _label("current key")
        tk.Label(form, text=cur_masked, anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Cascadia Code", 10)).grid(row=row[0], column=1,
                                                  sticky="w", pady=3)
        row[0] += 1
        _label("new API key"); key_e = _entry(show="*"); row[0] += 1
        _label("base URL"); base_e = _entry(prefill=cur_base); row[0] += 1
        _label("model"); model_e = _entry(prefill=cur_model); row[0] += 1
        _label("sandbox"); sandbox_var = _option(VALID_SANDBOX_MODES, cur_sandbox); row[0] += 1
        _label("approval"); approval_var = _option(VALID_APPROVAL_POLICIES, cur_approval); row[0] += 1
        _label("reasoning"); reasoning_var = _option(_REASONING_CHOICES, cur_reasoning); row[0] += 1

        # Vision (VL) backend: image-bearing turns route here (e.g. DashScope
        # Qwen-VL) while text/coding stays on the main model. Leave VL model
        # blank to disable routing. base URL / key fall back to the main ones
        # when only the VL model is set (same vendor).
        tk.Label(form, text="— Vision (VL) backend for image turns —",
                 anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9, "italic")).grid(
                     row=row[0], column=0, columnspan=2, sticky="w", pady=(12, 2))
        row[0] += 1
        _label("VL current key")
        tk.Label(form, text=cur_vl_masked, anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Cascadia Code", 10)).grid(row=row[0], column=1,
                                                  sticky="w", pady=3)
        row[0] += 1
        _label("VL new key"); vl_key_e = _entry(show="*"); row[0] += 1
        _label("VL base URL"); vl_base_e = _entry(prefill=cur_vl_base); row[0] += 1
        _label("VL model"); vl_model_e = _entry(prefill=cur_vl_model); row[0] += 1

        # Volcengine ARK key for Seedance video rendering (storyboard 出片).
        # Only used when actually rendering; planning/preview never touches it.
        tk.Label(form, text="— Seedance (ARK) key for storyboard 出片 —",
                 anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9, "italic")).grid(
                     row=row[0], column=0, columnspan=2, sticky="w", pady=(12, 2))
        row[0] += 1
        _label("ARK current key")
        tk.Label(form, text=cur_ark_masked, anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Cascadia Code", 10)).grid(row=row[0], column=1,
                                                  sticky="w", pady=3)
        row[0] += 1
        _label("ARK new key"); ark_key_e = _entry(show="*"); row[0] += 1

        status = tk.Label(form, text="", anchor="w", bg=P["bg"], fg=P["err"],
                          font=("Segoe UI", 9), wraplength=480, justify="left")
        status.grid(row=row[0] + 1, column=0, columnspan=2, sticky="we",
                    pady=(8, 0))

        def _do_save() -> None:
            from nanocodex.config import write_nanocodex_config
            # Rebuilding the loop would yank a running turn out from under the
            # worker. Refuse while busy (mirrors the model switcher).
            if self._busy:
                status.config(text="Busy — finish the current turn first.",
                              fg=P["err"])
                return
            updates = _collect_settings_updates(
                api_key=key_e.get().strip(),
                base_url=base_e.get().strip(),
                model=model_e.get().strip(),
                sandbox_mode=sandbox_var.get().strip(),
                approval_policy=approval_var.get().strip(),
                reasoning_effort=reasoning_var.get().strip(),
                vl_base_url=vl_base_e.get().strip(),
                vl_api_key=vl_key_e.get().strip(),
                vl_model=vl_model_e.get().strip(),
                ark_api_key=ark_key_e.get().strip(),
            )
            if not updates:
                status.config(text="Nothing to save.", fg=P["muted"])
                return
            try:
                write_nanocodex_config(updates)
            except OSError as exc:
                status.config(text=f"Could not write config: {exc}", fg=P["err"])
                return
            # A saved model must win over any in-session override, else the old
            # override would mask it on rebuild.
            if "model" in updates:
                self._overrides["model"] = updates["model"]
            self._append("Settings saved to ~/.nanocodex/config.toml.\n\n", "system")
            dlg = getattr(self, "_settings_dlg", None)
            if dlg is not None:
                try:
                    dlg.destroy()
                except Exception:  # noqa: BLE001
                    pass
            self._init_loop()

        tk.Button(
            form, text="Save", command=_do_save, bg=P["accent"],
            fg=P["accent_fg"], activebackground=P["accent"],
            activeforeground=P["accent_fg"], relief="flat", bd=0, padx=14,
            pady=4, font=("Segoe UI", 9, "bold"), cursor="hand2",
            highlightthickness=0).grid(row=row[0], column=1, sticky="e",
                                       pady=(10, 0))

    def _settings_section_mcp(self, parent) -> None:
        """MCP server CRUD, folded in from the old plugin manager.

        Reuses _refresh_plugin_list (it renders into self._plugin_list_frame).
        Edits persist immediately but only connect on the NEXT launch — the live
        MCP stdio session is not hot-reloaded.
        """
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "MCP servers",
            "Tools run OUTSIDE the sandbox — only add servers you trust. "
            "Changes take effect on the next launch (no hot-reload).")

        # Scrollable list of existing servers.
        body = tk.Frame(parent, bg=P["bg"])
        body.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=18)
        self._plugin_list_frame = body
        self._refresh_plugin_list()

        # --- add-server form -------------------------------------------------
        tk.Frame(parent, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X,
                                                         pady=8, padx=18)
        form = tk.Frame(parent, bg=P["bg"])
        form.pack(side=tk.TOP, fill=tk.X, padx=18, pady=(0, 14))
        form.columnconfigure(1, weight=1)
        tk.Label(form, text="Add server", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 10, "bold")).grid(row=0, column=0,
                                                     columnspan=2, sticky="w",
                                                     pady=(0, 4))

        def _row(label: str, r: int) -> "Any":
            tk.Label(form, text=label, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=r, column=0, sticky="w",
                                                padx=(0, 8))
            e = tk.Entry(form, bg=P["panel"], fg=P["fg"], relief="flat",
                         insertbackground=P["fg"], font=("Cascadia Code", 10))
            e.grid(row=r, column=1, sticky="we", pady=2)
            return e

        name_e = _row("name", 1)
        cmd_e = _row("command", 2)
        args_e = _row("args (space-sep)", 3)
        env_e = _row("env (K=V, comma-sep)", 4)

        status = tk.Label(form, text="", anchor="w", bg=P["bg"], fg=P["err"],
                          font=("Segoe UI", 9), wraplength=480, justify="left")
        status.grid(row=6, column=0, columnspan=2, sticky="we", pady=(4, 0))

        def _do_add() -> None:
            from nanocodex.tools.mcp_store import McpStore
            name = name_e.get().strip()
            command = cmd_e.get().strip()
            args = args_e.get().split()
            env: dict[str, str] = {}
            for pair in env_e.get().split(","):
                pair = pair.strip()
                if not pair or "=" not in pair:
                    continue
                k, _, v = pair.partition("=")
                env[k.strip()] = v.strip()
            try:
                McpStore().add(name, command, args, env)
            except (ValueError, OSError) as exc:
                status.config(text=f"Error: {exc}", fg=P["err"])
                return
            status.config(text=f"Added {name!r}. Restart to connect it.", fg=P["ok"])
            for e in (name_e, cmd_e, args_e, env_e):
                e.delete(0, "end")
            self._refresh_plugin_list()

        tk.Button(
            form, text="Add", command=_do_add, bg=P["accent"], fg=P["accent_fg"],
            activebackground=P["accent"], activeforeground=P["accent_fg"],
            relief="flat", bd=0, padx=14, pady=4, font=("Segoe UI", 9, "bold"),
            cursor="hand2", highlightthickness=0).grid(row=5, column=1,
                                                       sticky="e", pady=(6, 0))

    # --- marketplace section --------------------------------------------

    def _settings_section_marketplace(self, parent) -> None:
        """Browse + one-click install MCP servers from a built-in catalog and
        (optionally) a remote URL.

        Both sources install through the SAME McpStore the "MCP servers" section
        uses, so an installed server appears there too and connects on the next
        launch (no hot-reload). The remote catalog is only fetched when the user
        clicks Refresh — opening this page makes no network call.
        """
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "Marketplace",
            "One-click install MCP servers. Tools run OUTSIDE the sandbox — only "
            "install servers you trust. Takes effect on the next launch.")

        # --- built-in catalog (no network) ----------------------------------
        tk.Label(parent, text="Built-in", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 10, "bold")).pack(side=tk.TOP, fill=tk.X,
                                                     padx=18, pady=(4, 2))
        local = tk.Frame(parent, bg=P["bg"])
        local.pack(side=tk.TOP, fill=tk.X, padx=18)
        self._mkt_local_frame = local

        # --- remote catalog (fetched on demand) -----------------------------
        tk.Frame(parent, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X,
                                                         pady=8, padx=18)
        rhead = tk.Frame(parent, bg=P["bg"])
        rhead.pack(side=tk.TOP, fill=tk.X, padx=18)
        tk.Label(rhead, text="Remote", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 10, "bold")).pack(side=tk.LEFT)

        from nanocodex.tools.marketplace import MARKETPLACE_URL_ENV, marketplace_url
        url = marketplace_url()
        if url:
            refresh = tk.Button(
                rhead, text="Refresh", command=self._on_marketplace_refresh,
                bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                activeforeground=P["fg"], relief="flat", bd=0, padx=10, pady=2,
                font=("Segoe UI", 9), cursor="hand2", highlightthickness=0)
            refresh.pack(side=tk.RIGHT)
            self._mkt_refresh_btn = refresh

        self._mkt_status = tk.Label(
            parent, text="", anchor="w", bg=P["bg"], fg=P["muted"],
            font=("Segoe UI", 9), wraplength=480, justify="left")
        self._mkt_status.pack(side=tk.TOP, fill=tk.X, padx=18, pady=(2, 0))

        remote = tk.Frame(parent, bg=P["bg"])
        remote.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=18)
        self._mkt_remote_frame = remote

        self._refresh_marketplace_local()
        if not url:
            self._mkt_status.config(
                text=(f"No remote source configured. Set {MARKETPLACE_URL_ENV} "
                      "to a catalog URL and reopen Settings."),
                fg=P["muted"])

    def _installed_server_names(self) -> set[str]:
        """Names already in mcp.toml (so the marketplace can mark them installed)."""
        from nanocodex.tools.mcp_store import McpStore
        try:
            return {s.name for s in McpStore().list()}
        except Exception:  # noqa: BLE001
            return set()

    def _render_catalog_row(self, frame, entry, installed: set[str]) -> None:
        """Draw one catalog entry row with name/source/description + Install."""
        tk = self._tk
        P = self._palette
        row = tk.Frame(frame, bg=P["panel"])
        row.pack(side=tk.TOP, fill=tk.X, pady=3)
        is_installed = entry.name in installed
        tag = "installed" if is_installed else entry.source
        desc = f"{entry.name}  [{tag}]"
        if entry.description:
            desc += f"\n{entry.description}"
        tk.Label(row, text=desc, anchor="w", justify="left", bg=P["panel"],
                 fg=P["fg"], font=("Cascadia Code", 9), wraplength=360).pack(
                     side=tk.LEFT, fill=tk.X, expand=True, padx=(8, 6), pady=4)

        if is_installed:
            tk.Label(row, text="installed", bg=P["panel"], fg=P["muted"],
                     font=("Segoe UI", 9)).pack(side=tk.RIGHT, padx=(0, 10))
            return

        def _install(e=entry) -> None:
            self._install_marketplace_entry(e)

        tk.Button(row, text="Install", command=_install, bg=P["accent"],
                  fg=P["accent_fg"], activebackground=P["accent"],
                  activeforeground=P["accent_fg"], relief="flat", bd=0, padx=12,
                  pady=4, font=("Segoe UI", 9, "bold"), cursor="hand2",
                  highlightthickness=0).pack(side=tk.RIGHT, padx=(0, 8))

    def _refresh_marketplace_local(self) -> None:
        """Redraw the built-in catalog rows."""
        from nanocodex.tools.marketplace import BUILTIN_CATALOG
        frame = getattr(self, "_mkt_local_frame", None)
        if frame is None:
            return
        for child in frame.winfo_children():
            child.destroy()
        installed = self._installed_server_names()
        for entry in BUILTIN_CATALOG:
            self._render_catalog_row(frame, entry, installed)

    def _refresh_marketplace_remote(self, entries) -> None:
        """Redraw the remote catalog rows from a fetched entry list."""
        tk = self._tk
        P = self._palette
        frame = getattr(self, "_mkt_remote_frame", None)
        if frame is None:
            return
        for child in frame.winfo_children():
            child.destroy()
        if not entries:
            tk.Label(frame, text="No servers in the remote catalog.", anchor="w",
                     bg=P["bg"], fg=P["muted"], font=("Segoe UI", 10)).pack(
                         side=tk.TOP, fill=tk.X, pady=8)
            return
        installed = self._installed_server_names()
        for entry in entries:
            self._render_catalog_row(frame, entry, installed)

    def _on_marketplace_refresh(self) -> None:
        """Fetch the remote catalog in a background thread (never blocks the UI)."""
        import threading

        from nanocodex.tools.marketplace import marketplace_url
        url = marketplace_url()
        if not url:
            return
        if getattr(self, "_mkt_fetching", False):
            return
        self._mkt_fetching = True
        self._mkt_status.config(text=f"Fetching {url} ...", fg=self._palette["muted"])
        btn = getattr(self, "_mkt_refresh_btn", None)
        if btn is not None:
            btn.config(state="disabled")
        threading.Thread(target=self._run_marketplace_fetch, args=(url,),
                         daemon=True).start()

    def _run_marketplace_fetch(self, url: str) -> None:
        """Worker: fetch+parse remote catalog, hand the result back to the main
        thread via root.after (Tk-safe). Errors are reported, never crash."""
        from nanocodex.tools.marketplace import fetch_remote_catalog
        try:
            entries = fetch_remote_catalog(url)
            self.root.after(0, lambda: self._marketplace_fetch_done(entries, None))
        except Exception as exc:  # noqa: BLE001
            msg = f"{type(exc).__name__}: {exc}"
            self.root.after(0, lambda: self._marketplace_fetch_done(None, msg))

    def _marketplace_fetch_done(self, entries, error) -> None:
        """Main-thread callback after a remote fetch finishes."""
        P = self._palette
        self._mkt_fetching = False
        btn = getattr(self, "_mkt_refresh_btn", None)
        if btn is not None:
            try:
                btn.config(state="normal")
            except Exception:  # noqa: BLE001 - window may be closed
                pass
        status = getattr(self, "_mkt_status", None)
        if status is None or not status.winfo_exists():
            return
        if error is not None:
            status.config(text=f"Fetch failed: {error}", fg=P["err"])
            return
        status.config(text=f"Loaded {len(entries)} server(s) from remote catalog.",
                      fg=P["ok"])
        self._refresh_marketplace_remote(entries)

    def _install_marketplace_entry(self, entry) -> None:
        """Install a catalog entry. If it needs a path or env values, prompt for
        them in a small modal first; otherwise install immediately."""
        if entry.path_arg_index is not None or entry.env_keys:
            self._prompt_marketplace_install(entry)
            return
        self._do_marketplace_install(entry, path_value=None, env_values={})

    def _prompt_marketplace_install(self, entry) -> None:
        """Modal collecting the machine-specific path and/or env values an entry
        needs before install. Uses Entry widgets (env values masked)."""
        tk = self._tk
        P = self._palette
        dlg = tk.Toplevel(self.root, bg=P["bg"])
        dlg.title(f"Install {entry.name}")
        dlg.transient(self.root)
        dlg.geometry("520x300")
        tk.Label(dlg, text=f"Install {entry.name}", anchor="w", bg=P["bg"],
                 fg=P["fg"], font=("Segoe UI", 11, "bold")).pack(
                     side=tk.TOP, fill=tk.X, padx=16, pady=(14, 2))
        form = tk.Frame(dlg, bg=P["bg"])
        form.pack(side=tk.TOP, fill=tk.X, padx=16, pady=(6, 0))
        form.columnconfigure(1, weight=1)

        path_e = None
        r = 0
        if entry.path_arg_index is not None:
            tk.Label(form, text=(entry.path_label or "path"), anchor="w",
                     bg=P["bg"], fg=P["muted"], font=("Segoe UI", 9),
                     wraplength=480, justify="left").grid(
                         row=r, column=0, columnspan=2, sticky="w")
            r += 1
            path_e = tk.Entry(form, bg=P["panel"], fg=P["fg"], relief="flat",
                              insertbackground=P["fg"], font=("Cascadia Code", 10))
            path_e.grid(row=r, column=0, columnspan=2, sticky="we", pady=(2, 8))
            r += 1

        env_entries: dict[str, "Any"] = {}
        for key in entry.env_keys:
            tk.Label(form, text=key, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=r, column=0, sticky="w",
                                                padx=(0, 8), pady=2)
            e = tk.Entry(form, bg=P["panel"], fg=P["fg"], relief="flat", show="*",
                         insertbackground=P["fg"], font=("Cascadia Code", 10))
            e.grid(row=r, column=1, sticky="we", pady=2)
            env_entries[key] = e
            r += 1

        status = tk.Label(form, text="", anchor="w", bg=P["bg"], fg=P["err"],
                          font=("Segoe UI", 9), wraplength=480, justify="left")
        status.grid(row=r, column=0, columnspan=2, sticky="we", pady=(6, 0))

        def _submit() -> None:
            path_value = path_e.get().strip() if path_e is not None else None
            env_values = {k: e.get() for k, e in env_entries.items()}
            ok, msg = self._do_marketplace_install(
                entry, path_value=path_value, env_values=env_values)
            if ok:
                dlg.destroy()
            else:
                status.config(text=msg, fg=P["err"])

        btns = tk.Frame(dlg, bg=P["bg"])
        btns.pack(side=tk.BOTTOM, fill=tk.X, padx=16, pady=12)
        tk.Button(btns, text="Install", command=_submit, bg=P["accent"],
                  fg=P["accent_fg"], activebackground=P["accent"],
                  activeforeground=P["accent_fg"], relief="flat", bd=0, padx=14,
                  pady=4, font=("Segoe UI", 9, "bold"), cursor="hand2",
                  highlightthickness=0).pack(side=tk.RIGHT)
        tk.Button(btns, text="Cancel", command=dlg.destroy, bg=P["panel"],
                  fg=P["fg"], activebackground=P["border"], activeforeground=P["fg"],
                  relief="flat", bd=0, padx=14, pady=4, font=("Segoe UI", 9),
                  cursor="hand2", highlightthickness=0).pack(side=tk.RIGHT,
                                                             padx=(0, 8))

    def _do_marketplace_install(self, entry, *, path_value, env_values) -> "tuple[bool, str]":
        """Funnel an install through marketplace.install_entry → McpStore.

        Returns (ok, message). On success refreshes both the marketplace rows and
        the MCP servers list (if that section's frame exists) so they stay in
        sync. Never raises into the Tk callback.
        """
        from nanocodex.tools.marketplace import install_entry
        try:
            install_entry(entry, env_values or {}, path_value=path_value)
        except (ValueError, OSError) as exc:
            return (False, f"Error: {exc}")
        # Keep both views consistent.
        self._refresh_marketplace_local()
        if getattr(self, "_plugin_list_frame", None) is not None:
            try:
                self._refresh_plugin_list()
            except Exception:  # noqa: BLE001
                pass
        status = getattr(self, "_mkt_status", None)
        if status is not None and status.winfo_exists():
            status.config(text=f"Installed {entry.name!r}. Restart to connect it.",
                          fg=self._palette["ok"])
        return (True, "")

    def _settings_section_schedule(self, parent) -> None:
        """Manual CRUD over scheduled tasks (the SAME ScheduleStore the model's
        manage_schedule tool and the CLI use). Lets the user add/enable/disable/
        remove tasks visually instead of only via conversation.

        Tasks only FIRE while the managed scheduler is on (top-bar "Scheduler"
        toggle) or `nanocodex schedule run` is running — adding one here just
        persists it; this page says so. allow_desktop is exposed with a security
        warning, matching the conversational tool's guardrail.
        """
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "Scheduled tasks",
            "Add/enable/disable/remove tasks that run a prompt automatically. "
            "They only fire while the top-bar Scheduler is ON (or "
            "`nanocodex schedule run` is running).")

        # Scrollable list of existing tasks (rendered by _refresh_schedule_mgr).
        body = tk.Frame(parent, bg=P["bg"])
        body.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=18)
        self._sched_mgr_frame = body
        self._refresh_schedule_mgr()

        # --- add-task form ---------------------------------------------------
        tk.Frame(parent, bg=P["border"], height=1).pack(side=tk.TOP, fill=tk.X,
                                                         pady=8, padx=18)
        form = tk.Frame(parent, bg=P["bg"])
        form.pack(side=tk.TOP, fill=tk.X, padx=18, pady=(0, 14))
        form.columnconfigure(1, weight=1)
        tk.Label(form, text="Add task", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 10, "bold")).grid(row=0, column=0,
                                                     columnspan=2, sticky="w",
                                                     pady=(0, 4))

        def _label(text: str, r: int) -> None:
            tk.Label(form, text=text, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=r, column=0, sticky="w",
                                                padx=(0, 8))

        def _entry(r: int) -> "Any":
            e = tk.Entry(form, bg=P["panel"], fg=P["fg"], relief="flat",
                         insertbackground=P["fg"], font=("Cascadia Code", 10))
            e.grid(row=r, column=1, sticky="we", pady=2)
            return e

        _label("prompt", 1); prompt_e = _entry(1)
        # kind selector drives which extra fields matter (once/interval/daily).
        _label("kind", 2)
        from nanocodex.agent.schedule import VALID_KINDS
        kind_var = tk.StringVar(value=VALID_KINDS[0])
        kind_om = tk.OptionMenu(form, kind_var, *VALID_KINDS)
        kind_om.config(bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                       activeforeground=P["fg"], relief="flat", bd=0,
                       highlightthickness=0, font=("Segoe UI", 9), anchor="w")
        kind_om["menu"].config(bg=P["panel"], fg=P["fg"],
                               activebackground=P["accent"],
                               activeforeground=P["accent_fg"])
        kind_om.grid(row=2, column=1, sticky="we", pady=2)

        # All recurrence fields shown together with hints on which kind uses
        # which — simpler and more transparent than hiding/showing rows, and the
        # store ignores the irrelevant ones per kind.
        _label("run_at (once/interval start)", 3); run_at_e = _entry(3)
        _label("every_seconds (interval)", 4); every_e = _entry(4)
        _label("at_hour (daily 0-23)", 5); hour_e = _entry(5)
        _label("at_minute (daily 0-59)", 6); minute_e = _entry(6)
        hour_e.insert(0, "9")
        minute_e.insert(0, "0")

        # allow_desktop: off by default; turning it on lets unattended runs drive
        # the desktop. Mirror the conversational tool's explicit warning.
        allow_var = tk.BooleanVar(value=False)
        allow_chk = tk.Checkbutton(
            form, text="allow_desktop (unattended desktop actions)",
            variable=allow_var, bg=P["bg"], fg=P["muted"],
            activebackground=P["bg"], activeforeground=P["fg"],
            selectcolor=P["panel"], relief="flat", bd=0, font=("Segoe UI", 9),
            cursor="hand2", highlightthickness=0, anchor="w")
        allow_chk.grid(row=7, column=0, columnspan=2, sticky="w", pady=(4, 0))

        status = tk.Label(form, text="", anchor="w", bg=P["bg"], fg=P["err"],
                          font=("Segoe UI", 9), wraplength=480, justify="left")
        status.grid(row=9, column=0, columnspan=2, sticky="we", pady=(4, 0))

        def _do_add() -> None:
            from nanocodex.agent.schedule import ScheduleStore
            kwargs = _collect_schedule_add(
                prompt=prompt_e.get(), kind=kind_var.get(),
                run_at=run_at_e.get(), every_seconds=every_e.get(),
                at_hour=hour_e.get(), at_minute=minute_e.get(),
                allow_desktop=bool(allow_var.get()),
            )
            if not kwargs["prompt"]:
                status.config(text="A prompt is required.", fg=P["err"])
                return
            try:
                task = ScheduleStore().add(**kwargs)
            except (ValueError, TypeError) as exc:
                status.config(text=f"Error: {exc}", fg=P["err"])
                return
            note = ""
            if kwargs["allow_desktop"]:
                note = " [allow_desktop ON — unattended desktop actions]"
            status.config(
                text=(f"Added {task.id} ({task.kind}); next: "
                      f"{task.next_run or '(immediate)'}.{note}"),
                fg=P["ok"])
            prompt_e.delete(0, "end")
            run_at_e.delete(0, "end")
            every_e.delete(0, "end")
            self._refresh_schedule_mgr()
            # The sidebar read-out should reflect the new task right away.
            self._refresh_schedule_panel()

        tk.Button(
            form, text="Add", command=_do_add, bg=P["accent"], fg=P["accent_fg"],
            activebackground=P["accent"], activeforeground=P["accent_fg"],
            relief="flat", bd=0, padx=14, pady=4, font=("Segoe UI", 9, "bold"),
            cursor="hand2", highlightthickness=0).grid(row=8, column=1,
                                                       sticky="e", pady=(6, 0))

    def _refresh_schedule_mgr(self) -> None:
        """Redraw the task rows in the Scheduled-tasks settings section.

        Each row shows the task's prompt + recurrence summary with Enable/Disable
        and Remove buttons, mutating the shared ScheduleStore. Best-effort: a
        read failure shows a message rather than crashing the settings window.
        """
        from nanocodex.agent.schedule import ScheduleStore
        frame = getattr(self, "_sched_mgr_frame", None)
        if frame is None:
            return
        tk = self._tk
        P = self._palette
        for child in frame.winfo_children():
            child.destroy()

        try:
            tasks = ScheduleStore().tasks
        except Exception as exc:  # noqa: BLE001
            tk.Label(frame, text=f"Could not read schedule.json: {exc}",
                     anchor="w", bg=P["bg"], fg=P["err"],
                     font=("Segoe UI", 9)).pack(side=tk.TOP, fill=tk.X)
            return
        if not tasks:
            tk.Label(frame, text="No scheduled tasks yet.", anchor="w",
                     bg=P["bg"], fg=P["muted"], font=("Segoe UI", 10)).pack(
                         side=tk.TOP, fill=tk.X, pady=8)
            return

        for t in tasks:
            row = tk.Frame(frame, bg=P["panel"])
            row.pack(side=tk.TOP, fill=tk.X, pady=3)
            state = "on" if t.enabled else "off"
            dot = P["ok"] if t.enabled else P["muted"]
            tk.Label(row, text="●", bg=P["panel"], fg=dot,
                     font=("Segoe UI", 10)).pack(side=tk.LEFT, padx=(8, 6),
                                                 pady=6)
            prompt_preview = (t.prompt[:60] + "…") if len(t.prompt) > 60 else t.prompt
            recur = _format_schedule_recurrence(
                kind=t.kind, every_seconds=t.every_seconds,
                at_hour=t.at_hour, at_minute=t.at_minute)
            extra = "  [desktop]" if getattr(t, "allow_desktop", False) else ""
            desc = (f"{t.id}  [{state}]  {recur}{extra}\n{prompt_preview}\n"
                    f"next: {t.next_run or '—'}  runs: {t.runs}").rstrip()
            tk.Label(row, text=desc, anchor="w", justify="left", bg=P["panel"],
                     fg=P["fg"], font=("Cascadia Code", 9)).pack(
                         side=tk.LEFT, fill=tk.X, expand=True, pady=4)

            def _remove(task_id=t.id) -> None:
                from nanocodex.agent.schedule import ScheduleStore
                ScheduleStore().remove(task_id)
                self._refresh_schedule_mgr()
                self._refresh_schedule_panel()

            def _toggle(task_id=t.id, enabled=t.enabled) -> None:
                from nanocodex.agent.schedule import ScheduleStore
                ScheduleStore().set_enabled(task_id, not enabled)
                self._refresh_schedule_mgr()
                self._refresh_schedule_panel()

            tk.Button(row, text="Remove", command=_remove, bg=P["panel"],
                      fg=P["err"], activebackground=P["border"],
                      activeforeground=P["err"], relief="flat", bd=0, padx=10,
                      pady=4, font=("Segoe UI", 9), cursor="hand2",
                      highlightthickness=0).pack(side=tk.RIGHT, padx=(0, 8))
            tk.Button(row, text=("Disable" if t.enabled else "Enable"),
                      command=_toggle, bg=P["panel"], fg=P["fg"],
                      activebackground=P["border"], activeforeground=P["fg"],
                      relief="flat", bd=0, padx=10, pady=4, font=("Segoe UI", 9),
                      cursor="hand2", highlightthickness=0).pack(
                          side=tk.RIGHT, padx=(0, 6))

    def _settings_section_desktop(self, parent) -> None:
        """Read-only mirror of desktop-control state (toggles live in the top bar).

        nanocodex's desktop control runs through MCP (windows-computer-use-mcp)
        under approval gating. The live switches stay in the top bar; this is a
        status view explaining the security model — no new persisted state.
        """
        tk = self._tk
        P = self._palette
        self._settings_section_header(
            parent, "Desktop",
            "Desktop control runs through MCP tools under approval gating. "
            "The live switches stay in the top bar; this is a status view.")
        grid = tk.Frame(parent, bg=P["bg"])
        grid.pack(side=tk.TOP, fill=tk.X, padx=18)
        grid.columnconfigure(1, weight=1)

        def _state_row(r: int, label: str, on: bool, note: str) -> None:
            tk.Label(grid, text=label, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 9)).grid(row=r, column=0, sticky="w",
                                                padx=(0, 12), pady=3)
            tk.Label(grid, text=("ON" if on else "OFF"), anchor="w", bg=P["bg"],
                     fg=(P["ok"] if on else P["muted"]),
                     font=("Cascadia Code", 10, "bold")).grid(
                         row=r, column=1, sticky="w", pady=3)
            tk.Label(grid, text=note, anchor="w", bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 8), wraplength=440, justify="left").grid(
                         row=r + 1, column=0, columnspan=2, sticky="w",
                         padx=(0, 0), pady=(0, 6))

        _state_row(0, "Auto-approve", self._auto_approve_on,
                   "When ON, in-sandbox writes and commands run without asking.")
        _state_row(2, "Scheduler", self._scheduler_enabled,
                   "When ON, due scheduled tasks marked allow_desktop drive the "
                   "desktop unattended.")
        _state_row(4, "Allow all desktop (session)", self._allow_all_mcp,
                   "Set after approving a desktop action with 'Allow all "
                   "desktop'; every later MCP tool runs without prompting this "
                   "session.")

    # --- approval bridge (worker thread -> main thread) ------------------

    async def _approve_via_ui(self, req: ApprovalRequest) -> bool:
        """Approver callback (runs on the worker loop). Blocks on the UI.

        Short-circuits without a dialog when the global auto-approve toggle is
        on, when the user previously chose "allow all desktop (this session)"
        and this is an MCP action, or when this exact command was marked
        "always allow this command".
        """
        if _approval_short_circuit(
            req.command,
            auto_approve_on=self._auto_approve_on,
            allow_all_mcp=self._allow_all_mcp,
            always_allow=self._always_allow,
        ):
            return True
        ui_req = _ApprovalRequestUI(req)
        self._ui_queue.put(_UiEvent("approval", ui_req))
        # Wait for the main thread to answer without blocking the event loop.
        await asyncio.to_thread(ui_req.event.wait)
        if ui_req.always:
            self._always_allow.add(req.command)
        # "Allow all desktop (this session)" — every later MCP action runs
        # without prompting, the Codex "approve for session" semantics.
        if ui_req.session_all:
            self._allow_all_mcp = True
        return ui_req.answer

    def _show_approval_dialog(self, ui_req: _ApprovalRequestUI) -> None:
        req = ui_req.request
        tk = self._tk
        P = self._palette
        dlg = tk.Toplevel(self.root, bg=P["bg"])
        dlg.title("Approval required")
        dlg.transient(self.root)
        dlg.grab_set()  # modal
        dlg.resizable(False, False)
        dlg.geometry("560x280")

        # Buttons are packed FIRST at the bottom so they always reserve their
        # space and can never be clipped by the body (same lesson as the main
        # window's bottom bars).
        btns = tk.Frame(dlg, bg=P["bg"])
        btns.pack(side="bottom", fill="x", padx=16, pady=(0, 14))

        def _decide(answer: bool, always: bool = False, session_all: bool = False) -> None:
            ui_req.answer = answer
            ui_req.always = always
            ui_req.session_all = session_all
            dlg.destroy()
            ui_req.event.set()  # unblock the worker thread

        def dlg_btn(text, command, *, accent=False, danger=False):
            if accent:
                bg, fg, abg = P["accent"], P["accent_fg"], P["accent"]
            elif danger:
                bg, fg, abg = P["panel"], P["err"], P["border"]
            else:
                bg, fg, abg = P["panel"], P["fg"], P["border"]
            return tk.Button(btns, text=text, command=command, bg=bg, fg=fg,
                             activebackground=abg, activeforeground=fg,
                             relief="flat", bd=0, padx=12, pady=6,
                             font=("Segoe UI", 9), cursor="hand2", highlightthickness=0)

        dlg_btn("Deny", lambda: _decide(False), danger=True).pack(side="right")
        dlg_btn("Allow", lambda: _decide(True), accent=True).pack(side="right", padx=(0, 8))
        # The "approve for session" button differs by action type:
        #  - MCP desktop actions: one click allows ALL later desktop/MCP steps,
        #    so a multi-step flow (focus→click→type→press to send a WeChat msg)
        #    doesn't prompt on every micro-action. This is the Codex semantics.
        #  - shell / apply_patch: keep the narrower "this exact command" memory.
        if _is_mcp_command(req.command):
            dlg_btn("Allow all desktop (session)",
                    lambda: _decide(True, session_all=True)).pack(side="right", padx=(0, 8))
        else:
            dlg_btn("Always allow this command",
                    lambda: _decide(True, always=True)).pack(side="right", padx=(0, 8))

        # Body uses a Text widget, NOT a Label: on this machine's Tk, Labels in a
        # transient Toplevel render blank (the "blank approval popup" bug —
        # documented in HANDOFF). The context-details popup already uses Text and
        # renders fine, so mirror it here.
        txt = tk.Text(dlg, wrap="word", bg=P["bg"], fg=P["fg"],
                      font=("Cascadia Code", 10), relief="flat", bd=0,
                      padx=16, pady=14, highlightthickness=0, spacing3=4)
        txt.pack(side="top", fill="both", expand=True)
        txt.tag_config("head", font=("Segoe UI", 12, "bold"), foreground=P["fg"])
        txt.tag_config("muted", foreground=P["muted"])
        txt.insert("end", "Approval required\n\n", "head")
        txt.insert("end", f"{req.command}\n\n")
        txt.insert("end", f"Dir: {req.cwd}\n", "muted")
        if req.reason:
            txt.insert("end", f"\nReason: {req.reason}\n", "muted")
        if req.escalated:
            # Neutral wording: escalation covers both an on-failure shell retry
            # AND an MCP tool that acts outside the sandbox, so don't claim
            # "ran sandboxed and failed" (wrong for the desktop/MCP case).
            txt.insert("end", "\n(escalated — requires elevated approval)\n", "muted")
        txt.config(state="disabled")

        # Closing the window counts as Deny (don't leave the worker blocked).
        dlg.protocol("WM_DELETE_WINDOW", lambda: _decide(False))
        dlg.lift()
        dlg.update()                         # force a draw on this Tk build

    # --- sending a turn --------------------------------------------------

    def _on_continue(self) -> None:
        """Resume an unfinished turn (hit step-limit / paused mid-plan) with one
        click, instead of making the user type 'continue'."""
        if self._busy or self._loop is None:
            return
        self.continue_btn.config(state=self._tk.DISABLED)
        self._append("\nyou › continue\n", "user")
        self._cancel_event.clear()
        self._set_busy(True)
        self._worker = threading.Thread(
            target=self._run_turn_thread, args=("continue",), daemon=True
        )
        self._worker.start()

    def _on_send(self, _event=None) -> str:
        # Codex-style: you can type the NEXT task while one is running. If idle,
        # the turn starts now; if busy, it joins a queue and runs automatically
        # when the current turn (and anything ahead of it) finishes.
        if self._loop is None:
            return "break"
        text = self.entry.get("1.0", "end").strip()
        if not text:
            return "break"
        self.entry.delete("1.0", "end")
        # `# something` is a quick-capture into user memory (DeepSeek-TUI-style):
        # append a timestamped bullet to ~/.nanocodex/memory.md without sending a
        # turn. A bare "#" with no text falls through as a normal message.
        if text.startswith("#") and text[1:].strip():
            self._quick_capture_memory(text[1:].strip())
            return "break"
        # `/storyboard` (alias `/sb`): drive the dedicated storyboard panel from
        # the composer. `/storyboard render` renders the panel's already-previewed
        # state; `/storyboard <story>` opens the panel, prefills the story and
        # auto-runs the preview; bare `/storyboard` just opens the panel.
        low = text.lower()
        if low in ("/storyboard", "/sb") or low.startswith(("/storyboard ", "/sb ")):
            rest = text.split(None, 1)[1].strip() if " " in text else ""
            self._handle_storyboard_command(rest)
            return "break"
        if self._busy:
            # Queue it: the running turn keeps going, this waits its turn.
            self._pending_inputs.append(text)
            self._append(
                f"\n[queued ▸ {text}]  ({len(self._pending_inputs)} waiting)\n",
                "system",
            )
            self._refresh_send_label()
        else:
            self._start_turn(text)
        return "break"  # stop the <Return> binding from also inserting a newline

    def _autogrow_entry(self, _event=None) -> None:
        """Grow/shrink the composer to fit its content, within [min, max] rows.

        Counts the displayed lines and clamps the Text height to that range; past
        the max the box scrolls internally. Bound to <KeyRelease> and
        <<Modified>> so typing, pasting, and programmatic prefills all resize it.
        The <<Modified>> virtual event latches a flag we must clear, or it stops
        firing. Best-effort: any Tk error is swallowed so it never breaks input.
        """
        entry = getattr(self, "entry", None)
        if entry is None:
            return
        try:
            # <<Modified>> only re-fires after the modified flag is reset.
            entry.edit_modified(False)
            # Number of display lines = row index of the last char.
            rows = int(entry.index("end-1c").split(".")[0])
            rows = max(self._entry_min_rows, min(self._entry_max_rows, rows))
            if rows != int(entry.cget("height")):
                entry.config(height=rows)
        except Exception:  # noqa: BLE001 - cosmetic, never break the composer
            pass

    def _quick_capture_memory(self, note: str) -> None:
        """Append `note` to user memory (the `# ...` composer shortcut).

        Best-effort and synchronous: writing one bullet to a local file is
        instant, so no worker thread. Never raises into the UI — a failed write
        just prints the error. The new note takes effect on the NEXT loop build
        (memory is injected at system-prompt time), so we say so.
        """
        from nanocodex.agent.memory_store import MemoryStore

        try:
            bullet = MemoryStore().append(note)
        except (ValueError, OSError) as exc:
            self._append(f"\n[memory not saved: {exc}]\n", "result_err")
            return
        self._append(f"\n[remembered ▸ {bullet}]\n", "system")

    # --- 📎 file attachments (ride the next send) --------------------------

    def _on_attach(self, _event=None) -> str:
        """📎 button: pick local files to attach to the NEXT message.

        Images become OpenAI multimodal blocks (only seen by a vision-capable
        model); other (text-like) files are read and inlined into the prompt.
        Attachments are collected here on the main thread and consumed when an
        idle turn starts (`_consume_attachments`), so picking files then hitting
        Send carries them along.
        """
        from tkinter import filedialog

        if self._loop is None:
            return "break"
        paths = filedialog.askopenfilenames(
            title="Attach files for the next message", parent=self.root,
        )
        if not paths:
            return "break"
        self._attached_files.extend(paths)
        self._refresh_attach_label()
        names = ", ".join(Path(p).name for p in paths)
        self._append(
            f"\n[attached ▸ {names}]  ({len(self._attached_files)} for next send)\n",
            "system",
        )
        return "break"

    def _refresh_attach_label(self) -> None:
        """Show the pending attachment count on the 📎 button (cosmetic)."""
        btn = getattr(self, "attach_btn", None)
        if btn is None:
            return
        n = len(self._attached_files)
        try:
            btn.config(text="📎" if n == 0 else f"📎 {n}")
        except Exception:  # noqa: BLE001 - label is cosmetic, never crash
            pass

    def _consume_attachments(self, text: str) -> "str | list[dict]":
        """Fold pending attachments into the message content, then clear them.

        Returns a plain string when there are no images (text-only / no files),
        or an OpenAI multimodal block list when image files are attached. Image
        files go through `build_user_content`; text-like files are read and
        inlined (size-capped) so they stay in the prompt the model sees. Always
        clears `_attached_files` (one-shot, like the user expects of a 📎).
        """
        files = self._attached_files
        self._attached_files = []
        self._refresh_attach_label()
        if not files:
            return text

        from nanocodex.agent.images import ImageError, build_user_content

        image_exts = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"}
        image_paths: list[str] = []
        inlined: list[str] = []
        for p in files:
            if Path(p).suffix.lower() in image_exts:
                image_paths.append(p)
                continue
            try:
                raw = Path(p).read_text(encoding="utf-8", errors="replace")
            except OSError as exc:
                self._append(f"\n[attach skipped {Path(p).name}: {exc}]\n", "result_err")
                continue
            if len(raw) > 50_000:  # keep one big paste from blowing the prompt
                raw = raw[:50_000] + "\n…[truncated]"
            inlined.append(f"\n\n----- file: {Path(p).name} -----\n{raw}")

        full_text = text + "".join(inlined)
        if not image_paths:
            return full_text
        try:
            return build_user_content(full_text, image_paths)
        except ImageError as exc:
            self._append(f"\n[image attach failed: {exc}]\n", "result_err")
            return full_text

    def _on_enhance(self, _event=None) -> str:
        """✨ button: rewrite the composer text into a clearer prompt.

        Reads the current input, kicks off a background rewrite (the model call
        must not block the UI), and on completion shows a PREVIEW dialog so the
        user chooses to use the rewrite, keep the original, or cancel — a rewrite
        never silently replaces their words. The input box is left untouched
        until the user picks in the dialog.
        """
        from nanocodex.agent.enhance_prompt import should_enhance

        if self._loop is None:
            return "break"
        if getattr(self, "_enhancing", False):
            return "break"  # a rewrite is already in flight; ignore repeat clicks
        text = self.entry.get("1.0", "end").strip()
        if not should_enhance(text):
            # Empty, or too long to be worth a structuring pass — say why, no call.
            if text:
                self._append("\n[enhance skipped: input too long to rewrite]\n", "system")
            return "break"
        self._enhancing = True
        self._refresh_enhance_label()
        self._append("\n[enhancing your prompt…]\n", "system")
        threading.Thread(
            target=self._run_enhance_thread, args=(text,), daemon=True,
        ).start()
        return "break"

    def _run_enhance_thread(self, text: str) -> None:
        """Daemon thread: one provider.chat to rewrite *text*; result to the queue.

        Mirrors _run_turn_thread's "own asyncio loop, post results via the UI
        queue" shape, but it's a single stateless call (no tools, no session, no
        desktop lock — enhancement never touches the desktop). The original text
        rides along so the main thread can fall back to it / show both.
        """
        from nanocodex.agent.enhance_prompt import build_enhance_messages, clean_enhanced

        try:
            provider = self._loop.provider
            messages = build_enhance_messages(text)
            resp = asyncio.run(provider.chat(messages))
            rewritten = clean_enhanced(getattr(resp, "content", ""), original=text)
            self._ui_queue.put(_UiEvent("enhance_result", (text, rewritten)))
        except ProviderError as exc:
            self._ui_queue.put(_UiEvent("enhance_error", f"Provider error: {exc}"))
        except Exception as exc:  # noqa: BLE001 - surfaced to the UI
            self._ui_queue.put(_UiEvent("enhance_error", f"{type(exc).__name__}: {exc}"))

    def _refresh_enhance_label(self) -> None:
        """Reflect the in-flight state on the ✨ button (cosmetic, never crashes)."""
        btn = getattr(self, "enhance_btn", None)
        if btn is None:
            return
        try:
            btn.config(text="…" if getattr(self, "_enhancing", False) else "✨")
        except Exception:  # noqa: BLE001
            pass

    def _show_enhance_dialog(self, original: str, rewritten: str) -> None:
        """Preview the rewrite; let the user use it, keep the original, or cancel.

        A rewrite NEVER silently replaces the user's words — they pick here.
        'Use rewrite' replaces the composer text; 'Use original' leaves it as
        typed; 'Cancel' is the same (closes without changing anything).
        """
        tk = self._tk
        P = self._palette

        # Single-instance: drop any prior preview so repeats don't stack.
        prev = getattr(self, "_enhance_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:  # noqa: BLE001
                pass

        dlg = tk.Toplevel(self.root, bg=P["bg"])
        self._enhance_dlg = dlg
        dlg.title("Enhance prompt")
        dlg.transient(self.root)
        dlg.resizable(True, True)
        dlg.geometry("620x460")

        # Buttons FIRST at the bottom so they always reserve space (same lesson
        # as the main window's bottom bars / the approval dialog).
        btns = tk.Frame(dlg, bg=P["bg"])
        btns.pack(side="bottom", fill="x", padx=16, pady=(0, 14))

        def _use(text: str | None) -> None:
            if text is not None:
                self.entry.delete("1.0", "end")
                self.entry.insert("1.0", text)
                self.entry.focus_set()
            dlg.destroy()

        def dlg_btn(label, command, *, accent=False):
            bg = P["accent"] if accent else P["panel"]
            fg = P["accent_fg"] if accent else P["fg"]
            return tk.Button(
                btns, text=label, command=command, bg=bg, fg=fg,
                activebackground=P["border"] if not accent else P["accent"],
                activeforeground=fg, relief="flat", bd=0, padx=14, pady=6,
                font=("Segoe UI", 9, "bold"), cursor="hand2", highlightthickness=0,
            )

        dlg_btn("Use rewrite", lambda: _use(rewritten), accent=True).pack(side="right")
        dlg_btn("Use original", lambda: _use(original)).pack(side="right", padx=(0, 8))
        dlg_btn("Cancel", lambda: _use(None)).pack(side="right", padx=(0, 8))

        # Body: original (muted) above, rewrite (normal) below, each scrollable.
        body = tk.Frame(dlg, bg=P["bg"])
        body.pack(side="top", fill="both", expand=True, padx=16, pady=14)

        tk.Label(body, text="Your input", anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9, "bold")).pack(side="top", fill="x")
        orig_box = tk.Text(body, height=4, wrap="word", bg=P["panel"], fg=P["muted"],
                           relief="flat", bd=0, padx=10, pady=8,
                           font=("Cascadia Code", 10), highlightthickness=0)
        orig_box.pack(side="top", fill="x", pady=(2, 10))
        orig_box.insert("1.0", original)
        orig_box.config(state="disabled")

        tk.Label(body, text="Rewritten", anchor="w", bg=P["bg"], fg=P["accent"],
                 font=("Segoe UI", 9, "bold")).pack(side="top", fill="x")
        new_box = tk.Text(body, wrap="word", bg=P["panel"], fg=P["fg"],
                          relief="flat", bd=0, padx=10, pady=8,
                          font=("Cascadia Code", 10), highlightthickness=0)
        new_box.pack(side="top", fill="both", expand=True, pady=(2, 0))
        new_box.insert("1.0", rewritten)
        new_box.config(state="disabled")

        dlg.bind("<Escape>", lambda e: _use(None))

    # --- A/B configuration comparison ------------------------------------

    # --- 分镜 storyboard panel (chapters + shots preview, gated render) -----

    def _flat_btn(self, parent, text, command, *, accent=False):
        """Class-level twin of the _build_widgets-local flat_btn closure, so
        dialogs built outside _build_widgets (storyboard panel) get the same
        flat, palette-colored button without re-defining it each time."""
        tk = self._tk
        P = self._palette
        return tk.Button(
            parent, text=text, command=command,
            bg=P["accent"] if accent else P["panel"],
            fg=P["accent_fg"] if accent else P["fg"],
            activebackground=P["accent"] if accent else P["border"],
            activeforeground=P["accent_fg"] if accent else P["fg"],
            relief="flat", bd=0, padx=14, pady=6,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
        )

    def _handle_storyboard_command(self, rest: str) -> None:
        """`/storyboard` composer command. `render` -> render the previewed
        state; anything else -> open the panel (prefilling+auto-previewing the
        story text when given)."""
        if rest.lower() == "render":
            self._open_storyboard_panel()
            self._sb_run_render()
            return
        self._open_storyboard_panel(prefill_story=rest, auto_preview=bool(rest))

    def _open_storyboard_panel(self, *, prefill_story: str = "",
                               auto_preview: bool = False) -> None:
        """Open (or focus) the dedicated storyboard panel.

        Single-instance, like Settings: a story box + image picker + aspect
        ratio on top, a read-only two-level (chapters / shots) preview in the
        middle, an estimated Seedance cost + [出片] button at the bottom. Plan
        and render are two separate user-gated steps — preview never spends.
        """
        tk = self._tk
        P = self._palette

        prev = getattr(self, "_sb_dlg", None)
        if prev is not None:
            try:
                if prev.winfo_exists():
                    prev.deiconify()
                    prev.lift()
                    if prefill_story:
                        self._sb_story.delete("1.0", "end")
                        self._sb_story.insert("1.0", prefill_story)
                    if auto_preview:
                        self._sb_run_preview()
                    return
            except Exception:  # noqa: BLE001 - stale handle; rebuild below
                pass

        self._sb_image_paths: list[str] = []
        self._sb_state = None
        self._sb_busy = False

        dlg = tk.Toplevel(self.root, bg=P["bg"])
        self._sb_dlg = dlg
        dlg.title("分镜 — 故事 → 章节 → 镜头 → 出片")
        dlg.resizable(True, True)
        dlg.geometry("760x620")
        dlg.minsize(560, 460)

        # --- top: story text + controls ---
        top = tk.Frame(dlg, bg=P["bg"])
        top.pack(side=tk.TOP, fill=tk.X, padx=14, pady=(12, 6))
        tk.Label(top, text="故事文本", anchor="w", bg=P["bg"], fg=P["fg"],
                 font=("Segoe UI", 10, "bold")).pack(side=tk.TOP, fill=tk.X)
        story_wrap = tk.Frame(top, bg=P["panel"], highlightbackground=P["border"],
                              highlightthickness=1, bd=0)
        story_wrap.pack(side=tk.TOP, fill=tk.X, pady=(4, 8))
        self._sb_story = tk.Text(story_wrap, height=6, wrap=tk.WORD,
                                 font=("Cascadia Code", 10), bg=P["panel"], fg=P["fg"],
                                 insertbackground=P["fg"], relief="flat", bd=0,
                                 padx=10, pady=6, highlightthickness=0)
        self._sb_story.pack(fill=tk.X)
        if prefill_story:
            self._sb_story.insert("1.0", prefill_story)

        ctrl = tk.Frame(top, bg=P["bg"])
        ctrl.pack(side=tk.TOP, fill=tk.X)
        img_btn = self._flat_btn(ctrl, "选图片（可选）", self._sb_pick_images)
        img_btn.pack(side=tk.LEFT)
        self._sb_images_label = tk.Label(ctrl, text="未选图片", anchor="w",
                                         bg=P["bg"], fg=P["muted"], font=("Segoe UI", 9))
        self._sb_images_label.pack(side=tk.LEFT, padx=(8, 16))
        tk.Label(ctrl, text="比例", anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9)).pack(side=tk.LEFT)
        self._sb_ratio = tk.StringVar(value="16:9")
        om = tk.OptionMenu(ctrl, self._sb_ratio, "16:9", "9:16", "1:1", "4:3")
        om.config(bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                  activeforeground=P["fg"], relief="flat", bd=0,
                  highlightthickness=0, font=("Segoe UI", 9), anchor="w")
        om["menu"].config(bg=P["panel"], fg=P["fg"], activebackground=P["accent"],
                          activeforeground=P["accent_fg"])
        om.pack(side=tk.LEFT, padx=(6, 16))
        self._sb_preview_btn = self._flat_btn(ctrl, "生成预览", self._sb_run_preview,
                                              accent=True)
        self._sb_preview_btn.pack(side=tk.RIGHT)

        # --- thumbnail strip for picked reference images (filled on pick) ---
        # Always packed here (empty -> 0 height) so thumbs sit right under the
        # picker row; _sb_render_thumbs fills/clears its children.
        self._sb_thumb_strip = tk.Frame(top, bg=P["bg"])
        self._sb_thumb_strip.pack(side=tk.TOP, fill=tk.X, pady=(0, 4))
        self._sb_thumb_imgs = []  # keep PhotoImage refs alive (else GC blanks them)

        # --- middle: two-level preview (chapters then shots) ---
        mid = tk.Frame(dlg, bg=P["bg"])
        mid.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=14, pady=(2, 6))
        pv_vsb = tk.Scrollbar(mid, orient=tk.VERTICAL)
        self._sb_preview = tk.Text(
            mid, wrap="word", state=tk.DISABLED, font=("Cascadia Code", 10),
            bg=P["panel"], fg=P["fg"], relief="flat", bd=0, padx=10, pady=8,
            highlightthickness=0, yscrollcommand=pv_vsb.set, cursor="arrow",
        )
        pv_vsb.config(command=self._sb_preview.yview)
        pv_vsb.pack(side=tk.RIGHT, fill=tk.Y)
        self._sb_preview.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        self._sb_preview.tag_config("chapter", foreground=P["accent"],
                                    font=("Cascadia Code", 11, "bold"), spacing1=6)
        self._sb_preview.tag_config("shot", foreground=P["tool"],
                                    font=("Cascadia Code", 10, "bold"), spacing1=4)
        self._sb_preview.tag_config("field", foreground=P["muted"])
        self._sb_preview.tag_config("body", foreground=P["fg"])
        # Result-view status colors: success (teal) vs failure (red).
        self._sb_preview.tag_config("ok", foreground=P["tool"],
                                    font=("Cascadia Code", 10, "bold"), spacing1=6)
        self._sb_preview.tag_config("fail", foreground=P["err"],
                                    font=("Cascadia Code", 10, "bold"), spacing1=6)

        # --- results: per-shot status + ▶播放 / ↻重试 (filled after a render) ---
        # A scrollable list packed between the preview and the bottom bar. Each
        # shot gets a row: ✓ + ▶播放 when a clip rendered, ✗/— + ↻重试 otherwise
        # (so a failed shot can be re-generated on its own without re-spending on
        # the ones that already succeeded).
        res_outer = tk.Frame(dlg, bg=P["bg"])
        self._sb_results_outer = res_outer
        res_canvas = tk.Canvas(res_outer, bg=P["bg"], highlightthickness=0,
                               height=0)
        res_vsb = tk.Scrollbar(res_outer, orient=tk.VERTICAL,
                               command=res_canvas.yview)
        res_inner = tk.Frame(res_canvas, bg=P["bg"])
        res_inner.bind(
            "<Configure>",
            lambda e: res_canvas.configure(scrollregion=res_canvas.bbox("all")))
        res_win = res_canvas.create_window((0, 0), window=res_inner, anchor="nw")
        res_canvas.bind(
            "<Configure>", lambda e: res_canvas.itemconfig(res_win, width=e.width))
        res_canvas.configure(yscrollcommand=res_vsb.set)
        res_vsb.pack(side=tk.RIGHT, fill=tk.Y)
        res_canvas.pack(side=tk.LEFT, fill=tk.X, expand=True)
        self._sb_results_canvas = res_canvas
        self._sb_results_inner = res_inner
        # res_outer is packed lazily by _sb_render_results once rows exist.

        # --- bottom: cost estimate + render + progress ---
        bot = tk.Frame(dlg, bg=P["bg"])
        bot.pack(side=tk.BOTTOM, fill=tk.X, padx=14, pady=(0, 12))
        self._sb_cost_label = tk.Label(bot, text="", anchor="w", bg=P["bg"],
                                       fg=P["fg"], font=("Segoe UI", 9))
        self._sb_cost_label.pack(side=tk.LEFT, fill=tk.X, expand=True)
        self._sb_render_btn = tk.Button(
            bot, text="▶ 出片", command=self._sb_run_render,
            bg=P["panel"], fg=P["err"], activebackground=P["border"],
            activeforeground=P["err"], relief="flat", bd=0, padx=14, pady=6,
            font=("Segoe UI", 9), cursor="hand2", highlightthickness=0,
            state=tk.DISABLED,
        )
        self._sb_render_btn.pack(side=tk.RIGHT)
        self._sb_status = tk.Label(dlg, text="", anchor="w", bg=P["bg"],
                                   fg=P["muted"], font=("Segoe UI", 9),
                                   wraplength=720, justify="left")
        self._sb_status.pack(side=tk.BOTTOM, fill=tk.X, padx=14, pady=(0, 4))

        # Restore last session's inputs (story + images + ratio) unless the
        # caller prefilled a story (e.g. via the /storyboard command). This is
        # the panel "memory" — reopening or restarting brings back what you had.
        if not prefill_story:
            mem = self._sb_load_memory()
            if mem.get("story"):
                self._sb_story.delete("1.0", "end")
                self._sb_story.insert("1.0", mem["story"])
            imgs = [p for p in (mem.get("images") or []) if isinstance(p, str)]
            if imgs:
                self._sb_image_paths = imgs
                self._sb_images_label.config(text=f"{len(imgs)} 张图片")
                self._sb_render_thumbs()
            if mem.get("ratio"):
                try:
                    self._sb_ratio.set(mem["ratio"])
                except Exception:  # noqa: BLE001
                    pass

        # Save inputs on close so they survive across panel reopen / restart.
        def _on_close() -> None:
            self._sb_save_memory()
            try:
                dlg.destroy()
            except Exception:  # noqa: BLE001
                pass
        dlg.protocol("WM_DELETE_WINDOW", _on_close)

        if auto_preview and prefill_story:
            self._sb_run_preview()

    def _sb_memory_path(self):
        """Where the panel persists its inputs (story + images + ratio)."""
        from pathlib import Path as _Path
        return (_Path(self._workspace) / "storyboard_out" / "_panel.json").resolve()

    def _sb_save_memory(self) -> None:
        """Persist the panel's story + picked images + ratio (best-effort).

        Lets reopening the panel (or relaunching the app) restore what you had
        instead of starting blank. Any failure is swallowed so saving memory
        never disturbs the UI.
        """
        import json as _json
        try:
            story = self._sb_story.get("1.0", "end").strip()
        except Exception:  # noqa: BLE001
            story = ""
        ratio = "16:9"
        try:
            ratio = self._sb_ratio.get()
        except Exception:  # noqa: BLE001
            pass
        data = {
            "story": story,
            "images": list(getattr(self, "_sb_image_paths", []) or []),
            "ratio": ratio,
        }
        try:
            path = self._sb_memory_path()
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(_json.dumps(data, ensure_ascii=False, indent=2),
                            encoding="utf-8")
        except Exception:  # noqa: BLE001
            pass

    def _sb_load_memory(self) -> dict:
        """Load the panel's last-saved inputs; {} when none/unreadable."""
        import json as _json
        try:
            path = self._sb_memory_path()
            if path.is_file():
                data = _json.loads(path.read_text(encoding="utf-8"))
                if isinstance(data, dict):
                    return data
        except Exception:  # noqa: BLE001
            pass
        return {}

    def _sb_render_thumbs(self) -> None:
        """Show small thumbnails of the picked reference images under the picker.

        Uses Pillow to decode any format (PNG/JPEG/webp/…) and downscale to a
        ~64px-tall thumbnail. PhotoImage refs are kept in ``_sb_thumb_imgs`` so
        Tk doesn't GC them (which would blank the images). Best-effort per
        image: an unreadable file gets a small "?" placeholder; if Pillow is
        missing, the strip shows a one-line hint instead of crashing.
        """
        tk = self._tk
        P = self._palette
        strip = getattr(self, "_sb_thumb_strip", None)
        if strip is None:
            return
        for child in strip.winfo_children():
            child.destroy()
        self._sb_thumb_imgs = []
        paths = getattr(self, "_sb_image_paths", []) or []
        if not paths:
            return
        try:
            from PIL import Image, ImageTk
        except Exception:  # noqa: BLE001 - Pillow missing: count-only fallback
            tk.Label(strip, text="(装 pillow 可显示缩略图)", bg=P["bg"],
                     fg=P["muted"], font=("Segoe UI", 8)).pack(side=tk.LEFT)
            return
        from pathlib import Path as _Path
        for p in paths[:8]:  # cap the strip so many images don't overflow it
            cell = tk.Frame(strip, bg=P["bg"])
            cell.pack(side=tk.LEFT, padx=(0, 6))
            try:
                im = Image.open(p)
                im.thumbnail((96, 64))
                photo = ImageTk.PhotoImage(im)
                self._sb_thumb_imgs.append(photo)
                tk.Label(cell, image=photo, bg=P["bg"]).pack(side=tk.TOP)
            except Exception:  # noqa: BLE001 - unreadable -> placeholder
                tk.Label(cell, text="?", width=6, height=3, bg=P["panel"],
                         fg=P["err"], font=("Segoe UI", 9)).pack(side=tk.TOP)
            name = _Path(p).name
            if len(name) > 14:
                name = name[:12] + "…"
            tk.Label(cell, text=name, bg=P["bg"], fg=P["muted"],
                     font=("Segoe UI", 8)).pack(side=tk.TOP)
        if len(paths) > 8:
            tk.Label(strip, text=f"+{len(paths) - 8}", bg=P["bg"],
                     fg=P["muted"], font=("Segoe UI", 8)).pack(side=tk.LEFT)

    def _sb_pick_images(self) -> None:
        """Pick reference images for the storyboard (optional)."""
        from tkinter import filedialog
        paths = filedialog.askopenfilenames(
            title="选择参考图片（角色/背景）", parent=self._sb_dlg,
            filetypes=[("Images", "*.png *.jpg *.jpeg *.gif *.webp *.bmp"),
                       ("All files", "*.*")],
        )
        if not paths:
            return
        self._sb_image_paths = list(paths)
        n = len(self._sb_image_paths)
        self._sb_images_label.config(text=f"{n} 张图片")
        self._sb_render_thumbs()
        self._sb_save_memory()

    def _sb_set_status(self, text: str, *, error: bool = False) -> None:
        lbl = getattr(self, "_sb_status", None)
        if lbl is None:
            self._append(f"\n[storyboard] {text}\n",
                         "result_err" if error else "system")
            return
        try:
            lbl.config(text=text,
                       fg=self._palette["err"] if error else self._palette["muted"])
        except Exception:  # noqa: BLE001
            pass

    def _sb_build_obj(self, story: str) -> dict:
        """Build the schema project dict from the panel's story + images."""
        import uuid
        return {
            "project": {
                "id": f"sb_{uuid.uuid4().hex[:8]}",
                "title": story[:40] or "Untitled storyboard",
                "target_model": "seedance",
                "aspect_ratio": self._sb_ratio.get(),
                "language": "zh",
            },
            "inputs": {
                "story_text": story,
                "images": [
                    {"image_id": f"img_{i:02d}", "path": p, "kind": "unknown"}
                    for i, p in enumerate(self._sb_image_paths, 1)
                ],
            },
        }

    def _sb_build_deps(self, *, want_render: bool):
        """Wire pipeline deps from layered config (planner/chapters always;
        vision only with a VL backend + images; seedance only when rendering).
        Raises with a clear message when a needed key is missing."""
        import os

        from nanocodex.config import load_config
        from nanocodex.provider.deepseek import DeepSeekProvider
        from nanocodex.storyboard.clients import (
            ChapterPlanner, SeedanceClient, TextPlanner, VisionAnalyzer,
        )
        from nanocodex.storyboard.pipeline import PipelineDeps

        cfg = load_config(workspace=self._workspace)
        if not cfg.api_key:
            raise RuntimeError("未配置文本模型 API key（设置 DEEPSEEK_API_KEY）。")
        prov = DeepSeekProvider(api_key=cfg.api_key, base_url=cfg.base_url,
                                model=cfg.model, timeout_s=cfg.timeout_s)
        chapters = ChapterPlanner(prov)
        planner = TextPlanner(prov)
        vision = None
        if self._sb_image_paths and cfg.vl_model:
            vision = VisionAnalyzer(DeepSeekProvider(
                api_key=cfg.vl_api_key or cfg.api_key,
                base_url=cfg.vl_base_url or cfg.base_url,
                model=cfg.vl_model, timeout_s=cfg.timeout_s,
            ))
        seedance = None
        if want_render:
            # cfg.ark_api_key already folds in the ARK_API_KEY env var (config
            # merges it), so reading config alone covers both file + env.
            seedance = SeedanceClient(cfg.ark_api_key)
        return PipelineDeps(vision=vision, chapters=chapters, planner=planner,
                            seedance=seedance)

    def _sb_run_preview(self) -> None:
        """[生成预览]: plan chapters + shots on a worker thread (never renders)."""
        if getattr(self, "_sb_busy", False):
            return
        story = self._sb_story.get("1.0", "end").strip()
        if not story:
            self._sb_set_status("先填入故事文本。", error=True)
            return
        self._sb_busy = True
        try:
            self._sb_preview_btn.config(text="预览中…", state=self._tk.DISABLED)
        except Exception:  # noqa: BLE001
            pass
        self._sb_set_status("正在拆分章节、生成镜头…（不出片，不计费）")
        obj = self._sb_build_obj(story)
        threading.Thread(target=self._sb_preview_thread, args=(obj,),
                         daemon=True).start()

    def _sb_preview_thread(self, obj: dict) -> None:
        """Daemon thread: run_planning over its own asyncio loop; result to queue."""
        try:
            deps = self._sb_build_deps(want_render=False)
            from nanocodex.storyboard.pipeline import run_planning
            state = asyncio.run(run_planning(obj, deps))
            self._ui_queue.put(_UiEvent("sb_preview", state))
        except Exception as exc:  # noqa: BLE001 - surfaced to the panel
            self._ui_queue.put(_UiEvent("sb_error", f"{type(exc).__name__}: {exc}"))

    def _sb_run_render(self) -> None:
        """[出片]: render the previewed state after an explicit confirm (COSTS $$)."""
        if getattr(self, "_sb_busy", False):
            return
        state = getattr(self, "_sb_state", None)
        if state is None or not getattr(state, "payloads", None):
            self._sb_set_status("先生成预览再出片。", error=True)
            return
        from tkinter import messagebox
        from nanocodex.agent.pricing import estimate_seedance_cost_cny
        total_s = sum(float(s.duration_sec) for s in state.shots)
        est = estimate_seedance_cost_cny(total_s)
        ok = messagebox.askyesno(
            "出片确认（真实计费）",
            f"将调用 Seedance 出片 {len(state.payloads)} 个镜头，"
            f"预计花费 ≈ ¥{est:.2f} CNY（约 {total_s:.0f}s，720p 估算）。\n\n"
            "这会真实计费，确定继续？",
            parent=self._sb_dlg,
        )
        if not ok:
            return
        # Fresh render: reset how much of this state's cost has been folded into
        # the session total (so re-renders only add the delta, never double-count).
        self._sb_folded_cny = 0.0
        self._sb_busy = True
        try:
            self._sb_render_btn.config(state=self._tk.DISABLED)
        except Exception:  # noqa: BLE001
            pass
        self._sb_set_status("出片中…（每镜需轮询，耐心等）")
        threading.Thread(target=self._sb_render_thread, args=(state,),
                         daemon=True).start()

    def _sb_render_thread(self, state) -> None:
        """Daemon thread: render the planned state via Seedance, download the
        finished clips locally, then export. Results to the UI queue."""
        try:
            deps = self._sb_build_deps(want_render=True)
            from nanocodex.storyboard.pipeline import render, export
            out_dir = (self._workspace / "storyboard_out").resolve()

            def _prog(sid: str, i: int, st: str) -> None:
                self._ui_queue.put(_UiEvent("sb_render_progress", (sid, i, st)))

            render(state, deps, on_progress=_prog)
            self._sb_download_clips(state, out_dir)
            export(state, out_dir)
            self._ui_queue.put(_UiEvent("sb_render_done", state))
        except Exception as exc:  # noqa: BLE001 - surfaced to the panel
            self._ui_queue.put(_UiEvent("sb_error", f"{type(exc).__name__}: {exc}"))

    def _sb_download_clips(self, state, out_dir) -> None:
        """Download each successful shot's signed URL to ``out_dir/<shot_id>.mp4``.

        Signed Seedance URLs expire (~24h), so a local copy makes 播放 reliable.
        Best-effort: a download failure leaves the shot playable via its URL and
        never aborts the render. Already-downloaded clips are skipped.
        """
        import urllib.request
        from pathlib import Path as _Path
        out_dir = _Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        for sid, url in state.video_urls.items():
            if not url or str(url).startswith("[failed"):
                continue
            dest = out_dir / f"{sid}.mp4"
            if dest.exists() and dest.stat().st_size > 0:
                continue
            try:
                urllib.request.urlretrieve(url, dest)
            except Exception:  # noqa: BLE001 - URL still works as a fallback
                pass

    def _sb_rerender_one(self, shot_id: str) -> None:
        """Re-render a single (usually failed) shot after a per-shot confirm."""
        if getattr(self, "_sb_busy", False):
            return
        state = getattr(self, "_sb_render_state", None)
        if state is None:
            self._sb_set_status("没有可重出的镜头。", error=True)
            return
        from tkinter import messagebox
        from nanocodex.agent.pricing import estimate_seedance_cost_cny
        shot = next((s for s in state.shots if s.shot_id == shot_id), None)
        dur = float(shot.duration_sec) if shot else 5.0
        est = estimate_seedance_cost_cny(dur)
        ok = messagebox.askyesno(
            "重新生成（真实计费）",
            f"重新生成镜头 {shot_id}，预计 ≈ ¥{est:.2f} CNY"
            f"（约 {dur:.0f}s，720p 估算）。\n\n这会真实计费，确定继续？",
            parent=self._sb_dlg,
        )
        if not ok:
            return
        self._sb_busy = True
        self._sb_set_status(f"重新生成 {shot_id}…（真实计费）")
        threading.Thread(target=self._sb_rerender_thread,
                         args=(state, shot_id), daemon=True).start()

    def _sb_rerender_thread(self, state, shot_id: str) -> None:
        """Daemon thread: re-render ONE shot in place, download, export, refresh."""
        try:
            deps = self._sb_build_deps(want_render=True)
            from nanocodex.storyboard.pipeline import render_one, export
            out_dir = (self._workspace / "storyboard_out").resolve()

            def _prog(sid: str, i: int, st: str) -> None:
                self._ui_queue.put(_UiEvent("sb_render_progress", (sid, i, st)))

            render_one(state, deps, shot_id, on_progress=_prog)
            self._sb_download_clips(state, out_dir)
            export(state, out_dir)
            self._ui_queue.put(_UiEvent("sb_render_done", state))
        except Exception as exc:  # noqa: BLE001 - surfaced to the panel
            self._ui_queue.put(_UiEvent("sb_error", f"{type(exc).__name__}: {exc}"))

    def _sb_show_preview(self, state) -> None:
        """Render the chapters + shots into the read-only preview, set cost."""
        from nanocodex.agent.pricing import estimate_seedance_cost_cny
        tk = self._tk
        view = getattr(self, "_sb_preview", None)
        if view is None:
            return
        view.config(state=tk.NORMAL)
        view.delete("1.0", "end")

        if state.chapters:
            view.insert("end", "章节（故事细节）\n", "chapter")
            for i, ch in enumerate(state.chapters, 1):
                view.insert("end", f"  {i}. {ch.title}\n", "shot")
                if ch.summary:
                    view.insert("end", "     概要: ", "field")
                    view.insert("end", ch.summary + "\n", "body")
                if ch.setting:
                    view.insert("end", "     场景: ", "field")
                    view.insert("end", ch.setting + "\n", "body")
                if ch.characters:
                    view.insert("end", "     角色: ", "field")
                    view.insert("end", ", ".join(ch.characters) + "\n", "body")
                for km in ch.key_moments:
                    view.insert("end", f"      · {km}\n", "body")
            view.insert("end", "\n")

        view.insert("end", f"镜头（{len(state.shots)}）\n", "chapter")
        for s in state.shots:
            view.insert("end", f"  {s.shot_id}  {s.title}  ({s.duration_sec:g}s)\n",
                        "shot")
            if s.camera:
                view.insert("end", "     镜头: ", "field")
                view.insert("end", s.camera + "\n", "body")
            # 中文画面描述给人看（prompt_zh）；英文 prompt 是给视频模型出片用的，
            # 次要显示。老数据没有 prompt_zh 时退回显示英文 prompt。
            if getattr(s, "prompt_zh", ""):
                view.insert("end", "     画面: ", "field")
                view.insert("end", s.prompt_zh + "\n", "body")
                if s.prompt:
                    view.insert("end", "     出片(英): ", "field")
                    view.insert("end", s.prompt + "\n", "body")
            elif s.prompt:
                view.insert("end", "     prompt: ", "field")
                view.insert("end", s.prompt + "\n", "body")
            if s.negative_prompt:
                view.insert("end", "     负向: ", "field")
                view.insert("end", s.negative_prompt + "\n", "body")
        view.config(state=tk.DISABLED)
        view.see("1.0")

        total_s = sum(float(s.duration_sec) for s in state.shots)
        est = estimate_seedance_cost_cny(total_s)
        self._sb_cost_label.config(
            text=f"预计花费 ≈ ¥{est:.2f} CNY（{len(state.shots)} 镜，"
                 f"约 {total_s:.0f}s，720p 估算）")
        try:
            self._sb_render_btn.config(state=tk.NORMAL)
        except Exception:  # noqa: BLE001
            pass

    def _sb_show_render_done(self, state) -> None:
        """Show per-shot results + actual cost after a render; fold cost in.

        Keeps the rendered state so a failed shot can be re-generated on its
        own, then (re)builds the per-shot result list (✓ + ▶播放 / ✗ + ↻重试).
        """
        self._sb_render_state = state
        self._sb_busy = False
        run_cny = round(sum(float(c.get("cost_cny", 0.0))
                            for c in state.video_costs.values()), 4)
        ok_n = sum(1 for u in state.video_urls.values()
                   if not str(u).startswith("[failed"))
        # Fold this render's spend into the session-wide running total so the
        # status bar reflects it (same as the agent storyboard tool does).
        try:
            self._loop.tools.ctx.seedance_cost_cny += run_cny
        except Exception:  # noqa: BLE001
            pass
        self._update_context_usage()
        try:
            self._sb_render_btn.config(state=self._tk.NORMAL)
        except Exception:  # noqa: BLE001
            pass
        self._sb_render_results(state)
        self._sb_set_status(
            f"出片完成：{ok_n}/{len(state.video_urls)} 成功，本次 ¥{run_cny:.4f} CNY。"
            f"成功的已下载到 storyboard_out/，失败的可单独点「↻重试」重出。")

    def _sb_render_results(self, state) -> None:
        """(Re)build the per-shot result rows: status + ▶播放 / ↻重试.

        A succeeded shot shows ✓ + its title + ▶播放 (opens the local mp4, or
        the signed URL as fallback). A failed shot shows ✗ + the short error +
        ↻重试, which re-renders just that shot (no re-spend on the good ones).
        """
        tk = self._tk
        P = self._palette
        inner = getattr(self, "_sb_results_inner", None)
        if inner is None:
            return
        for child in inner.winfo_children():
            child.destroy()

        out_dir = (self._workspace / "storyboard_out").resolve()
        urls = state.video_urls or {}
        for s in state.shots:
            url = str(urls.get(s.shot_id, ""))
            ok = bool(url) and not url.startswith("[failed")
            row = tk.Frame(inner, bg=P["bg"])
            row.pack(side=tk.TOP, fill=tk.X, pady=1)
            mark = "✓" if ok else ("✗" if url else "—")
            tk.Label(row, text=mark, width=2, anchor="w", bg=P["bg"],
                     fg=(P["tool"] if ok else P["err"]),
                     font=("Segoe UI", 10, "bold")).pack(side=tk.LEFT)
            tk.Label(row, text=f"{s.shot_id}  {s.title}", anchor="w",
                     bg=P["bg"], fg=P["fg"], font=("Segoe UI", 9)).pack(
                         side=tk.LEFT, padx=(4, 8))
            if ok:
                local = out_dir / f"{s.shot_id}.mp4"
                self._flat_btn(
                    row, "▶ 播放",
                    lambda p=local, u=url: self._sb_play_clip(p, u),
                ).pack(side=tk.RIGHT)
            else:
                self._flat_btn(
                    row, "↻ 重试",
                    lambda sid=s.shot_id: self._sb_rerender_one(sid),
                ).pack(side=tk.RIGHT)
                if url:
                    short = url[8:60].replace("\n", " ")  # strip "[failed: "
                    tk.Label(row, text=short, anchor="e", bg=P["bg"],
                             fg=P["muted"], font=("Segoe UI", 8)).pack(
                                 side=tk.RIGHT, padx=(0, 8))

        # Reveal the results area (packed lazily so it stays hidden pre-render)
        # and cap its height so it scrolls instead of pushing the panel.
        try:
            self._sb_results_canvas.config(height=min(150, 26 * len(state.shots)))
            self._sb_results_outer.pack(side=tk.TOP, fill=tk.X,
                                        padx=14, pady=(0, 4))
        except Exception:  # noqa: BLE001
            pass

    def _sb_play_clip(self, local_path, url: str) -> None:
        """Open a rendered clip: prefer the local mp4, fall back to the URL.

        Uses the OS default handler (os.startfile on Windows) so the system
        video player opens it. A signed URL is the fallback when the local
        download is missing/failed (it still works until it expires ~24h).
        """
        import os
        from pathlib import Path as _Path
        target = None
        try:
            p = _Path(local_path)
            if p.exists() and p.stat().st_size > 0:
                target = str(p)
        except Exception:  # noqa: BLE001
            target = None
        if target is None:
            target = url
        try:
            startfile = getattr(os, "startfile", None)
            if startfile is not None:
                startfile(target)
            else:  # non-Windows fallback
                import webbrowser
                webbrowser.open(target)
        except Exception as exc:  # noqa: BLE001
            self._sb_set_status(f"打开失败：{exc}", error=True)

    def _on_ab_compare(self) -> None:
        """Open the A/B setup dialog: two configs + one prompt, run isolated.

        Disabled while busy (an A/B run rebuilds loops and drives files, same
        as a turn). Requires a clean git workspace — checked here so the user
        gets a clear reason instead of a mid-run failure.
        """
        if self._busy:
            self._append("\n[busy — wait for the current turn to finish]\n", "system")
            return
        if getattr(self, "_ab_running", False):
            self._append("\n[an A/B comparison is already running]\n", "system")
            return
        from nanocodex.agent.ab_compare import ensure_clean_git_workspace, ABGitError
        try:
            ensure_clean_git_workspace(self._workspace)
        except ABGitError as exc:
            self._append(
                f"\n[A/B needs a clean git workspace: {exc}]\n", "result_err",
            )
            return
        self._show_ab_setup_dialog()

    def _show_ab_setup_dialog(self) -> None:
        """Two columns of config controls + a shared prompt box + Run button."""
        tk = self._tk
        P = self._palette

        prev = getattr(self, "_ab_setup_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:  # noqa: BLE001
                pass

        cfg = getattr(self._loop, "_cfg", None) if self._loop else None
        info = cfg.redacted() if cfg is not None else {}
        cur_model = info.get("model", "")
        cur_sandbox = info.get("sandbox_mode", "workspace-write")
        cur_approval = info.get("approval_policy", "on-request")
        cur_reasoning = info.get("reasoning_effort", "auto")
        models = list(getattr(cfg, "available_models", []) or [])
        if cur_model and cur_model not in models:
            models = [cur_model, *models]
        if not models:
            models = [cur_model or "deepseek-v4-pro"]

        from nanocodex.config import VALID_SANDBOX_MODES, VALID_APPROVAL_POLICIES

        dlg = tk.Toplevel(self.root, bg=P["bg"])
        self._ab_setup_dlg = dlg
        dlg.title("A/B compare")
        dlg.transient(self.root)
        dlg.resizable(True, True)
        dlg.geometry("680x560")

        status = tk.Label(dlg, text="", anchor="w", bg=P["bg"], fg=P["err"],
                          font=("Segoe UI", 9), wraplength=620, justify="left")
        status.pack(side="bottom", fill="x", padx=16, pady=(0, 6))

        btns = tk.Frame(dlg, bg=P["bg"])
        btns.pack(side="bottom", fill="x", padx=16, pady=(0, 14))

        # Two side-by-side config columns.
        cols = tk.Frame(dlg, bg=P["bg"])
        cols.pack(side="top", fill="x", padx=16, pady=(14, 8))
        cols.columnconfigure(0, weight=1)
        cols.columnconfigure(1, weight=1)

        def _make_column(parent, title, *, col):
            box = tk.Frame(parent, bg=P["bg"])
            box.grid(row=0, column=col, sticky="we", padx=(0, 8) if col == 0 else (8, 0))
            box.columnconfigure(1, weight=1)
            tk.Label(box, text=title, anchor="w", bg=P["bg"], fg=P["accent"],
                     font=("Segoe UI", 10, "bold")).grid(
                row=0, column=0, columnspan=2, sticky="w", pady=(0, 6))
            r = [1]

            def _opt(label, choices, current):
                tk.Label(box, text=label, anchor="w", bg=P["bg"], fg=P["muted"],
                         font=("Segoe UI", 9)).grid(row=r[0], column=0, sticky="w",
                                                    padx=(0, 8), pady=3)
                var = tk.StringVar(value=current if current in choices else choices[0])
                om = tk.OptionMenu(box, var, *choices)
                om.config(bg=P["panel"], fg=P["fg"], activebackground=P["border"],
                          activeforeground=P["fg"], relief="flat", bd=0,
                          highlightthickness=0, font=("Segoe UI", 9), anchor="w")
                om["menu"].config(bg=P["panel"], fg=P["fg"],
                                  activebackground=P["accent"],
                                  activeforeground=P["accent_fg"])
                om.grid(row=r[0], column=1, sticky="we", pady=3)
                r[0] += 1
                return var

            return {
                "model": _opt("model", tuple(models), cur_model),
                "sandbox": _opt("sandbox", VALID_SANDBOX_MODES, cur_sandbox),
                "approval": _opt("approval", VALID_APPROVAL_POLICIES, cur_approval),
                "reasoning": _opt("reasoning", _REASONING_CHOICES, cur_reasoning),
            }

        vars_a = _make_column(cols, "Config A", col=0)
        vars_b = _make_column(cols, "Config B", col=1)

        # Shared prompt.
        tk.Label(dlg, text="Task prompt (runs in both, in isolated worktrees)",
                 anchor="w", bg=P["bg"], fg=P["muted"],
                 font=("Segoe UI", 9, "bold")).pack(side="top", fill="x", padx=16)
        prompt_box = tk.Text(dlg, height=6, wrap="word", bg=P["panel"], fg=P["fg"],
                             relief="flat", bd=0, padx=10, pady=8,
                             font=("Cascadia Code", 10), highlightthickness=0,
                             insertbackground=P["fg"])
        prompt_box.pack(side="top", fill="both", expand=True, padx=16, pady=(2, 8))

        def _overrides_from(v):
            return {
                "model": v["model"].get().strip(),
                "sandbox_mode": v["sandbox"].get().strip(),
                "approval_policy": v["approval"].get().strip(),
                "reasoning_effort": v["reasoning"].get().strip(),
            }

        def _run() -> None:
            prompt = prompt_box.get("1.0", "end").strip()
            if not prompt:
                status.config(text="Enter a task prompt.", fg=P["err"])
                return
            ov_a = _overrides_from(vars_a)
            ov_b = _overrides_from(vars_b)
            dlg.destroy()
            self._start_ab_run(prompt, ov_a, ov_b)

        def ab_btn(label, command, *, accent=False):
            bg = P["accent"] if accent else P["panel"]
            fg = P["accent_fg"] if accent else P["fg"]
            return tk.Button(
                btns, text=label, command=command, bg=bg, fg=fg,
                activebackground=P["border"] if not accent else P["accent"],
                activeforeground=fg, relief="flat", bd=0, padx=14, pady=6,
                font=("Segoe UI", 9, "bold"), cursor="hand2", highlightthickness=0,
            )

        ab_btn("Run A/B", _run, accent=True).pack(side="right")
        ab_btn("Cancel", dlg.destroy).pack(side="right", padx=(0, 8))
        dlg.bind("<Escape>", lambda e: dlg.destroy())

    def _start_ab_run(self, prompt: str, ov_a: dict, ov_b: dict) -> None:
        """Kick off the A/B worker thread (mirrors _start_turn's setup)."""
        from nanocodex.agent.ab_compare import ABConfig
        cfg_a = ABConfig(label="A", overrides=ov_a)
        cfg_b = ABConfig(label="B", overrides=ov_b)
        self._ab_running = True
        self._cancel_event.clear()
        self._set_busy(True)
        self._append(
            f"\n[A/B] running the task under two configs in isolated worktrees…\n"
            f"  A: model={ov_a.get('model')} reasoning={ov_a.get('reasoning_effort')}\n"
            f"  B: model={ov_b.get('model')} reasoning={ov_b.get('reasoning_effort')}\n",
            "system",
        )
        threading.Thread(
            target=self._run_ab_thread, args=(prompt, cfg_a, cfg_b), daemon=True,
        ).start()

    def _run_ab_thread(self, prompt: str, cfg_a, cfg_b) -> None:
        """Daemon thread: run both sides serially in isolated worktrees.

        Mirrors _run_turn_thread (own asyncio loop, desktop lock, results via
        the UI queue). Worktrees are NOT cleaned here — they're kept so the
        result dialog can adopt one side's diff; cleanup happens after the user
        picks (or on a hard failure below).
        """
        import time
        import tempfile
        import asyncio
        from pathlib import Path as _Path
        from nanocodex.cli import _build_loop, _auto_approve_approver
        from nanocodex.agent.ab_compare import (
            ensure_clean_git_workspace, create_worktree, collect_worktree_diff,
            cleanup_worktree, build_result, worktree_name, ABGitError,
        )

        got_lock = self._desktop_lock.acquire(blocking=False)
        if not got_lock:
            self._ui_queue.put(_UiEvent(
                "system_text",
                "\n[A/B waiting for a background scheduled task to finish…]\n",
            ))
            self._desktop_lock.acquire()
            got_lock = True

        worktrees: list[_Path] = []
        try:
            base_commit = ensure_clean_git_workspace(self._workspace)
            tmp_root = _Path(tempfile.mkdtemp(prefix="nanocodex-ab-"))
            token = str(int(time.time()))

            def _run_side(cfg):
                name = worktree_name(cfg.label, token)
                wt = create_worktree(self._workspace, base_commit, name, tmp_root)
                worktrees.append(wt)
                loop = _build_loop(
                    dict(cfg.overrides), wt, resume=False,
                    approver_factory=lambda _p: _auto_approve_approver(),
                    log_path=None,  # ephemeral: A/B runs never touch session history
                )
                start = time.time()
                tr = asyncio.run(loop.run_turn(
                    prompt, cancel_check=self._cancel_event.is_set,
                ))
                elapsed = time.time() - start
                diff = collect_worktree_diff(wt)
                return build_result(
                    cfg, tr, elapsed_s=elapsed, diff=diff, worktree_path=str(wt),
                )

            res_a = _run_side(cfg_a)
            res_b = _run_side(cfg_b)
            self._ui_queue.put(_UiEvent("ab_result", (res_a, res_b)))
        except ABGitError as exc:
            self._ui_queue.put(_UiEvent("error", f"A/B aborted: {exc}"))
            for wt in worktrees:
                cleanup_worktree(self._workspace, wt)
        except Exception as exc:  # noqa: BLE001 - surfaced to the UI
            self._ui_queue.put(_UiEvent("error", f"A/B failed: {type(exc).__name__}: {exc}"))
            for wt in worktrees:
                cleanup_worktree(self._workspace, wt)
        finally:
            if got_lock:
                self._desktop_lock.release()
            self._ui_queue.put(_UiEvent("ab_done"))

    def _show_ab_result_dialog(self, res_a, res_b) -> None:
        """Show both sides' summary + diff; adopt one or discard both.

        Adopting applies the chosen side's diff onto the real workspace; then
        BOTH worktrees are cleaned up. Discarding cleans both and changes
        nothing.
        """
        tk = self._tk
        P = self._palette
        from nanocodex.agent.ab_compare import (
            format_ab_comparison, adopt_diff, cleanup_worktree, ABGitError,
        )

        prev = getattr(self, "_ab_result_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:  # noqa: BLE001
                pass

        dlg = tk.Toplevel(self.root, bg=P["bg"])
        self._ab_result_dlg = dlg
        dlg.title("A/B result")
        dlg.transient(self.root)
        dlg.resizable(True, True)
        dlg.geometry("820x620")

        def _cleanup_both() -> None:
            for r in (res_a, res_b):
                wt = getattr(r, "worktree_path", "")
                if wt:
                    cleanup_worktree(self._workspace, Path(wt))

        def _adopt(chosen) -> None:
            try:
                adopt_diff(self._workspace, chosen.diff)
            except ABGitError as exc:
                self._append(f"\n[A/B adopt failed: {exc}]\n", "result_err")
                return
            _cleanup_both()
            self._append(
                f"\n[A/B] adopted config {chosen.label}'s changes into the "
                f"workspace ({len(chosen.diff.splitlines())} diff lines).\n",
                "system",
            )
            dlg.destroy()

        def _discard() -> None:
            _cleanup_both()
            self._append("\n[A/B] discarded both sides; workspace unchanged.\n", "system")
            dlg.destroy()

        btns = tk.Frame(dlg, bg=P["bg"])
        btns.pack(side="bottom", fill="x", padx=16, pady=(0, 14))

        def rb(label, command, *, accent=False):
            bg = P["accent"] if accent else P["panel"]
            fg = P["accent_fg"] if accent else P["fg"]
            return tk.Button(
                btns, text=label, command=command, bg=bg, fg=fg,
                activebackground=P["border"] if not accent else P["accent"],
                activeforeground=fg, relief="flat", bd=0, padx=14, pady=6,
                font=("Segoe UI", 9, "bold"), cursor="hand2", highlightthickness=0,
            )

        rb(f"Adopt A", lambda: _adopt(res_a), accent=True).pack(side="right")
        rb(f"Adopt B", lambda: _adopt(res_b), accent=True).pack(side="right", padx=(0, 8))
        rb("Discard both", _discard).pack(side="right", padx=(0, 8))

        # Summary line on top.
        tk.Label(dlg, text=format_ab_comparison(res_a, res_b), anchor="w",
                 justify="left", bg=P["bg"], fg=P["fg"], font=("Cascadia Code", 9),
                 wraplength=780).pack(side="top", fill="x", padx=16, pady=(14, 8))

        # Two diff panes side by side.
        panes = tk.Frame(dlg, bg=P["bg"])
        panes.pack(side="top", fill="both", expand=True, padx=16, pady=(0, 8))
        panes.columnconfigure(0, weight=1)
        panes.columnconfigure(1, weight=1)
        panes.rowconfigure(1, weight=1)

        for col, r in ((0, res_a), (1, res_b)):
            tk.Label(panes, text=f"Config {r.label} diff", anchor="w", bg=P["bg"],
                     fg=P["accent"], font=("Segoe UI", 9, "bold")).grid(
                row=0, column=col, sticky="w", padx=(0, 6) if col == 0 else (6, 0))
            box = tk.Text(panes, wrap="none", bg=P["panel"], fg=P["fg"], relief="flat",
                          bd=0, padx=10, pady=8, font=("Cascadia Code", 9),
                          highlightthickness=0)
            box.grid(row=1, column=col, sticky="nsew",
                     padx=(0, 6) if col == 0 else (6, 0))
            box.insert("1.0", r.diff or "(no file changes)")
            box.config(state="disabled")

        dlg.bind("<Escape>", lambda e: _discard())

    def _start_turn(self, text: str) -> None:
        """Echo the prompt and kick off a worker turn for it (idle path).

        Shared by _on_send (when not busy) and the queue drain at turn end, so
        the 'echo + clear cancel + busy + spawn worker' sequence lives once.

        Pending 📎 attachments are folded into the actual message content here
        (`_consume_attachments`): the transcript still echoes the plain typed
        text, but the worker receives the full content (a multimodal block list
        when images are attached, so a vision model can see them).
        """
        self._append(f"\nyou › {text}\n", "user")
        content = self._consume_attachments(text)
        self._cancel_event.clear()
        self._set_busy(True)
        self._worker = threading.Thread(
            target=self._run_turn_thread, args=(content,), daemon=True,
        )
        self._worker.start()

    def _drain_queue(self) -> None:
        """At turn end, start the next queued input if any (main thread).

        Stop only cancels the CURRENT turn (the user's choice); the queue keeps
        going, so a cancelled turn still hands off to whatever was waiting.
        """
        if self._busy or self._loop is None:
            return
        if not self._pending_inputs:
            return
        text = self._pending_inputs.pop(0)
        self._refresh_send_label()
        self._start_turn(text)

    def _refresh_send_label(self) -> None:
        """Update the Send button text to reflect the queue backlog."""
        btn = getattr(self, "send_btn", None)
        if btn is None:
            return
        try:
            btn.config(text=_send_button_label(queued=len(self._pending_inputs)))
        except Exception:  # noqa: BLE001 - label is cosmetic, never crash a turn
            pass

    def _request_stop(self) -> None:
        """Ask the running turn to stop at its next cancellation point."""
        # Ignore repeat clicks: only the first one prints and disables the btn.
        if self._busy and not self._cancel_event.is_set():
            self._cancel_event.set()
            self._append("\n[stopping… will halt after the current step]\n", "system")
            self.stop_btn.config(state=self._tk.DISABLED)  # avoid repeat clicks

    def _run_turn_thread(self, text: str) -> None:
        """Runs on a daemon thread; owns its own asyncio loop for this turn."""
        hooks = self._make_gui_hooks()
        result = None
        # Serialize against the managed scheduler: the user's turn and a
        # scheduled task drive the SAME mouse/keyboard, so they must never run
        # at once. The user always wins — take the lock BLOCKING (we're on a
        # worker thread, so this never freezes the UI). A scheduled task that
        # can't get the lock just skips its tick. Try non-blocking first so we
        # only print the "waiting" notice when a task is actually mid-run.
        got_lock = self._desktop_lock.acquire(blocking=False)
        if not got_lock:
            self._ui_queue.put(_UiEvent(
                "system_text",
                "\n[waiting for a background scheduled task to finish…]\n",
            ))
            self._desktop_lock.acquire()  # block until the task releases it
            got_lock = True
        try:
            result = asyncio.run(self._loop.run_turn(
                text, hooks, cancel_check=self._cancel_event.is_set,
            ))
        except ProviderError as exc:
            self._ui_queue.put(_UiEvent("error", f"Provider error: {exc}"))
        except Exception as exc:  # noqa: BLE001 - surfaced to the UI
            self._ui_queue.put(_UiEvent("error", f"{type(exc).__name__}: {exc}"))
        finally:
            if got_lock:
                self._desktop_lock.release()
            # Tell the UI WHY the turn ended, so "it just stopped" is never a
            # mystery (completed vs max_iterations vs cancelled vs error).
            self._ui_queue.put(_UiEvent("turn_end", result))
            self._ui_queue.put(_UiEvent("done"))

    def _make_gui_hooks(self) -> LoopHooks:
        q = self._ui_queue
        # Holds the apply_patch payload built at tool-start until its result
        # arrives. The loop runs tools strictly start→execute→result with no
        # interleaving (loop.py:196-201), so a single slot is race-free.
        pending_patch: dict[str, Any] = {}

        async def on_reasoning(delta: str) -> None:
            q.put(_UiEvent("reasoning", delta))

        async def on_content(delta: str) -> None:
            q.put(_UiEvent("stream", delta))

        async def on_stream_end() -> None:
            q.put(_UiEvent("stream", "\n"))

        async def on_tool_start(tc) -> None:
            q.put(_UiEvent("tool", f"\n  → {tc.name} {_summarize(tc)}\n"))
            # File-diff panel: only apply_patch carries patch text, and only
            # on_tool_start receives `tc`. Build the Tk-free payload now but DON'T
            # render yet — stash it so on_tool_result can emit only after the
            # patch actually applied (avoids showing an edit that failed to land,
            # e.g. a locator that didn't match).
            pending_patch.pop("payload", None)
            if tc.name == "apply_patch":
                payload = _build_file_edit_payload(tc.arguments.get("patch", ""))
                if payload:
                    pending_patch["payload"] = payload

        async def on_tool_result(name: str, result: str) -> None:
            first = result.strip().splitlines()[0] if result.strip() else "(no output)"
            ok = not first.startswith(("Error", "Sandbox denied"))
            q.put(_UiEvent("result", (ok, f"  ← {first[:160]}\n")))
            # Render the diff only once the patch is confirmed applied — the
            # tool result is the source of truth for success.
            payload = pending_patch.pop("payload", None)
            if name == "apply_patch" and ok and payload:
                q.put(_UiEvent("file_edit", payload))

        return LoopHooks(
            on_content_delta=on_content,
            on_reasoning_delta=on_reasoning,
            on_stream_end=on_stream_end,
            on_tool_start=on_tool_start,
            on_tool_result=on_tool_result,
        )

    # --- main-thread queue pump -----------------------------------------

    def _poll_queue(self) -> None:
        try:
            while True:
                ev = self._ui_queue.get_nowait()
                self._handle_event(ev)
        except queue.Empty:
            pass
        self.root.after(40, self._poll_queue)

    def _handle_event(self, ev: _UiEvent) -> None:
        if ev.kind == "stream":
            self._append(ev.payload, "content")
        elif ev.kind == "reasoning":
            self._append(ev.payload, "reasoning")
        elif ev.kind == "tool":
            self._append(ev.payload, "tool")
        elif ev.kind == "file_edit":
            self._on_file_edit(ev.payload)
        elif ev.kind == "result":
            ok, text = ev.payload
            self._append(text, "result_ok" if ok else "result_err")
        elif ev.kind == "error":
            self._append(f"\n{ev.payload}\n", "result_err")
        elif ev.kind == "system_text":
            self._append(ev.payload, "system")
        elif ev.kind == "approval":
            self._show_approval_dialog(ev.payload)
        elif ev.kind == "enhance_result":
            self._enhancing = False
            self._refresh_enhance_label()
            original, rewritten = ev.payload
            self._show_enhance_dialog(original, rewritten)
        elif ev.kind == "enhance_error":
            self._enhancing = False
            self._refresh_enhance_label()
            self._append(f"\n[enhance failed: {ev.payload}]\n", "result_err")
        elif ev.kind == "ab_result":
            res_a, res_b = ev.payload
            self._show_ab_result_dialog(res_a, res_b)
        elif ev.kind == "ab_done":
            # A/B finished (success, abort, or error). Clear the in-flight flags
            # so the UI unlocks; the result dialog (if any) was already shown by
            # the ab_result event that preceded this one.
            self._ab_running = False
            self._set_busy(False)
        elif ev.kind == "sb_preview":
            self._sb_busy = False
            self._sb_state = ev.payload
            try:
                self._sb_preview_btn.config(text="生成预览", state=self._tk.NORMAL)
            except Exception:  # noqa: BLE001
                pass
            self._sb_set_status("预览完成。检查无误后点「出片」（真实计费）。")
            self._sb_show_preview(ev.payload)
        elif ev.kind == "sb_render_progress":
            sid, i, st = ev.payload
            self._sb_set_status(f"出片中… {sid}: 轮询 {i} — {st}")
        elif ev.kind == "sb_render_done":
            self._sb_busy = False
            try:
                self._sb_render_btn.config(state=self._tk.NORMAL)
            except Exception:  # noqa: BLE001
                pass
            self._sb_show_render_done(ev.payload)
        elif ev.kind == "sb_error":
            self._sb_busy = False
            try:
                self._sb_preview_btn.config(text="生成预览", state=self._tk.NORMAL)
                self._sb_render_btn.config(state=self._tk.NORMAL)
            except Exception:  # noqa: BLE001
                pass
            self._sb_set_status(str(ev.payload), error=True)
        elif ev.kind == "turn_end":
            self._announce_turn_end(ev.payload)
        elif ev.kind == "done":
            self._render_plan()
            self._record_session_index()
            self._refresh_session_list()
            self._set_busy(False)
            # Codex-style queue: if the user typed more tasks while this turn
            # ran, start the next one now (Stop cancels only the current turn,
            # never the backlog).
            self._drain_queue()

    # --- helpers ---------------------------------------------------------

    def _record_session_index(self) -> None:
        """Upsert this workspace's summary into the global session directory.

        Runs on the main thread at turn end (the session message list is stable
        then). Best-effort: a directory-index failure must never disturb the
        conversation, so every error is swallowed.
        """
        loop = self._loop
        if loop is None:
            return
        try:
            cfg = getattr(loop, "_cfg", None)
            workspace = str(getattr(cfg, "workspace", "") or self._workspace or "")
            if not workspace:
                return
            log_path = ""
            lp = getattr(loop.session, "_log_path", None)
            if lp is not None:
                log_path = str(lp)
            self._session_index.record_turn(
                self._session_id, workspace, loop.session.for_model(),
                log_path=log_path,
            )
        except Exception:  # noqa: BLE001 - the index is a convenience, never critical
            pass

    def _refresh_session_list(self) -> None:
        """Repopulate the sidebar from the global index, newest activity first.

        Runs on the main thread (turn end / startup). Best-effort: a listing
        failure must never disturb the conversation, so errors are swallowed and
        the list is simply left as-is.
        """
        lb = getattr(self, "session_list", None)
        if lb is None:
            return
        try:
            entries = self._session_index.entries()
        except Exception:  # noqa: BLE001 - listing is a convenience, never critical
            return
        # Remember which conversation was selected (by session_id) so we keep
        # it highlighted across a refresh instead of jumping the selection.
        prev_id = None
        sel = lb.curselection()
        if sel and 0 <= sel[0] < len(self._session_entries):
            prev_id = self._session_entries[sel[0]].session_id

        lb.delete(0, self._tk.END)
        self._session_entries = list(entries)
        reselect = None
        import os
        for i, s in enumerate(self._session_entries):
            # One compact row: project folder + when it started + the prompt
            # title. The date disambiguates the multiple conversations a project
            # now keeps (each open mints a separate history entry). Full details
            # and the transcript replay live in the click-through view.
            folder = os.path.basename(str(s.workspace).rstrip("/\\")) or s.workspace
            day = (s.created_at or s.updated_at or "")[:10]
            head = f"{folder} {day}".strip()
            label = f"{head} — {s.title}" if s.title else head
            lb.insert(self._tk.END, label[:70])
            if s.session_id == prev_id:
                reselect = i
        if reselect is not None:
            lb.selection_set(reselect)

    def _refresh_schedule_panel(self) -> None:
        """Repaint the Scheduled panel from the store + the live running flag.

        Runs on the main thread (slow timer / toggle / startup). Everything but
        "running now" comes from ~/.nanocodex/schedule.json; the running id is
        the one live bit, set by the scheduler thread (atomic str assignment).
        Best-effort: a read/render failure must never disturb the conversation,
        so errors are swallowed and the panel is left as-is.
        """
        panel = getattr(self, "schedule_panel", None)
        if panel is None:
            return
        try:
            from nanocodex.agent.schedule import ScheduleStore
            tasks = ScheduleStore().tasks
        except Exception:  # noqa: BLE001 - listing is a convenience, never critical
            return
        running_id = self._scheduler_running_id  # atomic read of the live flag
        try:
            panel.config(state=self._tk.NORMAL)
            panel.delete("1.0", self._tk.END)
            if not tasks:
                panel.insert(self._tk.END, "no scheduled tasks\n", "empty")
            else:
                for t in tasks:
                    is_running = (running_id is not None
                                  and getattr(t, "id", None) == running_id)
                    line = _format_schedule_panel_line(
                        prompt=t.prompt, enabled=t.enabled, kind=t.kind,
                        next_run=t.next_run, last_run=t.last_run, runs=t.runs,
                        allow_desktop=getattr(t, "allow_desktop", False),
                        is_running=is_running,
                    )
                    tag = ("running" if is_running
                           else "off" if not t.enabled else "idle")
                    panel.insert(self._tk.END, line + "\n", tag)
            panel.config(state=self._tk.DISABLED)
        except Exception:  # noqa: BLE001 - rendering must never crash the GUI
            pass

    def _start_schedule_panel_refresh(self) -> None:
        """Arm the slow Scheduled-panel repaint loop (once).

        Separate from the 40ms _poll_queue: the panel only needs to track the
        running dot + next/last times, so a ~3s cadence is plenty and keeps the
        repaint cost negligible. The guard makes _init_loop's re-runs (project /
        model switch) idempotent — only the first call arms the timer.
        """
        if self._sched_panel_timer_on:
            self._refresh_schedule_panel()  # repaint now, don't stack a 2nd timer
            return
        self._sched_panel_timer_on = True

        def _tick() -> None:
            self._refresh_schedule_panel()
            self.root.after(3000, _tick)

        _tick()


    def _on_session_select(self, _event=None) -> None:
        """Replay the selected conversation: a summary header + the FULL frozen
        transcript (when a snapshot exists).

        Read-only: this surfaces the stored digest plus the complete message
        history saved at turn end, so the user can revisit what was actually
        said — not just a digest. It does NOT switch the live conversation;
        opening a different workspace is the existing 'Open project' flow.
        """
        lb = getattr(self, "session_list", None)
        if lb is None:
            return
        sel = lb.curselection()
        if not sel or not (0 <= sel[0] < len(self._session_entries)):
            return
        s = self._session_entries[sel[0]]
        tk = self._tk
        P = self._palette

        prev = getattr(self, "_session_dlg", None)
        if prev is not None:
            try:
                prev.destroy()
            except Exception:  # noqa: BLE001
                pass

        dlg = tk.Toplevel(self.root, bg=P["panel"])
        self._session_dlg = dlg
        dlg.title("Conversation replay")
        dlg.resizable(True, True)
        dlg.geometry("640x520")

        # Bottom button bar, packed FIRST (side=bottom) so it always reserves
        # its space and is never clipped by the transcript above (same lesson as
        # the main window's bottom bars). "Continue" forks this conversation into
        # a NEW one seeded from its snapshot — the original is left untouched.
        btnbar = tk.Frame(dlg, bg=P["panel"])
        btnbar.pack(side="bottom", fill="x", padx=12, pady=(0, 12))
        cont_btn = tk.Button(
            btnbar, text="Continue this conversation",
            command=lambda: self._continue_session(s),
            bg=P["accent"], fg=P["accent_fg"], activebackground=P["accent"],
            activeforeground=P["accent_fg"], relief="flat", bd=0, padx=14, pady=6,
            font=("Segoe UI", 9, "bold"), cursor="hand2", highlightthickness=0,
        )
        cont_btn.pack(side="right")
        # Continuing needs a saved snapshot to seed from; disable otherwise.
        if not getattr(s, "has_snapshot", False):
            cont_btn.config(state=tk.DISABLED, text="Continue (no snapshot)")

        txt = tk.Text(dlg, wrap="word", bg=P["panel"], fg=P["fg"],
                      font=("Segoe UI", 10), relief="flat", bd=0,
                      padx=16, pady=14, highlightthickness=0, spacing1=2, spacing3=2)
        # A scrollbar — a full transcript can be long.
        sb = tk.Scrollbar(dlg, command=txt.yview)
        txt.configure(yscrollcommand=sb.set)
        sb.pack(side="right", fill="y")
        txt.pack(side="left", fill="both", expand=True)
        txt.tag_config("h", foreground=P["fg"], font=("Segoe UI", 11, "bold"))
        txt.tag_config("k", foreground=P["muted"], font=("Segoe UI", 9, "bold"))
        txt.tag_config("v", foreground=P["fg"], font=("Segoe UI", 10))
        txt.tag_config("role_user", foreground=P["accent"], font=("Segoe UI", 10, "bold"))
        txt.tag_config("role_assistant", foreground=P["fg"], font=("Segoe UI", 10, "bold"))
        txt.tag_config("role_tool", foreground=P["tool"], font=("Segoe UI", 9, "bold"))
        txt.tag_config("msg", foreground=P["fg"], font=("Segoe UI", 10))
        txt.tag_config("toolnote", foreground=P["muted"], font=("Segoe UI", 9, "italic"))

        # --- summary header ---
        tools = ", ".join(s.recent_tools) if s.recent_tools else "none"
        txt.insert("end", (s.title or "(no prompt yet)") + "\n", "h")
        txt.insert("end", "\nWorkspace\n", "k")
        txt.insert("end", f"{s.workspace}\n", "v")
        txt.insert("end", "\nStarted / last activity\n", "k")
        txt.insert("end", f"{s.created_at or 'unknown'}  →  {s.updated_at or 'unknown'}\n", "v")
        txt.insert("end", "\nActivity\n", "k")
        txt.insert("end",
                   f"{s.user_messages} user / {s.assistant_messages} assistant "
                   f"message(s), {s.tool_calls} tool call(s)\n", "v")
        txt.insert("end", "\nRecent tools\n", "k")
        txt.insert("end", f"{tools}\n", "v")

        # --- full transcript replay (from the frozen snapshot) ---
        txt.insert("end", "\n" + "─" * 40 + "\n", "k")
        messages = None
        try:
            messages = self._session_index.load_snapshot(s.session_id)
        except Exception:  # noqa: BLE001 - replay is a convenience, never critical
            messages = None
        if not messages:
            txt.insert("end", "\n[no full transcript saved for this conversation]\n", "toolnote")
            if s.log_path:
                txt.insert("end", "\nTranscript log\n", "k")
                txt.insert("end", f"{s.log_path}\n", "v")
        else:
            txt.insert("end", "\nFull transcript\n\n", "k")
            self._render_transcript(txt, messages)
        txt.config(state=tk.DISABLED)

    def _render_transcript(self, txt, messages: "list[dict]") -> None:
        """Render a frozen message list into the replay Text widget (read-only).

        Skips the system prompt (scaffolding, not conversation); shows each
        user/assistant message and a compact one-line note per tool result, so
        the replay reads like the original chat.
        """
        from nanocodex.agent.session_index import _first_text  # reuse the flattener
        role_label = {"user": "You", "assistant": "nanocodex", "tool": "tool"}
        for m in messages:
            role = m.get("role")
            if role == "system":
                continue
            if role == "tool":
                name = m.get("name", "tool")
                content = str(m.get("content", "") or "")
                first = content.strip().splitlines()[0] if content.strip() else "(no output)"
                txt.insert("end", f"  ↳ {name}: ", "role_tool")
                txt.insert("end", f"{first[:160]}\n", "toolnote")
                continue
            label = role_label.get(role, role or "?")
            txt.insert("end", f"{label}\n", f"role_{role}" if role in ("user", "assistant") else "role_tool")
            body = _first_text(m.get("content"))
            if body:
                txt.insert("end", f"{body}\n", "msg")
            # Note any tool calls the assistant made on this turn.
            for tc in m.get("tool_calls") or []:
                fn = (tc.get("function") or {}).get("name", "tool")
                txt.insert("end", f"  → calls {fn}\n", "toolnote")
            txt.insert("end", "\n", "msg")

    def _continue_session(self, s) -> None:
        """Fork the selected past conversation into a NEW one and continue it.

        Non-destructive: the original session's snapshot/log are untouched. We
        load its frozen transcript, mint a FRESH session_id (so the continuation
        gets its own history entry and its own session.jsonl), seed a forked
        Session from those messages, and rebuild the live loop. The prior
        transcript is echoed into the main panel so the conversation reads as
        one continuous thread.
        """
        if self._busy:
            self._append("\n[busy — wait for the current turn to finish]\n", "system")
            return
        if self._session_index is None:
            self._append("\n[session history unavailable]\n", "result_err")
            return
        try:
            messages = self._session_index.load_snapshot(s.session_id)
        except Exception:  # noqa: BLE001 - treated as "no snapshot"
            messages = None
        if not messages:
            self._append("\n[no saved transcript to continue from]\n", "result_err")
            return

        # Close the replay dialog now that we're acting on it.
        dlg = getattr(self, "_session_dlg", None)
        if dlg is not None:
            try:
                dlg.destroy()
            except Exception:  # noqa: BLE001
                pass

        # A continuation is a NEW conversation: fresh id + its own snapshot/log,
        # so forking never overwrites the source. Switch the workspace to the
        # source's so paths/AGENTS.md resolve as they did originally.
        try:
            from nanocodex.agent.session_index import new_session_id
            self._session_id = new_session_id()
        except Exception:  # noqa: BLE001 - id is a convenience, never critical
            pass
        if getattr(s, "workspace", ""):
            self._workspace = Path(s.workspace)

        # Clear the live transcript, then replay the inherited messages so the
        # user sees the thread they're continuing, then build the seeded loop.
        self.output.config(state=self._tk.NORMAL)
        self.output.delete("1.0", "end")
        self.output.config(state=self._tk.DISABLED)
        self._append(f"Continuing conversation: {s.title or '(untitled)'}\n", "system")
        self._echo_seed_transcript(messages)

        # Hand the seed to _init_loop (consumed once), which forks the Session.
        self._pending_seed = messages
        self._init_loop()

    def _echo_seed_transcript(self, messages: "list[dict]") -> None:
        """Replay inherited messages into the MAIN panel (not the replay popup).

        Mirrors _render_transcript's role mapping but writes to self._append so
        the continued thread looks like the live conversation it's becoming.
        """
        from nanocodex.agent.session_index import _first_text
        for m in messages:
            role = m.get("role")
            if role == "system":
                continue
            if role == "tool":
                name = m.get("name", "tool")
                content = str(m.get("content", "") or "")
                first = content.strip().splitlines()[0] if content.strip() else "(no output)"
                self._append(f"  ↳ {name}: {first[:160]}\n", "tool")
                continue
            body = _first_text(m.get("content"))
            if role == "user":
                self._append(f"\nyou › {body}\n", "user")
            elif role == "assistant":
                if body:
                    self._append(f"{body}\n", "content")
                for tc in m.get("tool_calls") or []:
                    fn = (tc.get("function") or {}).get("name", "tool")
                    self._append(f"  → calls {fn}\n", "tool")
        self._append("\n" + "─" * 30 + " (continuing) " + "─" * 30 + "\n\n", "system")

    def _render_plan(self) -> None:
        plan = getattr(self._loop, "_plan", None)
        if not plan:
            return
        from nanocodex.tools import render_plan
        self._append("\n[plan]\n" + render_plan(plan) + "\n", "system")

    def _record_turn_cost(self, result) -> None:
        """Price this turn's usage and fold it into the running session total.

        Uses the REAL usage the provider reported (summed across the turn's
        model calls in loop.run_turn), priced via pricing.cost_usd against the
        active model. An unknown model price or empty usage gives None — we then
        show nothing for the turn rather than a misleading $0.00, but the session
        total (and its display) is still refreshed.
        """
        from nanocodex.agent.pricing import cost_usd

        usage = getattr(result, "usage", None) or {}
        cfg = getattr(self._loop, "_cfg", None) if self._loop else None
        model = getattr(cfg, "model", "") or ""
        turn_cost = cost_usd(model, usage)
        self._last_turn_cost_usd = turn_cost
        if turn_cost is not None:
            self._session_cost_usd += turn_cost
        # Cost is shown only in the status bar at the bottom (session total),
        # not echoed into the transcript per turn — keeps the conversation clean.
        self._update_context_usage()  # refresh the status bar's cost readout

    def _announce_turn_end(self, result) -> None:
        """Say WHY the turn ended, so a mid-task stop is never a silent mystery.

        result is a TurnResult or None (None = an exception already reported).
        """
        if result is None:
            return
        # Real-cost accounting: price this turn's accumulated usage (summed
        # across every model call the turn made) and fold it into the session
        # total. Best-effort — unknown model price or empty usage yields None,
        # and a failure here must never swallow the turn-end notice below.
        try:
            self._record_turn_cost(result)
        except Exception:  # noqa: BLE001 - cost display is a convenience
            pass
        reason = getattr(result, "stop_reason", "")
        notes = {
            "completed": None,  # normal: the model stopped asking for tools
            "cancelled": "[stopped by you]",
            "error": "[ended on a provider error — see above]",
            "max_iterations": (
                f"[stopped after hitting the {self._loop.max_iterations}-step limit — "
                "the task may be UNFINISHED. Type 'continue' to resume, or raise the "
                "limit with NANOCODEX_MAX_ITERATIONS / --max-iterations.]"
            ),
        }
        note = notes.get(reason)
        if note:
            self._append(f"\n{note}\n", "result_err")
            if reason == "max_iterations":
                # Resumable: light up the Continue button.
                self.continue_btn.config(state=self._tk.NORMAL)
        elif reason == "completed":
            # The model ended the turn by talking instead of acting. If the plan
            # still has unfinished steps, flag it — that's the "开发一半就停" case.
            plan = getattr(self._loop, "_plan", None) or []
            pending = [s for s in plan if s.get("status") != "completed"]
            if pending:
                self._append(
                    f"\n[the model paused with {len(pending)} plan step(s) not yet "
                    "completed. Type 'continue' to have it keep going.]\n",
                    "system",
                )
                self.continue_btn.config(state=self._tk.NORMAL)

    def _append(self, text: str, tag: str) -> None:
        self.output.config(state=self._tk.NORMAL)
        self.output.insert("end", text, tag)
        self.output.see("end")
        self.output.config(state=self._tk.DISABLED)

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        # Codex-style queueing: Send stays ENABLED while a turn runs so the user
        # can type the next task (it queues). The button TEXT reflects the
        # backlog ("Queue (N)"). Stop is the dedicated interrupt and tracks busy.
        self.send_btn.config(state=self._tk.NORMAL)
        self.stop_btn.config(state=self._tk.NORMAL if busy else self._tk.DISABLED)
        self._refresh_send_label()
        self._update_context_usage()

    def run(self) -> None:
        try:
            self.root.mainloop()
        finally:
            # Window closed: tell the scheduler thread to stop polling so it
            # exits cleanly at its next poll boundary (it's a daemon thread, so
            # this is a courtesy for a tidy shutdown, not strictly required).
            self._scheduler_stop.set()
            self._scheduler_enabled = False


def _summarize(tc) -> str:
    args = tc.arguments
    if tc.name == "shell":
        return str(args.get("command", ""))[:80]
    if tc.name == "apply_patch":
        patch = str(args.get("patch", ""))
        files = [ln.split(": ", 1)[-1] for ln in patch.splitlines()
                 if ln.startswith("*** ") and "File:" in ln]
        return ", ".join(files)[:80]
    if tc.name == "read_file":
        return str(args.get("path", ""))
    if tc.name == "web_search":
        return str(args.get("query", ""))[:80]
    if tc.name == "update_plan":
        return f"{len(args.get('plan', []))} steps"
    # Desktop MCP tools (mcp__windows_computer_use__<tool>): show, in plain
    # words, what's being done on the real desktop — this is the live
    # step-by-step of the desktop automation, shown right in the transcript.
    if tc.name.startswith("mcp__"):
        return _summarize_desktop(tc.name.rsplit("__", 1)[-1], args)
    return ""


def _summarize_desktop(tool: str, args: dict) -> str:
    """Human-readable description of one desktop action, for the live view."""
    if tool == "list_windows":
        return "listing open windows"
    if tool == "focus_window":
        return f"focusing window {args.get('window_id', '')}"
    if tool == "get_ui_tree":
        return f"reading UI tree of window {args.get('window_id', '')}"
    if tool == "capture_screen":
        return "capturing screen" + (" region" if args.get("region") else "")
    if tool == "click_element":
        return f"clicking element {args.get('element_id', '')} in window {args.get('window_id', '')}"
    if tool == "click_xy":
        return f"clicking at ({args.get('x')}, {args.get('y')})"
    if tool == "type_text":
        t = str(args.get("text", ""))
        return f"typing {t[:40]!r}" + ("…" if len(t) > 40 else "")
    if tool == "press_keys":
        return "pressing " + "+".join(args.get("keys", []))
    if tool == "wait":
        return f"waiting {args.get('ms', '')} ms"
    if tool.startswith("desktop_"):
        return str(args.get("instruction", ""))[:60]
    return ""


# --- right-side file-diff panel (pure helpers, no Tk / no disk) ----------
# The V4A patch format carries NO real line numbers, so UPDATE rows leave the
# gutter blank and lean on +/- coloring; only ADD rows get true 1..n numbering.
# These mirror the _build_status pure-function pattern so they're unit-testable
# without a display (see tests/test_gui_file_panel.py).

_FILE_PANEL_MAX_ROWS = 2000


def _line_gutter(n: "int | None", width: int = 4) -> str:
    """Right-aligned line-number gutter; blanks when the number is absent."""
    if n is None:
        return " " * (width + 1)
    return f"{n:>{width}} "


def _classify_patch_file(action) -> dict:
    """Turn one parsed FileAction into a render-ready dict of classified rows.

    Pure: consumes nanocodex.tools.patch data only, touches no Tk and no disk.
    Caps total rows at _FILE_PANEL_MAX_ROWS and flags `truncated` when hit.
    """
    from nanocodex.tools.patch import ActionType

    rows: list[dict] = []
    truncated = False

    def add_row(kind: str, text: str, *, old_no=None, new_no=None) -> bool:
        nonlocal truncated
        if len(rows) >= _FILE_PANEL_MAX_ROWS:
            truncated = True
            return False
        rows.append({"kind": kind, "old_no": old_no, "new_no": new_no, "text": text})
        return True

    op = "M"
    move_to = getattr(action, "move_to", None)
    if action.type is ActionType.ADD:
        op = "A"
        for i, line in enumerate(action.new_lines, start=1):
            if not add_row("added", line, new_no=i):
                break
    elif action.type is ActionType.DELETE:
        op = "D"
        add_row("context", "(file deleted)")
    else:  # UPDATE (possibly a rename when move_to is set)
        if move_to:
            op = "R"
        for chunk in action.chunks:
            locator = "  ".join(chunk.locators) if chunk.locators else ""
            if not add_row("hunk_sep", locator):
                break
            stop = False
            for line in chunk.del_lines:
                if not add_row("removed", line):
                    stop = True
                    break
            if stop:
                break
            for line in chunk.ins_lines:
                if not add_row("added", line):
                    stop = True
                    break
            if stop:
                break

    return {
        "path": action.path,
        "op": op,
        "move_to": move_to,
        "rows": rows,
        "truncated": truncated,
    }


def _build_file_edit_payload(patch_text: str) -> "dict | None":
    """Parse a V4A patch into a Tk-free payload for the file panel.

    Returns None on a parse error or a no-op patch (every file has zero rows),
    so a malformed or empty patch never blanks an already-rendered panel.
    """
    from nanocodex.tools.patch import PatchError, parse_patch

    if not patch_text or not isinstance(patch_text, str):
        return None
    try:
        actions = parse_patch(patch_text)
    except PatchError:
        return None
    files = [_classify_patch_file(a) for a in actions]
    if not any(f["rows"] for f in files):
        return None
    return {"files": files}


def _is_mcp_command(command: str) -> bool:
    """An approval request whose 'command' is an MCP tool name (mcp__<srv>__<tool>).

    MCP desktop tools post their tool NAME as the approval command (see
    McpTool._gate_decision), so this distinguishes a desktop/WeChat action from a
    plain shell command string.
    """
    return command.startswith("mcp__")


def _approval_short_circuit(
    command: str, *, auto_approve_on: bool, allow_all_mcp: bool,
    always_allow: set[str],
) -> bool:
    """Pure decision: may this approval request skip the dialog and auto-approve?

    Mirrors Codex's "approve for session" semantics. Returns True when:
      * global auto-approve is on (everything runs), OR
      * this is an MCP desktop action AND the user previously chose
        "allow all desktop (this session)", OR
      * this exact command was marked "always allow this command".

    Kept module-level and Tk-free so it can be unit-tested without a GUI.
    """
    if auto_approve_on:
        return True
    if allow_all_mcp and _is_mcp_command(command):
        return True
    if command in always_allow:
        return True
    return False


def _scheduler_run_plan(*, allow_desktop: bool, mcp_connected: bool) -> tuple[str, bool]:
    """Pure decision for how the managed scheduler runs one task.

    Returns ``(approver_kind, attach_mcp_tools)`` where ``approver_kind`` is
    ``"desktop_only"`` or ``"auto_deny"``. The whole security posture of the
    managed scheduler lives in this one mapping, so it's Tk-free and unit-tested:

      * allow_desktop + MCP connected -> desktop-only approver, ATTACH the
        desktop tools (the only way an unattended task can act on the desktop).
      * allow_desktop but MCP NOT connected -> still desktop-only approver, but
        there are no tools to attach (task simply can't reach the desktop).
      * allow_desktop FALSE (the default) -> auto-deny approver AND no desktop
        tools attached at all. Withholding the tools is strictly safer than
        attaching them and relying on the approver to refuse: the task literally
        has no capability to drive the desktop.
    """
    if allow_desktop:
        return "desktop_only", bool(mcp_connected)
    return "auto_deny", False


# Hard ceiling on ONE unattended scheduled turn. A scheduled task holds the
# desktop lock while it runs, so a stuck task (a hung MCP call, a model await
# that never returns, an agent spinning to the step limit) would otherwise pin
# the lock and freeze the user's GUI conversation. Bounded here so the lock is
# always given back. Generous enough for a real read+analyze+reply; override via
# NANOCODEX_SCHEDULER_TIMEOUT (seconds, <=0 disables). Only applies to UNATTENDED
# scheduled runs — the user's own interactive turn is never timed out (they watch
# it and can press Stop).
_SCHEDULER_TURN_TIMEOUT_S = 180.0


def _scheduler_turn_timeout() -> float:
    """Resolve the scheduled-turn timeout (env override, else the default)."""
    import os
    raw = os.environ.get("NANOCODEX_SCHEDULER_TIMEOUT")
    if raw:
        try:
            return float(raw)
        except ValueError:
            pass
    return _SCHEDULER_TURN_TIMEOUT_S


async def _run_scheduled_turn(*, lock, run, timeout_s, soft_grace_s=5.0):
    """Run ONE unattended scheduled turn under *lock*, bounded by a timeout.

    Tk-free and fully injectable so it unit-tests offline:

    * ``lock`` — a ``threading.Lock``-like (``acquire(blocking=False)`` /
      ``release()``). Taken NON-blocking: if the user is mid-turn we skip this
      firing (returns ``{"status": "skipped"}``) so the user's conversation
      always wins, and we never touch the lock we didn't get.
    * ``run`` — async callable ``run(cancel_check) -> result``; it builds the
      ephemeral loop, attaches tools, and runs the turn. ``cancel_check`` is the
      flag the agent loop already polls (every 0.1s, even inside a hung tool) to
      cancel cleanly. Everything that can hang lives inside ``run`` so the
      timeout covers loop-build + MCP attach + the turn itself.
    * ``timeout_s`` — hard ceiling (<=0 disables timing out).

    Two-stage stop, reusing the loop's proven cancellation path:
      1. SOFT: at ``timeout_s`` flip the cancel flag; the loop abandons the
         in-flight tool and returns a clean ``cancelled`` result.
      2. HARD: ``wait_for`` at ``timeout_s + soft_grace_s`` force-cancels the
         whole coroutine if even cooperative cancel can't return (e.g. stuck in
         a model HTTP await the cancel flag can't reach).

    The lock is ALWAYS released in ``finally`` — neither stage bypasses it.
    Returns an outcome dict the caller turns into a log line.
    """
    import asyncio

    if not lock.acquire(blocking=False):
        return {"status": "skipped"}
    try:
        cancel = {"flag": False}

        def cancel_check() -> bool:
            return cancel["flag"]

        coro = run(cancel_check)
        if not timeout_s or timeout_s <= 0:
            result = await coro
            return {
                "status": "done",
                "stop_reason": getattr(result, "stop_reason", ""),
                "summary": (getattr(result, "final_text", "") or "")[:100],
            }

        async def _soft_deadline() -> None:
            await asyncio.sleep(timeout_s)
            cancel["flag"] = True  # ask the turn to stop cooperatively

        deadline = asyncio.ensure_future(_soft_deadline())
        hard = timeout_s + max(1.0, soft_grace_s)
        try:
            result = await asyncio.wait_for(asyncio.ensure_future(coro), timeout=hard)
        except asyncio.TimeoutError:
            # Cooperative cancel didn't return in time -> force-killed.
            return {"status": "timeout", "timeout_s": timeout_s, "forced": True}
        finally:
            deadline.cancel()
            try:
                await deadline
            except asyncio.CancelledError:
                pass
            except Exception:  # noqa: BLE001 - deadline teardown, ignore
                pass
        if cancel["flag"]:
            # Soft deadline tripped: the turn returned, but only because we
            # asked it to stop. Report it as a timeout so the log is honest.
            return {"status": "timeout", "timeout_s": timeout_s, "forced": False}
        return {
            "status": "done",
            "stop_reason": getattr(result, "stop_reason", ""),
            "summary": (getattr(result, "final_text", "") or "")[:100],
        }
    except Exception as exc:  # noqa: BLE001 - one bad task mustn't kill the loop
        return {"status": "error", "error": f"{type(exc).__name__}: {exc}"}
    finally:
        lock.release()


def _now_iso() -> str:
    """Current local time as an ISO second-precision string (for log lines)."""
    from datetime import datetime
    return datetime.now().isoformat(timespec="seconds")


def _format_scheduler_log_entry(
    *, now_iso: str, task_id: str, allow_desktop: bool,
    stop_reason: str = "", error: str = "", summary: str = "",
) -> str:
    """Format one ~/.nanocodex/scheduler.log line (pure; timestamp injected).

    Unattended runs never touch the transcript (user's decision), so this file
    is the only record. Kept Tk-free and clock-free (caller passes ``now_iso``)
    so it unit-tests deterministically.
    """
    tag = "desktop" if allow_desktop else "no-desktop"
    head = f"{now_iso} [{task_id}] ({tag})"
    if error:
        return f"{head} ERROR: {error}"
    body = stop_reason or "done"
    if summary:
        body = f"{body}: {summary}"
    return f"{head} {body}"


def _hhmm(iso: str) -> str:
    """Pull HH:MM out of an ISO timestamp for compact display; tolerate junk."""
    if not iso or "T" not in iso:
        return iso or "?"
    return iso.split("T", 1)[1][:5]


def _format_schedule_panel_line(
    *, prompt: str, enabled: bool, kind: str = "once", next_run: str = "",
    last_run: str = "", runs: int = 0, allow_desktop: bool = False,
    is_running: bool = False,
) -> str:
    """Format one scheduled task into a 1-2 line sidebar panel block (pure).

    Tk-free + clock-free so it unit-tests deterministically. Layout:

        <glyph> <label> [desktop]
            <state/next/last/×runs>

    The glyph encodes live state: ``*`` running now (the scheduler thread sets
    this flag), ``-`` enabled & idle, ``=`` disabled. ``is_running`` is the only
    LIVE bit (from the scheduler thread); the rest is read from the ScheduleStore
    (next_run/last_run/runs/enabled). Plain ASCII glyphs — this string also feeds
    a Tk widget, but ASCII keeps it safe everywhere and easy to assert in tests.
    """
    label = " ".join((prompt or "").split())[:24] or "(empty)"
    if is_running:
        glyph, state = "*", "running now"
    elif not enabled:
        glyph, state = "=", "off"
    else:
        glyph, state = "-", ""
    badge = " [desktop]" if allow_desktop else ""
    head = f"{glyph} {label}{badge}"

    bits: list[str] = []
    if state:
        bits.append(state)
    if enabled and not is_running and next_run:
        bits.append(f"next {_hhmm(next_run)}")
    if last_run:
        bits.append(f"last {_hhmm(last_run)}")
    if runs:
        bits.append(f"x{runs}")
    if not bits:
        return head
    return head + "\n    " + "  ".join(bits)


def _settings_sections() -> tuple[str, ...]:
    """Ordered nav entries for the Settings window (Codex-style sections).

    Pure (no Tk) so the navigation order can be unit-tested. The strings double
    as both the nav-button labels and the keys _settings_show_section switches
    on. Only sections nanocodex actually has — no empty Codex stubs.
    """
    return ("General", "Config", "MCP servers", "Marketplace", "Scheduled tasks", "Desktop")


# Reasoning-effort choices offered in the Config section. Mirrors the buckets
# provider/deepseek.py:_apply_reasoning_effort actually understands:
#   auto -> defer (don't send the field)   max -> reasoning_effort=max
#   high -> reasoning_effort=high           off -> thinking disabled
_REASONING_CHOICES = ("auto", "max", "high", "off")


def _collect_schedule_add(
    *, prompt: str, kind: str, run_at: str = "", every_seconds: str = "",
    at_hour: str = "", at_minute: str = "", allow_desktop: bool = False,
) -> dict[str, Any]:
    """Coerce raw Scheduled-tasks form fields into ScheduleStore.add() kwargs.

    Pure (no Tk) so it unit-tests cleanly, and it mirrors exactly what the
    conversational manage_schedule tool does (tools/schedule_tool.py:_add): the
    GUI form and the model share ONE ScheduleStore, so both must coerce inputs
    the same way. We only normalize types here; the store does the real
    validation (raising ValueError on bad kind / empty prompt / etc.), so the
    manual page and the model surface identical errors.

    The string-typed numeric fields come straight from Tk Entry widgets; blanks
    coerce to the store's own defaults (0 / 9 / 0) rather than raising here.
    """
    def _int(value: str, default: int) -> int:
        try:
            return int(str(value).strip())
        except (TypeError, ValueError):
            return default

    return {
        "prompt": (prompt or "").strip(),
        "kind": (kind or "once").strip(),
        "run_at": (run_at or "").strip(),
        "every_seconds": _int(every_seconds, 0),
        "at_hour": _int(at_hour, 9),
        "at_minute": _int(at_minute, 0),
        "allow_desktop": bool(allow_desktop),
    }


def _format_schedule_recurrence(
    *, kind: str, every_seconds: int = 0, at_hour: int = 9, at_minute: int = 0,
) -> str:
    """One-line recurrence summary for a task row (pure, unit-testable).

    once     -> "once"
    interval -> "every Ns" (or a friendlier "every Nm"/"every Nh" for round
                minute/hour periods, so a 3600s task reads "every 1h")
    daily    -> "daily HH:MM"
    Unknown kinds fall back to the raw kind string.
    """
    if kind == "once":
        return "once"
    if kind == "interval":
        secs = max(0, int(every_seconds))
        if secs and secs % 3600 == 0:
            return f"every {secs // 3600}h"
        if secs and secs % 60 == 0:
            return f"every {secs // 60}m"
        return f"every {secs}s"
    if kind == "daily":
        return f"daily {int(at_hour):02d}:{int(at_minute):02d}"
    return str(kind)


def _collect_settings_updates(
    *, api_key: str, base_url: str, model: str,
    sandbox_mode: str, approval_policy: str, reasoning_effort: str,
    vl_base_url: str = "", vl_api_key: str = "", vl_model: str = "",
    ark_api_key: str = "",
) -> dict[str, str]:
    """Build the updates dict for write_nanocodex_config from raw field values.

    Pure (no Tk) so it unit-tests cleanly. Rules:
      * A blank new API key / VL key is OMITTED — an empty submit means "keep the
        existing key", never "set it to ''" (which would wipe it).
      * Every other field is included only when non-empty, so unchanged blanks
        don't overwrite saved values.
    All inputs are expected to be already .strip()'d by the caller.
    """
    updates: dict[str, str] = {}
    if api_key:
        updates["api_key"] = api_key
    if base_url:
        updates["base_url"] = base_url
    if model:
        updates["model"] = model
    if sandbox_mode:
        updates["sandbox_mode"] = sandbox_mode
    if approval_policy:
        updates["approval_policy"] = approval_policy
    if reasoning_effort:
        updates["reasoning_effort"] = reasoning_effort
    # Vision backend (image-bearing turns route here). Same blank-key rule.
    if vl_base_url:
        updates["vl_base_url"] = vl_base_url
    if vl_api_key:
        updates["vl_api_key"] = vl_api_key
    if vl_model:
        updates["vl_model"] = vl_model
    # Volcengine ARK key for Seedance video rendering (storyboard 出片). Same
    # blank-key rule: a blank submit keeps the existing key.
    if ark_api_key:
        updates["ark_api_key"] = ark_api_key
    return updates


def _send_button_label(*, queued: int) -> str:
    """Text for the Send button given how many inputs are QUEUED behind the
    running turn (Codex-style: you can type the next task while one runs).

    Pure so it unit-tests without Tk:
      * 0 queued  -> the normal "Send  ⏎".
      * N queued  -> "Queue (N)  ⏎" so the count of waiting tasks is visible on
        the button itself (the user's chosen surfacing, alongside a transcript
        note per enqueue). The label is the same whether or not a turn is
        currently running — what it reflects is the BACKLOG, not busy-ness.
    """
    if queued > 0:
        return f"Queue ({queued})  ⏎"
    return "Send  ⏎"


def _fmt_tok(n: int) -> str:
    """Format a token count like Claude Code: 666, 12.3k, 1.0M."""
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n / 1_000:.1f}k"
    return str(n)


def _fmt_usd(amount: float) -> str:
    """Format a USD cost. Sub-cent turns are common (a cache-hit prompt costs
    fractions of a cent), so show 4 decimals under $1 and 2 above — a flat
    ``$0.00`` would hide every cheap turn."""
    if amount <= 0:
        return "$0.00"
    if amount < 1:
        return f"${amount:.4f}"
    return f"${amount:,.2f}"


def _fmt_cny(amount: float) -> str:
    """Format a CNY cost. Seedance clips cost a few yuan each, so 2 decimals
    is plenty; sub-cent rounding isn't a concern as it is for USD turns."""
    if amount <= 0:
        return "¥0.00"
    return f"¥{amount:,.2f}"


def _build_status(*, busy: bool, auto_on: bool, model=None, tokens=None,
                  window=None, budget=None, error=None,
                  session_cost=None, seedance_cny=None) -> str:
    """Pure status-bar text builder (no Tk) so it can be unit-tested.

    Always shows state; shows the error if the loop failed to build (so the
    bar is never mysteriously blank); otherwise shows model + context usage.
    *session_cost* (USD float) is appended when present and > 0 — a fresh
    session with no priced turns yet shows nothing rather than "$0.00".
    *seedance_cny* (CNY float) is shown SEPARATELY from session_cost: Seedance
    bills in yuan on a different axis than the USD text models, and we don't
    invent an FX rate to merge them.
    """
    parts = ["working…" if busy else "ready"]
    if auto_on:
        parts.append("auto-approve: ON")
    if error:
        parts.append(f"error: {error}")
        return "  |  ".join(parts)
    if model:
        parts.append(str(model))
    if tokens is not None:
        if window and window > 0:
            pct = int(tokens / window * 100)
            parts.append(f"context: {_fmt_tok(tokens)} / {_fmt_tok(window)} ({pct}%)")
        else:
            parts.append(f"context: {_fmt_tok(tokens)}")
    if budget and budget > 0:
        parts.append(f"compact @ {_fmt_tok(budget)}")
    if session_cost is not None and session_cost > 0:
        parts.append(f"cost: {_fmt_usd(session_cost)}")
    if seedance_cny is not None and seedance_cny > 0:
        parts.append(f"seedance: {_fmt_cny(seedance_cny)}")
    return "  |  ".join(parts)


def launch(overrides: dict, workspace: Path, *, resume: bool = False) -> None:
    NanocodexGUI(overrides, workspace, resume=resume).run()


def main_cli() -> None:
    """Console entry point for ``nanocodex-gui``.

    Thin argparse front end (Typer isn't needed here): supports the same
    workspace / sandbox / approval / model / resume knobs as the CLI, then
    hands off to the Tk window.
    """
    import argparse

    parser = argparse.ArgumentParser(
        prog="nanocodex-gui",
        description="Launch the nanocodex desktop (Tkinter) window.",
    )
    parser.add_argument("--cd", dest="workdir", default=None,
                        help="Workspace directory (default: current).")
    parser.add_argument("-s", "--sandbox", default=None,
                        help="Sandbox mode: read-only | workspace-write | danger-full-access.")
    parser.add_argument("-a", "--approval", default=None,
                        help="Approval policy: untrusted | on-failure | on-request | never.")
    parser.add_argument("-m", "--model", default=None, help="Override the model name.")
    parser.add_argument("--context-budget", dest="context_budget", type=int, default=None,
                        help="Approx token budget that triggers context compaction (0 = off).")
    parser.add_argument("-r", "--resume", action="store_true",
                        help="Resume the previous session from this workspace's history.")
    args = parser.parse_args()

    overrides = {
        "sandbox_mode": args.sandbox,
        "approval_policy": args.approval,
        "model": args.model,
        "context_token_budget": args.context_budget,
    }
    # Workspace resolution: explicit --cd wins; otherwise reuse the last-opened
    # project; otherwise fall back to the current directory.
    if args.workdir:
        workspace = Path(args.workdir).resolve()
    else:
        workspace = _load_last_workspace() or Path.cwd()
    launch(overrides, workspace, resume=args.resume)


if __name__ == "__main__":
    main_cli()
