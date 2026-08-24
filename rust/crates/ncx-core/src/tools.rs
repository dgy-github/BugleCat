//! Tool trait, execution context, and registry Ã¢â‚¬â€ Rust port of
//! `nanocodex/tools/base.py` + the core tool set (`read_file`, `apply_patch`,
//! `update_plan`) and `nanocodex/tools/__init__.py`'s `ToolRegistry`.
//!
//! Single-threaded by design (the REPL runs on a current-thread runtime), so
//! shared mutable state (the plan) uses `Rc<RefCell<Ã¢â‚¬Â¦>>`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use async_trait::async_trait;
use ncx_config::HookConfig;
pub use ncx_sandbox::ApprovalRequest;
use ncx_sandbox::{Approver, Decision, SandboxPolicy, DANGER_FULL_ACCESS, ON_FAILURE};
use ncx_tools::{
    apply_patch, decode_text, looks_read_only, parse_patch, read_file as rf, PolicyExecutor,
};
use serde_json::{json, Value};

use crate::genome::Genome;
use crate::hooks::{run_matching_hooks, HookEvent};
use crate::lsp_tool::LspProvider;
use crate::memory::MemoryStore;
use crate::process_tools::ProcessManager;
use crate::skills::Skill;
use crate::terminal_tools::TerminalManager;
use crate::tool_middleware::{ToolMiddleware, ToolMiddlewareDecision};
use crate::tool_recovery::{
    classify_tool_result, fallback_call, infer_capabilities, resolve_unique_missing_read,
    ToolCapability,
};
use crate::user_question::UserQuestionHandler;

const DEFAULT_VISIBLE_TOOL_LIMIT: usize = 20;
const ALWAYS_VISIBLE_TOOLS: &[&str] = &[
    "read_file",
    "apply_patch",
    "update_plan",
    "shell",
    "tool_search",
    "skill",
    "list_directory",
    "path_info",
    "find_files",
    "grep",
    "glob",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub description: String,
    pub read_only: bool,
    pub capabilities: Vec<ToolCapability>,
}

/// A user's answer to an approval prompt. `Always` additionally remembers the
/// grant for the rest of the session (a command, or "all edits").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Deny,
    Once,
    Always,
}

impl ApprovalDecision {
    /// True when the action may proceed (`Once` or `Always`).
    pub fn approved(self) -> bool {
        !matches!(self, ApprovalDecision::Deny)
    }
}

/// Session-scoped "always allow" grants. Shared (`Rc<RefCell<Ã¢â‚¬Â¦>>`) so it survives
/// agent rebuilds within one session (model / permission-mode switches) but is
/// dropped when a new/forked/resumed session starts.
#[derive(Debug, Default, Clone)]
pub struct SessionGrants {
    /// Shell commands the user chose to always allow this session (exact match).
    pub commands: HashSet<String>,
    /// True once the user chose to always allow edits this session.
    pub allow_edits: bool,
}

/// Asks the user to approve an escalated action (e.g. a patch writing outside
/// the sandbox). The GUI implements this with a modal round-trip; the CLI with
/// a yes/no prompt; tests with a canned answer. `?Send` to match the loop.
///
/// Named `ApprovalHandler` to avoid clashing with `ncx_sandbox::Approver`
/// (which is the pure policy classifier, not a prompt).
#[async_trait(?Send)]
pub trait ApprovalHandler {
    async fn request(&self, req: ApprovalRequest) -> ApprovalDecision;
}

