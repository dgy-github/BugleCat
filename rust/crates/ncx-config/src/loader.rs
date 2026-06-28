//! Configuration loader — Rust port of the layered resolution in `load_config()`
//! from `nanocodex/config.py`.
//!
//! The loader avoids reading `std::env` directly so tests can inject a fake env
//! map without mutating process-global state.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use toml::map::Map as TomlMap;
use toml::Value;

use crate::config::{
    derive_permission_mode, Config, ConfigError, HookConfig, DEFAULT_BASE_URL, DEFAULT_MODEL,
    DEFAULT_MODELS, VALID_PERMISSION_MODES,
};

type Table = TomlMap<String, Value>;

// ── path defaults ─────────────────────────────────────────────────────────────

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Paths to the three config files.  Override in tests by constructing directly.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub deepseek: PathBuf,
    pub codex: PathBuf,
    pub nanocodex: PathBuf,
}

impl Default for ConfigPaths {
    fn default() -> Self {
        let home = home_dir();
        ConfigPaths {
            deepseek: home.join(".deepseek/config.toml"),
            codex: home.join(".codex/config.toml"),
            nanocodex: home.join(".nanocodex/config.toml"),
        }
    }
}

// ── override struct ───────────────────────────────────────────────────────────

/// Explicit overrides from the CLI (or tests).  `None` means "not set".
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub workspace: Option<PathBuf>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub fast_model: Option<String>,
    pub sandbox_mode: Option<String>,
    pub approval_policy: Option<String>,
    pub reasoning_effort: Option<String>,
    pub vl_base_url: Option<String>,
    pub vl_api_key: Option<String>,
    pub vl_model: Option<String>,
    pub ark_api_key: Option<String>,
    pub max_iterations: Option<i64>,
    pub max_tool_calls: Option<i64>,
    pub max_retries: Option<i64>,
    pub context_token_budget: Option<i64>,
    pub context_window: Option<i64>,
    pub context_edit_enabled: Option<bool>,
    pub context_edit_max_chars: Option<i64>,
    pub context_edit_keep_recent_messages: Option<i64>,
    pub context_edit_max_tool_result_chars: Option<i64>,
    pub available_models: Option<Vec<String>>,
    pub profile: Option<String>,
}

// ── TOML helpers ──────────────────────────────────────────────────────────────

fn load_toml(path: &Path) -> Table {
    if !path.is_file() {
        return Table::new();
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Table::new(),
    };
    match text.parse::<Value>() {
        Ok(Value::Table(t)) => t,
        _ => Table::new(),
    }
}

fn str_val(t: &Table, key: &str) -> Option<String> {
    t.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Coerce a TOML value to a non-empty String (strings, bools, ints).
fn to_string_val(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Integer(i) => Some(i.to_string()),
        _ => None,
    }
}

// ── per-file extractors ───────────────────────────────────────────────────────

/// Extract known fields from `~/.deepseek/config.toml`.
fn deepseek_values(raw: &Table) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if raw.is_empty() {
        return out;
    }
    if let Some(v) = str_val(raw, "base_url") {
        out.insert("base_url".into(), v);
    }
    // DeepSeek-CLI uses `default_text_model` for the chat model.
    if let Some(v) = str_val(raw, "default_text_model") {
        out.insert("model".into(), v);
    } else if let Some(v) = str_val(raw, "model") {
        out.insert("model".into(), v);
    }
    for key in &["sandbox_mode", "approval_policy", "reasoning_effort"] {
        if let Some(v) = str_val(raw, key) {
            out.insert(key.to_string(), v);
        }
    }
    // API key: top-level or nested under providers.deepseek.api_key.
    let api_key = str_val(raw, "api_key").or_else(|| {
        raw.get("providers")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("deepseek"))
            .and_then(|v| v.as_table())
            .and_then(|t| str_val(t, "api_key"))
    });
    if let Some(k) = api_key {
        out.insert("api_key".into(), k);
    }
    out
}

