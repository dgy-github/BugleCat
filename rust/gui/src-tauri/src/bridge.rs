//! Bridges the `!Send` agent loop to Tauri's multi-threaded world.
//!
//! `ncx_core::AgentLoop` is `!Send` (it holds `Rc<RefCell<…>>` plan state and
//! `#[async_trait(?Send)]` trait objects), so it can never cross threads. We
//! therefore pin it to ONE dedicated OS thread running its own current-thread
//! Tokio runtime. Communication crosses the thread boundary only as `Send`
//! data:
//!
//! * IN  — prompts arrive on a `tokio::mpsc` channel (`send_prompt` command).
//! * OUT — the loop's [`LoopEvent`]s and the final result are emitted to the
//!   frontend via the `AppHandle` (which IS `Send + Sync`) as `ncx://event`s.
//! * APPROVALS — a tool that escalates calls [`GuiApprover`], which emits an
//!   `approval` event and AWAITS a one-shot. The `approve` command (Tauri
//!   thread) resolves that one-shot via the shared [`PendingMap`]. This is the
//!   request/response round-trip that crosses the thread boundary mid-turn.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc::Sender as SyncSender, Arc, Mutex};

use async_trait::async_trait;
use ncx_app_server::AppServer;
use ncx_config::{
    load_config, permission_mode_to_knobs, write_nanocodex_config, Config, ConfigPaths,
    McpServerConfig, Overrides,
};
#[cfg(test)]
use ncx_core::ToolContext;
use ncx_core::{
    discover_codex_hooks, discover_codex_mcp_servers, discover_skills, expand_file_mentions,
    load_workspace_instructions, new_session_id, prepare_mcp_server_tools,
    suggest_title_with_provider, AgentLoop, ApprovalDecision, ApprovalHandler, ApprovalRequest,
    CheckpointStore, ConfiguredHarnessRuntime, ContextServiceDescriptor, GoalToolService,
    HarnessAgentRunner, HarnessRunnerEvent, LoopEvent, McpServiceDescriptor, MemoryStore,
    Orchestrator, OrchestratorConfig, OrchestratorControl, OrchestratorEvent,
    RuntimeContextSources, RuntimeHostBindings, Session, SessionGrants, UserQuestionHandler,
    UserQuestionRequest, COMPACTED_HISTORY_PREFIX,
};
use ncx_protocol::{
    ClientRequest, ExecutionMode, GoalRef, ItemId, ResponsePayload, Thread, ThreadId, ThreadItem,
    TurnId, TurnStatus, TurnUsage,
};
#[cfg(test)]
use ncx_sandbox::SandboxPolicy;
use ncx_thread_store::JsonThreadStore;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

mod goal_turn;
mod orchestrated_turn;

const SYSTEM_PROMPT: &str = "You are BugleCat (妙脆角猫咪), the warm, curious, and precise coding agent inside nanocodex. \
    Speak the user's language and lead with useful results. Your personality is friendly and confident, never childish: \
    an occasional subtle cat-themed phrase is welcome in greetings or celebrations, but never add it to errors, warnings, \
    code, logs, or serious technical explanations. Accuracy, action, and verification always come before role-play. \
    Use native workspace tools \
    (find_files, grep, glob, list_directory, path_info, read_file) for recursive discovery and \
    inspection, and prefer them over shell commands. Use apply_patch for edits and update_plan for \
    multi-step work. If a path is incomplete, search recursively instead of guessing. Keep responses concise. \
    Use goal tools only for genuinely long-running same-session objectives. Call get_goal before update_goal; \
    copy the exact goal id and revision. After resume, fork, or restart, continuation is disarmed until a direct \
    human request resumes it. Mark blocked only after the same concrete blocker persists for at least three admitted \
    goal rounds; difficulty or remaining work is not a blocker. \
    The final answer should contain only the execution result and a brief recommended next action; \
    do not recap tool calls, logs, or intermediate process.";

/// Injected into the system prompt when the active permission mode is `plan`.
const PLAN_MODE_NOTE: &str = "You are in PLAN MODE. Do NOT modify files or run state-changing \
    commands — the apply_patch tool is disabled and will refuse edits, and write/escalating shell \
    commands are blocked. Investigate (read files, run read-only commands) and produce a clear, \
    concrete plan for the user to review and approve. Present the plan as your final message; make \
    no changes.";

/// The Tauri event name every UI update is delivered on.
pub const EVENT: &str = "ncx://event";
pub const PROTOCOL_EVENT: &str = "ncx://protocol-event";

/// Pending approval requests, keyed by id. Shared between the agent thread
/// (inserts a one-shot sender when asking) and the `approve` command (takes it
/// to answer). `Send + Sync` so it can live in Tauri state.
pub type PendingMap = Arc<Mutex<HashMap<u64, (String, oneshot::Sender<ApprovalDecision>)>>>;
pub type PendingQuestionMap = Arc<Mutex<HashMap<u64, (String, oneshot::Sender<Option<String>>)>>>;

/// Shared cooperative cancellation state for the active GUI turn.
pub type CancelFlag = Arc<AtomicBool>;
pub type CancelRegistry = Arc<Mutex<HashMap<String, CancelFlag>>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRunKind {
    Human,
    Goal,
}
pub type RunningSessions = Arc<Mutex<HashMap<String, SessionRunKind>>>;
#[derive(Clone)]
pub struct DeferredPrompt {
    pub text: String,
    pub images: Vec<String>,
    pub execution_mode: ExecutionMode,
}
pub type DeferredPrompts = Arc<Mutex<HashMap<String, DeferredPrompt>>>;

/// A single-winner handoff between a synchronous Tauri request and the serial
/// GUI worker. If the caller times out, it atomically aborts a still-pending
/// command; a worker that finishes building afterward must not install the
/// stale session. Conversely, once the worker accepts, a racing timeout treats
/// the operation as successful so the App Server never compensates durable
/// state that the runtime already owns.
pub struct RuntimeActivationFence {
    state: AtomicU8,
}

impl RuntimeActivationFence {
    const PENDING: u8 = 0;
    const ACCEPTED: u8 = 1;
    const ABORTED: u8 = 2;

    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(Self::PENDING),
        }
    }

    pub fn accept(&self) -> bool {
        self.state
            .compare_exchange(
                Self::PENDING,
                Self::ACCEPTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn abort(&self) -> bool {
        self.state
            .compare_exchange(
                Self::PENDING,
                Self::ABORTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn is_accepted(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::ACCEPTED
    }

    pub fn is_aborted(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::ABORTED
    }
}

pub type RuntimeActivation = Arc<RuntimeActivationFence>;

/// Process-local coordinator for all commands that can replace the single
/// live GUI runtime.  A fence by itself only solves a timeout race for one
/// request; the coordinator additionally makes a newer New/Resume/Fork or
/// permission rebuild invalidate an older request that is still queued or
/// building.  The mutex is held only around the tiny generation/state
/// transition (or a synchronous commit closure), never across `await`.
#[derive(Default)]
pub struct RuntimeActivationCoordinator {
    state: Mutex<RuntimeActivationCoordinatorState>,
}

#[derive(Default)]
struct RuntimeActivationCoordinatorState {
    generation: u64,
    active: Option<(u64, RuntimeActivation)>,
}

impl RuntimeActivationCoordinator {
    /// Start the newest activation and invalidate an older pending handoff.
    pub fn begin(&self) -> RuntimeActivation {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, previous)) = state.active.take() {
            // An already accepted handoff owns the runtime and must not be
            // turned into a timeout failure. `abort` is therefore deliberately
            // a no-op for ACCEPTED; pending work is the only state invalidated.
            previous.abort();
        }
        state.generation = state.generation.saturating_add(1).max(1);
        let activation = Arc::new(RuntimeActivationFence::new());
        state.active = Some((state.generation, activation.clone()));
        activation
    }

    fn is_active_locked(
        state: &RuntimeActivationCoordinatorState,
        activation: &RuntimeActivation,
    ) -> bool {
        state
            .active
            .as_ref()
            .map(|(generation, current)| {
                *generation == state.generation && Arc::ptr_eq(current, activation)
            })
            .unwrap_or(false)
    }

    /// Return true while this activation is still the newest non-aborted one.
    pub fn is_current(&self, activation: &RuntimeActivation) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::is_active_locked(&state, activation) && !activation.is_aborted()
    }

    /// Atomically accept the handoff if it is still current.  This must happen
    /// immediately before replacing the worker's global agent/workspace state.
    #[allow(dead_code)]
    pub fn accept_if_current(&self, activation: &RuntimeActivation) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if Self::is_active_locked(&state, activation)
            && (activation.is_accepted() || activation.accept())
        {
            return true;
        }
        // Keep the per-request fence terminal so a caller that is still
        // waiting cannot later mistake this stale command for success.
        activation.abort();
        false
    }

    /// Accept and apply a synchronous runtime replacement as one linearizable
    /// operation. A caller uses this after an asynchronous build and puts
    /// every process-global assignment in `operation`; a newer `begin()` then
    /// cannot slip between acceptance and those assignments.
    pub fn commit_if_current<T>(
        &self,
        activation: &RuntimeActivation,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !Self::is_active_locked(&state, activation)
            || activation.is_aborted()
            || (!activation.is_accepted() && !activation.accept())
        {
            activation.abort();
            return None;
        }
        Some(operation())
    }

    /// Abort only this activation's pending state.  A timeout for an older
    /// request must never cancel the newer active token.
    pub fn abort_if_pending(&self, activation: &RuntimeActivation) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if Self::is_active_locked(&state, activation) {
            return activation.abort();
        }
        false
    }

    /// Check that an activation remains the newest non-aborted runtime command.
    /// Every side effect after an await must make this check again instead of
    /// assuming that an earlier acceptance still wins after a newer request.
    pub fn can_proceed(&self, activation: &RuntimeActivation) -> bool {
        self.is_current(activation)
    }

    /// Run one synchronous side effect only while this activation is current.
    /// This closes the check-then-write gap for config persistence without
    /// holding the coordinator lock over asynchronous agent construction.
    pub fn run_if_current<T>(
        &self,
        activation: &RuntimeActivation,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let _state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !Self::is_active_locked(&_state, activation)
            || activation.is_aborted()
            || activation.is_accepted()
        {
            return None;
        }
        Some(operation())
    }

    /// Run a compensating side effect only after this activation has lost the
    /// handoff, while it is still the newest token. Holding the coordinator
    /// lock across the short operation prevents a racing `begin()` or worker
    /// commit from making an old compensation overwrite a newer runtime.
    pub fn run_if_current_and_aborted<T>(
        &self,
        activation: &RuntimeActivation,
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        let _state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !Self::is_active_locked(&_state, activation) || !activation.is_aborted() {
            return None;
        }
        Some(operation())
    }
}

pub struct WorkerLifecycle {
    shutting_down: AtomicBool,
    handles: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl Default for WorkerLifecycle {
    fn default() -> Self {
        Self {
            shutting_down: AtomicBool::new(false),
            handles: Mutex::new(Vec::new()),
        }
    }
}

impl WorkerLifecycle {
    fn accepts_work(&self) -> bool {
        !self.shutting_down.load(Ordering::Acquire)
    }

    fn track(&self, handle: std::thread::JoinHandle<()>) {
        if !self.accepts_work() {
            let _ = handle.join();
            return;
        }
        if let Ok(mut handles) = self.handles.lock() {
            let mut active = Vec::with_capacity(handles.len() + 1);
            for current in handles.drain(..) {
                if current.is_finished() {
                    let _ = current.join();
                } else {
                    active.push(current);
                }
            }
            active.push(handle);
            *handles = active;
        }
    }

