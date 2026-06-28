//! Tool trait, execution context, and registry — Rust port of
//! `nanocodex/tools/base.py` + the core tool set (`read_file`, `apply_patch`,
//! `update_plan`) and `nanocodex/tools/__init__.py`'s `ToolRegistry`.
//!
//! Single-threaded by design (the REPL runs on a current-thread runtime), so
//! shared mutable state (the plan) uses `Rc<RefCell<…>>`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use async_trait::async_trait;
use ncx_config::HookConfig;
pub use ncx_sandbox::ApprovalRequest;
use ncx_sandbox::{Approver, Decision, SandboxPolicy, DANGER_FULL_ACCESS, ON_FAILURE};
use ncx_tools::{apply_patch, looks_read_only, parse_patch, read_file as rf, PolicyExecutor};
use serde_json::{json, Value};

use crate::genome::Genome;
use crate::hooks::{run_matching_hooks, HookEvent};
use crate::memory::MemoryStore;
use crate::skills::Skill;

const DEFAULT_VISIBLE_TOOL_LIMIT: usize = 9;
const ALWAYS_VISIBLE_TOOLS: &[&str] = &[
    "read_file",
    "apply_patch",
    "update_plan",
    "shell",
    "tool_search",
    "skill",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub description: String,
    pub read_only: bool,
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

/// Session-scoped "always allow" grants. Shared (`Rc<RefCell<…>>`) so it survives
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
    /// Approval policy name (`on-request` etc.) — drives the `shell` tool's
    /// auto-approve / ask / deny decision via [`ncx_sandbox::Approver`].
    pub approval_policy: String,
    /// When true (CC "default" mode), every `apply_patch` write — even inside the
    /// workspace — prompts the approver. False = in-workspace edits apply silently.
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
    /// Optional approval prompt. `None` = no prompting (escalations then rely on
    /// the policy alone, i.e. an out-of-sandbox write simply fails).
    pub approver: Option<Rc<dyn ApprovalHandler>>,
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
    /// Training-time harness overrides (NCX_GENOME). Empty by default — a no-op.
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
            approver: None,
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
}

impl ToolRegistry {
    /// Build the default registry: read_file, apply_patch, update_plan, shell.
    pub fn new(ctx: ToolContext) -> Self {
        let mut reg = ToolRegistry {
            ctx,
            tools: Vec::new(),
            by_name: HashMap::new(),
        };
        reg.register(Box::new(ReadFileTool));
        reg.register(Box::new(ApplyPatchTool));
        reg.register(Box::new(UpdatePlanTool));
        reg.register(Box::new(ShellTool));
        reg.register(Box::new(crate::search::GrepTool));
        reg.register(Box::new(crate::search::GlobTool));
        reg.register(Box::new(crate::search::WebSearchTool));
        reg.register(Box::new(crate::search::WebFetchTool));
        reg.register(Box::new(ToolSearchTool));
        // Only expose `remember` when a memory store is wired (CLI/GUI supply it).
        if reg.ctx.memory.is_some() {
            reg.register(Box::new(RememberTool));
        }
        // Only expose `skill` when at least one SKILL.md was discovered.
        if !reg.ctx.skills.is_empty() {
            reg.register(Box::new(SkillTool));
        }
        reg
    }

    /// Empty registry (tests register exactly what they need).
    pub fn empty(ctx: ToolContext) -> Self {
        ToolRegistry {
            ctx,
            tools: Vec::new(),
            by_name: HashMap::new(),
        }
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
            description,
            read_only: tool.read_only(),
        });
        self.tools.push(tool);
        self.by_name.insert(name, idx);
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
            return self.tools.iter().map(|t| self.schema_for(t.as_ref())).collect();
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
        match self.get(name) {
            Some(tool) => {
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
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if !hook_notes.is_empty() {
                    result.push_str("\n\n[hook output]\n");
                    result.push_str(&hook_notes);
                }
                result
            }
            None => format!("Error: unknown tool '{name}'."),
        }
    }
}

// ── concrete tools ────────────────────────────────────────────────────────────