/// Extract settings from `~/.nanocodex/config.toml` (flat, keys == Config fields).
fn nanocodex_values(raw: &Table) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in &[
        "api_key",
        "base_url",
        "model",
        "fast_model",
        "sandbox_mode",
        "approval_policy",
        "permission_mode",
        "reasoning_effort",
        "vl_base_url",
        "vl_api_key",
        "vl_model",
        "ark_api_key",
        "search_provider",
        "search_api_key",
        "max_iterations",
        "max_tool_calls",
        "max_retries",
        "context_token_budget",
        "context_window",
        "context_edit_enabled",
        "context_edit_max_chars",
        "context_edit_keep_recent_messages",
        "context_edit_max_tool_result_chars",
        "price_in",
        "price_out",
    ] {
        if let Some(v) = selected_scalar(raw, key) {
            out.insert(key.to_string(), v);
        }
    }
    out
}

/// Extract Codex-style settings from `~/.codex/config.toml`.
fn codex_values(raw: &Table) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(v) = str_val(raw, "model") {
        out.insert("model".into(), v);
    }
    if let Some(v) = str_val(raw, "approval_policy") {
        out.insert("approval_policy".into(), v);
    }
    if let Some(v) = str_val(raw, "sandbox_mode") {
        out.insert("sandbox_mode".into(), v);
    }
    if let Some(v) = str_val(raw, "model_reasoning_effort") {
        out.insert("reasoning_effort".into(), v);
    }
    out
}

/// Pull profile-able keys out of a `[profiles.<name>]` TOML table.
const PROFILE_KEYS: &[&str] = &[
    "model",
    "fast_model",
    "base_url",
    "sandbox_mode",
    "approval_policy",
    "reasoning_effort",
    "vl_base_url",
    "vl_api_key",
    "vl_model",
    "ark_api_key",
    "max_iterations",
    "max_tool_calls",
    "max_retries",
    "context_token_budget",
    "context_window",
    "context_edit_enabled",
    "context_edit_max_chars",
    "context_edit_keep_recent_messages",
    "context_edit_max_tool_result_chars",
];

fn profile_values(selected: &Table) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in PROFILE_KEYS {
        if let Some(v) = selected.get(*key).and_then(to_string_val) {
            out.insert(key.to_string(), v);
        }
    }
    out
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn as_int(s: Option<&str>, default: i64) -> i64 {
    s.and_then(|v| v.parse::<i64>().ok()).unwrap_or(default)
}

fn as_float(s: Option<&str>, default: f64) -> f64 {
    s.and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(default)
}

fn as_bool(s: Option<&str>, default: bool) -> bool {
    match s.map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if matches!(v.as_str(), "true" | "1" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "false" | "0" | "no" | "off") => false,
        _ => default,
    }
}

fn selected_scalar(raw: &Table, key: &str) -> Option<String> {
    raw.get(key).and_then(to_string_val)
}

