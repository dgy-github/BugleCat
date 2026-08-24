//! nanocodex GUI (Tauri v2) — Rust backend.
//!
//! The agent loop runs on a dedicated `!Send` thread (see [`bridge`]); the
//! frontend talks to it through the `send_prompt` command and listens for
//! `ncx://event` window events. `get_status` is a cheap synchronous snapshot
//! for the header.

mod bridge;
pub mod model_catalog;

use model_catalog::{catalog, find_preset, parse_openrouter_models, CatalogModel, CatalogProvider};

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ncx_app_server::{AppServer, AppServerAdapter, DispatchOutcome};
use ncx_config::{
    load_config, write_nanocodex_config, Config, ConfigPaths, Overrides, VALID_APPROVAL_POLICIES,
    VALID_SANDBOX_MODES,
};
use ncx_core::{
    custom_command_prompt, discover_codex_apps, discover_marketplaces, list_custom_commands,
    resolve_local_marketplace_plugin, AgentRuntimeProfile, CheckpointMeta, CheckpointStore,
    CodexPluginCatalog, CodexPluginManifest, CodexPluginRecord, ExternalPluginCatalog,
    ExternalPluginRecord, HarnessDiagnostics, HarnessRuntimeBuilder, Marketplace,
    MarketplacePlugin, MarketplaceSource, MemoryStore, RestoreReport, SessionIndex, ToolContext,
};
use ncx_protocol::{
    ClientRequest, ItemId, Thread, ThreadId, ThreadItem, ThreadMetadata, Turn, TurnId, TurnStatus,
};
use ncx_thread_store::{default_thread_store_path, JsonThreadStore};
use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use bridge::{
    emit_protocol_outcome, request_cancel, safe_session_file_stem, spawn_worker, CancelRegistry,
    Command, GrantRegistry, PendingMap, PendingQuestionMap, RunningSessions,
};

#[derive(Serialize)]
pub struct Status {
    model: String,
    sandbox: String,
    approval: String,
    permission_mode: String,
    reasoning_effort: String,
    workspace: String,
    /// Masked (`****1234`) — never the real key.
    api_key: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    price_in: f64,
    price_out: f64,
    price_currency: String,
}

/// Tauri managed state: the channel into the agent thread + pending approvals.
struct AppState {
    tx: UnboundedSender<Command>,
    pending: PendingMap,
    questions: PendingQuestionMap,
    question_counter: AtomicU64,
    cancels: CancelRegistry,
    app_server: Arc<AppServer<JsonThreadStore>>,
    openrouter_models: Mutex<Vec<CatalogModel>>,
}

#[derive(Serialize)]
pub struct CheckpointView {
    id: String,
    label: String,
    created_at: String,
    files: usize,
    skipped: usize,
    total_bytes: u64,
}

#[derive(Serialize)]
pub struct ConfigLocation {
    config_path: String,
    config_dir: String,
}

#[derive(Serialize)]
pub struct RestoreView {
    checkpoint_id: String,
    safety_checkpoint_id: Option<String>,
    restored_files: usize,
    deleted_files: usize,
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
        permission_mode: cfg.permission_mode.clone(),
        reasoning_effort: cfg.reasoning_effort.clone(),
        workspace: bridge::display_path(&cfg.workspace),
        api_key: red.get("api_key").cloned().unwrap_or_default(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        price_in: cfg.price_in,
        price_out: cfg.price_out,
        price_currency: cfg.price_currency.clone(),
    })
}

/// Queue a user prompt for the agent thread. `images` are absolute paths from
/// the file picker (attached as base64 vision blocks); non-image files are
/// passed by the UI as `@path` tokens inside `text`. Replies arrive as
/// `ncx://event`s.
fn validate_image_attachment_route(images: &[String], vl_model: &str) -> Result<(), String> {
    if !images.is_empty() && vl_model.trim().is_empty() {
        return Err(
            "尚未配置图片/文件解析模型。请打开“设置”，填写“图片/文件解析模型”；如果当前模型本身支持图片，可填写与主模型相同的模型名，接口和密钥留空即可沿用主配置。"
                .to_string(),
        );
    }
    Ok(())
}

fn queue_prompt(
    state: &AppState,
    session_id: String,
    text: String,
    images: Vec<String>,
) -> Result<(), String> {
    if !images.is_empty() {
        let cfg = load_config(Overrides {
            workspace: std::env::current_dir().ok(),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        validate_image_attachment_route(&images, &cfg.vl_model)?;
    }

    state
        .tx
        .send(Command::Prompt {
            session_id,
            text,
            images,
        })
        .map_err(|_| "agent thread is not running".to_string())
}

#[tauri::command]
fn get_config_location() -> Result<ConfigLocation, String> {
    config_location()
}

/// Switch the agent's workspace (the directory it operates on). Sets the process
/// working directory — which every command resolves against — then reloads the
/// agent so the new root, its project instructions, memory, and git all apply.
#[tauri::command]
fn set_workspace(path: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let p = PathBuf::from(bridge::display_path(Path::new(path.trim())));
    if !p.is_dir() {
        return Err(format!("not a directory: {}", p.display()));
    }
    std::env::set_current_dir(&p).map_err(|e| format!("cannot enter {}: {e}", p.display()))?;
    bridge::save_last_workspace(&p); // remember it across launches
    let _ = state.tx.send(Command::Reload);
    Ok(bridge::display_path(&p))
}

/// Change the approval policy live (no session reset) + persist it.
#[tauri::command]
fn set_approval(policy: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetApproval(policy))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Change the sandbox mode live (auto-execute = danger-full-access) + persist.
#[tauri::command]
fn set_sandbox(mode: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetSandbox(mode))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Switch the active model (persists + rebuilds keeping the current transcript).
#[tauri::command]
fn set_model(model: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let cached = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    if let Some(preset) = find_preset_by_model_id(&model, &cached) {
        let quick_switch_models = provider_models(&preset.provider_id, &cached)
            .into_iter()
            .map(|item| item.model_id)
            .collect::<Vec<_>>();
        drop(cached);
        write_preset(&preset, &quick_switch_models)?;
    }
    state
        .tx
        .send(Command::SetModel(model))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Switch the CC permission mode (plan / default / accept-edits / bypass).
#[tauri::command]
fn set_permission_mode(mode: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::SetPermissionMode(mode))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Ask the agent thread to re-emit its `ready` snapshot (called by the UI once
/// its event listener is up, so the initial emit isn't missed).
#[tauri::command]
fn request_ready(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .tx
        .send(Command::RequestReady)
        .map_err(|_| "agent thread is not running".to_string())
}

/// Versioned app-server entry point used by the GUI. All returned events carry
/// threadId, optional turnId and a monotonic sequence.
#[tauri::command]
fn app_server_request(
    request: ClientRequest,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> Result<DispatchOutcome, String> {
    let runtime = GuiAppServerAdapter { state: &state };
    let outcome = state
        .app_server
        .dispatch_with_runtime(request, &runtime)
        .map_err(|error| error.to_string())?;
    emit_protocol_outcome(&app, &outcome);
    Ok(outcome)
}

struct GuiAppServerAdapter<'a> {
    state: &'a AppState,
}

impl AppServerAdapter for GuiAppServerAdapter<'_> {
    fn create_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.state
            .tx
            .send(Command::New(thread_id.to_string()))
            .map_err(|_| "agent thread is not running".to_string())
    }

    fn activate_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.state
            .tx
            .send(Command::Resume(thread_id.to_string()))
            .map_err(|_| "agent thread is not running".to_string())
    }

    fn fork_thread(&self, source_id: &ThreadId, target_id: &ThreadId) -> Result<(), String> {
        self.state
            .tx
            .send(Command::Fork {
                source_id: source_id.to_string(),
                target_id: target_id.to_string(),
            })
            .map_err(|_| "agent thread is not running".to_string())
    }

    fn submit_turn(
        &self,
        thread_id: &ThreadId,
        text: String,
        images: Vec<String>,
    ) -> Result<(), String> {
        queue_prompt(self.state, thread_id.to_string(), text, images)
    }

    fn interrupt_latest(&self, thread_id: &ThreadId) -> Result<(), String> {
        cancel_session(self.state, thread_id.as_str());
        Ok(())
    }

    fn list_codex_plugins(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(list_codex_plugins()?).map_err(|error| error.to_string())
    }

    fn install_codex_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(install_codex_plugin(source, upgrade)?)
            .map_err(|error| error.to_string())
    }

    fn set_codex_plugin_enabled(&self, name: String, enabled: bool) -> Result<(), String> {
        set_codex_plugin_enabled(name, enabled)
    }

    fn uninstall_codex_plugin(&self, name: String) -> Result<(), String> {
        uninstall_codex_plugin(name)
    }

    fn list_marketplaces(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(list_plugin_marketplaces()?).map_err(|error| error.to_string())
    }

    fn install_marketplace_plugin(
        &self,
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(install_marketplace_plugin(
            marketplace_path,
            plugin_name,
            upgrade,
        )?)
        .map_err(|error| error.to_string())
    }
}

/// The current workspace (process working directory).
#[tauri::command]
fn get_workspace() -> Result<String, String> {
    std::env::current_dir()
        .map(|p| bridge::display_path(&p))
        .map_err(|e| e.to_string())
}

