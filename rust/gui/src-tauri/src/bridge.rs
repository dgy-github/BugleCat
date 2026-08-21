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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ncx_config::{
    load_config, permission_mode_to_knobs, write_nanocodex_config, ConfigPaths, Overrides,
};
use ncx_core::{
    discover_skills, expand_file_mentions, load_workspace_instructions, model_provider_from_config,
    new_session_id, skills_index_block, suggest_title_with_provider, vision_provider_from_config,
    AgentLoop, AgentRuntimeProfile, ApprovalDecision, ApprovalHandler, ApprovalRequest,
    CheckpointStore, LoopEvent, MemoryStore, Session, SessionGrants, SessionIndex, ToolContext,
    ToolRegistry, UserQuestionHandler, UserQuestionRequest, COMPACTED_HISTORY_PREFIX,
};
use ncx_sandbox::SandboxPolicy;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use native workspace tools \
    (find_files, grep, glob, list_directory, path_info, read_file) for recursive discovery and \
    inspection, and prefer them over shell commands. Use apply_patch for edits and update_plan for \
    multi-step work. If a path is incomplete, search recursively instead of guessing. Keep responses concise. \
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

/// Pending approval requests, keyed by id. Shared between the agent thread
/// (inserts a one-shot sender when asking) and the `approve` command (takes it
/// to answer). `Send + Sync` so it can live in Tauri state.
pub type PendingMap = Arc<Mutex<HashMap<u64, (String, oneshot::Sender<ApprovalDecision>)>>>;
pub type PendingQuestionMap = Arc<Mutex<HashMap<u64, (String, oneshot::Sender<Option<String>>)>>>;

/// Shared cooperative cancellation state for the active GUI turn.
pub type CancelFlag = Arc<AtomicBool>;
pub type CancelRegistry = Arc<Mutex<HashMap<String, CancelFlag>>>;
pub type RunningSessions = Arc<Mutex<HashSet<String>>>;
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
    },
    /// Rebuild the agent from the (just-saved) config — applies model / sandbox
    /// / key changes live. Starts a fresh session.
    Reload,
    /// Start a new empty conversation with an id allocated before it enters the
    /// serial worker queue. Project files remain shared; chat and plans do not.
    New(String),
    /// Continue a saved session: reseed the agent from its snapshot, keeping the
    /// same session id (future turns append to it).
    Resume(String),
    /// Branch a saved session: reseed a NEW session from the snapshot, leaving
    /// the source untouched (explore an alternative continuation).
    Fork(String),
    /// Change the approval policy live (no session reset) + persist it.
    SetApproval(String),
    /// Change the sandbox mode live (no session reset) + persist it. Used by the
    /// "auto-execute" mode (danger-full-access).
    SetSandbox(String),
    /// Switch the model: persist it and rebuild the agent reseeded with the
    /// current transcript, so the conversation survives the swap.
    SetModel(String),
    /// Switch the CC permission mode (plan / default / accept-edits / bypass):
    /// persist it (+ derived sandbox/approval) and rebuild reseeded so the new
    /// gating + plan nudge take effect without losing the conversation.
    SetPermissionMode(String),
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
    AssistantDelta { session_id: String, text: String },
    /// A chunk from the provider's explicit reasoning stream.
    ReasoningDelta { session_id: String, text: String },
    ContextCompacted {
        session_id: String,
        original_chars: usize,
        edited_chars: usize,
        dropped_messages: usize,
        compressed_tool_results: usize,
    },
    /// Assistant's final visible text (finalize the streamed bubble).
    Assistant { session_id: String, text: String },
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
    SessionTitle { session_id: String, title: String },
    /// A session was resumed/forked — the UI should replace its transcript with
    /// these restored messages.
    Loaded {
        session_id: String,
        messages: Vec<UiMsg>,
    },
    /// Fatal setup/turn error.
    Error { session_id: String, message: String },
}

/// A restored conversation message for the `loaded` event.
#[derive(Clone, Serialize)]
pub struct UiMsg {
    pub role: String,
    pub text: String,
}

