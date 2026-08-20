/// `read_file` â€” line-numbered reads. Read-only.
pub struct ReadFileTool;

#[async_trait(?Send)]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a text file and return its contents as 'LINE| TEXT'. Automatically decodes UTF-8, \
         UTF-8 BOM, UTF-16, GB18030/GBK, and Windows-1252 while rejecting binary files. Use \
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
        match decode_text(&raw) {
            Ok(decoded) => {
                let rendered = rf::render(path, &decoded.text, offset, limit);
                if decoded.encoding.is_plain_utf8() {
                    rendered
                } else {
                    format!("[decoded as {}]\n{rendered}", decoded.encoding.label())
                }
            }
            Err(reason) => format!("Error: cannot read text file {path}: {reason}."),
        }
    }
}

/// `apply_patch` â€” Codex V4A edits. Write (serial).
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

        // Plan mode (CC): edits are disabled â€” investigate and propose, change nothing.
        if ctx.plan_mode {
            return "plan mode: edits are disabled â€” propose a plan for the user to approve; \
                    no files were changed."
                .into();
        }

        let actions = match parse_patch(patch) {
            Ok(a) => a,
            Err(e) => return format!("Error applying patch: {e}"),
        };
        let escaping = escaping_targets(ctx, &actions);
        let approved = match approve_patch(ctx, patch, escaping).await {
            Ok(paths) => paths,
            Err(error) => return error,
        };

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

fn escaping_targets(ctx: &ToolContext, actions: &[ncx_tools::FileAction]) -> Vec<PathBuf> {
    let root = ctx
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| ctx.workspace.clone());
    let mut escaping = Vec::new();
    for action in actions {
        let paths = std::iter::once(&action.path).chain(action.move_to.iter());
        for relative in paths {
            let joined = root.join(relative);
            let target = joined.canonicalize().unwrap_or(joined);
            if !ctx.policy.can_write(&target) && !escaping.contains(&target) {
                escaping.push(target);
            }
        }
    }
    escaping
}

async fn approve_patch(
    ctx: &ToolContext,
    patch: &str,
    escaping: Vec<PathBuf>,
) -> Result<HashSet<PathBuf>, String> {
    let edits_granted = ctx.session_grants.borrow().allow_edits;
    let needs_prompt = !escaping.is_empty() || (ctx.require_edit_approval && !edits_granted);
    let Some(approver) = ctx.approver.as_ref().filter(|_| needs_prompt) else {
        return Ok(HashSet::new());
    };
    let (command, reason, escalated) = patch_approval_details(&escaping);
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
        return Err("Error: patch not approved by the user.".into());
    }
    if decision == ApprovalDecision::Always && escaping.is_empty() {
        ctx.session_grants.borrow_mut().allow_edits = true;
    }
    Ok(escaping.into_iter().collect())
}

fn patch_approval_details(escaping: &[PathBuf]) -> (String, String, bool) {
    if escaping.is_empty() {
        return (
            "apply_patch".to_string(),
            "Approve this edit before it is applied.".to_string(),
            false,
        );
    }
    let paths = escaping
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!("apply_patch writing outside the sandbox: {paths}"),
        "The patch modifies files outside the writable roots.".to_string(),
        true,
    )
}
