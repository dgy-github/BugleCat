//! nanocodex GUI (Tauri v2) — Rust backend.
//!
//! The agent loop runs on a dedicated `!Send` thread (see [`bridge`]); the
//! frontend talks to it through the `send_prompt` command and listens for
//! `ncx://event` window events. `get_status` is a cheap synchronous snapshot
//! for the header.

mod bridge;
mod forge_job;
mod forge_runtime;
mod memory_merge_job;
pub mod model_catalog;

use model_catalog::{
    catalog, find_preset, openrouter_model, yunmo_model, CatalogModel, CatalogProvider,
};

use base64::Engine as _;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ncx_app_server::{AppServer, AppServerAdapter, DispatchOutcome};
use ncx_config::{
    load_config, write_nanocodex_config, Config, ConfigPaths, Overrides,
    ProviderRouteInput as CustomProviderInput, ProviderRouteView as CustomProviderView,
    VALID_APPROVAL_POLICIES, VALID_PRICE_CURRENCIES, VALID_SANDBOX_MODES,
};
use ncx_core::{
    custom_command_prompt, discover_codex_apps, discover_marketplaces, list_custom_commands,
    resolve_local_marketplace_plugin, CheckpointMeta, CheckpointStore, CodexPluginCatalog,
    CodexPluginManifest, CodexPluginRecord, ConfiguredHarnessRuntime, ExternalPluginCatalog,
    ExternalPluginRecord, HarnessRuntimeBuilder, Marketplace, MarketplacePlugin, MarketplaceSource,
    MemoryStore, ProviderCatalogService, ProviderChatProbeService, ProviderDirectoryService,
    RestoreReport, RuntimeContextSources, RuntimeHostBindings, SessionIndex,
};
use ncx_protocol::{
    ClientRequest, ItemId, ResponsePayload, Thread, ThreadId, ThreadItem, ThreadMetadata, Turn,
    TurnId, TurnStatus,
};
use ncx_thread_store::{default_thread_store_path, JsonThreadStore};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use bridge::{
    emit_protocol_outcome, request_cancel, safe_session_file_stem, spawn_worker, CancelRegistry,
    Command, DeferredPrompt, DeferredPrompts, GrantRegistry, PendingMap, PendingQuestionMap,
    RunningSessions, RuntimeActivation, RuntimeActivationCoordinator, SessionRunKind,
    WorkerLifecycle, WorkerStartup,
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
    orchestrator_workers: i64,
    orchestrator_high_workers: i64,
    orchestrator_verify_retries: i64,
    orchestrator_max_depth: i64,
    orchestrator_max_subtasks: i64,
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
    running: RunningSessions,
    deferred_prompts: DeferredPrompts,
    worker_lifecycle: Arc<WorkerLifecycle>,
    app_server: Arc<AppServer<JsonThreadStore>>,
    provider_directory: ProviderDirectoryService,
    provider_catalog: ProviderCatalogService,
    provider_chat_probe: ProviderChatProbeService,
    provider_activation: ProviderActivationGate,
    /// Linearizes every command that can replace the process-global Agent
    /// runtime (new, resume, fork, and permission-mode rebuild).
    runtime_activation: Arc<RuntimeActivationCoordinator>,
    /// Keeps a runtime activation's CWD transition, worker handoff, and
    /// acknowledgement together.  The activation coordinator fences late
    /// worker commits; this gate also prevents an older caller from changing
    /// the process CWD after a newer caller already selected its workspace.
    ///
    /// Lock ordering is always `runtime_handoff_gate` then `workspace_gate`.
    runtime_handoff_gate: RuntimeHandoffGate,
    openrouter_models: Mutex<Vec<CatalogModel>>,
    yunmo_models: Mutex<Vec<CatalogModel>>,
    /// Serializes process-CWD transitions with workspace-bound GUI operations.
    /// Tauri commands can run concurrently, so every snapshot check and its
    /// filesystem/Git operation must share a linearization point with
    /// `WorkspaceSet`.
    workspace_gate: Mutex<()>,
    memory_merge: Arc<memory_merge_job::MemoryMergeCoordinator>,
    forge_job: Arc<forge_job::ForgeJobCoordinator>,
}

struct ProviderActivationGate {
    state: Mutex<ProviderActivationDiagnostics>,
}

/// A process-wide permit for the part of a runtime handoff that can change
/// the process CWD. A plain mutex would provide the same production
/// serialization, but this wrapper also exposes a deterministic test probe
/// for the contention boundary.
#[derive(Default)]
struct RuntimeHandoffGate {
    state: Mutex<RuntimeHandoffGateState>,
    available: Condvar,
    #[cfg(test)]
    waiter_arrived: Condvar,
}

#[derive(Default)]
struct RuntimeHandoffGateState {
    held: bool,
    #[cfg(test)]
    waiters: usize,
}

struct RuntimeHandoffPermit<'a> {
    gate: &'a RuntimeHandoffGate,
}

impl RuntimeHandoffGate {
    fn acquire(&self) -> Result<RuntimeHandoffPermit<'_>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "运行态切换状态不可用".to_string())?;
        if state.held {
            #[cfg(test)]
            {
                state.waiters += 1;
                self.waiter_arrived.notify_all();
            }
            while state.held {
                state = self
                    .available
                    .wait(state)
                    .map_err(|_| "运行态切换状态不可用".to_string())?;
            }
            #[cfg(test)]
            {
                state.waiters -= 1;
            }
        }
        state.held = true;
        Ok(RuntimeHandoffPermit { gate: self })
    }

    #[cfg(test)]
    fn wait_until_contended(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while state.waiters == 0 {
            let (next, timeout) = self
                .waiter_arrived
                .wait_timeout(state, Duration::from_secs(5))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(
                !timeout.timed_out(),
                "second runtime handoff never reached the serialization gate"
            );
            state = next;
        }
    }
}

impl Drop for RuntimeHandoffPermit<'_> {
    fn drop(&mut self) {
        let mut state = self
            .gate
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.held = false;
        self.gate.available.notify_one();
    }
}

#[derive(Clone, Debug, Serialize)]
struct ProviderActivationDiagnostics {
    generation: u64,
    status: String,
    last_error: Option<String>,
    updated_at_ms: i64,
}

impl Default for ProviderActivationGate {
    fn default() -> Self {
        Self {
            state: Mutex::new(ProviderActivationDiagnostics {
                generation: 0,
                status: "idle".into(),
                last_error: None,
                updated_at_ms: 0,
            }),
        }
    }
}

impl ProviderActivationGate {
    fn begin(&self) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "模型切换状态不可用".to_string())?;
        state.generation = state.generation.saturating_add(1);
        state.status = "validating".into();
        state.last_error = None;
        state.updated_at_ms = now_epoch_millis();
        Ok(state.generation)
    }

    fn commit<T>(
        &self,
        generation: u64,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "模型切换状态不可用".to_string())?;
        if state.generation != generation {
            return Err("模型选择已更新，本次较早切换已取消".to_string());
        }
        match commit() {
            Ok(value) => {
                state.status = "active".into();
                state.last_error = None;
                state.updated_at_ms = now_epoch_millis();
                Ok(value)
            }
            Err(error) => {
                state.status = "failed".into();
                state.last_error = Some(safe_activation_error(&error));
                state.updated_at_ms = now_epoch_millis();
                Err(error)
            }
        }
    }

    fn fail(&self, generation: u64, error: String) -> String {
        if let Ok(mut state) = self.state.lock() {
            if state.generation == generation {
                state.status = "failed".into();
                state.last_error = Some(safe_activation_error(&error));
                state.updated_at_ms = now_epoch_millis();
            }
        }
        error
    }

    fn diagnostics(&self) -> Result<ProviderActivationDiagnostics, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "模型切换状态不可用".to_string())
    }
}