/// Everything a tool needs, shared (cheaply cloned) across tools.
#[derive(Clone)]
pub struct ToolContext {
    pub workspace: PathBuf,
    pub policy: SandboxPolicy,
    /// Approval policy name (`on-request` etc.) Ã¢â‚¬â€ drives the `shell` tool's
    /// auto-approve / ask / deny decision via [`ncx_sandbox::Approver`].
    pub approval_policy: String,
    /// When true (CC "default" mode), every `apply_patch` write Ã¢â‚¬â€ even inside the
    /// workspace Ã¢â‚¬â€ prompts the approver. False = in-workspace edits apply silently.
    pub require_edit_approval: bool,
    /// When true (CC "plan" mode), `apply_patch` refuses all edits: investigate
    /// and propose a plan, change nothing.
    pub plan_mode: bool,
    /// Session-scoped "always allow" grants (shell commands / all edits).
    pub session_grants: Rc<RefCell<SessionGrants>>,
    /// Default command timeout (seconds) for the `shell` tool.
    pub timeout_s: u64,
    /// Shared mutable plan state for `update_plan` / the CLI to read.
    pub plan: Rc<RefCell<Vec<Value>>>,
    /// Turn that owns `plan`. A plan may remain visible after its turn ends,
    /// but it must never constrain a different user turn.
    pub plan_turn_id: Rc<Cell<Option<u64>>>,
    /// User turn currently being executed by the agent loop.
    pub active_turn_id: Rc<Cell<Option<u64>>>,
    /// Optional approval prompt. `None` = no prompting (escalations then rely on
    /// the policy alone, i.e. an out-of-sandbox write simply fails).
    pub approver: Option<Rc<dyn ApprovalHandler>>,
    /// Optional interactive question boundary. When absent, the question tool
    /// is not registered, so headless runtimes never advertise a dead tool.
    pub user_question_handler: Option<Rc<dyn UserQuestionHandler>>,
    /// Optional Language Server implementation used by the stable `lsp` tool.
    pub lsp_provider: Option<Rc<dyn LspProvider>>,
    pub(crate) process_manager: Rc<tokio::sync::Mutex<ProcessManager>>,
    pub(crate) terminal_manager: Rc<tokio::sync::Mutex<TerminalManager>>,
    /// Optional project memory store. When set, the `remember` tool is exposed.
    pub memory: Option<Rc<MemoryStore>>,
    /// Web search backend ("duckduckgo" | "tavily") and its key (for tavily).
    pub search_provider: String,
    pub search_api_key: String,
    /// Catalog used by `tool_search` and dynamic schema exposure.
    pub tool_catalog: Rc<RefCell<Vec<ToolCatalogEntry>>>,
    /// Tool names requested by `tool_search`; included in the next schema view.
    pub tool_hints: Rc<RefCell<Vec<String>>>,
    /// Deterministic project hooks configured from `[[hooks]]`.
    pub hooks: Rc<Vec<HookConfig>>,
    /// Discovered Agent Skills. When non-empty, the `skill` tool is exposed and
    /// the index is injected into the system prompt by the CLI/GUI.
    pub skills: Rc<Vec<Skill>>,
    /// Training-time harness overrides (NCX_GENOME). Empty by default Ã¢â‚¬â€ a no-op.
    /// Currently overrides per-tool descriptions seen by the model.
    pub genome: Rc<Genome>,
}

impl ToolContext {
    pub fn new(workspace: PathBuf, policy: SandboxPolicy) -> Self {
        ToolContext {
            workspace,
            policy,
            approval_policy: "on-request".to_string(),
            require_edit_approval: false,
            plan_mode: false,
            session_grants: Rc::new(RefCell::new(SessionGrants::default())),
            timeout_s: 120,
            plan: Rc::new(RefCell::new(Vec::new())),
            plan_turn_id: Rc::new(Cell::new(None)),
            active_turn_id: Rc::new(Cell::new(None)),
            approver: None,
            user_question_handler: None,
            lsp_provider: None,
            process_manager: Rc::new(tokio::sync::Mutex::new(ProcessManager::default())),
            terminal_manager: Rc::new(tokio::sync::Mutex::new(TerminalManager::default())),
            memory: None,
            search_provider: "duckduckgo".to_string(),
            search_api_key: String::new(),
            tool_catalog: Rc::new(RefCell::new(Vec::new())),
            tool_hints: Rc::new(RefCell::new(Vec::new())),
            hooks: Rc::new(Vec::new()),
            skills: Rc::new(Vec::new()),
            genome: Rc::new(Genome::default()),
        }
    }

    /// Configure the web search backend the `web_search` tool uses.
    pub fn with_search(mut self, provider: impl Into<String>, api_key: impl Into<String>) -> Self {
        self.search_provider = provider.into();
        self.search_api_key = api_key.into();
        self
    }