fn parse_hooks(raw: &Table) -> Vec<HookConfig> {
    raw.get("hooks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_table())
                .map(|table| {
                    let command = str_val(table, "command").unwrap_or_default();
                    let event = str_val(table, "event")
                        .map(|e| normalize_hook_event(&e))
                        .unwrap_or_else(|| "pre_tool".into());
                    HookConfig {
                        event,
                        matcher: str_val(table, "matcher").unwrap_or_else(|| "*".into()),
                        command,
                        timeout_s: table
                            .get("timeout_s")
                            .and_then(|v| v.as_integer())
                            .unwrap_or(10),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_hook_event(event: &str) -> String {
    match event.trim() {
        "PreToolUse" | "pre_tool_use" | "pre_tool" => "pre_tool".into(),
        "PostToolUse" | "post_tool_use" | "post_tool" => "post_tool".into(),
        "UserPromptSubmit" | "user_prompt_submit" | "user_prompt" => "user_prompt".into(),
        "Stop" | "stop" => "stop".into(),
        other => other.to_string(),
    }
}

/// Build the model-switcher list: active model first, then extras, deduped.
fn model_list(csv: Option<&str>, active: &str) -> Vec<String> {
    let names: Vec<String> = match csv {
        Some(s) => {
            let v: Vec<String> = s
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if v.is_empty() {
                DEFAULT_MODELS.iter().map(|s| s.to_string()).collect()
            } else {
                v
            }
        }
        None => DEFAULT_MODELS.iter().map(|s| s.to_string()).collect(),
    };

    let mut ordered = vec![active.to_string()];
    for n in &names {
        if n != active {
            ordered.push(n.clone());
        }
    }
    // deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    ordered.retain(|n| !n.is_empty() && seen.insert(n.clone()));
    ordered
}

// ── public API ────────────────────────────────────────────────────────────────

/// Names of `[profiles.<name>]` tables defined at `nanocodex_path`.
pub fn list_profiles_at(nanocodex_path: &Path) -> Vec<String> {
    let raw = load_toml(nanocodex_path);
    let Some(Value::Table(profiles)) = raw.get("profiles") else {
        return vec![];
    };
    let mut names: Vec<String> = profiles.keys().cloned().collect();
    names.sort();
    names
}

/// Names of `[profiles.<name>]` tables in `~/.nanocodex/config.toml`.
pub fn list_profiles() -> Vec<String> {
    list_profiles_at(&ConfigPaths::default().nanocodex)
}

// ── MCP server config ─────────────────────────────────────────────────────────

/// Load MCP server definitions from a `mcp.toml` file.
///
/// Format:
/// ```toml
/// [[servers]]
/// name    = "everything"
/// command = "npx"
/// args    = ["-y", "@modelcontextprotocol/server-everything"]
/// env     = { MY_VAR = "value" }   # optional
/// ```
pub fn load_mcp_servers_at(path: &Path) -> Vec<crate::config::McpServerConfig> {
    use crate::config::McpServerConfig;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let parsed: Value = match text.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.get("servers").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for s in arr {
        let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let command = s.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() || command.is_empty() {
            continue;
        }
        let args: Vec<String> = s
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let env: HashMap<String, String> = s
            .get("env")
            .and_then(|v| v.as_table())
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|val| (k.clone(), val.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        out.push(McpServerConfig { name, command, args, env });
    }
    out
}

/// Load MCP server definitions from `~/.nanocodex/mcp.toml`.
pub fn load_mcp_servers() -> Vec<crate::config::McpServerConfig> {
    load_mcp_servers_at(&home_dir().join(".nanocodex/mcp.toml"))
}

/// Resolve a [`Config`] using real env vars and default config-file paths.
pub fn load_config(overrides: Overrides) -> Result<Config, ConfigError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    load_config_impl(overrides, &ConfigPaths::default(), &env)
}

/// Resolve a [`Config`] with injectable paths (and real env vars).
pub fn load_config_with_paths(
    overrides: Overrides,
    paths: &ConfigPaths,
) -> Result<Config, ConfigError> {
    let env: HashMap<String, String> = std::env::vars().collect();
    load_config_impl(overrides, paths, &env)
}

/// Core loader — injectable for tests (fake env map, fake paths).
pub(crate) fn load_config_impl(
    overrides: Overrides,
    paths: &ConfigPaths,
    env: &HashMap<String, String>,
) -> Result<Config, ConfigError> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    merged.insert("base_url".into(), DEFAULT_BASE_URL.into());
    merged.insert("model".into(), DEFAULT_MODEL.into());

    // Lowest-priority layers (nanocodex wins over deepseek wins over codex).
    let nano_raw = load_toml(&paths.nanocodex);
    merged.extend(codex_values(&load_toml(&paths.codex)));
    merged.extend(deepseek_values(&load_toml(&paths.deepseek)));
    merged.extend(nanocodex_values(&nano_raw));

    // Profile: above files, below env/CLI.
    let prof_name = overrides
        .profile
        .clone()
        .or_else(|| env.get("NANOCODEX_PROFILE").cloned())
        .or_else(|| str_val(&nano_raw, "profile"));
    if let Some(name) = &prof_name {
        let profiles = nano_raw.get("profiles").and_then(|v| v.as_table());
        let selected = profiles
            .and_then(|t| t.get(name))
            .and_then(|v| v.as_table());
        let Some(table) = selected else {
            let available = profiles
                .map(|t| {
                    let mut ks: Vec<&str> = t.keys().map(|s| s.as_str()).collect();
                    ks.sort();
                    ks.join(", ")
                })
                .unwrap_or_else(|| "(none)".into());
            return Err(ConfigError(format!(
                "Profile {name:?} not found in nanocodex config. \
                 Available profiles: {available}."
            )));
        };
        merged.extend(profile_values(table));
    }

    // Environment variable layer.
    let env_map: &[(&str, &[&str])] = &[
        ("api_key", &["DEEPSEEK_API_KEY", "NANOCODEX_API_KEY"]),
        ("base_url", &["DEEPSEEK_BASE_URL", "NANOCODEX_BASE_URL"]),
        ("model", &["NANOCODEX_MODEL"]),
        ("fast_model", &["NANOCODEX_FAST_MODEL"]),
        ("vl_base_url", &["NANOCODEX_VL_BASE_URL"]),
        ("vl_api_key", &["DASHSCOPE_API_KEY", "NANOCODEX_VL_API_KEY"]),
        ("vl_model", &["NANOCODEX_VL_MODEL"]),
        ("ark_api_key", &["ARK_API_KEY", "NANOCODEX_ARK_API_KEY"]),
        ("search_provider", &["NANOCODEX_SEARCH_PROVIDER"]),
        (
            "search_api_key",
            &["TAVILY_API_KEY", "NANOCODEX_SEARCH_API_KEY"],
        ),
        ("sandbox_mode", &["NANOCODEX_SANDBOX"]),
        ("approval_policy", &["NANOCODEX_APPROVAL"]),
        ("permission_mode", &["NANOCODEX_PERMISSION_MODE"]),
        ("context_token_budget", &["NANOCODEX_CONTEXT_BUDGET"]),
        ("context_window", &["NANOCODEX_CONTEXT_WINDOW"]),
        ("context_edit_enabled", &["NANOCODEX_CONTEXT_EDIT_ENABLED"]),
        (
            "context_edit_max_chars",
            &["NANOCODEX_CONTEXT_EDIT_MAX_CHARS"],
        ),
        (
            "context_edit_keep_recent_messages",
            &["NANOCODEX_CONTEXT_EDIT_KEEP_RECENT"],
        ),
        (
            "context_edit_max_tool_result_chars",
            &["NANOCODEX_CONTEXT_EDIT_TOOL_RESULT_CHARS"],
        ),
        ("available_models", &["NANOCODEX_MODELS"]),
        ("max_iterations", &["NANOCODEX_MAX_ITERATIONS"]),
        ("max_tool_calls", &["NANOCODEX_MAX_TOOL_CALLS"]),
        ("max_retries", &["NANOCODEX_MAX_RETRIES"]),
    ];
    for (field, env_keys) in env_map {
        for env_key in *env_keys {
            if let Some(v) = env.get(*env_key).filter(|v| !v.is_empty()) {
                merged.insert(field.to_string(), v.clone());
                break;
            }
        }
    }

    // Explicit overrides (highest priority).
    macro_rules! apply_str {
        ($field:ident) => {
            if let Some(v) = overrides.$field {
                merged.insert(stringify!($field).to_string(), v);
            }
        };
    }
    macro_rules! apply_int {
        ($field:ident) => {
            if let Some(v) = overrides.$field {
                merged.insert(stringify!($field).to_string(), v.to_string());
            }
        };
    }
    macro_rules! apply_bool {
        ($field:ident) => {
            if let Some(v) = overrides.$field {
                merged.insert(stringify!($field).to_string(), v.to_string());
            }
        };
    }
    apply_str!(api_key);
    apply_str!(base_url);
    apply_str!(model);
    apply_str!(fast_model);
    apply_str!(sandbox_mode);
    apply_str!(approval_policy);
    apply_str!(reasoning_effort);
    apply_str!(vl_base_url);
    apply_str!(vl_api_key);
    apply_str!(vl_model);
    apply_str!(ark_api_key);
    apply_int!(max_iterations);
    apply_int!(max_tool_calls);
    apply_int!(max_retries);
    apply_int!(context_token_budget);
    apply_int!(context_window);
    apply_bool!(context_edit_enabled);
    apply_int!(context_edit_max_chars);
    apply_int!(context_edit_keep_recent_messages);
    apply_int!(context_edit_max_tool_result_chars);
    if let Some(models) = overrides.available_models {
        merged.insert("available_models".into(), models.join(","));
    }

    let active_model = merged
        .get("model")
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_MODEL)
        .to_string();
    let sandbox_mode = merged
        .get("sandbox_mode")
        .cloned()
        .unwrap_or_else(|| "workspace-write".into());
    let network_access = sandbox_mode == "danger-full-access";
    // permission_mode: use the stored value if valid, else migrate from the
    // legacy sandbox_mode so pre-existing configs keep their behavior.
    let permission_mode = merged
        .get("permission_mode")
        .filter(|m| VALID_PERMISSION_MODES.contains(&m.as_str()))
        .cloned()
        .unwrap_or_else(|| derive_permission_mode(&sandbox_mode).to_string());

    let workspace_base = overrides
        .workspace
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let workspace = workspace_base.canonicalize().unwrap_or(workspace_base);

    let cfg = Config {
        api_key: merged.get("api_key").cloned().unwrap_or_default(),
        base_url: merged
            .get("base_url")
            .cloned()
            .unwrap_or_else(|| DEFAULT_BASE_URL.into()),
        model: active_model.clone(),
        fast_model: merged.get("fast_model").cloned().unwrap_or_default(),
        sandbox_mode,
        approval_policy: merged
            .get("approval_policy")
            .cloned()
            .unwrap_or_else(|| "on-request".into()),
        permission_mode,
        reasoning_effort: merged
            .get("reasoning_effort")
            .cloned()
            .unwrap_or_else(|| "auto".into()),
        vl_base_url: merged.get("vl_base_url").cloned().unwrap_or_default(),
        vl_api_key: merged.get("vl_api_key").cloned().unwrap_or_default(),
        vl_model: merged.get("vl_model").cloned().unwrap_or_default(),
        ark_api_key: merged.get("ark_api_key").cloned().unwrap_or_default(),
        search_provider: merged
            .get("search_provider")
            .cloned()
            .unwrap_or_else(|| "duckduckgo".into()),
        search_api_key: merged.get("search_api_key").cloned().unwrap_or_default(),
        workspace,
        writable_roots: vec![],
        network_access,
        max_iterations: as_int(merged.get("max_iterations").map(|s| s.as_str()), 60),
        max_tool_calls: as_int(merged.get("max_tool_calls").map(|s| s.as_str()), 120),
        timeout_s: 120,
        max_retries: as_int(merged.get("max_retries").map(|s| s.as_str()), 3),
        context_token_budget: as_int(
            merged.get("context_token_budget").map(|s| s.as_str()),
            512_000,
        ),
        context_window: as_int(merged.get("context_window").map(|s| s.as_str()), 1_048_576),
        context_edit_enabled: as_bool(merged.get("context_edit_enabled").map(|s| s.as_str()), true),
        context_edit_max_chars: as_int(
            merged.get("context_edit_max_chars").map(|s| s.as_str()),
            120_000,
        ),
        context_edit_keep_recent_messages: as_int(
            merged
                .get("context_edit_keep_recent_messages")
                .map(|s| s.as_str()),
            30,
        ),
        context_edit_max_tool_result_chars: as_int(
            merged
                .get("context_edit_max_tool_result_chars")
                .map(|s| s.as_str()),
            4_000,
        ),
        available_models: model_list(
            merged.get("available_models").map(|s| s.as_str()),
            &active_model,
        ),
        price_in: as_float(merged.get("price_in").map(|s| s.as_str()), 0.0),
        price_out: as_float(merged.get("price_out").map(|s| s.as_str()), 0.0),
        hooks: parse_hooks(&nano_raw),
        mcp_servers: vec![],
    };
    Ok(cfg)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn empty_env() -> HashMap<String, String> {
        HashMap::new()
    }

    fn env1(k: &str, v: &str) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(k.to_string(), v.to_string());
        m
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn no_paths(tmp: &Path) -> ConfigPaths {
        ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: tmp.join("nope-nano.toml"),
        }
    }

    #[test]
    fn config_redacts_api_key() {
        let cfg = Config {
            api_key: "sk-abcdef123456".into(),
            base_url: "u".into(),
            model: "m".into(),
            ..Config::default()
        };
        let red = cfg.redacted();
        assert_eq!(red["api_key"], "****3456");
        assert!(!red.values().any(|v| v.contains("abcdef")));
    }

    #[test]
    fn validate_rejects_bad_sandbox_mode() {
        let cfg = Config {
            api_key: "k".into(),
            sandbox_mode: "banana".into(),
            ..Config::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("sandbox_mode"));
    }

    #[test]
    fn validate_rejects_missing_key() {
        let cfg = Config::default();
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("API key"));
    }

    #[test]
    fn compaction_defaults_on_with_1m_window() {
        let cfg = Config::default();
        assert!(cfg.context_token_budget > 0);
        assert_eq!(cfg.context_token_budget, 512_000);
        assert_eq!(cfg.context_window, 1_048_576);
    }

    #[test]
    fn load_reads_deepseek_file() {
        let tmp = std::env::temp_dir().join("ncx_config_test_deepseek");
        fs::create_dir_all(&tmp).unwrap();
        let ds = tmp.join("deepseek.toml");
        write(
            &ds,
            r#"
api_key = "sk-fromfile"
base_url = "https://api.deepseek.com/beta"
default_text_model = "deepseek-v4-pro"
sandbox_mode = "workspace-write"
approval_policy = "on-request"
"#,
        );
        let paths = ConfigPaths {
            deepseek: ds,
            codex: tmp.join("nope.toml"),
            nanocodex: tmp.join("nope-nano.toml"),
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        cfg.validate().unwrap();
        assert_eq!(cfg.api_key, "sk-fromfile");
        assert_eq!(cfg.base_url, "https://api.deepseek.com/beta");
        assert_eq!(cfg.model, "deepseek-v4-pro");
    }

    #[test]
    fn overrides_win_over_file() {
        let tmp = std::env::temp_dir().join("ncx_config_test_override");
        fs::create_dir_all(&tmp).unwrap();
        let ds = tmp.join("deepseek.toml");
        write(
            &ds,
            "api_key = \"k\"\ndefault_text_model = \"deepseek-v4-pro\"\n",
        );
        let paths = ConfigPaths {
            deepseek: ds,
            codex: tmp.join("nope.toml"),
            nanocodex: tmp.join("nope-nano.toml"),
        };
        let ovr = Overrides {
            workspace: Some(tmp.clone()),
            model: Some("deepseek-chat".into()),
            sandbox_mode: Some("read-only".into()),
            ..Default::default()
        };
        let cfg = load_config_impl(ovr, &paths, &empty_env()).unwrap();
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(cfg.sandbox_mode, "read-only");
    }

    #[test]
    fn deepseek_nested_provider_key() {
        let tmp = std::env::temp_dir().join("ncx_config_test_nested");
        fs::create_dir_all(&tmp).unwrap();
        let ds = tmp.join("deepseek.toml");
        write(
            &ds,
            "base_url = \"u\"\n[providers.deepseek]\napi_key = \"sk-nested\"\n",
        );
        let paths = ConfigPaths {
            deepseek: ds,
            codex: tmp.join("nope.toml"),
            nanocodex: tmp.join("nope-nano.toml"),
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        assert_eq!(cfg.api_key, "sk-nested");
    }

    #[test]
    fn max_iterations_default_and_override() {
        let tmp = std::env::temp_dir().join("ncx_config_test_maxiter");
        fs::create_dir_all(&tmp).unwrap();
        let paths = no_paths(&tmp);

        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        assert_eq!(cfg.max_iterations, 60);

        let cfg2 = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                max_iterations: Some(100),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        assert_eq!(cfg2.max_iterations, 100);
    }

    #[test]
    fn max_iterations_from_env() {
        let tmp = std::env::temp_dir().join("ncx_config_test_maxiter_env");
        fs::create_dir_all(&tmp).unwrap();
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &no_paths(&tmp),
            &env1("NANOCODEX_MAX_ITERATIONS", "80"),
        )
        .unwrap();
        assert_eq!(cfg.max_iterations, 80);
    }

    #[test]
    fn runtime_budget_and_context_edit_fields_load_from_file_env_and_overrides() {
        let tmp = std::env::temp_dir().join("ncx_config_test_runtime_control");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            concat!(
                "api_key = \"sk-base\"\n",
                "max_tool_calls = 33\n",
                "context_edit_enabled = false\n",
                "context_edit_max_chars = 9000\n",
                "context_edit_keep_recent_messages = 11\n",
                "context_edit_max_tool_result_chars = 700\n",
            ),
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };
        let mut env = HashMap::new();
        env.insert("NANOCODEX_MAX_TOOL_CALLS".into(), "44".into());
        env.insert("NANOCODEX_CONTEXT_EDIT_ENABLED".into(), "true".into());

        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                context_edit_max_chars: Some(12_345),
                ..Default::default()
            },
            &paths,
            &env,
        )
        .unwrap();

        assert_eq!(cfg.max_tool_calls, 44);
        assert!(cfg.context_edit_enabled);
        assert_eq!(cfg.context_edit_max_chars, 12_345);
        assert_eq!(cfg.context_edit_keep_recent_messages, 11);
        assert_eq!(cfg.context_edit_max_tool_result_chars, 700);
    }

    #[test]
    fn hooks_load_from_nanocodex_file() {
        let tmp = std::env::temp_dir().join("ncx_config_test_hooks");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            r#"
api_key = "sk-base"

[[hooks]]
event = "pre_tool"
matcher = "shell|apply_patch"
command = "echo hook"
timeout_s = 3

[[hooks]]
event = "post_tool"
command = "echo post"
"#,
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp),
                ..Default::default()
            },
            &paths,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(cfg.hooks.len(), 2);
        assert_eq!(cfg.hooks[0].matcher, "shell|apply_patch");
        assert_eq!(cfg.hooks[0].timeout_s, 3);
        assert_eq!(cfg.hooks[1].matcher, "*");
    }

    #[test]
    fn hook_event_aliases_are_normalized() {
        let tmp = std::env::temp_dir().join("ncx_config_test_hook_aliases");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            r#"
api_key = "sk-base"

[[hooks]]
event = "UserPromptSubmit"
command = "echo prompt"

[[hooks]]
event = "Stop"
command = "echo stop"
"#,
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp),
                ..Default::default()
            },
            &paths,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(cfg.hooks[0].event, "user_prompt");
        assert_eq!(cfg.hooks[1].event, "stop");
        cfg.validate().unwrap();
    }

    #[test]
    fn hook_missing_command_fails_validation() {
        let tmp = std::env::temp_dir().join("ncx_config_test_hook_missing_command");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            r#"
api_key = "sk-base"

[[hooks]]
event = "pre_tool"
matcher = "shell"
"#,
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp),
                ..Default::default()
            },
            &paths,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(cfg.hooks.len(), 1);
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("command must not be empty"));
    }

    #[test]
    fn nanocodex_file_wins_over_deepseek() {
        let tmp = std::env::temp_dir().join("ncx_config_test_nanowins");
        fs::create_dir_all(&tmp).unwrap();
        let ds = tmp.join("deepseek.toml");
        let nano = tmp.join("nano.toml");
        write(
            &ds,
            "api_key = \"sk-ds\"\ndefault_text_model = \"deepseek-v4-pro\"\n",
        );
        write(&nano, "api_key = \"sk-nano\"\nmodel = \"deepseek-chat\"\n");
        let paths = ConfigPaths {
            deepseek: ds,
            codex: tmp.join("nope.toml"),
            nanocodex: nano,
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        assert_eq!(cfg.api_key, "sk-nano");
        assert_eq!(cfg.model, "deepseek-chat");
    }

    #[test]
    fn env_wins_over_nanocodex_file() {
        let tmp = std::env::temp_dir().join("ncx_config_test_envwins");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(&nano, "api_key = \"sk-nano\"\n");
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &env1("DEEPSEEK_API_KEY", "sk-env"),
        )
        .unwrap();
        assert_eq!(cfg.api_key, "sk-env");
    }

    #[test]
    fn max_retries_default_and_env() {
        let tmp = std::env::temp_dir().join("ncx_config_test_retries");
        fs::create_dir_all(&tmp).unwrap();
        // Default 3
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &no_paths(&tmp),
            &env1("DEEPSEEK_API_KEY", "sk-env"),
        )
        .unwrap();
        assert_eq!(cfg.max_retries, 3);

        // Override via env
        let mut e = env1("DEEPSEEK_API_KEY", "sk-env");
        e.insert("NANOCODEX_MAX_RETRIES".into(), "5".into());
        let cfg2 = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &no_paths(&tmp),
            &e,
        )
        .unwrap();
        assert_eq!(cfg2.max_retries, 5);

        // Garbage falls back to default
        let mut e3 = env1("DEEPSEEK_API_KEY", "sk-env");
        e3.insert("NANOCODEX_MAX_RETRIES".into(), "not-a-number".into());
        let cfg3 = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &no_paths(&tmp),
            &e3,
        )
        .unwrap();
        assert_eq!(cfg3.max_retries, 3);
    }

    #[test]
    fn profile_overrides_base_but_below_env() {
        let tmp = std::env::temp_dir().join("ncx_config_test_profile");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            concat!(
                "api_key = \"sk-base\"\n",
                "model = \"deepseek-chat\"\n",
                "reasoning_effort = \"auto\"\n",
                "\n",
                "[profiles.fast]\n",
                "model = \"deepseek-v4-pro\"\n",
                "reasoning_effort = \"high\"\n",
                "sandbox_mode = \"read-only\"\n",
            ),
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };

        // Profile applied
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                profile: Some("fast".into()),
                ..Default::default()
            },
            &paths,
            &empty_env(),
        )
        .unwrap();
        assert_eq!(cfg.model, "deepseek-v4-pro");
        assert_eq!(cfg.reasoning_effort, "high");
        assert_eq!(cfg.sandbox_mode, "read-only");

        // Env beats profile
        let cfg2 = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                profile: Some("fast".into()),
                ..Default::default()
            },
            &paths,
            &env1("NANOCODEX_MODEL", "deepseek-reasoner"),
        )
        .unwrap();
        assert_eq!(cfg2.model, "deepseek-reasoner");
    }

    #[test]
    fn profile_name_from_env_and_unknown_raises() {
        let tmp = std::env::temp_dir().join("ncx_config_test_profile_env");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            "api_key = \"sk-base\"\n[profiles.fast]\nmodel = \"m-fast\"\n",
        );
        let paths = ConfigPaths {
            deepseek: tmp.join("nope-ds.toml"),
            codex: tmp.join("nope-cx.toml"),
            nanocodex: nano,
        };

        // Name from env
        let cfg = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &env1("NANOCODEX_PROFILE", "fast"),
        )
        .unwrap();
        assert_eq!(cfg.model, "m-fast");

        // Unknown name -> error mentioning the name
        let err = load_config_impl(
            Overrides {
                workspace: Some(tmp.clone()),
                ..Default::default()
            },
            &paths,
            &env1("NANOCODEX_PROFILE", "ghost"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("ghost"), "error: {err}");
    }

    #[test]
    fn list_profiles_returns_sorted_names() {
        let tmp = std::env::temp_dir().join("ncx_config_test_listprof");
        fs::create_dir_all(&tmp).unwrap();
        let nano = tmp.join("nano.toml");
        write(
            &nano,
            "[profiles.a]\nmodel=\"x\"\n[profiles.b]\nmodel=\"y\"\n",
        );
        let names = list_profiles_at(&nano);
        assert_eq!(names, vec!["a", "b"]);
    }
}