fn safe_activation_error(error: &str) -> String {
    let mut redact_next = false;
    error
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            let redacted = if redact_next
                || lower.starts_with("sk-")
                || lower.starts_with("api_key=")
                || lower.starts_with("apikey=")
                || lower.starts_with("token=")
            {
                "[已脱敏]"
            } else {
                part
            };
            redact_next = lower == "bearer";
            redacted
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
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

fn list_custom_providers(state: &AppState) -> Result<Vec<CustomProviderView>, String> {
    let routes = state.provider_directory.list()?;
    for route in routes
        .iter()
        .filter(|route| route.id.starts_with("preset:"))
    {
        let provider_id = route.id.trim_start_matches("preset:");
        let models = if provider_id == "openrouter" {
            state
                .openrouter_models
                .lock()
                .map_err(|_| "OpenRouter 模型缓存不可用")?
                .clone()
        } else if provider_id == "yunmo" {
            state
                .yunmo_models
                .lock()
                .map_err(|_| "云末模型缓存不可用")?
                .clone()
        } else {
            provider_models(provider_id, &[], &[])
        };
        let model_ids = models
            .into_iter()
            .map(|model| model.model_id)
            .collect::<Vec<_>>();
        if !model_ids.is_empty() && model_ids != route.models {
            state
                .provider_directory
                .reconcile_models(&route.id, model_ids)?;
        }
    }
    state.provider_directory.list()
}
fn save_custom_provider(
    directory: &ProviderDirectoryService,
    input: CustomProviderInput,
) -> Result<CustomProviderView, String> {
    directory.save(input)
}
fn delete_custom_provider(directory: &ProviderDirectoryService, id: String) -> Result<(), String> {
    directory.delete(&id)
}
fn discover_custom_provider_models(
    directory: &ProviderDirectoryService,
    catalog: &ProviderCatalogService,
    id: String,
) -> Result<Vec<String>, String> {
    let provider = directory.get(&id)?;
    catalog
        .discover_route(&provider)
        .map(|models| models.into_iter().map(|model| model.id).collect())
}
fn probe_custom_provider_chat(
    directory: &ProviderDirectoryService,
    probe: &ProviderChatProbeService,
    id: String,
    model: String,
) -> Result<serde_json::Value, String> {
    let route = directory.get(&id)?;
    serde_json::to_value(probe.probe_route(&route, &model)?).map_err(|error| error.to_string())
}
fn activate_custom_provider_for_state(
    id: String,
    model: String,
    state: &AppState,
) -> Result<(), String> {
    let generation = state.provider_activation.begin()?;
    // Probe the complete candidate before touching either providers.json or
    // config.toml. A failed endpoint/token/model check leaves the current
    // conversation route exactly as it was.
    let candidate = state
        .provider_directory
        .get(&id)
        .map_err(|error| state.provider_activation.fail(generation, error))?;
    state
        .provider_catalog
        .validate_route_model(&candidate, &model)
        .map_err(|error| state.provider_activation.fail(generation, error))?;
    let provider = state.provider_activation.commit(generation, || {
        state.provider_directory.activate(&id, &model)
    })?;
    // Prompt workers resolve the committed route for every turn. This command
    // only refreshes the active UI snapshot; it deliberately does not rebuild
    // tools, MCP, skills or the current transcript.
    state
        .tx
        .send(Command::SetModel(provider.selected_model))
        .map_err(|_| "agent thread is not running".to_string())
}

#[derive(Serialize)]
pub struct RestoreView {
    checkpoint_id: String,
    safety_checkpoint_id: Option<String>,
    restored_files: usize,
    deleted_files: usize,
}

/// Load the resolved config and return a display-safe snapshot.
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
        orchestrator_workers: cfg.orchestrator_workers,
        orchestrator_high_workers: cfg.orchestrator_high_workers,
        orchestrator_verify_retries: cfg.orchestrator_verify_retries,
        orchestrator_max_depth: cfg.orchestrator_max_depth,
        orchestrator_max_subtasks: cfg.orchestrator_max_subtasks,
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
fn validate_image_attachment_route(
    images: &[String],
    model: &str,
    parser_enabled: bool,
    vl_model: &str,
) -> Result<(), String> {
    if images.is_empty() || ncx_core::model_supports_native_vision(model) {
        return Ok(());
    }
    if !parser_enabled {
        return Err(
            "当前主模型未声明附件解析能力。请打开“设置 → 插件”，启用“阿里本地附件解析”；该插件默认关闭。"
                .to_string(),
        );
    }
    if vl_model.trim().is_empty() {
        return Err("阿里本地附件解析插件已启用，但尚未配置解析模型。".to_string());
    }
    Ok(())
}

fn queue_prompt(
    state: &AppState,
    session_id: String,
    text: String,
    images: Vec<String>,
    execution_mode: ncx_protocol::ExecutionMode,
) -> Result<(), String> {
    if !images.is_empty() {
        let cfg = load_config(Overrides {
            workspace: std::env::current_dir().ok(),
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
        validate_image_attachment_route(
            &images,
            &cfg.model,
            cfg.alibaba_attachment_parser_enabled,
            &cfg.vl_model,
        )?;
    }

    if defer_prompt_for_goal(
        &state.running,
        &state.deferred_prompts,
        &session_id,
        DeferredPrompt {
            text: text.clone(),
            images: images.clone(),
            execution_mode,
        },
    )? {
        cancel_session(state, &session_id);
        return Ok(());
    }

    state
        .tx
        .send(Command::Prompt {
            session_id,
            text,
            images,
            execution_mode,
        })
        .map_err(|_| "agent thread is not running".to_string())
}

fn defer_prompt_for_goal(
    running: &RunningSessions,
    deferred_prompts: &DeferredPrompts,
    session_id: &str,
    prompt: DeferredPrompt,
) -> Result<bool, String> {
    // Hold the run-state lock until the deferred prompt is installed. Goal
    // cleanup takes the same lock before consuming this queue, so a finish race
    // cannot strand an accepted human instruction.
    let running = running
        .lock()
        .map_err(|_| "会话执行状态不可用".to_string())?;
    if running.get(session_id).copied() != Some(SessionRunKind::Goal) {
        return Ok(false);
    }
    let mut deferred = deferred_prompts
        .lock()
        .map_err(|_| "会话输入队列不可用".to_string())?;
    if deferred.contains_key(session_id) {
        return Err("长期目标停止后已有一条用户输入等待执行".to_string());
    }
    deferred.insert(session_id.to_string(), prompt);
    Ok(true)
}

#[tauri::command]
fn get_config_location() -> Result<ConfigLocation, String> {
    config_location()
}

/// Switch the process workspace. The frontend follows this with an App Server
/// `threadCreateActivate`, so the new empty Thread is durable before the GUI
/// binds to its id. Do not create a legacy-only session here: Goal reads and
/// the rest of the protocol would have no matching Thread record.
fn transition_workspace_for_state(path: &Path, state: &AppState) -> Result<String, String> {
    let p = PathBuf::from(bridge::display_path(path));
    if !p.is_dir() {
        return Err(format!("not a directory: {}", p.display()));
    }
    let _workspace_gate = state
        .workspace_gate
        .lock()
        .map_err(|_| "工作区切换状态不可用".to_string())?;
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    if memory_merge_cancellation_required_for_workspace_transition(&p, &current) {
        // This is deliberately before changing the process CWD. The
        // coordinator's commit fence shares the same cancellation lock, so a
        // prior workspace merge cannot write its prepared draft after this
        // point. Merely navigating between Threads in the same workspace must
        // not cancel an unrelated merge.
        state.memory_merge.cancel_for_workspace_switch()?;
        std::env::set_current_dir(&p).map_err(|e| format!("cannot enter {}: {e}", p.display()))?;
    }
    bridge::save_last_workspace(&p); // remember it across launches
    Ok(bridge::display_path(&p))
}

/// Switch the process workspace. The frontend follows this with an App Server
/// `threadCreateActivate`, so the new empty Thread is durable before the GUI
/// binds to its id. Do not create a legacy-only session here: Goal reads and the
/// rest of the protocol would have no matching Thread record.
fn set_workspace_for_state(path: String, state: &AppState) -> Result<String, String> {
    // A normal WorkspaceSet is serialized with in-flight New/Resume/Fork
    // handoffs. Startup calls `transition_workspace_for_state` directly,
    // before a worker exists, and deliberately does not need this gate.
    let _runtime_handoff_gate = state.runtime_handoff_gate.acquire()?;
    transition_workspace_for_state(Path::new(path.trim()), state)
}

fn thread_metadata_for_state(
    thread_id: &ThreadId,
    state: &AppState,
) -> Result<ThreadMetadata, String> {
    let outcome = state
        .app_server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    match outcome.response.payload {
        ResponsePayload::Thread(thread) => Ok(thread.metadata),
        _ => Err(format!("会话 {thread_id} 返回了无效的 Thread 元数据")),
    }
}

fn workspace_path_from_metadata(metadata: &ThreadMetadata) -> PathBuf {
    PathBuf::from(bridge::display_path(Path::new(&metadata.workspace)))
}

/// Compare a caller's workspace snapshot with the process' current workspace.
/// Canonicalization removes separator/case/`..` spelling differences while
/// requiring both paths to resolve to existing directories; an unresolved
/// caller path is rejected rather than guessed.
fn workspace_matches(expected: &str, current: &Path) -> bool {
    let Ok(expected) = std::fs::canonicalize(expected) else {
        return false;
    };
    let Ok(current) = std::fs::canonicalize(current) else {
        return false;
    };
    #[cfg(windows)]
    {
        expected
            .to_string_lossy()
            .eq_ignore_ascii_case(&current.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        expected == current
    }
}

/// A memory merge is bound to the process workspace that started it. Only a
/// real workspace change crosses that boundary; Thread navigation inside one
/// project deliberately keeps the merge alive.
fn memory_merge_cancellation_required_for_workspace_transition(
    target: &Path,
    current: &Path,
) -> bool {
    !workspace_matches(&bridge::display_path(target), current)
}

/// Reject an operation when its caller observed a different workspace than the
/// one currently selected by the process. Keeping this check separate makes
/// the fail-closed boundary directly testable; callers must still hold
/// `workspace_gate` while they use the returned current workspace.
fn require_workspace_match(expected: &str, current: &Path) -> Result<(), String> {
    if workspace_matches(expected, current) {
        return Ok(());
    }
    Err(format!(
        "工作区已切换，拒绝执行旧项目请求（请求：{}，当前：{}）",
        expected,
        bridge::display_path(current),
    ))
}

/// Run a workspace-bound operation against the caller's exact workspace
/// snapshot. Tauri invokes can overlap, while the process CWD is global: the
/// gate therefore covers both the snapshot comparison and the operation so a
/// later `workspaceSet` cannot redirect an already-accepted request.
fn with_workspace_snapshot<T>(
    state: &AppState,
    expected_workspace: &str,
    operation: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let _workspace_gate = state
        .workspace_gate
        .lock()
        .map_err(|_| "工作区切换状态不可用".to_string())?;
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    require_workspace_match(expected_workspace, &current)?;
    operation(&current)
}

/// Switch the active model (persists + rebuilds keeping the current transcript).
fn set_model_for_state(model: String, state: &AppState) -> Result<(), String> {
    let generation = state.provider_activation.begin()?;
    let cfg = load_config(Overrides::default()).map_err(|error| {
        state
            .provider_activation
            .fail(generation, error.to_string())
    })?;
    if let Some(provider_id) = cfg.active_provider_id.strip_prefix("preset:") {
        let cached = state.openrouter_models.lock().map_err(|_| {
            state
                .provider_activation
                .fail(generation, "OpenRouter 模型缓存不可用".to_string())
        })?;
        let yunmo = state.yunmo_models.lock().map_err(|_| {
            state
                .provider_activation
                .fail(generation, "云末模型缓存不可用".to_string())
        })?;
        let preset = provider_models(provider_id, &cached, &yunmo)
            .into_iter()
            .find(|item| item.model_id == model)
            .ok_or_else(|| {
                state
                    .provider_activation
                    .fail(generation, format!("模型 {model} 不属于当前预设模型商"))
            })?;
        let quick_switch_models = provider_models(provider_id, &cached, &yunmo)
            .into_iter()
            .map(|item| item.model_id)
            .collect::<Vec<_>>();
        drop(cached);
        drop(yunmo);
        state.provider_activation.commit(generation, || {
            write_preset(&state.provider_directory, &preset, &quick_switch_models)
        })?;
        return state
            .tx
            .send(Command::SetModel(model))
            .map_err(|_| "agent thread is not running".to_string());
    }
    if cfg.active_provider_id != "legacy" {
        let candidate = state
            .provider_directory
            .get(&cfg.active_provider_id)
            .map_err(|error| state.provider_activation.fail(generation, error))?;
        state
            .provider_catalog
            .validate_route_model(&candidate, &model)
            .map_err(|error| state.provider_activation.fail(generation, error))?;
        state.provider_activation.commit(generation, || {
            state
                .provider_directory
                .activate(&cfg.active_provider_id, &model)
        })?;
        return state
            .tx
            .send(Command::SetModel(model))
            .map_err(|_| "agent thread is not running".to_string());
    }
    let cached = state.openrouter_models.lock().map_err(|_| {
        state
            .provider_activation
            .fail(generation, "OpenRouter 模型缓存不可用".to_string())
    })?;
    let yunmo = state.yunmo_models.lock().map_err(|_| {
        state
            .provider_activation
            .fail(generation, "云末模型缓存不可用".to_string())
    })?;
    if let Some(preset) = find_preset_by_model_id(&model, &cfg.base_url, &cached, &yunmo) {
        let quick_switch_models = provider_models(&preset.provider_id, &cached, &yunmo)
            .into_iter()
            .map(|item| item.model_id)
            .collect::<Vec<_>>();
        drop(cached);
        drop(yunmo);
        state.provider_activation.commit(generation, || {
            write_preset(&state.provider_directory, &preset, &quick_switch_models)
        })?;
    } else {
        let updates = HashMap::from([("model", model.as_str())]);
        state.provider_activation.commit(generation, || {
            write_nanocodex_config(&updates, &ConfigPaths::default().nanocodex)
                .map_err(|error| error.to_string())
        })?;
    }
    state
        .tx
        .send(Command::SetModel(model))
        .map_err(|_| "agent thread is not running".to_string())
}

/// Switch the CC permission mode (plan / default / accept-edits / bypass).
fn set_permission_mode_for_state(
    thread_id: &ThreadId,
    mode: String,
    state: &AppState,
) -> Result<(), String> {
    // Hold the handoff gate from token creation through the worker's result.
    // This establishes one order for every runtime replacement, including
    // the process-CWD transition performed by the navigation operations.
    let _runtime_handoff_gate = state.runtime_handoff_gate.acquire()?;
    // Do not snapshot the process CWD here. A workspace transition can run
    // between this request and the worker command. The worker validates this
    // durable Thread against its active session and resolves its own stored
    // workspace before rebuilding.
    let activation = state.runtime_activation.begin();
    let (completion_tx, completion_rx) = mpsc::channel();
    if state
        .tx
        .send(Command::SetPermissionMode {
            thread_id: thread_id.to_string(),
            mode,
            activation: activation.clone(),
            completion: completion_tx,
        })
        .is_err()
    {
        state.runtime_activation.abort_if_pending(&activation);
        return Err("agent thread is not running".to_string());
    }
    await_runtime_activation(
        completion_rx,
        &state.runtime_activation,
        activation,
        "切换权限模式超时：后台 Agent 未在 30 秒内完成重建",
    )
}

/// Ask the agent thread to re-emit its `ready` snapshot (called by the UI once
/// its event listener is up, so the initial emit isn't missed).
fn request_ready_for_state(state: &AppState) -> Result<(), String> {
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
    let runtime = GuiAppServerAdapter {
        state: &state,
        app: &app,
    };
    let outcome = state
        .app_server
        .dispatch_with_runtime(request, &runtime)
        .map_err(|error| error.to_string())?;
    emit_protocol_outcome(&app, &outcome);
    Ok(outcome)
}

struct GuiAppServerAdapter<'a> {
    state: &'a AppState,
    app: &'a AppHandle,
}

/// Wait for a serialized GUI runtime command without guessing who won a
/// timeout race. The worker installs a new live session only after it wins the
/// same activation fence. If it already accepted while the receiver timed out,
/// treating the request as successful prevents the App Server from deleting
/// durable state now owned by that runtime.
fn await_runtime_activation(
    completion: mpsc::Receiver<Result<(), String>>,
    coordinator: &RuntimeActivationCoordinator,
    activation: RuntimeActivation,
    timeout_message: &str,
) -> Result<(), String> {
    match completion.recv_timeout(Duration::from_secs(30)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) if coordinator.abort_if_pending(&activation) => {
            Err(timeout_message.to_string())
        }
        Err(mpsc::RecvTimeoutError::Timeout) if activation.is_accepted() => Ok(()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(timeout_message.to_string()),
        Err(mpsc::RecvTimeoutError::Disconnected) if activation.is_accepted() => Ok(()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            coordinator.abort_if_pending(&activation);
            Err("后台 Agent 在完成初始化前停止响应".to_string())
        }
    }
}

/// Finish one runtime handoff and compensate a process-global CWD change when
/// the worker rejected (or never accepted) the activation. The handoff gate is
/// held by every caller, so this helper acquires `workspace_gate` second and
/// keeps the established `runtime_handoff_gate -> workspace_gate` order.
///
/// A timeout/error first aborts the pending activation. The coordinator then
/// runs the short rollback only if that token is still the newest one and is
/// terminally aborted. An accepted token (including a result racing a timeout)
/// and a token superseded by a newer handoff are left untouched.
fn finish_runtime_handoff(
    state: &AppState,
    activation: &RuntimeActivation,
    previous_cwd: &Path,
    target_workspace: &Path,
    result: Result<(), String>,
) -> Result<(), String> {
    let Err(error) = result else {
        return result;
    };
    let rollback = run_failed_handoff_rollback(
        &state.runtime_activation,
        activation,
        || -> Result<(), String> {
            let _workspace_gate = state
                .workspace_gate
                .lock()
                .map_err(|_| "工作区回滚状态不可用".to_string())?;
            let current =
                std::env::current_dir().map_err(|current_error| current_error.to_string())?;
            // Never overwrite a workspace selected after this handoff. Under the
            // runtime gate this should only differ when an external caller or a
            // deliberately racing test changed the process CWD.
            if !workspace_matches(&bridge::display_path(target_workspace), &current) {
                return Ok(());
            }
            if memory_merge_cancellation_required_for_workspace_transition(previous_cwd, &current) {
                // The target workspace may have started a merge while the worker
                // was initializing. Cancel it before returning to the old CWD so
                // its prepared draft cannot commit after this failed handoff.
                state.memory_merge.cancel_for_workspace_switch()?;
            }
            std::env::set_current_dir(previous_cwd).map_err(|rollback_error| {
                format!(
                    "cannot restore workspace {}: {rollback_error}",
                    previous_cwd.display()
                )
            })?;
            bridge::save_last_workspace(previous_cwd);
            Ok(())
        },
    );
    match rollback {
        None | Some(Ok(())) => Err(error),
        Some(Err(rollback_error)) => {
            Err(format!("{error}；恢复进入前工作区失败：{rollback_error}"))
        }
    }
}

/// Fence an unsuccessful activation before running a short compensation. The
/// closure is skipped for an accepted token and for a stale token replaced by
/// a later handoff, so it is safe for process-global effects such as CWD.
fn run_failed_handoff_rollback<T>(
    coordinator: &RuntimeActivationCoordinator,
    activation: &RuntimeActivation,
    rollback: impl FnOnce() -> T,
) -> Option<T> {
    coordinator.abort_if_pending(activation);
    coordinator.run_if_current_and_aborted(activation, rollback)
}

impl AppServerAdapter for GuiAppServerAdapter<'_> {
    fn validate_harness_profile(&self, profile: &str, workspace: &str) -> Result<(), String> {
        let workspace = PathBuf::from(bridge::display_path(Path::new(workspace)));
        HarnessRuntimeBuilder::configured_for_profile(&workspace, Some(profile)).map(|_| ())
    }

    fn create_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        // Keep the handoff gate until the worker accepts or rejects this
        // activation. `transition_workspace_for_state` then takes the
        // workspace gate second, preserving the global lock order.
        let _runtime_handoff_gate = self.state.runtime_handoff_gate.acquire()?;
        let previous_cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let activation = self.state.runtime_activation.begin();
        let metadata = match thread_metadata_for_state(thread_id, self.state) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.state.runtime_activation.abort_if_pending(&activation);
                return Err(error);
            }
        };
        let workspace = workspace_path_from_metadata(&metadata);
        let target_workspace = workspace.clone();
        if let Err(error) = transition_workspace_for_state(&workspace, self.state) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err(error),
            );
        }
        if !self.state.runtime_activation.can_proceed(&activation) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("新建会话已被更新的运行态切换取消".to_string()),
            );
        }
        let (completion_tx, completion_rx) = mpsc::channel();
        if self
            .state
            .tx
            .send(Command::New {
                id: thread_id.to_string(),
                workspace,
                harness_profile: metadata.harness_profile,
                activation: activation.clone(),
                completion: completion_tx,
            })
            .is_err()
        {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("agent thread is not running".to_string()),
            );
        }
        finish_runtime_handoff(
            self.state,
            &activation,
            &previous_cwd,
            &target_workspace,
            await_runtime_activation(
                completion_rx,
                &self.state.runtime_activation,
                activation.clone(),
                "切换工作区超时：后台 Agent 未在 30 秒内完成初始化",
            ),
        )
    }

    fn activate_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        let _runtime_handoff_gate = self.state.runtime_handoff_gate.acquire()?;
        let previous_cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let activation = self.state.runtime_activation.begin();
        let metadata = match thread_metadata_for_state(thread_id, self.state) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.state.runtime_activation.abort_if_pending(&activation);
                return Err(error);
            }
        };
        let workspace = workspace_path_from_metadata(&metadata);
        let target_workspace = workspace.clone();
        if let Err(error) = transition_workspace_for_state(&workspace, self.state) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err(error),
            );
        }
        if !self.state.runtime_activation.can_proceed(&activation) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("恢复会话已被更新的运行态切换取消".to_string()),
            );
        }
        let (completion_tx, completion_rx) = mpsc::channel();
        if self
            .state
            .tx
            .send(Command::Resume {
                id: thread_id.to_string(),
                workspace,
                activation: activation.clone(),
                completion: completion_tx,
            })
            .is_err()
        {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("agent thread is not running".to_string()),
            );
        }
        finish_runtime_handoff(
            self.state,
            &activation,
            &previous_cwd,
            &target_workspace,
            await_runtime_activation(
                completion_rx,
                &self.state.runtime_activation,
                activation.clone(),
                "恢复会话超时：后台 Agent 未在 30 秒内完成切换",
            ),
        )
    }

    fn fork_thread(&self, source_id: &ThreadId, target_id: &ThreadId) -> Result<(), String> {
        let _runtime_handoff_gate = self.state.runtime_handoff_gate.acquire()?;
        let previous_cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let activation = self.state.runtime_activation.begin();
        let metadata = match thread_metadata_for_state(target_id, self.state) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.state.runtime_activation.abort_if_pending(&activation);
                return Err(error);
            }
        };
        let workspace = workspace_path_from_metadata(&metadata);
        let target_workspace = workspace.clone();
        if let Err(error) = transition_workspace_for_state(&workspace, self.state) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err(error),
            );
        }
        if !self.state.runtime_activation.can_proceed(&activation) {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("分叉会话已被更新的运行态切换取消".to_string()),
            );
        }
        let (completion_tx, completion_rx) = mpsc::channel();
        if self
            .state
            .tx
            .send(Command::Fork {
                source_id: source_id.to_string(),
                target_id: target_id.to_string(),
                workspace,
                activation: activation.clone(),
                completion: completion_tx,
            })
            .is_err()
        {
            return finish_runtime_handoff(
                self.state,
                &activation,
                &previous_cwd,
                &target_workspace,
                Err("agent thread is not running".to_string()),
            );
        }
        finish_runtime_handoff(
            self.state,
            &activation,
            &previous_cwd,
            &target_workspace,
            await_runtime_activation(
                completion_rx,
                &self.state.runtime_activation,
                activation.clone(),
                "分叉会话超时：后台 Agent 未在 30 秒内完成切换",
            ),
        )
    }

    fn submit_turn(
        &self,
        thread_id: &ThreadId,
        text: String,
        images: Vec<String>,
        execution_mode: ncx_protocol::ExecutionMode,
    ) -> Result<(), String> {
        queue_prompt(
            self.state,
            thread_id.to_string(),
            text,
            images,
            execution_mode,
        )
    }

    fn interrupt_latest(&self, thread_id: &ThreadId) -> Result<(), String> {
        cancel_session(self.state, thread_id.as_str());
        Ok(())
    }

    fn continue_goal(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.state
            .tx
            .send(Command::ContinueGoal(thread_id.to_string()))
            .map_err(|_| "长期目标执行队列不可用".to_string())
    }

    fn runtime_status(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(get_status()?).map_err(|error| error.to_string())
    }

    fn refresh_ready(&self) -> Result<(), String> {
        request_ready_for_state(self.state)
    }

    fn set_workspace(&self, path: String) -> Result<String, String> {
        set_workspace_for_state(path, self.state)
    }

    fn approve(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        decision: String,
    ) -> Result<(), String> {
        approve_for_thread(self.state, thread_id.map(ThreadId::as_str), id, decision)
    }

    fn answer(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        answer: Option<String>,
    ) -> Result<(), String> {
        answer_for_thread(self.state, thread_id.map(ThreadId::as_str), id, answer)
    }

    fn read_settings(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(get_settings(&self.state.provider_catalog)?)
            .map_err(|error| error.to_string())
    }

    fn update_settings(
        &self,
        updates: std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        save_settings_for_state(updates.into_iter().collect(), self.state)
    }

    fn set_model(&self, model: String) -> Result<(), String> {
        set_model_for_state(model, self.state)
    }

    fn set_permission_mode(&self, thread_id: &ThreadId, mode: String) -> Result<(), String> {
        set_permission_mode_for_state(thread_id, mode, self.state)
    }

    fn read_model_catalog(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(get_model_catalog_for_state(self.state))
            .map_err(|error| error.to_string())
    }

    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(apply_model_preset_for_state(
            provider_id,
            model_id,
            self.state,
        )?)
        .map_err(|error| error.to_string())
    }

    fn list_custom_providers(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(list_custom_providers(self.state)?).map_err(|error| error.to_string())
    }

    fn save_custom_provider(
        &self,
        id: Option<String>,
        name: String,
        protocol: String,
        base_url: String,
        api_key: Option<String>,
        models: Vec<String>,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(save_custom_provider(
            &self.state.provider_directory,
            CustomProviderInput {
                id,
                name,
                protocol,
                base_url,
                api_key,
                models,
            },
        )?)
        .map_err(|error| error.to_string())
    }

    fn delete_custom_provider(&self, id: String) -> Result<(), String> {
        delete_custom_provider(&self.state.provider_directory, id)
    }

    fn discover_custom_provider_models(&self, id: String) -> Result<Vec<String>, String> {
        discover_custom_provider_models(
            &self.state.provider_directory,
            &self.state.provider_catalog,
            id,
        )
    }

    fn activate_custom_provider(&self, id: String, model: String) -> Result<(), String> {
        activate_custom_provider_for_state(id, model, self.state)
    }

    fn probe_custom_provider_chat(
        &self,
        id: String,
        model: String,
    ) -> Result<serde_json::Value, String> {
        probe_custom_provider_chat(
            &self.state.provider_directory,
            &self.state.provider_chat_probe,
            id,
            model,
        )
    }

    fn harness_diagnostics(&self) -> Result<serde_json::Value, String> {
        get_harness_diagnostics(
            &self.state.provider_directory,
            &self.state.provider_activation,
        )
    }

    fn list_external_plugins(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(list_external_plugins()?).map_err(|error| error.to_string())
    }

    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(install_external_plugin(source, upgrade)?)
            .map_err(|error| error.to_string())
    }

    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String> {
        set_external_plugin_enabled(id, enabled)
    }

    fn list_memory(&self, expected_workspace: String) -> Result<serde_json::Value, String> {
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            serde_json::to_value(memory_list_at(workspace)?).map_err(|error| error.to_string())
        })
    }

    fn add_memory(
        &self,
        note: String,
        tags: Vec<String>,
        expected_workspace: String,
    ) -> Result<bool, String> {
        let _workspace_gate = self
            .state
            .workspace_gate
            .lock()
            .map_err(|_| "工作区切换状态不可用".to_string())?;
        let current = std::env::current_dir().map_err(|error| error.to_string())?;
        if !workspace_matches(&expected_workspace, &current) {
            return Err("工作区已切换，拒绝写入当前项目记忆".to_string());
        }
        memory_add(note, tags)
    }

    fn consolidate_memory(&self, expected_workspace: String) -> Result<u64, String> {
        let _workspace_gate = self
            .state
            .workspace_gate
            .lock()
            .map_err(|_| "工作区切换状态不可用".to_string())?;
        let current = std::env::current_dir().map_err(|error| error.to_string())?;
        if !workspace_matches(&expected_workspace, &current) {
            return Err("工作区已切换，拒绝整理当前项目记忆".to_string());
        }
        memory_consolidate().map(|count| count as u64)
    }

    fn start_memory_merge(&self, expected_workspace: String) -> Result<serde_json::Value, String> {
        let _workspace_gate = self
            .state
            .workspace_gate
            .lock()
            .map_err(|_| "工作区切换状态不可用".to_string())?;
        let cfg = load_config(Overrides {
            workspace: std::env::current_dir().ok(),
            ..Default::default()
        })
        .map_err(|error| error.to_string())?;
        let workspace = cfg.workspace.clone();
        if !workspace_matches(&expected_workspace, &workspace) {
            return Err(format!(
                "工作区已切换，拒绝在当前项目启动记忆整理（请求：{}，当前：{}）",
                expected_workspace,
                bridge::display_path(&workspace),
            ));
        }
        serde_json::to_value(self.state.memory_merge.start(cfg, workspace)?)
            .map_err(|error| error.to_string())
    }

    fn memory_merge_status(
        &self,
        expected_workspace: String,
        generation: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            serde_json::to_value(
                self.state
                    .memory_merge
                    .status_for_workspace(workspace, generation)?,
            )
            .map_err(|error| error.to_string())
        })
    }

    fn cancel_memory_merge(
        &self,
        expected_workspace: String,
        generation: u64,
    ) -> Result<serde_json::Value, String> {
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            serde_json::to_value(
                self.state
                    .memory_merge
                    .cancel_for_workspace(workspace, generation)?,
            )
            .map_err(|error| error.to_string())
        })
    }

    fn forge_runtime_status(&self) -> Result<serde_json::Value, String> {
        let resource_dir = self
            .app
            .path()
            .resource_dir()
            .map_err(|_| "无法定位应用资源目录".to_string())?;
        Ok(match forge_runtime::discover(&resource_dir) {
            Ok(paths) => serde_json::json!({
                "available": paths.root.is_dir()
                    && paths.python.is_file()
                    && paths.script.is_file()
                    && paths.agent.is_file(),
                "schema": "buglecat-forge-runtime/v1"
            }),
            Err(reason) => serde_json::json!({ "available": false, "reason": reason }),
        })
    }

    fn start_forge_job(
        &self,
        expected_workspace: String,
        rounds: u8,
        repeats: u8,
        timeout_s: u64,
        budget_s: u64,
        teacher: String,
        accept_margin: u8,
    ) -> Result<serde_json::Value, String> {
        let resource_dir = self
            .app
            .path()
            .resource_dir()
            .map_err(|_| "无法定位应用资源目录".to_string())?;
        let runtime = forge_runtime::discover(&resource_dir)?;
        let input = forge_job::ForgeJobInput {
            rounds,
            repeats,
            timeout_s,
            budget_s,
            teacher,
            accept_margin,
        };
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            self.state
                .forge_job
                .start(input, runtime, workspace.to_path_buf())
        })
        .and_then(|status| serde_json::to_value(status).map_err(|error| error.to_string()))
    }

    fn forge_job_status(
        &self,
        expected_workspace: String,
        generation: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            serde_json::to_value(
                self.state
                    .forge_job
                    .status_for_workspace(workspace, generation)?,
            )
            .map_err(|error| error.to_string())
        })
    }

    fn cancel_forge_job(
        &self,
        expected_workspace: String,
        generation: u64,
    ) -> Result<serde_json::Value, String> {
        with_workspace_snapshot(self.state, &expected_workspace, |workspace| {
            serde_json::to_value(
                self.state
                    .forge_job
                    .cancel_for_workspace(workspace, generation)?,
            )
            .map_err(|error| error.to_string())
        })
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

    fn search_dsh_marketplace(
        &self,
        source: String,
        manifest_url: Option<String>,
        query: String,
    ) -> Result<serde_json::Value, String> {
        dsh_marketplace_search(&source, manifest_url.as_deref(), &query)
    }

    fn preview_dsh_marketplace_plugin(
        &self,
        item: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        dsh_marketplace_preview(&item)
    }

    fn install_dsh_marketplace_plugin(
        &self,
        item: serde_json::Value,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(install_dsh_marketplace_plugin(&item, upgrade)?)
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
    orchestrator_workers: i64,
    orchestrator_high_workers: i64,
    orchestrator_verify_retries: i64,
    orchestrator_max_depth: i64,
    orchestrator_max_subtasks: i64,
    context_edit_enabled: bool,
    context_edit_max_chars: i64,
    context_edit_keep_recent_messages: i64,
    context_edit_max_tool_result_chars: i64,
    alibaba_attachment_parser_enabled: bool,
    price_in: f64,
    price_out: f64,
    price_currency: String,
    api_key_masked: String,
    has_api_key: bool,
    deepseek_api_key_masked: String,
    has_deepseek_api_key: bool,
    yunmo_api_key_masked: String,
    has_yunmo_api_key: bool,
    vl_api_key_masked: String,
    has_vl_api_key: bool,
    dashscope_token_plan_key_masked: String,
    has_dashscope_token_plan_key: bool,
    dashscope_workspace_key_masked: String,
    has_dashscope_workspace_key: bool,
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
        orchestrator_workers: cfg.orchestrator_workers,
        orchestrator_high_workers: cfg.orchestrator_high_workers,
        orchestrator_verify_retries: cfg.orchestrator_verify_retries,
        orchestrator_max_depth: cfg.orchestrator_max_depth,
        orchestrator_max_subtasks: cfg.orchestrator_max_subtasks,
        context_edit_enabled: cfg.context_edit_enabled,
        context_edit_max_chars: cfg.context_edit_max_chars,
        context_edit_keep_recent_messages: cfg.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: cfg.context_edit_max_tool_result_chars,
        alibaba_attachment_parser_enabled: cfg.alibaba_attachment_parser_enabled,
        price_in: cfg.price_in,
        price_out: cfg.price_out,
        price_currency: cfg.price_currency.clone(),
        api_key_masked: redacted.get("api_key").cloned().unwrap_or_default(),
        has_api_key: !cfg.api_key.is_empty(),
        deepseek_api_key_masked: redacted
            .get("deepseek_api_key")
            .cloned()
            .unwrap_or_default(),
        has_deepseek_api_key: !cfg.deepseek_api_key.is_empty(),
        yunmo_api_key_masked: redacted.get("yunmo_api_key").cloned().unwrap_or_default(),
        has_yunmo_api_key: !cfg.yunmo_api_key.is_empty(),
        vl_api_key_masked: redacted.get("vl_api_key").cloned().unwrap_or_default(),
        has_vl_api_key: !cfg.vl_api_key.is_empty(),
        dashscope_token_plan_key_masked: redacted
            .get("dashscope_token_plan_key")
            .cloned()
            .unwrap_or_default(),
        has_dashscope_token_plan_key: !cfg.dashscope_token_plan_key.is_empty(),
        dashscope_workspace_key_masked: redacted
            .get("dashscope_workspace_key")
            .cloned()
            .unwrap_or_default(),
        has_dashscope_workspace_key: !cfg.dashscope_workspace_key.is_empty(),
        available_models: cfg.available_models.clone(),
        sandbox_modes: VALID_SANDBOX_MODES.iter().map(|s| s.to_string()).collect(),
        approval_policies: VALID_APPROVAL_POLICIES
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Read the current settings for the panel (with dropdown option lists).
fn get_settings(provider_catalog: &ProviderCatalogService) -> Result<Settings, String> {
    let workspace = std::env::current_dir().ok();
    let cfg = load_config(Overrides {
        workspace,
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let mut settings = settings_from_config(&cfg);
    settings.available_models = merged_available_models(&cfg, provider_catalog);
    Ok(settings)
}

fn valid_provider_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
        })
}

fn merged_available_models(cfg: &Config, provider_catalog: &ProviderCatalogService) -> Vec<String> {
    let discovered = provider_catalog
        .discover_config(cfg)
        .unwrap_or_default()
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for model in std::iter::once(cfg.model.as_str())
        .chain(cfg.available_models.iter().map(String::as_str))
        .chain(discovered.iter().map(String::as_str))
    {
        if valid_provider_model_id(model) && seen.insert(model.to_string()) {
            models.push(model.to_string());
        }
    }
    models
}

/// Persist settings to `~/.nanocodex/config.toml`, then rebuild the agent so the
/// change applies live. Empty values are skipped (so a blank API key keeps the
/// existing one). Only known keys are written.
fn save_settings_for_state(
    updates: std::collections::HashMap<String, String>,
    state: &AppState,
) -> Result<(), String> {
    let path = ConfigPaths::default().nanocodex;
    persist_validated_settings(&updates, &path)?;
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

fn persist_validated_settings(
    updates: &HashMap<String, String>,
    path: &Path,
) -> Result<(), String> {
    validate_orchestrator_setting_updates(updates)?;
    let borrowed: HashMap<&str, &str> = updates
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    write_nanocodex_config(&borrowed, path).map_err(|e| e.to_string())
}

fn validate_orchestrator_setting_updates(updates: &HashMap<String, String>) -> Result<(), String> {
    for (key, label, min, max) in [
        ("orchestrator_workers", "普通任务 Worker", 1, 4),
        ("orchestrator_high_workers", "高风险任务 Worker", 1, 6),
        ("orchestrator_verify_retries", "验证重试", 0, 3),
        ("orchestrator_max_depth", "递归深度", 0, 2),
        ("orchestrator_max_subtasks", "子任务上限", 1, 12),
    ] {
        let Some(raw) = updates.get(key) else {
            continue;
        };
        let value = raw
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("{label}必须是 {min}–{max} 的整数"))?;
        if !(min..=max).contains(&value) {
            return Err(format!("{label}必须是 {min}–{max} 的整数，当前为 {value}"));
        }
    }
    for (key, label, min, max) in [
        ("max_iterations", "最大迭代次数", 1, 10_000),
        ("max_tool_calls", "最大工具调用数", 1, 100_000),
        (
            "context_edit_max_chars",
            "上下文最大字符数",
            1_000,
            1_000_000,
        ),
        ("context_edit_keep_recent_messages", "保留消息数", 1, 1_000),
        (
            "context_edit_max_tool_result_chars",
            "工具结果最大字符数",
            100,
            100_000,
        ),
    ] {
        if let Some(raw) = updates.get(key) {
            let value = raw
                .trim()
                .parse::<i64>()
                .map_err(|_| format!("{label}必须是 {min}–{max} 的整数"))?;
            if !(min..=max).contains(&value) {
                return Err(format!("{label}必须是 {min}–{max} 的整数，当前为 {value}"));
            }
        }
    }
    for key in ["context_edit_enabled", "alibaba_attachment_parser_enabled"] {
        if let Some(raw) = updates.get(key) {
            if !matches!(raw.trim(), "true" | "false") {
                return Err(format!("{key}必须是 true 或 false"));
            }
        }
    }
    for key in ["sandbox_mode", "approval_policy"] {
        if let Some(raw) = updates.get(key) {
            let valid = if key == "sandbox_mode" {
                VALID_SANDBOX_MODES.contains(&raw.trim())
            } else {
                VALID_APPROVAL_POLICIES.contains(&raw.trim())
            };
            if !valid {
                return Err(format!("{key}不是受支持的值"));
            }
        }
    }
    if let Some(raw) = updates.get("price_currency") {
        if !VALID_PRICE_CURRENCIES.contains(&raw.trim()) {
            return Err("price_currency 不是受支持的币种".into());
        }
    }
    for key in ["price_in", "price_out"] {
        if let Some(raw) = updates.get(key) {
            let value = raw
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("{key}必须是非负数字"))?;
            if !value.is_finite() || !(0.0..=1_000_000.0).contains(&value) {
                return Err(format!("{key}必须在 0 到 1000000 之间"));
            }
        }
    }
    for key in ["model", "vl_model"] {
        if let Some(raw) = updates.get(key) {
            if !valid_provider_model_id(raw.trim()) {
                return Err(format!("{key}格式无效"));
            }
        }
    }
    for key in ["base_url", "vl_base_url"] {
        if let Some(raw) = updates.get(key) {
            let value = raw.trim();
            if !value.is_empty() && !(value.starts_with("https://") || value.starts_with("http://"))
            {
                return Err(format!("{key}必须是 http(s) URL"));
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct ModelCatalogResponse {
    providers: Vec<CatalogProvider>,
    /// 当实时目录不可用时，前端仍可使用内置目录。
    stale: bool,
}

fn catalog_response(
    openrouter_models: &[CatalogModel],
    yunmo_models: &[CatalogModel],
    stale: bool,
) -> ModelCatalogResponse {
    let mut providers = catalog();
    if !openrouter_models.is_empty() {
        if let Some(provider) = providers
            .iter_mut()
            .find(|provider| provider.id == "openrouter")
        {
            provider.models = openrouter_models.to_vec();
        }
    }
    if !yunmo_models.is_empty() {
        if let Some(provider) = providers.iter_mut().find(|provider| provider.id == "yunmo") {
            provider.models = yunmo_models.to_vec();
        }
    }
    ModelCatalogResponse { providers, stale }
}

fn write_preset(
    directory: &ProviderDirectoryService,
    preset: &CatalogModel,
    quick_switch_models: &[String],
) -> Result<(), String> {
    let cfg = load_config(Overrides::default()).map_err(|error| error.to_string())?;
    write_preset_with_config(directory, preset, quick_switch_models, &cfg)
}

fn write_preset_with_config(
    directory: &ProviderDirectoryService,
    preset: &CatalogModel,
    quick_switch_models: &[String],
    cfg: &Config,
) -> Result<(), String> {
    let route_id = format!("preset:{}", preset.provider_id);
    let provider_key = resolve_preset_key(directory, cfg, preset)?;
    let provider_name = catalog()
        .into_iter()
        .find(|provider| provider.id == preset.provider_id)
        .map(|provider| provider.name)
        .unwrap_or_else(|| preset.provider_id.clone());
    directory.save_and_activate_preset(
        CustomProviderInput {
            id: Some(route_id),
            name: provider_name,
            protocol: "openai".to_string(),
            base_url: preset.base_url.clone(),
            api_key: Some(provider_key),
            models: quick_switch_models.to_vec(),
        },
        &preset.model_id,
        &preset.price_in.to_string(),
        &preset.price_out.to_string(),
        &preset.price_currency,
    )?;
    Ok(())
}

fn resolve_preset_key(
    directory: &ProviderDirectoryService,
    cfg: &Config,
    preset: &CatalogModel,
) -> Result<String, String> {
    let route_id = format!("preset:{}", preset.provider_id);
    let saved_key = directory
        .get(&route_id)
        .ok()
        .map(|route| route.api_key)
        .filter(|key| !key.trim().is_empty());
    let compatibility_key = match preset.provider_id.as_str() {
        "deepseek" => (!cfg.deepseek_api_key.trim().is_empty())
            .then(|| cfg.deepseek_api_key.clone())
            .or_else(|| {
                cfg.base_url
                    .contains("api.deepseek.com")
                    .then(|| cfg.api_key.clone())
            }),
        "yunmo" => (!cfg.yunmo_api_key.trim().is_empty())
            .then(|| cfg.yunmo_api_key.clone())
            .or_else(|| {
                cfg.base_url
                    .contains("api.yunmo-ai.com")
                    .then(|| cfg.api_key.clone())
            }),
        _ => (cfg.base_url.trim_end_matches('/') == preset.base_url.trim_end_matches('/'))
            .then(|| cfg.api_key.clone()),
    };
    let provider_key = saved_key
        .or(compatibility_key)
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| format!("请先为 {} 配置独立 Token", preset.provider_id))?;
    Ok(provider_key)
}

fn write_available_models(models: &[String]) -> Result<(), String> {
    let available_models = models.join(",");
    let updates = HashMap::from([("available_models", available_models.as_str())]);
    let path = ConfigPaths::default().nanocodex;
    write_nanocodex_config(&updates, &path).map_err(|error| error.to_string())
}

fn provider_models(
    provider_id: &str,
    openrouter_models: &[CatalogModel],
    yunmo_models: &[CatalogModel],
) -> Vec<CatalogModel> {
    if provider_id == "openrouter" && !openrouter_models.is_empty() {
        return openrouter_models.to_vec();
    }
    if provider_id == "yunmo" && !yunmo_models.is_empty() {
        return yunmo_models.to_vec();
    }
    catalog()
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .map(|provider| provider.models)
        .unwrap_or_default()
}

fn find_preset_by_model_id(
    model_id: &str,
    active_base_url: &str,
    openrouter_models: &[CatalogModel],
    yunmo_models: &[CatalogModel],
) -> Option<CatalogModel> {
    let candidates = openrouter_models
        .iter()
        .cloned()
        .chain(yunmo_models.iter().cloned())
        .chain(catalog().into_iter().flat_map(|provider| provider.models));
    let active_base_url = active_base_url.trim_end_matches('/');
    candidates
        .filter(|model| model.model_id == model_id)
        .find(|model| model.base_url.trim_end_matches('/') == active_base_url)
}

/// 返回内置目录，并在有缓存时带上 OpenRouter 的实时模型清单。
fn get_model_catalog_for_state(state: &AppState) -> ModelCatalogResponse {
    let cached = state
        .openrouter_models
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let yunmo = state
        .yunmo_models
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    catalog_response(&cached, &yunmo, false)
}

/// 通过 OpenRouter 公共接口拉取模型和每 Token 费用；接口无需 API 密钥。
#[tauri::command]
async fn refresh_openrouter_models(
    state: tauri::State<'_, AppState>,
) -> Result<ModelCatalogResponse, String> {
    let catalog = state.provider_catalog.clone();
    let models = tokio::task::spawn_blocking(move || {
        catalog
            .discover_public("https://openrouter.ai/api/v1")
            .map(|models| models.into_iter().map(openrouter_model).collect::<Vec<_>>())
    })
    .await
    .map_err(|error| format!("OpenRouter 模型目录任务失败：{error}"))??;
    let mut cached = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    *cached = models;
    let yunmo = state
        .yunmo_models
        .lock()
        .map_err(|_| "云末模型缓存不可用".to_string())?;
    Ok(catalog_response(&cached, &yunmo, false))
}

#[tauri::command]
async fn refresh_yunmo_models(
    state: tauri::State<'_, AppState>,
) -> Result<ModelCatalogResponse, String> {
    let cfg = load_config(Overrides::default()).map_err(|error| error.to_string())?;
    let key = state
        .provider_directory
        .get("preset:yunmo")
        .ok()
        .map(|route| route.api_key)
        .filter(|key| !key.trim().is_empty())
        .or_else(|| (!cfg.yunmo_api_key.trim().is_empty()).then(|| cfg.yunmo_api_key.clone()))
        .ok_or_else(|| "请先配置云末 AI 独立 Token".to_string())?;
    let catalog = state.provider_catalog.clone();
    let discovered = tokio::task::spawn_blocking(move || {
        let request_cfg = Config {
            base_url: "https://api.yunmo-ai.com/v1".into(),
            provider_protocol: "openai".into(),
            api_key: key,
            ..Config::default()
        };
        catalog.discover_config(&request_cfg)
    })
    .await
    .map_err(|error| format!("云末模型目录任务失败：{error}"))??;
    let model_ids = discovered
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();
    let models = model_ids
        .iter()
        .map(|id| yunmo_model(id))
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("云末模型目录没有返回可用模型".to_string());
    }
    if cfg.base_url.contains("api.yunmo-ai.com") {
        write_available_models(&model_ids)?;
    }
    let mut yunmo = state
        .yunmo_models
        .lock()
        .map_err(|_| "云末模型缓存不可用".to_string())?;
    *yunmo = models;
    let openrouter = state
        .openrouter_models
        .lock()
        .map_err(|_| "OpenRouter 模型缓存不可用".to_string())?;
    Ok(catalog_response(&openrouter, &yunmo, false))
}

/// 选择一个模型预设时，统一保存模型、接口、费用币种和当前厂商的快捷模型。
fn apply_model_preset_for_state(
    provider_id: String,
    model_id: String,
    state: &AppState,
) -> Result<CatalogModel, String> {
    let generation = state.provider_activation.begin()?;
    let cached = state.openrouter_models.lock().map_err(|_| {
        state
            .provider_activation
            .fail(generation, "OpenRouter 模型缓存不可用".to_string())
    })?;
    let yunmo = state.yunmo_models.lock().map_err(|_| {
        state
            .provider_activation
            .fail(generation, "云末模型缓存不可用".to_string())
    })?;
    let preset = if provider_id == "openrouter" {
        cached
            .iter()
            .find(|model| model.model_id == model_id)
            .cloned()
            .or_else(|| find_preset(&provider_id, &model_id))
    } else if provider_id == "yunmo" {
        yunmo
            .iter()
            .find(|model| model.model_id == model_id)
            .cloned()
            .or_else(|| find_preset(&provider_id, &model_id))
    } else {
        find_preset(&provider_id, &model_id)
    }
    .ok_or_else(|| {
        state.provider_activation.fail(
            generation,
            "未找到所选模型预设，请先刷新 OpenRouter 模型目录".to_string(),
        )
    })?;
    let quick_switch_models = provider_models(&provider_id, &cached, &yunmo)
        .into_iter()
        .map(|model| model.model_id)
        .collect::<Vec<_>>();
    drop(cached);
    drop(yunmo);

    state.provider_activation.commit(generation, || {
        write_preset(&state.provider_directory, &preset, &quick_switch_models)
    })?;
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
fn approve_for_thread(
    state: &AppState,
    thread_id: Option<&str>,
    id: u64,
    decision: String,
) -> Result<(), String> {
    let dec = match decision.as_str() {
        "always" => ncx_core::ApprovalDecision::Always,
        "once" | "approve" | "yes" | "true" => ncx_core::ApprovalDecision::Once,
        _ => ncx_core::ApprovalDecision::Deny,
    };
    let mut pending = state.pending.lock().unwrap();
    let expected = thread_id.unwrap_or_default();
    if pending.get(&id).map(|(owner, _)| owner.as_str()) != Some(expected) {
        return Err(format!(
            "approval {id} does not belong to thread {expected}"
        ));
    }
    let sender = pending.remove(&id);
    match sender {
        Some((_, tx)) => tx
            .send(dec)
            .map_err(|_| "approval already resolved".to_string()),
        None => Err(format!("no pending approval with id {id}")),
    }
}

/// Answer or dismiss a pending `ask_user_question` request.
fn answer_for_thread(
    state: &AppState,
    thread_id: Option<&str>,
    id: u64,
    answer: Option<String>,
) -> Result<(), String> {
    let answer = answer.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });
    let mut questions = state.questions.lock().unwrap();
    let expected = thread_id.unwrap_or_default();
    if questions.get(&id).map(|(owner, _)| owner.as_str()) != Some(expected) {
        return Err(format!(
            "question {id} does not belong to thread {expected}"
        ));
    }
    let sender = questions.remove(&id);
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
fn get_checkpoints(
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CheckpointView>, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        Ok(CheckpointStore::new(workspace)
            .list()
            .into_iter()
            .map(checkpoint_view)
            .collect())
    })
}

