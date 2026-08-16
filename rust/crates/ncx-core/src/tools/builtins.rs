/// `update_plan` â€” record a step plan into the shared context.
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

/// `shell` â€” run a command under the sandbox + approval state machine. Port of
/// `nanocodex/tools/shell.py`. Without this the agent can't build, test, or run
/// git. Not read-only (always sequential).
pub struct ShellTool;

impl ShellTool {
    /// Does this command want something the sandbox forbids? (Heuristic, mirrors
    /// the Python `_needs_escalation`.)
    pub(crate) fn needs_escalation(ctx: &ToolContext, command: &str, workdir: &Path) -> bool {
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
        let workdir = resolve_shell_workdir(ctx, args);
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
        let needs_escalation = ShellTool::needs_escalation(ctx, command, &workdir);
        if let Err(error) = authorize_shell(ctx, command, &workdir, justification, needs_escalation).await {
            return error;
        }
        run_shell(ctx, command, &workdir, timeout, justification).await
    }
}

pub(crate) fn resolve_shell_workdir(ctx: &ToolContext, args: &Value) -> PathBuf {
    let Some(path) = args.get("workdir").and_then(Value::as_str).filter(|p| !p.is_empty()) else {
        return ctx.workspace.clone();
    };
    let path = PathBuf::from(path);
    let joined = if path.is_absolute() { path } else { ctx.workspace.join(path) };
    joined.canonicalize().unwrap_or(joined)
}

pub(crate) async fn authorize_shell(
    ctx: &ToolContext,
    command: &str,
    workdir: &Path,
    justification: &str,
    needs_escalation: bool,
) -> Result<(), String> {
    let pre_allowed = ctx.session_grants.borrow().commands.contains(command);
    let decision = if pre_allowed {
        Decision::AutoApprove
    } else {
        Approver::new(&ctx.approval_policy).classify(command, needs_escalation)
    };
    match decision {
        Decision::AutoApprove => Ok(()),
        Decision::AutoDeny => Err("Error: command denied by approval policy 'never' (it requires \
            escalated permissions). Adjust the approach to stay within the sandbox, or ask the \
            user to change the policy.".into()),
        Decision::Ask => request_shell_approval(
            ctx,
            command,
            workdir,
            justification,
            needs_escalation,
        ).await,
    }
}

async fn request_shell_approval(
    ctx: &ToolContext,
    command: &str,
    workdir: &Path,
    justification: &str,
    escalated: bool,
) -> Result<(), String> {
    let Some(handler) = &ctx.approver else {
        return Err("Error: command requires approval but no approver is configured.".into());
    };
    let reason = if justification.is_empty() { "Command requires approval." } else { justification };
    let answer = handler.request(ApprovalRequest {
        command: command.to_string(),
        reason: reason.to_string(),
        cwd: workdir.display().to_string(),
        escalated,
        details: String::new(),
    }).await;
    if !answer.approved() {
        return Err("Error: command not approved by the user.".into());
    }
    if answer == ApprovalDecision::Always {
        ctx.session_grants.borrow_mut().commands.insert(command.to_string());
    }
    Ok(())
}

pub(crate) async fn run_shell(
    ctx: &ToolContext,
    command: &str,
    workdir: &Path,
    timeout: u64,
    justification: &str,
) -> String {
    let executor = PolicyExecutor::new();
    let mut result = executor.run(command, workdir, timeout).await;
    if !result.ok()
        && ctx.approval_policy == ON_FAILURE
        && !result.timed_out
        && approve_failed_retry(ctx, command, workdir, justification, result.exit_code).await
    {
        result = executor.run(command, workdir, timeout).await;
    }
    result.render()
}

async fn approve_failed_retry(
    ctx: &ToolContext,
    command: &str,
    workdir: &Path,
    justification: &str,
    exit_code: i32,
) -> bool {
    let Some(handler) = &ctx.approver else { return false };
    handler.request(ApprovalRequest {
        command: command.to_string(),
        reason: format!("Sandboxed run failed (exit {exit_code}). {justification}").trim().to_string(),
        cwd: workdir.display().to_string(),
        escalated: true,
        details: String::new(),
    }).await.approved()
}

/// `remember` â€” record a verified, reusable note into project memory. Only the
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
         it. Only record things you have actually confirmed â€” not guesses. \
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
            Ok(false) => "Already in project memory (or empty) â€” not duplicated.".into(),
            Err(e) => format!("Error saving to memory: {e}"),
        }
    }
}

/// `skill` â€” load the full instructions for a discovered Agent Skill
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
        let Some(skill) = ctx
            .skills
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
        else {
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