pub(crate) fn emit(app: &AppHandle, ev: UiEvent) {
    let _ = app.emit(EVENT, ev);
}

fn should_generate_session_title(is_first_turn: bool, stop_reason: &str) -> bool {
    is_first_turn && stop_reason == "completed"
}

/// Build the loop's event sink (forwards [`LoopEvent`]s to the frontend). A
/// fresh one is needed after every (re)build of the agent.
fn make_sink(app: AppHandle, session_id: String) -> Box<dyn FnMut(LoopEvent)> {
    Box::new(move |ev: LoopEvent| {
        let ui = match ev {
            LoopEvent::AssistantDelta(text) => UiEvent::AssistantDelta {
                session_id: session_id.clone(),
                text,
            },
            LoopEvent::ReasoningDelta(text) => UiEvent::ReasoningDelta {
                session_id: session_id.clone(),
                text,
            },
            LoopEvent::ContextCompacted(stats) => UiEvent::ContextCompacted {
                session_id: session_id.clone(),
                original_chars: stats.original_chars,
                edited_chars: stats.edited_chars,
                dropped_messages: stats.dropped_messages,
                compressed_tool_results: stats.compressed_tool_results,
            },
            LoopEvent::AssistantText(text) => UiEvent::Assistant {
                session_id: session_id.clone(),
                text,
            },
            LoopEvent::ToolStart { name, args } => UiEvent::ToolStart {
                session_id: session_id.clone(),
                name,
                args,
            },
            LoopEvent::ToolResult { name, result } => UiEvent::ToolResult {
                session_id: session_id.clone(),
                name,
                result,
            },
        };
        emit(&app, ui);
    })
}

/// Tell the UI which model / sandbox / workspace / session is now active.
fn emit_ready(app: &AppHandle, workspace: &std::path::Path, session_id: &str) {
    if let Ok(cfg) = load_config(Overrides {
        workspace: Some(workspace.to_path_buf()),
        ..Default::default()
    }) {
        emit(
            app,
            UiEvent::Ready {
                model: cfg.model,
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

/// Switch the process cwd to a resumed session's original workspace (best-effort)
/// so the conversation reopens against the right project. Strips the Windows
/// verbatim `\\?\` prefix, which `set_current_dir` rejects.
fn restore_session_workspace(ws: Option<&str>) {
    let Some(ws) = ws else { return };
    let p = PathBuf::from(strip_verbatim_prefix(ws));
    if p.is_dir() {
        let _ = std::env::set_current_dir(&p);
        save_last_workspace(&p);
    }
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
    session_index: Arc<Mutex<SessionIndex>>,
    session_id: String,
    workspace: PathBuf,
    request: String,
) {
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
                let provider = model_provider_from_config(&cfg, cfg.model.clone());
                let Some(title) = suggest_title_with_provider(&provider, &request).await else {
                    return;
                };
                let persisted = session_index
                    .lock()
                    .map(|mut index| index.set_title(&session_id, &title))
                    .unwrap_or(false);
                if persisted {
                    emit(&app, UiEvent::SessionTitle { session_id, title });
                }
            });
        });
}