    /// Attach a project memory store (enables the `remember` tool).
    pub fn with_memory(mut self, memory: Rc<MemoryStore>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach an approval handler (the GUI/CLI supplies one).
    pub fn with_approver(mut self, approver: Rc<dyn ApprovalHandler>) -> Self {
        self.approver = Some(approver);
        self
    }

    /// Attach an interactive user-question handler.
    pub fn with_user_question_handler(mut self, handler: Rc<dyn UserQuestionHandler>) -> Self {
        self.user_question_handler = Some(handler);
        self
    }

    /// Attach a Language Server provider.
    pub fn with_lsp_provider(mut self, provider: Rc<dyn LspProvider>) -> Self {
        self.lsp_provider = Some(provider);
        self
    }

    /// Set the approval policy the `shell` tool uses to gate commands.
    pub fn with_approval_policy(mut self, policy: impl Into<String>) -> Self {
        self.approval_policy = policy.into();
        self
    }

    /// Require approval for every `apply_patch` write (CC "default" mode).
    pub fn with_require_edit_approval(mut self, on: bool) -> Self {
        self.require_edit_approval = on;
        self
    }

    /// Refuse all `apply_patch` edits (CC "plan" mode).
    pub fn with_plan_mode(mut self, on: bool) -> Self {
        self.plan_mode = on;
        self
    }

    /// Share a session-grants store across agent rebuilds (so "always allow"
    /// survives model / permission-mode switches within one session).
    pub fn with_session_grants(mut self, grants: Rc<RefCell<SessionGrants>>) -> Self {
        self.session_grants = grants;
        self
    }

    /// Set the default shell command timeout (seconds).
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_s = secs;
        self
    }

    /// Attach deterministic project hooks.
    pub fn with_hooks(mut self, hooks: Vec<HookConfig>) -> Self {
        self.hooks = Rc::new(hooks);
        self
    }

    /// Attach discovered Agent Skills (enables the `skill` tool).
    pub fn with_skills(mut self, skills: Vec<Skill>) -> Self {
        self.skills = Rc::new(skills);
        self
    }

    /// Attach training-time harness overrides (NCX_GENOME). Empty = no-op.
    pub fn with_genome(mut self, genome: Genome) -> Self {
        self.genome = Rc::new(genome);
        self
    }
}

/// An agent capability exposed to the model as an OpenAI function tool.
#[async_trait(?Send)]
pub trait Tool {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;

    /// True for pure-read tools (no side effects); the loop may run a run of
    /// consecutive read-only calls concurrently. Default false.
    fn read_only(&self) -> bool {
        false
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String;

    fn to_schema(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.parameters(),
            },
        })
    }
}

/// Holds the tool set and the shared context. `ctx` is public so callers (and
/// tests) can read the plan after a turn.
pub struct ToolRegistry {
    pub ctx: ToolContext,
    tools: Vec<Box<dyn Tool>>,
    by_name: HashMap<String, usize>,
    middleware: Vec<Rc<dyn ToolMiddleware>>,
    middleware_names: HashSet<String>,
    pub(crate) plugin_state: crate::plugins::PluginRuntimeState,
}