    pub fn shutdown_and_join(
        &self,
        tx: &tokio::sync::mpsc::UnboundedSender<Command>,
        cancels: &CancelRegistry,
        pending: &PendingMap,
        questions: &PendingQuestionMap,
    ) {
        if self.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }
        let sessions = cancels
            .lock()
            .map(|registry| {
                registry
                    .iter()
                    .map(|(id, cancel)| (id.clone(), cancel.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (session_id, cancel) in sessions {
            request_cancel(&session_id, &cancel, pending, questions);
        }
        let _ = tx.send(Command::Shutdown);
        loop {
            let handles = self
                .handles
                .lock()
                .map(|mut handles| handles.drain(..).collect::<Vec<_>>())
                .unwrap_or_default();
            if handles.is_empty() {
                break;
            }
            for handle in handles {
                let _ = handle.join();
            }
        }
    }
}
pub type GrantRegistry = Arc<Mutex<HashMap<String, SessionGrants>>>;

static APPROVAL_COUNTER: AtomicU64 = AtomicU64::new(1);
static QUESTION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Request cancellation and release any approval dialog the turn is waiting on.
///
/// The agent loop polls the flag during model/tool work. Resolving approvals as
/// denied is necessary because an approval future otherwise has no opportunity
/// to observe the flag while it is suspended.
pub fn request_cancel(
    session_id: &str,
    cancel: &CancelFlag,
    pending: &PendingMap,
    questions: &PendingQuestionMap,
) -> usize {
    cancel.store(true, Ordering::Release);
    let senders = take_session_pending(pending, session_id);
    let count = senders.len();
    for sender in senders {
        let _ = sender.send(ApprovalDecision::Deny);
    }
    let question_senders = take_session_questions(questions, session_id);
    let question_count = question_senders.len();
    for sender in question_senders {
        let _ = sender.send(None);
    }
    count + question_count
}

fn take_session_pending(
    pending: &PendingMap,
    session_id: &str,
) -> Vec<oneshot::Sender<ApprovalDecision>> {
    let mut pending = pending.lock().unwrap();
    let ids = pending
        .iter()
        .filter_map(|(id, (owner, _))| (owner == session_id).then_some(*id))
        .collect::<Vec<_>>();
    ids.into_iter()
        .filter_map(|id| pending.remove(&id).map(|(_, sender)| sender))
        .collect()
}

fn take_session_questions(
    pending: &PendingQuestionMap,
    session_id: &str,
) -> Vec<oneshot::Sender<Option<String>>> {
    let mut pending = pending.lock().unwrap();
    let ids = pending
        .iter()
        .filter_map(|(id, (owner, _))| (owner == session_id).then_some(*id))
        .collect::<Vec<_>>();
    ids.into_iter()
        .filter_map(|id| pending.remove(&id).map(|(_, sender)| sender))
        .collect()
}

#[cfg(test)]
fn reset_cancel(cancel: &CancelFlag) {
    cancel.store(false, Ordering::Release);
}

/// A request from the UI to the agent thread.
pub enum Command {
    /// A user turn. `images` are absolute paths attached via the file picker;
    /// each becomes a base64 `image_url` block (vision routing). Non-image files
    /// are passed by the UI as `@path` tokens inside `text` (expanded as mentions).
    Prompt {
        session_id: String,
        text: String,
        images: Vec<String>,
        execution_mode: ExecutionMode,
    },
    /// Start a new empty conversation with an id allocated before it enters the
    /// serial worker queue. Project files remain shared; chat and plans do not.
    New {
        id: String,
        workspace: PathBuf,
        harness_profile: String,
        activation: RuntimeActivation,
        completion: SyncSender<Result<(), String>>,
    },
    /// Continue a saved session: reseed the agent from its snapshot, keeping the
    /// same session id (future turns append to it).
    Resume {
        id: String,
        workspace: PathBuf,
        activation: RuntimeActivation,
        completion: SyncSender<Result<(), String>>,
    },
    /// Branch a saved session: reseed a NEW session from the snapshot, leaving
    /// the source untouched (explore an alternative continuation).
    Fork {
        source_id: String,
        target_id: String,
        workspace: PathBuf,
        activation: RuntimeActivation,
        completion: SyncSender<Result<(), String>>,
    },
    /// Change the approval policy live (no session reset) + persist it.
    /// Change the sandbox mode live (no session reset) + persist it. Used by the
    /// "auto-execute" mode (danger-full-access).
    /// Notify the worker that a complete provider route/model transaction was
    /// committed. The current transcript stays in place and the next turn
    /// resolves the new route.
    SetModel(String),
    /// Run an explicitly armed persisted Goal in this existing conversation.
    /// The exact GoalRef is carried through the host queue so a delayed command
    /// cannot accidentally start a replacement Goal for the same Thread.
    ContinueGoal {
        thread_id: String,
        goal: GoalRef,
    },
    Shutdown,
    /// Switch the CC permission mode (plan / default / accept-edits / bypass):
    /// persist it (+ derived sandbox/approval) and rebuild reseeded so the new
    /// gating + plan nudge take effect without losing the conversation.
    SetPermissionMode {
        /// Durable Thread selected when the request was made. Never use the
        /// coordinator's mutable active session as an implicit target: a
        /// navigation command may be ahead of this queued rebuild.
        thread_id: String,
        mode: String,
        activation: RuntimeActivation,
        /// The protocol request does not complete until the worker has either
        /// rebuilt this exact Thread or rejected it as stale.
        completion: SyncSender<Result<(), String>>,
    },
    /// Re-emit the `ready` snapshot (model / sandbox / session id / models /
    /// permission mode). The frontend calls this once its listener is up, since
    /// the agent thread's initial emit can fire before that listener exists.
    RequestReady,
}

/// What the frontend receives on the `ncx://event` channel. `kind` discriminates.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    /// The agent thread is ready (config loaded) — carries a status snapshot.
    Ready {
        model: String,
        provider_id: String,
        provider_protocol: String,
        sandbox: String,
        workspace: String,
        session_id: String,
        models: Vec<String>,
        permission_mode: String,
        reasoning_effort: String,
        /// True when the workspace is the user's home dir / fs root (not a
        /// project). The UI then prompts for a real workspace and blocks prompts
        /// instead of silently operating there (dangerous under full-access).
        needs_workspace: bool,
    },
    /// A streamed chunk of assistant text (append to the in-progress bubble).
    AssistantDelta {
        session_id: String,
        text: String,
    },
    /// A chunk from the provider's explicit reasoning stream.
    ReasoningDelta {
        session_id: String,
        text: String,
    },
    ContextCompacted {
        session_id: String,
        original_chars: usize,
        edited_chars: usize,
        dropped_messages: usize,
        compressed_tool_results: usize,
    },
    OrchestratorStage {
        session_id: String,
        stage: String,
        detail: String,
    },
    OrchestratorActivity {
        session_id: String,
        worker: usize,
        tool: String,
        phase: String,
        failure: Option<String>,
    },
    GoalRunStarted {
        session_id: String,
    },
    HumanTurnStarted {
        session_id: String,
    },
    /// Assistant's final visible text (finalize the streamed bubble).
    Assistant {
        session_id: String,
        text: String,
        model: String,
        confirmed_model: Option<String>,
    },
    /// A tool is about to run.
    ToolStart {
        session_id: String,
        name: String,
        args: String,
    },
    /// A tool finished.
    ToolResult {
        session_id: String,
        name: String,
        result: String,
    },
    /// An escalated action needs the user's yes/no. Answer via the `approve`
    /// command with this `id`.
    Approval {
        session_id: String,
        id: u64,
        command: String,
        reason: String,
        cwd: String,
        details: String,
    },
    Question {
        session_id: String,
        id: u64,
        question: String,
        options: Vec<String>,
        allow_free_text: bool,
    },
    /// The turn finished.
    Done {
        session_id: String,
        final_text: String,
        stop_reason: String,
        usage: Value,
    },
    /// A compact title was generated and persisted for a newly completed session.
    SessionTitle {
        session_id: String,
        title: String,
    },
    /// A session was resumed/forked — the UI should replace its transcript with
    /// these restored messages.
    Loaded {
        session_id: String,
        messages: Vec<UiMsg>,
    },
    /// Fatal setup/turn error.
    Error {
        session_id: String,
        message: String,
    },
}

/// A restored conversation message for the `loaded` event.
#[derive(Clone, Serialize)]
pub struct UiMsg {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_model: Option<String>,
}

pub(crate) fn emit(app: &AppHandle, ev: UiEvent) {
    let _ = app.emit(EVENT, ev);
}

pub(crate) fn emit_protocol_outcome(app: &AppHandle, outcome: &ncx_app_server::DispatchOutcome) {
    for event in &outcome.events {
        let _ = app.emit(PROTOCOL_EVENT, event);
    }
}

fn should_generate_session_title(is_first_turn: bool, stop_reason: &str) -> bool {
    is_first_turn && stop_reason == "completed"
}

fn fallback_session_title(request: &str) -> Option<String> {
    let normalized = request.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut title = normalized.trim();
    if title.is_empty() {
        return None;
    }
    if matches!(
        title,
        "你好" | "您好" | "在吗" | "嗨" | "hello" | "Hello" | "hi" | "Hi"
    ) {
        return Some("日常问候".into());
    }
    for prefix in [
        "可以帮我",
        "能不能帮我",
        "能否帮我",
        "请帮我",
        "麻烦帮我",
        "帮我",
    ] {
        if let Some(stripped) = title.strip_prefix(prefix) {
            title = stripped.trim_start_matches([' ', '，', ',', '：', ':']);
            break;
        }
    }
    let mut compact = title
        .trim_matches([' ', '。', '.', '！', '!', '？', '?', '，', ',', '：', ':'])
        .chars()
        .take(24)
        .collect::<String>();
    if title.chars().count() > 24 {
        compact.push('…');
    }
    (!compact.is_empty()).then_some(compact)
}

fn rename_session(
    app: &AppHandle,
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
    title: &str,
) -> bool {
    let outcome = ThreadId::new(session_id.to_string())
        .ok()
        .and_then(|thread_id| {
            app_server
                .dispatch(ClientRequest::ThreadRename {
                    thread_id,
                    title: title.to_string(),
                })
                .ok()
        });
    if let Some(outcome) = &outcome {
        emit_protocol_outcome(app, outcome);
        emit(
            app,
            UiEvent::SessionTitle {
                session_id: session_id.to_string(),
                title: title.to_string(),
            },
        );
    }
    outcome.is_some()
}

/// Build the loop's event sink (forwards [`LoopEvent`]s to the frontend). A
/// fresh one is needed after every (re)build of the agent.
fn make_sink(
    app: AppHandle,
    session_id: String,
    app_server: Option<Arc<AppServer<JsonThreadStore>>>,
    turn_id: Option<TurnId>,
) -> Box<dyn FnMut(LoopEvent)> {
    let thread_id = ThreadId::new(session_id.clone()).ok();
    let mut latest_tool_call = None;
    Box::new(move |ev: LoopEvent| {
        let (ui, item) = match ev {
            LoopEvent::AssistantDelta(text) => (
                UiEvent::AssistantDelta {
                    session_id: session_id.clone(),
                    text,
                },
                None,
            ),
            LoopEvent::ReasoningDelta(text) => (
                UiEvent::ReasoningDelta {
                    session_id: session_id.clone(),
                    text,
                },
                None,
            ),
            LoopEvent::ContextCompacted(stats) => {
                let ui = UiEvent::ContextCompacted {
                    session_id: session_id.clone(),
                    original_chars: stats.original_chars,
                    edited_chars: stats.edited_chars,
                    dropped_messages: stats.dropped_messages,
                    compressed_tool_results: stats.compressed_tool_results,
                };
                (
                    ui,
                    Some(ThreadItem::ContextCompaction {
                        id: protocol_item_id("compact"),
                        summary: format!(
                            "{} -> {} chars",
                            stats.original_chars, stats.edited_chars
                        ),
                        dropped_items: u32::try_from(stats.dropped_messages).unwrap_or(u32::MAX),
                    }),
                )
            }
            LoopEvent::AssistantText {
                text,
                model,
                confirmed_model,
            } => {
                let item = ThreadItem::AssistantMessage {
                    id: protocol_item_id("assistant"),
                    text: text.clone(),
                    model: Some(model.clone()),
                    confirmed_model: confirmed_model.clone(),
                };
                (
                    UiEvent::Assistant {
                        session_id: session_id.clone(),
                        text,
                        model,
                        confirmed_model,
                    },
                    Some(item),
                )
            }
            LoopEvent::ToolStart { name, args } => {
                let id = protocol_item_id("tool-call");
                latest_tool_call = Some(id.clone());
                let arguments =
                    serde_json::from_str(&args).unwrap_or_else(|_| json!({ "raw": args }));
                let item = ThreadItem::ToolCall {
                    id,
                    name: name.clone(),
                    arguments,
                };
                (
                    UiEvent::ToolStart {
                        session_id: session_id.clone(),
                        name,
                        args,
                    },
                    Some(item),
                )
            }
            LoopEvent::ToolResult { name, result } => {
                if let (Some(server), Some(thread_id), Some(turn_id)) =
                    (app_server.as_ref(), thread_id.clone(), turn_id.clone())
                {
                    for artifact in media_artifact_items(&name, &result) {
                        if let Ok(outcome) = server.dispatch(ClientRequest::ItemAppend {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            item: artifact,
                        }) {
                            emit_protocol_outcome(&app, &outcome);
                        }
                    }
                }
                let call_id = latest_tool_call
                    .take()
                    .unwrap_or_else(|| protocol_item_id("tool-call"));
                let item = ThreadItem::ToolResult {
                    id: protocol_item_id("tool-result"),
                    call_id,
                    output: result.clone(),
                    success: true,
                };
                (
                    UiEvent::ToolResult {
                        session_id: session_id.clone(),
                        name,
                        result,
                    },
                    Some(item),
                )
            }
        };
        if let (Some(server), Some(thread_id), Some(turn_id), Some(item)) = (
            app_server.as_ref(),
            thread_id.clone(),
            turn_id.clone(),
            item,
        ) {
            if let Ok(outcome) = server.dispatch(ClientRequest::ItemAppend {
                thread_id,
                turn_id,
                item,
            }) {
                emit_protocol_outcome(&app, &outcome);
            }
        }
        emit(&app, ui);
    })
}

fn media_artifact_items(tool_name: &str, result: &str) -> Vec<ThreadItem> {
    let kind = match tool_name {
        "generate_image" => "image",
        "generate_video" => "video",
        _ => return Vec::new(),
    };
    let Ok(payload) = serde_json::from_str::<Value>(result) else {
        return Vec::new();
    };
    if payload.get("status").and_then(Value::as_str) != Some("succeeded") {
        return Vec::new();
    }
    payload
        .get("urls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .enumerate()
        .map(|(index, url)| ThreadItem::Artifact {
            id: protocol_item_id("artifact"),
            kind: kind.to_string(),
            name: format!(
                "{} {}",
                if kind == "image" {
                    "生成图片"
                } else {
                    "生成视频"
                },
                index + 1
            ),
            url: url.to_string(),
        })
        .collect()
}

fn protocol_item_id(kind: &str) -> ItemId {
    ItemId::new(format!("{kind}-{}", new_session_id())).expect("generated item id is non-empty")
}

/// Tell the UI which model / sandbox / workspace / session is now active.
fn emit_ready(app: &AppHandle, workspace: &std::path::Path, session_id: &str) {
    if let Ok(cfg) = load_config(Overrides {
        workspace: Some(workspace.to_path_buf()),
        ..Default::default()
    }) {
        let provider_id = visible_provider_id(&cfg);
        emit(
            app,
            UiEvent::Ready {
                model: cfg.model,
                provider_id,
                provider_protocol: cfg.provider_protocol,
                sandbox: cfg.sandbox_mode,
                workspace: display_path(workspace),
                session_id: session_id.to_string(),
                models: cfg.available_models,
                permission_mode: cfg.permission_mode,
                reasoning_effort: cfg.reasoning_effort,
                needs_workspace: is_unsafe_workspace(workspace),
            },
        );
    }
}

fn visible_provider_id(cfg: &Config) -> String {
    if cfg.active_provider_id != "legacy" {
        return cfg.active_provider_id.clone();
    }
    let active_base = cfg.base_url.trim_end_matches('/');
    crate::model_catalog::catalog()
        .into_iter()
        .find(|provider| {
            provider.models.iter().any(|model| {
                model
                    .base_url
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(active_base)
            })
        })
        .map(|provider| provider.id)
        .unwrap_or_else(|| "manual".into())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// File where the GUI remembers the last chosen workspace, so it doesn't fall
/// back to the launch cwd (often the user's home) on every start.
pub fn last_workspace_file() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".nanocodex").join("gui_workspace.txt"))
}

/// Persist the chosen workspace (best-effort).
pub fn save_last_workspace(path: &std::path::Path) {
    if let Some(f) = last_workspace_file() {
        if let Some(parent) = f.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(f, display_path(path));
    }
}

/// The last chosen workspace, if it still exists as a directory.
pub fn load_last_workspace() -> Option<PathBuf> {
    let f = last_workspace_file()?;
    let raw = std::fs::read_to_string(f).ok()?;
    let p = PathBuf::from(strip_verbatim_prefix(raw.trim()));
    p.is_dir().then_some(p)
}

/// Return a stable user-facing path without Windows' verbatim namespace prefix.
pub fn display_path(path: &Path) -> String {
    strip_verbatim_prefix(&path.to_string_lossy())
}

fn strip_verbatim_prefix(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        raw.strip_prefix(r"\\?\").unwrap_or(raw).to_string()
    }
}

/// True when `dir` is the user's home directory or the filesystem root — not a
/// project. Operating there silently (especially under danger-full-access) is a
/// foot-gun, so the UI prompts for a real workspace instead.
pub fn is_unsafe_workspace(dir: &std::path::Path) -> bool {
    is_unsafe_against(dir, home_dir().as_deref())
}

/// Core of [`is_unsafe_workspace`], with `home` injected so it is testable.
fn is_unsafe_against(dir: &std::path::Path, home: Option<&std::path::Path>) -> bool {
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if let Some(home) = home {
        let home_c = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
        if canon == home_c {
            return true;
        }
    }
    canon.parent().is_none()
}

/// Approval handler that round-trips through the frontend modal.
struct GuiApprover {
    app: AppHandle,
    active_session: Arc<Mutex<String>>,
    pending: PendingMap,
}

struct GuiQuestioner {
    app: AppHandle,
    active_session: Arc<Mutex<String>>,
    pending: PendingQuestionMap,
}

struct GuiOrchestratorControl {
    app: AppHandle,
    session_id: String,
    cancel: CancelFlag,
}

impl OrchestratorControl for GuiOrchestratorControl {
    fn emit(&self, event: OrchestratorEvent) {
        emit(
            &self.app,
            UiEvent::OrchestratorStage {
                session_id: self.session_id.clone(),
                stage: format!("{:?}", event.stage).to_lowercase(),
                detail: event.detail,
            },
        );
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}

#[async_trait(?Send)]
impl UserQuestionHandler for GuiQuestioner {
    async fn request(&self, request: UserQuestionRequest) -> Option<String> {
        let id = QUESTION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let session_id = current_session_id(&self.active_session);
        self.pending
            .lock()
            .unwrap()
            .insert(id, (session_id.clone(), tx));
        emit(
            &self.app,
            UiEvent::Question {
                session_id,
                id,
                question: request.question,
                options: request.options,
                allow_free_text: request.allow_free_text,
            },
        );
        rx.await.unwrap_or(None)
    }
}

#[async_trait(?Send)]
impl ApprovalHandler for GuiApprover {
    async fn request(&self, req: ApprovalRequest) -> ApprovalDecision {
        let id = APPROVAL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let session_id = current_session_id(&self.active_session);
        self.pending
            .lock()
            .unwrap()
            .insert(id, (session_id.clone(), tx));
        emit(
            &self.app,
            UiEvent::Approval {
                session_id,
                id,
                command: req.command,
                reason: req.reason,
                cwd: req.cwd,
                details: req.details,
            },
        );
        // Window closed / channel dropped -> treat as denied (fail safe).
        rx.await.unwrap_or(ApprovalDecision::Deny)
    }
}

/// Build the agent loop and its workspace from the resolved config.
///
/// `seed` reseeds the conversation: `(session_id, messages)` — used by Resume
/// (keep the id) and Fork (a new id). `None` starts a fresh session.
fn latest_plan_from_messages(messages: &[Value]) -> Vec<Value> {
    let mut crossed_turn_boundary = false;
    for message in messages.iter().rev() {
        if message.get("role").and_then(Value::as_str) == Some("user")
            || message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.trim().ends_with("Stopped by user."))
        {
            crossed_turn_boundary = true;
        }
        let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        for call in calls.iter().rev() {
            let Some(function) = call.get("function") else {
                continue;
            };
            if function.get("name").and_then(Value::as_str) != Some("update_plan") {
                continue;
            }
            let Some(arguments) = function.get("arguments") else {
                continue;
            };
            let parsed = match arguments {
                Value::String(raw) => serde_json::from_str::<Value>(raw).ok(),
                Value::Object(_) => Some(arguments.clone()),
                _ => None,
            };
            if let Some(plan) = parsed
                .as_ref()
                .and_then(|value| value.get("plan"))
                .and_then(Value::as_array)
            {
                return if crossed_turn_boundary {
                    Vec::new()
                } else {
                    plan.clone()
                };
            }
        }
    }
    Vec::new()
}

fn current_session_id(active: &Arc<Mutex<String>>) -> String {
    active.lock().map(|id| id.clone()).unwrap_or_default()
}

fn set_active_session(active: &Arc<Mutex<String>>, session_id: &str) {
    if let Ok(mut id) = active.lock() {
        *id = session_id.to_string();
    }
}

fn spawn_title_generation(
    app: AppHandle,
    app_server: Arc<AppServer<JsonThreadStore>>,
    session_id: String,
    workspace: PathBuf,
    request: String,
) {
    if let Some(title) = fallback_session_title(&request) {
        rename_session(&app, app_server.as_ref(), &session_id, &title);
    }
    let _ = std::thread::Builder::new()
        .name("ncx-title".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async move {
                let Ok(cfg) = load_config(Overrides {
                    workspace: Some(workspace),
                    ..Default::default()
                }) else {
                    return;
                };
                let provider = ConfiguredHarnessRuntime::from_config(cfg).primary_provider();
                let Some(title) = suggest_title_with_provider(provider.as_ref(), &request).await
                else {
                    return;
                };
                rename_session(&app, app_server.as_ref(), &session_id, &title);
            });
        });
}

