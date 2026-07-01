use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

pub const DEFAULT_API_ADDR: &str = "127.0.0.1:8000";
pub const DEFAULT_ADMIN_ADDR: &str = "127.0.0.1:8001";
pub const DEFAULT_IMAGE_MODEL: &str = "jimeng-image-3.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayConfig {
    pub api_addr: SocketAddr,
    pub admin_addr: SocketAddr,
    pub state_path: PathBuf,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self, GatewayError> {
        let api_addr = std::env::var("NCX_DREAMINA_API_ADDR")
            .unwrap_or_else(|_| DEFAULT_API_ADDR.to_string())
            .parse()
            .map_err(|e| GatewayError::config(format!("invalid NCX_DREAMINA_API_ADDR: {e}")))?;
        let admin_addr = std::env::var("NCX_DREAMINA_ADMIN_ADDR")
            .unwrap_or_else(|_| DEFAULT_ADMIN_ADDR.to_string())
            .parse()
            .map_err(|e| GatewayError::config(format!("invalid NCX_DREAMINA_ADMIN_ADDR: {e}")))?;
        let state_path = std::env::var("NCX_DREAMINA_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".ncx-dreamina-gateway/state.json"));
        Ok(Self {
            api_addr,
            admin_addr,
            state_path,
        })
    }
}

#[derive(Debug)]
pub struct GatewayError {
    status: StatusCode,
    message: String,
}

impl GatewayError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GatewayError {}