impl ToolRegistry {
    pub fn harness_diagnostics(&self) -> crate::plugins::HarnessDiagnostics {
        crate::plugins::HarnessDiagnostics {
            llm: self
                .service::<crate::plugins::LlmServiceDescriptor>("llm.provider")
                .is_some(),
            interaction: self
                .service::<crate::plugins::InteractionService>("interaction")
                .is_some(),
            policy: self
                .service::<crate::plugins::PolicyService>("policy")
                .is_some(),
            context: self
                .service::<crate::plugins::ContextServiceDescriptor>("context")
                .is_some(),
            memory: self
                .service::<crate::plugins::MemoryServiceDescriptor>("memory")
                .is_some(),
            compaction: self
                .service::<crate::plugins::CompactionServiceDescriptor>("compaction")
                .is_some(),
            mcp: self
                .service::<crate::plugins::McpServiceDescriptor>("mcp")
                .is_some(),
            attachment: self
                .service::<crate::plugins::AttachmentServiceDescriptor>("attachment")
                .is_some(),
            media: self
                .service::<crate::plugins::MediaServiceDescriptor>("media")
                .is_some(),
            cost_telemetry: self
                .service::<crate::plugins::CostTelemetryService>("cost.telemetry")
                .is_some(),
        }
    }
    /// Replace a runtime capability service after plugin composition has run.
    /// This is used for frontend-owned implementations such as configured LLMs.
    pub fn replace_service<T: std::any::Any>(&mut self, name: &str, service: Rc<T>) {
        self.plugin_state
            .services
            .insert(name.trim().to_string(), service);
    }
    /// Read a typed capability service installed by the active Harness profile.
    pub fn service<T: std::any::Any>(&self, name: &str) -> Option<Rc<T>> {
        self.plugin_state
            .services
            .get(name)
            .cloned()
            .and_then(|service| service.downcast::<T>().ok())
    }
    /// Install a named Harness plugin bundle into this registry.
    pub fn install_plugin(&mut self, plugin: &dyn crate::plugins::HarnessPlugin) {
        plugin
            .install(
                &mut crate::plugins::PluginHost::new(self),
                &toml::Value::Table(Default::default()),
            )
            .expect("plugin installation must succeed");
    }

    /// Build the default in-process tool registry.
    pub fn new(ctx: ToolContext) -> Self {
        crate::plugins::HarnessRuntimeBuilder::default().build(ctx)
    }

    /// Empty registry (tests register exactly what they need).
    pub fn empty(ctx: ToolContext) -> Self {
        ToolRegistry {
            ctx,
            tools: Vec::new(),
            by_name: HashMap::new(),
            middleware: Vec::new(),
            middleware_names: HashSet::new(),
            plugin_state: Default::default(),
        }
    }

    /// Register one in-process tool layer. Names are unique so configuration
    /// and diagnostics can remove the exact layer they installed.
    pub fn register_middleware(
        &mut self,
        middleware: Rc<dyn ToolMiddleware>,
    ) -> Result<(), String> {
        let name = middleware.name().trim();
        if name.is_empty() {
            return Err("tool middleware name cannot be empty".to_string());
        }
        if !self.middleware_names.insert(name.to_string()) {
            return Err(format!("tool middleware '{name}' is already registered"));
        }
        self.middleware.push(middleware);
        Ok(())
    }

    /// Remove a middleware layer by its stable registration name.
    pub fn unregister_middleware(&mut self, name: &str) -> bool {
        if !self.middleware_names.remove(name) {
            return false;
        }
        self.middleware
            .retain(|middleware| middleware.name() != name);
        true
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        let idx = self.tools.len();
        // Apply any NCX_GENOME description override so tool_search (which scores
        // catalog descriptions) sees the same text the model does.
        let description = self
            .ctx
            .genome
            .describe(&name, tool.description())
            .to_string();
        self.ctx.tool_catalog.borrow_mut().push(ToolCatalogEntry {
            name: name.clone(),
            capabilities: infer_capabilities(&name, &description),
            description,
            read_only: tool.read_only(),
        });
        self.tools.push(tool);
        self.by_name.insert(name, idx);
    }

    /// Atomically replace an owned subset of tools after validating additions.
    ///
    /// Validation happens before the registry is mutated, so duplicate names or
    /// collisions with tools outside `remove_names` leave the old set intact.
    /// This is the commit boundary used by MCP reload: newly connected clients
    /// are prepared first, then old client handles are dropped in one step.
    pub fn replace_tools(
        &mut self,
        remove_names: &[String],
        additions: Vec<Box<dyn Tool>>,
    ) -> Result<Vec<String>, String> {
        let remove: HashSet<&str> = remove_names.iter().map(String::as_str).collect();
        let mut additions_seen = HashSet::new();
        let mut addition_names = Vec::with_capacity(additions.len());
        for tool in &additions {
            let name = tool.name().trim();
            if name.is_empty() {
                return Err("replacement tool name cannot be empty".to_string());
            }
            if !additions_seen.insert(name.to_string()) {
                return Err(format!("replacement contains duplicate tool '{name}'"));
            }
            if self.by_name.contains_key(name) && !remove.contains(name) {
                return Err(format!(
                    "replacement tool '{name}' conflicts with an existing tool"
                ));
            }
            addition_names.push(name.to_string());
        }

        self.tools.retain(|tool| !remove.contains(tool.name()));
        self.tools.extend(additions);
        self.rebuild_tool_indexes();
        Ok(addition_names)
    }