/// `tool_search` — discover tools by name/description when the registry is too
/// large to expose every schema every turn. Read-only.
pub struct ToolSearchTool;

#[async_trait(?Send)]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }
    fn description(&self) -> &str {
        "Search available tools by keyword when you need a capability that is not currently visible. Returns matching tool names and makes them available next turn."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Capability or tool keywords to search for."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
            },
            "required": ["query"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(query) = args.get("query").and_then(|v| v.as_str()) else {
            return "Error: 'query' is required and must be a string.".into();
        };
        let max = args
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(8)
            .clamp(1, 20) as usize;
        let q = tool_words(query);
        let catalog = ctx.tool_catalog.borrow();
        let mut scored: Vec<(i64, &ToolCatalogEntry)> = catalog
            .iter()
            .map(|e| (catalog_score(e, &q), e))
            .filter(|(s, _)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let mut hints = ctx.tool_hints.borrow_mut();
        hints.clear();
        if scored.is_empty() {
            return format!("No tools matched '{query}'.");
        }
        let mut out = format!("Tools matching '{query}':");
        for (_, entry) in scored.into_iter().take(max) {
            hints.push(entry.name.clone());
            out.push_str(&format!(
                "\n- {}{}: {}",
                entry.name,
                if entry.read_only { " (read-only)" } else { "" },
                entry.description
            ));
        }
        out
    }
}

fn tool_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in s
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
    {
        let w = raw.trim_matches('_');
        if w.len() >= 2 && !out.iter().any(|x| x == w) {
            out.push(w.to_string());
        }
    }
    out
}

fn catalog_score(entry: &ToolCatalogEntry, query_words: &[String]) -> i64 {
    if query_words.is_empty() {
        return 0;
    }
    let hay = format!(
        "{} {}",
        entry.name.to_lowercase(),
        entry.description.to_lowercase()
    );
    let mut score = 0;
    for q in query_words {
        if entry.name.eq_ignore_ascii_case(q) {
            score += 100;
        } else if entry.name.to_lowercase().contains(q) {
            score += 50;
        } else if hay.contains(q) {
            score += 20;
        }
    }
    score
}

/// `read_file` — line-numbered reads. Read-only.
pub struct ReadFileTool;

#[async_trait(?Send)]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a UTF-8 text file and return its contents as 'LINE| TEXT'. Use \
         'offset' (1-indexed) and 'limit' for large files."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path (absolute or workspace-relative)."},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1},
            },
            "required": ["path"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
            return "Error: 'path' is required and must be a string.".into();
        };
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let p = PathBuf::from(path);
        let abs = if p.is_absolute() {
            p
        } else {
            ctx.workspace.join(path)
        };
        let resolved = abs.canonicalize().unwrap_or(abs);

        if !ctx.policy.can_read(&resolved) {
            return format!("Error: reading {path} is not allowed under the sandbox policy.");
        }
        if !resolved.exists() {
            return format!("Error: file not found: {path}");
        }
        if !resolved.is_file() {
            return format!("Error: not a file: {path}");
        }
        let raw = match std::fs::read(&resolved) {
            Ok(b) => b,
            Err(e) => return format!("Error reading file: {e}"),
        };
        match std::str::from_utf8(&raw) {
            Ok(text) => rf::render(path, text, offset, limit),
            Err(_) => format!("Error: cannot read non-UTF-8 file {path}."),
        }
    }
}

/// `apply_patch` — Codex V4A edits. Write (serial).
pub struct ApplyPatchTool;

