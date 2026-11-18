//! Scoped Agent manifest (`ncx-scope/v1`) loader and runtime enforcement (M0).
//!
//! Implements the scope contract from `docs/scoped-agent-distillation.zh-CN.md`:
//! - Parses and validates workspace (`agents/<id>.toml`) and user
//!   (`~/.nanocodex/agents/<id>.toml`) scope manifests.
//! - Monotonically narrows [`SandboxPolicy`] (read roots, writable roots,
//!   network, OS sandbox, and protected scope paths — never widening global
//!   permissions).
//! - Filters registered tools (`allow` / `deny`) so out-of-scope tools are
//!   neither advertised in schemas nor callable.
//! - Enforces conditional `[[tools.approval]]` rules via [`ScopeGuardMiddleware`].
//! - Isolates memory under `data.memory_namespace` and supplies scope summaries
//!   for prompt context and Jev pre-execution `exceeds_scope` review.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use async_trait::async_trait;
use ncx_sandbox::{ApprovalRequest, SandboxPolicy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::memory::MemoryStore;
use crate::tool_middleware::{ToolMiddleware, ToolMiddlewareDecision};
use crate::tools::{ApprovalDecision, ToolContext, ToolFilter, ToolRegistry};

pub const SCOPE_SCHEMA: &str = "ncx-scope/v1";
pub const SCOPE_ENV: &str = "NCX_SCOPE";
pub const SCOPE_MIDDLEWARE_NAME: &str = "scope.guard";

const VALID_ESCALATION_MODES: &[&str] = &["fallback", "human", "refuse"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeManifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: u32,
    #[serde(default)]
    pub duty: ScopeDuty,
    #[serde(default)]
    pub data: ScopeData,
    #[serde(default)]
    pub tools: ScopeTools,
    #[serde(default)]
    pub model: ScopeModel,
    #[serde(default)]
    pub acceptance: ScopeAcceptance,
    #[serde(default)]
    pub escalation: ScopeEscalation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDuty {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub accepts: Vec<String>,
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub output: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeData {
    #[serde(default)]
    pub read_roots: Vec<PathBuf>,
    #[serde(default)]
    pub memory_namespace: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeTools {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub writable_roots: Vec<PathBuf>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub os_sandbox: Option<String>,
    #[serde(default)]
    pub approval: Vec<ScopeApprovalRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeApprovalRule {
    pub when: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeModel {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeAcceptance {
    #[serde(default)]
    pub suite: Option<PathBuf>,
    #[serde(default)]
    pub pass_rate: Option<f64>,
    #[serde(default = "default_true")]
    pub no_regression: bool,
}

impl Default for ScopeAcceptance {
    fn default() -> Self {
        Self {
            suite: None,
            pass_rate: None,
            no_regression: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeEscalation {
    #[serde(default = "default_fallback")]
    pub on_out_of_scope: String,
    #[serde(default = "default_human")]
    pub on_low_confidence: String,
}

impl Default for ScopeEscalation {
    fn default() -> Self {
        Self {
            on_out_of_scope: default_fallback(),
            on_low_confidence: default_human(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_fallback() -> String {
    "fallback".to_string()
}

fn default_human() -> String {
    "human".to_string()
}

/// Validated scope manifest paired with the canonical/normalized file path it
/// was loaded from (protected against runtime self-modification).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScope {
    pub manifest: ScopeManifest,
    pub source_path: PathBuf,
}

impl ScopeManifest {
    /// Parse and validate an `ncx-scope/v1` TOML document.
    pub fn from_toml_str(raw: &str) -> Result<Self, String> {
        let manifest: Self =
            toml::from_str(raw).map_err(|err| format!("invalid scope TOML: {err}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema.trim() != SCOPE_SCHEMA {
            return Err(format!(
                "unsupported scope schema {:?}; expected {:?}",
                self.schema, SCOPE_SCHEMA
            ));
        }
        if !valid_scope_id(&self.id) {
            return Err(format!(
                "invalid scope id {:?}; expected non-empty ASCII [a-zA-Z0-9_-]+",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("scope 'name' must not be empty".to_string());
        }
        if self.version == 0 {
            return Err("scope 'version' must be >= 1".to_string());
        }
        if let Some(ns) = &self.data.memory_namespace {
            if !valid_scope_id(ns) {
                return Err(format!(
                    "invalid data.memory_namespace {:?}; expected ASCII [a-zA-Z0-9_-]+",
                    ns
                ));
            }
        }
        if let Some(os_mode) = &self.tools.os_sandbox {
            if !matches!(os_mode.trim(), "off" | "enforce") {
                return Err(format!(
                    "invalid tools.os_sandbox {:?}; expected \"off\" or \"enforce\"",
                    os_mode
                ));
            }
        }
        for rule in &self.tools.approval {
            if rule.when.trim().is_empty() {
                return Err("tools.approval.when must not be empty".to_string());
            }
            validate_when_expr(&rule.when)?;
        }
        if let Some(rate) = self.acceptance.pass_rate {
            if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
                return Err(format!(
                    "invalid acceptance.pass_rate {rate}; expected 0.0..=1.0"
                ));
            }
        }
        for (field, val) in [
            (
                "escalation.on_out_of_scope",
                &self.escalation.on_out_of_scope,
            ),
            (
                "escalation.on_low_confidence",
                &self.escalation.on_low_confidence,
            ),
        ] {
            if !VALID_ESCALATION_MODES.contains(&val.trim()) {
                return Err(format!(
                    "invalid {field} {val:?}; expected one of {VALID_ESCALATION_MODES:?}"
                ));
            }
        }
        Ok(())
    }

    /// True when `query` matches at least one keyword in `duty.accepts`
    /// (case-insensitive), or when `accepts` is empty.
    pub fn matches_accepts(&self, query: &str) -> bool {
        if self.duty.accepts.is_empty() {
            return true;
        }
        let folded = query.to_lowercase();
        self.duty
            .accepts
            .iter()
            .filter(|kw| !kw.trim().is_empty())
            .any(|kw| folded.contains(&kw.trim().to_lowercase()))
    }
}

impl LoadedScope {
    pub fn load_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw_path = path.as_ref();
        let text = fs::read_to_string(raw_path)
            .map_err(|err| format!("cannot read scope file {}: {err}", raw_path.display()))?;
        let manifest = ScopeManifest::from_toml_str(&text)?;
        let source_path = raw_path
            .canonicalize()
            .map(ncx_sandbox::strip_verbatim_prefix)
            .unwrap_or_else(|_| ncx_sandbox::normalize(raw_path));
        Ok(Self {
            manifest,
            source_path,
        })
    }

    /// Build the [`ToolFilter`] declared by `[tools]`.
    pub fn tool_filter(&self) -> ToolFilter {
        let allow =
            (!self.manifest.tools.allow.is_empty()).then(|| self.manifest.tools.allow.clone());
        ToolFilter::new(allow, self.manifest.tools.deny.clone())
    }

    /// Monotonically narrow `base` according to `[data]` and `[tools]`, while
    /// locking the scope manifest itself and `agents/` directories against writes.
    pub fn narrow_sandbox(&self, base: &SandboxPolicy, home: Option<&Path>) -> SandboxPolicy {
        let mut protected = vec![self.source_path.clone(), base.workspace.join("agents")];
        if let Some(home) = home {
            protected.push(home.join(".nanocodex").join("agents"));
        }
        let read_roots = (!self.manifest.data.read_roots.is_empty())
            .then_some(self.manifest.data.read_roots.as_slice());
        let enforce_os = self.manifest.tools.os_sandbox.as_deref() == Some("enforce");
        base.narrow(
            read_roots,
            &self.manifest.tools.writable_roots,
            self.manifest.tools.network,
            enforce_os,
            &protected,
        )
    }

    /// Build the namespace-isolated [`MemoryStore`] when `data.memory_namespace`
    /// is configured, or fall back to `base_memory_dir`.
    pub fn memory_store(&self, base_memory_dir: &Path) -> Result<MemoryStore, String> {
        match self.manifest.data.memory_namespace.as_deref() {
            Some(ns) => MemoryStore::scoped(base_memory_dir, ns),
            None => Ok(MemoryStore::new(base_memory_dir)),
        }
    }

    /// Apply `[model]` endpoint and model defaults to `cfg` unless overridden by CLI.
    pub fn apply_to_config(&self, cfg: &mut ncx_config::Config, explicit_model_override: bool) {
        if explicit_model_override {
            return;
        }
        if let Some(endpoint) = self
            .manifest
            .model
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            cfg.base_url = endpoint.to_string();
            if ncx_config::is_local_base_url(endpoint) && cfg.provider_protocol != "anthropic" {
                cfg.provider_protocol = "openai".to_string();
            }
        }
        let preferred_model = self
            .manifest
            .model
            .adapter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.manifest
                    .model
                    .base
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            });
        if let Some(model) = preferred_model {
            cfg.model = model.to_string();
            if !cfg.available_models.iter().any(|m| m == model) {
                cfg.available_models.insert(0, model.to_string());
            }
        }
    }

    /// Install tool filtering and conditional approval middleware on `registry`.
    pub fn install_into_registry(self: &Rc<Self>, registry: &mut ToolRegistry) {
        registry.ctx.scope = Some(self.clone());
        registry.apply_tool_filter(self.tool_filter());
        let _ = registry.unregister_middleware(SCOPE_MIDDLEWARE_NAME);
        let _ = registry.register_middleware(Rc::new(ScopeGuardMiddleware {
            scope: self.clone(),
        }));
    }

    /// Prompt fragment describing the active scope's duty, boundaries, and
    /// out-of-scope handoff rules.
    pub fn context_fragment(&self) -> String {
        let m = &self.manifest;
        let mut lines = vec![format!(
            "[Active Scope: {} (id={}, v{})]",
            m.name, m.id, m.version
        )];
        if !m.duty.summary.trim().is_empty() {
            lines.push(format!("Duty: {}", m.duty.summary.trim()));
        }
        if !m.duty.input.trim().is_empty() {
            lines.push(format!("Expected input: {}", m.duty.input.trim()));
        }
        if !m.duty.output.trim().is_empty() {
            lines.push(format!("Required output: {}", m.duty.output.trim()));
        }
        if !m.data.read_roots.is_empty() {
            let roots: Vec<String> = m
                .data
                .read_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            lines.push(format!("Readable roots: {}", roots.join(", ")));
        }
        if m.tools.writable_roots.is_empty() {
            lines.push("Writable roots: (none — read-only)".to_string());
        } else {
            let roots: Vec<String> = m
                .tools
                .writable_roots
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            lines.push(format!("Writable roots: {}", roots.join(", ")));
        }
        lines.push(format!(
            "Out-of-scope policy: on_out_of_scope={}, on_low_confidence={}. Do NOT execute tasks outside this scope; state clearly that the request is out of scope so it can be escalated.",
            m.escalation.on_out_of_scope, m.escalation.on_low_confidence
        ));
        lines.join("\n")
    }

    /// Compact summary supplied to Jev pre-execution `exceeds_scope` review.
    pub fn jev_summary(&self) -> String {
        let m = &self.manifest;
        let writes: Vec<String> = m
            .tools
            .writable_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let reads: Vec<String> = m
            .data
            .read_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        format!(
            "scope_id={}; name={}; duty={}; input={}; output={}; read_roots=[{}]; writable_roots=[{}]; network={}",
            m.id,
            m.name,
            m.duty.summary.trim(),
            m.duty.input.trim(),
            m.duty.output.trim(),
            reads.join(", "),
            writes.join(", "),
            m.tools.network,
        )
    }
}

pub fn valid_scope_id(id: &str) -> bool {
    let trimmed = id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn default_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Resolve a scope by ID (`agents/<id>.toml` in workspace, then `~/.nanocodex/agents/<id>.toml`)
/// or by explicit `.toml` file path.
pub fn resolve_scope(workspace: &Path, selector: &str) -> Result<LoadedScope, String> {
    resolve_scope_with_home(workspace, default_home().as_deref(), selector)
}

pub fn resolve_scope_with_home(
    workspace: &Path,
    home: Option<&Path>,
    selector: &str,
) -> Result<LoadedScope, String> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err("scope selector cannot be empty".to_string());
    }
    let looks_like_path =
        trimmed.ends_with(".toml") || trimmed.contains('/') || trimmed.contains('\\');
    if looks_like_path {
        let candidate = Path::new(trimmed);
        let full = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            workspace.join(candidate)
        };
        return LoadedScope::load_file(full);
    }

    if !valid_scope_id(trimmed) {
        return Err(format!(
            "invalid scope id {trimmed:?}; expected ASCII [a-zA-Z0-9_-]+ or a .toml path"
        ));
    }
    let ws_candidate = workspace.join("agents").join(format!("{trimmed}.toml"));
    if ws_candidate.is_file() {
        let loaded = LoadedScope::load_file(&ws_candidate)?;
        if loaded.manifest.id != trimmed {
            return Err(format!(
                "scope file {} declares id {:?}, expected {:?}",
                ws_candidate.display(),
                loaded.manifest.id,
                trimmed
            ));
        }
        return Ok(loaded);
    }
    if let Some(home) = home {
        let user_candidate = home
            .join(".nanocodex")
            .join("agents")
            .join(format!("{trimmed}.toml"));
        if user_candidate.is_file() {
            let loaded = LoadedScope::load_file(&user_candidate)?;
            if loaded.manifest.id != trimmed {
                return Err(format!(
                    "scope file {} declares id {:?}, expected {:?}",
                    user_candidate.display(),
                    loaded.manifest.id,
                    trimmed
                ));
            }
            return Ok(loaded);
        }
    }
    Err(format!(
        "scope {trimmed:?} not found in {}/agents/{trimmed}.toml or ~/.nanocodex/agents/{trimmed}.toml",
        workspace.display()
    ))
}

/// Resolve an active scope from `NCX_SCOPE` when set.
pub fn resolve_scope_from_env(workspace: &Path) -> Result<Option<LoadedScope>, String> {
    let Ok(raw) = std::env::var(SCOPE_ENV) else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    resolve_scope(workspace, &raw).map(Some)
}

/// Discover all valid scopes across `~/.nanocodex/agents` and `<workspace>/agents`
/// (workspace scopes shadow global scopes with the same `id`).
pub fn discover_scopes_with_home(workspace: &Path, home: Option<&Path>) -> Vec<LoadedScope> {
    let mut by_id: BTreeMap<String, LoadedScope> = BTreeMap::new();
    let dirs = home
        .into_iter()
        .map(|h| h.join(".nanocodex").join("agents"))
        .chain(std::iter::once(workspace.join("agents")));
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") || !path.is_file() {
                continue;
            }
            if let Ok(loaded) = LoadedScope::load_file(&path) {
                by_id.insert(loaded.manifest.id.clone(), loaded);
            }
        }
    }
    by_id.into_values().collect()
}

// ── Conditional approval rule engine (`[[tools.approval]]`) ──────────────────

pub struct ScopeGuardMiddleware {
    scope: Rc<LoadedScope>,
}

#[async_trait(?Send)]
impl ToolMiddleware for ScopeGuardMiddleware {
    fn name(&self) -> &str {
        SCOPE_MIDDLEWARE_NAME
    }

    async fn before_execute(
        &self,
        ctx: &ToolContext,
        tool_name: &str,
        args: &Value,
    ) -> ToolMiddlewareDecision {
        if !self.scope.tool_filter().allows(tool_name) {
            return ToolMiddlewareDecision::Block {
                reason: format!(
                    "tool '{tool_name}' is not permitted in scope '{}'",
                    self.scope.manifest.id
                ),
            };
        }

        for rule in &self.scope.manifest.tools.approval {
            if rule.matches(tool_name, args) {
                let why = if rule.reason.trim().is_empty() {
                    rule.when.trim().to_string()
                } else {
                    format!("{} ({})", rule.reason.trim(), rule.when.trim())
                };
                let Some(approver) = &ctx.approver else {
                    return ToolMiddlewareDecision::Block {
                        reason: format!(
                            "scope '{}' requires human approval for `{tool_name}`: {why}",
                            self.scope.manifest.id
                        ),
                    };
                };
                let decision = approver
                    .request(ApprovalRequest {
                        command: format!("{tool_name} {}", compact_json(args)),
                        reason: format!("[Scope '{}'] {why}", self.scope.manifest.name),
                        cwd: ctx.workspace.display().to_string(),
                        escalated: true,
                        details: serde_json::to_string_pretty(args).unwrap_or_default(),
                    })
                    .await;
                if decision == ApprovalDecision::Deny {
                    return ToolMiddlewareDecision::Block {
                        reason: format!(
                            "denied by human approval rule in scope '{}': {why}",
                            self.scope.manifest.id
                        ),
                    };
                }
            }
        }

        ToolMiddlewareDecision::Continue
    }
}

fn compact_json(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() <= 120 {
        s
    } else {
        let prefix: String = s.chars().take(120).collect();
        format!("{prefix}...")
    }
}

impl ScopeApprovalRule {
    /// Evaluate `when` against `(tool_name, args)`.
    ///
    /// Supports `"*"` / `"always"` / `"true"`, boolean `||` and `&&` groups,
    /// and atomic comparisons (`>=`, `<=`, `!=`, `==`, `>`, `<`, `contains`)
    /// over `tool`, JSON argument fields (nested or top-level), and embedded
    /// `key=value` / `"key": value` / `--key value` tokens inside string payloads.
    pub fn matches(&self, tool_name: &str, args: &Value) -> bool {
        eval_or_expr(self.when.trim(), tool_name, args)
    }
}

fn validate_when_expr(expr: &str) -> Result<(), String> {
    let trimmed = expr.trim();
    if matches!(trimmed, "*" | "always" | "true") {
        return Ok(());
    }
    for or_part in split_outside_quotes(trimmed, "||") {
        for and_part in split_outside_quotes(or_part.trim(), "&&") {
            let clause = and_part.trim();
            if matches!(clause, "*" | "always" | "true") {
                continue;
            }
            parse_clause(clause).ok_or_else(|| {
                format!(
                    "unsupported tools.approval.when clause {clause:?}; expected `<field> <op> <value>`"
                )
            })?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompOp {
    Ge,
    Le,
    Ne,
    Eq,
    Gt,
    Lt,
    Contains,
}

fn parse_clause(clause: &str) -> Option<(&str, CompOp, &str)> {
    for (sym, op) in [
        (">=", CompOp::Ge),
        ("<=", CompOp::Le),
        ("!=", CompOp::Ne),
        ("==", CompOp::Eq),
        (">", CompOp::Gt),
        ("<", CompOp::Lt),
    ] {
        if let Some((lhs, rhs)) = split_once_outside_quotes(clause, sym) {
            let lhs = lhs.trim();
            let rhs = rhs.trim();
            if !lhs.is_empty() && !rhs.is_empty() {
                return Some((lhs, op, rhs));
            }
        }
    }
    if let Some((lhs, rhs)) = split_once_outside_quotes(clause, " contains ") {
        let lhs = lhs.trim();
        let rhs = rhs.trim();
        if !lhs.is_empty() && !rhs.is_empty() {
            return Some((lhs, CompOp::Contains, rhs));
        }
    }
    None
}

fn eval_or_expr(expr: &str, tool_name: &str, args: &Value) -> bool {
    if matches!(expr, "*" | "always" | "true") {
        return true;
    }
    split_outside_quotes(expr, "||").into_iter().any(|or_part| {
        split_outside_quotes(or_part.trim(), "&&")
            .into_iter()
            .all(|and_part| eval_clause(and_part.trim(), tool_name, args))
    })
}

fn eval_clause(clause: &str, tool_name: &str, args: &Value) -> bool {
    if matches!(clause, "*" | "always" | "true") {
        return true;
    }
    let Some((field, op, raw_rhs)) = parse_clause(clause) else {
        return false;
    };
    let unquoted_rhs = unquote(raw_rhs);
    let candidates = extract_field_values(field, tool_name, args);
    if candidates.is_empty() {
        return false;
    }
    candidates
        .iter()
        .any(|val| compare_value(val, op, &unquoted_rhs))
}

fn compare_value(lhs: &Value, op: CompOp, rhs: &str) -> bool {
    if let (Some(l_num), Ok(r_num)) = (value_as_f64(lhs), rhs.parse::<f64>()) {
        return match op {
            CompOp::Gt => l_num > r_num,
            CompOp::Ge => l_num >= r_num,
            CompOp::Lt => l_num < r_num,
            CompOp::Le => l_num <= r_num,
            CompOp::Eq => (l_num - r_num).abs() < f64::EPSILON,
            CompOp::Ne => (l_num - r_num).abs() >= f64::EPSILON,
            CompOp::Contains => lhs.to_string().contains(rhs),
        };
    }
    let l_str = match lhs {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    };
    match op {
        CompOp::Eq => l_str == rhs,
        CompOp::Ne => l_str != rhs,
        CompOp::Contains => l_str.contains(rhs),
        _ => false,
    }
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn extract_field_values(field: &str, tool_name: &str, args: &Value) -> Vec<Value> {
    if field == "tool" {
        return vec![Value::String(tool_name.to_string())];
    }
    let mut out = Vec::new();
    // 1. Dotted path lookup (`a.b.c`).
    if field.contains('.') {
        let mut cur = Some(args);
        for seg in field.split('.') {
            cur = cur.and_then(|v| v.get(seg));
        }
        if let Some(v) = cur {
            out.push(v.clone());
        }
    }
    // 2. Recursive key & embedded string lookup across `args`.
    collect_key_values(field, args, &mut out);
    out
}

fn collect_key_values(field: &str, value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.get(field) {
                out.push(v.clone());
            }
            for v in map.values() {
                collect_key_values(field, v, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_key_values(field, item, out);
            }
        }
        Value::String(s) => {
            extract_embedded_scalars(field, s, out);
        }
        _ => {}
    }
}

/// Extract scalar values for `field` inside shell commands or patch text such as
/// `"refund_amount": 600`, `refund_amount=600`, or `--refund_amount 600`.
fn extract_embedded_scalars(field: &str, text: &str, out: &mut Vec<Value>) {
    let patterns = [
        format!("\"{field}\":"),
        format!("'{field}':"),
        format!("{field}="),
        format!("--{field} "),
        format!("--{} ", field.replace('_', "-")),
    ];
    for pat in patterns {
        let mut rest = text;
        while let Some(idx) = rest.find(&pat) {
            let after = rest[idx + pat.len()..].trim_start();
            let token: String = if let Some(stripped) = after.strip_prefix('"') {
                stripped.chars().take_while(|c| *c != '"').collect()
            } else if let Some(stripped) = after.strip_prefix('\'') {
                stripped.chars().take_while(|c| *c != '\'').collect()
            } else {
                after
                    .chars()
                    .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | '}' | ']' | ';'))
                    .collect()
            };
            if !token.is_empty() {
                if let Ok(num) = token.parse::<f64>() {
                    if let Some(n) = serde_json::Number::from_f64(num) {
                        out.push(Value::Number(n));
                    }
                } else {
                    out.push(Value::String(token));
                }
            }
            rest = &rest[idx + pat.len()..];
        }
    }
}

fn unquote(s: &str) -> String {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() >= 2 {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn split_outside_quotes<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let bytes = s.as_bytes();
    let sep_bytes = sep.as_bytes();
    let mut in_quote: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_quote = Some(b);
            i += 1;
            continue;
        }
        if i + sep_bytes.len() <= bytes.len() && &bytes[i..i + sep_bytes.len()] == sep_bytes {
            parts.push(&s[start..i]);
            i += sep_bytes.len();
            start = i;
            continue;
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

fn split_once_outside_quotes<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let bytes = s.as_bytes();
    let sep_bytes = sep.as_bytes();
    let mut in_quote: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_quote = Some(b);
            i += 1;
            continue;
        }
        if i + sep_bytes.len() <= bytes.len() && &bytes[i..i + sep_bytes.len()] == sep_bytes {
            return Some((&s[..i], &s[i + sep_bytes.len()..]));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use ncx_sandbox::{READ_ONLY, WORKSPACE_WRITE};
    use serde_json::json;

    use super::*;
    use crate::agent_loop::AgentLoop;
    use crate::model_provider::Provider;
    use crate::session::Session;
    use crate::tools::ApprovalHandler;
    use ncx_provider::{ModelResponse, ToolCall};

    const REFUND_TOML: &str = r#"
schema = "ncx-scope/v1"
id = "refund"
name = "售后退款处理"
version = 3

[duty]
summary = "处理退款工单：核对订单与规则，给出结论和回复草稿"
accepts = ["退款", "退货", "refund"]
input = "一张退款工单（订单号、诉求、聊天记录）"
output = "结论（同意/拒绝/转人工）+ 依据 + 回复草稿"

[data]
read_roots = ["data/orders", "docs/refund-policy"]
memory_namespace = "refund"

[tools]
allow = ["read_file", "grep", "apply_patch", "remember"]
deny = ["web_fetch", "shell"]
writable_roots = ["out/refund"]
network = false
os_sandbox = "enforce"

[[tools.approval]]
when = "refund_amount > 500"
reason = "大额退款"

[model]
base = "qwen3-8b-instruct-q4_k_m.gguf"
adapter = "refund-v3"
endpoint = "http://127.0.0.1:8080/v1"
fallback = "preset:deepseek/deepseek-v4-pro"

[acceptance]
suite = "agents/refund/acceptance.jsonl"
pass_rate = 0.95
no_regression = true

[escalation]
on_out_of_scope = "fallback"
on_low_confidence = "human"
"#;

    #[test]
    fn parses_design_doc_refund_scope_manifest_and_validates_invariants() {
        let m = ScopeManifest::from_toml_str(REFUND_TOML).expect("valid refund manifest");
        assert_eq!(m.id, "refund");
        assert_eq!(m.version, 3);
        assert!(m.matches_accepts("帮我处理这张退货工单"));
        assert!(!m.matches_accepts("帮我写一段贪吃蛇游戏"));
        assert_eq!(m.data.memory_namespace.as_deref(), Some("refund"));
        assert_eq!(m.tools.approval.len(), 1);
        assert!(m.tools.approval[0].matches("apply_patch", &json!({"refund_amount": 800})));
        assert!(!m.tools.approval[0].matches("apply_patch", &json!({"refund_amount": 200})));
        assert!(m.tools.approval[0].matches(
            "apply_patch",
            &json!({"patch": "+{\"order\": 1, \"refund_amount\": 501}"})
        ));
    }

    #[test]
    fn rejects_invalid_schema_version_or_bad_approval_clause() {
        assert!(ScopeManifest::from_toml_str(
            r#"schema = "wrong/v1"
id = "x"
name = "x"
version = 1"#
        )
        .is_err());
        assert!(ScopeManifest::from_toml_str(
            r#"schema = "ncx-scope/v1"
id = "../escape"
name = "x"
version = 1"#
        )
        .is_err());
        assert!(ScopeManifest::from_toml_str(
            r#"schema = "ncx-scope/v1"
id = "x"
name = "x"
version = 1
[[tools.approval]]
when = "not_a_comparison""#
        )
        .is_err());
    }

    #[test]
    fn workspace_scope_shadows_user_scope_and_enforces_narrowing() {
        let root = crate::test_support::unique_temp_dir("ncx_scope_resolve");
        let ws = root.join("ws");
        let home = root.join("home");
        fs::create_dir_all(ws.join("agents")).unwrap();
        fs::create_dir_all(home.join(".nanocodex/agents")).unwrap();

        let user_toml = REFUND_TOML.replace("version = 3", "version = 1");
        fs::write(home.join(".nanocodex/agents/refund.toml"), user_toml).unwrap();
        fs::write(ws.join("agents/refund.toml"), REFUND_TOML).unwrap();

        let loaded = resolve_scope_with_home(&ws, Some(&home), "refund").unwrap();
        assert_eq!(loaded.manifest.version, 3);

        // Global read-only cannot be widened by scope writable_roots.
        let ro_base = SandboxPolicy::new(READ_ONLY, &ws);
        let narrowed_ro = loaded.narrow_sandbox(&ro_base, Some(&home));
        assert_eq!(narrowed_ro.mode, READ_ONLY);
        assert!(!narrowed_ro.can_write(ws.join("out/refund/decision-1.json")));

        // Global workspace-write is narrowed strictly to out/refund, and scope files are locked.
        let rw_base = SandboxPolicy::new(WORKSPACE_WRITE, &ws).with_network_access(true);
        let narrowed_rw = loaded.narrow_sandbox(&rw_base, Some(&home));
        assert!(!narrowed_rw.network_access);
        assert!(narrowed_rw.os_enforced);
        assert!(narrowed_rw.can_write(ws.join("out/refund/decision-1.json")));
        assert!(!narrowed_rw.can_write(ws.join("agents/refund.toml")));
        assert!(!narrowed_rw.can_write(ws.join("docs/refund-policy/rules.md")));
        assert!(narrowed_rw.can_read(ws.join("data/orders/101.json")));
        assert!(!narrowed_rw.can_read(ws.join("src/unrelated.rs")));
    }

    struct CountingApprover {
        decision: ApprovalDecision,
        calls: Cell<usize>,
    }

    #[async_trait(?Send)]
    impl ApprovalHandler for CountingApprover {
        async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
            self.calls.set(self.calls.get() + 1);
            self.decision
        }
    }

    struct ScriptedLocalProvider {
        replies: std::cell::RefCell<std::collections::VecDeque<ModelResponse>>,
    }

    #[async_trait(?Send)]
    impl Provider for ScriptedLocalProvider {
        fn model(&self) -> &str {
            "qwen3-8b-instruct-q4_k_m.gguf"
        }
        async fn chat(
            &self,
            _messages: &[Value],
            _tools: &[Value],
            _effort: Option<&str>,
        ) -> ModelResponse {
            self.replies.borrow_mut().pop_front().unwrap_or_default()
        }
    }

    #[tokio::test]
    async fn m0_end_to_end_scoped_agent_runs_allowed_work_and_blocks_overreach() {
        let root = crate::test_support::unique_temp_dir("ncx_scope_m0_e2e");
        let ws = root.join("ws");
        fs::create_dir_all(ws.join("agents")).unwrap();
        fs::create_dir_all(ws.join("data/orders")).unwrap();
        fs::create_dir_all(ws.join("docs/refund-policy")).unwrap();
        fs::create_dir_all(ws.join("secret")).unwrap();

        fs::write(ws.join("agents/refund.toml"), REFUND_TOML).unwrap();
        fs::write(
            ws.join("data/orders/order-1.json"),
            r#"{"id":"order-1","amount":120}"#,
        )
        .unwrap();
        fs::write(
            ws.join("docs/refund-policy/policy.md"),
            "<= 500 auto approve",
        )
        .unwrap();
        fs::write(ws.join("secret/salary.txt"), "TOP-SECRET").unwrap();

        let scope = Rc::new(resolve_scope_with_home(&ws, None, "refund").unwrap());
        let policy = scope.narrow_sandbox(&SandboxPolicy::new(WORKSPACE_WRITE, &ws), None);
        let mem = Rc::new(scope.memory_store(&ws.join(".ncx/memory")).unwrap());
        let approver = Rc::new(CountingApprover {
            decision: ApprovalDecision::Deny,
            calls: Cell::new(0),
        });
        let ctx = ToolContext::new(ws.clone(), policy)
            .with_memory(mem.clone())
            .with_approver(approver.clone())
            .with_tool_filter(scope.tool_filter());
        let mut tools = ToolRegistry::new(ctx);
        scope.install_into_registry(&mut tools);

        // 1. Out-of-scope tools (`shell`, `web_search`) are completely unregistered.
        assert!(tools.get("shell").is_none());
        assert!(tools.get("web_search").is_none());
        assert!(tools.get("read_file").is_some());
        assert!(tools.get("apply_patch").is_some());

        // 2. Drive a full local AgentLoop turn that tries:
        //    - allowed read inside `data/orders`
        //    - forbidden read outside `data.read_roots` (`secret/salary.txt`)
        //    - forbidden write to `agents/refund.toml` (scope self-modification)
        //    - high-value write (`refund_amount: 800`) held by `[[tools.approval]]`
        //    - compliant write (`refund_amount: 120`) into `out/refund/decision-1.json`
        let valid_patch = "*** Begin Patch\n*** Add File: out/refund/decision-1.json\n+{\"decision\":\"approve\",\"refund_amount\":120}\n*** End Patch";
        let high_value_patch = "*** Begin Patch\n*** Add File: out/refund/decision-big.json\n+{\"decision\":\"approve\",\"refund_amount\":800}\n*** End Patch";
        let tamper_scope_patch = "*** Begin Patch\n*** Update File: agents/refund.toml\n@@\n-version = 3\n+version = 99\n*** End Patch";

        let provider = ScriptedLocalProvider {
            replies: std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                ModelResponse {
                    tool_calls: vec![
                        ToolCall {
                            id: "c1".into(),
                            name: "read_file".into(),
                            arguments: json!({"path": "data/orders/order-1.json"}),
                        },
                        ToolCall {
                            id: "c2".into(),
                            name: "read_file".into(),
                            arguments: json!({"path": "secret/salary.txt"}),
                        },
                    ],
                    finish_reason: "tool_calls".into(),
                    ..Default::default()
                },
                ModelResponse {
                    tool_calls: vec![
                        ToolCall {
                            id: "c3".into(),
                            name: "apply_patch".into(),
                            arguments: json!({"patch": tamper_scope_patch}),
                        },
                        ToolCall {
                            id: "c4".into(),
                            name: "apply_patch".into(),
                            arguments: json!({"patch": high_value_patch}),
                        },
                        ToolCall {
                            id: "c5".into(),
                            name: "apply_patch".into(),
                            arguments: json!({"patch": valid_patch}),
                        },
                        ToolCall {
                            id: "c6".into(),
                            name: "remember".into(),
                            arguments: json!({"note": "Order-1 followed <=500 rule", "tags": ["refund"]}),
                        },
                    ],
                    finish_reason: "tool_calls".into(),
                    ..Default::default()
                },
                ModelResponse {
                    content: "结论：同意退款 120 元（大额与越权操作均已拦截）。".into(),
                    finish_reason: "stop".into(),
                    ..Default::default()
                },
            ])),
        };

        let mut agent = AgentLoop::new(
            Box::new(provider),
            tools,
            Session::new(&scope.context_fragment()),
        );
        let result = agent.run_turn(json!("处理 order-1 退款工单"), None).await;

        assert_eq!(result.stop_reason, "completed");
        // Allowed output file exists; high-value denied file does not; scope manifest untouched!
        assert!(ws.join("out/refund/decision-1.json").is_file());
        assert!(!ws.join("out/refund/decision-big.json").exists());
        assert!(fs::read_to_string(ws.join("agents/refund.toml"))
            .unwrap()
            .contains("version = 3"));
        // Approval handler was invoked ONLY for the >500 patch, NOT for tampering with agents/refund.toml!
        assert_eq!(approver.calls.get(), 1);
        // Secret file content never entered conversation transcript.
        let transcript = serde_json::to_string(&agent.session.full_messages()).unwrap();
        assert!(!transcript.contains("TOP-SECRET"));
        assert!(transcript.contains("order-1"));
        // Memory wrote into `.ncx/memory/scopes/refund/LEARNINGS.md`, NOT global `.ncx/memory/LEARNINGS.md`.
        assert!(ws.join(".ncx/memory/scopes/refund/LEARNINGS.md").is_file());
        assert!(!ws.join(".ncx/memory/LEARNINGS.md").exists());
    }
}