    fn rebuild_tool_indexes(&mut self) {
        self.by_name.clear();
        let mut catalog = Vec::with_capacity(self.tools.len());
        for (index, tool) in self.tools.iter().enumerate() {
            let name = tool.name().to_string();
            self.by_name.insert(name.clone(), index);
            catalog.push(ToolCatalogEntry {
                description: self
                    .ctx
                    .genome
                    .describe(&name, tool.description())
                    .to_string(),
                name,
                read_only: tool.read_only(),
                capabilities: infer_capabilities(tool.name(), tool.description()),
            });
        }
        *self.ctx.tool_catalog.borrow_mut() = catalog;
        self.ctx
            .tool_hints
            .borrow_mut()
            .retain(|name| self.by_name.contains_key(name));
    }

    /// Build a tool's function schema with the effective (possibly genome-
    /// overridden) description. This is the model-facing surface; the `Tool`
    /// trait's own `to_schema()` keeps returning the unmodified default.
    fn schema_for(&self, tool: &dyn Tool) -> Value {
        let description = self.ctx.genome.describe(tool.name(), tool.description());
        json!({
            "type": "function",
            "function": {
                "name": tool.name(),
                "description": description,
                "parameters": tool.parameters(),
            },
        })
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.by_name.get(name).map(|&i| self.tools[i].as_ref())
    }

    pub fn is_read_only(&self, name: &str) -> bool {
        self.get(name).map(|t| t.read_only()).unwrap_or(false)
    }

    /// JSON schemas for every registered tool (the `tools` request field).
    pub fn schemas(&self) -> Vec<Value> {
        self.schemas_for_query("")
    }

    /// JSON schemas for the tool view relevant to the current task. Small
    /// registries expose everything; larger ones expose core tools, recent
    /// `tool_search` hits, and the best lexical matches for `query`.
    pub fn schemas_for_query(&self, query: &str) -> Vec<Value> {
        self.schemas_limited_for_query(query, DEFAULT_VISIBLE_TOOL_LIMIT)
    }

    pub fn schemas_limited_for_query(&self, query: &str, limit: usize) -> Vec<Value> {
        if self.tools.len() <= limit {
            return self
                .tools
                .iter()
                .map(|t| self.schema_for(t.as_ref()))
                .collect();
        }

        let mut selected: HashSet<String> = HashSet::new();
        for name in ALWAYS_VISIBLE_TOOLS {
            if self.by_name.contains_key(*name) {
                selected.insert((*name).to_string());
            }
        }
        for name in self.ctx.tool_hints.borrow().iter() {
            if self.by_name.contains_key(name) {
                selected.insert(name.clone());
            }
        }

        let q = tool_words(query);
        let mut scored: Vec<(i64, String)> = self
            .ctx
            .tool_catalog
            .borrow()
            .iter()
            .filter(|e| !selected.contains(&e.name))
            .map(|e| (catalog_score(e, &q), e.name.clone()))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (score, name) in scored {
            if selected.len() >= limit {
                break;
            }
            if score > 0 || q.is_empty() {
                selected.insert(name);
            }
        }

        self.tools
            .iter()
            .filter(|t| selected.contains(t.name()))
            .map(|t| self.schema_for(t.as_ref()))
            .collect()
    }

    /// Run a tool by name. Unknown tool -> an error string for the model.
    pub async fn execute(&self, name: &str, args: &Value) -> String {
        self.execute_attempt(name, args).await
    }