/// The files captured by a checkpoint (for the checkpoint detail expander).
#[tauri::command]
fn checkpoint_files(
    id: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        CheckpointStore::new(workspace)
            .get(&id)
            .map(|meta| meta.files)
            .ok_or_else(|| format!("no checkpoint with id {id}"))
    })
}

#[tauri::command]
fn create_checkpoint(
    label: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<CheckpointView, String> {
    let label = if label.trim().is_empty() {
        "manual checkpoint"
    } else {
        label.trim()
    };
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        CheckpointStore::new(workspace)
            .create(label)
            .map(checkpoint_view)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
fn restore_checkpoint(
    id: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<RestoreView, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        CheckpointStore::new(workspace)
            .restore(&id)
            .map(restore_view)
            .map_err(|error| error.to_string())
    })
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

/// Run a git command in the already-fenced workspace; Ok(stdout) or Err(stderr).
fn run_git(workspace: &Path, args: &[&str]) -> Result<String, String> {
    let out = ProcessCommand::new("git")
        .args(args)
        .current_dir(workspace)
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
fn git_branches(
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BranchInfo>, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        let current = run_git(workspace, &["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string();
        let listing = run_git(workspace, &["branch", "--format=%(refname:short)"])?;
        Ok(listing
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|name| BranchInfo {
                current: name == current,
                name: name.to_string(),
            })
            .collect())
    })
}

#[tauri::command]
fn git_create_branch(
    name: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        run_git(workspace, &["checkout", "-b", name]).map(|_| ())
    })
}

#[tauri::command]
fn git_switch_branch(
    name: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        run_git(workspace, &["checkout", name]).map(|_| ())
    })
}

#[derive(Serialize)]
pub struct CommitInfo {
    hash: String,
    subject: String,
    when: String,
}

/// Recent commits on a branch (for the branch detail expander).
#[tauri::command]
fn git_log(
    name: String,
    limit: u32,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CommitInfo>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("branch name is required".into());
    }
    let n = format!("-{}", limit.clamp(1, 50));
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        // %h <US> subject <US> relative-date  (0x1f field separator).
        let out = run_git(
            workspace,
            &["log", &n, "--pretty=format:%h\u{1f}%s\u{1f}%cr", name],
        )?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\u{1f}').collect();
                (parts.len() == 3).then(|| CommitInfo {
                    hash: parts[0].to_string(),
                    subject: parts[1].to_string(),
                    when: parts[2].to_string(),
                })
            })
            .collect())
    })
}

/// The working-tree diff vs HEAD (staged + unstaged) for the diff panel.
#[tauri::command]
fn git_diff(
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        let out = run_git(workspace, &["diff", "HEAD"])?;
        Ok(if out.trim().is_empty() {
            "(no changes in the working tree)".into()
        } else {
            out
        })
    })
}

#[derive(Serialize)]
pub struct FileChange {
    path: String,
    added: i64, // -1 = unknown (binary/untracked)
    removed: i64,
    kind: String, // modified | added | deleted | renamed | untracked
}

// The right-side diff panel renders one DOM node per line. Bound both the
// response payload and the line count before crossing the Tauri boundary so a
// generated or minified file cannot freeze the WebView just by being opened.
const MAX_DIFF_PREVIEW_BYTES: usize = 192 * 1024;
const MAX_DIFF_PREVIEW_LINES: usize = 1_000;

#[derive(Serialize)]
struct DiffPreview {
    text: String,
    truncated: bool,
}

fn bounded_diff_preview(text: &str) -> DiffPreview {
    let mut line_end = text.len();
    let mut omitted_only_terminal_newline = false;
    let mut newline_count = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte != b'\n' {
            continue;
        }
        newline_count += 1;
        if newline_count == MAX_DIFF_PREVIEW_LINES {
            // The Svelte renderer splits on newlines, which turns a
            // trailing newline into an additional empty DOM node. Exclude the
            // delimiter itself so every returned preview has at most the
            // advertised number of rendered line nodes.
            line_end = index;
            omitted_only_terminal_newline = index + 1 == text.len();
            break;
        }
    }

    let mut end = line_end.min(MAX_DIFF_PREVIEW_BYTES);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    DiffPreview {
        text: text[..end].to_string(),
        truncated: end < text.len() && !(omitted_only_terminal_newline && end == line_end),
    }
}

/// The raw prefix collected from a child diff stream. The stream reader keeps
/// the first byte/line outside the preview only as a probe, so the caller can
/// distinguish an exact-limit diff from one that must be terminated.
struct BoundedDiffBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Read a bounded child-output prefix without buffering the complete stream.
/// Reaching either limit performs a one-byte probe: EOF means the preview is
/// exact; any byte beyond it makes the caller kill and reap the child.
fn read_bounded_diff_bytes<R: std::io::Read>(
    reader: &mut R,
    byte_limit: usize,
    line_limit: usize,
) -> std::io::Result<BoundedDiffBytes> {
    let mut bytes = Vec::with_capacity(byte_limit);
    let mut buffer = [0_u8; 16 * 1024];
    let mut newline_count = 0;

    if line_limit == 0 || byte_limit == 0 {
        let mut probe = [0_u8; 1];
        return Ok(BoundedDiffBytes {
            bytes,
            truncated: reader.read(&mut probe)? != 0,
        });
    }

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(BoundedDiffBytes {
                bytes,
                truncated: false,
            });
        }

        let remaining = byte_limit.saturating_sub(bytes.len());
        if remaining == 0 {
            let mut probe = [0_u8; 1];
            return Ok(BoundedDiffBytes {
                bytes,
                truncated: reader.read(&mut probe)? != 0,
            });
        }

        let take = read.min(remaining);
        let start = bytes.len();
        bytes.extend_from_slice(&buffer[..take]);
        if let Some(end) = buffer[..take].iter().enumerate().find_map(|(index, byte)| {
            (*byte == b'\n').then(|| {
                newline_count += 1;
                (newline_count == line_limit).then_some(start + index + 1)
            })?
        }) {
            bytes.truncate(end);
            let mut probe = [0_u8; 1];
            return Ok(BoundedDiffBytes {
                bytes,
                truncated: end < start + take || read > take || reader.read(&mut probe)? != 0,
            });
        }

        if read > take {
            return Ok(BoundedDiffBytes {
                bytes,
                truncated: true,
            });
        }
        if bytes.len() == byte_limit {
            let mut probe = [0_u8; 1];
            return Ok(BoundedDiffBytes {
                bytes,
                truncated: reader.read(&mut probe)? != 0,
            });
        }
    }
}

/// Converts a bounded raw prefix to text without manufacturing a replacement
/// character for a UTF-8 scalar split by the byte limit. Actual invalid input
/// remains unreadable/binary and uses the command's normal fallback path.
fn bounded_diff_utf8(bytes: Vec<u8>, truncated: bool) -> Option<String> {
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(error) => {
            let valid_up_to = error.utf8_error().valid_up_to();
            let incomplete_tail = error.utf8_error().error_len().is_none();
            if !truncated || !incomplete_tail {
                return None;
            }
            let mut bytes = error.into_bytes();
            bytes.truncate(valid_up_to);
            String::from_utf8(bytes).ok()
        }
    }
}

/// Read only a bounded prefix from `git diff`. `Command::output()` is unsafe
/// for this panel because it buffers the complete diff before the UI limit is
/// applied; a generated tracked file can otherwise consume unbounded memory.
/// The child is always waited on, including the early-kill path, so Windows
/// does not retain a hidden git process after the panel request returns.
fn bounded_git_diff_preview(workspace: &Path, path: &str) -> Result<Option<DiffPreview>, String> {
    let mut child = ProcessCommand::new("git")
        .args(["diff", "--no-ext-diff", "--no-textconv", "HEAD", "--", path])
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to run git: {error}"))?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("git diff stdout was not captured".to_string());
    };
    let bounded =
        read_bounded_diff_bytes(&mut stdout, MAX_DIFF_PREVIEW_BYTES, MAX_DIFF_PREVIEW_LINES)
            .map_err(|error| format!("failed to read git diff: {error}"));
    drop(stdout);
    let bounded = match bounded {
        Ok(bounded) => bounded,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if bounded.truncated {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for git diff: {error}"))?;
    if !status.success() && !bounded.truncated {
        return Ok(None);
    }
    let Some(text) = bounded_diff_utf8(bounded.bytes, bounded.truncated) else {
        return Ok(None);
    };
    let mut preview = bounded_diff_preview(&text);
    preview.truncated |= bounded.truncated;
    Ok(Some(preview))
}

/// Resolve a diff path only inside the active workspace. Git's `--` argument
/// is safe from option injection, but the untracked-file fallback also opens a
/// filesystem path and must reject absolute paths, `..`, and symlinks escaping
/// the workspace boundary.
fn workspace_relative_file(workspace: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    if !is_workspace_relative_path(relative) {
        return None;
    }
    let workspace = workspace.canonicalize().ok()?;
    let candidate = workspace.join(relative).canonicalize().ok()?;
    candidate.starts_with(&workspace).then_some(candidate)
}

fn is_workspace_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn bounded_untracked_diff_preview(content: &str) -> DiffPreview {
    let mut text = String::new();
    for (lines, line) in content.lines().enumerate() {
        if lines == MAX_DIFF_PREVIEW_LINES {
            return DiffPreview {
                text,
                truncated: true,
            };
        }
        if !text.is_empty() && !append_diff_preview_chunk(&mut text, "\n") {
            return DiffPreview {
                text,
                truncated: true,
            };
        }
        if !append_diff_preview_chunk(&mut text, "+") || !append_diff_preview_chunk(&mut text, line)
        {
            return DiffPreview {
                text,
                truncated: true,
            };
        }
    }
    DiffPreview {
        text,
        truncated: false,
    }
}

fn read_untracked_diff_preview(path: &Path) -> Result<DiffPreview, std::io::Error> {
    let file = fs::File::open(path)?;
    // Read only a few bytes past the budget so we can detect truncation while
    // still trimming an incomplete UTF-8 scalar at the boundary.
    let probe_limit = (MAX_DIFF_PREVIEW_BYTES as u64).saturating_add(4);
    let mut bytes = Vec::with_capacity(probe_limit as usize);
    let mut limited = std::io::Read::take(file, probe_limit);
    std::io::Read::read_to_end(&mut limited, &mut bytes)?;
    let source_was_longer = bytes.len() > MAX_DIFF_PREVIEW_BYTES;
    if source_was_longer {
        bytes.truncate(MAX_DIFF_PREVIEW_BYTES);
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        // A valid UTF-8 file can end inside one four-byte scalar only because
        // the byte probe intentionally stopped at its fixed resource limit.
        Err(error) if source_was_longer && error.valid_up_to() >= bytes.len().saturating_sub(3) => {
            bytes.truncate(error.valid_up_to());
            std::str::from_utf8(&bytes).expect("valid UTF-8 prefix")
        }
        Err(error) => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    };
    let mut preview = bounded_untracked_diff_preview(text);
    preview.truncated |= source_was_longer;
    Ok(preview)
}

/// Appends as much as fits without splitting a UTF-8 scalar. Callers stop on
/// `false`, so even a single minified source line cannot inflate the preview.
fn append_diff_preview_chunk(preview: &mut String, chunk: &str) -> bool {
    let remaining = MAX_DIFF_PREVIEW_BYTES.saturating_sub(preview.len());
    if chunk.len() <= remaining {
        preview.push_str(chunk);
        return true;
    }
    let mut end = remaining;
    while end > 0 && !chunk.is_char_boundary(end) {
        end -= 1;
    }
    preview.push_str(&chunk[..end]);
    false
}

/// The working-tree change set vs HEAD: one entry per changed file with +/-
/// line counts (like the reference's working-tree panel).
#[tauri::command]
fn git_changes(
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileChange>, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        use std::collections::BTreeMap;

        let mut map: BTreeMap<String, FileChange> = BTreeMap::new();
        // Tracked changes vs HEAD: added \t removed \t path.
        if let Ok(numstat) = run_git(workspace, &["diff", "HEAD", "--numstat"]) {
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
        if let Ok(status) = run_git(workspace, &["status", "--porcelain"]) {
            for line in status.lines() {
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
                    .and_modify(|file| file.kind = kind.to_string())
                    .or_insert(FileChange {
                        path,
                        added: -1,
                        removed: -1,
                        kind: kind.to_string(),
                    });
            }
        }
        Ok(map.into_values().collect())
    })
}