struct GuiGoalToolService {
    app_server: Arc<AppServer<JsonThreadStore>>,
    thread_id: ThreadId,
}

impl GuiGoalToolService {
    fn dispatch(&self, request: ClientRequest) -> Result<ncx_protocol::GoalView, String> {
        let outcome = self
            .app_server
            .dispatch(request)
            .map_err(|error| error.to_string())?;
        match outcome.response.payload {
            ResponsePayload::Goal(Some(goal)) => Ok(goal),
            ResponsePayload::Goal(None) => Err("current goal was not found".into()),
            _ => Err("App Server returned an unexpected goal response".into()),
        }
    }
}

impl GoalToolService for GuiGoalToolService {
    fn get(&self) -> Result<Option<ncx_protocol::GoalView>, String> {
        let outcome = self
            .app_server
            .dispatch(ClientRequest::GoalRead {
                thread_id: self.thread_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        match outcome.response.payload {
            ResponsePayload::Goal(goal) => Ok(goal),
            _ => Err("App Server returned an unexpected goal response".into()),
        }
    }

    fn create(
        &self,
        objective: String,
        max_goal_rounds: u32,
    ) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalCreate {
            thread_id: self.thread_id.clone(),
            objective,
            max_goal_rounds,
        })
    }

    fn edit(
        &self,
        goal: ncx_protocol::GoalRef,
        objective: String,
        max_goal_rounds: u32,
    ) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalEdit {
            thread_id: self.thread_id.clone(),
            goal,
            objective,
            max_goal_rounds,
        })
    }

    fn pause(&self, goal: ncx_protocol::GoalRef) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalPause {
            thread_id: self.thread_id.clone(),
            goal,
        })
    }

    fn resume(&self, goal: ncx_protocol::GoalRef) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalResume {
            thread_id: self.thread_id.clone(),
            goal,
        })
    }

    fn complete(&self, goal: ncx_protocol::GoalRef) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalComplete {
            thread_id: self.thread_id.clone(),
            goal,
        })
    }

    fn block(
        &self,
        goal: ncx_protocol::GoalRef,
        reason: ncx_protocol::GoalBlockReason,
    ) -> Result<ncx_protocol::GoalView, String> {
        self.dispatch(ClientRequest::GoalBlock {
            thread_id: self.thread_id.clone(),
            goal,
            reason,
        })
    }
}