fn cancel_session(state: &AppState, session_id: &str) {
    let cancel = state
        .cancels
        .lock()
        .ok()
        .and_then(|registry| registry.get(session_id).cloned());
    if let Some(cancel) = cancel {
        request_cancel(session_id, &cancel, &state.pending, &state.questions);
    }
}

#[tauri::command]
fn open_config_file() -> Result<(), String> {
    let path = ensure_config_file()?;
    open_file(&path)
}

#[tauri::command]
fn open_config_dir() -> Result<(), String> {
    let dir = ensure_config_dir()?;
    open_dir(&dir)
}

/// The editable settings shown in the Settings panel. The API key is never
/// returned in the clear — only whether one is set, plus a masked tail.
#[derive(Serialize)]
pub struct Settings {
    model: String,
    base_url: String,
    vl_model: String,
    vl_base_url: String,
    sandbox_mode: String,
    approval_policy: String,
    reasoning_effort: String,
    max_iterations: i64,
    max_tool_calls: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    context_edit_keep_recent_messages: i64,
    context_edit_max_tool_result_chars: i64,
    price_in: f64,
    price_out: f64,
    price_currency: String,
    api_key_masked: String,
    has_api_key: bool,
    vl_api_key_masked: String,
    has_vl_api_key: bool,
    available_models: Vec<String>,
    sandbox_modes: Vec<String>,
    approval_policies: Vec<String>,
}

fn settings_from_config(cfg: &Config) -> Settings {
    let redacted = cfg.redacted();
    Settings {
        model: cfg.model.clone(),
        base_url: cfg.base_url.clone(),
        vl_model: cfg.vl_model.clone(),
        vl_base_url: cfg.vl_base_url.clone(),
        sandbox_mode: cfg.sandbox_mode.clone(),
        approval_policy: cfg.approval_policy.clone(),
        reasoning_effort: cfg.reasoning_effort.clone(),
        max_iterations: cfg.max_iterations,
        max_tool_calls: cfg.max_tool_calls,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        context_edit_keep_recent_messages: cfg.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: cfg.context_edit_max_tool_result_chars,
        price_in: cfg.price_in,
        price_out: cfg.price_out,
        price_currency: cfg.price_currency.clone(),
        api_key_masked: redacted.get("api_key").cloned().unwrap_or_default(),
        has_api_key: !cfg.api_key.is_empty(),
        vl_api_key_masked: redacted.get("vl_api_key").cloned().unwrap_or_default(),
        has_vl_api_key: !cfg.vl_api_key.is_empty(),
        available_models: cfg.available_models.clone(),
        sandbox_modes: VALID_SANDBOX_MODES.iter().map(|s| s.to_string()).collect(),
        approval_policies: VALID_APPROVAL_POLICIES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
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
    Ok(settings_from_config(&cfg))
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
    // Apply live while preserving the active conversation/session id. A config
    // save must not silently turn into an unrelated new chat.
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let _ = state.tx.send(Command::SetModel(cfg.model));
    Ok(())
}

#[derive(Serialize)]
struct ModelCatalogResponse {
    providers: Vec<CatalogProvider>,
    /// 当实时目录不可用时，前端仍可使用内置目录。
    stale: bool,
}

fn catalog_response(openrouter_models: &[CatalogModel], stale: bool) -> ModelCatalogResponse {
    let mut providers = catalog();
    if !openrouter_models.is_empty() {
        if let Some(provider) = providers
            .iter_mut()
            .find(|provider| provider.id == "openrouter")
        {
            provider.models = openrouter_models.to_vec();
        }
    }
    ModelCatalogResponse { providers, stale }
}

fn preset_updates<T: AsRef<str>>(
    preset: &CatalogModel,
    quick_switch_models: &[T],
) -> HashMap<&'static str, String> {
    let available_models = quick_switch_models
        .iter()
        .map(|model| model.as_ref())
        .collect::<Vec<_>>()
        .join(",");
    HashMap::from([
        ("model", preset.model_id.clone()),
        ("base_url", preset.base_url.clone()),
        ("price_in", preset.price_in.to_string()),
        ("price_out", preset.price_out.to_string()),
        ("price_currency", preset.price_currency.clone()),
        ("available_models", available_models),
    ])
}

fn write_preset(preset: &CatalogModel, quick_switch_models: &[String]) -> Result<(), String> {
    let updates = preset_updates(preset, quick_switch_models);
    let borrowed: HashMap<&str, &str> = updates
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let path = ConfigPaths::default().nanocodex;
    write_nanocodex_config(&borrowed, &path).map_err(|error| error.to_string())
}

fn provider_models(provider_id: &str, openrouter_models: &[CatalogModel]) -> Vec<CatalogModel> {
    if provider_id == "openrouter" && !openrouter_models.is_empty() {
        return openrouter_models.to_vec();
    }
    catalog()
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .map(|provider| provider.models)
        .unwrap_or_default()
}

fn find_preset_by_model_id(
    model_id: &str,
    openrouter_models: &[CatalogModel],
) -> Option<CatalogModel> {
    openrouter_models
        .iter()
        .find(|model| model.model_id == model_id)
        .cloned()
        .or_else(|| {
            catalog()
                .into_iter()
                .flat_map(|provider| provider.models)
                .find(|model| model.model_id == model_id)
        })
}

/// 返回内置目录，并在有缓存时带上 OpenRouter 的实时模型清单。
#[tauri::command]
fn get_model_catalog(state: tauri::State<'_, AppState>) -> Result<ModelCatalogResponse, String> {
    let cached = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    Ok(catalog_response(&cached, false))
}

