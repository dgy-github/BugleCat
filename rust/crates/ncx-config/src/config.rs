//! Config struct, defaults, and validation — Rust port of the `Config` dataclass
//! in `nanocodex/config.py`.

use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
pub const DEFAULT_MODEL: &str = "deepseek-chat";
pub const DEFAULT_MODELS: &[&str] = &["deepseek-v4-pro", "deepseek-chat", "deepseek-reasoner"];

pub const VALID_SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
pub const VALID_APPROVAL_POLICIES: &[&str] = &["untrusted", "on-failure", "on-request", "never"];

/// Resolved runtime configuration — mirrors the Python `Config` dataclass.
///
/// The API key is never logged or printed; use [`Config::redacted`] for
/// display-safe snapshots.
#[derive(Debug, Clone)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    /// Optional cheaper/faster model for sub-agent workers (flash+pro tiering).
    /// Empty = sub-agents use `model`. Shares base_url/api_key with the main model.
    pub fast_model: String,
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub reasoning_effort: String,
    /// Vision endpoint for image-bearing turns (empty = same vendor as main model).
    pub vl_base_url: String,
    pub vl_api_key: String,
    pub vl_model: String,
    /// Volcengine ARK key for Seedance video rendering (storyboard).
    pub ark_api_key: String,
    /// Web search backend: "duckduckgo" (default, keyless) or "tavily".
    pub search_provider: String,
    /// API key for the keyed search backend (Tavily). Empty = fall back to DDG.
    pub search_api_key: String,
    pub workspace: PathBuf,
    pub writable_roots: Vec<PathBuf>,
    pub network_access: bool,
    pub max_iterations: i64,
    pub timeout_s: i64,
    /// SDK retry count for transient errors (408/409/429/5xx); default 3.
    pub max_retries: i64,
    pub context_token_budget: i64,
    pub context_window: i64,
    pub available_models: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            api_key: String::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            fast_model: String::new(),
            sandbox_mode: "workspace-write".to_string(),
            approval_policy: "on-request".to_string(),
            reasoning_effort: "auto".to_string(),
            vl_base_url: String::new(),
            vl_api_key: String::new(),
            vl_model: String::new(),
            ark_api_key: String::new(),
            search_provider: "duckduckgo".to_string(),
            search_api_key: String::new(),
            workspace: std::env::current_dir().unwrap_or_default(),
            writable_roots: vec![],
            network_access: false,
            max_iterations: 60,
            timeout_s: 120,
            max_retries: 3,
            context_token_budget: 512_000,
            context_window: 1_048_576,
            available_models: DEFAULT_MODELS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Config {
    /// Validate required/enum fields; returns `ConfigError` on first violation.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.api_key.is_empty() {
            return Err(ConfigError(
                "No API key found. Set DEEPSEEK_API_KEY or add api_key to \
                 ~/.nanocodex/config.toml."
                    .to_string(),
            ));
        }
        if !VALID_SANDBOX_MODES.contains(&self.sandbox_mode.as_str()) {
            return Err(ConfigError(format!(
                "Invalid sandbox_mode {:?}; expected one of {:?}.",
                self.sandbox_mode, VALID_SANDBOX_MODES
            )));
        }
        if !VALID_APPROVAL_POLICIES.contains(&self.approval_policy.as_str()) {
            return Err(ConfigError(format!(
                "Invalid approval_policy {:?}; expected one of {:?}.",
                self.approval_policy, VALID_APPROVAL_POLICIES
            )));
        }
        Ok(())
    }

    /// Display-safe snapshot: API keys are masked to `****<last4>`.
    pub fn redacted(&self) -> HashMap<&'static str, String> {
        let mask = |key: &str| -> String {
            if key.is_empty() {
                return "(unset)".to_string();
            }
            let tail = if key.len() >= 4 {
                &key[key.len() - 4..]
            } else {
                ""
            };
            format!("****{tail}")
        };
        let mut m = HashMap::new();
        m.insert("api_key", mask(&self.api_key));
        m.insert("base_url", self.base_url.clone());
        m.insert("model", self.model.clone());
        m.insert("sandbox_mode", self.sandbox_mode.clone());
        m.insert("approval_policy", self.approval_policy.clone());
        m.insert("reasoning_effort", self.reasoning_effort.clone());
        m.insert("vl_base_url", self.vl_base_url.clone());
        m.insert("vl_api_key", mask(&self.vl_api_key));
        m.insert("vl_model", self.vl_model.clone());
        m.insert("ark_api_key", mask(&self.ark_api_key));
        m.insert("workspace", self.workspace.to_string_lossy().to_string());
        m.insert("max_iterations", self.max_iterations.to_string());
        m.insert("timeout_s", self.timeout_s.to_string());
        m.insert("max_retries", self.max_retries.to_string());
        m
    }
}

/// Configuration error — mirrors `ConfigError(RuntimeError)` in Python.
#[derive(Debug, Clone)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}
