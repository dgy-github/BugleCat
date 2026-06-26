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

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ncx_config::{load_config, Config, Overrides};
use ncx_core::{
    expand_file_mentions, AgentLoop, ApprovalHandler, ApprovalRequest, ContextEditPolicy,
    LoopEvent, MemoryStore, Session, TaskBudget, ToolContext, ToolRegistry,
};
use ncx_provider::DeepSeekProvider;
use ncx_sandbox::SandboxPolicy;
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use the provided tools \
    (read_file, apply_patch, update_plan) to inspect and edit the workspace. Prefer apply_patch \
    for edits. Keep responses concise.";

/// The Tauri event name every UI update is delivered on.
pub const EVENT: &str = "ncx://event";

/// Pending approval requests, keyed by id. Shared between the agent thread
/// (inserts a one-shot sender when asking) and the `approve` command (takes it
/// to answer). `Send + Sync` so it can live in Tauri state.
pub type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<bool>>>>;

/// A request from the UI to the agent thread.
pub enum Command {
    Prompt(String),
    /// Rebuild the agent from the (just-saved) config — applies model / sandbox
    /// / key changes live. Starts a fresh session.
    Reload,
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
    },
    /// Assistant produced visible text.
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
    },
    /// Fatal setup/turn error.
    Error { message: String },
}

fn emit(app: &AppHandle, ev: UiEvent) {
    let _ = app.emit(EVENT, ev);
}

/// Build the loop's event sink (forwards [`LoopEvent`]s to the frontend). A
/// fresh one is needed after every (re)build of the agent.
fn make_sink(app: AppHandle) -> Box<dyn FnMut(LoopEvent)> {
    Box::new(move |ev: LoopEvent| {
        let ui = match ev {
            LoopEvent::AssistantText(text) => UiEvent::Assistant { text },
            LoopEvent::ToolStart { name, args } => UiEvent::ToolStart { name, args },
            LoopEvent::ToolResult { name, result } => UiEvent::ToolResult { name, result },
        };
        emit(&app, ui);
    })
}

/// Tell the UI which model / sandbox / workspace is now active.
fn emit_ready(app: &AppHandle, workspace: &std::path::Path) {
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
    async fn request(&self, req: ApprovalRequest) -> bool {
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
        rx.await.unwrap_or(false)
    }
}

/// Build the agent loop and its workspace from the resolved config.
fn build_agent(approver: Rc<dyn ApprovalHandler>) -> Result<(AgentLoop, PathBuf), String> {
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
    let policy = SandboxPolicy::new(cfg.sandbox_mode.clone(), &cfg.workspace)
        .with_network_access(cfg.network_access);
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    let recall = memory.recall("", 8, 4000); // recency at session start (no task yet)
    let system_prompt = if recall.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}\n\n{recall}")
    };
    let ctx = ToolContext::new(cfg.workspace.clone(), policy)
        .with_approval_policy(cfg.approval_policy.clone())
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(cfg.hooks.clone())
        .with_approver(approver);
    let tools = ToolRegistry::new(ctx);
    let session = Session::new(system_prompt);
    let agent = AgentLoop::new(Box::new(provider), tools, session)
        .with_task_budget(task_budget_from_config(&cfg))
        .with_context_edit(context_edit_from_config(&cfg));
    Ok((agent, cfg.workspace.clone()))
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
                let (mut agent, mut workspace) = match build_agent(approver.clone()) {
                    Ok(v) => v,
                    Err(e) => {
                        emit(&app, UiEvent::Error { message: e });
                        return;
                    }
                };
                agent.set_event_sink(make_sink(app.clone()));
                emit_ready(&app, &workspace);

                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::Prompt(text) => {
                            let expanded = expand_file_mentions(&text, &workspace);
                            let result = agent.run_turn(json!(expanded), None).await;
                            emit(
                                &app,
                                UiEvent::Done {
                                    final_text: result.final_text,
                                    stop_reason: result.stop_reason,
                                },
                            );
                        }
                        Command::Reload => match build_agent(approver.clone()) {
                            Ok((a, ws)) => {
                                agent = a;
                                workspace = ws;
                                agent.set_event_sink(make_sink(app.clone()));
                                emit_ready(&app, &workspace);
                            }
                            Err(e) => emit(&app, UiEvent::Error { message: e }),
                        },
                    }
                }
            });
        })
        .expect("spawn ncx-agent thread");
}
