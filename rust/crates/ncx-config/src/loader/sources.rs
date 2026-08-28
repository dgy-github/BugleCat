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
        "deepseek_api_key",
        "yunmo_api_key",
        "base_url",
        "provider_protocol",
        "active_provider_id",
        "model",
        "fast_model",
        "sandbox_mode",
        "approval_policy",
        "permission_mode",
        "reasoning_effort",
        "vl_base_url",
        "vl_api_key",
        "vl_model",
        "alibaba_attachment_parser_enabled",
        "dashscope_token_plan_key",
        "dashscope_workspace_key",
        "ark_api_key",
        "search_provider",
        "search_api_key",
        "max_iterations",
        "max_tool_calls",
        "max_parallel_tool_calls",
        "orchestrator_workers",
        "orchestrator_high_workers",
        "orchestrator_verify_retries",
        "orchestrator_max_depth",
        "orchestrator_max_subtasks",
        "max_retries",
        "context_token_budget",
        "context_window",
        "context_edit_enabled",
        "context_edit_max_chars",
        "context_edit_keep_recent_messages",
        "context_edit_max_tool_result_chars",
        "price_in",
        "price_out",
        "price_currency",
        "available_models",
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
    "dashscope_token_plan_key",
    "dashscope_workspace_key",
    "ark_api_key",
    "max_iterations",
    "max_tool_calls",
    "max_parallel_tool_calls",
    "orchestrator_workers",
    "orchestrator_high_workers",
    "orchestrator_verify_retries",
    "orchestrator_max_depth",
    "orchestrator_max_subtasks",
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

#[derive(Deserialize)]
struct ProviderRoute {
    id: String,
    protocol: String,
    base_url: String,
    api_key: String,
    models: Vec<String>,
    #[serde(default)]
    selected_model: String,
}

/// Resolve all connection-bearing fields from one provider record. The route
/// ID is the pointer; individual flat settings must never be mixed across
/// providers when a custom route is active.
fn apply_active_provider_route(
    merged: &mut BTreeMap<String, String>,
    config_path: &Path,
) -> Result<(), ConfigError> {
    let Some(provider_id) = merged.get("active_provider_id").cloned() else {
        return Ok(());
    };
    if provider_id == "legacy" {
        return Ok(());
    }

    let path = config_path.with_file_name("providers.json");
    let bytes = std::fs::read(&path).map_err(|error| {
        ConfigError(format!(
            "Active provider {provider_id:?} cannot be loaded from {}: {error}",
            path.display()
        ))
    })?;
    let routes: Vec<ProviderRoute> = serde_json::from_slice(&bytes).map_err(|error| {
        ConfigError(format!(
            "Invalid provider configuration at {}: {error}",
            path.display()
        ))
    })?;
    let route = routes
        .into_iter()
        .find(|route| route.id == provider_id)
        .ok_or_else(|| ConfigError(format!("Active provider {provider_id:?} does not exist")))?;
    if !matches!(route.protocol.as_str(), "openai" | "anthropic")
        || route.base_url.trim().is_empty()
        || route.api_key.trim().is_empty()
        || route.selected_model.trim().is_empty()
    {
        return Err(ConfigError(format!(
            "Active provider {provider_id:?} is incomplete; protocol, Base URL, Token, and selected model are required"
        )));
    }

    merged.insert("provider_protocol".into(), route.protocol);
    merged.insert("base_url".into(), route.base_url);
    merged.insert("api_key".into(), route.api_key);
    merged.insert("model".into(), route.selected_model.clone());
    let mut models = route.models;
    if !models.iter().any(|model| model == &route.selected_model) {
        models.insert(0, route.selected_model);
    }
    merged.insert("available_models".into(), models.join(","));
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────