    /// Execute with one conservative retry and argument-compatible read-only fallbacks.
    pub async fn execute_with_recovery(&self, name: &str, args: &Value) -> String {
        let first = self.execute_attempt(name, args).await;
        let Some(mut failure) = classify_tool_result(&first) else {
            return first;
        };
        if !self.is_read_only(name) {
            return first;
        }

        if name == "read_file" && failure == crate::tool_recovery::ToolFailureClass::NotFound {
            if let Some((resolved, recovered_args)) =
                resolve_unique_missing_read(&self.ctx.workspace, args)
            {
                let recovered = self.execute_attempt(name, &recovered_args).await;
                if classify_tool_result(&recovered).is_none() {
                    return format!(
                        "[recovery: recursively resolved missing file to {resolved}]\n{recovered}"
                    );
                }
            }
        }

        let mut latest = first.clone();
        if failure.retryable() {
            latest = self.execute_attempt(name, args).await;
            let Some(retry_failure) = classify_tool_result(&latest) else {
                return format!("[recovery: retried {name} after {failure}]\n{latest}");
            };
            failure = retry_failure;
        }

        if let Some((fallback_name, fallback_args)) = fallback_call(name, args, failure) {
            if self.is_read_only(fallback_name) {
                let fallback = self.execute_attempt(fallback_name, &fallback_args).await;
                if classify_tool_result(&fallback).is_none() {
                    return format!(
                        "[recovery: {name} -> {fallback_name} after {failure}]\n{fallback}"
                    );
                }
                return format!(
                    "Error: {name} failed ({failure}); fallback {fallback_name} also failed.\n\
                     primary: {first}\nfallback: {fallback}"
                );
            }
        }
        latest
    }

    async fn execute_attempt(&self, name: &str, args: &Value) -> String {
        match self.get(name) {
            Some(tool) => {
                let (entered, blocked) = self.enter_middleware(name, args).await;
                let result = match blocked {
                    Some(result) => result,
                    None => self.execute_with_hooks(tool, name, args).await,
                };
                self.leave_middleware(entered, name, args, result).await
            }
            None => format!("Error: unknown tool '{name}'."),
        }
    }

    async fn enter_middleware(&self, name: &str, args: &Value) -> (usize, Option<String>) {
        for (index, middleware) in self.middleware.iter().enumerate() {
            match middleware.before_execute(&self.ctx, name, args).await {
                ToolMiddlewareDecision::Continue => {}
                ToolMiddlewareDecision::Block { reason } => {
                    return (
                        index + 1,
                        Some(format!(
                            "Error: {name} blocked by tool middleware '{}': {reason}",
                            middleware.name()
                        )),
                    );
                }
            }
        }
        (self.middleware.len(), None)
    }

    async fn leave_middleware(
        &self,
        entered: usize,
        name: &str,
        args: &Value,
        mut result: String,
    ) -> String {
        for middleware in self.middleware[..entered].iter().rev() {
            if let Some(replacement) = middleware
                .after_execute(&self.ctx, name, args, &result)
                .await
            {
                result = replacement;
            }
        }
        result
    }

    async fn execute_with_hooks(&self, tool: &dyn Tool, name: &str, args: &Value) -> String {
        let pre = run_matching_hooks(
            &self.ctx.hooks,
            HookEvent::PreTool,
            name,
            args,
            None,
            &self.ctx.workspace,
        )
        .await;
        if pre.blocked {
            return format!("Error: {name} blocked by pre_tool hook.\n{}", pre.notes);
        }

        let mut result = tool.execute(&self.ctx, args).await;
        let post = run_matching_hooks(
            &self.ctx.hooks,
            HookEvent::PostTool,
            name,
            args,
            Some(&result),
            &self.ctx.workspace,
        )
        .await;
        let hook_notes = [pre.notes, post.notes]
            .into_iter()
            .filter(|note| !note.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !hook_notes.is_empty() {
            result.push_str("\n\n[hook output]\n");
            result.push_str(&hook_notes);
        }
        result
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ concrete tools Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

// Physical responsibility split; include! preserves the established crate::tools API.
include!("tools/catalog.rs");
include!("tools/file.rs");
include!("tools/builtins.rs");
include!("tools/tests.rs");