async fn build_agent(
    approver: Rc<dyn ApprovalHandler>,
    questioner: Rc<dyn UserQuestionHandler>,
    seed: Option<(String, Vec<Value>)>,
    grants: Rc<RefCell<SessionGrants>>,
    workspace: PathBuf,
    harness_profile: Option<String>,
    app_server: Arc<AppServer<JsonThreadStore>>,
) -> Result<(AgentLoop, PathBuf, String, PathBuf), String> {
    let restored_plan = seed
        .as_ref()
        .map(|(_, messages)| latest_plan_from_messages(messages))
        .unwrap_or_default();
    let (session_id, seed_messages) = match seed {
        Some((id, messages)) => (id, Some(messages)),
        None => (new_session_id(), None),
    };
    let overrides = Overrides {
        workspace: Some(workspace),
        ..Default::default()
    };
    let cfg = load_config(overrides).map_err(|e| e.to_string())?;
    cfg.validate().map_err(|e| e.to_string())?;

    let mut runtime = ConfiguredHarnessRuntime::from_config(cfg.clone());
    if let Some(profile) = harness_profile {
        runtime = runtime.with_harness_profile(profile);
    }
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    // Memory is recalled per prompt by AgentLoop (query-scoped), not dumped here.
    // Workspace-only: do NOT inject the developer's global ~/.claude/~/.codex
    // files (their handoff protocol would make a plain "hi" read HANDOFF.md etc.).
    let instructions = load_workspace_instructions(&cfg.workspace, 16_000);
    let skills = discover_skills(&cfg.workspace);
    let plan_note = if runtime.profile().permissions.plan_mode {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let mut hooks = cfg.hooks.clone();
    hooks.extend(discover_codex_hooks(&cfg.workspace)?);
    let sources = RuntimeContextSources::new(instructions, skills, plan_note)
        .with_memory(memory)
        .with_hooks(hooks);
    let bindings = RuntimeHostBindings {
        approver: Some(approver),
        questioner: Some(questioner),
        grants: Some(grants),
        goal_service: Some(Rc::new(GuiGoalToolService {
            app_server,
            thread_id: ThreadId::new(session_id.clone()).map_err(|error| error.to_string())?,
        })),
    };
    let mut tools = runtime.build_tools(cfg.workspace.clone(), sources, bindings)?;
    if !restored_plan.is_empty() {
        tools.ctx.plan.replace(restored_plan);
    }
    let mcp_servers = discover_codex_mcp_servers(&cfg.workspace)?;
    if !mcp_servers.is_empty() {
        let mut prepared = Vec::new();
        for server in &mcp_servers {
            append_prepared_mcp_tools(
                server,
                prepare_mcp_server_tools(&server.name, &server.command, &server.args, &server.env)
                    .await,
                &mut prepared,
            );
        }
        let active_tools = prepared.len();
        tools.replace_tools(&[], prepared)?;
        tools.replace_service(
            "mcp",
            Rc::new(McpServiceDescriptor {
                enabled: true,
                configured_servers: mcp_servers.len(),
                active_tools,
            }),
        );
    }
    let system_prompt = tools
        .service::<ContextServiceDescriptor>("context")
        .ok_or_else(|| "Harness Context 服务未启用".to_string())?
        .assemble(runtime_system_prompt(
            &cfg.active_provider_id,
            &cfg.provider_protocol,
            &cfg.model,
        ));
    let log_dir = cfg.workspace.join(".nanocodex").join("sessions");
    std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
    let log_path = session_log_path(&cfg.workspace, &session_id);
    let session = match seed_messages {
        Some(messages) => Session::fork(system_prompt, messages, Some(log_path.clone())),
        None => Session::with_log(system_prompt, Some(log_path.clone())),
    };
    let agent = runtime
        .profile()
        .clone()
        .apply(AgentLoop::from_runtime_services(tools, session)?);
    Ok((agent, cfg.workspace.clone(), session_id, log_path))
}

fn append_prepared_mcp_tools(
    server: &McpServerConfig,
    result: Result<Vec<Box<dyn ncx_core::Tool>>, String>,
    prepared: &mut Vec<Box<dyn ncx_core::Tool>>,
) {
    match result {
        Ok(mut server_tools) => prepared.append(&mut server_tools),
        Err(error) => eprintln!("跳过无法启动的 Codex MCP server '{}': {error}", server.name),
    }
}

fn runtime_system_prompt(provider_id: &str, protocol: &str, model: &str) -> String {
    format!(
        "{SYSTEM_PROMPT} Runtime route metadata from the local client: provider ID = {:?}, protocol = {:?}, requested model ID = {:?}. \
         When the user asks which model or provider is active, report these exact client-selected values. \
         Do not replace the requested model ID with a family name inferred from your training identity, and do not claim \
         that the requested ID proves the upstream vendor's internal implementation. If response metadata is available in \
         the conversation UI, distinguish its confirmed model ID from the requested model ID.",
        provider_id, protocol, model
    )
}

fn session_log_path(workspace: &Path, session_id: &str) -> PathBuf {
    workspace
        .join(".nanocodex")
        .join("sessions")
        .join(format!("{}.jsonl", safe_session_file_stem(session_id)))
}

pub(crate) fn safe_session_file_stem(session_id: &str) -> String {
    session_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn protocol_thread_seed(
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
) -> Option<(Vec<Value>, Option<String>)> {
    let thread_id = ThreadId::new(session_id.to_string()).ok()?;
    let outcome = app_server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .ok()?;
    let ResponsePayload::Thread(thread) = outcome.response.payload else {
        return None;
    };
    let messages = app_server
        .dispatch(ClientRequest::ThreadModelContextRead {
            thread_id: thread.metadata.id.clone(),
        })
        .ok()
        .and_then(|outcome| match outcome.response.payload {
            ResponsePayload::ModelContext(Some(context)) => Some(context.messages),
            _ => None,
        })
        .unwrap_or_else(|| protocol_thread_messages(&thread, false));
    Some((messages, Some(thread.metadata.workspace)))
}

/// Resolve a durable Thread workspace for background workers. Never fall back
/// to the process CWD: another GUI navigation may legitimately change that
/// global value while this session is still queued or running.
fn protocol_thread_workspace(
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
) -> Result<PathBuf, String> {
    let Some((_, Some(stored_workspace))) = protocol_thread_seed(app_server, session_id) else {
        return Err(format!("会话 {session_id} 缺少持久工作区"));
    };
    let workspace = PathBuf::from(strip_verbatim_prefix(&stored_workspace));
    if !workspace.is_dir() {
        return Err(format!(
            "会话 {session_id} 的工作区不存在：{}",
            workspace.display()
        ));
    }
    Ok(workspace)
}

/// Resolve the only session a queued permission-mode rebuild may touch.
///
/// Navigation commands and configuration commands share the coordinator queue,
/// but a workspace transition can enqueue a different session before an older
/// permission request arrives. Requiring both identities to match keeps that
/// stale request from rebuilding the newly active session. The workspace is
/// intentionally read from the durable Thread rather than process CWD.
fn permission_mode_rebuild_input(
    app_server: &AppServer<JsonThreadStore>,
    active_session_id: &str,
    target_session_id: &str,
) -> Result<(Vec<Value>, PathBuf, String), String> {
    if active_session_id != target_session_id {
        return Err(format!(
            "会话已切换，拒绝将权限模式请求从 {target_session_id} 应用到 {active_session_id}"
        ));
    }
    let Some((messages, _)) = protocol_thread_seed(app_server, target_session_id) else {
        return Err(format!(
            "当前会话 {target_session_id} 缺少持久工作区，拒绝重建权限模式。"
        ));
    };
    let workspace = protocol_thread_workspace(app_server, target_session_id)?;
    let harness_profile = protocol_thread_profile(app_server, target_session_id);
    Ok((messages, workspace, harness_profile))
}

fn protocol_thread_profile(app_server: &AppServer<JsonThreadStore>, session_id: &str) -> String {
    let Ok(thread_id) = ThreadId::new(session_id.to_string()) else {
        return "full".into();
    };
    app_server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .ok()
        .and_then(|outcome| match outcome.response.payload {
            ResponsePayload::Thread(thread) => Some(thread.metadata.harness_profile),
            _ => None,
        })
        .unwrap_or_else(|| "full".into())
}

/// Restore the visible transcript from durable thread turns, never from the
/// compacted model context. Context compaction is an LLM optimization and must
/// not remove messages from the user's conversation history.
fn protocol_thread_ui(
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
) -> Option<Vec<UiMsg>> {
    let thread_id = ThreadId::new(session_id.to_string()).ok()?;
    let outcome = app_server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .ok()?;
    let ResponsePayload::Thread(thread) = outcome.response.payload else {
        return None;
    };
    Some(snapshot_to_ui(&protocol_thread_messages(&thread, true)))
}

fn latest_protocol_thread_seed(
    app_server: &AppServer<JsonThreadStore>,
    workspace: &Path,
) -> Option<(String, Vec<Value>)> {
    let outcome = app_server
        .dispatch(ClientRequest::ThreadList {
            include_archived: false,
        })
        .ok()?;
    let ResponsePayload::Threads(threads) = outcome.response.payload else {
        return None;
    };
    threads.into_iter().find_map(|metadata| {
        let id = metadata.id.to_string();
        let (messages, stored_workspace) = protocol_thread_seed(app_server, &id)?;
        (stored_workspace.as_deref() == Some(workspace.to_string_lossy().as_ref()))
            .then_some((id, messages))
    })
}

fn ensure_protocol_thread(
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
    workspace: &Path,
) -> Result<(), String> {
    let thread_id = ThreadId::new(session_id.to_string()).map_err(|error| error.to_string())?;
    if app_server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .is_ok()
    {
        return Ok(());
    }
    app_server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id),
            workspace: workspace.display().to_string(),
            title: "(no prompt yet)".to_string(),
            harness_profile: "full".to_string(),
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn protocol_thread_messages(thread: &Thread, include_ui_metadata: bool) -> Vec<Value> {
    thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter_map(|item| match item {
            ThreadItem::UserMessage { text, .. } => {
                Some(json!({"role": "user", "content": text}))
            }
            ThreadItem::GoalMessage { text, .. } if !include_ui_metadata => {
                Some(json!({"role": "user", "content": text}))
            }
            ThreadItem::GoalMessage { .. } => None,
            ThreadItem::AssistantMessage {
                text,
                model,
                confirmed_model,
                ..
            } => Some(if include_ui_metadata {
                json!({
                    "role": "assistant",
                    "content": text,
                    "_ncx_model": model,
                    "_ncx_confirmed_model": confirmed_model,
                })
            } else {
                json!({"role": "assistant", "content": text})
            }),
            ThreadItem::ToolCall {
                id,
                name,
                arguments,
            } => Some(json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": id.as_str(),
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(arguments).unwrap_or_else(|_| "{}".into())
                    }
                }]
            })),
            ThreadItem::ToolResult {
                call_id, output, ..
            } => Some(json!({
                "role": "tool",
                "tool_call_id": call_id.as_str(),
                "content": output
            })),
            ThreadItem::ContextCompaction { summary, .. } => Some(json!({
                "role": "user",
                "content": format!("{COMPACTED_HISTORY_PREFIX}；协议存储]\n{summary}")
            })),
            ThreadItem::Reasoning { .. } | ThreadItem::Artifact { .. } => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn spawn_turn_worker(
    app: AppHandle,
    app_server: Arc<AppServer<JsonThreadStore>>,
    pending: PendingMap,
    questions: PendingQuestionMap,
    cancels: CancelRegistry,
    running: RunningSessions,
    deferred_prompts: DeferredPrompts,
    lifecycle: Arc<WorkerLifecycle>,
    session_grants: GrantRegistry,
    session_id: String,
    workspace: PathBuf,
    messages: Vec<Value>,
    text: String,
    images: Vec<String>,
    execution_mode: ExecutionMode,
    harness_profile: String,
) {
    if !lifecycle.accepts_work() {
        emit(
            &app,
            UiEvent::Error {
                session_id: session_id.clone(),
                message: "应用正在关闭，无法启动会话执行线程。".into(),
            },
        );
        return;
    }
    let inserted = claim_session(&running, &session_id, SessionRunKind::Human);
    if !inserted {
        emit(
            &app,
            UiEvent::Error {
                session_id,
                message: "该会话仍在执行中，请等待完成或先停止它。".into(),
            },
        );
        return;
    }

    let turn_id =
        TurnId::new(format!("turn-{}", new_session_id())).expect("generated turn id is non-empty");
    let protocol_turn = match ProtocolTurnGuard::start(ProtocolTurnStart {
        app: Some(app.clone()),
        server: app_server.clone(),
        session_id: &session_id,
        workspace: &workspace,
        turn_id: turn_id.clone(),
        user_text: &text,
        execution_mode,
        harness_profile: &harness_profile,
    }) {
        Ok(turn) => turn,
        Err(message) => {
            if let Ok(mut sessions) = running.lock() {
                sessions.remove(&session_id);
            }
            emit(
                &app,
                UiEvent::Error {
                    session_id,
                    message,
                },
            );
            return;
        }
    };

    let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    if let Ok(mut registry) = cancels.lock() {
        registry.insert(session_id.clone(), cancel.clone());
    }

    let cleanup_cancels = cancels.clone();
    let cleanup_running = running.clone();
    let cleanup_session_id = session_id.clone();
    let failure_app = app.clone();
    let thread_lifecycle = lifecycle.clone();
    let spawned = std::thread::Builder::new()
        .name(format!(
            "ncx-turn-{}",
            session_id.chars().take(8).collect::<String>()
        ))
        .spawn(move || {
            let goal_pending = pending.clone();
            let goal_questions = questions.clone();
            let goal_cancels = cancels.clone();
            let goal_running = running.clone();
            let goal_deferred = deferred_prompts.clone();
            let goal_lifecycle = thread_lifecycle.clone();
            let goal_grants = session_grants.clone();
            let goal_app = app.clone();
            let goal_server = app_server.clone();
            let goal_session_id = session_id.clone();
            let mut protocol_turn = protocol_turn;
            let finish = || {
                if let Ok(mut registry) = cancels.lock() {
                    registry.remove(&session_id);
                }
                if let Ok(mut sessions) = running.lock() {
                    sessions.remove(&session_id);
                }
            };
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                emit(
                    &app,
                    UiEvent::Error {
                        session_id: session_id.clone(),
                        message: "无法创建会话执行线程。".into(),
                    },
                );
                finish();
                return;
            };

            rt.block_on(async {
                let active_session = Arc::new(Mutex::new(session_id.clone()));
                let approver: Rc<dyn ApprovalHandler> = Rc::new(GuiApprover {
                    app: app.clone(),
                    active_session: active_session.clone(),
                    pending,
                });
                let questioner: Rc<dyn UserQuestionHandler> = Rc::new(GuiQuestioner {
                    app: app.clone(),
                    active_session,
                    pending: questions,
                });
                let initial_grants = session_grants
                    .lock()
                    .ok()
                    .and_then(|registry| registry.get(&session_id).cloned())
                    .unwrap_or_default();
                let grants = Rc::new(RefCell::new(initial_grants));
                if execution_mode == ExecutionMode::Orchestrator {
                    orchestrated_turn::run(
                        app.clone(),
                        app_server.clone(),
                        session_grants.clone(),
                        session_id.clone(),
                        workspace.clone(),
                        messages,
                        text,
                        images,
                        cancel.clone(),
                        approver,
                        questioner,
                        grants,
                        &mut protocol_turn,
                        harness_profile.clone(),
                    )
                    .await;
                    return;
                }
                let built = build_agent(
                    approver,
                    questioner,
                    Some((session_id.clone(), messages)),
                    grants.clone(),
                    workspace.clone(),
                    Some(harness_profile),
                    app_server.clone(),
                )
                .await;
                let (mut agent, _, _, _) = match built {
                    Ok(value) => value,
                    Err(message) => {
                        emit(
                            &app,
                            UiEvent::Error {
                                session_id: session_id.clone(),
                                message,
                            },
                        );
                        return;
                    }
                };
                agent.set_event_sink(make_sink(
                    app.clone(),
                    session_id.clone(),
                    Some(app_server.clone()),
                    Some(turn_id.clone()),
                ));
                let is_first_turn = agent
                    .session
                    .messages
                    .iter()
                    .all(|message| message.get("role").and_then(Value::as_str) != Some("user"));
                let expanded = expand_file_mentions(&text, &workspace);
                save_auto_checkpoint(&workspace, &expanded);
                if let Err(message) = validate_image_attachments(&agent.tools, &images) {
                    emit(
                        &app,
                        UiEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    );
                    return;
                }
                let user_input = match build_image_user_input(&expanded, &images) {
                    Ok(value) => value,
                    Err(message) => {
                        emit(
                            &app,
                            UiEvent::Error {
                                session_id: session_id.clone(),
                                message,
                            },
                        );
                        return;
                    }
                };
                let is_cancelled = || cancel.load(Ordering::Acquire);
                let result = agent.run_turn(user_input, Some(&is_cancelled)).await;
                let estimated_cost = agent.estimated_cost(&result);
                if let Ok(mut registry) = session_grants.lock() {
                    registry.insert(session_id.clone(), grants.borrow().clone());
                }
                emit(
                    &app,
                    UiEvent::Done {
                        session_id: session_id.clone(),
                        final_text: result.final_text.clone(),
                        stop_reason: result.stop_reason.clone(),
                        usage: serde_json::to_value(&result.usage).unwrap_or(Value::Null),
                    },
                );
                match app_server.dispatch(ClientRequest::ThreadModelContextReplace {
                    thread_id: protocol_turn.thread_id.clone(),
                    messages: agent.session.messages.clone(),
                }) {
                    Ok(outcome) => emit_protocol_outcome(&app, &outcome),
                    Err(error) => emit(
                        &app,
                        UiEvent::Error {
                            session_id: session_id.clone(),
                            message: format!("保存模型上下文失败：{error}"),
                        },
                    ),
                }
                let (currency, estimated_cost) = estimated_cost
                    .map(|(currency, cost)| (Some(currency), Some(cost)))
                    .unwrap_or((None, None));
                protocol_turn.complete_with_usage(
                    &result.stop_reason,
                    TurnUsage {
                        tokens: result.usage.clone(),
                        estimated_cost,
                        currency,
                    },
                );
                if should_generate_session_title(is_first_turn, &result.stop_reason) {
                    spawn_title_generation(
                        app.clone(),
                        app_server.clone(),
                        session_id.clone(),
                        workspace.clone(),
                        text,
                    );
                }
            });
            finish();
            if let Some(goal_ref) = armed_goal_ref(&goal_server, &goal_session_id) {
                spawn_goal_worker(
                    goal_app,
                    goal_server,
                    goal_pending,
                    goal_questions,
                    goal_cancels,
                    goal_running,
                    goal_deferred,
                    goal_lifecycle,
                    goal_grants,
                    goal_session_id,
                    goal_ref,
                );
            }
        });
    if let Ok(handle) = spawned {
        lifecycle.track(handle);
    } else {
        if let Ok(mut registry) = cleanup_cancels.lock() {
            registry.remove(&cleanup_session_id);
        }
        if let Ok(mut sessions) = cleanup_running.lock() {
            sessions.remove(&cleanup_session_id);
        }
        emit(
            &failure_app,
            UiEvent::Error {
                session_id: cleanup_session_id,
                message: "无法启动会话执行线程。".into(),
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_goal_worker(
    app: AppHandle,
    app_server: Arc<AppServer<JsonThreadStore>>,
    pending: PendingMap,
    questions: PendingQuestionMap,
    cancels: CancelRegistry,
    running: RunningSessions,
    deferred_prompts: DeferredPrompts,
    lifecycle: Arc<WorkerLifecycle>,
    session_grants: GrantRegistry,
    session_id: String,
    expected_goal: GoalRef,
) {
    // The command may have waited behind another runtime operation. Recheck
    // the exact GoalRef before claiming the worker lease so a delayed resume
    // cannot start a replacement Goal on the same Thread.
    if !goal_is_armed_for(&app_server, &session_id, &expected_goal) {
        return;
    }
    if !lifecycle.accepts_work() {
        if let Ok(thread_id) = ThreadId::new(session_id) {
            let _ = app_server.disarm_goal_if_matches(&thread_id, &expected_goal);
        }
        return;
    }
    if !claim_session(&running, &session_id, SessionRunKind::Goal) {
        // A human turn that won the lease is allowed to finish first. The Goal
        // remains armed and can be resumed explicitly without corrupting it.
        return;
    }
    let workspace = match protocol_thread_workspace(&app_server, &session_id) {
        Ok(workspace) => workspace,
        Err(message) => {
            if let Ok(mut sessions) = running.lock() {
                sessions.remove(&session_id);
            }
            if let Ok(thread_id) = ThreadId::new(session_id.clone()) {
                let _ = app_server.disarm_goal_if_matches(&thread_id, &expected_goal);
            }
            emit(
                &app,
                UiEvent::Error {
                    session_id,
                    message: format!("长期目标对应的会话不可用，自动续轮已关闭：{message}"),
                },
            );
            return;
        }
    };
    let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    if let Ok(mut registry) = cancels.lock() {
        registry.insert(session_id.clone(), cancel.clone());
    }
    let cleanup_session = session_id.clone();
    let cleanup_running = running.clone();
    let cleanup_cancels = cancels.clone();
    let failure_app = app.clone();
    let failure_server = app_server.clone();
    let thread_lifecycle = lifecycle.clone();
    let expected_goal_for_worker = expected_goal.clone();
    let spawned = std::thread::Builder::new()
        .name(format!(
            "ncx-goal-{}",
            session_id.chars().take(8).collect::<String>()
        ))
        .spawn(move || {
            let deferred_pending = pending.clone();
            let deferred_questions = questions.clone();
            let deferred_cancels = cancels.clone();
            let deferred_running = running.clone();
            let deferred_grants = session_grants.clone();
            let deferred_lifecycle = thread_lifecycle.clone();
            let deferred_app = app.clone();
            let deferred_server = app_server.clone();
            let deferred_session_id = session_id.clone();
            let finish = || {
                if let Ok(mut registry) = cancels.lock() {
                    registry.remove(&session_id);
                }
                if let Ok(mut sessions) = running.lock() {
                    sessions.remove(&session_id);
                }
            };
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                if let Ok(thread_id) = ThreadId::new(session_id.clone()) {
                    let _ =
                        app_server.disarm_goal_if_matches(&thread_id, &expected_goal_for_worker);
                }
                emit(
                    &app,
                    UiEvent::Error {
                        session_id: session_id.clone(),
                        message: "无法创建长期目标执行线程，自动续轮已关闭。".into(),
                    },
                );
                finish();
                return;
            };
            runtime.block_on(async {
                let active_session = Arc::new(Mutex::new(session_id.clone()));
                let approver: Rc<dyn ApprovalHandler> = Rc::new(GuiApprover {
                    app: app.clone(),
                    active_session: active_session.clone(),
                    pending,
                });
                let questioner: Rc<dyn UserQuestionHandler> = Rc::new(GuiQuestioner {
                    app: app.clone(),
                    active_session,
                    pending: questions,
                });
                goal_turn::run(goal_turn::GoalTurnInput {
                    app: app.clone(),
                    app_server: app_server.clone(),
                    session_grants: session_grants.clone(),
                    session_id: session_id.clone(),
                    workspace: workspace.clone(),
                    cancel,
                    approver,
                    questioner,
                    expected_goal: expected_goal_for_worker.clone(),
                })
                .await;
            });
            finish();
            let deferred = deferred_prompts
                .lock()
                .ok()
                .and_then(|mut prompts| prompts.remove(&deferred_session_id));
            if let Some(prompt) = deferred {
                emit(
                    &deferred_app,
                    UiEvent::HumanTurnStarted {
                        session_id: deferred_session_id.clone(),
                    },
                );
                let (messages, target_workspace) =
                    protocol_thread_seed(&deferred_server, &deferred_session_id)
                        .map(|(messages, stored_workspace)| {
                            (
                                messages,
                                stored_workspace
                                    .map(PathBuf::from)
                                    .unwrap_or_else(|| workspace.clone()),
                            )
                        })
                        .unwrap_or_else(|| (Vec::new(), workspace));
                let profile = protocol_thread_profile(&deferred_server, &deferred_session_id);
                spawn_turn_worker(
                    deferred_app,
                    deferred_server,
                    deferred_pending,
                    deferred_questions,
                    deferred_cancels,
                    deferred_running,
                    deferred_prompts.clone(),
                    deferred_lifecycle,
                    deferred_grants,
                    deferred_session_id,
                    target_workspace,
                    messages,
                    prompt.text,
                    prompt.images,
                    prompt.execution_mode,
                    profile,
                );
            }
        });
    if let Ok(handle) = spawned {
        lifecycle.track(handle);
    } else {
        if let Ok(mut registry) = cleanup_cancels.lock() {
            registry.remove(&cleanup_session);
        }
        if let Ok(mut sessions) = cleanup_running.lock() {
            sessions.remove(&cleanup_session);
        }
        if let Ok(thread_id) = ThreadId::new(cleanup_session.clone()) {
            let _ = failure_server.disarm_goal_if_matches(&thread_id, &expected_goal);
        }
        emit(
            &failure_app,
            UiEvent::Error {
                session_id: cleanup_session,
                message: "无法启动长期目标执行线程，自动续轮已关闭。".into(),
            },
        );
    }
}

struct ProtocolTurnGuard {
    app: Option<AppHandle>,
    server: Arc<AppServer<JsonThreadStore>>,
    thread_id: ThreadId,
    turn_id: TurnId,
    finished: bool,
}

struct ProtocolTurnStart<'a> {
    app: Option<AppHandle>,
    server: Arc<AppServer<JsonThreadStore>>,
    session_id: &'a str,
    workspace: &'a Path,
    turn_id: TurnId,
    user_text: &'a str,
    execution_mode: ExecutionMode,
    harness_profile: &'a str,
}

impl ProtocolTurnGuard {
    fn start(input: ProtocolTurnStart<'_>) -> Result<Self, String> {
        let ProtocolTurnStart {
            app,
            server,
            session_id,
            workspace,
            turn_id,
            user_text,
            execution_mode,
            harness_profile,
        } = input;
        let thread_id = ThreadId::new(session_id.to_string()).map_err(|error| error.to_string())?;
        if server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: thread_id.clone(),
            })
            .is_err()
        {
            let outcome = server
                .dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(thread_id.clone()),
                    workspace: workspace.display().to_string(),
                    title: "(no prompt yet)".to_string(),
                    harness_profile: harness_profile.to_string(),
                })
                .map_err(|error| error.to_string())?;
            if let Some(app) = &app {
                emit_protocol_outcome(app, &outcome);
            }
        }
        let outcome = server
            .dispatch(ClientRequest::TurnStart {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
                execution_mode,
            })
            .map_err(|error| error.to_string())?;
        if let Some(app) = &app {
            emit_protocol_outcome(app, &outcome);
        }
        let mut guard = Self {
            app,
            server,
            thread_id,
            turn_id,
            finished: false,
        };
        match guard.server.dispatch(ClientRequest::ItemAppend {
            thread_id: guard.thread_id.clone(),
            turn_id: guard.turn_id.clone(),
            item: ThreadItem::UserMessage {
                id: protocol_item_id("user"),
                text: user_text.to_string(),
            },
        }) {
            Ok(outcome) => {
                if let Some(app) = &guard.app {
                    emit_protocol_outcome(app, &outcome);
                }
            }
            Err(error) => {
                guard.complete("failed to persist user message");
                return Err(error.to_string());
            }
        }
        Ok(guard)
    }

    fn complete(&mut self, stop_reason: &str) {
        self.complete_with_usage(stop_reason, TurnUsage::default());
    }

    fn complete_with_usage(&mut self, stop_reason: &str, usage: TurnUsage) {
        let status = match stop_reason {
            "completed" => TurnStatus::Completed,
            "cancelled" | "canceled" => TurnStatus::Cancelled,
            _ => TurnStatus::Failed,
        };
        if let Ok(outcome) = self.server.dispatch(ClientRequest::TurnComplete {
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            status,
            error: (status == TurnStatus::Failed).then(|| stop_reason.to_string()),
            usage,
        }) {
            if let Some(app) = &self.app {
                emit_protocol_outcome(app, &outcome);
            }
        }
        self.finished = true;
    }
}