/// A bounded preview of the diff for one file (vs HEAD). Untracked files show
/// their content as added lines. Both paths share the same line/byte budget.
#[tauri::command]
fn git_file_diff(
    path: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<DiffPreview, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        if !is_workspace_relative_path(Path::new(&path)) {
            return Ok(bounded_diff_preview(
                "(no textual diff — invalid path outside the workspace)",
            ));
        }
        if let Some(preview) = bounded_git_diff_preview(workspace, &path)? {
            if !preview.text.trim().is_empty() {
                return Ok(preview);
            }
        }
        // Untracked / no tracked diff: show the file content as added lines.
        let Some(file) = workspace_relative_file(workspace, &path) else {
            return Ok(bounded_diff_preview(
                "(no textual diff — binary, unreadable, or outside the workspace)",
            ));
        };
        match read_untracked_diff_preview(&file) {
            Ok(preview) => Ok(preview),
            Err(_) => Ok(bounded_diff_preview(
                "(no textual diff — binary or unreadable)",
            )),
        }
    })
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
fn list_dir(
    rel: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DirEntry>, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        let wsc = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());
        let target = if rel.trim().is_empty() {
            wsc.clone()
        } else {
            wsc.join(&rel)
        };
        let target = target.canonicalize().map_err(|error| error.to_string())?;
        if !target.starts_with(&wsc) {
            return Err("path is outside the workspace".into());
        }
        const SKIP: &[&str] = &[".git", "node_modules", "target", ".nanocodex"];
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&target).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            if SKIP.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            let path = path
                .strip_prefix(&wsc)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(DirEntry { name, path, is_dir });
        }
        out.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
        Ok(out)
    })
}

/// Read a workspace file's text for the file-preview panel. Mirrors `list_dir`'s
/// containment; capped; refuses non-UTF-8 (binary) files.
#[tauri::command]
fn read_workspace_file(
    rel: String,
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        let wsc = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());
        let target = wsc
            .join(&rel)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !target.starts_with(&wsc) {
            return Err("path is outside the workspace".into());
        }
        let meta = std::fs::metadata(&target).map_err(|error| error.to_string())?;
        if meta.len() > 400_000 {
            return Err(format!("文件太大，无法预览（{} KB）", meta.len() / 1024));
        }
        let bytes = std::fs::read(&target).map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|_| "二进制文件，无法预览".to_string())
    })
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

fn validated_local_artifact(path: &str) -> Result<PathBuf, String> {
    let target = PathBuf::from(path.trim());
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "mp4" | "webm" | "mov" | "pdf"
    ) {
        return Err("只允许打开图片、视频或 PDF 产物".to_string());
    }
    let canonical = target
        .canonicalize()
        .map_err(|_| "产物文件不存在或已经移动".to_string())?;
    if !canonical.is_file() {
        return Err("产物路径不是文件".to_string());
    }
    Ok(canonical)
}

fn local_image_preview_data(path: &str) -> Result<String, String> {
    let target = validated_local_artifact(path)?;
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => return Err("该产物不支持会话内图片预览".to_string()),
    };
    let metadata = fs::metadata(&target).map_err(|error| error.to_string())?;
    if metadata.len() > 12 * 1024 * 1024 {
        return Err("图片超过 12 MiB，仅允许点击外部查看".to_string());
    }
    let bytes = fs::read(target).map_err(|error| error.to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn local_artifact_preview(path: String) -> Result<String, String> {
    local_image_preview_data(&path)
}

#[tauri::command]
fn open_local_artifact(path: String) -> Result<(), String> {
    let target = validated_local_artifact(&path)?;
    #[cfg(target_os = "windows")]
    {
        ProcessCommand::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(&target)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("无法打开产物文件: {error}"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        open_file(&target)
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

/// List accumulated learnings (newest first) from an explicit workspace.
fn memory_list_at(workspace: &Path) -> Result<Vec<MemoryNote>, String> {
    let mut entries = MemoryStore::new(workspace.join(".ncx").join("memory")).entries();
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
fn memory_consolidate() -> Result<usize, String> {
    memory_store().consolidate(0.85).map_err(|e| e.to_string())
}

/// Manually record a verified learning into project memory.
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
fn memory_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".ncx").join("memory").join("LEARNINGS.md")
}

/// Open the project memory file in the OS editor (creating it if missing).
#[tauri::command]
fn open_memory_file(
    expected_workspace: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    with_workspace_snapshot(&state, &expected_workspace, |workspace| {
        let path = memory_file_path(workspace);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if !path.exists() {
            std::fs::write(&path, "# Project memory (nanocodex)\n\n")
                .map_err(|error| error.to_string())?;
        }
        open_file(&path)
    })
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

fn get_harness_diagnostics(
    provider_directory: &ProviderDirectoryService,
    provider_activation: &ProviderActivationGate,
) -> Result<serde_json::Value, String> {
    let (cfg, workspace) = configured_workspace()?;
    let alibaba_attachment_parser_enabled = cfg.alibaba_attachment_parser_enabled;
    let runtime = ConfiguredHarnessRuntime::from_config(cfg);
    let tools = runtime.build_tools(
        workspace.clone(),
        RuntimeContextSources::new(String::new(), Vec::new(), String::new()),
        RuntimeHostBindings::default(),
    )?;
    let mut diagnostics =
        serde_json::to_value(tools.harness_diagnostics()).map_err(|error| error.to_string())?;
    let object = diagnostics
        .as_object_mut()
        .ok_or_else(|| "Harness 诊断格式无效".to_string())?;
    object.insert(
        "alibaba_attachment_parser".into(),
        serde_json::Value::Bool(alibaba_attachment_parser_enabled),
    );
    object.insert(
        "image_generation_ready".into(),
        serde_json::Value::Bool(tools.get("generate_image").is_some()),
    );
    object.insert(
        "video_generation_ready".into(),
        serde_json::Value::Bool(tools.get("generate_video").is_some()),
    );
    object.insert(
        "external_tools_ready".into(),
        serde_json::Value::Bool(tools.schemas().iter().any(|schema| {
            schema["function"]["name"]
                .as_str()
                .is_some_and(|name| name.contains("__"))
        })),
    );
    object.insert(
        "provider_route".into(),
        serde_json::to_value(provider_directory.diagnostics()?)
            .map_err(|error| error.to_string())?,
    );
    object.insert(
        "provider_activation".into(),
        serde_json::to_value(provider_activation.diagnostics()?)
            .map_err(|error| error.to_string())?,
    );
    Ok(diagnostics)
}

fn external_plugin_catalog() -> Result<ExternalPluginCatalog, String> {
    let (_, workspace) = configured_workspace()?;
    Ok(ExternalPluginCatalog::new(
        workspace.join(".ncx").join("plugins"),
    ))
}

fn list_external_plugins() -> Result<Vec<ExternalPluginRecord>, String> {
    external_plugin_catalog()?.discover()
}

fn install_external_plugin(source: String, upgrade: bool) -> Result<ExternalPluginRecord, String> {
    let catalog = external_plugin_catalog()?;
    if upgrade {
        catalog.upgrade(Path::new(&source))
    } else {
        catalog.install(Path::new(&source))
    }
}

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
    ui_slots: Vec<DshUiSlotContribution>,
    ui_slot_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct DshUiSlotContribution {
    #[serde(default)]
    plugin: String,
    slot: String,
    id: String,
    label: String,
    #[serde(default)]
    order: i32,
    #[serde(default)]
    description: String,
    #[serde(default)]
    url: Option<String>,
}

impl CodexPluginView {
    fn new(plugin: CodexPluginRecord, app_count: usize) -> Self {
        let (ui_slots, ui_slot_error) = match load_dsh_ui_slots(&plugin) {
            Ok(slots) => (slots, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        Self {
            skill_roots: plugin.skill_paths().len(),
            has_mcp: plugin.manifest.mcp_servers.is_some() || plugin.mcp_path().is_some(),
            has_apps: plugin.manifest.apps.is_some() || plugin.apps_path().is_some(),
            app_count,
            has_hooks: plugin.manifest.hooks.is_some() || plugin.hooks_path().is_some(),
            ui_slots,
            ui_slot_error,
            manifest: plugin.manifest,
            root: plugin.root,
            enabled: plugin.enabled,
        }
    }
}

fn load_dsh_ui_slots(plugin: &CodexPluginRecord) -> Result<Vec<DshUiSlotContribution>, String> {
    let path = plugin.root.join(".ncx/ui-slots.json");
    if !plugin.enabled || !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("UI Slots 声明超过 64 KiB".to_string());
    }
    let mut slots: Vec<DshUiSlotContribution> =
        serde_json::from_slice(&bytes).map_err(|error| format!("UI Slots 声明无效: {error}"))?;
    if slots.len() > 24 {
        return Err("UI Slots 声明超过 24 项".to_string());
    }
    let mut identities = std::collections::HashSet::new();
    for item in &mut slots {
        if !matches!(
            item.slot.as_str(),
            "settings.plugins.tab" | "sidebar.footer.action" | "shell.overlay"
        ) || item.id.is_empty()
            || item.id.len() > 80
            || item.label.is_empty()
            || item.label.len() > 80
            || item
                .url
                .as_deref()
                .is_some_and(|url| !url.starts_with("https://"))
        {
            return Err("UI Slots 声明包含不安全或未支持的字段".to_string());
        }
        if !identities.insert((item.slot.clone(), item.id.clone())) {
            return Err("UI Slots 声明包含重复的 slot/id".to_string());
        }
        item.plugin = plugin.manifest.name.clone();
    }
    slots.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(slots)
}

fn install_codex_plugin(source: String, upgrade: bool) -> Result<CodexPluginRecord, String> {
    let source = Path::new(&source);
    ensure_no_codex_plugin_capability_conflict(source, upgrade)?;
    codex_plugin_catalog()?.install_or_upgrade(source, upgrade)
}

fn set_codex_plugin_enabled(name: String, enabled: bool) -> Result<(), String> {
    let catalog = codex_plugin_catalog()?;
    if enabled {
        let plugin = catalog
            .discover()?
            .into_iter()
            .find(|plugin| plugin.manifest.name == name)
            .ok_or_else(|| format!("插件 '{name}' 尚未安装"))?;
        ensure_no_codex_plugin_capability_conflict_in(&catalog, &plugin.root, true)?;
    }
    catalog.set_enabled(&name, enabled)
}

fn uninstall_codex_plugin(name: String) -> Result<(), String> {
    codex_plugin_catalog()?.uninstall(&name)
}

#[derive(Serialize)]
struct MarketplaceView {
    path: String,
    marketplace: Marketplace,
}

const DSHFIND_ENDPOINT: &str = "https://api.dshfind.com/v1/plugins";
const DSH_1024_ENDPOINT: &str = "https://deepseek1024.com/api/v1/plugins";
const DSH_MARKET_MAX_BYTES: usize = 5 * 1024 * 1024;

fn restricted_market_get(
    url: reqwest::Url,
    expected_origin: &str,
) -> Result<serde_json::Value, String> {
    if url.scheme() != "https" || url.origin().ascii_serialization() != expected_origin {
        return Err("DSH 市场只允许已登记的 HTTPS Origin".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .map_err(|error| format!("DSH 市场请求失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("DSH 市场返回 HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > DSH_MARKET_MAX_BYTES as u64)
    {
        return Err("DSH 市场响应超过 5 MiB 限制".to_string());
    }
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    if bytes.len() > DSH_MARKET_MAX_BYTES {
        return Err("DSH 市场响应超过 5 MiB 限制".to_string());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("DSH 市场响应不是有效 JSON: {error}"))
}

fn dsh_marketplace_search(
    source: &str,
    manifest_url: Option<&str>,
    query: &str,
) -> Result<serde_json::Value, String> {
    let (provider, payload) = match source {
        "dshfind" => {
            let mut url =
                reqwest::Url::parse(DSHFIND_ENDPOINT).map_err(|error| error.to_string())?;
            url.query_pairs_mut()
                .append_pair("page", "1")
                .append_pair("per_page", "100");
            if !query.trim().is_empty() {
                url.query_pairs_mut().append_pair("q", query.trim());
            }
            (
                "dshfind",
                restricted_market_get(url, "https://api.dshfind.com")?,
            )
        }
        "dsh-1024store" => (
            "dsh-1024store",
            restricted_market_get(
                reqwest::Url::parse(DSH_1024_ENDPOINT).map_err(|error| error.to_string())?,
                "https://deepseek1024.com",
            )?,
        ),
        "standard-http" => {
            let manifest_url = manifest_url.ok_or_else(|| "标准市场源缺少清单 URL".to_string())?;
            let manifest_url = reqwest::Url::parse(manifest_url)
                .map_err(|_| "标准市场清单 URL 无效".to_string())?;
            let origin = manifest_url.origin().ascii_serialization();
            let manifest = restricted_market_get(manifest_url, &origin)?;
            let endpoint = manifest
                .pointer("/transport/endpoint")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "标准市场清单缺少 transport.endpoint".to_string())?;
            let mut endpoint =
                reqwest::Url::parse(endpoint).map_err(|_| "标准市场 endpoint 无效".to_string())?;
            if endpoint.origin().ascii_serialization() != origin {
                return Err("标准市场 endpoint 必须与清单同源".to_string());
            }
            if !query.trim().is_empty() {
                endpoint.query_pairs_mut().append_pair("q", query.trim());
            }
            ("standard-http", restricted_market_get(endpoint, &origin)?)
        }
        _ => return Err("未知 DSH 市场源".to_string()),
    };
    let raw_items = payload
        .get("data")
        .or_else(|| payload.get("packages"))
        .or_else(|| payload.get("items"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "DSH 市场响应缺少插件列表".to_string())?;
    let needle = query.trim().to_lowercase();
    let items = raw_items
        .iter()
        .filter_map(|item| normalize_dsh_market_item(provider, item))
        .filter(|item| {
            needle.is_empty()
                || item["name"]
                    .as_str()
                    .is_some_and(|value| value.to_lowercase().contains(&needle))
                || item["summary"]
                    .as_str()
                    .is_some_and(|value| value.to_lowercase().contains(&needle))
        })
        .take(100)
        .collect::<Vec<_>>();
    let categories = normalize_dsh_categories(payload.get("categories"), &items);
    let meta = payload.get("meta").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "total": items.len(),
            "catalogTotal": raw_items.len()
        })
    });
    Ok(serde_json::json!({
        "source": provider,
        "items": items,
        "categories": categories,
        "meta": meta,
        "total": items.len()
    }))
}

fn valid_dsh_category_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn dsh_category_id(raw: &serde_json::Value) -> Option<&str> {
    raw.get("category")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("id").and_then(serde_json::Value::as_str))
        })
        .map(str::trim)
        .filter(|value| valid_dsh_category_id(value))
}

fn normalize_dsh_categories(
    raw: Option<&serde_json::Value>,
    items: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut categories = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(raw) = raw.and_then(serde_json::Value::as_array) {
        for category in raw.iter().take(100) {
            let Some(id) = category
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|id| valid_dsh_category_id(id))
            else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            let en = category
                .get("en")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .chars()
                .take(80)
                .collect::<String>();
            let zh = category
                .get("zh")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&en)
                .chars()
                .take(80)
                .collect::<String>();
            let count = category
                .get("count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            categories.push(serde_json::json!({ "id": id, "en": en, "zh": zh, "count": count }));
        }
    }
    if categories.is_empty() {
        let mut counts = std::collections::BTreeMap::<String, u64>::new();
        for item in items {
            let id = item
                .get("category")
                .and_then(serde_json::Value::as_str)
                .filter(|id| valid_dsh_category_id(id))
                .unwrap_or("unclassified");
            *counts.entry(id.to_string()).or_default() += 1;
        }
        categories.extend(counts.into_iter().map(|(id, count)| {
            let label = if id == "unclassified" {
                "待分类"
            } else {
                id.as_str()
            };
            serde_json::json!({ "id": id, "en": label, "zh": label, "count": count })
        }));
    }
    categories
}

fn normalize_dsh_market_item(provider: &str, raw: &serde_json::Value) -> Option<serde_json::Value> {
    if raw.get("is_risky").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    let id = raw
        .get("full_name")
        .or_else(|| raw.get("id"))?
        .as_str()?
        .trim();
    let name = raw
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(id)
        .trim();
    if id.is_empty() || name.is_empty() || id.len() > 160 || name.len() > 120 {
        return None;
    }
    let summary = raw
        .get("description")
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("zh").and_then(serde_json::Value::as_str))
                .or_else(|| value.get("en").and_then(serde_json::Value::as_str))
        })
        .unwrap_or(name)
        .chars()
        .take(1000)
        .collect::<String>();
    let repository = raw
        .get("repository_url")
        .or_else(|| raw.get("url"))
        .and_then(serde_json::Value::as_str);
    let methods = raw
        .pointer("/install/methods")
        .or_else(|| raw.get("installMethods"))
        .and_then(serde_json::Value::as_array);
    let npm = methods.and_then(|methods| {
        methods.iter().find_map(|method| {
            let kind = method.get("kind")?.as_str()?;
            let verified = method
                .get("verification")
                .and_then(serde_json::Value::as_str)
                == Some("verified");
            let safe_build = method
                .get("requiresBuildAllowance")
                .and_then(serde_json::Value::as_bool)
                != Some(true);
            if kind == "npm" && verified && safe_build {
                Some((
                    method.get("spec")?.as_str()?,
                    method.get("revision")?.as_str()?,
                ))
            } else {
                None
            }
        })
    });
    let (package, version, compatibility, reason) = npm
        .map(|(package, version)| {
            (
                Some(package),
                Some(version),
                "review",
                "需要 Host 核验 NPM 包和运行时依赖",
            )
        })
        .unwrap_or((
            None,
            None,
            "incompatible",
            "市场未提供可验证的精确 NPM 安装目标",
        ));
    let category = dsh_category_id(raw).unwrap_or("unclassified");
    Some(serde_json::json!({
        "source": provider, "id": id, "name": name, "summary": summary,
        "repository": repository, "package": package, "version": version,
        "category": category, "compatibility": compatibility, "compatibilityReason": reason
    }))
}

fn dsh_marketplace_preview(item: &serde_json::Value) -> Result<serde_json::Value, String> {
    let package = item
        .get("package")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "该插件没有可验证的 NPM 包".to_string())?;
    let version = item
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "该插件没有固定版本".to_string())?;
    if !valid_npm_package(package) || version.is_empty() || version.len() > 64 {
        return Err("插件 NPM 身份无效".to_string());
    }
    let registry_url = format!(
        "https://registry.npmjs.org/{}/{version}",
        package.replace('/', "%2F")
    );
    let manifest = restricted_market_get(
        reqwest::Url::parse(&registry_url).map_err(|error| error.to_string())?,
        "https://registry.npmjs.org",
    )?;
    if manifest.get("name").and_then(serde_json::Value::as_str) != Some(package)
        || manifest.get("version").and_then(serde_json::Value::as_str) != Some(version)
    {
        return Err("NPM 返回身份与市场目录不一致".to_string());
    }
    let scripts = manifest
        .get("scripts")
        .and_then(serde_json::Value::as_object);
    let lifecycle = scripts.is_some_and(|scripts| {
        ["preinstall", "install", "postinstall"]
            .iter()
            .any(|name| scripts.contains_key(*name))
    });
    let dependencies = manifest
        .get("dependencies")
        .and_then(serde_json::Value::as_object);
    let dependency_names = dependencies
        .map(|deps| deps.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let uses_slots = dependency_names
        .iter()
        .any(|name| name == "@deepseek-ai/dsh-client-ui-slots");
    let uses_private_runtime = dependency_names.iter().any(|name| {
        name.starts_with("@deepseek-ai/dsh-") && name != "@deepseek-ai/dsh-client-ui-slots"
    });
    let (compatibility, reason) = if lifecycle {
        ("incompatible", "包含 NPM 安装生命周期脚本，禁止自动安装")
    } else if uses_private_runtime {
        ("incompatible", "依赖尚未映射的 DSH 私有运行时")
    } else if uses_slots {
        (
            "ui-adapter",
            "将静态核验并映射设置页、侧栏动作和 Shell Overlay；不会执行第三方 React",
        )
    } else {
        (
            "convertible",
            "未发现动态安装脚本或 DSH 私有运行时，可进入资源转换检查",
        )
    };
    Ok(serde_json::json!({
        "package": package, "version": version, "compatibility": compatibility,
        "compatible": compatibility != "incompatible", "reason": reason,
        "risks": [
            format!("依赖数量：{}", dependency_names.len()),
            if lifecycle { "包含 lifecycle scripts" } else { "未发现 lifecycle scripts" }
        ]
    }))
}

fn install_dsh_marketplace_plugin(
    item: &serde_json::Value,
    upgrade: bool,
) -> Result<CodexPluginRecord, String> {
    let preview = dsh_marketplace_preview(item)?;
    if !matches!(
        preview
            .get("compatibility")
            .and_then(serde_json::Value::as_str),
        Some("convertible" | "ui-adapter")
    ) {
        return Err(preview
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("该 DSH 插件不能转换为 nanocodex 资源插件")
            .to_string());
    }
    let package = item
        .get("package")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "缺少 NPM 包名".to_string())?;
    let version = item
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "缺少 NPM 版本".to_string())?;
    let plugin_name = item
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(package);
    let marketplace_plugin = MarketplacePlugin {
        name: plugin_name.to_string(),
        source: MarketplaceSource::Npm {
            package: package.to_string(),
            version: Some(version.to_string()),
            registry: Some("https://registry.npmjs.org".to_string()),
        },
    };
    let (_, workspace) = configured_workspace()?;
    let (source, cleanup) = materialize_marketplace_plugin(
        &workspace,
        Path::new("marketplace.json"),
        &marketplace_plugin,
    )?;
    let result = prepare_dsh_portable_plugin(&source, package, version)
        .and_then(|_| ensure_no_codex_plugin_capability_conflict(&source, upgrade))
        .and_then(|_| codex_plugin_catalog()?.install_or_upgrade(&source, upgrade));
    if let Some(cleanup) = cleanup {
        let _ = remove_plugin_staging(&workspace, &cleanup);
    }
    result
}

fn codex_plugin_resource_ids(root: &Path) -> Result<std::collections::BTreeSet<String>, String> {
    let mut ids = std::collections::BTreeSet::new();
    let manifest_path = root.join(".codex-plugin/plugin.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|_| "插件缺少 .codex-plugin/plugin.json".to_string())?,
    )
    .map_err(|error| format!("插件清单无效: {error}"))?;
    if let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) {
        ids.insert(format!("plugin:{name}"));
    }
    let skills = root.join("skills");
    if skills.is_dir() {
        for entry in fs::read_dir(skills).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.path().join("SKILL.md").is_file() {
                ids.insert(format!(
                    "skill:{}",
                    entry.file_name().to_string_lossy().to_lowercase()
                ));
            }
        }
    }
    let mcp_path = root.join(".mcp.json");
    if mcp_path.is_file() {
        let mcp: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(mcp_path).map_err(|error| error.to_string())?)
                .map_err(|error| format!("MCP 配置无效: {error}"))?;
        if let Some(servers) = mcp.get("mcpServers").and_then(serde_json::Value::as_object) {
            ids.extend(
                servers
                    .keys()
                    .map(|name| format!("mcp:{}", name.to_lowercase())),
            );
        }
    }
    Ok(ids)
}