#[async_trait(?Send)]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }
    fn description(&self) -> &str {
        // The format rules + worked example are load-bearing: without them the
        // model emits git/unified-diff syntax (--- /dev/null, @@ -0,0 +1 @@),
        // which this V4A parser rejects, and the turn loops. Mirrors the Python
        // ApplyPatchTool description exactly.
        "Create, update, delete, or move files using the V4A patch format. \
         This is the preferred way to edit code. The patch must be wrapped in \
         '*** Begin Patch' / '*** End Patch'. Use '*** Add File: <path>', \
         '*** Update File: <path>', '*** Delete File: <path>', and optional \
         '*** Move to: <path>'. Inside an Add File, prefix every new line with \
         '+'. Inside an Update File, prefix context lines with a space, removed \
         lines with '-', added lines with '+', and use '@@ <context>' to locate \
         the right spot. Do NOT use git/unified-diff syntax ('--- a/file', \
         '+++ b/file', '@@ -1,2 +3,4 @@'). Example to create a file:\n\
         *** Begin Patch\n\
         *** Add File: src/hello.txt\n\
         +hi.\n\
         *** End Patch\n\
         Example to edit a file:\n\
         *** Begin Patch\n\
         *** Update File: src/app.py\n\
         @@ def main():\n\
         -    print(\"hi\")\n\
         +    print(\"hello\")\n\
         *** End Patch"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {"type": "string", "description": "Full patch text incl. Begin/End markers."},
            },
            "required": ["patch"],
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(patch) = args.get("patch").and_then(|v| v.as_str()) else {
            return "Error: 'patch' is required and must be a string.".into();
        };

        // Plan mode (CC): edits are disabled — investigate and propose, change nothing.
        if ctx.plan_mode {
            return "plan mode: edits are disabled — propose a plan for the user to approve; \
                    no files were changed."
                .into();
        }

        // Parse up front to find any target outside the writable sandbox. Those
        // require approval (mirrors how Codex/the Python tool escalates). A parse
        // error is reported directly without involving the approval layer.
        let actions = match parse_patch(patch) {
            Ok(a) => a,
            Err(e) => return format!("Error applying patch: {e}"),
        };
        let root = ctx
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| ctx.workspace.clone());
        let resolve = |rel: &str| -> PathBuf {
            let joined = root.join(rel);
            joined.canonicalize().unwrap_or(joined)
        };
        let mut escaping: Vec<PathBuf> = Vec::new();
        for a in &actions {
            let mut rels = vec![a.path.clone()];
            if let Some(m) = &a.move_to {
                rels.push(m.clone());
            }
            for rel in rels {
                let target = resolve(&rel);
                if !ctx.policy.can_write(&target) && !escaping.contains(&target) {
                    escaping.push(target);
                }
            }
        }

        // Prompt when the patch escapes the sandbox OR when the mode requires
        // approval for every edit (CC "default") — unless the user already chose
        // "always allow edits" this session. A single prompt covers the patch.
        let edits_granted = ctx.session_grants.borrow().allow_edits;
        let needs_prompt = !escaping.is_empty() || (ctx.require_edit_approval && !edits_granted);
        let mut approved: HashSet<PathBuf> = HashSet::new();
        if needs_prompt {
            if let Some(approver) = &ctx.approver {
                let (command, reason, escalated) = if escaping.is_empty() {
                    (
                        "apply_patch".to_string(),
                        "Approve this edit before it is applied.".to_string(),
                        false,
                    )
                } else {
                    let rels = escaping
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    (
                        format!("apply_patch writing outside the sandbox: {rels}"),
                        "The patch modifies files outside the writable roots.".to_string(),
                        true,
                    )
                };
                let decision = approver
                    .request(ApprovalRequest {
                        command,
                        reason,
                        cwd: ctx.workspace.display().to_string(),
                        escalated,
                        details: patch.to_string(),
                    })
                    .await;
                if !decision.approved() {
                    return "Error: patch not approved by the user.".into();
                }
                // "Always" on an in-workspace edit prompt = stop asking for edits
                // this session (escaping writes are never blanket-granted).
                if decision == ApprovalDecision::Always && escaping.is_empty() {
                    ctx.session_grants.borrow_mut().allow_edits = true;
                }
                approved = escaping.into_iter().collect();
            }
            // No approver: fall through — can_write rejects out-of-sandbox targets
            // (prior behavior); in-workspace edits proceed (the CLI never sets
            // require_edit_approval, so this only affects approver-less callers).
        }

        let policy = ctx.policy.clone();
        let can_write = move |p: &Path| policy.can_write(p) || approved.contains(p);
        match apply_patch(patch, &ctx.workspace, can_write) {
            Ok(outcome) => {
                let summary = outcome.summary();
                if summary.is_empty() {
                    "Patch applied (no changes).".into()
                } else {
                    format!("Patch applied successfully:\n{summary}")
                }
            }
            Err(e) => format!("Error applying patch: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

    fn tmp_ws(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncx_approve_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    struct Answer(bool);
    #[async_trait(?Send)]
    impl ApprovalHandler for Answer {
        async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
            if self.0 {
                ApprovalDecision::Once
            } else {
                ApprovalDecision::Deny
            }
        }
    }

    struct AlwaysAnswer;
    #[async_trait(?Send)]
    impl ApprovalHandler for AlwaysAnswer {
        async fn request(&self, _req: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Always
        }
    }

    // A patch whose Add-File path climbs out of the workspace needs approval.
    const ESCAPING: &str = "*** Begin Patch\n*** Add File: ../escape.txt\n+x\n*** End Patch";

    #[tokio::test]
    async fn denied_escaping_patch_is_blocked() {
        let ws = tmp_ws("deny");
        let ctx = ToolContext::new(
            ws,
            SandboxPolicy::new(WORKSPACE_WRITE, std::env::temp_dir()),
        )
        .with_approver(Rc::new(Answer(false)));
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": ESCAPING }))
            .await;
        assert!(out.contains("not approved"), "{out}");
    }

    #[tokio::test]
    async fn no_approver_escaping_patch_errors_out_of_sandbox() {
        let ws = tmp_ws("noapprover");
        let ctx = ToolContext::new(
            ws,
            SandboxPolicy::new(WORKSPACE_WRITE, std::env::temp_dir()),
        );
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": ESCAPING }))
            .await;
        // Without an approver the write is simply rejected by the policy.
        assert!(out.contains("Error applying patch"), "{out}");
        assert!(out.contains("outside the writable sandbox"), "{out}");
    }

    #[tokio::test]
    async fn in_workspace_patch_needs_no_approval() {
        let ws = tmp_ws("inws");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let patch = "*** Begin Patch\n*** Add File: ok.txt\n+hi\n*** End Patch";
        let out = ApplyPatchTool
            .execute(&ctx, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("Patch applied successfully"), "{out}");
        assert_eq!(std::fs::read_to_string(ws.join("ok.txt")).unwrap(), "hi\n");
    }

    #[tokio::test]
    async fn plan_mode_refuses_edits() {
        let ws = tmp_ws("planmode");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(true))) // even with an approver saying yes…
            .with_plan_mode(true);
        let patch = "*** Begin Patch\n*** Add File: nope.txt\n+x\n*** End Patch";
        let out = ApplyPatchTool.execute(&ctx, &json!({ "patch": patch })).await;
        assert!(out.contains("plan mode"), "{out}");
        assert!(!ws.join("nope.txt").exists(), "no file should be written");
    }

    #[tokio::test]
    async fn require_edit_approval_prompts_in_workspace() {
        let ws = tmp_ws("editapprove");
        let patch = "*** Begin Patch\n*** Add File: gated.txt\n+hi\n*** End Patch";
        // Denied → not applied, even though the path is inside the workspace.
        let denied = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)))
            .with_require_edit_approval(true);
        let out = ApplyPatchTool
            .execute(&denied, &json!({ "patch": patch }))
            .await;
        assert!(out.contains("not approved"), "{out}");
        assert!(!ws.join("gated.txt").exists(), "denied edit must not write");

        // Approved → applied.
        let ok = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(true)))
            .with_require_edit_approval(true);
        let out2 = ApplyPatchTool.execute(&ok, &json!({ "patch": patch })).await;
        assert!(out2.contains("Patch applied successfully"), "{out2}");
        assert_eq!(std::fs::read_to_string(ws.join("gated.txt")).unwrap(), "hi\n");
    }

    #[tokio::test]
    async fn always_allow_edits_skips_later_prompts() {
        let ws = tmp_ws("alwaysedit");
        let grants = Rc::new(RefCell::new(SessionGrants::default()));
        // First edit: user picks "Always" → applied + grant remembered.
        let ctx1 = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(AlwaysAnswer))
            .with_require_edit_approval(true)
            .with_session_grants(grants.clone());
        let p1 = "*** Begin Patch\n*** Add File: a.txt\n+1\n*** End Patch";
        let o1 = ApplyPatchTool.execute(&ctx1, &json!({ "patch": p1 })).await;
        assert!(o1.contains("Patch applied successfully"), "{o1}");
        assert!(grants.borrow().allow_edits, "edit grant remembered");
        // Second edit shares the grants but has a DENY approver — the session grant
        // means it is never consulted, so the edit still applies.
        let ctx2 = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_approver(Rc::new(Answer(false)))
            .with_require_edit_approval(true)
            .with_session_grants(grants.clone());
        let p2 = "*** Begin Patch\n*** Add File: b.txt\n+2\n*** End Patch";
        let o2 = ApplyPatchTool.execute(&ctx2, &json!({ "patch": p2 })).await;
        assert!(o2.contains("Patch applied successfully"), "{o2}");
        assert!(ws.join("b.txt").exists(), "second edit applied without prompt");
    }

    #[tokio::test]
    async fn shell_read_only_command_auto_runs() {
        // A read-only command under on-request auto-approves and runs — no approver.
        let ws = tmp_ws("shell_ro");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "echo ncx_shell_ok" }))
            .await;
        assert!(out.contains("ncx_shell_ok"), "{out}");
        assert!(out.contains("Exit code: 0"), "{out}");
    }

    #[tokio::test]
    async fn shell_escalating_command_denied_without_approval() {
        // read-only sandbox: a write-ish command escalates; a denying approver blocks it.
        let ws = tmp_ws("shell_esc");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approver(Rc::new(Answer(false)));
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "rm -rf build" }))
            .await;
        assert!(out.contains("not approved"), "{out}");
    }

    #[tokio::test]
    async fn shell_escalating_command_runs_when_approved() {
        let ws = tmp_ws("shell_ok");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(ncx_sandbox::READ_ONLY, &ws))
            .with_approver(Rc::new(Answer(true)));
        // `mkdir` isn't read-only -> escalates; approved -> actually runs (cross-platform).
        let out = ShellTool
            .execute(&ctx, &json!({ "command": "mkdir ncxsub" }))
            .await;
        assert!(!out.contains("not approved"), "{out}");
        assert!(out.contains("Exit code: 0"), "{out}");
        assert!(ws.join("ncxsub").is_dir());
    }

    struct NamedTool(&'static str, &'static str);
    #[async_trait(?Send)]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            self.1
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            "ok".into()
        }
    }

    #[tokio::test]
    async fn tool_search_returns_matches_and_hints_schema_exposure() {
        let ws = tmp_ws("tool_search");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ToolSearchTool));
        reg.register(Box::new(NamedTool("alpha", "general alpha helper")));
        reg.register(Box::new(NamedTool(
            "deploy",
            "build release packages and installers",
        )));
        reg.register(Box::new(NamedTool("debugger", "inspect failures")));

        let out = reg
            .execute("tool_search", &json!({"query": "installer release"}))
            .await;
        assert!(out.contains("deploy"), "{out}");
        assert!(reg.ctx.tool_hints.borrow().contains(&"deploy".to_string()));

        let schemas = reg.schemas_limited_for_query("", 2);
        let names: Vec<String> = schemas
            .iter()
            .filter_map(|s| s["function"]["name"].as_str().map(String::from))
            .collect();
        assert!(names.contains(&"tool_search".to_string()));
        assert!(names.contains(&"deploy".to_string()));
    }

    fn schema_desc(schemas: &[Value], name: &str) -> Option<String> {
        schemas.iter().find_map(|s| {
            let f = &s["function"];
            if f["name"] == name {
                f["description"].as_str().map(String::from)
            } else {
                None
            }
        })
    }

    #[tokio::test]
    async fn empty_genome_leaves_schema_and_catalog_byte_identical() {
        let ws = tmp_ws("genome_noop");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ReadFileTool));
        // schema description == the tool's own default
        let schemas = reg.schemas_limited_for_query("", 9);
        assert_eq!(
            schema_desc(&schemas, "read_file").as_deref(),
            Some(ReadFileTool.description())
        );
        // catalog description == default too
        let cat = reg.ctx.tool_catalog.borrow();
        assert_eq!(cat[0].description, ReadFileTool.description());
    }

    #[tokio::test]
    async fn genome_override_reaches_schema_and_catalog() {
        use crate::genome::Genome;
        let ws = tmp_ws("genome_override");
        let mut g = Genome::default();
        g.tool_desc
            .insert("read_file".into(), "OVERRIDDEN read desc".into());
        let ctx =
            ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws)).with_genome(g);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(ReadFileTool));
        reg.register(Box::new(ShellTool));

        // The model-facing schema shows the override for read_file...
        let schemas = reg.schemas_limited_for_query("", 9);
        assert_eq!(
            schema_desc(&schemas, "read_file").as_deref(),
            Some("OVERRIDDEN read desc")
        );
        // ...and shell (no override) keeps its default.
        assert_eq!(
            schema_desc(&schemas, "shell").as_deref(),
            Some(ShellTool.description())
        );
        // tool_search's catalog sees the override too.
        let cat = reg.ctx.tool_catalog.borrow();
        let rf = cat.iter().find(|e| e.name == "read_file").unwrap();
        assert_eq!(rf.description, "OVERRIDDEN read desc");
    }

    #[tokio::test]
    async fn skill_tool_loads_body_and_reports_unknown() {
        use crate::skills::Skill;
        let ws = tmp_ws("skill_tool");
        let dir = ws.join("skills").join("greeter");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: greeter\ndescription: say hi\n---\n\nStep 1: greet warmly.",
        )
        .unwrap();
        let skill = Skill {
            name: "greeter".into(),
            description: "say hi".into(),
            path: dir.join("SKILL.md"),
            dir: dir.clone(),
            embedded: None,
        };
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_skills(vec![skill]);

        let out = SkillTool.execute(&ctx, &json!({"name": "greeter"})).await;
        assert!(out.contains("Step 1: greet warmly."), "{out}");
        assert!(out.contains("greeter"), "{out}");

        let miss = SkillTool.execute(&ctx, &json!({"name": "nope"})).await;
        assert!(miss.contains("no skill named 'nope'"), "{miss}");
        assert!(miss.contains("greeter"), "{miss}");
    }

    #[tokio::test]
    async fn skill_tool_registered_only_when_skills_present() {
        use crate::skills::Skill;
        let ws = tmp_ws("skill_reg");
        let bare = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
        assert!(ToolRegistry::new(bare).get("skill").is_none());

        let withskill = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_skills(vec![Skill {
                name: "x".into(),
                description: String::new(),
                path: ws.join("SKILL.md"),
                dir: ws.clone(),
                embedded: None,
            }]);
        assert!(ToolRegistry::new(withskill).get("skill").is_some());
    }

    #[tokio::test]
    async fn pre_tool_hook_can_block_execution() {
        let ws = tmp_ws("hook_pre_block");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_hooks(vec![HookConfig {
                event: "pre_tool".into(),
                matcher: "dummy".into(),
                command: "exit 1".into(),
                timeout_s: 3,
            }]);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(NamedTool("dummy", "test tool")));

        let out = reg.execute("dummy", &json!({})).await;

        assert!(out.contains("blocked by pre_tool hook"), "{out}");
        assert!(!out.ends_with("ok"), "{out}");
    }

    #[tokio::test]
    async fn post_tool_hook_output_is_returned() {
        let ws = tmp_ws("hook_post_note");
        let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
            .with_hooks(vec![HookConfig {
                event: "post_tool".into(),
                matcher: "*".into(),
                command: "echo post-ok".into(),
                timeout_s: 3,
            }]);
        let mut reg = ToolRegistry::empty(ctx);
        reg.register(Box::new(NamedTool("dummy", "test tool")));

        let out = reg.execute("dummy", &json!({})).await;

        assert!(out.contains("ok"), "{out}");
        assert!(out.contains("[hook output]"), "{out}");
        assert!(out.contains("post-ok"), "{out}");
    }
}