impl Drop for ProtocolTurnGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Ok(outcome) = self.server.dispatch(ClientRequest::TurnComplete {
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            status: TurnStatus::Failed,
            error: Some("turn worker exited before completion".to_string()),
            usage: TurnUsage::default(),
        }) {
            if let Some(app) = &self.app {
                emit_protocol_outcome(app, &outcome);
            }
        }
    }
}

fn claim_session(running: &RunningSessions, session_id: &str, kind: SessionRunKind) -> bool {
    running
        .lock()
        .map(|mut sessions| sessions.insert(session_id.to_string(), kind).is_none())
        .unwrap_or(false)
}

fn armed_goal_ref(app_server: &AppServer<JsonThreadStore>, session_id: &str) -> Option<GoalRef> {
    let Ok(thread_id) = ThreadId::new(session_id.to_string()) else {
        return None;
    };
    app_server
        .dispatch(ClientRequest::GoalRead { thread_id })
        .ok()
        .and_then(|outcome| match outcome.response.payload {
            ResponsePayload::Goal(Some(goal))
                if goal.activation == ncx_protocol::GoalActivation::Armed
                    && goal.goal.phase == ncx_protocol::GoalPhase::Active =>
            {
                Some(GoalRef {
                    id: goal.goal.id,
                    revision: goal.goal.revision,
                })
            }
            _ => None,
        })
}

fn goal_is_armed_for(
    app_server: &AppServer<JsonThreadStore>,
    session_id: &str,
    expected: &GoalRef,
) -> bool {
    armed_goal_ref(app_server, session_id).is_some_and(|actual| actual == *expected)
}

/// Spawn the lightweight navigation/config coordinator. It drains commands in
/// order, but each prompt is handed to its own session-scoped turn thread so
/// different conversations can continue concurrently.
pub struct WorkerStartup {
    pub app: AppHandle,
    pub app_server: Arc<AppServer<JsonThreadStore>>,
    pub rx: UnboundedReceiver<Command>,
    pub pending: PendingMap,
    pub questions: PendingQuestionMap,
    pub cancels: CancelRegistry,
    pub running: RunningSessions,
    pub deferred_prompts: DeferredPrompts,
    pub lifecycle: Arc<WorkerLifecycle>,
    pub session_grants: GrantRegistry,
    pub runtime_activation: Arc<RuntimeActivationCoordinator>,
    pub startup_workspace: PathBuf,
}