fn ensure_no_codex_plugin_capability_conflict(source: &Path, upgrade: bool) -> Result<(), String> {
    ensure_no_codex_plugin_capability_conflict_in(&codex_plugin_catalog()?, source, upgrade)
}

fn ensure_no_codex_plugin_capability_conflict_in(
    catalog: &CodexPluginCatalog,
    source: &Path,
    upgrade: bool,
) -> Result<(), String> {
    let incoming = codex_plugin_resource_ids(source)?;
    let incoming_name = incoming
        .iter()
        .find(|id| id.starts_with("plugin:"))
        .cloned();
    for installed in catalog
        .discover()?
        .into_iter()
        .filter(|plugin| plugin.enabled)
    {
        let installed_ids = codex_plugin_resource_ids(&installed.root)?;
        if upgrade
            && incoming_name
                .as_ref()
                .is_some_and(|name| installed_ids.contains(name))
        {
            continue;
        }
        let conflicts = incoming
            .intersection(&installed_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !conflicts.is_empty() {
            return Err(format!(
                "安装已阻断：插件 {} 与已启用插件 {} 存在重复能力：{}。请先停用或卸载冲突插件。",
                incoming_name
                    .as_deref()
                    .unwrap_or("未知插件")
                    .trim_start_matches("plugin:"),
                installed.manifest.name,
                conflicts.join("、")
            ));
        }
    }
    Ok(())
}

fn prepare_dsh_portable_plugin(source: &Path, package: &str, version: &str) -> Result<(), String> {
    let package_json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(source.join("package.json"))
            .map_err(|_| "NPM 包缺少 package.json".to_string())?,
    )
    .map_err(|error| format!("NPM package.json 无效: {error}"))?;
    if package_json.get("name").and_then(serde_json::Value::as_str) != Some(package)
        || package_json
            .get("version")
            .and_then(serde_json::Value::as_str)
            != Some(version)
    {
        return Err("解包后的 NPM 身份与风险预览不一致".to_string());
    }
    if source.join(".codex-plugin/plugin.json").is_file() {
        return Ok(());
    }
    let has_skills = source.join("skills").is_dir();
    let has_mcp = source.join(".mcp.json").is_file();
    let command_dirs = [source.join("commands"), source.join(".cursor/commands")];
    let command_files = command_dirs
        .iter()
        .filter(|dir| dir.is_dir())
        .flat_map(|dir| {
            fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    let ui_slots = extract_dsh_ui_slots(source, &package_json)?;
    if !has_skills && !has_mcp && command_files.is_empty() && ui_slots.is_empty() {
        return Err("该包不包含可转换资源，或使用了尚未支持的 DSH UI Slot".to_string());
    }
    if !command_files.is_empty() {
        let generated = source.join("skills");
        fs::create_dir_all(&generated).map_err(|error| error.to_string())?;
        for command in command_files {
            let stem = command
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("command");
            let safe = stem
                .chars()
                .map(|value| {
                    if value.is_ascii_alphanumeric() || value == '-' {
                        value
                    } else {
                        '-'
                    }
                })
                .collect::<String>();
            let skill_dir = generated.join(format!("dsh-command-{safe}"));
            fs::create_dir_all(&skill_dir).map_err(|error| error.to_string())?;
            let body = fs::read_to_string(&command).map_err(|error| error.to_string())?;
            let content = if body.trim_start().starts_with("---") {
                body
            } else {
                format!("---\nname: dsh-command-{safe}\ndescription: Converted DSH command {stem}\n---\n\n{body}")
            };
            fs::write(skill_dir.join("SKILL.md"), content).map_err(|error| error.to_string())?;
        }
    }
    let manifest_dir = source.join(".codex-plugin");
    fs::create_dir_all(&manifest_dir).map_err(|error| error.to_string())?;
    let mut manifest = serde_json::json!({
        "name": package.replace(['@', '/'], "-"),
        "version": version,
        "description": "Converted from a verified DSH Marketplace package",
        "keywords": ["dsh-marketplace", "converted"]
    });
    let fields = manifest.as_object_mut().expect("manifest object");
    if source.join("skills").is_dir() {
        fields.insert("skills".into(), serde_json::json!("./skills"));
    }
    if has_mcp {
        fields.insert("mcpServers".into(), serde_json::json!("./.mcp.json"));
    }
    if !ui_slots.is_empty() {
        let ui_dir = source.join(".ncx");
        fs::create_dir_all(&ui_dir).map_err(|error| error.to_string())?;
        fs::write(
            ui_dir.join("ui-slots.json"),
            serde_json::to_vec_pretty(&ui_slots).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fields.insert(
            "interface".into(),
            serde_json::json!({ "dshUiSlots": "./.ncx/ui-slots.json" }),
        );
    }
    fs::write(
        manifest_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn extract_dsh_ui_slots(
    source: &Path,
    package_json: &serde_json::Value,
) -> Result<Vec<DshUiSlotContribution>, String> {
    const SUPPORTED: [&str; 3] = [
        "settings.plugins.tab",
        "sidebar.footer.action",
        "shell.overlay",
    ];
    let mut found = std::collections::BTreeSet::new();
    let mut pending = vec![source.to_path_buf()];
    let mut scanned = 0usize;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| matches!(name, "node_modules" | ".git" | "target"))
                {
                    continue;
                }
                pending.push(path);
                continue;
            }
            if !path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|ext| matches!(ext, "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx"))
            {
                continue;
            }
            let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
            if metadata.len() > 512 * 1024 {
                continue;
            }
            scanned += metadata.len() as usize;
            if scanned > 4 * 1024 * 1024 {
                return Err("DSH UI 静态检查超过 4 MiB 限制".to_string());
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            for slot in SUPPORTED {
                if text.contains(slot) {
                    found.insert(slot);
                }
            }
        }
    }
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let label = package_json
        .get("displayName")
        .or_else(|| package_json.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("DSH 插件")
        .to_string();
    let description = package_json
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("由 DSH UI Slots 安全适配")
        .to_string();
    let url = package_json
        .get("homepage")
        .and_then(serde_json::Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_string);
    let id = package_json["name"]
        .as_str()
        .unwrap_or("dsh-plugin")
        .replace(['@', '/'], "-");
    Ok(found
        .into_iter()
        .map(|slot| DshUiSlotContribution {
            plugin: String::new(),
            slot: slot.to_string(),
            id: id.clone(),
            label: label.clone(),
            order: if slot == "settings.plugins.tab" {
                20
            } else {
                10
            },
            description: description.clone(),
            url: url.clone(),
        })
        .collect())
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
    let result = ensure_no_codex_plugin_capability_conflict(&source, upgrade)
        .and_then(|_| codex_plugin_catalog()?.install_or_upgrade(&source, upgrade));
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
                harness_profile: "full".into(),
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
                model: None,
                confirmed_model: None,
            });
        }
        turns.push(Turn {
            id: TurnId::new(format!("legacy-turn-{index}")).expect("non-empty legacy turn id"),
            status: TurnStatus::Completed,
            execution_mode: ncx_protocol::ExecutionMode::Agent,
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
    let running: RunningSessions = Arc::new(Mutex::new(HashMap::new()));
    let running_for_worker = running.clone();
    let deferred_prompts: DeferredPrompts = Arc::new(Mutex::new(HashMap::new()));
    let deferred_prompts_for_worker = deferred_prompts.clone();
    let worker_lifecycle = Arc::new(WorkerLifecycle::default());
    let worker_lifecycle_for_setup = worker_lifecycle.clone();
    let session_grants: GrantRegistry = Arc::new(Mutex::new(HashMap::new()));
    let session_index = Arc::new(Mutex::new(SessionIndex::default()));
    let thread_store = Arc::new(
        JsonThreadStore::open(default_thread_store_path())
            .expect("open the versioned nanocodex thread store"),
    );
    let app_server = Arc::new(AppServer::new(thread_store, now_epoch_millis));
    let provider_directory = ProviderDirectoryService::default();
    let provider_catalog = ProviderCatalogService::default();
    let provider_chat_probe = ProviderChatProbeService::default();
    let runtime_activation = Arc::new(RuntimeActivationCoordinator::default());
    let runtime_activation_for_worker = runtime_activation.clone();
    if let Ok(index) = session_index.lock() {
        if let Err(error) = migrate_legacy_threads(&index, &app_server) {
            eprintln!("thread migration: {error}");
        }
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            tx,
            pending,
            questions,
            question_counter: AtomicU64::new(1_000_000),
            cancels,
            running,
            deferred_prompts,
            worker_lifecycle,
            app_server: app_server.clone(),
            provider_directory,
            provider_catalog,
            provider_chat_probe,
            provider_activation: ProviderActivationGate::default(),
            runtime_activation,
            runtime_handoff_gate: RuntimeHandoffGate::default(),
            openrouter_models: Mutex::new(Vec::new()),
            yunmo_models: Mutex::new(Vec::new()),
            workspace_gate: Mutex::new(()),
            memory_merge: Arc::new(memory_merge_job::MemoryMergeCoordinator::default()),
            forge_job: Arc::new(forge_job::ForgeJobCoordinator::default()),
        })
        .setup(move |app| {
            // Restore the saved workspace only after AppState exists, so startup
            // uses the same gated transition as every later workspace change.
            if let Some(saved_workspace) = bridge::load_last_workspace() {
                let state = app.state::<AppState>();
                if let Err(error) = transition_workspace_for_state(&saved_workspace, &state) {
                    eprintln!("restore GUI workspace: {error}");
                }
            }
            let startup_workspace = std::env::current_dir().unwrap_or_default();
            if let (Some(window), Some(icon)) = (
                app.get_webview_window("main"),
                app.default_window_icon().cloned(),
            ) {
                window.set_icon(icon)?;
            }
            // Hand the agent thread an AppHandle (to emit events), the receiver
            // (to take prompts), and the shared pending-approvals map.
            spawn_worker(WorkerStartup {
                app: app.handle().clone(),
                app_server: app_server.clone(),
                rx,
                pending: pending_for_worker,
                questions: questions_for_worker,
                cancels: cancels_for_worker,
                running: running_for_worker,
                deferred_prompts: deferred_prompts_for_worker,
                lifecycle: worker_lifecycle_for_setup,
                session_grants,
                runtime_activation: runtime_activation_for_worker,
                startup_workspace,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_server_request,
            e2e_ask_question,
            refresh_openrouter_models,
            refresh_yunmo_models,
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
            open_local_artifact,
            local_artifact_preview,
            list_mcp,
            save_temp_image,
            open_session_log,
            open_session_snapshot,
            get_custom_commands,
            expand_custom_command,
            open_memory_file,
            get_workspace,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the nanocodex GUI");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = handle.state::<AppState>();
            state.worker_lifecycle.shutdown_and_join(
                &state.tx,
                &state.cancels,
                &state.pending,
                &state.questions,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ncx-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn runtime_activation_reports_worker_disconnect_without_calling_it_a_timeout() {
        let (sender, receiver) = mpsc::channel::<Result<(), String>>();
        drop(sender);
        let coordinator = RuntimeActivationCoordinator::default();
        let activation = coordinator.begin();

        let error =
            await_runtime_activation(receiver, &coordinator, activation.clone(), "timed out")
                .unwrap_err();
        assert_eq!(error, "后台 Agent 在完成初始化前停止响应");
        assert!(activation.is_aborted());
    }

    #[test]
    fn runtime_activation_coordinator_invalidates_only_pending_work() {
        let coordinator = RuntimeActivationCoordinator::default();
        let older = coordinator.begin();
        let newer = coordinator.begin();
        assert!(older.is_aborted());
        assert!(!coordinator.can_proceed(&older));
        assert!(coordinator.is_current(&newer));
        assert!(!coordinator.accept_if_current(&older));
        assert!(coordinator.accept_if_current(&newer));

        // A completed handoff stays accepted when a later command begins; it
        // simply stops being eligible for any additional global writes.
        let accepted = coordinator.begin();
        assert!(coordinator.accept_if_current(&accepted));
        let replacement = coordinator.begin();
        assert!(accepted.is_accepted());
        assert!(!coordinator.abort_if_pending(&accepted));
        assert!(!coordinator.can_proceed(&accepted));
        assert!(coordinator.is_current(&replacement));
    }

    #[test]
    fn accepted_runtime_activation_survives_a_late_receiver_disconnect() {
        let (sender, receiver) = mpsc::channel::<Result<(), String>>();
        drop(sender);
        let coordinator = RuntimeActivationCoordinator::default();
        let activation = coordinator.begin();
        assert!(coordinator.accept_if_current(&activation));

        assert!(
            await_runtime_activation(receiver, &coordinator, activation.clone(), "timed out",)
                .is_ok()
        );
        assert!(activation.is_accepted());
    }

    #[test]
    fn runtime_activation_timeout_cannot_abort_a_newer_token() {
        let coordinator = RuntimeActivationCoordinator::default();
        let older = coordinator.begin();
        let newer = coordinator.begin();
        assert!(!coordinator.abort_if_pending(&older));
        assert!(coordinator.is_current(&newer));
        assert!(!newer.is_aborted());
    }

    #[test]
    fn failed_handoff_rollback_only_runs_for_the_current_unaccepted_activation() {
        let coordinator = RuntimeActivationCoordinator::default();
        let failed = coordinator.begin();
        let mut rollbacks = 0;

        // The failed caller atomically fences its worker before compensating.
        // This is the same gate that protects the real process-CWD restore,
        // but uses a local counter so the test is independent of wall clocks
        // and the test process's global CWD.
        assert_eq!(
            run_failed_handoff_rollback(&coordinator, &failed, || {
                rollbacks += 1;
                "restored"
            }),
            Some("restored")
        );
        assert_eq!(rollbacks, 1);
        assert!(failed.is_aborted());

        let accepted = coordinator.begin();
        assert!(coordinator.accept_if_current(&accepted));
        assert_eq!(
            run_failed_handoff_rollback(&coordinator, &accepted, || {
                rollbacks += 1;
                "must not restore an accepted handoff"
            }),
            None
        );
        assert_eq!(rollbacks, 1);

        let stale = coordinator.begin();
        let newer = coordinator.begin();
        assert_eq!(
            run_failed_handoff_rollback(&coordinator, &stale, || {
                rollbacks += 1;
                "must not restore over a newer handoff"
            }),
            None
        );
        assert_eq!(rollbacks, 1);
        assert!(coordinator.is_current(&newer));
        assert!(!newer.is_aborted());
    }

    #[test]
    fn workspace_snapshot_match_is_fail_closed() {
        let root = unique_test_dir("memory-merge-workspace");
        let other = unique_test_dir("memory-merge-other");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&other).unwrap();
        let expected = root.to_string_lossy().to_string();
        assert!(workspace_matches(&expected, &root));
        assert!(!workspace_matches(&other.to_string_lossy(), &root));
        assert!(!memory_merge_cancellation_required_for_workspace_transition(&root, &root));
        assert!(memory_merge_cancellation_required_for_workspace_transition(
            &other, &root
        ));
        assert!(!workspace_matches("", &root));
        assert!(require_workspace_match(&expected, &root).is_ok());
        let error = require_workspace_match(&other.to_string_lossy(), &root).unwrap_err();
        assert!(error.contains("工作区已切换"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(other);
    }

    #[test]
    fn invalid_orchestrator_budget_never_changes_the_config_file() {
        let root = unique_test_dir("orchestrator-budget-invalid");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let original = b"model = \"known-model\"\norchestrator_workers = 2\n";
        fs::write(&path, original).unwrap();

        for invalid in ["9", "NaN"] {
            let updates = HashMap::from([
                ("model".into(), "must-not-be-written".into()),
                ("orchestrator_workers".into(), invalid.into()),
            ]);
            let error = persist_validated_settings(&updates, &path).unwrap_err();
            assert!(error.contains("普通任务 Worker"));
            assert_eq!(fs::read(&path).unwrap(), original);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_general_settings_never_change_the_config_file() {
        let root = unique_test_dir("general-settings-invalid");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        let original = b"model = \"known-model\"\nprice_in = 1.0\n";
        fs::write(&path, original).unwrap();

        for (key, value) in [
            ("max_iterations", "0"),
            ("context_edit_enabled", "yes"),
            ("sandbox_mode", "unrestricted"),
            ("price_in", "NaN"),
            ("price_currency", "EUR"),
            ("model", "bad model"),
            ("base_url", "file:///tmp/endpoint"),
        ] {
            let updates = HashMap::from([
                ("model".into(), "must-not-be-written".into()),
                (key.into(), value.into()),
            ]);
            assert!(
                persist_validated_settings(&updates, &path).is_err(),
                "{key}={value} should fail"
            );
            assert_eq!(fs::read(&path).unwrap(), original);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn orchestrator_budget_boundaries_are_persisted_together() {
        let root = unique_test_dir("orchestrator-budget-boundaries");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.toml");
        fs::write(&path, b"model = \"known-model\"\n").unwrap();
        let updates = HashMap::from([
            ("orchestrator_workers".into(), "4".into()),
            ("orchestrator_high_workers".into(), "1".into()),
            ("orchestrator_verify_retries".into(), "0".into()),
            ("orchestrator_max_depth".into(), "2".into()),
            ("orchestrator_max_subtasks".into(), "12".into()),
        ]);

        persist_validated_settings(&updates, &path).unwrap();
        let persisted = fs::read_to_string(&path).unwrap();
        for (key, expected) in [
            ("orchestrator_workers", "4"),
            ("orchestrator_high_workers", "1"),
            ("orchestrator_verify_retries", "0"),
            ("orchestrator_max_depth", "2"),
            ("orchestrator_max_subtasks", "12"),
        ] {
            assert!(
                persisted.contains(&format!("{key} = \"{expected}\"")),
                "missing {key}: {persisted}"
            );
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_artifact_opening_allows_media_and_rejects_executables() {
        let root = std::env::temp_dir().join(format!("ncx-local-artifact-{}", now_epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        let image = root.join("preview.png");
        let executable = root.join("unsafe.exe");
        fs::write(&image, b"image").unwrap();
        fs::write(&executable, b"binary").unwrap();
        assert_eq!(
            validated_local_artifact(image.to_str().unwrap()).unwrap(),
            image.canonicalize().unwrap()
        );
        assert!(validated_local_artifact(executable.to_str().unwrap())
            .unwrap_err()
            .contains("只允许"));
        assert!(
            validated_local_artifact(root.join("missing.png").to_str().unwrap())
                .unwrap_err()
                .contains("不存在")
        );
        assert!(local_image_preview_data(image.to_str().unwrap())
            .unwrap()
            .starts_with("data:image/png;base64,"));
        let _ = fs::remove_dir_all(root);
    }
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
                harness_profile: "full".into(),
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
        let cfg = Config {
            vl_model: "qwen3.7-plus".into(),
            vl_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".into(),
            vl_api_key: "secret-vision-key".into(),
            dashscope_token_plan_key: "sk-sp-secret-plan".into(),
            dashscope_workspace_key: "sk-ws-secret-workspace".into(),
            ..Config::default()
        };

        let settings = settings_from_config(&cfg);

        assert_eq!(settings.vl_model, "qwen3.7-plus");
        assert_eq!(
            settings.vl_base_url,
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert!(settings.has_vl_api_key);
        assert_ne!(settings.vl_api_key_masked, cfg.vl_api_key);
        assert!(settings.vl_api_key_masked.starts_with("****"));
        assert!(settings.has_dashscope_token_plan_key);
        assert!(settings.has_dashscope_workspace_key);
        assert_ne!(
            settings.dashscope_token_plan_key_masked,
            cfg.dashscope_token_plan_key
        );
        assert_ne!(
            settings.dashscope_workspace_key_masked,
            cfg.dashscope_workspace_key
        );
    }

    #[test]
    fn image_attachment_requires_an_explicit_parser_model() {
        assert!(validate_image_attachment_route(&[], "text-only", false, "").is_ok());
        assert!(
            validate_image_attachment_route(&["test.png".into()], "gpt-5.6-sol", false, "").is_ok()
        );
        assert!(validate_image_attachment_route(
            &["test.png".into()],
            "text-only",
            true,
            "qwen3.7-plus"
        )
        .is_ok());

        let error = validate_image_attachment_route(&["test.png".into()], "text-only", false, "")
            .unwrap_err();
        assert!(error.contains("设置 → 插件"));
        assert!(error.contains("设置"));
    }

    #[test]
    fn topbar_exposes_reasoning_effort_quick_switch() {
        let composer = include_str!("../../src/components/Composer.svelte");
        let controls = include_str!("../../src/lib/model-controls-controller.svelte.ts");
        assert!(composer.contains("class=\"reasoning-pill\""));
        assert!(!composer.contains("disabled={busy} title=\"切换 DeepSeek 思考模式\""));
        assert!(composer.contains("当前运行不变，可选择下次会话使用的思考级别"));
        assert!(controls.contains("思考程度"));
        assert!(controls.contains("selectReasoningEffort"));
        assert!(controls.contains("当前运行不变，下次会话生效"));
        assert!(controls.contains("reasoningEffortsForModel"));
        assert!(controls.contains("gpt-5\\.6-(sol|terra)"));
        assert!(controls.contains("DeepSeek max"));
        assert!(controls.contains("OpenAI ${value} reasoning effort"));
        assert!(!controls.contains("{ id: \"medium\", label:"));
    }

    #[test]
    fn compact_session_controls_keep_only_frequent_actions_in_the_composer() {
        let topbar = include_str!("../../src/components/TopBar.svelte")
            .split_once("<header class=\"topbar\">")
            .unwrap()
            .1
            .split_once("</header>")
            .unwrap()
            .0;
        let composer = include_str!("../../src/components/Composer.svelte")
            .split_once("<div class=\"composer-shell\">")
            .unwrap()
            .1
            .split_once("</footer>")
            .unwrap()
            .0;
        assert!(composer.contains("<div class=\"composer-meta\">"));
        assert!(!topbar.contains("model-wrap"));
        assert!(!topbar.contains("reasoning-wrap"));
        assert!(!topbar.contains("ws-pill"));
        assert!(composer.contains("model-wrap"));
        assert!(composer.contains("reasoning-wrap"));
        assert!(composer.contains("ws-pill"));
        assert!(composer.contains("approval-wrap"));
        assert!(!composer.contains("execution-wrap"));
        assert!(!composer.contains("profile-wrap"));

        let settings = include_str!("../../src/components/SettingsModal.svelte");
        assert!(settings.contains("Agent / 编排模式"));
        assert!(settings.contains("当前会话 Harness"));
        assert!(settings.contains("当前会话下一轮生效"));
        assert!(settings.contains("disabled={harnessProfileLocked}"));

        let css = include_str!("../../src/app.css");
        assert!(css.contains(".composer-meta .approval-wrap { order: 1; }"));
        assert!(css.contains(".composer-meta .model-wrap { order: 2; }"));
        assert!(css.contains(".composer-meta .reasoning-wrap { order: 3; }"));
        assert!(css.contains(".composer-spacer { order: 5; flex: 1 1 auto; }"));
        assert!(css.contains(".composer-underbar .ws-pill"));
        assert!(css.contains("position: relative; min-height: 2rem"));
        assert!(css.contains("--accent:       #0a84ff"));
        assert!(css.contains("backdrop-filter: blur(28px)"));
        assert!(css
            .contains(".menu-backdrop:hover, .menu-backdrop:active, .menu-backdrop:focus-visible"));
    }

    #[test]
    fn durable_goal_controls_are_visible_explicit_and_cost_gated() {
        let app = include_str!("../../src/App.svelte");
        let composer = include_str!("../../src/components/Composer.svelte");
        let controller = include_str!("../../src/lib/goal-controller.svelte.ts");
        let protocol = include_str!("../../src/lib/app-server-client.ts");
        let css = include_str!("../../src/app.css");
        assert!(app.contains("new GoalController(thread)"));
        assert!(app.contains("observedGoalBusy && !busy"));
        assert!(composer.contains("目标：{goalStatusLabel}"));
        assert!(composer.contains("自动续轮："));
        assert!(composer.contains("剩余上限：{goalRemainingRounds} 轮"));
        assert!(composer.contains("暂停自动续轮"));
        assert!(composer.contains("确认并继续"));
        assert!(controller.contains("window.confirm("));
        assert!(controller.contains("可能产生模型费用"));
        assert!(
            controller.contains("goal: { id: current.goal.id, revision: current.goal.revision }")
        );
        assert!(controller.contains("await this.refresh(threadId)"));
        assert!(protocol.contains("type ProtocolGoalView"));
        assert!(css.contains(".goal-pill.armed"));
        assert!(css.contains(".goal-menu"));
    }

    #[test]
    fn human_prompt_is_atomically_deferred_only_for_a_running_goal() {
        let running: RunningSessions = Arc::new(Mutex::new(HashMap::from([(
            "goal-thread".to_string(),
            SessionRunKind::Goal,
        )])));
        let deferred: DeferredPrompts = Arc::new(Mutex::new(HashMap::new()));
        let prompt = DeferredPrompt {
            text: "新的用户要求".into(),
            images: Vec::new(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        };

        assert!(defer_prompt_for_goal(&running, &deferred, "goal-thread", prompt.clone()).unwrap());
        assert_eq!(deferred.lock().unwrap()["goal-thread"].text, "新的用户要求");
        assert!(defer_prompt_for_goal(&running, &deferred, "goal-thread", prompt.clone()).is_err());
        assert!(!defer_prompt_for_goal(&running, &deferred, "human-thread", prompt).unwrap());
    }

    #[test]
    fn completed_tool_activity_is_hidden_from_final_and_history_views() {
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let model = include_str!("../../src/lib/conversation-model.ts");
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let bridge = include_str!("bridge.rs");
        assert!(thread.contains("role: \"tool_group\""));
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
        assert!(thread.matches("hideCompletedToolActivity(").count() >= 3);
        assert!(thread.contains("this.captureTrajectory(event.session_id);"));
        assert!(thread.contains("this.clone(this.messages.slice(start))"));
        assert!(thread.contains("this.trajectoryBySession.set(sessionId"));
        let app = include_str!("../../src/App.svelte");
        assert!(app.contains(
            "activeView === \"trajectory\" ? thread.trajectoryMessages : thread.messages"
        ));
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
    fn dsh_conversation_details_remain_functional() {
        let app = include_str!("../../src/App.svelte");
        let topbar = include_str!("../../src/components/TopBar.svelte");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let composer = include_str!("../../src/components/Composer.svelte");

        assert!(app.contains("bind:activeView"));
        assert!(topbar.contains("activeView = \"trajectory\""));
        assert!(conversation.contains("trajectory-view"));
        assert!(conversation.contains("copyMessage(message.text, index)"));
        assert!(conversation.contains("toggleFeedback(index, \"good\")"));
        assert!(conversation.contains("forkCurrent"));
        assert!(composer.contains("{turnCount} 轮 · {stepCount} 步"));
        assert!(composer.contains("累计 ≈"));
    }

    #[test]
    fn dsh_session_naming_and_row_actions_remain_functional() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let sidebar = include_str!("../../src/components/SessionSidebar.svelte");
        let protocol = include_str!("../../src/lib/app-server-client.ts");

        assert!(lifecycle.contains("rename = async"));
        assert!(lifecycle.contains("method: \"threadRename\""));
        assert!(lifecycle.contains("nextForkTitle"));
        assert!(lifecycle.contains("`${base} (${index})`"));
        assert!(sidebar.contains("会话名称"));
        assert!(sidebar.contains("role=\"menuitem\""));
        assert!(sidebar.contains("归档会话"));
        assert!(protocol.contains("historicalFallbackTitle(firstUserMessage)"));
        assert!(protocol.contains("if (!firstUserMessage) firstUserMessage = item.text"));
    }

    #[test]
    fn model_reasoning_is_visible_separately_from_tool_activity() {
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let css = include_str!("../../src/app.css");
        let bridge = include_str!("bridge.rs");
        let core = include_str!("../../../crates/ncx-core/src/agent_loop.rs");
        let provider = include_str!("../../../crates/ncx-provider/src/api.rs");

        assert!(provider.contains("StreamDelta::Reasoning"));
        assert!(core.contains("ReasoningDelta(String)"));
        assert!(bridge.contains("UiEvent::ReasoningDelta"));
        assert!(thread.contains("case \"reasoning_delta\":"));
        assert!(thread.contains("role: \"reasoning\""));
        assert!(conversation.contains("思考过程"));
        assert!(css.contains(".reasoning-run"));
    }

    #[test]
    fn composer_occupies_layout_space_instead_of_covering_conversation() {
        let css = include_str!("../../src/app.css");
        assert!(css.contains("position: relative; z-index: 5; flex: 0 0 auto"));
        assert!(css.contains("padding: 1.6rem 3rem 2rem"));
        assert!(!css.contains("padding: 1.6rem 3rem 9.5rem"));
    }

    #[test]
    fn settings_center_keeps_features_in_separate_navigable_sections() {
        let settings = include_str!("../../src/components/SettingsModal.svelte");
        let css = include_str!("../../src/app.css");
        for section in ["通用", "模型与费用", "连接与媒体", "上下文", "插件"] {
            assert!(settings.contains(section));
        }
        assert!(settings.contains("activeSection === \"general\""));
        assert!(settings.contains("activeSection === \"models\""));
        assert!(settings.contains("activeSection === \"connection\""));
        assert!(settings.contains("activeSection === \"context\""));
        assert!(settings.contains("class=\"settings-footer\""));
        assert!(css.contains("grid-template-columns: 15rem minmax(0, 1fr)"));
        assert!(css.contains(".settings-content"));
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
    fn dsh_market_normalization_hides_risky_and_unverified_install_targets() {
        let risky = serde_json::json!({"full_name":"owner/risky","name":"risky","repository_url":"https://github.com/owner/risky","is_risky":true});
        assert!(normalize_dsh_market_item("dshfind", &risky).is_none());

        let unverified = serde_json::json!({"full_name":"owner/demo","name":"demo","repository_url":"https://github.com/owner/demo"});
        let item = normalize_dsh_market_item("dshfind", &unverified).unwrap();
        assert_eq!(item["compatibility"], "incompatible");
        assert!(item["package"].is_null());
    }

    #[test]
    fn dsh_market_normalization_exposes_only_verified_exact_npm_targets() {
        let raw = serde_json::json!({
            "full_name":"owner/demo",
            "name":"demo",
            "repository_url":"https://github.com/owner/demo",
            "install": {"methods": [{
                "kind":"npm", "verification":"verified", "requiresBuildAllowance":false,
                "spec":"dsh-plugin-demo", "revision":"1.2.3"
            }]}
        });
        let item = normalize_dsh_market_item("dshfind", &raw).unwrap();
        assert_eq!(item["compatibility"], "review");
        assert_eq!(item["package"], "dsh-plugin-demo");
        assert_eq!(item["version"], "1.2.3");
    }

    #[test]
    fn dsh_market_categories_are_preserved_deduplicated_and_bounded() {
        let items = vec![
            serde_json::json!({"category":"memory"}),
            serde_json::json!({"category":"memory"}),
        ];
        let raw = serde_json::json!([
            {"id":"memory","en":"Memory","zh":"记忆","count":23},
            {"id":"memory","en":"Duplicate","zh":"重复","count":99},
            {"id":"../unsafe","en":"Unsafe","zh":"不安全","count":1}
        ]);
        let categories = normalize_dsh_categories(Some(&raw), &items);
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0]["id"], "memory");
        assert_eq!(categories[0]["zh"], "记忆");
        assert_eq!(categories[0]["count"], 23);
    }

    #[test]
    fn dsh_market_unknown_or_invalid_category_becomes_unclassified() {
        let raw =
            serde_json::json!({"full_name":"owner/demo","name":"demo","category":"../unsafe"});
        let item = normalize_dsh_market_item("dshfind", &raw).unwrap();
        assert_eq!(item["category"], "unclassified");
        let categories = normalize_dsh_categories(None, &[item]);
        assert_eq!(categories[0]["id"], "unclassified");
        assert_eq!(categories[0]["count"], 1);
    }

    #[test]
    fn dsh_install_conflict_ids_cover_plugin_skills_and_mcp_servers() {
        let root = std::env::temp_dir().join(format!("ncx-dsh-conflict-{}", std::process::id()));
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        fs::create_dir_all(root.join("skills/pdf-export")).unwrap();
        fs::write(
            root.join(".codex-plugin/plugin.json"),
            r#"{"name":"reports"}"#,
        )
        .unwrap();
        fs::write(root.join("skills/pdf-export/SKILL.md"), "# PDF").unwrap();
        fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"browser":{}}}"#).unwrap();

        let ids = codex_plugin_resource_ids(&root).unwrap();
        assert!(ids.contains("plugin:reports"));
        assert!(ids.contains("skill:pdf-export"));
        assert!(ids.contains("mcp:browser"));
        let install_source = include_str!("lib.rs");
        assert!(
            install_source.contains("ensure_no_codex_plugin_capability_conflict(&source, upgrade)")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_codex_install_boundary_rejects_enabled_duplicate_capabilities() {
        let root = std::env::temp_dir().join(format!(
            "ncx-plugin-conflict-boundary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let installed_source = root.join("installed-source");
        let incoming_source = root.join("incoming-source");
        let catalog = CodexPluginCatalog::new(root.join("catalog"));
        for (source, name) in [
            (&installed_source, "installed-reports"),
            (&incoming_source, "incoming-reports"),
        ] {
            fs::create_dir_all(source.join(".codex-plugin")).unwrap();
            fs::create_dir_all(source.join("skills/pdf-export")).unwrap();
            fs::write(
                source.join(".codex-plugin/plugin.json"),
                format!(r#"{{"name":"{name}"}}"#),
            )
            .unwrap();
            fs::write(source.join("skills/pdf-export/SKILL.md"), "# PDF").unwrap();
        }
        catalog.install(&installed_source).unwrap();

        let error =
            ensure_no_codex_plugin_capability_conflict_in(&catalog, &incoming_source, false)
                .unwrap_err();
        assert!(error.contains("安装已阻断"));
        assert!(error.contains("skill:pdf-export"));

        catalog.set_enabled("installed-reports", false).unwrap();
        ensure_no_codex_plugin_capability_conflict_in(&catalog, &incoming_source, false).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_yunmo_models_replace_the_static_fallback_catalog() {
        let live = vec![yunmo_model("gpt-5.6-sol"), yunmo_model("gpt-5.6-terra")];
        let response = catalog_response(&[], &live, false);
        let yunmo = response
            .providers
            .iter()
            .find(|provider| provider.id == "yunmo")
            .unwrap();
        assert_eq!(
            yunmo
                .models
                .iter()
                .map(|model| model.model_id.as_str())
                .collect::<Vec<_>>(),
            vec!["gpt-5.6-sol", "gpt-5.6-terra"]
        );
    }

    #[test]
    fn dsh_markdown_commands_convert_into_a_valid_codex_resource_plugin() {
        let root = std::env::temp_dir().join(format!(
            "ncx-dsh-convert-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("commands")).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"dsh-plugin-demo","version":"1.2.3"}"#,
        )
        .unwrap();
        fs::write(
            root.join("commands/review.md"),
            "Review the current changes.",
        )
        .unwrap();

        prepare_dsh_portable_plugin(&root, "dsh-plugin-demo", "1.2.3").unwrap();

        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".codex-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["name"], "dsh-plugin-demo");
        assert!(root.join("skills/dsh-command-review/SKILL.md").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dsh_react_slots_convert_to_bounded_declarative_contributions() {
        let root = std::env::temp_dir().join(format!("ncx-dsh-slots-{}", now_epoch_millis()));
        fs::create_dir_all(root.join("src/client")).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"dsh-plugin-ui","version":"1.0.0","description":"安全 UI"}"#,
        )
        .unwrap();
        fs::write(root.join("src/client/index.tsx"), "slots.inject('settings.plugins.tab'); slots.inject('sidebar.footer.action'); slots.inject('shell.overlay');").unwrap();
        prepare_dsh_portable_plugin(&root, "dsh-plugin-ui", "1.0.0").unwrap();
        let slots: Vec<DshUiSlotContribution> =
            serde_json::from_slice(&fs::read(root.join(".ncx/ui-slots.json")).unwrap()).unwrap();
        assert_eq!(slots.len(), 3);
        assert!(slots.iter().any(|item| item.slot == "shell.overlay"));
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(".codex-plugin/plugin.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["interface"]["dshUiSlots"], "./.ncx/ui-slots.json");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dsh_unknown_react_slot_remains_incompatible() {
        let root =
            std::env::temp_dir().join(format!("ncx-dsh-unknown-slot-{}", now_epoch_millis()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"dsh-plugin-ui","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            root.join("src/index.tsx"),
            "slots.inject('conversation.secret.dynamic')",
        )
        .unwrap();
        let error = prepare_dsh_portable_plugin(&root, "dsh-plugin-ui", "1.0.0").unwrap_err();
        assert!(error.contains("尚未支持"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn dsh_slot_loader_rejects_duplicate_and_unsafe_declarations() {
        let root =
            std::env::temp_dir().join(format!("ncx-dsh-invalid-slot-{}", now_epoch_millis()));
        fs::create_dir_all(root.join(".ncx")).unwrap();
        fs::write(
            root.join(".ncx/ui-slots.json"),
            r#"[
          {"slot":"shell.overlay","id":"same","label":"A","url":"http://unsafe.example"},
          {"slot":"shell.overlay","id":"same","label":"B"}
        ]"#,
        )
        .unwrap();
        let plugin = CodexPluginRecord {
            manifest: CodexPluginManifest {
                name: "invalid-ui".into(),
                ..Default::default()
            },
            root: root.clone(),
            enabled: true,
        };
        assert!(load_dsh_ui_slots(&plugin).unwrap_err().contains("不安全"));
        fs::write(
            root.join(".ncx/ui-slots.json"),
            r#"[
          {"slot":"shell.overlay","id":"same","label":"A"},
          {"slot":"shell.overlay","id":"same","label":"B"}
        ]"#,
        )
        .unwrap();
        assert!(load_dsh_ui_slots(&plugin).unwrap_err().contains("重复"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reasoning_stays_collapsed_and_bounded_while_streaming() {
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let model = include_str!("../../src/lib/conversation-model.ts");
        assert!(model.contains("REASONING_DISPLAY_MAX_CHARS"));
        assert!(thread.contains("appendReasoning(message.text, event.text)"));
        assert!(conversation.contains(
            "<details class=\"reasoning-run\" class:settled={message.settled} class:current-run={busy && index > lastUserIndex}>"
        ));
        assert!(!conversation.contains(
            "class=\"reasoning-run\" class:settled={message.settled} class:current-run={busy && index > lastUserIndex} open="
        ));
        assert!(conversation.contains("<pre class=\"reasoning-content\">{message.text}</pre>"));
    }

    #[test]
    fn completed_turn_removes_transient_reasoning_cards() {
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        assert!(thread.contains("private removeReasoning()"));
        assert!(thread.contains("this.removeReasoning();"));
        assert!(thread.contains("this.messages = hideCompletedToolActivity(this.messages);"));
        assert!(thread.contains("case \"done\":"));
        assert!(thread.contains("case \"error\":"));
        assert!(thread
            .contains("this.messagesBySession.set(event.session_id, this.clone(this.messages));"));
        assert!(thread.contains("message.role !== \"reasoning\""));
    }

    #[test]
    fn completed_turn_keeps_prior_history_and_only_its_final_conclusion() {
        let model = include_str!("../../src/lib/conversation-model.ts");
        assert!(model.contains("function keepConversationConclusions("));
        assert!(model.contains("if (pendingAnswer) compacted.push(pendingAnswer);"));
        assert!(model.contains("compacted.push({ ...message });"));
        assert!(model.contains("pendingAnswer = { ...message };"));
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        assert!(thread.contains("keepConversationConclusions(this.messages, event.final_text)"));
    }

    #[test]
    fn stop_button_remains_retryable_until_turn_finishes() {
        let controller = include_str!("../../src/lib/composer-controller.svelte.ts");
        let composer = include_str!("../../src/components/Composer.svelte");
        assert!(controller.contains("if (!this.thread.busy) return;"));
        assert!(composer.contains("{#if busy}"));
        assert!(composer.contains("class=\"stop-btn visible\""));
        assert!(composer.contains("title={stopping ? \"再次停止\" : \"停止生成\"}"));
        assert!(!composer.contains("aria-label={busy ? \"排队\" : \"发送\"}"));
        assert!(!controller.contains("if (!this.thread.busy || this.thread.stopping) return;"));
    }

    #[test]
    fn model_switch_remains_available_while_a_turn_is_running() {
        let composer = include_str!("../../src/components/Composer.svelte");
        assert!(composer.contains("disabled={models.length === 0}"));
        assert!(!composer.contains("disabled={models.length === 0 || busy}"));
        assert!(composer.contains("当前任务继续使用原 Route，下一轮使用新 Route"));
    }

    #[test]
    fn sidebar_toggle_is_not_duplicated_when_sidebar_is_open() {
        let topbar = include_str!("../../src/components/TopBar.svelte");
        assert!(topbar.contains("{#if !sidebarOpen}<button class=\"collapse\""));
        assert!(!topbar.contains("title={sidebarOpen ? \"收起侧边栏\" : \"展开侧边栏\"}"));
    }

    #[test]
    fn automatic_context_compaction_is_visible_and_session_scoped() {
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let css = include_str!("../../src/app.css");
        let bridge = include_str!("bridge.rs");
        let core = include_str!("../../../crates/ncx-core/src/agent_loop/turn.rs");

        assert!(core.contains(".compact_safely_if_needed("));
        assert!(bridge.contains("UiEvent::ContextCompacted"));
        assert!(thread.contains("case \"context_compacted\":"));
        assert!(thread.contains("this.accepts(event.session_id)"));
        assert!(thread.contains("已自动压缩上下文"));
        assert!(thread.contains("role: \"compact\""));
        assert!(css.contains(".compact"));
    }

    #[test]
    fn session_usage_survives_restart_and_session_switches() {
        let usage = include_str!("../../src/lib/usage-controller.svelte.ts");
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        assert!(usage.contains("restore(sessionId: string)"));
        assert!(usage.contains("persist(sessionId: string)"));
        assert!(usage.contains("ncx.sessionUsage."));
        assert!(thread.contains("this.usage.add(event.session_id"));
        assert!(usage.contains("this.persist(sessionId)"));
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        assert!(lifecycle.matches("this.usage.restore(").count() >= 2);
        assert!(lifecycle.matches("this.usage.reset()").count() >= 2);
    }

    #[test]
    fn workspace_switch_invalidates_sidebar_and_profile_requests() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        assert!(lifecycle.contains("workspaceChanged = (): void"));
        assert!(lifecycle.contains("this.invalidateRefresh();"));
        assert!(lifecycle.contains("this.sessions = []"));
        assert!(lifecycle.contains("this.usage.replaceProtocolUsage([])"));
        assert!(lifecycle.contains("reset = this.workspaceChanged"));
        assert!(lifecycle.contains("const threadId = this.thread.currentId"));
        assert!(lifecycle.contains("const selection = ++this.profileSelectionGeneration"));
        assert!(
            lifecycle.contains("private profileSelectionQueue: Promise<void> = Promise.resolve()")
        );
        assert!(lifecycle.contains("await this.enqueueProfileSelection(async () =>"));
        assert!(lifecycle.contains("this.profileSelectionQueue.then(task, task)"));
        assert!(lifecycle.contains("params: { threadId, harnessProfile: profile }"));
        assert!(lifecycle.contains("isCurrentProfileSelection(threadId, selection)"));
        assert!(
            lifecycle
                .matches("if (!this.isCurrentProfileSelection(threadId, selection)) return;")
                .count()
                >= 3
        );
        assert!(lifecycle.contains("params: { threadId }"));
        assert!(lifecycle.contains("selection === this.profileSelectionGeneration"));
        assert!(
            app.contains("panels.workspaceChanged();\n      threadLifecycle.workspaceChanged();")
        );
    }

    #[test]
    fn frontend_rejects_events_from_inactive_sessions() {
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        assert!(thread.contains("accepts(sessionId: string)"));
        assert!(thread.contains("if (!this.accepts(event.session_id)) return"));
        assert!(thread.contains("pendingReadySession"));
        assert!(thread.contains("expectReady(sessionId: string)"));
        assert!(thread.contains("event.session_id !== this.pendingReadySession"));
        assert!(thread.contains("session_id: string; text: string"));
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
        assert!(bridge.contains("Command::New {"));
        assert!(bridge.contains("Some((id, Vec::new()))"));
        let new_branch = bridge
            .split_once("Command::New {")
            .unwrap()
            .1
            .split_once("Command::Resume")
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
        let composer = include_str!("../../src/lib/composer-controller.svelte.ts");
        assert!(bridge.contains("fn spawn_turn_worker("));
        assert!(bridge.contains("session_id: target_id"));
        assert!(bridge.contains("spawn_turn_worker("));
        assert!(backend.contains("session_id: String"));
        assert!(composer.contains("method: \"turnSubmit\""));
        assert!(composer.contains("threadId: targetSessionId"));
    }

    #[test]
    fn model_switch_keeps_the_current_thread_and_does_not_rebuild_harness() {
        let bridge = include_str!("bridge.rs");
        let branch = bridge
            .split_once("Command::SetModel(model) =>")
            .unwrap()
            .1
            .split_once("Command::SetPermissionMode {")
            .unwrap()
            .0;
        assert!(branch.contains("emit_ready(&app, &workspace, &session_id)"));
        assert!(!branch.contains("build_agent("));
        assert!(!branch.contains("write_nanocodex_config"));
        assert!(!branch.contains("UiEvent::Loaded"));

        let backend = include_str!("lib.rs");
        assert!(backend.contains("validate_route_model(&candidate, &model)?"));
        assert!(backend.contains(".activate(&cfg.active_provider_id, &model)?"));
    }

    #[test]
    fn stale_provider_activation_cannot_commit_after_a_newer_selection() {
        let gate = ProviderActivationGate::default();
        let older = gate.begin().unwrap();
        let newer = gate.begin().unwrap();
        let older_commit_ran = std::cell::Cell::new(false);

        let error = gate
            .commit(older, || {
                older_commit_ran.set(true);
                Ok(())
            })
            .unwrap_err();
        assert!(error.contains("较早切换已取消"));
        assert!(!older_commit_ran.get());

        let committed = gate.commit(newer, || Ok("latest")).unwrap();
        assert_eq!(committed, "latest");
    }

    #[test]
    fn provider_activation_diagnostics_track_lifecycle_without_secrets() {
        let gate = ProviderActivationGate::default();
        assert_eq!(gate.diagnostics().unwrap().status, "idle");

        let failed = gate.begin().unwrap();
        assert_eq!(gate.diagnostics().unwrap().status, "validating");
        gate.fail(
            failed,
            "HTTP 401 Bearer relay-secret token=another-secret sk-third-secret".into(),
        );
        let diagnostics = gate.diagnostics().unwrap();
        assert_eq!(diagnostics.status, "failed");
        let error = diagnostics.last_error.unwrap();
        assert!(!error.contains("relay-secret"));
        assert!(!error.contains("another-secret"));
        assert!(!error.contains("third-secret"));
        assert!(error.contains("[已脱敏]"));

        let active = gate.begin().unwrap();
        gate.commit(active, || Ok(())).unwrap();
        let diagnostics = gate.diagnostics().unwrap();
        assert_eq!(diagnostics.status, "active");
        assert!(diagnostics.last_error.is_none());
        assert!(diagnostics.updated_at_ms > 0);
    }

    #[test]
    fn frontend_thread_lifecycle_uses_the_versioned_app_server_protocol() {
        let app = include_str!("../../src/App.svelte");
        let frontend = format!(
            "{app}\n{}\n{}\n{}",
            include_str!("../../src/lib/slash-controller.svelte.ts"),
            include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts"),
            include_str!("../../src/lib/composer-controller.svelte.ts"),
        );
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
                frontend.contains(&format!("method: \"{method}\"")),
                "missing app-server request for {method}"
            );
        }
        assert!(!app.contains("invoke(\"archive_session\""));
        assert!(!app.contains("invoke<SessionRow[]>(\"list_sessions\")"));
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let refresh = lifecycle
            .split_once("refresh =")
            .unwrap()
            .1
            .split_once("archive = async")
            .unwrap()
            .0;
        assert!(!refresh.contains("method: \"threadRead\""));
    }

    #[test]
    fn sidebar_refresh_coalesces_bursty_events_with_one_trailing_pass() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        assert!(lifecycle.contains("private refreshInFlight: Promise<void> | null = null;"));
        assert!(lifecycle.contains("private refreshDirty = false;"));
        assert!(lifecycle.contains("if (this.refreshInFlight) {\n      this.refreshDirty = true;"));
        assert!(lifecycle.contains("const flight = this.runRefreshLoop();"));
        assert!(lifecycle.contains("await this.refreshOnce();"));
        assert!(lifecycle.contains("} while (this.refreshDirty);"));
        assert!(lifecycle.contains("const generation = ++this.refreshGeneration;"));
        assert!(lifecycle.contains("if (generation !== this.refreshGeneration) return;"));
    }

    #[test]
    fn goal_controller_retries_only_the_new_thread_not_found_race() {
        let controller = include_str!("../../src/lib/goal-controller.svelte.ts");
        assert!(controller.contains("readWithNewThreadRetry"));
        assert!(controller.contains("`${threadId} was not found`"));
        assert!(controller.contains("window.setTimeout(resolve, 120)"));
        assert!(controller.contains("throw error"));
    }

    #[test]
    fn resumed_assistant_messages_keep_requested_and_response_model_metadata() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        assert!(lifecycle.contains("model: item.model"));
        assert!(lifecycle.contains("confirmedModel: item.confirmedModel"));
        assert!(conversation.contains("请求 {message.model} → 响应字段 {message.confirmedModel}"));
        assert!(conversation.contains("该字段不证明中转站上游的内部模型"));
    }

    #[test]
    fn composer_shows_the_complete_active_provider_route() {
        let bridge = include_str!("bridge.rs");
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        let controls = include_str!("../../src/lib/model-controls-controller.svelte.ts");
        let composer = include_str!("../../src/components/Composer.svelte");
        assert!(bridge.contains("provider_id,"));
        assert!(bridge.contains("let provider_id = visible_provider_id(&cfg)"));
        assert!(bridge.contains("provider_protocol: cfg.provider_protocol"));
        assert!(runtime.contains("this.models.currentProvider = event.provider_id"));
        assert!(runtime.contains("this.models.currentProtocol = event.provider_protocol"));
        assert!(controls.contains("get routeLabel(): string"));
        assert!(controls.contains("method: \"customProviderList\""));
        assert!(controls.contains("method: \"customProviderActivate\""));
        assert!(controls.contains("method: \"modelPresetApply\""));
        assert!(controls.contains("id === \"deepseek\" && settings.has_deepseek_api_key"));
        assert!(controls.contains("id === \"yunmo\" && settings.has_yunmo_api_key"));
        assert!(controls.contains("route.id.replace(/^preset:/, \"\") === normalizedCurrent"));
        assert!(controls.contains("this.routes = visible ? [{ ...visible, active: true }] : []"));
        assert!(controls.contains("切换 Provider 失败，当前 Route 未改变"));
        assert!(controls.contains("get currentProviderName(): string"));
        assert!(composer.contains("{currentProviderName || currentProvider || \"Route\"}"));
        assert!(composer.contains("{#each routes as route (route.id)}"));
        assert!(composer.contains("data-provider={route.id} data-model={model}"));
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
            .split_once(".build(tauri::generate_context!())")
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
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        let protocol_client = include_str!("../../src/lib/app-server-client.ts");
        assert!(runtime.contains("listen<ProtocolEventEnvelope>(\"ncx://protocol-event\""));
        assert!(runtime.contains("this.sequenceGate.accept(envelope)"));
        assert!(protocol_client.contains("envelope.protocolVersion !== 3 || !envelope.threadId"));
        assert!(protocol_client.contains("this.sequences.get(envelope.threadId) || 0"));
        assert!(protocol_client.contains("envelope.sequence <= previous"));
        assert!(
            protocol_client.contains("this.sequences.set(envelope.threadId, envelope.sequence)")
        );
    }

    #[test]
    fn workspace_panel_operations_fence_stale_completion_and_busy_state() {
        for source in [
            include_str!("../../src/lib/git-workspace-controller.svelte.ts"),
            include_str!("../../src/lib/checkpoint-controller.svelte.ts"),
            include_str!("../../src/lib/memory-controller.svelte.ts"),
        ] {
            // A workspace reset must invalidate pending operations, while
            // overlapping operations in the same workspace keep the spinner
            // visible until the last live operation settles.
            assert!(source.contains("activeBusyOperations = new Set<number>()"));
            assert!(source.contains("this.activeBusyOperations.clear()"));
            assert!(source.contains("if (!this.activeBusyOperations.delete(operation)) return;"));
            assert!(source.contains("this.busy = this.activeBusyOperations.size > 0"));
        }
    }

    #[test]
    fn workspace_panel_requests_carry_and_enforce_a_workspace_snapshot() {
        let backend = include_str!("lib.rs");
        assert!(backend.contains("fn with_workspace_snapshot<T>("));
        assert!(backend.contains("require_workspace_match(expected_workspace, &current)?;"));
        assert!(backend.contains("fn run_git(workspace: &Path, args: &[&str])"));
        for command in [
            "fn get_checkpoints(",
            "fn checkpoint_files(",
            "fn create_checkpoint(",
            "fn restore_checkpoint(",
            "fn git_branches(",
            "fn git_create_branch(",
            "fn git_switch_branch(",
            "fn git_log(",
            "fn git_diff(",
            "fn git_changes(",
            "fn git_file_diff(",
            "fn list_dir(",
            "fn read_workspace_file(",
            "fn open_memory_file(",
        ] {
            let start = backend.find(command).unwrap();
            let signature = &backend[start..start + 240.min(backend.len() - start)];
            assert!(
                signature.contains("expected_workspace: String"),
                "{command} must reject a stale workspace request"
            );
        }

        let git = include_str!("../../src/lib/git-workspace-controller.svelte.ts");
        for command in [
            "git_branches",
            "git_create_branch",
            "git_switch_branch",
            "git_log",
            "git_changes",
            "git_file_diff",
        ] {
            assert!(
                git.contains(&format!("\"{command}\", {{")) && git.contains("expectedWorkspace"),
                "Git request {command} must carry the UI workspace snapshot"
            );
        }
        let checkpoints = include_str!("../../src/lib/checkpoint-controller.svelte.ts");
        for command in [
            "get_checkpoints",
            "checkpoint_files",
            "create_checkpoint",
            "restore_checkpoint",
        ] {
            assert!(
                checkpoints.contains(&format!("\"{command}\", {{"))
                    && checkpoints.contains("expectedWorkspace"),
                "checkpoint request {command} must carry the UI workspace snapshot"
            );
        }
        let files = include_str!("../../src/lib/file-browser-controller.svelte.ts");
        assert!(files.contains("\"list_dir\", { rel: relativePath, expectedWorkspace }"));
        assert!(files.contains("\"read_workspace_file\", { rel: entry.path, expectedWorkspace }"));
        let memory = include_str!("../../src/lib/memory-controller.svelte.ts");
        assert!(memory.contains("method: \"memoryList\",\n      params: { workspace },"));
        assert!(memory.contains(
            "method: \"memoryMergeStatusRead\",\n        params: { workspace, generation },"
        ));
        assert!(memory.contains(
            "method: \"memoryMergeCancel\",\n        params: { workspace, generation },"
        ));
        assert!(memory.contains("await this.pollMerge(status.generation, operation, workspace);"));
        assert!(memory.contains("\"open_memory_file\", { expectedWorkspace }"));
        let memory_list = backend
            .split_once("fn list_memory(&self, expected_workspace: String)")
            .unwrap()
            .1
            .split_once("fn add_memory(")
            .unwrap()
            .0;
        assert!(memory_list.contains("with_workspace_snapshot(self.state, &expected_workspace"));
        assert!(memory_list.contains("memory_list_at(workspace)"));
        assert!(backend.contains("fn memory_merge_status(\n        &self,\n        expected_workspace: String,\n        generation: Option<u64>,"));
        assert!(backend.contains(".status_for_workspace(workspace, generation)?"));
        assert!(backend.contains("fn cancel_memory_merge(\n        &self,\n        expected_workspace: String,\n        generation: u64,"));
        assert!(backend.contains(".cancel_for_workspace(workspace, generation)?"));
    }

    #[test]
    fn diff_preview_caps_lines_bytes_and_preserves_utf8() {
        let exact_bytes =
            read_bounded_diff_bytes(&mut std::io::Cursor::new(b"abcd"), 4, 10).unwrap();
        assert_eq!(exact_bytes.bytes.as_slice(), b"abcd");
        assert!(!exact_bytes.truncated);

        let over_bytes =
            read_bounded_diff_bytes(&mut std::io::Cursor::new(b"abcde"), 4, 10).unwrap();
        assert_eq!(over_bytes.bytes.as_slice(), b"abcd");
        assert!(over_bytes.truncated);

        let exact_lines =
            read_bounded_diff_bytes(&mut std::io::Cursor::new(b"a\nb\n"), 64, 2).unwrap();
        assert_eq!(exact_lines.bytes.as_slice(), b"a\nb\n");
        assert!(!exact_lines.truncated);

        let over_lines =
            read_bounded_diff_bytes(&mut std::io::Cursor::new(b"a\nb\nc"), 64, 2).unwrap();
        assert_eq!(over_lines.bytes.as_slice(), b"a\nb\n");
        assert!(over_lines.truncated);

        let split_utf8 =
            read_bounded_diff_bytes(&mut std::io::Cursor::new("a界".as_bytes()), 3, 10).unwrap();
        assert_eq!(split_utf8.bytes.as_slice(), &"a界".as_bytes()[..3]);
        assert!(split_utf8.truncated);
        assert_eq!(
            bounded_diff_utf8(split_utf8.bytes, split_utf8.truncated).as_deref(),
            Some("a")
        );

        let line_limited = (0..=MAX_DIFF_PREVIEW_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let lines = bounded_diff_preview(&line_limited);
        assert!(lines.truncated);
        assert_eq!(lines.text.lines().count(), MAX_DIFF_PREVIEW_LINES);
        assert_eq!(lines.text.split('\n').count(), MAX_DIFF_PREVIEW_LINES);
        assert!(!lines.text.ends_with('\n'));
        assert!(!lines
            .text
            .contains(&format!("line-{MAX_DIFF_PREVIEW_LINES}")));

        let exact_rendered_lines = (0..MAX_DIFF_PREVIEW_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let exact_rendered = bounded_diff_preview(&exact_rendered_lines);
        assert!(!exact_rendered.truncated);
        assert_eq!(
            exact_rendered.text.split('\n').count(),
            MAX_DIFF_PREVIEW_LINES
        );
        assert!(!exact_rendered.text.ends_with('\n'));

        let byte_limited = "界".repeat(MAX_DIFF_PREVIEW_BYTES);
        let bytes = bounded_diff_preview(&byte_limited);
        assert!(bytes.truncated);
        assert!(bytes.text.len() <= MAX_DIFF_PREVIEW_BYTES);
        assert!(byte_limited.is_char_boundary(bytes.text.len()));

        let untracked = bounded_untracked_diff_preview(&line_limited);
        assert!(untracked.truncated);
        assert_eq!(untracked.text.lines().count(), MAX_DIFF_PREVIEW_LINES);
        assert!(untracked.text.starts_with("+line-0"));

        let root = unique_test_dir("diff-preview-utf8");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("large.txt");
        fs::write(
            &path,
            format!("{}界tail", "a".repeat(MAX_DIFF_PREVIEW_BYTES - 1)),
        )
        .unwrap();
        let file_preview = read_untracked_diff_preview(&path).unwrap();
        assert!(file_preview.truncated);
        assert!(file_preview.text.len() <= MAX_DIFF_PREVIEW_BYTES);
        assert!(file_preview.text.starts_with('+'));
        assert_eq!(
            workspace_relative_file(&root, "large.txt"),
            Some(path.canonicalize().unwrap())
        );
        assert!(is_workspace_relative_path(Path::new("nested/large.txt")));
        assert!(!is_workspace_relative_path(Path::new("")));
        assert!(!is_workspace_relative_path(Path::new(".")));
        assert!(!is_workspace_relative_path(Path::new("..")));
        assert!(!is_workspace_relative_path(Path::new("../outside.txt")));
        assert!(!is_workspace_relative_path(Path::new("/absolute.txt")));
        #[cfg(windows)]
        assert!(!is_workspace_relative_path(Path::new(
            "C:drive-relative.txt"
        )));
        assert!(workspace_relative_file(&root, "../outside.txt").is_none());
        assert!(workspace_relative_file(&root, &path.to_string_lossy()).is_none());
        let _ = fs::remove_dir_all(root);

        let complete = bounded_diff_preview("one\ntwo\n");
        assert!(!complete.truncated);
        assert_eq!(complete.text, "one\ntwo\n");
    }

    #[test]
    fn workspace_diff_panel_keeps_rows_intact_and_uses_the_panel_scroll() {
        let css = include_str!("../../src/app.css");
        let controller = include_str!("../../src/lib/git-workspace-controller.svelte.ts");
        let panel = include_str!("../../src/components/WorkspacePanels.svelte");
        let file_row = css
            .split_once(".wt-file {")
            .unwrap()
            .1
            .split_once('}')
            .unwrap()
            .0;
        assert!(file_row.contains("flex: 0 0 auto;"));
        assert!(css
            .contains(".rightpanel .wt-list { max-height: none; overflow: visible; margin: 0; }"));
        assert!(css.contains(
            ".rightpanel .wt-diff { max-height: none; overflow-x: auto; overflow-y: hidden; }"
        ));
        assert!(css.contains(".wt-diff-notice"));
        assert!(
            controller.contains("export type DiffPreview = { text: string; truncated: boolean }")
        );
        assert!(controller.contains("private diffPreviewGeneration = 0;"));
        assert!(controller.contains("private pendingDiffPath: string | null = null;"));
        assert!(controller.contains(
            "if (Object.hasOwn(this.diffOpenFiles, path) || this.pendingDiffPath === path) {"
        ));
        assert!(controller.contains("this.pendingDiffPath = path;"));
        assert!(controller.contains("this.pendingDiffPath = null;"));
        assert!(controller.contains("invoke<DiffPreview | string>(\"git_file_diff\""));
        assert!(
            controller.contains("this.diffOpenFiles = { [path]: normalizeDiffPreview(response) };")
        );
        assert!(panel.contains("diffOpenFiles[file.path].truncated"));
        assert!(panel.contains("Object.hasOwn(diffOpenFiles, file.path)"));
        assert!(panel.contains("diffOpenFiles[file.path].text.split(\"\\n\")"));
        assert!(panel.contains("预览已截断"));
    }

    #[test]
    fn runtime_start_stop_cannot_cross_dispose_generations() {
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        assert!(
            runtime.contains("if (!this.isActive(generation)) return;\n      await this.stop();")
        );
        assert!(runtime.contains("if (!this.isActive(generation)) return;"));
        assert!(
            runtime.contains("if (this.isActive(generation)) this.thread.handle(event.payload)")
        );
        assert!(runtime.contains("Promise.allSettled(listeners.map"));
        assert!(app.contains("})().catch((error) =>"));
        assert!(app.contains("if (disposed) return;"));
    }

    #[test]
    fn forge_observer_disposes_pollers_when_the_app_unmounts() {
        let forge = include_str!("../../src/lib/forge-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        assert!(forge.contains("dispose = (): void"));
        assert!(forge.contains("this.pollGeneration += 1"));
        assert!(forge.contains("isCurrentPoll(lifecycle, poll)"));
        assert!(forge.contains("const poll = ++this.pollGeneration;"));
        assert!(forge.contains("void this.poll(job.generation, lifecycle, poll, workspace);"));
        assert!(forge.contains("if (!this.isActive(lifecycle, operation)) return;"));
        assert!(forge.contains("private readonly workspace: () => string"));
        assert!(forge.contains("params: { workspace, generation },"));
        assert!(forge.contains(
            "method: \"forgeJobStatusRead\",\n          params: { workspace, generation },"
        ));
        assert!(app.contains("new ForgeController("));
        assert!(app.contains("() => runtime.workspace"));
        assert!(app.contains("forgeController.dispose();"));
    }

    #[test]
    fn forge_workspace_switch_clears_the_old_workspace_projection() {
        let forge = include_str!("../../src/lib/forge-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        assert!(forge.contains("workspaceChanged = (): void"));
        assert!(forge.contains("this.pollGeneration += 1"));
        assert!(forge.contains("this.activeLoadingOperations.clear()"));
        assert!(forge.contains("this.loading = false"));
        assert!(forge.contains("this.runtime = null"));
        assert!(forge.contains("this.job = null"));
        assert!(forge.contains("if (this.workspace()) void this.refresh();"));
        assert!(forge.contains("const workspace = this.workspace();"));
        assert!(forge.contains("params: { workspace, generation },"));
        assert!(app.contains("forgeController.workspaceChanged();"));
    }

    #[test]
    fn composer_submits_and_surfaces_orchestrator_execution_mode() {
        let composer = include_str!("../../src/lib/composer-controller.svelte.ts");
        let view = include_str!("../../src/components/SettingsModal.svelte");
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let bridge = include_str!("bridge.rs");
        let orchestrated_turn = include_str!("bridge/orchestrated_turn.rs");
        assert!(composer.contains("executionMode = $state<\"agent\" | \"orchestrator\">"));
        assert!(composer.contains("text, images, executionMode"));
        assert!(composer.contains("next.executionMode"));
        assert!(composer.contains("多 Agent 模式暂不支持图片附件"));
        assert!(view.contains("运行中的任务不变，当前会话下一轮生效"));
        assert!(view.contains("多 Agent 编排"));
        assert!(thread.contains("kind: \"orchestrator_stage\""));
        assert!(thread.contains("kind: \"orchestrator_activity\""));
        assert!(thread.contains("message.phase = \"finished\""));
        assert!(conversation.contains("message.role === \"orchestrator_activity\""));
        assert!(conversation.contains("失败 (${message.failure})"));
        assert!(bridge.contains("ExecutionMode::Orchestrator"));
        assert!(bridge.contains("UiEvent::OrchestratorStage"));
        assert!(orchestrated_turn.contains("UiEvent::OrchestratorActivity"));
    }

    #[test]
    fn runtime_and_interaction_controls_use_the_app_server_protocol() {
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        for method in [
            "runtimeStatusRead",
            "runtimeReadyRefresh",
            "threadCreateActivate",
            "interactionApprove",
            "interactionAnswer",
        ] {
            assert!(
                runtime.contains(&format!("method: \"{method}\"")),
                "missing app-server request for {method}"
            );
        }
        for legacy in [
            "invoke(\"get_status\"",
            "invoke(\"request_ready\"",
            "invoke<string>(\"set_workspace\"",
            "invoke(\"approve\"",
            "invoke(\"answer_question\"",
        ] {
            assert!(
                !runtime.contains(legacy),
                "legacy runtime command remains: {legacy}"
            );
        }
        assert!(
            !runtime.contains("method: \"workspaceSet\""),
            "the workspace picker must hand off CWD and Thread creation atomically"
        );
        assert!(
            runtime.find("method: \"interactionApprove\"").unwrap()
                < runtime.find("this.thread.removeApproval").unwrap()
        );
        assert!(
            runtime.find("method: \"interactionAnswer\"").unwrap()
                < runtime.find("this.thread.removeQuestion").unwrap()
        );
    }

    #[test]
    fn workspace_switch_creates_a_durable_protocol_thread_before_binding_it() {
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        let composer = include_str!("../../src/lib/composer-controller.svelte.ts");
        let composer_view = include_str!("../../src/components/Composer.svelte");
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        let backend = include_str!("lib.rs");
        let switch_backend = backend
            .split_once("fn set_workspace_for_state")
            .unwrap()
            .1
            .split_once("fn set_model_for_state")
            .unwrap()
            .0;

        let chooser = runtime
            .split_once("chooseWorkspace = async")
            .unwrap()
            .1
            .split_once("  decide = async")
            .unwrap()
            .0;

        assert!(chooser.contains("method: \"threadCreateActivate\""));
        assert!(chooser.contains("workspace: directory,"));
        assert!(
            !chooser.contains("method: \"workspaceSet\""),
            "the picker must not split the CWD transition from durable Thread creation"
        );
        assert!(runtime.contains("private workspacePickerOpen = false;"));
        assert!(runtime.contains("if (this.thread.switching || this.workspacePickerOpen) return;"));
        assert!(runtime.contains("const pickerSessionId = this.thread.currentId;"));
        assert!(runtime.contains("this.thread.switching = true;"));
        assert!(runtime.contains("workspace: string;"));
        assert!(runtime.contains("this.workspace = status.workspace;"));
        assert!(runtime.contains("if (this.thread.currentId !== pickerSessionId) return;"));
        assert!(
            runtime
                .find("const pickerSessionId = this.thread.currentId")
                .unwrap()
                < runtime.find("const directory = await open").unwrap()
        );
        assert!(
            runtime.find("const directory = await open").unwrap()
                < runtime
                    .find("if (this.thread.currentId !== pickerSessionId) return;")
                    .unwrap()
        );
        assert!(chooser.contains("const workspace = created.metadata.workspace;"));
        assert!(chooser.contains("const workspaceDidChange = this.workspace !== workspace;"));
        assert!(chooser.contains("this.workspace = workspace;"));
        assert!(chooser.contains("if (workspaceDidChange) this.workspaceChanged();"));
        assert!(runtime.contains("const workspaceDidChange = this.workspace !== event.workspace;"));
        assert!(runtime.contains("if (workspaceDidChange) {\n      this.workspaceChanged();\n      void this.lifecycle.refresh();\n    }"));
        assert!(
            chooser.find("method: \"threadCreateActivate\"").unwrap()
                < chooser
                    .find("const workspace = created.metadata.workspace;")
                    .unwrap()
        );
        assert!(
            chooser
                .find("const workspace = created.metadata.workspace;")
                .unwrap()
                < chooser.find("this.thread.currentId = threadId").unwrap()
        );
        assert_eq!(
            composer
                .matches("if (this.thread.switching) return;")
                .count(),
            3,
            "send, queued dispatch, and direct dispatch must all be fenced"
        );
        assert!(composer_view.contains("disabled={switching}"));
        assert!(composer_view.contains("disabled={switching || needsWorkspace"));
        let permission_picker = composer_view
            .split_once("<div class=\"approval-wrap\">")
            .unwrap()
            .1
            .split_once("{#if goalView}")
            .unwrap()
            .0;
        let permission_trigger = permission_picker
            .split_once("<button class=\"approval-pill\"")
            .unwrap()
            .1
            .split_once("title=\"权限模式\"")
            .unwrap()
            .0;
        assert!(
            permission_trigger.contains("disabled={switching}"),
            "permission-mode entry must be unavailable while a Thread navigation is in flight"
        );
        let permission_option = permission_picker
            .split_once("<button class=\"approval-opt\"")
            .unwrap()
            .1
            .split_once("<span class=\"opt-check\">")
            .unwrap()
            .0;
        assert!(
            permission_option.contains("disabled={switching}"),
            "an already-open permission-mode menu must not submit a stale Thread request"
        );
        assert!(app.contains("switching={thread.switching}"));
        let loaded_handler = thread
            .split_once("private handleLoaded")
            .unwrap()
            .1
            .split_once("private handleError")
            .unwrap()
            .0;
        let error_handler = thread
            .split_once("private handleError")
            .unwrap()
            .1
            .split_once("private settleReasoning")
            .unwrap()
            .0;
        assert!(!loaded_handler.contains("this.switching = false;"));
        assert!(!error_handler.contains("this.switching = false;"));
        assert!(!switch_backend.contains("Command::Reload"));
        assert!(backend.contains("recv_timeout(Duration::from_secs(30))"));
    }

    #[test]
    fn failed_workspace_navigation_keeps_ready_fences_and_rolls_back_cwd() {
        let runtime = include_str!("../../src/lib/app-runtime-controller.svelte.ts");
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        let app = include_str!("../../src/App.svelte");
        let backend = include_str!("lib.rs");

        // The picker delegates the entire CWD + worker handoff to one backend
        // request. Its failure branch reads the final state, never queues a
        // stale workspaceSet compensation after a newer handoff.
        let chooser = runtime
            .split_once("chooseWorkspace = async")
            .unwrap()
            .1
            .split_once("  decide = async")
            .unwrap()
            .0;
        assert!(!chooser.contains("method: \"workspaceSet\""));
        assert!(chooser.contains("method: \"threadCreateActivate\""));
        assert!(chooser.contains("workspace: directory,"));
        assert!(chooser.contains("const workspace = created.metadata.workspace;"));
        assert!(!runtime.contains("restoreWorkspace = async"));
        let chooser_failure = chooser.split_once("} catch (error) {").unwrap().1;
        assert!(chooser_failure.contains("this.thread.sealReadyFence();"));
        assert!(chooser_failure.contains("await this.reconcileWorkspace();"));
        assert!(chooser_failure.contains("this.sameWorkspace(previousWorkspace, currentWorkspace)"));
        assert!(!chooser_failure.contains("this.restoreWorkspace("));
        assert!(chooser_failure.contains("if (previousId) this.thread.expectReady(previousId);"));
        assert!(
            chooser_failure
                .find("this.thread.sealReadyFence();")
                .unwrap()
                < chooser_failure
                    .find("await this.reconcileWorkspace();")
                    .unwrap()
        );
        assert!(!runtime.contains("clearExpectedReady"));

        // Resume/Fork transition process CWD in the adapter. If an unaccepted
        // handoff fails, the backend rolls it back under its gates; the UI only
        // reads the resulting CWD before rebinding its old Thread. It must not
        // issue a delayed workspaceSet that can overwrite a newer handoff.
        for operation in ["resume = async", "fork = async"] {
            let body = lifecycle
                .split_once(operation)
                .unwrap()
                .1
                .split_once("  openLog = async")
                .unwrap()
                .0;
            assert!(body.contains("const previousWorkspace = this.workspace();"));
            assert!(body.contains("const navigation = this.beginNavigation();"));
            assert!(body.contains("await this.reconcileWorkspaceAfterRejectedNavigation("));
            assert!(body.contains("if (previousId) this.thread.expectReady(previousId);"));
            assert!(body.contains("this.leaveNavigationUnbound("));
            assert!(
                body.find("await this.reconcileWorkspaceAfterRejectedNavigation(")
                    .unwrap()
                    < body
                        .find("if (previousId) this.thread.expectReady(previousId);")
                        .unwrap()
            );
            // Once the backend handoff has returned successfully, later UI
            // reads/renames must not run the pre-handoff CWD compensation.
            let accepted = body.find("if (activationAccepted)").unwrap();
            let rollback = body
                .find("await this.reconcileWorkspaceAfterRejectedNavigation(")
                .unwrap();
            assert!(accepted < rollback);
            assert!(body.contains("this.keepAcceptedNavigation("));
            assert!(body.contains("let activationAccepted = false;"));
            assert!(body.contains("activationAccepted = true;"));
            let accepted_branch = &body[accepted..rollback];
            if operation == "resume = async" {
                let clear = body.find("this.thread.clearSkippedLoaded(id);").unwrap();
                let accepted_return = accepted + accepted_branch.find("return;").unwrap();
                assert!(
                    accepted_return < rollback && rollback < clear,
                    "an accepted resume must retain its legacy Loaded suppression"
                );
            } else {
                let skip = body
                    .find("this.thread.skipNextLoaded(newThreadId);")
                    .unwrap();
                let clear = body
                    .find("this.thread.clearSkippedLoaded(newThreadId);")
                    .unwrap();
                let accepted_return = accepted + accepted_branch.find("return;").unwrap();
                assert!(skip < accepted);
                assert!(
                    accepted_return < rollback && rollback < clear,
                    "an accepted fork must retain its legacy Loaded suppression"
                );
            }
        }
        assert!(lifecycle.contains("this.thread.sealReadyFence();"));
        assert!(!lifecycle.contains("workspaceRecovery.restore("));
        assert!(lifecycle.contains("this.workspaceRecovery.reconcile()"));
        assert!(lifecycle.contains("private sameWorkspace"));
        assert!(!app.contains("restore: (path) => runtime.restoreWorkspace(path)"));
        assert!(app.contains("reconcile: () => runtime.reconcileWorkspace()"));

        let handoff = backend
            .split_once("fn finish_runtime_handoff(")
            .unwrap()
            .1
            .split_once("impl AppServerAdapter")
            .unwrap()
            .0;
        let cancel = handoff
            .find("state.memory_merge.cancel_for_workspace_switch()?;")
            .unwrap();
        let restore = handoff
            .find("std::env::set_current_dir(previous_cwd)")
            .unwrap();
        assert!(handoff.contains("run_failed_handoff_rollback"));
        assert!(cancel < restore);

        // The pending expected id is only consumed by an accepted matching
        // Ready; sealing blocks all Ready events in a safe empty state.
        let ready_handler = thread
            .split_once("private handleReady")
            .unwrap()
            .1
            .split_once("private handleAssistantDelta")
            .unwrap()
            .0;
        assert!(thread.contains("private readyFenceClosed = false;"));
        assert!(thread.contains("sealReadyFence(): void"));
        assert!(ready_handler.contains("if (this.readyFenceClosed) return;"));
        assert!(ready_handler.contains("this.consumeExpectedReady(event.session_id);"));
        assert_eq!(thread.matches("consumeExpectedReady(").count(), 2);
    }

    #[test]
    fn ordinary_status_notes_do_not_use_the_error_palette() {
        let conversation = include_str!("../../src/components/ConversationView.svelte");
        let css = include_str!("../../src/app.css");
        assert!(conversation.contains("noteTone(message.text)"));
        assert!(conversation.contains("已切换"));
        assert!(css.contains(".note.success"));
        assert!(css.contains(".note.error"));
        let base_note = css
            .split_once(".note {")
            .unwrap()
            .1
            .split_once('}')
            .unwrap()
            .0;
        assert!(!base_note.contains("var(--danger)"));
    }

    #[test]
    fn settings_and_model_controls_use_the_app_server_protocol() {
        let model = include_str!("../../src/lib/model-controls-controller.svelte.ts");
        let settings = include_str!("../../src/lib/settings-controller.svelte.ts");
        let frontend = format!("{model}\n{settings}");
        for method in [
            "settingsRead",
            "settingsUpdate",
            "runtimeModelSet",
            "runtimePermissionModeSet",
            "modelCatalogRead",
            "modelPresetApply",
        ] {
            assert!(
                frontend.contains(&format!("method: \"{method}\"")),
                "missing app-server request for {method}"
            );
        }
        for legacy in [
            "invoke(\"set_model\"",
            "invoke<Settings>(\"get_settings\"",
            "invoke(\"save_settings\"",
            "invoke(\"set_permission_mode\"",
            "invoke<ModelCatalogResponse>(\"get_model_catalog\"",
            "invoke<CatalogModel>(\"apply_model_preset\"",
        ] {
            assert!(
                !frontend.contains(legacy),
                "legacy model command remains: {legacy}"
            );
        }
        assert!(settings.contains("sandbox_mode: settings.sandbox_mode"));
        assert!(settings.contains("approval_policy: settings.approval_policy"));
        assert!(model.contains("private readonly currentThreadId: () => string"));
        assert!(model.contains("params: { threadId, mode: id }"));
    }

    #[test]
    fn all_plugin_settings_share_the_app_server_protocol_boundary() {
        let plugins = include_str!("../../src/lib/plugin-controller.svelte.ts");
        for method in [
            "harnessDiagnosticsRead",
            "externalPluginList",
            "externalPluginInstall",
            "externalPluginSetEnabled",
            "codexPluginList",
            "codexPluginInstall",
            "codexPluginSetEnabled",
            "codexPluginUninstall",
            "marketplaceList",
            "marketplacePluginInstall",
        ] {
            assert!(
                plugins.contains(&format!("method: \"{method}\"")),
                "missing app-server plugin request for {method}"
            );
        }
        for legacy in [
            "invoke<HarnessDiagnostics>(\"get_harness_diagnostics\"",
            "invoke<ExternalPlugin[]>(\"list_external_plugins\"",
            "invoke(\"install_external_plugin\"",
            "invoke(\"set_external_plugin_enabled\"",
        ] {
            assert!(
                !plugins.contains(legacy),
                "legacy plugin command remains: {legacy}"
            );
        }
    }

    #[test]
    fn project_memory_uses_the_app_server_service_boundary() {
        let memory = include_str!("../../src/lib/memory-controller.svelte.ts");
        for method in ["memoryList", "memoryAdd", "memoryConsolidate"] {
            assert!(
                memory.contains(&format!("method: \"{method}\"")),
                "missing app-server memory request for {method}"
            );
        }
        assert!(memory.contains("params: { workspace },"));
        for legacy in [
            "invoke<MemoryNote[]>(\"memory_list\"",
            "invoke<number>(\"memory_consolidate\"",
            "invoke<boolean>(\"memory_add\"",
        ] {
            assert!(
                !memory.contains(legacy),
                "legacy memory command remains: {legacy}"
            );
        }
        assert!(memory.contains("invoke(\"open_memory_file\""));
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
        assert!(bridge.contains("let mcp_servers = discover_codex_mcp_servers(&cfg.workspace)"));
        assert!(bridge.contains("prepare_mcp_server_tools("));
        assert!(bridge.contains("append_prepared_mcp_tools("));
        assert!(bridge.contains("跳过无法启动的 Codex MCP server"));
        assert!(bridge.contains("configured_servers: mcp_servers.len()"));
        assert!(bridge.contains("active_tools"));
        assert!(bridge.contains("tools.replace_service("));
    }

    #[test]
    fn switching_sessions_does_not_stop_the_previous_turn() {
        let composer = include_str!("../../src/lib/composer-controller.svelte.ts");
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let resume = lifecycle
            .split_once("resume = async")
            .unwrap()
            .1
            .split_once("fork = async")
            .unwrap()
            .0;
        assert!(!resume.contains("stop_generation"));
        assert!(composer.contains("this.thread.setRunning(targetSessionId, true)"));
        let thread = include_str!("../../src/lib/thread-controller.svelte.ts");
        assert!(thread.contains("this.setRunning(event.session_id, false)"));
        assert!(lifecycle.contains("this.thread.switching = false;"));
        assert!(lifecycle.contains("Promise.allSettled"));
        assert!(thread.contains("clearSkippedLoaded"));
    }

    #[test]
    fn history_session_switch_keeps_the_active_turn_running() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let sidebar = include_str!("../../src/components/SessionSidebar.svelte");
        let bridge = include_str!("bridge.rs");
        assert!(sidebar.contains(
            "class=\"recent-main\" title={s.snippet || s.title} disabled={switchingSession}"
        ));
        assert!(sidebar.contains("disabled={switchingSession || !s.has_snapshot} onclick={() => { menuId = \"\"; forkSession"));
        let resume = lifecycle
            .split_once("resume = async")
            .unwrap()
            .1
            .split_once("fork = async")
            .unwrap()
            .0;
        assert!(!resume.contains("stop_generation"));
        assert!(resume.contains("this.thread.busy = this.thread.runningSessions.has(id)"));
        assert!(resume.contains("method: \"threadActivate\""));
        let backend_resume = bridge
            .split_once("Command::Resume {")
            .unwrap()
            .1
            .split_once("Command::Fork")
            .unwrap()
            .0;
        assert!(!backend_resume.contains("build_agent("));
        assert!(backend_resume.contains("UiEvent::Loaded"));
        assert!(backend_resume.contains("emit_ready(&app, &workspace, &session_id)"));
        assert!(backend_resume.contains("workspace = command_workspace;"));
        assert!(backend_resume.contains("completion.send(Ok(()))"));
    }

    #[test]
    fn worker_navigation_uses_durable_workspaces_and_acknowledges_failures() {
        let bridge = include_str!("bridge.rs");
        let backend = include_str!("lib.rs");

        // Only the Tauri-side transition owns global CWD writes. In
        // particular, a queued Resume/Fork must never reset it after a later
        // workspaceSet command has completed.
        assert!(!bridge.contains("restore_session_workspace"));
        assert!(!bridge.contains("std::env::set_current_dir"));
        assert!(!bridge.contains("std::env::current_dir"));
        assert!(backend.contains("fn transition_workspace_for_state"));
        assert!(backend.contains("transition_workspace_for_state(&workspace, self.state)?"));
        assert!(backend.contains("transition_workspace_for_state(&saved_workspace, &state)"));

        let new_branch = bridge
            .split_once("Command::New {")
            .unwrap()
            .1
            .split_once("Command::Resume")
            .unwrap()
            .0;
        assert!(new_branch.contains("workspace: command_workspace"));
        assert!(new_branch.contains("completion.send(Ok(()))"));
        assert!(new_branch.contains("completion.send(Err(e.clone()))"));

        let resume_branch = bridge
            .split_once("Command::Resume {")
            .unwrap()
            .1
            .split_once("Command::Fork")
            .unwrap()
            .0;
        assert!(resume_branch.contains("workspace = command_workspace;"));
        assert!(resume_branch.contains("completion.send(Err(message.clone()))"));
        assert!(resume_branch.contains("completion.send(Ok(()))"));

        let fork_branch = bridge
            .split_once("Command::Fork {")
            .unwrap()
            .1
            .split_once("Command::SetModel")
            .unwrap()
            .0;
        assert!(fork_branch.contains("command_workspace"));
        assert!(fork_branch.contains("completion.send(Err(message.clone()))"));
        assert!(fork_branch.contains("completion.send(Err(e.clone()))"));
        assert!(fork_branch.contains("completion.send(Ok(()))"));

        let permission_branch = bridge
            .rsplit_once("Command::SetPermissionMode {")
            .unwrap()
            .1
            .split_once("Command::RequestReady")
            .unwrap()
            .0;
        assert!(permission_branch.contains("thread_id: target_id"));
        assert!(permission_branch.contains("permission_mode_rebuild_input("));
        let guard = permission_branch
            .find("permission_mode_rebuild_input(")
            .unwrap();
        let persist = permission_branch.find("write_nanocodex_config").unwrap();
        assert!(
            guard < persist,
            "stale target must be rejected before config write"
        );
        assert!(permission_branch.contains("completion.send(Err(error.clone()))"));
        assert!(permission_branch.contains("build_agent("));
    }

    #[test]
    fn runtime_handoff_serializes_cwd_transition_through_worker_acknowledgement() {
        let gate = Arc::new(RuntimeHandoffGate::default());
        let (events_tx, events_rx) = mpsc::channel();
        let (worker_ack_tx, worker_ack_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        let first_gate = Arc::clone(&gate);
        let first_events_tx = events_tx.clone();
        let first = std::thread::spawn(move || {
            let _permit = first_gate.acquire().unwrap();
            first_events_tx.send("A transition").unwrap();
            ready_tx.send(()).unwrap();
            worker_ack_rx.recv().unwrap();
            first_events_tx.send("A ack").unwrap();
            // The permit is deliberately held until this acknowledgement has
            // been published, just as the real handoff waits for its worker.
        });

        ready_rx.recv().unwrap();
        assert_eq!(events_rx.recv().unwrap(), "A transition");

        let second_gate = Arc::clone(&gate);
        let second_events_tx = events_tx.clone();
        let second = std::thread::spawn(move || {
            let _permit = second_gate.acquire().unwrap();
            second_events_tx.send("B transition").unwrap();
        });

        // This is a state-based contention point, not a sleep or an elapsed
        // time guess: B has registered as a waiter while A still owns the
        // permit, so no CWD transition from B can have happened yet.
        gate.wait_until_contended();
        assert!(matches!(
            events_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        worker_ack_tx.send(()).unwrap();
        assert_eq!(events_rx.recv().unwrap(), "A ack");
        assert_eq!(events_rx.recv().unwrap(), "B transition");
        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn archived_sessions_render_below_recent_sessions() {
        let lifecycle = include_str!("../../src/lib/thread-lifecycle-controller.svelte.ts");
        let sidebar_component = include_str!("../../src/components/SessionSidebar.svelte");
        assert!(lifecycle.contains("this.sessions.filter((session) => !session.archived)"));
        assert!(lifecycle.contains("this.sessions.filter((session) => session.archived)"));

        let sidebar = sidebar_component
            .split_once("<div class=\"side-recents\">")
            .unwrap()
            .1
            .split_once("<div class=\"side-foot\">")
            .unwrap()
            .0;
        let recent = sidebar.find("{#each projectGroups as group}").unwrap();
        let archive_toggle = sidebar.find("class=\"side-archive-toggle\"").unwrap();
        let archived = sidebar
            .find("{#each archivedProjectGroups as group}")
            .unwrap();
        assert!(recent < archive_toggle);
        assert!(archive_toggle < archived);
        assert!(sidebar.contains("aria-expanded={showArchived}"));
        assert!(sidebar.contains("aria-expanded={archivedProjectOpen[group.path] !== false}"));
        assert!(sidebar.contains("toggleArchivedProject(group.path)"));

        let css = include_str!("../../src/app.css");
        assert!(css.contains(".side-archive-toggle"));
        assert!(css.contains(".side-archived-list"));
    }

    #[test]
    fn sessions_are_grouped_under_collapsible_projects() {
        let sidebar_component = include_str!("../../src/components/SessionSidebar.svelte");
        let protocol = include_str!("../../src/lib/app-server-client.ts");

        let sidebar = sidebar_component
            .split_once("<div class=\"side-recents\">")
            .unwrap()
            .1
            .split_once("<div class=\"side-foot\">")
            .unwrap()
            .0;
        assert!(protocol.contains("workspace: normalizeWorkspacePath(thread.metadata.workspace)"));
        assert!(protocol.contains("normalized.startsWith(\"\\\\\\\\?\\\\\")"));
        assert!(sidebar_component.contains("const key = path.toLocaleLowerCase()"));
        assert!(sidebar_component
            .contains("const projectGroups = $derived(groupByWorkspace(recentSessions))"));
        assert!(sidebar_component.contains(
            "const archivedProjectGroups = $derived(groupByWorkspace(archivedSessions))"
        ));
        assert!(sidebar.contains("{#each projectGroups as group}"));
        assert!(sidebar.contains("class=\"project-toggle\""));
        assert!(sidebar.contains("aria-expanded={projectOpen[group.path] !== false}"));
        assert!(sidebar.contains("{#each group.sessions as s}"));

        let css = include_str!("../../src/app.css");
        assert!(css.contains(".project-toggle"));
        assert!(css.contains(".project-sessions"));
        assert!(css.contains("padding-left: 1.35rem"));
        assert!(!sidebar_component.contains("class=\"foot-ws\""));
    }

    #[test]
    fn duplicate_model_id_keeps_the_current_provider_route() {
        let yunmo = vec![yunmo_model("gpt-5.6-sol")];
        let selected =
            find_preset_by_model_id("gpt-5.6-sol", "https://api.yunmo-ai.com/v1/", &[], &yunmo)
                .expect("yunmo route should match by Base URL");
        assert_eq!(selected.provider_id, "yunmo");
        assert_eq!(selected.base_url, "https://api.yunmo-ai.com/v1");

        assert!(
            find_preset_by_model_id("gpt-5.6-sol", "https://custom.example/v1", &[], &yunmo,)
                .is_none()
        );
    }

    #[test]
    fn legacy_deepseek_and_yunmo_keys_migrate_to_independent_preset_routes() {
        let root = std::env::temp_dir().join(format!(
            "ncx-preset-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = ConfigPaths {
            deepseek: root.join("deepseek.toml"),
            codex: root.join("codex.toml"),
            nanocodex: root.join("config.toml"),
        };
        let directory = ProviderDirectoryService::from_paths(&paths);
        let cfg = Config {
            deepseek_api_key: "legacy-deepseek".into(),
            yunmo_api_key: "legacy-yunmo".into(),
            ..Config::default()
        };
        let deepseek = find_preset("deepseek", "deepseek-v4-flash").unwrap();
        let yunmo = yunmo_model("gpt-5.6-sol");

        write_preset_with_config(
            &directory,
            &deepseek,
            std::slice::from_ref(&deepseek.model_id),
            &cfg,
        )
        .unwrap();
        assert_eq!(
            directory.get("preset:deepseek").unwrap().api_key,
            "legacy-deepseek"
        );
        write_preset_with_config(
            &directory,
            &yunmo,
            std::slice::from_ref(&yunmo.model_id),
            &cfg,
        )
        .unwrap();
        assert_eq!(
            directory.get("preset:yunmo").unwrap().api_key,
            "legacy-yunmo"
        );
        assert_eq!(
            directory.diagnostics().unwrap().active_provider_id,
            "preset:yunmo"
        );
        let snapshot = std::fs::read_to_string(&paths.nanocodex).unwrap();
        assert!(snapshot.contains("model = \"gpt-5.6-sol\""));
        assert!(!serde_json::to_string(&directory.list().unwrap())
            .unwrap()
            .contains("legacy-yunmo"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_provider_settings_use_only_the_app_server_boundary() {
        let component = include_str!("../../src/components/CustomProvidersSettings.svelte");
        for method in [
            "customProviderList",
            "customProviderSave",
            "customProviderDelete",
            "customProviderModelsDiscover",
            "customProviderActivate",
            "customProviderChatProbe",
        ] {
            assert!(
                component.contains(&format!("method: \"{method}\"")),
                "missing app-server request {method}"
            );
        }
        assert!(component.contains("真实请求 1 个输出 Token"));
        assert!(component.contains("目录可用不代表对话权限已开通"));
        assert!(!component.contains("@tauri-apps/api/core"));
        for legacy in [
            "list_custom_providers",
            "save_custom_provider",
            "delete_custom_provider",
            "discover_custom_provider_models",
            "activate_custom_provider",
        ] {
            assert!(
                !component.contains(legacy),
                "legacy Tauri invoke remains: {legacy}"
            );
        }
    }
}