/// `update_plan` — record a step plan into the shared context.
pub struct UpdatePlanTool;

#[async_trait(?Send)]
impl Tool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }
    fn description(&self) -> &str {
        "Record or update the current step plan (a list of {step, status})."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]},
                        },
                    },
                },
            },
            "required": ["plan"],
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(plan) = args.get("plan").and_then(|v| v.as_array()) else {
            return "Error: 'plan' is required and must be an array.".into();
        };
        *ctx.plan.borrow_mut() = plan.clone();
        let n = plan.len();
        format!("Plan updated ({n} steps).")
    }
}

/// `shell` — run a command under the sandbox + approval state machine. Port of
/// `nanocodex/tools/shell.py`. Without this the agent can't build, test, or run
/// git. Not read-only (always sequential).
pub struct ShellTool;

impl ShellTool {
    /// Does this command want something the sandbox forbids? (Heuristic, mirrors
    /// the Python `_needs_escalation`.)
    fn needs_escalation(ctx: &ToolContext, command: &str, workdir: &Path) -> bool {
        if ctx.policy.mode == DANGER_FULL_ACCESS {
            return false;
        }
        if !ctx.policy.writes_allowed() {
            // read-only: anything not plainly read-only escalates.
            return !looks_read_only(command);
        }
        // workspace-write: escalate only if the workdir itself isn't writable.
        !ctx.policy.can_write(workdir)
    }
}