/// 通过 OpenRouter 公共接口拉取模型和每 Token 费用；接口无需 API 密钥。
#[tauri::command]
async fn refresh_openrouter_models(
    state: tauri::State<'_, AppState>,
) -> Result<ModelCatalogResponse, String> {
    let response = reqwest::Client::new()
        .get("https://openrouter.ai/api/v1/models")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| format!("OpenRouter 模型目录请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("OpenRouter 模型目录请求失败：{error}"))?;
    let body = response
        .text()
        .await
        .map_err(|error| format!("OpenRouter 模型目录读取失败：{error}"))?;
    let models = parse_openrouter_models(&body)?;
    let mut cached = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    *cached = models;
    Ok(catalog_response(&cached, false))
}

/// 选择一个模型预设时，统一保存模型、接口、费用币种和当前厂商的快捷模型。
#[tauri::command]
fn apply_model_preset(
    provider_id: String,
    model_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<CatalogModel, String> {
    let cached = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    let preset = if provider_id == "openrouter" {
        cached
            .iter()
            .find(|model| model.model_id == model_id)
            .cloned()
            .or_else(|| find_preset(&provider_id, &model_id))
    } else {
        find_preset(&provider_id, &model_id)
    }
    .ok_or_else(|| "未找到所选模型预设，请先刷新 OpenRouter 模型目录".to_string())?;
    let quick_switch_models = provider_models(&provider_id, &cached)
        .into_iter()
        .map(|model| model.model_id)
        .collect::<Vec<_>>();
    drop(cached);

    write_preset(&preset, &quick_switch_models)?;
    // 写入已经成功；重建会话的消息若暂时无法发送，也不能误报为保存失败。
    let _ = state.tx.send(Command::SetModel(preset.model_id.clone()));
    Ok(preset)
}

fn config_location() -> Result<ConfigLocation, String> {
    let path = ConfigPaths::default().nanocodex;
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent directory".to_string())?
        .to_path_buf();
    Ok(ConfigLocation {
        config_path: path.display().to_string(),
        config_dir: dir.display().to_string(),
    })
}

fn ensure_config_dir() -> Result<PathBuf, String> {
    let path = ConfigPaths::default().nanocodex;
    let dir = path
        .parent()
        .ok_or_else(|| "config path has no parent directory".to_string())?
        .to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn ensure_config_file() -> Result<PathBuf, String> {
    let path = ConfigPaths::default().nanocodex;
    if !path.exists() {
        let empty: HashMap<&str, &str> = HashMap::new();
        write_nanocodex_config(&empty, &path).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(target_os = "windows")]
fn open_file(path: &Path) -> Result<(), String> {
    ProcessCommand::new("notepad.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open config file: {e}"))
}

#[cfg(target_os = "windows")]
fn open_dir(path: &Path) -> Result<(), String> {
    ProcessCommand::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open config directory: {e}"))
}

#[cfg(target_os = "macos")]
fn open_file(path: &Path) -> Result<(), String> {
    open_with("open", path, "config file")
}

#[cfg(target_os = "macos")]
fn open_dir(path: &Path) -> Result<(), String> {
    open_with("open", path, "config directory")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_file(path: &Path) -> Result<(), String> {
    open_with("xdg-open", path, "config file")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_dir(path: &Path) -> Result<(), String> {
    open_with("xdg-open", path, "config directory")
}

#[cfg(not(target_os = "windows"))]
fn open_with(program: &str, path: &Path, label: &str) -> Result<(), String> {
    ProcessCommand::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open {label}: {e}"))
}

/// Answer a pending approval request (raised by an `approval` event).
/// `decision` is "deny" | "once" | "always" (always = remember this session).
#[tauri::command]
fn approve(id: u64, decision: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let dec = match decision.as_str() {
        "always" => ncx_core::ApprovalDecision::Always,
        "once" | "approve" | "yes" | "true" => ncx_core::ApprovalDecision::Once,
        _ => ncx_core::ApprovalDecision::Deny,
    };
    let sender = state.pending.lock().unwrap().remove(&id);
    match sender {
        Some((_, tx)) => tx
            .send(dec)
            .map_err(|_| "approval already resolved".to_string()),
        None => Err(format!("no pending approval with id {id}")),
    }
}

/// Answer or dismiss a pending `ask_user_question` request.
#[tauri::command]
fn answer_question(
    id: u64,
    answer: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let answer = answer.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    let sender = state.questions.lock().unwrap().remove(&id);
    match sender {
        Some((_, tx)) => tx
            .send(answer)
            .map_err(|_| "question already resolved".to_string()),
        None => Err(format!("no pending question with id {id}")),
    }
}

/// Deterministic debug-only entry point used by the real WebView click test.
#[tauri::command]
async fn e2e_ask_question(
    app: tauri::AppHandle,
    question: String,
    options: Vec<String>,
    allow_free_text: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, String> {
    if !cfg!(debug_assertions) {
        return Err("E2E question command is disabled in release builds".to_string());
    }
    let id = state.question_counter.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    state
        .questions
        .lock()
        .unwrap()
        .insert(id, (String::new(), sender));
    bridge::emit(
        &app,
        bridge::UiEvent::Question {
            session_id: String::new(),
            id,
            question,
            options,
            allow_free_text,
        },
    );
    receiver
        .await
        .map_err(|_| "question response channel closed".to_string())
}

#[tauri::command]
fn get_checkpoints() -> Result<Vec<CheckpointView>, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    Ok(CheckpointStore::new(&cfg.workspace)
        .list()
        .into_iter()
        .map(checkpoint_view)
        .collect())
}

/// The files captured by a checkpoint (for the checkpoint detail expander).
#[tauri::command]
fn checkpoint_files(id: String) -> Result<Vec<String>, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    CheckpointStore::new(&cfg.workspace)
        .get(&id)
        .map(|m| m.files)
        .ok_or_else(|| format!("no checkpoint with id {id}"))
}

#[tauri::command]
fn create_checkpoint(label: String) -> Result<CheckpointView, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let label = if label.trim().is_empty() {
        "manual checkpoint"
    } else {
        label.trim()
    };
    CheckpointStore::new(&cfg.workspace)
        .create(label)
        .map(checkpoint_view)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn restore_checkpoint(id: String) -> Result<RestoreView, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    CheckpointStore::new(&cfg.workspace)
        .restore(&id)
        .map(restore_view)
        .map_err(|e| e.to_string())
}

fn checkpoint_view(meta: CheckpointMeta) -> CheckpointView {
    CheckpointView {
        id: meta.id,
        label: meta.label,
        created_at: meta.created_at,
        files: meta.files.len(),
        skipped: meta.skipped_paths.len(),
        total_bytes: meta.total_bytes,
    }
}

fn restore_view(report: RestoreReport) -> RestoreView {
    RestoreView {
        checkpoint_id: report.checkpoint_id,
        safety_checkpoint_id: report.safety_checkpoint_id,
        restored_files: report.restored_files,
        deleted_files: report.deleted_files,
    }
}

// ── Phase 1: git branches + diff + session history (no agent-thread bridge) ────

#[derive(Serialize)]
pub struct BranchInfo {
    name: String,
    current: bool,
}

/// Run a git command in the workspace; Ok(stdout) or Err(stderr).
fn run_git(args: &[&str]) -> Result<String, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let out = ProcessCommand::new("git")
        .args(args)
        .current_dir(&ws)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("git {args:?} failed")
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
fn git_branches() -> Result<Vec<BranchInfo>, String> {
    let current = run_git(&["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    let listing = run_git(&["branch", "--format=%(refname:short)"])?;
    Ok(listing
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|name| BranchInfo {
            current: name == current,
            name: name.to_string(),
        })
        .collect())
}

#[tauri::command]
fn git_create_branch(name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    run_git(&["checkout", "-b", name]).map(|_| ())
}

#[tauri::command]
fn git_switch_branch(name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    run_git(&["checkout", name]).map(|_| ())
}

#[derive(Serialize)]
pub struct CommitInfo {
    hash: String,
    subject: String,
    when: String,
}

/// Recent commits on a branch (for the branch detail expander).
#[tauri::command]
fn git_log(name: String, limit: u32) -> Result<Vec<CommitInfo>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    let n = format!("-{}", limit.clamp(1, 50));
    // %h <US> subject <US> relative-date  (0x1f field separator).
    let out = run_git(&["log", &n, "--pretty=format:%h\u{1f}%s\u{1f}%cr", name])?;
    Ok(out
        .lines()
        .filter_map(|line| {
            let p: Vec<&str> = line.split('\u{1f}').collect();
            (p.len() == 3).then(|| CommitInfo {
                hash: p[0].to_string(),
                subject: p[1].to_string(),
                when: p[2].to_string(),
            })
        })
        .collect())
}

/// The working-tree diff vs HEAD (staged + unstaged) for the diff panel.
#[tauri::command]
fn git_diff() -> Result<String, String> {
    let out = run_git(&["diff", "HEAD"])?;
    Ok(if out.trim().is_empty() {
        "(no changes in the working tree)".into()
    } else {
        out
    })
}

#[derive(Serialize)]
pub struct FileChange {
    path: String,
    added: i64, // -1 = unknown (binary/untracked)
    removed: i64,
    kind: String, // modified | added | deleted | renamed | untracked
}

/// The working-tree change set vs HEAD: one entry per changed file with +/-
/// line counts (like the reference's working-tree panel).
#[tauri::command]
fn git_changes() -> Result<Vec<FileChange>, String> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, FileChange> = BTreeMap::new();
    // Tracked changes vs HEAD: added \t removed \t path.
    if let Ok(numstat) = run_git(&["diff", "HEAD", "--numstat"]) {
        for line in numstat.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 3 {
                let path = parts[2].trim().to_string();
                map.insert(
                    path.clone(),
                    FileChange {
                        added: parts[0].parse().unwrap_or(-1),
                        removed: parts[1].parse().unwrap_or(-1),
                        kind: "modified".into(),
                        path,
                    },
                );
            }
        }
    }
    // Status pass: refine kind + add untracked files.
    if let Ok(st) = run_git(&["status", "--porcelain"]) {
        for line in st.lines() {
            if line.len() < 4 {
                continue;
            }
            let code = &line[..2];
            let path = line[3..].trim().trim_matches('"').to_string();
            let kind = if code.contains('?') {
                "untracked"
            } else if code.contains('A') {
                "added"
            } else if code.contains('D') {
                "deleted"
            } else if code.contains('R') {
                "renamed"
            } else {
                "modified"
            };
            map.entry(path.clone())
                .and_modify(|f| f.kind = kind.to_string())
                .or_insert(FileChange {
                    path,
                    added: -1,
                    removed: -1,
                    kind: kind.to_string(),
                });
        }
    }
    Ok(map.into_values().collect())
}

/// The diff for a single file (vs HEAD). Untracked files show their content as
/// added lines.
#[tauri::command]
fn git_file_diff(path: String) -> Result<String, String> {
    let out = run_git(&["diff", "HEAD", "--", &path]).unwrap_or_default();
    if !out.trim().is_empty() {
        return Ok(out);
    }
    // Untracked / no tracked diff: show the file content as added lines.
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    match std::fs::read_to_string(ws.join(&path)) {
        Ok(c) => Ok(c
            .lines()
            .take(500)
            .map(|l| format!("+{l}"))
            .collect::<Vec<_>>()
            .join("\n")),
        Err(_) => Ok("(no textual diff — binary or unreadable)".into()),
    }
}

#[derive(Serialize)]
pub struct DirEntry {
    name: String,
    path: String, // workspace-relative, forward slashes
    is_dir: bool,
}

/// List a directory under the workspace (`rel` = "" for the root). Skips heavy
/// noise dirs; dirs first, then files, alphabetical.
#[tauri::command]
fn list_dir(rel: String) -> Result<Vec<DirEntry>, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let wsc = ws.canonicalize().unwrap_or(ws.clone());
    let target = if rel.trim().is_empty() {
        wsc.clone()
    } else {
        wsc.join(&rel)
    };
    let target = target.canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&wsc) {
        return Err("path is outside the workspace".into());
    }
    const SKIP: &[&str] = &[".git", "node_modules", "target", ".nanocodex"];
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&target).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let p = entry.path();
        let is_dir = p.is_dir();
        let path = p
            .strip_prefix(&wsc)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(DirEntry { name, path, is_dir });
    }
    out.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    Ok(out)
}

/// Read a workspace file's text for the file-preview panel. Mirrors `list_dir`'s
/// containment; capped; refuses non-UTF-8 (binary) files.
#[tauri::command]
fn read_workspace_file(rel: String) -> Result<String, String> {
    let ws = std::env::current_dir().map_err(|e| e.to_string())?;
    let wsc = ws.canonicalize().unwrap_or(ws);
    let target = wsc.join(&rel).canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&wsc) {
        return Err("path is outside the workspace".into());
    }
    let meta = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if meta.len() > 400_000 {
        return Err(format!("文件太大，无法预览（{} KB）", meta.len() / 1024));
    }
    let bytes = std::fs::read(&target).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "二进制文件，无法预览".to_string())
}