fn build_agent(
    approver: Rc<dyn ApprovalHandler>,
    questioner: Rc<dyn UserQuestionHandler>,
    seed: Option<(String, Vec<Value>)>,
    grants: Rc<RefCell<SessionGrants>>,
    workspace_override: Option<PathBuf>,
) -> Result<(AgentLoop, PathBuf, String, PathBuf, SessionIndex), String> {
    let restored_plan = seed
        .as_ref()
        .map(|(_, messages)| latest_plan_from_messages(messages))
        .unwrap_or_default();
    let workspace = workspace_override.or_else(|| std::env::current_dir().ok());
    let overrides = Overrides {
        workspace,
        ..Default::default()
    };
    let cfg = load_config(overrides).map_err(|e| e.to_string())?;
    cfg.validate().map_err(|e| e.to_string())?;

    let runtime_profile = AgentRuntimeProfile::from_config(&cfg);
    let provider = model_provider_from_config(&cfg, cfg.model.clone());
    let policy = runtime_profile.sandbox_policy(&cfg.workspace);
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    // Memory is recalled per prompt by AgentLoop (query-scoped), not dumped here.
    // Workspace-only: do NOT inject the developer's global ~/.claude/~/.codex
    // files (their handoff protocol would make a plain "hi" read HANDOFF.md etc.).
    let instructions = load_workspace_instructions(&cfg.workspace, 16_000);
    let skills = discover_skills(&cfg.workspace);
    let skills_index = skills_index_block(&skills);
    let plan_note = if runtime_profile.permissions.plan_mode {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let system_prompt =
        compose_system_prompt(SYSTEM_PROMPT, &[instructions, skills_index, plan_note]);
    let ctx = runtime_profile
        .apply_tool_context(ToolContext::new(cfg.workspace.clone(), policy))
        .with_session_grants(grants)
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(cfg.hooks.clone())
        .with_skills(skills)
        .with_approver(approver)
        .with_user_question_handler(questioner);
    if !restored_plan.is_empty() {
        ctx.plan.replace(restored_plan);
    }
    let tools = ncx_core::HarnessRuntimeBuilder::default().build(ctx);
    let (session_id, seed_messages) = match seed {
        Some((id, messages)) => (id, Some(messages)),
        None => (new_session_id(), None),
    };
    let log_dir = cfg.workspace.join(".nanocodex").join("sessions");
    std::fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
    let log_path = session_log_path(&cfg.workspace, &session_id);
    let session = match seed_messages {
        Some(messages) => Session::fork(system_prompt, messages, Some(log_path.clone())),
        None => Session::with_log(system_prompt, Some(log_path.clone())),
    };
    let agent = runtime_profile
        .apply(AgentLoop::new(Box::new(provider), tools, session))
        .with_vision_provider(vision_provider_from_config(&cfg));
    Ok((
        agent,
        cfg.workspace.clone(),
        session_id,
        log_path,
        SessionIndex::default(),
    ))
}

fn session_log_path(workspace: &Path, session_id: &str) -> PathBuf {
    workspace
        .join(".nanocodex")
        .join("sessions")
        .join(format!("{session_id}.jsonl"))
}

fn compose_system_prompt(base: &str, blocks: &[String]) -> String {
    let mut out = base.to_string();
    for block in blocks {
        if !block.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(block.trim());
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn spawn_turn_worker(
    app: AppHandle,
    pending: PendingMap,
    questions: PendingQuestionMap,
    cancels: CancelRegistry,
    running: RunningSessions,
    session_grants: GrantRegistry,
    session_index: Arc<Mutex<SessionIndex>>,
    session_id: String,
    workspace: PathBuf,
    messages: Vec<Value>,
    text: String,
    images: Vec<String>,
) {
    let inserted = claim_session(&running, &session_id);
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

    let cancel: CancelFlag = Arc::new(AtomicBool::new(false));
    if let Ok(mut registry) = cancels.lock() {
        registry.insert(session_id.clone(), cancel.clone());
    }

    let cleanup_cancels = cancels.clone();
    let cleanup_running = running.clone();
    let cleanup_session_id = session_id.clone();
    let failure_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name(format!(
            "ncx-turn-{}",
            session_id.chars().take(8).collect::<String>()
        ))
        .spawn(move || {
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
                let built = build_agent(
                    approver,
                    questioner,
                    Some((session_id.clone(), messages)),
                    grants.clone(),
                    Some(workspace.clone()),
                );
                let (mut agent, _, _, log_path, _) = match built {
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
                agent.set_event_sink(make_sink(app.clone(), session_id.clone()));
                let is_first_turn = agent
                    .session
                    .messages
                    .iter()
                    .all(|message| message.get("role").and_then(Value::as_str) != Some("user"));
                let expanded = expand_file_mentions(&text, &workspace);
                save_auto_checkpoint(&workspace, &expanded);
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
                if let Ok(mut registry) = session_grants.lock() {
                    registry.insert(session_id.clone(), grants.borrow().clone());
                }
                if let Ok(mut index) = session_index.lock() {
                    let _ = index.record_turn(&session_id, &workspace, &agent.session, &log_path);
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
                if should_generate_session_title(is_first_turn, &result.stop_reason) {
                    spawn_title_generation(
                        app.clone(),
                        session_index.clone(),
                        session_id.clone(),
                        workspace.clone(),
                        text,
                    );
                }
            });
            finish();
        });
    if spawned.is_err() {
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

fn claim_session(running: &RunningSessions, session_id: &str) -> bool {
    running
        .lock()
        .map(|mut sessions| sessions.insert(session_id.to_string()))
        .unwrap_or(false)
}

/// Spawn the lightweight navigation/config coordinator. It drains commands in
/// order, but each prompt is handed to its own session-scoped turn thread so
/// different conversations can continue concurrently.
pub fn spawn_worker(
    app: AppHandle,
    mut rx: UnboundedReceiver<Command>,
    pending: PendingMap,
    questions: PendingQuestionMap,
    cancels: CancelRegistry,
    running: RunningSessions,
    session_grants: GrantRegistry,
    session_index: Arc<Mutex<SessionIndex>>,
) {
    std::thread::Builder::new()
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
                // Restore the last chosen workspace so we don't fall back to the
                // launch cwd (often the user's home) on every start.
                if let Some(ws) = load_last_workspace() {
                    let _ = std::env::set_current_dir(&ws);
                }
                let startup_seed = std::env::current_dir().ok().and_then(|workspace| {
                    session_index
                        .lock()
                        .ok()
                        .and_then(|index| index.latest_resumable_for_workspace(&workspace))
                        .map(|(summary, messages)| (summary.session_id, messages))
                });
                // Session "always allow" grants — fresh per session, kept across
                // model / permission-mode rebuilds, replaced on new/resume/fork.
                let mut grants = Rc::new(RefCell::new(SessionGrants::default()));
                let (mut agent, mut workspace, mut session_id, startup_log_path, _) =
                    match build_agent(
                        approver.clone(),
                        questioner.clone(),
                        startup_seed,
                        grants.clone(),
                        None,
                    ) {
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
                agent.set_event_sink(make_sink(app.clone(), session_id.clone()));
                if let Ok(mut index) = session_index.lock() {
                    let _ = index.record_turn(
                        &session_id,
                        &workspace,
                        &agent.session,
                        &startup_log_path,
                    );
                }
                emit_ready(&app, &workspace, &session_id);

                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::Prompt {
                            session_id: target_id,
                            text,
                            images,
                        } => {
                            let (messages, target_workspace) = session_index
                                .lock()
                                .map(|index| {
                                    let messages =
                                        index.load_snapshot(&target_id).unwrap_or_default();
                                    let target_workspace = index
                                        .get(&target_id)
                                        .map(|summary| PathBuf::from(&summary.workspace))
                                        .unwrap_or_else(|| workspace.clone());
                                    (messages, target_workspace)
                                })
                                .unwrap_or_else(|_| (Vec::new(), workspace.clone()));
                            spawn_turn_worker(
                                app.clone(),
                                pending.clone(),
                                questions.clone(),
                                cancels.clone(),
                                running.clone(),
                                session_grants.clone(),
                                session_index.clone(),
                                target_id,
                                target_workspace,
                                messages,
                                text,
                                images,
                            );
                        }
                        Command::New(id) => {
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((id, Vec::new())),
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, lp, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    if let Ok(mut index) = session_index.lock() {
                                        let _ = index.record_turn(
                                            &session_id,
                                            &workspace,
                                            &agent.session,
                                            &lp,
                                        );
                                    }
                                    emit_ready(&app, &workspace, &session_id);
                                    emit(
                                        &app,
                                        UiEvent::Loaded {
                                            session_id: session_id.clone(),
                                            messages: Vec::new(),
                                        },
                                    );
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: String::new(),
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::Reload => {
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                None,
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, lp, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    if let Ok(mut index) = session_index.lock() {
                                        let _ = index.record_turn(
                                            &session_id,
                                            &workspace,
                                            &agent.session,
                                            &lp,
                                        );
                                    }
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: session_id.clone(),
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::Resume(id) => {
                            let loaded = session_index.lock().ok().and_then(|index| {
                                index.load_snapshot(&id).map(|messages| {
                                    (messages, index.get(&id).map(|s| s.workspace.clone()))
                                })
                            });
                            let Some((msgs, restored_workspace)) = loaded else {
                                emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: id.clone(),
                                        message: format!("no saved snapshot for session {id}"),
                                    },
                                );
                                continue;
                            };
                            let ui = snapshot_to_ui(&msgs);
                            // Reopen the conversation in ITS original workspace, not
                            // whatever dir we're currently in — otherwise a resumed
                            // session runs against the wrong project.
                            restore_session_workspace(restored_workspace.as_deref());
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((id.clone(), msgs)),
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, _, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                    emit(
                                        &app,
                                        UiEvent::Loaded {
                                            session_id: session_id.clone(),
                                            messages: ui,
                                        },
                                    );
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: id,
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::Fork(id) => {
                            let loaded = session_index.lock().ok().and_then(|index| {
                                index.load_snapshot(&id).map(|messages| {
                                    (messages, index.get(&id).map(|s| s.workspace.clone()))
                                })
                            });
                            let Some((msgs, restored_workspace)) = loaded else {
                                emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: id.clone(),
                                        message: format!("no saved snapshot for session {id}"),
                                    },
                                );
                                continue;
                            };
                            let ui = snapshot_to_ui(&msgs);
                            restore_session_workspace(restored_workspace.as_deref());
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((new_session_id(), msgs)),
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, lp, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    if let Ok(mut index) = session_index.lock() {
                                        let _ = index.record_turn(
                                            &session_id,
                                            &workspace,
                                            &agent.session,
                                            &lp,
                                        );
                                    }
                                    emit_ready(&app, &workspace, &session_id);
                                    emit(
                                        &app,
                                        UiEvent::Loaded {
                                            session_id: session_id.clone(),
                                            messages: ui,
                                        },
                                    );
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: id,
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::SetApproval(policy) => {
                            // Live update — no session reset — and persist it.
                            agent.tools.ctx.approval_policy = policy.clone();
                            let mut m = std::collections::HashMap::new();
                            m.insert("approval_policy", policy.as_str());
                            let _ = write_nanocodex_config(&m, &ConfigPaths::default().nanocodex);
                        }
                        Command::SetSandbox(mode) => {
                            // Live update the sandbox (auto-execute = danger-full-access).
                            agent.tools.ctx.policy = SandboxPolicy::new(&mode, &workspace);
                            let mut m = std::collections::HashMap::new();
                            m.insert("sandbox_mode", mode.as_str());
                            let _ = write_nanocodex_config(&m, &ConfigPaths::default().nanocodex);
                            emit_ready(&app, &workspace, &session_id);
                        }
                        Command::SetModel(model) => {
                            // Persist the model, then rebuild reseeded with the current
                            // transcript so the conversation survives the swap. We do NOT
                            // emit Loaded — the UI keeps its richer transcript as-is.
                            let mut m = std::collections::HashMap::new();
                            m.insert("model", model.as_str());
                            let _ = write_nanocodex_config(&m, &ConfigPaths::default().nanocodex);
                            let msgs = session_index
                                .lock()
                                .map(|index| index.load_snapshot(&session_id).unwrap_or_default())
                                .unwrap_or_default();
                            // Same session → keep the "always allow" grants.
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((session_id.clone(), msgs)),
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, _, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: session_id.clone(),
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::SetPermissionMode(mode) => {
                            // Persist the mode (+ derived sandbox/approval for consistency),
                            // then rebuild reseeded so the new gating + plan nudge apply
                            // without losing the conversation.
                            let (sandbox, approval, _re, _plan) = permission_mode_to_knobs(&mode);
                            let mut m = std::collections::HashMap::new();
                            m.insert("permission_mode", mode.as_str());
                            m.insert("sandbox_mode", sandbox);
                            m.insert("approval_policy", approval);
                            let _ = write_nanocodex_config(&m, &ConfigPaths::default().nanocodex);
                            let msgs = session_index
                                .lock()
                                .map(|index| index.load_snapshot(&session_id).unwrap_or_default())
                                .unwrap_or_default();
                            // Same session → keep the "always allow" grants.
                            match build_agent(
                                approver.clone(),
                                questioner.clone(),
                                Some((session_id.clone(), msgs)),
                                grants.clone(),
                                None,
                            ) {
                                Ok((a, ws, sid, _, _)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    set_active_session(&active_session, &session_id);
                                    agent
                                        .set_event_sink(make_sink(app.clone(), session_id.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(
                                    &app,
                                    UiEvent::Error {
                                        session_id: session_id.clone(),
                                        message: e,
                                    },
                                ),
                            }
                        }
                        Command::RequestReady => {
                            emit_ready(&app, &workspace, &session_id);
                            let messages = session_index
                                .lock()
                                .ok()
                                .and_then(|index| index.load_snapshot(&session_id));
                            if let Some(messages) = messages.filter(|items| !items.is_empty()) {
                                emit(
                                    &app,
                                    UiEvent::Loaded {
                                        session_id: session_id.clone(),
                                        messages: snapshot_to_ui(&messages),
                                    },
                                );
                            }
                        }
                    }
                }
            });
        })
        .expect("spawn ncx-agent thread");
}

/// Convert a full model snapshot into the lightweight visible transcript.
/// Tool calls/results and intermediate assistant narration stay on disk for
/// model continuity, but never cross the backend/UI boundary during restore.
fn snapshot_to_ui(messages: &[Value]) -> Vec<UiMsg> {
    let mut out = Vec::new();
    let mut final_assistant: Option<String> = None;
    let mut saw_user = false;

    let flush_final = |out: &mut Vec<UiMsg>, pending: &mut Option<String>| {
        if let Some(text) = pending.take() {
            out.push(UiMsg {
                role: "assistant".into(),
                text,
            });
        }
    };

    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = snapshot_text_content(m.get("content"));
        match role {
            "user" => {
                flush_final(&mut out, &mut final_assistant);
                if content.starts_with(COMPACTED_HISTORY_PREFIX) {
                    out.push(UiMsg {
                        role: "compact".into(),
                        text: "较早的会话内容已自动压缩，关键要求和完成结果已保留。".into(),
                    });
                    continue;
                }
                if !content.trim().is_empty() {
                    out.push(UiMsg {
                        role: "user".into(),
                        text: content,
                    });
                    saw_user = true;
                }
            }
            "assistant" => {
                if saw_user && !content.trim().is_empty() {
                    final_assistant = Some(content);
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
    use ncx_core::Skill;
    use ncx_sandbox::WORKSPACE_WRITE;

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
            json!({"role": "assistant", "content": "处理完成。"}),
        ];

        let restored = serde_json::to_value(snapshot_to_ui(&messages)).unwrap();
        assert_eq!(restored.as_array().unwrap().len(), 2);
        assert_eq!(restored[0]["role"], "user");
        assert_eq!(restored[1]["role"], "assistant");
        assert_eq!(restored[1]["text"], "处理完成。");
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
        })
        .unwrap();
        assert_eq!(event["session_id"], "session-1");

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
        let running: RunningSessions = Arc::new(Mutex::new(HashSet::new()));
        assert!(claim_session(&running, "session-1"));
        assert!(claim_session(&running, "session-2"));
        assert!(!claim_session(&running, "session-1"));
        assert_eq!(running.lock().unwrap().len(), 2);
    }

    #[test]
    fn concurrent_sessions_write_to_distinct_logs() {
        let workspace = Path::new("D:/workspace");
        assert_ne!(
            session_log_path(workspace, "session-1"),
            session_log_path(workspace, "session-2")
        );
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