#[async_trait(?Send)]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Run a shell command in the workspace and return its stdout, stderr, and \
         exit code. Use this to build, run tests, inspect the tree, run git, etc. \
         Prefer read_file/apply_patch for reading and editing files. Commands run \
         under a sandbox policy; some require user approval."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "The command to run, as typed in a shell."},
                "workdir": {"type": "string", "description": "Working directory (defaults to the workspace root)."},
                "timeout": {"type": "integer", "minimum": 1, "maximum": 600, "description": "Timeout in seconds."},
                "justification": {"type": "string", "description": "Why this is needed; shown if approval is required."},
            },
            "required": ["command"],
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
            return "Error: 'command' is required and must be a string.".into();
        };
        // Resolve workdir (relative -> under workspace).
        let workdir = match args.get("workdir").and_then(|v| v.as_str()) {
            Some(w) if !w.is_empty() => {
                let p = PathBuf::from(w);
                let abs = if p.is_absolute() {
                    p
                } else {
                    ctx.workspace.join(w)
                };
                abs.canonicalize().unwrap_or(abs)
            }
            _ => ctx.workspace.clone(),
        };
        if !workdir.exists() {
            return format!(
                "Error: working directory does not exist: {}",
                workdir.display()
            );
        }
        let timeout = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(ctx.timeout_s);
        let justification = args
            .get("justification")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let needs_esc = ShellTool::needs_escalation(ctx, command, &workdir);
        // Session "always allow": a command the user blessed this session skips
        // the approval prompt entirely.
        let pre_allowed = ctx.session_grants.borrow().commands.contains(command);
        let decision = if pre_allowed {
            Decision::AutoApprove
        } else {
            Approver::new(&ctx.approval_policy).classify(command, needs_esc)
        };

        match decision {
            Decision::AutoDeny => {
                return "Error: command denied by approval policy 'never' (it requires escalated \
                        permissions). Adjust the approach to stay within the sandbox, or ask the \
                        user to change the policy."
                    .into();
            }
            Decision::Ask => match &ctx.approver {
                Some(h) => {
                    let reason = if justification.is_empty() {
                        "Command requires approval.".to_string()
                    } else {
                        justification.to_string()
                    };
                    let ans = h
                        .request(ApprovalRequest {
                            command: command.to_string(),
                            reason,
                            cwd: workdir.display().to_string(),
                            escalated: needs_esc,
                            details: String::new(),
                        })
                        .await;
                    if !ans.approved() {
                        return "Error: command not approved by the user.".into();
                    }
                    if ans == ApprovalDecision::Always {
                        ctx.session_grants
                            .borrow_mut()
                            .commands
                            .insert(command.to_string());
                    }
                }
                None => {
                    return "Error: command requires approval but no approver is configured."
                        .into();
                }
            },
            Decision::AutoApprove => {}
        }

        let exec = PolicyExecutor::new();
        let mut result = exec.run(command, &workdir, timeout).await;

        // on-failure: if the sandboxed run failed (not a timeout), offer to retry.
        if !result.ok() && ctx.approval_policy == ON_FAILURE && !result.timed_out {
            if let Some(h) = &ctx.approver {
                let ans = h
                    .request(ApprovalRequest {
                        command: command.to_string(),
                        reason: format!(
                            "Sandboxed run failed (exit {}). {justification}",
                            result.exit_code
                        )
                        .trim()
                        .to_string(),
                        cwd: workdir.display().to_string(),
                        escalated: true,
                        details: String::new(),
                    })
                    .await;
                if ans.approved() {
                    result = exec.run(command, &workdir, timeout).await;
                }
            }
        }

        result.render()
    }
}