impl IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                    "type": "ncx_dreamina_gateway_error"
                }
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderToken {
    pub id: String,
    pub label: String,
    pub sessionid: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKey {
    pub id: String,
    pub key: String,
    pub label: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayState {
    pub admin_password_hash: Option<String>,
    pub provider_tokens: Vec<ProviderToken>,
    pub api_keys: Vec<ApiKey>,
    pub next_token_index: usize,
}

impl Default for GatewayState {
    fn default() -> Self {
        Self {
            admin_password_hash: None,
            provider_tokens: Vec::new(),
            api_keys: vec![ApiKey {
                id: "local-dev-key".to_string(),
                key: "sk-local-dev".to_string(),
                label: "local development key".to_string(),
                created_at: now_unix(),
                last_used_at: None,
            }],
            next_token_index: 0,
        }
    }
}

impl GatewayState {
    pub fn verify_api_key(&mut self, raw: &str) -> bool {
        let key = raw.trim();
        if key.is_empty() {
            return false;
        }
        if let Some(found) = self.api_keys.iter_mut().find(|candidate| candidate.key == key) {
            found.last_used_at = Some(now_unix());
            return true;
        }
        false
    }

    pub fn verify_admin_password(&self, raw: &str) -> bool {
        match &self.admin_password_hash {
            Some(expected) => *expected == hash_secret(raw),
            None => true,
        }
    }

    pub fn add_provider_token(
        &mut self,
        label: Option<String>,
        sessionid: String,
    ) -> Result<SafeProviderToken, GatewayError> {
        let sessionid = sessionid.trim().to_string();
        if sessionid.len() < 12 {
            return Err(GatewayError::bad_request(
                "sessionid must be at least 12 characters",
            ));
        }
        if self.provider_tokens.len() >= 5 {
            return Err(GatewayError::bad_request(
                "local test pool supports up to 5 provider tokens",
            ));
        }
        if self.provider_tokens.iter().any(|item| item.sessionid == sessionid) {
            return Err(GatewayError::bad_request("provider token already exists"));
        }
        let item = ProviderToken {
            id: Uuid::new_v4().to_string(),
            label: label
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("account {}", self.provider_tokens.len() + 1)),
            sessionid,
            created_at: now_unix(),
            last_used_at: None,
        };
        let safe = SafeProviderToken::from(&item);
        self.provider_tokens.push(item);
        Ok(safe)
    }

    pub fn generate_api_key(&mut self, label: Option<String>) -> SafeApiKey {
        let key = format!("sk-ncx-{}", Uuid::new_v4().simple());
        let item = ApiKey {
            id: Uuid::new_v4().to_string(),
            key,
            label: label
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("client {}", self.api_keys.len() + 1)),
            created_at: now_unix(),
            last_used_at: None,
        };
        let safe = SafeApiKey::from(&item);
        self.api_keys.push(item);
        safe
    }

    pub fn pick_provider_token(&mut self) -> Option<ProviderToken> {
        if self.provider_tokens.is_empty() {
            return None;
        }
        let index = self.next_token_index % self.provider_tokens.len();
        self.next_token_index = (index + 1) % self.provider_tokens.len();
        self.provider_tokens[index].last_used_at = Some(now_unix());
        Some(self.provider_tokens[index].clone())
    }

    pub fn safe_snapshot(&self) -> SafeState {
        SafeState {
            admin_ready: self.admin_password_hash.is_some(),
            provider_tokens: self.provider_tokens.iter().map(SafeProviderToken::from).collect(),
            api_keys: self.api_keys.iter().map(SafeApiKey::from).collect(),
            models: built_in_models(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafeProviderToken {
    pub id: String,
    pub label: String,
    pub redacted_sessionid: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl From<&ProviderToken> for SafeProviderToken {
    fn from(value: &ProviderToken) -> Self {
        Self {
            id: value.id.clone(),
            label: value.label.clone(),
            redacted_sessionid: redact_secret(&value.sessionid),
            created_at: value.created_at,
            last_used_at: value.last_used_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafeApiKey {
    pub id: String,
    pub label: String,
    pub redacted_key: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl From<&ApiKey> for SafeApiKey {
    fn from(value: &ApiKey) -> Self {
        Self {
            id: value.id.clone(),
            label: value.label.clone(),
            redacted_key: redact_secret(&value.key),
            created_at: value.created_at,
            last_used_at: value.last_used_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SafeState {
    pub admin_ready: bool,
    pub provider_tokens: Vec<SafeProviderToken>,
    pub api_keys: Vec<SafeApiKey>,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    store: Arc<StateStore>,
}

impl AppState {
    pub async fn load(path: impl Into<PathBuf>) -> Result<Self, GatewayError> {
        let store = StateStore::load(path.into()).await?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

#[derive(Debug)]
struct StateStore {
    path: PathBuf,
    state: Mutex<GatewayState>,
}

impl StateStore {
    async fn load(path: PathBuf) -> Result<Self, GatewayError> {
        let state = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => serde_json::from_str(&raw)
                .map_err(|e| GatewayError::io(format!("invalid state file: {e}")))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => GatewayState::default(),
            Err(err) => return Err(GatewayError::io(format!("failed to read state: {err}"))),
        };
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }

    async fn save(&self, state: &GatewayState) -> Result<(), GatewayError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| GatewayError::io(format!("failed to create state dir: {e}")))?;
        }
        let body = serde_json::to_vec_pretty(state)
            .map_err(|e| GatewayError::io(format!("failed to serialize state: {e}")))?;
        tokio::fs::write(&self.path, body)
            .await
            .map_err(|e| GatewayError::io(format!("failed to write state: {e}")))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub owned_by: String,
}

pub fn built_in_models() -> Vec<ModelInfo> {
    [
        "jimeng-image-5.0-lite",
        "jimeng-image-4.7",
        "jimeng-image-4.6",
        "jimeng-image-4.5",
        "jimeng-image-4.1",
        "jimeng-image-4.0",
        "jimeng-image-3.1",
        "jimeng-image-3.0",
        "jimeng-image-2.0-pro",
        "jimeng-video-seedance-2.0-mini",
        "jimeng-video-seedance-2.0-fast",
        "jimeng-video-seedance-2.0-pro",
        "jimeng-video-seedance-1.5-pro",
        "jimeng-video-3.0-pro",
        "jimeng-video-3.0",
        "jimeng-video-3.0-fast",
        "jimeng-video-s2.0",
        "jimeng-video-2.0-pro",
    ]
    .into_iter()
    .map(|id| ModelInfo {
        id: id.to_string(),
        object: "model".to_string(),
        owned_by: "ncx-dreamina-gateway".to_string(),
    })
    .collect()
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct AddTokenRequest {
    password: Option<String>,
    label: Option<String>,
    sessionid: String,
}

#[derive(Debug, Deserialize)]
pub struct GenerateKeyRequest {
    password: Option<String>,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ImagesRequest {
    pub model: Option<String>,
    pub prompt: String,
    pub n: Option<u32>,
    pub size: Option<String>,
    pub response_format: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub stream: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Value,
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/v1/images/generations", post(images_generations))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub fn admin_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(admin_index))
        .route("/admin/status", get(admin_status))
        .route("/admin/setup", post(admin_setup))
        .route("/admin/tokens", post(admin_add_token))
        .route("/admin/api-keys", post(admin_generate_key))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "ncx-dreamina-gateway",
        "provider_mode": "mock"
    }))
}

async fn models() -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": built_in_models()
    }))
}

async fn images_generations(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ImagesRequest>,
) -> Result<Json<Value>, GatewayError> {
    authorize_api_key(&app, &headers).await?;
    let model = request
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_IMAGE_MODEL.to_string());
    if request.prompt.trim().is_empty() {
        return Err(GatewayError::bad_request("prompt must not be empty"));
    }
    let token = pick_token_label(&app).await?;
    let count = request.n.unwrap_or(1).clamp(1, 4);
    let created = now_unix();
    let mut data = Vec::new();
    for index in 0..count {
        let url = format!(
            "http://127.0.0.1/mock/dreamina/{}/{}.png",
            created,
            index + 1
        );
        if request.response_format.as_deref() == Some("b64_json") {
            data.push(json!({
                "b64_json": "",
                "revised_prompt": format_mock_prompt(&request.prompt, &model, &token),
            }));
        } else {
            data.push(json!({
                "url": url,
                "revised_prompt": format_mock_prompt(&request.prompt, &model, &token),
            }));
        }
    }
    Ok(Json(json!({
        "created": created,
        "data": data,
        "model": model,
        "size": request.size.unwrap_or_else(|| "1024x1024".to_string()),
        "provider_mode": "mock"
    })))
}

async fn chat_completions(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Json<Value>, GatewayError> {
    authorize_api_key(&app, &headers).await?;
    if request.stream.unwrap_or(false) {
        return Err(GatewayError::bad_request(
            "streaming is not implemented in the local mock gateway",
        ));
    }
    let model = request
        .model
        .unwrap_or_else(|| DEFAULT_IMAGE_MODEL.to_string());
    let prompt = extract_prompt(&request.messages);
    if prompt.trim().is_empty() {
        return Err(GatewayError::bad_request("messages must contain user text"));
    }
    let token = pick_token_label(&app).await?;
    let created = now_unix();
    let content = format!(
        "Mock Dreamina image generation accepted.\nmodel: {model}\naccount: {token}\nprompt: {prompt}\nmock_url: http://127.0.0.1/mock/dreamina/{created}/1.png"
    );
    Ok(Json(json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        "provider_mode": "mock"
    })))
}

async fn admin_index() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

async fn admin_status(State(app): State<AppState>) -> Json<SafeState> {
    let state = app.store.state.lock().await;
    Json(state.safe_snapshot())
}

async fn admin_setup(
    State(app): State<AppState>,
    Json(request): Json<SetupRequest>,
) -> Result<Json<Value>, GatewayError> {
    if request.password.trim().len() < 8 {
        return Err(GatewayError::bad_request(
            "admin password must be at least 8 characters",
        ));
    }
    let mut state = app.store.state.lock().await;
    if state.admin_password_hash.is_some() {
        return Err(GatewayError::bad_request("admin password is already set"));
    }
    state.admin_password_hash = Some(hash_secret(&request.password));
    app.store.save(&state).await?;
    Ok(Json(json!({"ok": true, "message": "admin password set"})))
}

async fn admin_add_token(
    State(app): State<AppState>,
    Json(request): Json<AddTokenRequest>,
) -> Result<Json<SafeProviderToken>, GatewayError> {
    let mut state = app.store.state.lock().await;
    require_admin(&state, request.password.as_deref())?;
    let safe = state.add_provider_token(request.label, request.sessionid)?;
    app.store.save(&state).await?;
    Ok(Json(safe))
}

async fn admin_generate_key(
    State(app): State<AppState>,
    Json(request): Json<GenerateKeyRequest>,
) -> Result<Json<Value>, GatewayError> {
    let mut state = app.store.state.lock().await;
    require_admin(&state, request.password.as_deref())?;
    let before = state.api_keys.len();
    let safe = state.generate_api_key(request.label);
    let full_key = state.api_keys[before].key.clone();
    app.store.save(&state).await?;
    Ok(Json(json!({
        "api_key": full_key,
        "safe": safe,
        "message": "copy api_key now; status views only show redacted keys"
    })))
}

fn require_admin(state: &GatewayState, password: Option<&str>) -> Result<(), GatewayError> {
    if !state.verify_admin_password(password.unwrap_or("")) {
        return Err(GatewayError::unauthorized("invalid admin password"));
    }
    Ok(())
}

async fn authorize_api_key(app: &AppState, headers: &HeaderMap) -> Result<(), GatewayError> {
    let token = bearer_token(headers)
        .ok_or_else(|| GatewayError::unauthorized("missing Authorization: Bearer sk-..."))?;
    let mut state = app.store.state.lock().await;
    if state.verify_api_key(&token) {
        app.store.save(&state).await?;
        return Ok(());
    }
    Err(GatewayError::unauthorized("invalid API key"))
}

async fn pick_token_label(app: &AppState) -> Result<String, GatewayError> {
    let mut state = app.store.state.lock().await;
    let label = state
        .pick_provider_token()
        .map(|token| token.label)
        .unwrap_or_else(|| "mock-without-provider-token".to_string());
    app.store.save(&state).await?;
    Ok(label)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.trim().strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_prompt(messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    for message in messages {
        if message.role != "user" {
            continue;
        }
        match &message.content {
            Value::String(text) => parts.push(text.clone()),
            Value::Array(items) => {
                for item in items {
                    if let Some(text) = item
                        .get("text")
                        .and_then(|value| value.as_str())
                        .or_else(|| item.get("content").and_then(|value| value.as_str()))
                    {
                        parts.push(text.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    parts.join("\n")
}

fn format_mock_prompt(prompt: &str, model: &str, token_label: &str) -> String {
    format!(
        "[local mock] model={model}; provider_account={token_label}; prompt={}",
        prompt.trim()
    )
}

pub fn redact_secret(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "(empty)".to_string();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}****{tail}")
}

pub fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn remove_state_file(path: &Path) -> Result<(), GatewayError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(GatewayError::io(format!(
            "failed to remove state file: {err}"
        ))),
    }
}

const ADMIN_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>NCX Dreamina Gateway</title>
  <style>
    body { margin: 0; font-family: "Microsoft YaHei", system-ui, sans-serif; background: #f8f3e9; color: #232018; }
    main { width: min(980px, calc(100% - 32px)); margin: 40px auto; }
    section { background: #fffdf7; border: 1px solid #eadfc9; border-radius: 22px; padding: 22px; margin: 18px 0; box-shadow: 0 20px 50px rgba(70, 55, 30, .12); }
    h1 { font-size: 34px; margin: 0 0 8px; }
    h2 { margin: 0 0 14px; }
    label { display: block; margin: 10px 0 6px; font-weight: 700; }
    input { width: 100%; padding: 11px 12px; border: 1px solid #d9ccb7; border-radius: 12px; font-size: 15px; }
    button { margin-top: 12px; padding: 10px 14px; border: 0; border-radius: 12px; background: #1f7a5b; color: #fff; font-weight: 800; cursor: pointer; }
    pre { white-space: pre-wrap; background: #242016; color: #fff7e6; border-radius: 14px; padding: 14px; overflow: auto; }
    .hint { color: #71685c; }
    .danger { border-left: 5px solid #bf6a2a; padding-left: 12px; }
  </style>
</head>
<body>
<main>
  <h1>NCX Dreamina Gateway</h1>
  <p class="hint">本地测试控制台。默认 provider 是 mock，不会自动抓取浏览器 cookie，也不会向 Dreamina 发送真实请求。</p>
  <section>
    <h2>1. 首次设置管理员密码</h2>
    <label>管理员密码</label>
    <input id="setupPassword" type="password" placeholder="至少 8 位" />
    <button onclick="setupAdmin()">设置密码</button>
  </section>
  <section>
    <h2>2. 添加手动获取的 sessionid</h2>
    <p class="hint danger">只粘贴你自己账号、你有权使用的 sessionid。页面只展示脱敏值，状态文件会保存在本机。</p>
    <label>管理员密码</label>
    <input id="adminPassword" type="password" />
    <label>账号标签</label>
    <input id="tokenLabel" placeholder="account 1" />
    <label>sessionid</label>
    <input id="sessionid" placeholder="手动粘贴 sessionid，用于后续真实 adapter；mock 模式不会外发" />
    <button onclick="addToken()">添加账号</button>
  </section>
  <section>
    <h2>3. 生成 OpenAI 兼容 API Key</h2>
    <label>Key 标签</label>
    <input id="keyLabel" placeholder="NextChat local" />
    <button onclick="generateKey()">生成 sk-...</button>
  </section>
  <section>
    <h2>状态</h2>
    <button onclick="refreshStatus()">刷新状态</button>
    <pre id="status">(loading)</pre>
  </section>
</main>
<script>
async function postJson(url, body) {
  const res = await fetch(url, { method: "POST", headers: {"content-type": "application/json"}, body: JSON.stringify(body) });
  const data = await res.json();
  if (!res.ok) throw new Error(JSON.stringify(data, null, 2));
  return data;
}
async function setupAdmin() {
  try {
    const data = await postJson("/admin/setup", { password: document.getElementById("setupPassword").value });
    alert(data.message || "ok");
    refreshStatus();
  } catch (err) { alert(err.message); }
}
async function addToken() {
  try {
    const data = await postJson("/admin/tokens", {
      password: document.getElementById("adminPassword").value,
      label: document.getElementById("tokenLabel").value,
      sessionid: document.getElementById("sessionid").value
    });
    alert("added " + data.redacted_sessionid);
    refreshStatus();
  } catch (err) { alert(err.message); }
}
async function generateKey() {
  try {
    const data = await postJson("/admin/api-keys", {
      password: document.getElementById("adminPassword").value,
      label: document.getElementById("keyLabel").value
    });
    alert("API Key: " + data.api_key);
    refreshStatus();
  } catch (err) { alert(err.message); }
}
async function refreshStatus() {
  const res = await fetch("/admin/status");
  document.getElementById("status").textContent = JSON.stringify(await res.json(), null, 2);
}
refreshStatus();
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_without_leaking_middle() {
        assert_eq!(redact_secret("abcdefghijklmnop"), "abcd****mnop");
        assert_eq!(redact_secret("short"), "****");
    }

    #[test]
    fn default_state_has_local_dev_key() {
        let mut state = GatewayState::default();
        assert!(state.verify_api_key("sk-local-dev"));
        assert!(!state.verify_api_key("sk-nope"));
    }

    #[test]
    fn token_pool_round_robins() {
        let mut state = GatewayState::default();
        state
            .add_provider_token(Some("a".into()), "sessionid-aaaa".into())
            .unwrap();
        state
            .add_provider_token(Some("b".into()), "sessionid-bbbb".into())
            .unwrap();
        assert_eq!(state.pick_provider_token().unwrap().label, "a");
        assert_eq!(state.pick_provider_token().unwrap().label, "b");
        assert_eq!(state.pick_provider_token().unwrap().label, "a");
    }

    #[test]
    fn extracts_openai_multimodal_text() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: json!([
                {"type": "text", "text": "draw a cat"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
            ]),
        }];
        assert_eq!(extract_prompt(&messages), "draw a cat");
    }

    #[test]
    fn model_list_contains_requested_jimeng_image_3() {
        assert!(built_in_models()
            .iter()
            .any(|model| model.id == "jimeng-image-3.0"));
    }
}