/// Open an http(s) URL in the default browser (e.g. the /feedback command).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http(s) URLs are allowed".into());
    }
    #[cfg(target_os = "windows")]
    {
        ProcessCommand::new("explorer.exe")
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        ProcessCommand::new("open")
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ProcessCommand::new("xdg-open")
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[derive(Serialize)]
pub struct McpRow {
    name: String,
    command: String,
}

/// Configured MCP servers (from ~/.nanocodex/mcp.toml) for the /mcp command.
#[tauri::command]
fn list_mcp() -> Result<Vec<McpRow>, String> {
    Ok(ncx_config::load_mcp_servers()
        .into_iter()
        .map(|s| {
            let command = if s.args.is_empty() {
                s.command
            } else {
                format!("{} {}", s.command, s.args.join(" "))
            };
            McpRow {
                name: s.name,
                command,
            }
        })
        .collect())
}

/// Write pasted/clipboard image bytes to a temp file and return its path, so it
/// can be attached through the normal image pipeline.
#[tauri::command]
fn save_temp_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let dir = std::env::temp_dir().join("ncx_gui_paste");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ext = if ext.trim().is_empty() {
        "png".into()
    } else {
        ext
    };
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("paste_{n}.{ext}"));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

// ── Hermes: project-memory self-evolution panel ───────────────────────────────

#[derive(Serialize)]
pub struct MemoryNote {
    ts: u64,
    tags: Vec<String>,
    text: String,
}

/// The project memory store for the current workspace.
fn memory_store() -> MemoryStore {
    let ws = std::env::current_dir().unwrap_or_default();
    MemoryStore::new(ws.join(".ncx").join("memory"))
}

/// List accumulated learnings (newest first).
#[tauri::command]
fn memory_list() -> Result<Vec<MemoryNote>, String> {
    let mut entries = memory_store().entries();
    entries.sort_by_key(|entry| Reverse(entry.ts));
    Ok(entries
        .into_iter()
        .map(|e| MemoryNote {
            ts: e.ts,
            tags: e.tags,
            text: e.text,
        })
        .collect())
}

/// Trigger self-evolution maintenance: fold near-duplicate notes (heuristic,
/// local — no model). Returns how many entries were removed.
#[tauri::command]
fn memory_consolidate() -> Result<usize, String> {
    memory_store().consolidate(0.85).map_err(|e| e.to_string())
}

/// Manually record a verified learning into project memory.
#[tauri::command]
fn memory_add(note: String, tags: Vec<String>) -> Result<bool, String> {
    let note = note.trim();
    if note.is_empty() {
        return Err("note is required".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    memory_store()
        .remember(note, &tags, now)
        .map_err(|e| e.to_string())
}

/// Path to the project memory markdown file (`.ncx/memory/LEARNINGS.md`).
fn memory_file_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(".ncx")
        .join("memory")
        .join("LEARNINGS.md")
}

/// Open the project memory file in the OS editor (creating it if missing).
#[tauri::command]
fn open_memory_file() -> Result<(), String> {
    let path = memory_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if !path.exists() {
        std::fs::write(&path, "# Project memory (nanocodex)\n\n").map_err(|e| e.to_string())?;
    }
    open_file(&path)
}

/// Open a saved session's raw JSONL log in the OS editor.
#[tauri::command]
fn open_session_log(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Ok(thread_id) = ThreadId::new(session_id.clone()) {
        if let Ok(outcome) = state
            .app_server
            .dispatch(ClientRequest::ThreadRead { thread_id })
        {
            if let ncx_protocol::ResponsePayload::Thread(thread) = outcome.response.payload {
                let path = PathBuf::from(thread.metadata.workspace)
                    .join(".nanocodex/sessions")
                    .join(format!("{}.jsonl", safe_session_file_stem(&session_id)));
                if path.is_file() {
                    return open_file(&path);
                }
            }
        }
    }
    let index = SessionIndex::default();
    let summary = index
        .get(&session_id)
        .ok_or_else(|| format!("unknown session: {session_id}"))?;
    if summary.log_path.trim().is_empty() {
        return Err("session has no log path".into());
    }
    let path = PathBuf::from(&summary.log_path);
    if !path.exists() {
        return Err(format!("session log does not exist: {}", path.display()));
    }
    open_file(&path)
}

/// Open a saved session's frozen snapshot in the OS editor.
#[tauri::command]
fn open_session_snapshot(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if let Ok(thread_id) = ThreadId::new(session_id.clone()) {
        if let Ok(outcome) = state
            .app_server
            .dispatch(ClientRequest::ThreadRead { thread_id })
        {
            if let ncx_protocol::ResponsePayload::Thread(thread) = outcome.response.payload {
                let model_context = state
                    .app_server
                    .dispatch(ClientRequest::ThreadModelContextRead {
                        thread_id: thread.metadata.id.clone(),
                    })
                    .ok()
                    .and_then(|outcome| match outcome.response.payload {
                        ncx_protocol::ResponsePayload::ModelContext(context) => context,
                        _ => None,
                    });
                let root = std::env::temp_dir().join("ncx-thread-exports");
                fs::create_dir_all(&root).map_err(|error| error.to_string())?;
                let path = root.join(format!("{}.json", safe_session_file_stem(&session_id)));
                let bytes = serde_json::to_vec_pretty(&serde_json::json!({
                    "thread": thread,
                    "modelContext": model_context,
                }))
                .map_err(|error| error.to_string())?;
                fs::write(&path, bytes).map_err(|error| error.to_string())?;
                return open_file(&path);
            }
        }
    }
    let index = SessionIndex::default();
    let summary = index
        .get(&session_id)
        .ok_or_else(|| format!("unknown session: {session_id}"))?;
    if !summary.has_snapshot {
        return Err("session has no snapshot".into());
    }
    let path = index.snapshot_path(&session_id);
    if !path.exists() {
        return Err(format!(
            "session snapshot does not exist: {}",
            path.display()
        ));
    }
    open_file(&path)
}

#[derive(Serialize)]
pub struct CustomCommandView {
    scope: String,
    name: String,
    slash: String,
    path: String,
}

/// List project/user custom slash commands (`.nanocodex|.claude/commands/*.md`).
#[tauri::command]
fn get_custom_commands() -> Result<Vec<CustomCommandView>, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    Ok(list_custom_commands(&cfg.workspace)
        .into_iter()
        .map(|cmd| CustomCommandView {
            scope: cmd.scope.to_string(),
            name: cmd.name.clone(),
            slash: format!("/{}:{}", cmd.scope, cmd.name),
            path: cmd.path.display().to_string(),
        })
        .collect())
}

/// Expand a custom command (with args) into the prompt the agent should run.
#[tauri::command]
fn expand_custom_command(slash: String, arg: String) -> Result<String, String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    match custom_command_prompt(&cfg.workspace, &slash, &arg) {
        Ok(Some(prompt)) => Ok(prompt),
        Ok(None) => Err(format!("unknown custom command: {slash}")),
        Err(e) => Err(e),
    }
}