pub fn spawn_worker(startup: WorkerStartup) {
    let WorkerStartup {
        app,
        app_server,
        mut rx,
        pending,
        questions,
        cancels,
        running,
        deferred_prompts,
        lifecycle,
        session_grants,
        runtime_activation,
        startup_workspace,
    } = startup;
    let coordinator_lifecycle = lifecycle.clone();
    let spawned = std::thread::Builder::new()
        .name("ncx-agent".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent-thread tokio runtime builds");

            rt.block_on(async move {
                let active_session = Arc::new(Mutex::new(String::new()));
                let approver: Rc<dyn ApprovalHandler> = Rc::new(GuiApprover {
                    app: app.clone(),
                    active_session: active_session.clone(),
                    pending: pending.clone(),
                });
                let questioner: Rc<dyn UserQuestionHandler> = Rc::new(GuiQuestioner {
                    app: app.clone(),
                    active_session: active_session.clone(),
                    pending: questions.clone(),
                });
                // Tauri setup selected this before the worker started. Keep it
                // explicit so later workspace commands cannot affect initial
                // config/plugin discovery through the process-global CWD.
                let startup_seed = latest_protocol_thread_seed(&app_server, &startup_workspace);
                let startup_profile = startup_seed
                    .as_ref()
                    .map(|(id, _)| protocol_thread_profile(&app_server, id));
                // Session "always allow" grants — fresh per session, kept across
                // model / permission-mode rebuilds, replaced on new/resume/fork.
                let mut grants = Rc::new(RefCell::new(SessionGrants::default()));
                let (mut agent, mut workspace, mut session_id, _) = match build_agent(
                    approver.clone(),
                    questioner.clone(),
                    startup_seed,
                    grants.clone(),
                    startup_workspace.clone(),
                    startup_profile,
                    app_server.clone(),
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        emit(
                            &app,
                            UiEvent::Error {
                                session_id: String::new(),
                                message: e,
                            },
                        );
                        return;
                    }
                };
                set_active_session(&active_session, &session_id);
                agent.set_event_sink(make_sink(app.clone(), session_id.clone(), None, None));
                if let Err(message) = ensure_protocol_thread(&app_server, &session_id, &workspace) {
                    emit(
                        &app,
                        UiEvent::Error {
                            session_id: session_id.clone(),
                            message,
                        },
                    );
                }
                emit_ready(&app, &workspace, &session_id);

                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::Prompt {
                            session_id: target_id,
                            text,
                            images,
                            execution_mode,
                        } => {
                            let (messages, target_workspace) =
                                protocol_thread_seed(&app_server, &target_id)
                                    .map(|(messages, stored_workspace)| {
                                        (
                                            messages,
                                            stored_workspace
                                                .map(PathBuf::from)
                                                .unwrap_or_else(|| workspace.clone()),
                                        )
                                    })
                                    .unwrap_or_else(|| (Vec::new(), workspace.clone()));
                            let harness_profile = protocol_thread_profile(&app_server, &target_id);
                            spawn_turn_worker(
                                app.clone(),
                                app_server.clone(),
                                pending.clone(),
                                questions.clone(),
                                cancels.clone(),
                                running.clone(),
                                deferred_prompts.clone(),
                                lifecycle.clone(),
                                session_grants.clone(),
                                target_id,
                                target_workspace,
                                messages,
                                text,
                                images,
                                execution_mode,
                                harness_profile,
                            );
                        }
                        Command::New {
                            id,
                            workspace: command_workspace,
                            harness_profile,
                            activation,
                            completion,
                        } => {
                            // The Tauri caller may have timed out while this
                            // command waited behind another navigation. Do not
                            // create an expensive stale runtime in that case.
                            if !runtime_activation.can_proceed(&activation) {
                                let _ = completion.send(Err(
                                    "新建会话初始化已被更新的运行态切换取消".to_string(),
                                ));
                                continue;
                            }
                            let next_grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((id, Vec::new())),
                                next_grants.clone(),
                                command_workspace,
                                Some(harness_profile),
                                app_server.clone(),
                            )
                            .await
                            {
                                Ok((a, ws, sid, _)) => {
                                    // The successful builder must win this
                                    // fence before touching live state. If the
                                    // caller won with abort(), its App Server
                                    // transaction has already compensated the
                                    // durable Thread.
                                    if runtime_activation
                                        .commit_if_current(&activation, || {
                                            grants = next_grants;
                                            agent = a;
                                            workspace = ws;
                                            session_id = sid;
                                            set_active_session(&active_session, &session_id);
                                        })
                                        .is_none()
                                    {
                                        let _ = completion
                                            .send(Err("新建会话初始化已被更新的运行态切换取消"
                                                .to_string()));
                                        continue;
                                    }
                                    agent.set_event_sink(make_sink(
                                        app.clone(),
                                        session_id.clone(),
                                        None,
                                        None,
                                    ));
                                    emit_ready(&app, &workspace, &session_id);
                                    emit(
                                        &app,
                                        UiEvent::Loaded {
                                            session_id: session_id.clone(),
                                            messages: Vec::new(),
                                        },
                                    );
                                    let _ = completion.send(Ok(()));
                                }
                                Err(e) => {
                                    // Suppress a late build failure after the
                                    // request has already timed out and the UI
                                    // returned to its previous session.
                                    let report_error =
                                        runtime_activation.abort_if_pending(&activation);
                                    let _ = completion.send(Err(e.clone()));
                                    if report_error {
                                        emit(
                                            &app,
                                            UiEvent::Error {
                                                session_id: String::new(),
                                                message: e,
                                            },
                                        )
                                    }
                                }
                            }
                        }
                        Command::Resume {
                            id,
                            workspace: command_workspace,
                            activation,
                            completion,
                        } => {
                            if !runtime_activation.can_proceed(&activation) {
                                let _ = completion
                                    .send(Err("恢复会话已被更新的运行态切换取消".to_string()));
                                continue;
                            }
                            let loaded = protocol_thread_seed(&app_server, &id);
                            let Some((msgs, _)) = loaded else {
                                let message = format!("no saved snapshot for session {id}");
                                let report_error = runtime_activation.abort_if_pending(&activation);
                                let _ = completion.send(Err(message.clone()));
                                if report_error {
                                    emit(
                                        &app,
                                        UiEvent::Error {
                                            session_id: id.clone(),
                                            message,
                                        },
                                    );
                                }
                                continue;
                            };
                            let ui = protocol_thread_ui(&app_server, &id)
                                .unwrap_or_else(|| snapshot_to_ui(&msgs));
                            // The seed/UI reads above may race a newer
                            // navigation. Accept and replace every global
                            // runtime field as one linearizable handoff.
                            if runtime_activation
                                .commit_if_current(&activation, || {
                                    // The runtime adapter already transitioned the process CWD
                                    // under its workspace gate. Keep the worker independent from
                                    // that global state: the durable Thread metadata is the sole
                                    // authority for this session's runtime workspace.
                                    workspace = command_workspace;
                                    grants = Rc::new(RefCell::new(SessionGrants::default()));
                                    if let Ok(mut registry) = session_grants.lock() {
                                        registry.remove(&id);
                                    }
                                    session_id = id;
                                    set_active_session(&active_session, &session_id);
                                })
                                .is_none()
                            {
                                let _ = completion
                                    .send(Err("恢复会话已被更新的运行态切换取消".to_string()));
                                continue;
                            }
                            // A resumed thread is a state/navigation operation. Building
                            // tools, plugins and the current Provider Route belongs to the
                            // per-turn worker below; awaiting it here blocks the command
                            // queue and can strand a later prompt in optimistic "busy" UI.
                            emit_ready(&app, &workspace, &session_id);
                            emit(
                                &app,
                                UiEvent::Loaded {
                                    session_id: session_id.clone(),
                                    messages: ui,
                                },
                            );
                            let _ = completion.send(Ok(()));
                        }
                        Command::Fork {
                            source_id,
                            target_id,
                            workspace: command_workspace,
                            activation,
                            completion,
                        } => {
                            if !runtime_activation.can_proceed(&activation) {
                                let _ = completion.send(Err(
                                    "分叉会话初始化已被更新的运行态切换取消".to_string(),
                                ));
                                continue;
                            }
                            // App Server already copied the source snapshot
                            // into `target_id` transactionally. Reading the
                            // source here would let later source turns leak
                            // into this fork while it waits in the worker queue.
                            let loaded = protocol_thread_seed(&app_server, &target_id);
                            let Some((msgs, _)) = loaded else {
                                let message = format!(
                                    "no saved snapshot for fork target {target_id} (source {source_id})"
                                );
                                let report_error = runtime_activation.abort_if_pending(&activation);
                                let _ = completion.send(Err(message.clone()));
                                if report_error {
                                    emit(
                                        &app,
                                        UiEvent::Error {
                                            session_id: target_id.clone(),
                                            message,
                                        },
                                    );
                                }
                                continue;
                            };
                            let ui = protocol_thread_ui(&app_server, &target_id)
                                .unwrap_or_else(|| snapshot_to_ui(&msgs));
                            let next_grants = Rc::new(RefCell::new(SessionGrants::default()));
                            let harness_profile = protocol_thread_profile(&app_server, &target_id);
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((target_id.clone(), msgs)),
                                next_grants.clone(),
                                command_workspace,
                                Some(harness_profile),
                                app_server.clone(),
                            )
                            .await
                            {
                                Ok((a, ws, sid, _)) => {
                                    if runtime_activation
                                        .commit_if_current(&activation, || {
                                            grants = next_grants;
                                            agent = a;
                                            workspace = ws;
                                            session_id = sid;
                                            set_active_session(&active_session, &session_id);
                                        })
                                        .is_none()
                                    {
                                        let _ = completion
                                            .send(Err("分叉会话初始化已被更新的运行态切换取消"
                                                .to_string()));
                                        continue;
                                    }
                                    agent.set_event_sink(make_sink(
                                        app.clone(),
                                        session_id.clone(),
                                        None,
                                        None,
                                    ));
                                    emit_ready(&app, &workspace, &session_id);
                                    emit(
                                        &app,
                                        UiEvent::Loaded {
                                            session_id: session_id.clone(),
                                            messages: ui,
                                        },
                                    );
                                    let _ = completion.send(Ok(()));
                                }
                                Err(e) => {
                                    let report_error =
                                        runtime_activation.abort_if_pending(&activation);
                                    let _ = completion.send(Err(e.clone()));
                                    if report_error {
                                        emit(
                                            &app,
                                            UiEvent::Error {
                                                session_id: target_id.clone(),
                                                message: e,
                                            },
                                        )
                                    }
                                }
                            }
                        }
                        Command::SetModel(model) => {
                            // The caller has already atomically committed the complete
                            // provider route. Prompt workers resolve config per turn, so
                            // rebuilding the whole Harness here would only risk an
                            // unrelated MCP/skill failure and disturb live state.
                            let _ = model;
                            emit_ready(&app, &workspace, &session_id);
                        }
                        Command::ContinueGoal { thread_id, goal } => {
                            spawn_goal_worker(
                                app.clone(),
                                app_server.clone(),
                                pending.clone(),
                                questions.clone(),
                                cancels.clone(),
                                running.clone(),
                                deferred_prompts.clone(),
                                lifecycle.clone(),
                                session_grants.clone(),
                                thread_id,
                                goal,
                            );
                        }
                        Command::SetPermissionMode {
                            thread_id: target_id,
                            mode,
                            activation,
                            completion,
                        } => {
                            if !runtime_activation.can_proceed(&activation) {
                                let _ = completion
                                    .send(Err("切换权限模式已被更新的运行态切换取消".to_string()));
                                continue;
                            }
                            let (msgs, command_workspace, harness_profile) =
                                match permission_mode_rebuild_input(
                                    &app_server,
                                    &session_id,
                                    &target_id,
                                ) {
                                    Ok(input) => input,
                                    Err(error) => {
                                        let report_error =
                                            runtime_activation.abort_if_pending(&activation);
                                        let _ = completion.send(Err(error.clone()));
                                        if report_error {
                                            emit(
                                                &app,
                                                UiEvent::Error {
                                                    session_id: target_id,
                                                    message: error,
                                                },
                                            );
                                        }
                                        continue;
                                    }
                                };
                            // Persist the mode (+ derived sandbox/approval for consistency),
                            // then rebuild reseeded so the new gating + plan nudge apply
                            // without losing the conversation.
                            let (sandbox, approval, _re, _plan) = permission_mode_to_knobs(&mode);
                            let mut m = std::collections::HashMap::new();
                            m.insert("permission_mode", mode.as_str());
                            m.insert("sandbox_mode", sandbox);
                            m.insert("approval_policy", approval);
                            // Do the synchronous config commit under the same
                            // coordinator lock used by begin(). Otherwise a
                            // navigation can supersede this request between a
                            // current-token check and the actual write.
                            let Some(write_result) = runtime_activation
                                .run_if_current(&activation, || {
                                    write_nanocodex_config(&m, &ConfigPaths::default().nanocodex)
                                })
                            else {
                                let _ = completion
                                    .send(Err("切换权限模式已被更新的运行态切换取消".to_string()));
                                continue;
                            };
                            if let Err(error) = write_result {
                                let error = error.to_string();
                                let report_error = runtime_activation.abort_if_pending(&activation);
                                let _ = completion.send(Err(error.clone()));
                                if report_error {
                                    emit(
                                        &app,
                                        UiEvent::Error {
                                            session_id: target_id,
                                            message: error,
                                        },
                                    );
                                }
                                continue;
                            }
                            // Same session → keep the "always allow" grants.
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((target_id.clone(), msgs)),
                                grants.clone(),
                                command_workspace,
                                Some(harness_profile),
                                app_server.clone(),
                            )
                            .await
                            {
                                Ok((a, ws, sid, _)) => {
                                    if runtime_activation
                                        .commit_if_current(&activation, || {
                                            agent = a;
                                            workspace = ws;
                                            session_id = sid;
                                            set_active_session(&active_session, &session_id);
                                        })
                                        .is_none()
                                    {
                                        let _ =
                                            completion
                                                .send(Err("切换权限模式已被更新的运行态切换取消"
                                                    .to_string()));
                                        continue;
                                    }
                                    agent.set_event_sink(make_sink(
                                        app.clone(),
                                        session_id.clone(),
                                        None,
                                        None,
                                    ));
                                    emit_ready(&app, &workspace, &session_id);
                                    let _ = completion.send(Ok(()));
                                }
                                Err(e) => {
                                    let report_error =
                                        runtime_activation.abort_if_pending(&activation);
                                    let _ = completion.send(Err(e.clone()));
                                    if report_error {
                                        emit(
                                            &app,
                                            UiEvent::Error {
                                                session_id: target_id,
                                                message: e,
                                            },
                                        )
                                    }
                                }
                            }
                        }
                        Command::RequestReady => {
                            emit_ready(&app, &workspace, &session_id);
                            let messages = protocol_thread_ui(&app_server, &session_id);
                            if let Some(messages) = messages.filter(|items| !items.is_empty()) {
                                emit(
                                    &app,
                                    UiEvent::Loaded {
                                        session_id: session_id.clone(),
                                        messages,
                                    },
                                );
                            }
                        }
                        Command::Shutdown => break,
                    }
                }
            });
        });
    if let Ok(handle) = spawned {
        coordinator_lifecycle.track(handle);
    }
}

/// Convert a full model snapshot into the lightweight visible transcript.
/// Tool calls/results and intermediate assistant narration stay on disk for
/// model continuity, but never cross the backend/UI boundary during restore.
fn snapshot_to_ui(messages: &[Value]) -> Vec<UiMsg> {
    let mut out = Vec::new();
    let mut final_assistant: Option<UiMsg> = None;
    let mut saw_user = false;
    let mut tool_names = HashMap::<String, String>::new();

    let flush_final = |out: &mut Vec<UiMsg>, pending: &mut Option<UiMsg>| {
        if let Some(message) = pending.take() {
            out.push(message);
        }
    };

    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = snapshot_text_content(m.get("content"));
        if role == "assistant" {
            for call in m
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let (Some(id), Some(name)) = (
                    call.get("id").and_then(Value::as_str),
                    call.pointer("/function/name").and_then(Value::as_str),
                ) {
                    tool_names.insert(id.to_string(), name.to_string());
                }
            }
        }
        match role {
            "user" => {
                flush_final(&mut out, &mut final_assistant);
                if content.starts_with(COMPACTED_HISTORY_PREFIX) {
                    out.push(UiMsg {
                        role: "compact".into(),
                        text: "较早的会话内容已自动压缩，关键要求和完成结果已保留。".into(),
                        model: None,
                        confirmed_model: None,
                    });
                    continue;
                }
                if !content.trim().is_empty() {
                    out.push(UiMsg {
                        role: "user".into(),
                        text: content,
                        model: None,
                        confirmed_model: None,
                    });
                    saw_user = true;
                }
            }
            "assistant" => {
                if saw_user && !content.trim().is_empty() {
                    final_assistant = Some(UiMsg {
                        role: "assistant".into(),
                        text: content,
                        model: m
                            .get("_ncx_model")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        confirmed_model: m
                            .get("_ncx_confirmed_model")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                }
            }
            "tool" => {
                let Some(name) = m
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .and_then(|id| tool_names.get(id))
                else {
                    continue;
                };
                for artifact in media_artifact_items(name, &content) {
                    if let ThreadItem::Artifact {
                        kind, name, url, ..
                    } = artifact
                    {
                        out.push(UiMsg {
                            role: format!("artifact_{kind}"),
                            text: format!("{name}\n{url}"),
                            model: None,
                            confirmed_model: None,
                        });
                    }
                }
            }
            _ => {} // system/tool/unsupported roles never reach restored UI
        }
    }
    flush_final(&mut out, &mut final_assistant);
    out
}

fn snapshot_text_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn save_auto_checkpoint(workspace: &std::path::Path, prompt: &str) {
    let label = format!("gui: {}", clipped_label(prompt, 80));
    let _ = CheckpointStore::new(workspace).create(&label);
}

fn clipped_label(text: &str, limit: usize) -> String {
    let s = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= limit {
        s
    } else {
        format!(
            "{}...",
            s.chars().take(limit.saturating_sub(3)).collect::<String>()
        )
    }
}

// ── vision attachments (ported from the CLI) ──────────────────────────────────

fn validate_image_attachments(
    tools: &ncx_core::ToolRegistry,
    images: &[String],
) -> Result<(), String> {
    let service = tools.service::<ncx_core::AttachmentServiceDescriptor>("attachment");
    validate_image_attachment_paths(service.as_deref(), images)
}

fn validate_image_attachment_paths(
    service: Option<&ncx_core::AttachmentServiceDescriptor>,
    images: &[String],
) -> Result<(), String> {
    if images.is_empty() {
        return Ok(());
    }
    // Image transport is a core chat capability. Harness attachment services
    // may tighten its policy, but their absence must not block a native
    // multimodal model or the separately configured parser fallback.
    let max_bytes = service
        .map(|service| service.max_bytes)
        .unwrap_or(20 * 1024 * 1024);
    const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];
    for value in images {
        let path = std::path::Path::new(value);
        let extension = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let allowed = service
            .map(|service| service.extensions.iter().any(|value| value == &extension))
            .unwrap_or_else(|| IMAGE_EXTENSIONS.contains(&extension.as_str()));
        if !allowed {
            return Err(format!("附件格式 .{extension} 未被当前插件允许"));
        }
        let size = std::fs::metadata(path)
            .map_err(|e| format!("无法读取附件 {value}: {e}"))?
            .len();
        if size > max_bytes {
            return Err(format!("附件 {value} 超过 {max_bytes} 字节限制"));
        }
    }
    Ok(())
}

/// Build the user turn input. No images -> plain text; with image paths ->
/// an OpenAI multimodal `content` array (text block + one base64 `image_url`
/// block per file), which trips AgentLoop's image detection -> vision routing.
fn build_image_user_input(text: &str, images: &[String]) -> Result<Value, String> {
    if images.is_empty() {
        return Ok(json!(text));
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    for path in images {
        let p = std::path::Path::new(path);
        let bytes = std::fs::read(p).map_err(|e| format!("cannot read image {path}: {e}"))?;
        let url = format!("data:{};base64,{}", image_mime(p), base64_encode(&bytes));
        content.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Ok(Value::Array(content))
}

fn image_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    }
}

