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
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ncx_config::{
    load_config, permission_mode_to_knobs, write_nanocodex_config, Config, ConfigPaths, Overrides,
};
use ncx_core::{
    discover_skills, expand_file_mentions, load_workspace_instructions, new_session_id,
    skills_index_block, AgentLoop, ApprovalDecision, ApprovalHandler, ApprovalRequest,
    CheckpointStore, ContextEditPolicy, LoopEvent, MemoryStore, Provider, Session, SessionGrants,
    SessionIndex, TaskBudget, ToolContext, ToolRegistry,
};
use ncx_provider::DeepSeekProvider;
use ncx_sandbox::SandboxPolicy;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use the provided tools \
    (read_file, apply_patch, update_plan) to inspect and edit the workspace. Prefer apply_patch \
    for edits. Keep responses concise.";

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
pub type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<ApprovalDecision>>>>;

/// A request from the UI to the agent thread.
pub enum Command {
    /// A user turn. `images` are absolute paths attached via the file picker;
    /// each becomes a base64 `image_url` block (vision routing). Non-image files
    /// are passed by the UI as `@path` tokens inside `text` (expanded as mentions).
    Prompt { text: String, images: Vec<String> },
    /// Rebuild the agent from the (just-saved) config — applies model / sandbox
    /// / key changes live. Starts a fresh session.
    Reload,
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
    },
    /// A streamed chunk of assistant text (append to the in-progress bubble).
    AssistantDelta { text: String },
    /// Assistant's final visible text (finalize the streamed bubble).
    Assistant { text: String },
    /// A tool is about to run.
    ToolStart { name: String, args: String },
    /// A tool finished.
    ToolResult { name: String, result: String },
    /// An escalated action needs the user's yes/no. Answer via the `approve`
    /// command with this `id`.
    Approval {
        id: u64,
        command: String,
        reason: String,
        cwd: String,
        details: String,
    },
    /// The turn finished.
    Done {
        final_text: String,
        stop_reason: String,
        usage: Value,
    },
    /// A session was resumed/forked — the UI should replace its transcript with
    /// these restored messages.
    Loaded { messages: Vec<UiMsg> },
    /// Fatal setup/turn error.
    Error { message: String },
}

/// A restored conversation message for the `loaded` event.
#[derive(Clone, Serialize)]
pub struct UiMsg {
    pub role: String,
    pub text: String,
}

fn emit(app: &AppHandle, ev: UiEvent) {
    let _ = app.emit(EVENT, ev);
}

/// Build the loop's event sink (forwards [`LoopEvent`]s to the frontend). A
/// fresh one is needed after every (re)build of the agent.
fn make_sink(app: AppHandle) -> Box<dyn FnMut(LoopEvent)> {
    Box::new(move |ev: LoopEvent| {
        let ui = match ev {
            LoopEvent::AssistantDelta(text) => UiEvent::AssistantDelta { text },
            LoopEvent::AssistantText(text) => UiEvent::Assistant { text },
            LoopEvent::ToolStart { name, args } => UiEvent::ToolStart { name, args },
            LoopEvent::ToolResult { name, result } => UiEvent::ToolResult { name, result },
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
                workspace: workspace.display().to_string(),
                session_id: session_id.to_string(),
                models: cfg.available_models,
                permission_mode: cfg.permission_mode,
            },
        );
    }
}

/// Approval handler that round-trips through the frontend modal.
struct GuiApprover {
    app: AppHandle,
    pending: PendingMap,
    counter: AtomicU64,
}

