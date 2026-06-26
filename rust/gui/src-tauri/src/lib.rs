//! nanocodex GUI (Tauri v2) — Rust backend.
//!
//! The agent loop runs on a dedicated `!Send` thread (see [`bridge`]); the
//! frontend talks to it through the `send_prompt` command and listens for
//! `ncx://event` window events. `get_status` is a cheap synchronous snapshot
//! for the header.

mod bridge;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ncx_config::{
    load_config, write_nanocodex_config, ConfigPaths, Overrides, VALID_APPROVAL_POLICIES,
    VALID_SANDBOX_MODES,
};
use serde::Serialize;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use bridge::{spawn_worker, Command, PendingMap};

#[derive(Serialize)]
pub struct Status {
    model: String,
    sandbox: String,
    approval: String,
    workspace: String,
    /// Masked (`****1234`) — never the real key.
    api_key: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
}

/// Tauri managed state: the channel into the agent thread + pending approvals.
struct AppState {
    tx: UnboundedSender<Command>,
    pending: PendingMap,
}

/// Load the resolved config and return a display-safe snapshot.
#[tauri::command]
fn get_status() -> Result<Status, String> {
    let workspace = std::env::current_dir().ok();
    let overrides = Overrides {
        workspace,
        ..Default::default()
    };
    let cfg = load_config(overrides).map_err(|e| e.to_string())?;
    let red = cfg.redacted();
    Ok(Status {
        model: cfg.model.clone(),
        sandbox: cfg.sandbox_mode.clone(),
        approval: cfg.approval_policy.clone(),
        workspace: cfg.workspace.display().to_string(),
        api_key: red.get("api_key").cloned().unwrap_or_default(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
    })
}

/// Queue a user prompt for the agent thread. Replies arrive as `ncx://event`s.
#[tauri::command]
fn send_prompt(text: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::Prompt(text))
        .map_err(|_| "agent thread is not running".to_string())
}

/// The editable settings shown in the Settings panel. The API key is never
/// returned in the clear — only whether one is set, plus a masked tail.
#[derive(Serialize)]
pub struct Settings {
    model: String,
    base_url: String,
    sandbox_mode: String,
    approval_policy: String,
    reasoning_effort: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    context_edit_keep_recent_messages: i64,
    context_edit_max_tool_result_chars: i64,
    api_key_masked: String,
    has_api_key: bool,
    available_models: Vec<String>,
    sandbox_modes: Vec<String>,
    approval_policies: Vec<String>,
}

/// Read the current settings for the panel (with dropdown option lists).
#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    let workspace = std::env::current_dir().ok();
    let cfg = load_config(Overrides {
        workspace,
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let masked = cfg.redacted().get("api_key").cloned().unwrap_or_default();
    Ok(Settings {
        model: cfg.model.clone(),
        base_url: cfg.base_url.clone(),
        sandbox_mode: cfg.sandbox_mode.clone(),
        approval_policy: cfg.approval_policy.clone(),
        reasoning_effort: cfg.reasoning_effort.clone(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        context_edit_keep_recent_messages: cfg.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: cfg.context_edit_max_tool_result_chars,
        api_key_masked: masked,
        has_api_key: !cfg.api_key.is_empty(),
        available_models: cfg.available_models.clone(),
        sandbox_modes: VALID_SANDBOX_MODES.iter().map(|s| s.to_string()).collect(),
        approval_policies: VALID_APPROVAL_POLICIES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Persist settings to `~/.nanocodex/config.toml`, then rebuild the agent so the
/// change applies live. Empty values are skipped (so a blank API key keeps the
/// existing one). Only known keys are written.
#[tauri::command]
fn save_settings(
    updates: std::collections::HashMap<String, String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let borrowed: HashMap<&str, &str> = updates
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let path = ConfigPaths::default().nanocodex;
    write_nanocodex_config(&borrowed, &path).map_err(|e| e.to_string())?;
    // Apply live (fresh session with the new config).
    let _ = state.tx.send(Command::Reload);
    Ok(())
}

/// Answer a pending approval request (raised by an `approval` event).
#[tauri::command]
fn approve(id: u64, approved: bool, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let sender = state.pending.lock().unwrap().remove(&id);
    match sender {
        Some(tx) => tx
            .send(approved)
            .map_err(|_| "approval already resolved".to_string()),
        None => Err(format!("no pending approval with id {id}")),
    }
}

pub fn run() {
    let (tx, rx) = unbounded_channel::<Command>();
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let pending_for_worker = pending.clone();

    tauri::Builder::default()
        .manage(AppState { tx, pending })
        .setup(move |app| {
            // Hand the agent thread an AppHandle (to emit events), the receiver
            // (to take prompts), and the shared pending-approvals map.
            spawn_worker(app.handle().clone(), rx, pending_for_worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            send_prompt,
            approve,
            get_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running the nanocodex GUI");
}