/// `remember` — record a verified, reusable note into project memory. Only the
/// model should call this for things it has CONFIRMED (a gotcha, a convention, a
/// working solution), so the store stays trustworthy. Recalled notes are
/// surfaced as leads, not facts.
pub struct RememberTool;

#[async_trait(?Send)]
impl Tool for RememberTool {
    fn name(&self) -> &str {
        "remember"
    }
    fn description(&self) -> &str {
        "Save a short, VERIFIED, reusable note to project memory (a gotcha, a \
         project convention, or a confirmed solution) so future sessions recall \
         it. Only record things you have actually confirmed — not guesses. \
         Optionally tag it for retrieval."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note": {"type": "string", "description": "The verified fact/gotcha/solution, one or two sentences."},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Optional keywords for retrieval."},
            },
            "required": ["note"],
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(note) = args.get("note").and_then(|v| v.as_str()) else {
            return "Error: 'note' is required and must be a string.".into();
        };
        let Some(store) = &ctx.memory else {
            return "Error: project memory is not enabled.".into();
        };
        let tags: Vec<String> = args
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match store.remember(note, &tags, now) {
            Ok(true) => "Saved to project memory.".into(),
            Ok(false) => "Already in project memory (or empty) — not duplicated.".into(),
            Err(e) => format!("Error saving to memory: {e}"),
        }
    }
}

