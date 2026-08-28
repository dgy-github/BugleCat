//! Configuration loader — Rust port of the layered resolution in `load_config()`
//! from `nanocodex/config.py`.
//!
//! The loader avoids reading `std::env` directly so tests can inject a fake env
//! map without mutating process-global state.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;
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
    pub deepseek_api_key: Option<String>,
    pub yunmo_api_key: Option<String>,
    pub base_url: Option<String>,
    pub provider_protocol: Option<String>,
    pub active_provider_id: Option<String>,
    pub model: Option<String>,
    pub fast_model: Option<String>,
    pub sandbox_mode: Option<String>,
    pub approval_policy: Option<String>,
    pub reasoning_effort: Option<String>,
    pub vl_base_url: Option<String>,
    pub vl_api_key: Option<String>,
    pub vl_model: Option<String>,
    pub alibaba_attachment_parser_enabled: Option<bool>,
    pub dashscope_token_plan_key: Option<String>,
    pub dashscope_workspace_key: Option<String>,
    pub ark_api_key: Option<String>,
    pub max_iterations: Option<i64>,
    pub max_tool_calls: Option<i64>,
    pub max_parallel_tool_calls: Option<i64>,
    pub orchestrator_workers: Option<i64>,
    pub orchestrator_high_workers: Option<i64>,
    pub orchestrator_verify_retries: Option<i64>,
    pub orchestrator_max_depth: Option<i64>,
    pub orchestrator_max_subtasks: Option<i64>,
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

include!("loader/sources.rs");
fn as_int(s: Option<&str>, default: i64) -> i64 {
    s.and_then(|v| v.parse::<i64>().ok()).unwrap_or(default)
}