#[async_trait(?Send)]
impl ApprovalHandler for GuiApprover {
    async fn request(&self, req: ApprovalRequest) -> ApprovalDecision {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        emit(
            &self.app,
            UiEvent::Approval {
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
fn build_agent(
    approver: Rc<dyn ApprovalHandler>,
    seed: Option<(String, Vec<Value>)>,
    grants: Rc<RefCell<SessionGrants>>,
) -> Result<(AgentLoop, PathBuf, String, PathBuf, SessionIndex), String> {
    let workspace = std::env::current_dir().ok();
    let overrides = Overrides {
        workspace,
        ..Default::default()
    };
    let cfg = load_config(overrides).map_err(|e| e.to_string())?;
    cfg.validate().map_err(|e| e.to_string())?;

    let provider = DeepSeekProvider::with_opts(
        cfg.api_key.clone(),
        &cfg.base_url,
        cfg.model.clone(),
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    );
    // The CC permission mode is the single source of truth for gating: it derives
    // the sandbox, approval policy, per-edit approval, and plan flag.
    let (sandbox_mode, approval_policy, require_edit, plan) =
        permission_mode_to_knobs(&cfg.permission_mode);
    let network = sandbox_mode == "danger-full-access";
    let policy = SandboxPolicy::new(sandbox_mode, &cfg.workspace).with_network_access(network);
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    let recall = memory.recall("", 8, 4000); // recency at session start (no task yet)
    // Workspace-only: do NOT inject the developer's global ~/.claude/~/.codex
    // files (their handoff protocol would make a plain "hi" read HANDOFF.md etc.).
    let instructions = load_workspace_instructions(&cfg.workspace, 16_000);
    let skills = discover_skills(&cfg.workspace);
    let skills_index = skills_index_block(&skills);
    let plan_note = if plan {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let system_prompt =
        compose_system_prompt(SYSTEM_PROMPT, &[instructions, recall, skills_index, plan_note]);
    let ctx = ToolContext::new(cfg.workspace.clone(), policy)
        .with_approval_policy(approval_policy)
        .with_require_edit_approval(require_edit)
        .with_plan_mode(plan)
        .with_session_grants(grants)
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(cfg.hooks.clone())
        .with_skills(skills)
        .with_approver(approver);
    let tools = ToolRegistry::new(ctx);
    let log_path = cfg.workspace.join(".nanocodex").join("session.jsonl");
    let (session_id, session) = match seed {
        Some((id, messages)) => (
            id,
            Session::fork(system_prompt, messages, Some(log_path.clone())),
        ),
        None => (
            new_session_id(),
            Session::with_log(system_prompt, Some(log_path.clone())),
        ),
    };
    let agent = AgentLoop::new(Box::new(provider), tools, session)
        .with_task_budget(task_budget_from_config(&cfg))
        .with_context_edit(context_edit_from_config(&cfg))
        .with_vision_provider(build_vision_provider(&cfg));
    Ok((
        agent,
        cfg.workspace.clone(),
        session_id,
        log_path,
        SessionIndex::default(),
    ))
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

fn positive_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn nonnegative_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value).ok().unwrap_or(fallback)
}

fn task_budget_from_config(cfg: &Config) -> TaskBudget {
    TaskBudget {
        max_model_calls: positive_usize(cfg.max_iterations, 60),
        max_tool_calls: nonnegative_usize(cfg.max_tool_calls, 120),
    }
}

fn context_edit_from_config(cfg: &Config) -> ContextEditPolicy {
    ContextEditPolicy {
        enabled: cfg.context_edit_enabled,
        max_chars: positive_usize(cfg.context_edit_max_chars, 120_000),
        keep_recent_messages: positive_usize(cfg.context_edit_keep_recent_messages, 30),
        max_tool_result_chars: positive_usize(cfg.context_edit_max_tool_result_chars, 4_000),
    }
}

/// Spawn the dedicated agent thread. Returns immediately; the thread lives for
/// the app's lifetime, draining `rx` one prompt at a time (turns are serial).
pub fn spawn_worker(app: AppHandle, mut rx: UnboundedReceiver<Command>, pending: PendingMap) {
    std::thread::Builder::new()
        .name("ncx-agent".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent-thread tokio runtime builds");

            rt.block_on(async move {
                let approver: Rc<dyn ApprovalHandler> = Rc::new(GuiApprover {
                    app: app.clone(),
                    pending: pending.clone(),
                    counter: AtomicU64::new(1),
                });
                // Session "always allow" grants — fresh per session, kept across
                // model / permission-mode rebuilds, replaced on new/resume/fork.
                let mut grants = Rc::new(RefCell::new(SessionGrants::default()));
                let (mut agent, mut workspace, mut session_id, mut log_path, mut session_index) =
                    match build_agent(approver.clone(), None, grants.clone()) {
                        Ok(v) => v,
                        Err(e) => {
                            emit(&app, UiEvent::Error { message: e });
                            return;
                        }
                    };
                agent.set_event_sink(make_sink(app.clone()));
                emit_ready(&app, &workspace, &session_id);

                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::Prompt { text, images } => {
                            let expanded = expand_file_mentions(&text, &workspace);
                            save_auto_checkpoint(&workspace, &expanded);
                            let user_input = match build_image_user_input(&expanded, &images) {
                                Ok(v) => v,
                                Err(e) => {
                                    emit(&app, UiEvent::Error { message: e });
                                    continue;
                                }
                            };
                            let result = agent.run_turn(user_input, None).await;
                            let _ = session_index.record_turn(
                                &session_id,
                                &workspace,
                                &agent.session,
                                &log_path,
                            );
                            emit(
                                &app,
                                UiEvent::Done {
                                    final_text: result.final_text,
                                    stop_reason: result.stop_reason,
                                    usage: serde_json::to_value(&result.usage)
                                        .unwrap_or(Value::Null),
                                },
                            );
                        }
                        Command::Reload => {
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(approver.clone(), None, grants.clone()) {
                                Ok((a, ws, sid, lp, idx)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    log_path = lp;
                                    session_index = idx;
                                    agent.set_event_sink(make_sink(app.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(&app, UiEvent::Error { message: e }),
                            }
                        }
                        Command::Resume(id) | Command::Fork(id)
                            if session_index.load_snapshot(&id).is_none() =>
                        {
                            emit(
                                &app,
                                UiEvent::Error {
                                    message: format!("no saved snapshot for session {id}"),
                                },
                            );
                        }
                        Command::Resume(id) => {
                            let msgs = session_index.load_snapshot(&id).unwrap_or_default();
                            let ui = snapshot_to_ui(&msgs);
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(approver.clone(), Some((id, msgs)), grants.clone()) {
                                Ok((a, ws, sid, lp, idx)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    log_path = lp;
                                    session_index = idx;
                                    agent.set_event_sink(make_sink(app.clone()));
                                    emit(&app, UiEvent::Loaded { messages: ui });
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(&app, UiEvent::Error { message: e }),
                            }
                        }
                        Command::Fork(id) => {
                            let msgs = session_index.load_snapshot(&id).unwrap_or_default();
                            let ui = snapshot_to_ui(&msgs);
                            grants = Rc::new(RefCell::new(SessionGrants::default()));
                            match build_agent(approver.clone(), Some((new_session_id(), msgs)), grants.clone()) {
                                Ok((a, ws, sid, lp, idx)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    log_path = lp;
                                    session_index = idx;
                                    agent.set_event_sink(make_sink(app.clone()));
                                    emit(&app, UiEvent::Loaded { messages: ui });
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(&app, UiEvent::Error { message: e }),
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
                            let msgs = session_index.load_snapshot(&session_id).unwrap_or_default();
                            // Same session → keep the "always allow" grants.
                            match build_agent(approver.clone(), Some((session_id.clone(), msgs)), grants.clone()) {
                                Ok((a, ws, sid, lp, idx)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    log_path = lp;
                                    session_index = idx;
                                    agent.set_event_sink(make_sink(app.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(&app, UiEvent::Error { message: e }),
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
                            let msgs = session_index.load_snapshot(&session_id).unwrap_or_default();
                            // Same session → keep the "always allow" grants.
                            match build_agent(approver.clone(), Some((session_id.clone(), msgs)), grants.clone()) {
                                Ok((a, ws, sid, lp, idx)) => {
                                    agent = a;
                                    workspace = ws;
                                    session_id = sid;
                                    log_path = lp;
                                    session_index = idx;
                                    agent.set_event_sink(make_sink(app.clone()));
                                    emit_ready(&app, &workspace, &session_id);
                                }
                                Err(e) => emit(&app, UiEvent::Error { message: e }),
                            }
                        }
                        Command::RequestReady => emit_ready(&app, &workspace, &session_id),
                    }
                }
            });
        })
        .expect("spawn ncx-agent thread");
}

/// Convert snapshot messages (OpenAI shape) into UI transcript entries for the
/// `loaded` event. Skips the system message; renders tool calls as a note line.
fn snapshot_to_ui(messages: &[Value]) -> Vec<UiMsg> {
    let mut out = Vec::new();
    for m in messages {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "user" => out.push(UiMsg {
                role: "user".into(),
                text: content.to_string(),
            }),
            "assistant" => {
                if !content.trim().is_empty() {
                    out.push(UiMsg {
                        role: "assistant".into(),
                        text: content.to_string(),
                    });
                }
                if let Some(calls) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    let names: Vec<String> = calls
                        .iter()
                        .filter_map(|c| {
                            c.get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|n| n.as_str())
                                .map(String::from)
                        })
                        .collect();
                    if !names.is_empty() {
                        out.push(UiMsg {
                            role: "note".into(),
                            text: format!("⚙ {}", names.join(", ")),
                        });
                    }
                }
            }
            _ => {} // skip system + tool-result messages in the transcript
        }
    }
    out
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

/// Build a dedicated vision provider from `vl_*` config, or `None` when no VL
/// model is set (image turns then stay on the main provider). Only `vl_model`
/// is required; `vl_base_url`/`vl_api_key` fall back to the main ones.
fn build_vision_provider(cfg: &Config) -> Option<Box<dyn Provider>> {
    if cfg.vl_model.trim().is_empty() {
        return None;
    }
    let base_url = if cfg.vl_base_url.trim().is_empty() {
        &cfg.base_url
    } else {
        &cfg.vl_base_url
    };
    let api_key = if cfg.vl_api_key.trim().is_empty() {
        cfg.api_key.clone()
    } else {
        cfg.vl_api_key.clone()
    };
    Some(Box::new(DeepSeekProvider::with_opts(
        api_key,
        base_url,
        cfg.vl_model.clone(),
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    )))
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