/// `skill` — load the full instructions for a discovered Agent Skill
/// (progressive disclosure level 2). The system prompt advertises only each
/// skill's name + description; this returns the complete `SKILL.md` body plus
/// the skill's directory so the model can `read_file` any bundled resources.
/// Read-only.
pub struct SkillTool;

#[async_trait(?Send)]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }
    fn description(&self) -> &str {
        "Load the full instructions for an available skill by name (see the \
         skills list in the system prompt). Call this BEFORE acting when a task \
         matches a skill's description; it returns the skill's detailed playbook \
         and its directory, where bundled helper files can be read with read_file."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The skill name to load (exact match)."},
            },
            "required": ["name"],
        })
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
            return "Error: 'name' is required and must be a string.".into();
        };
        let name = name.trim();
        let Some(skill) = ctx.skills.iter().find(|s| s.name.eq_ignore_ascii_case(name)) else {
            let available = ctx
                .skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return if available.is_empty() {
                "Error: no skills are available.".into()
            } else {
                format!("Error: no skill named '{name}'. Available skills: {available}.")
            };
        };
        match skill.load_body() {
            Ok(body) => {
                let where_ = if skill.is_builtin() {
                    "builtin skill".to_string()
                } else {
                    format!("files in {}", skill.dir.display())
                };
                format!("Skill '{}' ({where_}):\n\n{}", skill.name, body)
            }
            Err(e) => format!("Error loading skill '{}': {e}", skill.name),
        }
    }
}