fn as_float(s: Option<&str>, default: f64) -> f64 {
    s.and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(default)
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
        let name = s
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let command = s
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() || command.is_empty() {
            continue;
        }
        let args: Vec<String> = s
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
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
        out.push(McpServerConfig {
            name,
            command,
            args,
            env,
        });
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
    merged.insert("provider_protocol".into(), "openai".into());
    merged.insert("model".into(), DEFAULT_MODEL.into());
    merged.insert("price_currency".into(), "CNY".into());

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

    apply_active_provider_route(&mut merged, &paths.nanocodex)?;

    // Environment variable layer.
    let env_map: &[(&str, &[&str])] = &[
        ("api_key", &["DEEPSEEK_API_KEY", "NANOCODEX_API_KEY"]),
        ("deepseek_api_key", &["NANOCODEX_DEEPSEEK_API_KEY"]),
        ("yunmo_api_key", &["NANOCODEX_YUNMO_API_KEY"]),
        ("base_url", &["DEEPSEEK_BASE_URL", "NANOCODEX_BASE_URL"]),
        ("provider_protocol", &["NANOCODEX_PROVIDER_PROTOCOL"]),
        ("model", &["NANOCODEX_MODEL"]),
        ("fast_model", &["NANOCODEX_FAST_MODEL"]),
        ("vl_base_url", &["NANOCODEX_VL_BASE_URL"]),
        ("vl_api_key", &["DASHSCOPE_API_KEY", "NANOCODEX_VL_API_KEY"]),
        ("vl_model", &["NANOCODEX_VL_MODEL"]),
        (
            "alibaba_attachment_parser_enabled",
            &["NANOCODEX_ALIBABA_ATTACHMENT_PARSER_ENABLED"],
        ),
        (
            "dashscope_token_plan_key",
            &[
                "DASHSCOPE_TOKEN_PLAN_KEY",
                "NANOCODEX_DASHSCOPE_TOKEN_PLAN_KEY",
            ],
        ),
        (
            "dashscope_workspace_key",
            &[
                "DASHSCOPE_WORKSPACE_KEY",
                "NANOCODEX_DASHSCOPE_WORKSPACE_KEY",
            ],
        ),
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
        ("price_currency", &["NANOCODEX_PRICE_CURRENCY"]),
        ("max_iterations", &["NANOCODEX_MAX_ITERATIONS"]),
        ("max_tool_calls", &["NANOCODEX_MAX_TOOL_CALLS"]),
        (
            "max_parallel_tool_calls",
            &["NANOCODEX_MAX_PARALLEL_TOOL_CALLS"],
        ),
        ("max_retries", &["NANOCODEX_MAX_RETRIES"]),
        ("orchestrator_workers", &["NANOCODEX_ORCHESTRATOR_WORKERS"]),
        (
            "orchestrator_high_workers",
            &["NANOCODEX_ORCHESTRATOR_HIGH_WORKERS"],
        ),
        (
            "orchestrator_verify_retries",
            &["NANOCODEX_ORCHESTRATOR_VERIFY_RETRIES"],
        ),
        (
            "orchestrator_max_depth",
            &["NANOCODEX_ORCHESTRATOR_MAX_DEPTH"],
        ),
        (
            "orchestrator_max_subtasks",
            &["NANOCODEX_ORCHESTRATOR_MAX_SUBTASKS"],
        ),
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
    apply_str!(deepseek_api_key);
    apply_str!(yunmo_api_key);
    apply_str!(base_url);
    apply_str!(provider_protocol);
    apply_str!(active_provider_id);
    apply_str!(model);
    apply_str!(fast_model);
    apply_str!(sandbox_mode);
    apply_str!(approval_policy);
    apply_str!(reasoning_effort);
    apply_str!(vl_base_url);
    apply_str!(vl_api_key);
    apply_str!(vl_model);
    apply_bool!(alibaba_attachment_parser_enabled);
    apply_str!(dashscope_token_plan_key);
    apply_str!(dashscope_workspace_key);
    apply_str!(ark_api_key);
    apply_int!(max_iterations);
    apply_int!(max_tool_calls);
    apply_int!(max_parallel_tool_calls);
    apply_int!(max_retries);
    apply_int!(orchestrator_workers);
    apply_int!(orchestrator_high_workers);
    apply_int!(orchestrator_verify_retries);
    apply_int!(orchestrator_max_depth);
    apply_int!(orchestrator_max_subtasks);
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
        deepseek_api_key: merged.get("deepseek_api_key").cloned().unwrap_or_default(),
        yunmo_api_key: merged.get("yunmo_api_key").cloned().unwrap_or_default(),
        base_url: merged
            .get("base_url")
            .cloned()
            .unwrap_or_else(|| DEFAULT_BASE_URL.into()),
        provider_protocol: merged
            .get("provider_protocol")
            .cloned()
            .unwrap_or_else(|| "openai".into()),
        active_provider_id: merged
            .get("active_provider_id")
            .cloned()
            .unwrap_or_else(|| "legacy".into()),
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
        alibaba_attachment_parser_enabled: as_bool(
            merged
                .get("alibaba_attachment_parser_enabled")
                .map(String::as_str),
            false,
        ),
        dashscope_token_plan_key: merged
            .get("dashscope_token_plan_key")
            .cloned()
            .unwrap_or_default(),
        dashscope_workspace_key: merged
            .get("dashscope_workspace_key")
            .cloned()
            .unwrap_or_default(),
        ark_api_key: merged.get("ark_api_key").cloned().unwrap_or_default(),
        search_provider: merged
            .get("search_provider")
            .cloned()
            .unwrap_or_else(|| "duckduckgo".into()),
        search_api_key: merged.get("search_api_key").cloned().unwrap_or_default(),
        workspace,
        writable_roots: vec![],
        network_access,
        max_iterations: as_int(merged.get("max_iterations").map(|s| s.as_str()), 150),
        max_tool_calls: as_int(merged.get("max_tool_calls").map(|s| s.as_str()), 300),
        max_parallel_tool_calls: as_int(
            merged.get("max_parallel_tool_calls").map(|s| s.as_str()),
            8,
        ),
        timeout_s: 120,
        max_retries: as_int(merged.get("max_retries").map(|s| s.as_str()), 3),
        orchestrator_workers: as_int(merged.get("orchestrator_workers").map(|s| s.as_str()), 2),
        orchestrator_high_workers: as_int(
            merged.get("orchestrator_high_workers").map(|s| s.as_str()),
            3,
        ),
        orchestrator_verify_retries: as_int(
            merged
                .get("orchestrator_verify_retries")
                .map(|s| s.as_str()),
            1,
        ),
        orchestrator_max_depth: as_int(merged.get("orchestrator_max_depth").map(|s| s.as_str()), 1),
        orchestrator_max_subtasks: as_int(
            merged.get("orchestrator_max_subtasks").map(|s| s.as_str()),
            6,
        ),
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
        price_currency: merged
            .get("price_currency")
            .cloned()
            .unwrap_or_else(|| "CNY".into()),
        hooks: parse_hooks(&nano_raw),
        mcp_servers: vec![],
    };
    Ok(cfg)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "loader/tests.rs"]
mod tests;