/// Standard base64 (RFC 4648, `=` padded). Hand-rolled to avoid a new crate dep.
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_core::{Skill, Tool};
    use ncx_sandbox::WORKSPACE_WRITE;

    #[test]
    fn runtime_activation_fence_has_one_winner() {
        let fence = RuntimeActivationFence::new();
        assert!(!fence.is_accepted());
        assert!(!fence.is_aborted());
        assert!(fence.accept());
        assert!(fence.is_accepted());
        assert!(!fence.accept());
        assert!(!fence.abort());

        let aborted = RuntimeActivationFence::new();
        assert!(aborted.abort());
        assert!(aborted.is_aborted());
        assert!(!aborted.accept());
    }

    struct PreparedMcpTestTool;

    #[async_trait(?Send)]
    impl Tool for PreparedMcpTestTool {
        fn name(&self) -> &str {
            "valid_mcp_tool"
        }

        fn description(&self) -> &str {
            "test MCP tool"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, _: &ToolContext, _: &Value) -> String {
            String::new()
        }
    }

    #[test]
    fn unavailable_mcp_server_does_not_discard_prepared_servers() {
        let broken = McpServerConfig {
            name: "broken".into(),
            command: "missing-ncx-mcp".into(),
            args: Vec::new(),
            env: HashMap::new(),
        };
        let valid = McpServerConfig {
            name: "valid".into(),
            command: "mock".into(),
            args: Vec::new(),
            env: HashMap::new(),
        };
        let mut prepared: Vec<Box<dyn Tool>> = Vec::new();

        append_prepared_mcp_tools(&broken, Err("spawn missing-ncx-mcp".into()), &mut prepared);
        append_prepared_mcp_tools(
            &valid,
            Ok(vec![Box::new(PreparedMcpTestTool)]),
            &mut prepared,
        );

        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].name(), "valid_mcp_tool");
    }

    fn protocol_server(name: &str) -> (Arc<AppServer<JsonThreadStore>>, PathBuf) {
        let root = std::env::temp_dir().join(format!("ncx-protocol-{name}-{}", new_session_id()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Arc::new(JsonThreadStore::open(root.join("threads.json")).unwrap());
        (Arc::new(AppServer::new(store, || 1)), root)
    }

    #[test]
    fn stale_permission_mode_rebuild_cannot_target_the_new_active_session() {
        let (server, root) = protocol_server("permission-mode-target");
        let first_workspace = root.join("first");
        let second_workspace = root.join("second");
        std::fs::create_dir_all(&first_workspace).unwrap();
        std::fs::create_dir_all(&second_workspace).unwrap();
        for (thread_id, workspace) in [
            ("permission-first", &first_workspace),
            ("permission-second", &second_workspace),
        ] {
            server
                .dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(ThreadId::new(thread_id).unwrap()),
                    workspace: workspace.to_string_lossy().into_owned(),
                    title: thread_id.into(),
                    harness_profile: "full".into(),
                })
                .unwrap();
        }

        let error = permission_mode_rebuild_input(&server, "permission-second", "permission-first")
            .unwrap_err();
        assert!(error.contains("会话已切换"));
        assert!(error.contains("permission-first"));
        assert!(error.contains("permission-second"));

        let (messages, workspace, profile) =
            permission_mode_rebuild_input(&server, "permission-second", "permission-second")
                .unwrap();
        assert!(messages.is_empty());
        assert_eq!(workspace, second_workspace);
        assert_eq!(profile, "full");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fork_target_seed_is_snapshotted_from_the_target_thread() {
        let (server, root) = protocol_server("fork-target-seed");
        let source = ThreadId::new("fork-source").unwrap();
        let target = ThreadId::new("fork-target").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(source.clone()),
                workspace: root.to_string_lossy().into_owned(),
                title: "source".into(),
                harness_profile: "full".into(),
            })
            .unwrap();
        server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: source.clone(),
                messages: vec![json!({"role": "user", "content": "snapshot-at-fork"})],
            })
            .unwrap();
        server
            .dispatch(ClientRequest::ThreadFork {
                thread_id: source.clone(),
                new_thread_id: target.clone(),
            })
            .unwrap();
        // A source turn arriving after the fork must not be visible to the
        // target worker, even if that worker was delayed in the queue.
        server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: source,
                messages: vec![json!({"role": "user", "content": "later-source-turn"})],
            })
            .unwrap();

        let (messages, _) = protocol_thread_seed(&server, target.as_str()).unwrap();
        let _ui = protocol_thread_ui(&server, target.as_str()).unwrap();
        let profile = protocol_thread_profile(&server, target.as_str());
        assert_eq!(messages[0]["content"], "snapshot-at-fork");
        assert_eq!(profile, "full");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn native_image_transport_does_not_require_harness_attachment_service() {
        let root = std::env::temp_dir().join(format!("ncx-native-image-{}", new_session_id()));
        std::fs::create_dir_all(&root).unwrap();
        let image = root.join("sample.png");
        std::fs::write(&image, b"png").unwrap();
        assert!(
            validate_image_attachment_paths(None, &[image.to_string_lossy().into_owned()]).is_ok()
        );
        let input =
            build_image_user_input("inspect", &[image.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(input[1]["type"], "image_url");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gui_goal_tool_service_routes_through_app_server_domain() {
        let (server, root) = protocol_server("goal-tools");
        let thread_id = ThreadId::new("goal-thread").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(thread_id.clone()),
                workspace: root.to_string_lossy().into_owned(),
                title: "goal".into(),
                harness_profile: "full".into(),
            })
            .unwrap();
        let service = GuiGoalToolService {
            app_server: server,
            thread_id,
        };
        let created = service.create("finish migration".into(), 8).unwrap();
        assert_eq!(created.goal.objective, "finish migration");
        assert_eq!(created.activation, ncx_protocol::GoalActivation::Disarmed);
        assert_eq!(service.get().unwrap(), Some(created));
    }

    #[test]
    fn goal_round_prompt_replays_to_model_but_stays_out_of_visible_history() {
        let thread = Thread {
            metadata: ncx_protocol::ThreadMetadata {
                id: ThreadId::new("goal-history").unwrap(),
                workspace: "workspace".into(),
                title: "goal".into(),
                archived: false,
                harness_profile: "full".into(),
                created_at: 1,
                updated_at: 2,
            },
            turns: vec![ncx_protocol::Turn {
                id: TurnId::new("goal-turn").unwrap(),
                status: TurnStatus::Completed,
                execution_mode: ExecutionMode::Agent,
                items: vec![ThreadItem::GoalMessage {
                    id: ItemId::new("goal-message").unwrap(),
                    text: "automatic continuation".into(),
                    goal_id: ncx_protocol::GoalId::new("goal").unwrap(),
                    revision: 2,
                    round: 1,
                }],
                started_at: 1,
                completed_at: Some(2),
                error: None,
                usage: Default::default(),
            }],
        };
        let replay = protocol_thread_messages(&thread, false);
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0]["role"], "user");
        assert_eq!(replay[0]["content"], "automatic continuation");
        assert!(protocol_thread_messages(&thread, true).is_empty());
    }

    #[test]
    fn protocol_turn_persists_user_item_and_releases_ownership_on_completion() {
        let (server, root) = protocol_server("complete");
        let turn_id = TurnId::new("turn-1").unwrap();
        let mut guard = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server: server.clone(),
            session_id: "thread-1",
            workspace: &root,
            turn_id: turn_id.clone(),
            user_text: "执行任务",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        })
        .unwrap();
        guard.complete("completed");

        let thread_id = ThreadId::new("thread-1").unwrap();
        let outcome = server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: thread_id.clone(),
            })
            .unwrap();
        let ncx_protocol::ResponsePayload::Thread(thread) = outcome.response.payload else {
            panic!("expected thread response");
        };
        assert_eq!(thread.turns[0].status, TurnStatus::Completed);
        assert!(
            matches!(thread.turns[0].items[0], ThreadItem::UserMessage { ref text, .. } if text == "执行任务")
        );

        let next = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server,
            session_id: "thread-1",
            workspace: &root,
            turn_id: TurnId::new("turn-2").unwrap(),
            user_text: "下一轮",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        });
        assert!(next.is_ok(), "completed turn must release thread ownership");
        drop(next);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn visible_history_uses_full_turns_when_model_context_is_compacted() {
        let (server, root) = protocol_server("visible-history");
        let turn_id = TurnId::new("turn-history").unwrap();
        let mut guard = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server: server.clone(),
            session_id: "thread-history",
            workspace: &root,
            turn_id: turn_id.clone(),
            user_text: "必须保留的原始问题",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        })
        .unwrap();
        server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: ThreadId::new("thread-history").unwrap(),
                turn_id,
                item: ThreadItem::AssistantMessage {
                    id: ItemId::new("assistant-history").unwrap(),
                    text: "完整回答".into(),
                    model: None,
                    confirmed_model: None,
                },
            })
            .unwrap();
        guard.complete("completed");
        server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: ThreadId::new("thread-history").unwrap(),
                messages: vec![json!({
                    "role": "user",
                    "content": format!("{COMPACTED_HISTORY_PREFIX}]\n内部摘要")
                })],
            })
            .unwrap();

        let visible = protocol_thread_ui(&server, "thread-history").unwrap();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].role, "user");
        assert_eq!(visible[0].text, "必须保留的原始问题");
        assert_eq!(visible[1].text, "完整回答");
        assert!(visible.iter().all(|message| message.role != "compact"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn protocol_thread_projects_back_to_model_and_history_messages() {
        let thread = Thread {
            metadata: ncx_protocol::ThreadMetadata {
                id: ThreadId::new("thread-projection").unwrap(),
                workspace: "workspace".into(),
                title: "projection".into(),
                archived: false,
                harness_profile: "full".into(),
                created_at: 1,
                updated_at: 2,
            },
            turns: vec![ncx_protocol::Turn {
                id: TurnId::new("turn-projection").unwrap(),
                status: TurnStatus::Completed,
                execution_mode: ExecutionMode::Agent,
                items: vec![
                    ThreadItem::UserMessage {
                        id: ItemId::new("user").unwrap(),
                        text: "生成 PDF".into(),
                    },
                    ThreadItem::ToolCall {
                        id: ItemId::new("call").unwrap(),
                        name: "shell".into(),
                        arguments: json!({"command": "build"}),
                    },
                    ThreadItem::ToolResult {
                        id: ItemId::new("result").unwrap(),
                        call_id: ItemId::new("call").unwrap(),
                        output: "ok".into(),
                        success: true,
                    },
                    ThreadItem::AssistantMessage {
                        id: ItemId::new("assistant").unwrap(),
                        text: "PDF 已生成".into(),
                        model: None,
                        confirmed_model: None,
                    },
                ],
                started_at: 1,
                completed_at: Some(2),
                error: None,
                usage: TurnUsage::default(),
            }],
        };
        let messages = protocol_thread_messages(&thread, false);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call");
        assert_eq!(messages[2]["tool_call_id"], "call");
        assert_eq!(messages[3]["content"], "PDF 已生成");
        let visible = snapshot_to_ui(&messages);
        assert_eq!(
            visible.len(),
            2,
            "history keeps user and final conclusion only"
        );
    }

    #[test]
    fn stored_model_context_wins_over_reconstructing_noisy_thread_items() {
        let (server, root) = protocol_server("stored-context");
        let mut guard = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server: server.clone(),
            session_id: "thread-context",
            workspace: &root,
            turn_id: TurnId::new("turn-context").unwrap(),
            user_text: "旧请求",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        })
        .unwrap();
        guard.complete("completed");
        server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: ThreadId::new("thread-context").unwrap(),
                messages: vec![json!({
                    "role": "user",
                    "content": format!("{COMPACTED_HISTORY_PREFIX}]\n保留的关键结论")
                })],
            })
            .unwrap();

        let (messages, _) = protocol_thread_seed(&server, "thread-context").unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("保留的关键结论"));
        assert!(!messages
            .iter()
            .any(|message| message["content"] == "旧请求"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dropped_protocol_turn_fails_and_releases_ownership() {
        let (server, root) = protocol_server("drop");
        let guard = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server: server.clone(),
            session_id: "thread-drop",
            workspace: &root,
            turn_id: TurnId::new("turn-drop-1").unwrap(),
            user_text: "会异常退出",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        })
        .unwrap();
        drop(guard);

        let next = ProtocolTurnGuard::start(ProtocolTurnStart {
            app: None,
            server,
            session_id: "thread-drop",
            workspace: &root,
            turn_id: TurnId::new("turn-drop-2").unwrap(),
            user_text: "恢复执行",
            execution_mode: ExecutionMode::Agent,
            harness_profile: "full",
        });
        assert!(
            next.is_ok(),
            "dropped turn must not leave permanent ownership"
        );
        drop(next);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restored_history_omits_tool_calls_and_results_from_the_ui() {
        let messages = vec![
            json!({"role": "user", "content": "请处理文件"}),
            json!({"role": "assistant", "content": "我先检查。"}),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {
                        "id": "call-shell",
                        "type": "function",
                        "function": {"name": "shell", "arguments": "{\"command\":\"dir\"}"}
                    },
                    {
                        "id": "call-patch",
                        "type": "function",
                        "function": {"name": "apply_patch", "arguments": "{\"patch\":\"*** Begin Patch\"}"}
                    }
                ]
            }),
            json!({"role": "tool", "tool_call_id": "call-shell", "content": "Exit code: 0\nfile.txt"}),
            json!({"role": "tool", "tool_call_id": "call-patch", "content": "Error: patch rejected"}),
            json!({"role": "assistant", "content": "处理完成。", "_ncx_model": "gpt-requested", "_ncx_confirmed_model": "gpt-confirmed"}),
        ];

        let restored = serde_json::to_value(snapshot_to_ui(&messages)).unwrap();
        assert_eq!(restored.as_array().unwrap().len(), 2);
        assert_eq!(restored[0]["role"], "user");
        assert_eq!(restored[1]["role"], "assistant");
        assert_eq!(restored[1]["text"], "处理完成。");
        assert_eq!(restored[1]["model"], "gpt-requested");
        assert_eq!(restored[1]["confirmed_model"], "gpt-confirmed");
        assert!(restored[1].get("tools").is_none());
    }

    #[test]
    fn restored_history_replaces_internal_compaction_summary_with_a_marker() {
        let messages = vec![
            json!({
                "role": "user",
                "content": format!("{COMPACTED_HISTORY_PREFIX}；测试]\n用户：旧要求\n助手完成结果：secret detail")
            }),
            json!({"role": "user", "content": "继续"}),
            json!({"role": "assistant", "content": "处理完成。"}),
        ];

        let restored = serde_json::to_value(snapshot_to_ui(&messages)).unwrap();
        assert_eq!(restored[0]["role"], "compact");
        assert!(!restored[0]["text"]
            .as_str()
            .unwrap()
            .contains("secret detail"));
        assert_eq!(restored[1]["text"], "继续");
        assert_eq!(restored[2]["text"], "处理完成。");
    }

    #[test]
    fn restored_session_uses_the_latest_saved_plan() {
        let messages = vec![
            json!({
                "role": "assistant",
                "tool_calls": [{
                    "id": "plan-1",
                    "type": "function",
                    "function": {
                        "name": "update_plan",
                        "arguments": "{\"plan\":[{\"step\":\"collect\",\"status\":\"in_progress\"}]}"
                    }
                }]
            }),
            json!({
                "role": "assistant",
                "tool_calls": [{
                    "id": "plan-2",
                    "type": "function",
                    "function": {
                        "name": "update_plan",
                        "arguments": "{\"plan\":[{\"step\":\"collect\",\"status\":\"completed\"},{\"step\":\"write PDF\",\"status\":\"in_progress\"}]}"
                    }
                }]
            }),
        ];

        let plan = latest_plan_from_messages(&messages);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0]["status"], "completed");
        assert_eq!(plan[1]["step"], "write PDF");
        assert_eq!(plan[1]["status"], "in_progress");
    }

    #[test]
    fn cancelled_session_does_not_restore_its_unfinished_plan() {
        let messages = vec![
            json!({"role": "user", "content": "generate the PDF"}),
            json!({
                "role": "assistant",
                "tool_calls": [{
                    "id": "plan-pdf",
                    "type": "function",
                    "function": {
                        "name": "update_plan",
                        "arguments": "{\"plan\":[{\"step\":\"verify PDF\",\"status\":\"in_progress\"}]}"
                    }
                }]
            }),
            json!({"role": "tool", "tool_call_id": "plan-pdf", "content": "Plan updated (1 steps)."}),
            json!({"role": "assistant", "content": "Stopped by user."}),
        ];

        assert!(latest_plan_from_messages(&messages).is_empty());
    }

    #[test]
    fn title_generation_only_runs_after_a_completed_first_turn() {
        assert!(should_generate_session_title(true, "completed"));
        assert!(!should_generate_session_title(false, "completed"));
        assert!(!should_generate_session_title(true, "cancelled"));
        assert!(!should_generate_session_title(true, "error"));
    }

    #[test]
    fn fallback_title_handles_greetings_and_removes_request_prefixes() {
        assert_eq!(fallback_session_title("你好"), Some("日常问候".into()));
        assert_eq!(
            fallback_session_title("帮我修复历史会话打不开的问题"),
            Some("修复历史会话打不开的问题".into())
        );
    }

    #[test]
    fn fallback_title_normalizes_multiline_requests_and_limits_length() {
        let title = fallback_session_title(
            "请帮我  修复历史会话名称没有自动生成的问题\n并且确保模型失败时也能正常显示",
        )
        .unwrap();

        assert!(!title.contains('\n'));
        assert!(title.chars().count() <= 25);
        assert_ne!(title, "新会话");
    }

    #[test]
    fn fallback_title_rejects_empty_requests() {
        assert_eq!(fallback_session_title(" \n\t "), None);
    }

    #[test]
    fn system_prompt_defines_the_buglecat_persona_without_sacrificing_safety() {
        assert!(SYSTEM_PROMPT.contains("BugleCat (妙脆角猫咪)"));
        assert!(SYSTEM_PROMPT
            .contains("Accuracy, action, and verification always come before role-play"));
        assert!(SYSTEM_PROMPT.contains("never add it to errors, warnings"));
    }

    #[test]
    fn runtime_prompt_reports_the_exact_client_selected_route() {
        let prompt = runtime_system_prompt("yunmo", "openai", "gpt-5.6-sol");

        assert!(prompt.contains("provider ID = \"yunmo\""));
        assert!(prompt.contains("protocol = \"openai\""));
        assert!(prompt.contains("requested model ID = \"gpt-5.6-sol\""));
        assert!(prompt.contains("do not claim"));
    }

    #[test]
    fn ready_snapshot_hides_the_internal_legacy_provider_marker() {
        let mut cfg = Config {
            active_provider_id: "legacy".into(),
            base_url: "https://api.yunmo-ai.com/v1/".into(),
            ..Config::default()
        };
        assert_eq!(visible_provider_id(&cfg), "yunmo");
        cfg.base_url = "https://unlisted.example/v1".into();
        assert_eq!(visible_provider_id(&cfg), "manual");
        cfg.active_provider_id = "company-relay".into();
        assert_eq!(visible_provider_id(&cfg), "company-relay");
    }

    #[test]
    fn successful_media_tool_results_become_clickable_artifacts() {
        let items = media_artifact_items(
            "generate_image",
            r#"{"status":"succeeded","urls":["https://example.com/a.png"]}"#,
        );
        assert!(matches!(
            &items[0],
            ThreadItem::Artifact { kind, name, url, .. }
                if kind == "image" && name == "生成图片 1" && url == "https://example.com/a.png"
        ));
        assert!(media_artifact_items("shell", "https://example.com/a.png").is_empty());
        assert!(media_artifact_items("generate_image", "Error: failed").is_empty());
    }

    #[test]
    fn restored_media_tool_results_keep_the_artifact_link() {
        let messages = vec![
            json!({"role":"user","content":"生成图片"}),
            json!({"role":"assistant","tool_calls":[{"id":"call-1","type":"function","function":{"name":"generate_image","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"call-1","content":"{\"status\":\"succeeded\",\"urls\":[\"https://example.com/a.png\"]}"}),
            json!({"role":"assistant","content":"图片已生成。"}),
        ];
        let visible = snapshot_to_ui(&messages);
        assert!(visible
            .iter()
            .any(|message| message.role == "artifact_image"
                && message.text.contains("https://example.com/a.png")));
    }

    #[test]
    fn session_title_event_uses_the_frontend_contract() {
        let event = serde_json::to_value(UiEvent::SessionTitle {
            session_id: "session-1".into(),
            title: "修复会话标题".into(),
        })
        .unwrap();

        assert_eq!(event["kind"], "session_title");
        assert_eq!(event["session_id"], "session-1");
        assert_eq!(event["title"], "修复会话标题");
    }

    #[test]
    fn every_turn_event_carries_its_session_id() {
        let event = serde_json::to_value(UiEvent::Assistant {
            session_id: "session-1".into(),
            text: "完成".into(),
            model: "gpt-5.6-sol".into(),
            confirmed_model: Some("gpt-5.6-sol".into()),
        })
        .unwrap();
        assert_eq!(event["session_id"], "session-1");
        assert_eq!(event["model"], "gpt-5.6-sol");
        assert_eq!(event["confirmed_model"], "gpt-5.6-sol");

        let loaded = serde_json::to_value(UiEvent::Loaded {
            session_id: "session-2".into(),
            messages: Vec::new(),
        })
        .unwrap();
        assert_eq!(loaded["session_id"], "session-2");

        let compacted = serde_json::to_value(UiEvent::ContextCompacted {
            session_id: "session-3".into(),
            original_chars: 200_000,
            edited_chars: 80_000,
            dropped_messages: 30,
            compressed_tool_results: 12,
        })
        .unwrap();
        assert_eq!(compacted["kind"], "context_compacted");
        assert_eq!(compacted["session_id"], "session-3");
    }

    #[test]
    fn home_is_unsafe_but_a_project_dir_is_not() {
        let base = std::env::temp_dir().join(format!("ncx_ws_{}", new_session_id()));
        let home = base.join("home");
        let project = base.join("home").join("proj");
        std::fs::create_dir_all(&project).unwrap();

        assert!(
            is_unsafe_against(&home, Some(&home)),
            "home dir must be unsafe"
        );
        assert!(
            !is_unsafe_against(&project, Some(&home)),
            "a project under home must be safe"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn display_path_hides_windows_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\D:\work")), r"D:\work");
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share")),
            r"\\server\share"
        );
    }

    #[test]
    fn image_mime_covers_frontend_attachment_formats() {
        assert_eq!(image_mime(Path::new("attachment.png")), "image/png");
        assert_eq!(image_mime(Path::new("attachment.jpg")), "image/jpeg");
        assert_eq!(image_mime(Path::new("attachment.jpeg")), "image/jpeg");
        assert_eq!(image_mime(Path::new("attachment.gif")), "image/gif");
        assert_eq!(image_mime(Path::new("attachment.webp")), "image/webp");
        assert_eq!(image_mime(Path::new("attachment.bmp")), "image/bmp");
    }

    #[tokio::test]
    async fn cancellation_sets_flag_and_denies_pending_approvals() {
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let questions: PendingQuestionMap = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();
        pending
            .lock()
            .unwrap()
            .insert(7, ("session-1".into(), sender));
        let (question_sender, question_receiver) = oneshot::channel();
        questions
            .lock()
            .unwrap()
            .insert(8, ("session-1".into(), question_sender));

        assert_eq!(
            request_cancel("session-1", &cancel, &pending, &questions),
            2
        );
        assert!(cancel.load(Ordering::Acquire));
        assert!(pending.lock().unwrap().is_empty());
        assert!(questions.lock().unwrap().is_empty());
        assert!(matches!(receiver.await.unwrap(), ApprovalDecision::Deny));
        assert_eq!(question_receiver.await.unwrap(), None);

        reset_cancel(&cancel);
        assert!(!cancel.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn cancellation_does_not_touch_another_session() {
        let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let questions: PendingQuestionMap = Arc::new(Mutex::new(HashMap::new()));
        let (first, first_rx) = oneshot::channel();
        let (second, second_rx) = oneshot::channel();
        pending
            .lock()
            .unwrap()
            .insert(1, ("session-1".into(), first));
        pending
            .lock()
            .unwrap()
            .insert(2, ("session-2".into(), second));

        assert_eq!(
            request_cancel("session-1", &cancel, &pending, &questions),
            1
        );
        assert!(matches!(first_rx.await.unwrap(), ApprovalDecision::Deny));
        assert!(pending.lock().unwrap().contains_key(&2));
        let (_, second) = pending.lock().unwrap().remove(&2).unwrap();
        second.send(ApprovalDecision::Once).unwrap();
        assert!(matches!(second_rx.await.unwrap(), ApprovalDecision::Once));
    }

    #[test]
    fn distinct_sessions_can_run_concurrently_but_one_session_cannot_overlap_itself() {
        let running: RunningSessions = Arc::new(Mutex::new(HashMap::new()));
        assert!(claim_session(&running, "session-1", SessionRunKind::Human));
        assert!(claim_session(&running, "session-2", SessionRunKind::Goal));
        assert!(!claim_session(&running, "session-1", SessionRunKind::Goal));
        assert_eq!(running.lock().unwrap().len(), 2);
    }

    #[test]
    fn shutdown_cancels_and_joins_every_owned_worker() {
        let lifecycle = WorkerLifecycle::default();
        let cancels: CancelRegistry = Arc::new(Mutex::new(HashMap::new()));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let questions: PendingQuestionMap = Arc::new(Mutex::new(HashMap::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        cancels
            .lock()
            .unwrap()
            .insert("goal-thread".into(), cancel.clone());
        let (approval_tx, approval_rx) = oneshot::channel();
        pending
            .lock()
            .unwrap()
            .insert(1, ("goal-thread".into(), approval_tx));
        let (question_tx, question_rx) = oneshot::channel();
        questions
            .lock()
            .unwrap()
            .insert(2, ("goal-thread".into(), question_tx));
        let interactions_finished = Arc::new(AtomicBool::new(false));
        let interactions_done = interactions_finished.clone();
        lifecycle.track(std::thread::spawn(move || {
            assert!(matches!(
                approval_rx.blocking_recv(),
                Ok(ApprovalDecision::Deny)
            ));
            assert!(matches!(question_rx.blocking_recv(), Ok(None)));
            interactions_done.store(true, Ordering::Release);
        }));
        let worker_finished = Arc::new(AtomicBool::new(false));
        let finished = worker_finished.clone();
        lifecycle.track(std::thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            finished.store(true, Ordering::Release);
        }));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let coordinator_finished = Arc::new(AtomicBool::new(false));
        let coordinator_done = coordinator_finished.clone();
        lifecycle.track(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            runtime.block_on(async {
                assert!(matches!(rx.recv().await, Some(Command::Shutdown)));
            });
            coordinator_done.store(true, Ordering::Release);
        }));

        lifecycle.shutdown_and_join(&tx, &cancels, &pending, &questions);

        assert!(worker_finished.load(Ordering::Acquire));
        assert!(interactions_finished.load(Ordering::Acquire));
        assert!(coordinator_finished.load(Ordering::Acquire));
        assert!(!lifecycle.accepts_work());
        lifecycle.shutdown_and_join(&tx, &cancels, &pending, &questions);
    }

    #[test]
    fn concurrent_sessions_write_to_distinct_logs() {
        let workspace = Path::new("D:/workspace");
        assert_ne!(
            session_log_path(workspace, "session-1"),
            session_log_path(workspace, "session-2")
        );
    }

    #[test]
    fn session_ids_cannot_escape_the_log_directory() {
        assert_eq!(safe_session_file_stem("../thread\\evil"), ".._thread_evil");
        let root = PathBuf::from("workspace");
        let path = session_log_path(&root, "../thread\\evil");
        assert_eq!(path.parent().unwrap(), root.join(".nanocodex/sessions"));
    }

    #[tokio::test]
    async fn gui_registry_smoke_covers_builtin_and_optional_tool_combinations() {
        let root = std::env::temp_dir().join(format!("ncx_gui_tools_{}", new_session_id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn smoke() {}\n").unwrap();
        let memory = Rc::new(MemoryStore::new(root.join(".ncx/memory")));
        let skill = Skill {
            name: "smoke-skill".into(),
            description: "GUI registry smoke fixture".into(),
            capability: Default::default(),
            always_apply: false,
            path: PathBuf::from("<builtin>/smoke-skill/SKILL.md"),
            dir: PathBuf::from("<builtin>/smoke-skill"),
            embedded: Some("Use the fixture.".into()),
        };
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &root))
            .with_memory(memory)
            .with_skills(vec![skill]);
        let registry = ncx_core::HarnessRuntimeBuilder::default().build(ctx);

        for name in [
            "read_file",
            "apply_patch",
            "str_replace_editor",
            "update_plan",
            "shell",
            "grep",
            "grep_literal",
            "glob",
            "find_files",
            "web_search",
            "web_fetch",
            "list_directory",
            "path_info",
            "git_status",
            "git_diff",
            "tool_search",
            "remember",
            "skill",
        ] {
            assert!(registry.get(name).is_some(), "GUI runtime missing {name}");
        }

        let read = registry
            .execute_with_recovery("read_file", &json!({"path": "src/lib.rs"}))
            .await;
        assert!(read.contains("pub fn smoke"), "{read}");
        let search = registry
            .execute_with_recovery("grep", &json!({"pattern": "smoke"}))
            .await;
        assert!(search.contains("src/lib.rs:1"), "{search}");
        let discovery = registry
            .execute_with_recovery("glob", &json!({"pattern": "**/*.rs"}))
            .await;
        assert!(discovery.contains("src/lib.rs"), "{discovery}");
        let literal_discovery = registry
            .execute_with_recovery("find_files", &json!({"query": "lib.rs", "exact": true}))
            .await;
        assert!(
            literal_discovery.contains("src/lib.rs"),
            "{literal_discovery}"
        );
        let listing = registry
            .execute_with_recovery("list_directory", &json!({"path": "src"}))
            .await;
        assert!(listing.contains("lib.rs"), "{listing}");
        let path_info = registry
            .execute_with_recovery("path_info", &json!({"path": "src/lib.rs"}))
            .await;
        assert!(path_info.contains("\"exists\":true"), "{path_info}");
        let plan = registry
            .execute_with_recovery(
                "update_plan",
                &json!({"plan": [{"step": "GUI smoke", "status": "completed"}]}),
            )
            .await;
        assert!(!plan.starts_with("Error:"), "{plan}");
        let edit = registry
            .execute_with_recovery(
                "str_replace_editor",
                &json!({"command": "create", "path": "created.txt", "new_str": "created by GUI smoke\n"}),
            )
            .await;
        assert!(edit.contains("Patch applied successfully"), "{edit}");
        let discovered = registry
            .execute_with_recovery("tool_search", &json!({"query": "version control"}))
            .await;
        assert!(discovered.contains("version-control"), "{discovered}");
        let skill_result = registry
            .execute_with_recovery("skill", &json!({"name": "smoke-skill"}))
            .await;
        assert!(skill_result.contains("Use the fixture"), "{skill_result}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