fn configured_workspace() -> Result<(Config, PathBuf), String> {
    let cfg = load_config(Overrides {
        workspace: std::env::current_dir().ok(),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let workspace = cfg.workspace.clone();
    Ok((cfg, workspace))
}

fn now_epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[tauri::command]
fn get_harness_diagnostics() -> Result<HarnessDiagnostics, String> {
    let (cfg, workspace) = configured_workspace()?;
    let profile = AgentRuntimeProfile::from_config(&cfg);
    let context = profile.apply_tool_context(ToolContext::new(
        workspace.clone(),
        profile.sandbox_policy(&workspace),
    ));
    Ok(HarnessRuntimeBuilder::configured(&workspace)?
        .build(context)
        .harness_diagnostics())
}

fn external_plugin_catalog() -> Result<ExternalPluginCatalog, String> {
    let (_, workspace) = configured_workspace()?;
    Ok(ExternalPluginCatalog::new(
        workspace.join(".ncx").join("plugins"),
    ))
}

#[tauri::command]
fn list_external_plugins() -> Result<Vec<ExternalPluginRecord>, String> {
    external_plugin_catalog()?.discover()
}

#[tauri::command]
fn install_external_plugin(source: String, upgrade: bool) -> Result<ExternalPluginRecord, String> {
    let catalog = external_plugin_catalog()?;
    if upgrade {
        catalog.upgrade(Path::new(&source))
    } else {
        catalog.install(Path::new(&source))
    }
}

#[tauri::command]
fn set_external_plugin_enabled(id: String, enabled: bool) -> Result<(), String> {
    external_plugin_catalog()?.set_enabled(&id, enabled)
}

fn codex_plugin_catalog() -> Result<CodexPluginCatalog, String> {
    let (_, workspace) = configured_workspace()?;
    Ok(CodexPluginCatalog::new(
        workspace.join(".ncx").join("codex-plugins"),
    ))
}

fn list_codex_plugins() -> Result<Vec<CodexPluginView>, String> {
    let (_, workspace) = configured_workspace()?;
    let apps = discover_codex_apps(&workspace)?;
    Ok(codex_plugin_catalog()?
        .discover()?
        .into_iter()
        .map(|plugin| {
            let app_count = apps
                .iter()
                .filter(|app| app.plugin == plugin.manifest.name)
                .count();
            CodexPluginView::new(plugin, app_count)
        })
        .collect())
}

#[derive(Serialize)]
struct CodexPluginView {
    manifest: CodexPluginManifest,
    root: PathBuf,
    enabled: bool,
    skill_roots: usize,
    has_mcp: bool,
    has_apps: bool,
    app_count: usize,
    has_hooks: bool,
}

impl CodexPluginView {
    fn new(plugin: CodexPluginRecord, app_count: usize) -> Self {
        Self {
            skill_roots: plugin.skill_paths().len(),
            has_mcp: plugin.manifest.mcp_servers.is_some() || plugin.mcp_path().is_some(),
            has_apps: plugin.manifest.apps.is_some() || plugin.apps_path().is_some(),
            app_count,
            has_hooks: plugin.manifest.hooks.is_some() || plugin.hooks_path().is_some(),
            manifest: plugin.manifest,
            root: plugin.root,
            enabled: plugin.enabled,
        }
    }
}

fn install_codex_plugin(source: String, upgrade: bool) -> Result<CodexPluginRecord, String> {
    codex_plugin_catalog()?.install_or_upgrade(Path::new(&source), upgrade)
}

fn set_codex_plugin_enabled(name: String, enabled: bool) -> Result<(), String> {
    codex_plugin_catalog()?.set_enabled(&name, enabled)
}

fn uninstall_codex_plugin(name: String) -> Result<(), String> {
    codex_plugin_catalog()?.uninstall(&name)
}

#[derive(Serialize)]
struct MarketplaceView {
    path: String,
    marketplace: Marketplace,
}

fn list_plugin_marketplaces() -> Result<Vec<MarketplaceView>, String> {
    let (_, workspace) = configured_workspace()?;
    Ok(discover_marketplaces(&workspace)?
        .into_iter()
        .map(|(path, marketplace)| MarketplaceView {
            path: path.display().to_string(),
            marketplace,
        })
        .collect())
}

fn install_marketplace_plugin(
    marketplace_path: String,
    plugin_name: String,
    upgrade: bool,
) -> Result<CodexPluginRecord, String> {
    let path = PathBuf::from(&marketplace_path);
    let (_, workspace) = configured_workspace()?;
    let canonical_path = path.canonicalize().map_err(|error| error.to_string())?;
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !canonical_path.starts_with(&canonical_workspace) {
        return Err("Marketplace 清单必须位于当前工作区".to_string());
    }
    let marketplace: Marketplace = serde_json::from_str(
        &fs::read_to_string(&canonical_path).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("无效 Marketplace: {error}"))?;
    let plugin = marketplace
        .plugins
        .iter()
        .find(|candidate| candidate.name == plugin_name)
        .ok_or_else(|| format!("Marketplace 中不存在插件 '{plugin_name}'"))?;
    let (source, cleanup) = materialize_marketplace_plugin(&workspace, &canonical_path, plugin)?;
    let result = codex_plugin_catalog()?.install_or_upgrade(&source, upgrade);
    if let Some(cleanup) = cleanup {
        let _ = remove_plugin_staging(&workspace, &cleanup);
    }
    result
}

fn materialize_marketplace_plugin(
    workspace: &Path,
    marketplace_path: &Path,
    plugin: &MarketplacePlugin,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    match &plugin.source {
        MarketplaceSource::Local { .. } => Ok((
            resolve_local_marketplace_plugin(marketplace_path, plugin)?,
            None,
        )),
        MarketplaceSource::Git {
            url,
            path,
            ref_name,
            sha,
        } => {
            if !(url.starts_with("https://")
                || url.starts_with("ssh://")
                || url.starts_with("git@"))
            {
                return Err("Git Marketplace 只允许 HTTPS 或 SSH 地址".to_string());
            }
            let staging = plugin_staging_dir(workspace, &plugin.name)?;
            let mut command = ProcessCommand::new("git");
            command.args(["clone", "--depth", "1"]);
            if let Some(ref_name) = ref_name.as_deref().filter(|value| !value.trim().is_empty()) {
                command.args(["--branch", ref_name]);
            }
            let output = command
                .arg(url)
                .arg(&staging)
                .output()
                .map_err(|error| format!("启动 git clone 失败: {error}"))?;
            if !output.status.success() {
                let _ = remove_plugin_staging(workspace, &staging);
                return Err(format!(
                    "Git 插件下载失败: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            if let Some(sha) = sha.as_deref().filter(|value| !value.trim().is_empty()) {
                let checkout = ProcessCommand::new("git")
                    .args(["-C"])
                    .arg(&staging)
                    .args(["checkout", "--detach", sha])
                    .output()
                    .map_err(|error| format!("启动 git checkout 失败: {error}"))?;
                if !checkout.status.success() {
                    let _ = remove_plugin_staging(workspace, &staging);
                    return Err(format!(
                        "Git 插件固定版本失败: {}",
                        String::from_utf8_lossy(&checkout.stderr).trim()
                    ));
                }
            }
            let source = match resolve_staged_subpath(&staging, path.as_deref()) {
                Ok(source) => source,
                Err(error) => {
                    let _ = remove_plugin_staging(workspace, &staging);
                    return Err(error);
                }
            };
            Ok((source, Some(staging)))
        }
        MarketplaceSource::Npm {
            package,
            version,
            registry,
        } => {
            if !valid_npm_package(package) {
                return Err("NPM 包名格式无效".to_string());
            }
            let staging = plugin_staging_dir(workspace, &plugin.name)?;
            fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
            let spec = version
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|version| format!("{package}@{version}"))
                .unwrap_or_else(|| package.clone());
            let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
            let mut command = ProcessCommand::new(npm);
            command
                .args(["pack", &spec, "--pack-destination"])
                .arg(&staging);
            if let Some(registry) = registry.as_deref().filter(|value| !value.trim().is_empty()) {
                command.args(["--registry", registry]);
            }
            let output = command
                .output()
                .map_err(|error| format!("启动 npm pack 失败: {error}"))?;
            if !output.status.success() {
                let _ = remove_plugin_staging(workspace, &staging);
                return Err(format!(
                    "NPM 插件下载失败: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            let archive = fs::read_dir(&staging)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| path.extension().and_then(|value| value.to_str()) == Some("tgz"));
            let Some(archive) = archive else {
                let _ = remove_plugin_staging(workspace, &staging);
                return Err("npm pack 未生成 tgz 文件".to_string());
            };
            let extract = ProcessCommand::new("tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&staging)
                .output()
                .map_err(|error| format!("启动 tar 解包失败: {error}"))?;
            if !extract.status.success() {
                let _ = remove_plugin_staging(workspace, &staging);
                return Err(format!(
                    "NPM 插件解包失败: {}",
                    String::from_utf8_lossy(&extract.stderr).trim()
                ));
            }
            Ok((staging.join("package"), Some(staging)))
        }
    }
}

fn plugin_staging_dir(workspace: &Path, name: &str) -> Result<PathBuf, String> {
    let safe_name = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe_name.trim_matches('_').is_empty() {
        return Err("Marketplace 插件名称无效".to_string());
    }
    let root = workspace.join(".ncx/codex-plugin-stage");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    Ok(root.join(format!("{safe_name}-{}-{nonce}", std::process::id())))
}

fn resolve_staged_subpath(staging: &Path, subpath: Option<&str>) -> Result<PathBuf, String> {
    let Some(subpath) = subpath.filter(|value| !value.trim().is_empty()) else {
        return Ok(staging.to_path_buf());
    };
    let relative = Path::new(subpath);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("Git 插件子目录不得越界".to_string());
    }
    let root = staging.canonicalize().map_err(|error| error.to_string())?;
    let resolved = staging
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !resolved.starts_with(root) {
        return Err("Git 插件子目录不得越界".to_string());
    }
    Ok(resolved)
}

fn remove_plugin_staging(workspace: &Path, staging: &Path) -> Result<(), String> {
    let canonical_root = workspace
        .join(".ncx/codex-plugin-stage")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let canonical_staging = staging.canonicalize().map_err(|error| error.to_string())?;
    if canonical_staging == canonical_root || !canonical_staging.starts_with(&canonical_root) {
        return Err("拒绝清理工作区之外的插件暂存目录".to_string());
    }
    fs::remove_dir_all(canonical_staging).map_err(|error| error.to_string())
}

fn valid_npm_package(package: &str) -> bool {
    !package.is_empty()
        && package.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'/' | b'.' | b'-' | b'_')
        })
        && !package.contains("..")
        && !package.starts_with('/')
}

fn migrate_legacy_threads(
    index: &SessionIndex,
    app_server: &AppServer<JsonThreadStore>,
) -> Result<usize, String> {
    let mut pending = Vec::new();
    for summary in index.entries() {
        let thread_id =
            ThreadId::new(summary.session_id.clone()).map_err(|error| error.to_string())?;
        if app_server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: thread_id.clone(),
            })
            .is_ok()
        {
            continue;
        }
        let messages = index.load_snapshot(&summary.session_id).unwrap_or_default();
        let updated_at = summary
            .updated_at
            .parse::<i64>()
            .unwrap_or_else(|_| now_epoch_millis());
        let created_at = summary.created_at.parse::<i64>().unwrap_or(updated_at);
        let thread = Thread {
            metadata: ThreadMetadata {
                id: thread_id,
                workspace: summary.workspace,
                title: summary.title,
                archived: summary.archived,
                created_at,
                updated_at,
            },
            turns: legacy_conclusion_turns(&messages, created_at),
        };
        pending.push(thread);
    }
    let imported = pending.len();
    if !pending.is_empty() {
        app_server
            .dispatch(ClientRequest::ThreadsImport { threads: pending })
            .map_err(|error| error.to_string())?;
    }
    Ok(imported)
}

fn legacy_conclusion_turns(messages: &[serde_json::Value], timestamp: i64) -> Vec<Turn> {
    let mut turns = Vec::new();
    let mut current_user: Option<String> = None;
    let mut final_answer: Option<String> = None;
    let flush = |user: Option<String>, answer: Option<String>, turns: &mut Vec<Turn>| {
        let Some(user) = user else { return };
        let index = turns.len();
        let mut items = vec![ThreadItem::UserMessage {
            id: ItemId::new(format!("legacy-user-{index}")).expect("non-empty legacy item id"),
            text: user,
        }];
        if let Some(answer) = answer.filter(|answer| !answer.trim().is_empty()) {
            items.push(ThreadItem::AssistantMessage {
                id: ItemId::new(format!("legacy-assistant-{index}"))
                    .expect("non-empty legacy item id"),
                text: answer,
            });
        }
        turns.push(Turn {
            id: TurnId::new(format!("legacy-turn-{index}")).expect("non-empty legacy turn id"),
            status: TurnStatus::Completed,
            items,
            started_at: timestamp,
            completed_at: Some(timestamp),
            error: None,
            usage: Default::default(),
        });
    };
    for message in messages {
        match message.get("role").and_then(serde_json::Value::as_str) {
            Some("user") => {
                flush(current_user.take(), final_answer.take(), &mut turns);
                current_user = legacy_message_text(message.get("content"));
            }
            Some("assistant") => {
                if let Some(text) = legacy_message_text(message.get("content")) {
                    if !text.trim().is_empty() {
                        final_answer = Some(text);
                    }
                }
            }
            _ => {}
        }
    }
    flush(current_user, final_answer, &mut turns);
    turns
}

fn legacy_message_text(content: Option<&serde_json::Value>) -> Option<String> {
    match content? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(parts) => parts.iter().find_map(|part| {
            (part.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .then(|| {
                    part.get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        }),
        _ => None,
    }
}

pub fn run() {
    let (tx, rx) = unbounded_channel::<Command>();
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let pending_for_worker = pending.clone();
    let questions: PendingQuestionMap = Arc::new(Mutex::new(HashMap::new()));
    let questions_for_worker = questions.clone();
    let cancels: CancelRegistry = Arc::new(Mutex::new(HashMap::new()));
    let cancels_for_worker = cancels.clone();
    let running: RunningSessions = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let running_for_worker = running.clone();
    let session_grants: GrantRegistry = Arc::new(Mutex::new(HashMap::new()));
    let session_index = Arc::new(Mutex::new(SessionIndex::default()));
    let thread_store = Arc::new(
        JsonThreadStore::open(default_thread_store_path())
            .expect("open the versioned nanocodex thread store"),
    );
    let app_server = Arc::new(AppServer::new(thread_store, now_epoch_millis));
    if let Ok(index) = session_index.lock() {
        if let Err(error) = migrate_legacy_threads(&index, &app_server) {
            eprintln!("thread migration: {error}");
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            tx,
            pending,
            questions,
            question_counter: AtomicU64::new(1_000_000),
            cancels,
            app_server: app_server.clone(),
            openrouter_models: Mutex::new(Vec::new()),
        })
        .setup(move |app| {
            // Hand the agent thread an AppHandle (to emit events), the receiver
            // (to take prompts), and the shared pending-approvals map.
            spawn_worker(
                app.handle().clone(),
                app_server.clone(),
                rx,
                pending_for_worker,
                questions_for_worker,
                cancels_for_worker,
                running_for_worker,
                session_grants,
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            app_server_request,
            approve,
            answer_question,
            e2e_ask_question,
            get_settings,
            save_settings,
            get_model_catalog,
            refresh_openrouter_models,
            apply_model_preset,
            get_config_location,
            open_config_file,
            open_config_dir,
            get_checkpoints,
            checkpoint_files,
            create_checkpoint,
            restore_checkpoint,
            git_branches,
            git_log,
            git_create_branch,
            git_switch_branch,
            git_diff,
            git_changes,
            git_file_diff,
            list_dir,
            read_workspace_file,
            open_url,
            list_mcp,
            save_temp_image,
            open_session_log,
            open_session_snapshot,
            get_custom_commands,
            expand_custom_command,
            get_harness_diagnostics,
            list_external_plugins,
            install_external_plugin,
            set_external_plugin_enabled,
            memory_list,
            memory_consolidate,
            memory_add,
            open_memory_file,
            set_workspace,
            get_workspace,
            set_approval,
            set_sandbox,
            set_model,
            set_permission_mode,
            request_ready
        ])
        .run(tauri::generate_context!())
        .expect("error while running the nanocodex GUI");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_catalog::CatalogModel;
    use ncx_config::Config;

    #[test]
    fn archiving_persists_without_waiting_for_the_agent_command_queue() {
        let root = std::env::temp_dir().join(format!(
            "ncx_archive_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store_path = root.join("threads.json");
        let server = AppServer::new(
            Arc::new(JsonThreadStore::open(store_path.clone()).unwrap()),
            || 1,
        );
        let thread_id = ThreadId::new("other-session").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(thread_id.clone()),
                workspace: root.display().to_string(),
                title: "other session".into(),
            })
            .unwrap();
        server
            .dispatch(ClientRequest::ThreadArchive {
                thread_id: thread_id.clone(),
                archived: true,
            })
            .unwrap();

        let reloaded = AppServer::new(Arc::new(JsonThreadStore::open(store_path).unwrap()), || 2);
        let outcome = reloaded
            .dispatch(ClientRequest::ThreadRead { thread_id })
            .unwrap();
        let ncx_protocol::ResponsePayload::Thread(thread) = outcome.response.payload else {
            panic!("thread read response expected");
        };
        assert!(thread.metadata.archived);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settings_snapshot_exposes_vision_parser_without_leaking_its_key() {
        let mut cfg = Config::default();
        cfg.vl_model = "qwen3.7-plus".into();
        cfg.vl_base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
        cfg.vl_api_key = "secret-vision-key".into();

        let settings = settings_from_config(&cfg);

        assert_eq!(settings.vl_model, "qwen3.7-plus");
        assert_eq!(
            settings.vl_base_url,
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert!(settings.has_vl_api_key);
        assert_ne!(settings.vl_api_key_masked, cfg.vl_api_key);
        assert!(settings.vl_api_key_masked.starts_with("****"));
    }

    #[test]
    fn image_attachment_requires_an_explicit_parser_model() {
        assert!(validate_image_attachment_route(&[], "").is_ok());
        assert!(validate_image_attachment_route(&["test.png".into()], "qwen3.7-plus").is_ok());

        let error = validate_image_attachment_route(&["test.png".into()], "").unwrap_err();
        assert!(error.contains("图片/文件解析模型"));
        assert!(error.contains("设置"));
    }

    #[test]
    fn topbar_exposes_reasoning_effort_quick_switch() {
        let app = include_str!("../../src/App.svelte");
        let composer = include_str!("../../src/components/Composer.svelte");
        assert!(composer.contains("class=\"reasoning-pill\""));
        assert!(app.contains("思考程度"));
        assert!(app.contains("selectReasoningEffort"));
        assert!(app.contains("智能体自动"));
        assert!(app.contains("智能体增强"));
        assert!(!app.contains("{ id: \"low\", label:"));
        assert!(!app.contains("{ id: \"medium\", label:"));
    }

    #[test]
    fn session_controls_live_in_the_composer_with_apple_visual_tokens() {
        let topbar = include_str!("../../src/components/TopBar.svelte")
            .split_once("<header class=\"topbar\">")
            .unwrap()
            .1
            .split_once("</header>")
            .unwrap()
            .0;
        let composer = include_str!("../../src/components/Composer.svelte")
            .split_once("<div class=\"composer-meta\">")
            .unwrap()
            .1
            .split_once("{#if queued.length}")
            .unwrap()
            .0;
        assert!(!topbar.contains("model-wrap"));
        assert!(!topbar.contains("reasoning-wrap"));
        assert!(!topbar.contains("ws-pill"));
        assert!(composer.contains("model-wrap"));
        assert!(composer.contains("reasoning-wrap"));
        assert!(composer.contains("ws-pill"));

        let css = include_str!("../../src/app.css");
        assert!(css.contains("--accent:       #0a84ff"));
        assert!(css.contains("backdrop-filter: blur(28px)"));
        assert!(css
            .contains(".menu-backdrop:hover, .menu-backdrop:active, .menu-backdrop:focus-visible"));
    }

    #[test]
    fn completed_tool_activity_is_hidden_from_final_and_history_views() {
        let app = include_str!("../../src/App.svelte");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let model = include_str!("../../src/lib/conversation-model.ts");
        let bridge = include_str!("bridge.rs");
        assert!(app.contains("role: \"tool_group\""));
        assert!(model.contains(
            "type ToolGroup = { role: \"tool_group\"; tools: ToolEntry[]; settled: boolean }"
        ));

        let cleanup = model
            .split_once("function hideCompletedToolActivity")
            .unwrap()
            .1
            .split_once("function toolGroupFailureCount")
            .unwrap()
            .0;
        assert!(cleanup.contains("message.role !== \"tool_group\""));
        assert!(app.matches("hideCompletedToolActivity(").count() >= 3);
        assert!(bridge.contains("only the execution result and a brief recommended next action"));
        assert!(bridge.contains("do not recap tool calls, logs, or intermediate process"));

        // Tool logs remain visible while the turn is running.
        let group = conversation
            .split_once("{:else}")
            .unwrap()
            .1
            .split_once("{#if busy && streamingIdx === null && reasoningIdx === null}")
            .unwrap()
            .0;
        assert!(group.contains("<details class=\"tool-run\""));
        assert!(group.contains("class:settled={message.settled}"));
        assert!(group.contains("open={!message.settled}"));
        assert!(group.contains("已执行 {message.tools.length} 个工具"));
        assert!(group.contains("toolGroupFailureCount(message)"));
        assert!(group.contains("{#each message.tools as tool}"));
        assert!(group.contains("tool.name"));
        assert!(group.contains("tool.args"));
        assert!(group.contains("tool.result"));

        let css = include_str!("../../src/app.css");
        let tool_css = css
            .split_once("Tool calls")
            .unwrap()
            .1
            .split_once("Composer (footer)")
            .unwrap()
            .0;
        assert!(tool_css.contains("--tool-log-text:"));
        assert!(tool_css.contains("--tool-log-muted:"));
        assert!(tool_css.contains("--tool-log-line:"));
        assert!(tool_css.contains("border-left: 1px solid var(--tool-log-line)"));
        assert!(tool_css.contains("background: transparent"));
        assert!(tool_css.contains("color: var(--tool-log-text)"));
        assert!(tool_css.contains("color: var(--tool-log-muted)"));
        assert!(tool_css.contains(".tool-run:not(.settled) > summary"));
        assert!(tool_css.contains(".tool-run.settled > summary"));
        assert!(!tool_css.contains("border-radius: var(--r-md)"));
    }

    #[test]
    fn model_reasoning_is_visible_separately_from_tool_activity() {
        let app = include_str!("../../src/App.svelte");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let css = include_str!("../../src/app.css");
        let bridge = include_str!("bridge.rs");
        let core = include_str!("../../../crates/ncx-core/src/agent_loop.rs");
        let provider = include_str!("../../../crates/ncx-provider/src/api.rs");

        assert!(provider.contains("StreamDelta::Reasoning"));
        assert!(core.contains("ReasoningDelta(String)"));
        assert!(bridge.contains("UiEvent::ReasoningDelta"));
        assert!(app.contains("case \"reasoning_delta\":"));
        assert!(app.contains("role: \"reasoning\""));
        assert!(conversation.contains("思考过程"));
        assert!(css.contains(".reasoning-run"));
    }

    #[test]
    fn legacy_migration_keeps_each_user_request_and_final_conclusion_only() {
        let turns = legacy_conclusion_turns(
            &[
                serde_json::json!({"role":"system","content":"rules"}),
                serde_json::json!({"role":"user","content":"生成 PDF"}),
                serde_json::json!({"role":"assistant","content":"开始检查"}),
                serde_json::json!({"role":"tool","content":"very noisy output"}),
                serde_json::json!({"role":"assistant","content":"PDF 已生成"}),
                serde_json::json!({"role":"user","content":"继续优化"}),
                serde_json::json!({"role":"assistant","content":"优化完成"}),
            ],
            100,
        );
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].items.len(), 2);
        assert!(matches!(
            &turns[0].items[1],
            ThreadItem::AssistantMessage { text, .. } if text == "PDF 已生成"
        ));
        assert!(matches!(
            &turns[1].items[1],
            ThreadItem::AssistantMessage { text, .. } if text == "优化完成"
        ));
    }

    #[test]
    fn marketplace_staging_rejects_escape_paths_and_invalid_npm_packages() {
        let workspace =
            std::env::temp_dir().join(format!("ncx-market-stage-{}", now_epoch_millis()));
        let staging = plugin_staging_dir(&workspace, "demo/plugin").unwrap();
        std::fs::create_dir_all(staging.join("safe")).unwrap();
        assert!(resolve_staged_subpath(&staging, Some("safe")).is_ok());
        assert!(resolve_staged_subpath(&staging, Some("../outside")).is_err());
        assert!(valid_npm_package("@scope/plugin-name"));
        assert!(!valid_npm_package("../plugin"));
        assert!(!valid_npm_package("plugin name"));
        remove_plugin_staging(&workspace, &staging).unwrap();
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn reasoning_stays_collapsed_and_bounded_while_streaming() {
        let app = include_str!("../../src/App.svelte");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let model = include_str!("../../src/lib/conversation-model.ts");
        assert!(model.contains("REASONING_DISPLAY_MAX_CHARS"));
        assert!(app.contains("appendReasoning(m.text, p.text)"));
        assert!(conversation.contains("<details class=\"reasoning-run\" class:settled={message.settled}>"));
        assert!(!conversation.contains(
            "<details class=\"reasoning-run\" class:settled={message.settled} open={!message.settled}>"
        ));
        assert!(conversation.contains("<pre class=\"reasoning-content\">{message.text}</pre>"));
    }

    #[test]
    fn completed_turn_removes_transient_reasoning_cards() {
        let app = include_str!("../../src/App.svelte");
        assert!(app.contains("function removeReasoningMessages()"));
        assert!(app.contains("removeReasoningMessages();"));
        assert!(app.contains("messages = hideCompletedToolActivity(messages);"));
        assert!(app.contains("case \"done\":"));
        assert!(app.contains("case \"error\":"));
        assert!(app.contains("sessionMessages.set(p.session_id, cloneMessages(messages));"));
        assert!(app.contains("message.role !== \"reasoning\""));
        assert!(app.contains("message.role !== \"tool_group\""));
    }

    #[test]
    fn completed_turn_keeps_prior_history_and_only_its_final_conclusion() {
        let app = include_str!("../../src/App.svelte");
        let model = include_str!("../../src/lib/conversation-model.ts");
        assert!(model.contains("function keepConversationConclusions("));
        assert!(model.contains("if (pendingAnswer) compacted.push(pendingAnswer);"));
        assert!(model.contains("compacted.push({ ...message });"));
        assert!(model.contains("pendingAnswer = { ...message };"));
        assert!(app.contains("keepConversationConclusions(messages, p.final_text)"));
    }

    #[test]
    fn stop_button_remains_retryable_until_turn_finishes() {
        let app = include_str!("../../src/App.svelte");
        let composer = include_str!("../../src/components/Composer.svelte");
        assert!(app.contains("if (!busy) return;"));
        assert!(composer.contains("disabled={!busy} title={stopping ? \"再次停止\" : \"停止生成\"}"));
        assert!(!app.contains("if (!busy || stopping) return;"));
    }

    #[test]
    fn automatic_context_compaction_is_visible_and_session_scoped() {
        let app = include_str!("../../src/App.svelte");
        let css = include_str!("../../src/app.css");
        let bridge = include_str!("bridge.rs");
        let core = include_str!("../../../crates/ncx-core/src/agent_loop/turn.rs");

        assert!(core.contains("agent.session.compact_if_needed"));
        assert!(bridge.contains("UiEvent::ContextCompacted"));
        assert!(app.contains("case \"context_compacted\":"));
        assert!(app.contains("acceptsSessionEvent(p.session_id)"));
        assert!(app.contains("已自动压缩上下文"));
        assert!(app.contains("role: \"compact\""));
        assert!(css.contains(".compact"));
    }

    #[test]
    fn session_usage_survives_restart_and_session_switches() {
        let app = include_str!("../../src/App.svelte");
        let usage = include_str!("../../src/lib/usage-controller.svelte.ts");
        assert!(usage.contains("restore(sessionId: string)"));
        assert!(usage.contains("persist(sessionId: string)"));
        assert!(usage.contains("ncx.sessionUsage."));
        assert!(app.contains("usage.add(p.session_id"));
        assert!(usage.contains("this.persist(sessionId)"));
        assert!(app.matches("usage.restore(currentSessionId)").count() >= 2);
        assert!(app.matches("usage.reset()").count() >= 2);
    }

    #[test]
    fn frontend_rejects_events_from_inactive_sessions() {
        let app = include_str!("../../src/App.svelte");
        assert!(app.contains("function acceptsSessionEvent(sessionId: string)"));
        assert!(app.contains("if (!acceptsSessionEvent(p.session_id)) break"));
        assert!(app.contains("session_id: string; text: string"));
    }

    #[test]
    fn title_generation_does_not_block_the_agent_command_queue() {
        let bridge = include_str!("bridge.rs");
        assert!(bridge.contains("spawn_title_generation("));
        assert!(!bridge.contains("agent.suggest_title(&text).await"));
    }

    #[test]
    fn new_session_starts_with_empty_chat_and_plan_context() {
        let bridge = include_str!("bridge.rs");
        let backend = include_str!("lib.rs");
        assert!(bridge.contains("Command::New(id)"));
        assert!(bridge.contains("Some((id, Vec::new()))"));
        let new_branch = bridge
            .split_once("Command::New(id)")
            .unwrap()
            .1
            .split_once("Command::Reload")
            .unwrap()
            .0;
        assert!(!new_branch.contains("record_turn("));
        assert!(backend.contains("ClientRequest::ThreadCreateActivate"));
        assert!(new_branch.contains("messages: Vec::new()"));
    }

    #[test]
    fn prompts_are_dispatched_to_independent_session_workers() {
        let bridge = include_str!("bridge.rs");
        let backend = include_str!("lib.rs");
        let app = include_str!("../../src/App.svelte");
        assert!(bridge.contains("fn spawn_turn_worker("));
        assert!(bridge.contains("session_id: target_id"));
        assert!(bridge.contains("spawn_turn_worker("));
        assert!(backend.contains("session_id: String"));
        assert!(app.contains("method: \"turnSubmit\""));
        assert!(app.contains("threadId: targetSessionId"));
    }

    #[test]
    fn frontend_thread_lifecycle_uses_the_versioned_app_server_protocol() {
        let app = include_str!("../../src/App.svelte");
        for method in [
            "threadCreateActivate",
            "threadActivate",
            "threadForkActivate",
            "turnSubmit",
            "turnInterruptLatest",
            "threadArchive",
            "threadRename",
            "threadReadVisible",
        ] {
            assert!(
                app.contains(&format!("method: \"{method}\"")),
                "missing app-server request for {method}"
            );
        }
        assert!(!app.contains("invoke(\"archive_session\""));
        assert!(!app.contains("invoke<SessionRow[]>(\"list_sessions\")"));
        let refresh = app
            .split_once("async function refreshSessions")
            .unwrap()
            .1
            .split_once("async function loadNotes")
            .unwrap()
            .0;
        assert!(!refresh.contains("method: \"threadRead\""));
    }

    #[test]
    fn gui_runtime_no_longer_reads_or_writes_legacy_session_snapshots() {
        let bridge = include_str!("bridge.rs");
        let backend = include_str!("lib.rs");
        assert!(!bridge.contains("SessionIndex"));
        assert!(!bridge.contains("load_snapshot("));
        assert!(!bridge.contains("record_turn("));
        let handler = backend
            .split_once(".invoke_handler(tauri::generate_handler![")
            .unwrap()
            .1
            .split_once(".run(tauri::generate_context!())")
            .unwrap()
            .0;
        for legacy_command in [
            "send_prompt,",
            "stop_generation,",
            "list_sessions,",
            "resume_session,",
            "fork_session,",
            "archive_session,",
            "new_session,",
            "list_codex_plugins,",
            "install_codex_plugin,",
            "set_codex_plugin_enabled,",
            "uninstall_codex_plugin,",
            "list_plugin_marketplaces,",
            "install_marketplace_plugin,",
        ] {
            assert!(!handler.contains(legacy_command));
        }
        assert!(backend.contains("migrate_legacy_threads(&index, &app_server)"));
    }

    #[test]
    fn frontend_rejects_stale_or_cross_version_protocol_events() {
        let app = include_str!("../../src/App.svelte");
        let protocol_client = include_str!("../../src/lib/app-server-client.ts");
        assert!(app.contains("listen<ProtocolEventEnvelope>(\"ncx://protocol-event\""));
        assert!(app.contains("protocolSequenceGate.accept(envelope)"));
        assert!(protocol_client.contains("envelope.protocolVersion !== 2 || !envelope.threadId"));
        assert!(protocol_client.contains("this.sequences.get(envelope.threadId) || 0"));
        assert!(protocol_client.contains("envelope.sequence <= previous"));
        assert!(protocol_client.contains("this.sequences.set(envelope.threadId, envelope.sequence)"));
    }

    #[test]
    fn tauri_delegates_protocol_routing_to_the_app_server() {
        let backend = include_str!("lib.rs");
        let body = backend
            .split_once("fn app_server_request(")
            .unwrap()
            .1
            .split_once("struct GuiAppServerAdapter")
            .unwrap()
            .0;
        assert!(body.contains("dispatch_with_runtime(request, &runtime)"));
        assert!(!body.contains("ClientRequest::"));
    }

    #[test]
    fn enabled_codex_mcp_resources_feed_the_gui_runtime_diagnostics() {
        let bridge = include_str!("bridge.rs");
        assert!(bridge.contains("discover_codex_mcp_servers(&cfg.workspace)?"));
        assert!(bridge.contains("prepare_mcp_server_tools("));
        assert!(bridge.contains("configured_servers: mcp_servers.len()"));
        assert!(bridge.contains("active_tools"));
        assert!(bridge.contains("tools.replace_service("));
    }

    #[test]
    fn switching_sessions_does_not_stop_the_previous_turn() {
        let app = include_str!("../../src/App.svelte");
        let resume = app
            .split_once("async function resumeSession")
            .unwrap()
            .1
            .split_once("async function forkSession")
            .unwrap()
            .0;
        assert!(!resume.contains("stop_generation"));
        assert!(app.contains("setSessionRunning(targetSessionId, true)"));
        assert!(app.contains("setSessionRunning(p.session_id, false)"));
    }

    #[test]
    fn history_session_switch_keeps_the_active_turn_running() {
        let app = include_str!("../../src/App.svelte");
        let sidebar = include_str!("../../src/components/SessionSidebar.svelte");
        assert!(sidebar.contains("disabled={switchingSession || !s.has_snapshot}"));
        let resume = app
            .split_once("async function resumeSession")
            .unwrap()
            .1
            .split_once("async function forkSession")
            .unwrap()
            .0;
        assert!(!resume.contains("stop_generation"));
        assert!(resume.contains("busy = runningSessions.has(id)"));
        assert!(resume.contains("method: \"threadActivate\""));
    }

    #[test]
    fn archived_sessions_render_below_recent_sessions() {
        let app = include_str!("../../src/App.svelte");
        let sidebar_component = include_str!("../../src/components/SessionSidebar.svelte");
        assert!(app.contains("sessions.filter((s) => !s.archived)"));
        assert!(app.contains("sessions.filter((s) => s.archived)"));

        let sidebar = sidebar_component
            .split_once("<div class=\"side-recents\">")
            .unwrap()
            .1
            .split_once("<div class=\"side-foot\">")
            .unwrap()
            .0;
        let recent = sidebar.find("{#each recentSessions as s}").unwrap();
        let archive_toggle = sidebar.find("class=\"side-archive-toggle\"").unwrap();
        let archived = sidebar.find("{#each archivedSessions as s}").unwrap();
        assert!(recent < archive_toggle);
        assert!(archive_toggle < archived);
        assert!(sidebar.contains("aria-expanded={showArchived}"));

        let css = include_str!("../../src/app.css");
        assert!(css.contains(".side-archive-toggle"));
        assert!(css.contains(".side-archived-list"));
    }

    #[test]
    fn recent_sessions_are_collapsible_and_closed_by_default() {
        let app = include_str!("../../src/App.svelte");
        let sidebar_component = include_str!("../../src/components/SessionSidebar.svelte");
        assert!(app.contains("let showRecent = $state(false)"));

        let sidebar = sidebar_component
            .split_once("<div class=\"side-recents\">")
            .unwrap()
            .1
            .split_once("<div class=\"side-foot\">")
            .unwrap()
            .0;
        assert!(sidebar.contains("class=\"side-recent-toggle\""));
        assert!(sidebar.contains("aria-expanded={showRecent}"));
        assert!(sidebar.contains("onclick={() => (showRecent = !showRecent)}"));
        assert!(sidebar.contains("{#if showRecent}"));
        assert!(
            sidebar.find("{#if showRecent}").unwrap()
                < sidebar.find("{#each recentSessions as s}").unwrap()
        );

        let css = include_str!("../../src/app.css");
        assert!(css.contains(".side-recent-toggle"));
        assert!(css.contains(".side-recent-caret"));
        assert!(css.contains(".side-recent-list"));
    }

    #[test]
    fn preset_updates_model_endpoint_price_currency_and_quick_switch_list_together() {
        let preset = CatalogModel {
            provider_id: "openai".into(),
            model_id: "gpt-5-mini".into(),
            display_name: "GPT-5 mini".into(),
            base_url: "https://api.openai.com/v1".into(),
            price_in: 0.25,
            price_out: 2.0,
            price_currency: "USD".into(),
            price_source: crate::model_catalog::PriceSource::OfficialDirect,
            pricing_note: None,
            source_url: "https://openai.com/api/pricing".into(),
            updated_at: "2026-08-17".into(),
            context_length: None,
            direct_available: true,
        };

        let updates = preset_updates(&preset, &["gpt-5-mini", "gpt-5"]);
        assert_eq!(updates["model"], "gpt-5-mini");
        assert_eq!(updates["base_url"], "https://api.openai.com/v1");
        assert_eq!(updates["price_currency"], "USD");
        assert_eq!(updates["available_models"], "gpt-5-mini,gpt-5");
    }
}
