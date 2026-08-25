use super::*;

pub(crate) fn render_skills(skills: &[ncx_core::Skill]) -> String {
    if skills.is_empty() {
        return "(no skills available — add SKILL.md dirs under .ncx/skills/)".into();
    }
    let mut out = format!("Available skills ({}):", skills.len());
    for s in skills {
        let tag = if s.is_builtin() { " [builtin]" } else { "" };
        if s.description.is_empty() {
            out.push_str(&format!("\n  {}{tag}", s.name));
        } else {
            out.push_str(&format!("\n  {}{tag}\n      {}", s.name, s.description));
        }
    }
    out.push_str("\n\nThe agent loads a skill's full instructions on demand via the `skill` tool.");
    out
}

pub(crate) fn render_help() -> String {
    let mut out = String::from("Commands:");
    for (cmd, help) in SLASH_HELP {
        out.push_str(&format!("\n  {cmd:<12} {help}"));
    }
    out
}

pub(crate) fn render_help_for_workspace(workspace: &Path) -> String {
    let mut out = render_help();
    let custom = list_custom_commands(workspace);
    if !custom.is_empty() {
        out.push_str("\n\nCustom commands:");
        for cmd in custom {
            out.push_str(&format!(
                "\n  /{}:{:<10} {}",
                cmd.scope,
                cmd.name,
                cmd.path.display()
            ));
        }
        out.push_str("\n  /<name>       Runs project commands before user commands.");
    }
    out
}

pub(crate) fn render_status(cfg: &ncx_config::Config) -> String {
    let red = cfg.redacted();
    format!(
        "model:     {}\nbase_url:  {}\nsandbox:   {}\napproval:  {}\nworkspace: {}\napi_key:   {}\nmodel_budget: {}  tool_budget: {}  retries: {}\ncontext_edit: {}  max_chars: {}  keep_recent: {}  tool_result_chars: {}\nhooks:     {}",
        cfg.model,
        cfg.base_url,
        cfg.sandbox_mode,
        cfg.approval_policy,
        cfg.workspace.display(),
        red.get("api_key").cloned().unwrap_or_default(),
        cfg.max_iterations,
        cfg.max_tool_calls,
        cfg.max_retries,
        cfg.context_edit_enabled,
        cfg.context_edit_max_chars,
        cfg.context_edit_keep_recent_messages,
        cfg.context_edit_max_tool_result_chars,
        cfg.hooks.len(),
    )
}

// ── /export ──────────────────────────────────────────────────────────────────

/// Export the conversation to a Markdown file and return a status line.
///
/// With no argument the file lands at `<workspace>/.nanocodex/exports/<id>.md`,
/// overwriting any prior export there (a managed, per-session location). An
/// explicit argument is the target path (relative paths resolve against the
/// workspace, absolute paths are used as-is); an explicit path that already
/// exists — file or directory — is refused rather than overwritten, so a typo
/// like `/export Cargo.toml` cannot clobber an existing file. The system prompt
/// and every user/assistant/tool message are rendered; inline image data is
/// shown as a `[image]` placeholder, never dumped.
pub(crate) fn export_session_text(
    session: &Session,
    cfg: &ncx_config::Config,
    session_id: &str,
    arg: &str,
) -> String {
    let path = export_target_path(&cfg.workspace, session_id, arg);
    // Only the default managed path may overwrite (its own prior export). An
    // explicitly named destination is never clobbered.
    if !arg.trim().is_empty() {
        if path.is_dir() {
            return format!(
                "export failed: {} is a directory; pass a file path",
                path.display()
            );
        }
        if path.exists() {
            return format!(
                "export failed: {} already exists; choose a different name or delete it first",
                path.display()
            );
        }
    }
    let markdown = render_session_markdown(
        &session.system,
        &session.messages,
        &cfg.model,
        &cfg.workspace,
    );
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return format!("export failed: cannot create {}: {e}", parent.display());
        }
    }
    match std::fs::write(&path, markdown.as_bytes()) {
        Ok(()) => format!(
            "Exported {} message(s) to {}",
            session.messages.len(),
            path.display()
        ),
        Err(e) => format!("export failed: {e}"),
    }
}

/// Resolve where `/export` writes: the trimmed argument as a path (relative to
/// the workspace unless absolute), or a default under `.nanocodex/exports/`.
pub(crate) fn export_target_path(workspace: &Path, session_id: &str, arg: &str) -> PathBuf {
    let arg = arg.trim();
    if arg.is_empty() {
        let name = if session_id.is_empty() {
            "session"
        } else {
            session_id
        };
        return workspace
            .join(".nanocodex")
            .join("exports")
            .join(format!("{name}.md"));
    }
    let p = PathBuf::from(arg);
    if p.is_absolute() {
        p
    } else {
        workspace.join(p)
    }
}

/// Render the system prompt + message history as a single Markdown document.
pub(crate) fn render_session_markdown(
    system: &str,
    messages: &[Value],
    model: &str,
    workspace: &Path,
) -> String {
    let mut out = String::from("# nanocodex session export\n\n");
    let (users, assistants, tools) = count_roles(messages);
    out.push_str(&format!("- model: `{model}`\n"));
    out.push_str(&format!("- workspace: `{}`\n", workspace.display()));
    out.push_str(&format!(
        "- messages: {} (user {users}, assistant {assistants}, tool {tools})\n",
        messages.len()
    ));

    if !system.trim().is_empty() {
        out.push_str("\n## System prompt\n\n");
        out.push_str("<details><summary>show</summary>\n\n");
        push_fenced(&mut out, "", system.trim());
        out.push_str("\n</details>\n");
    }

    for msg in messages {
        match msg.get("role").and_then(|v| v.as_str()).unwrap_or("?") {
            "user" => {
                out.push_str("\n## User\n\n");
                push_block(&mut out, &content_to_markdown(msg.get("content")));
            }
            "assistant" => {
                out.push_str("\n## Assistant\n\n");
                if let Some(reasoning) = msg.get("reasoning_content").and_then(|v| v.as_str()) {
                    if !reasoning.trim().is_empty() {
                        out.push_str("<details><summary>reasoning</summary>\n\n");
                        push_fenced(&mut out, "", reasoning.trim());
                        out.push_str("\n</details>\n\n");
                    }
                }
                let content = content_to_markdown(msg.get("content"));
                if !content.trim().is_empty() {
                    push_block(&mut out, &content);
                }
                if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    for call in calls {
                        let func = call.get("function");
                        let name = func
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        let args = func
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        out.push_str(&format!("\n**tool call: `{name}`**\n\n"));
                        push_fenced(&mut out, "json", args.trim());
                    }
                }
            }
            "tool" => {
                let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                out.push_str(&format!("\n### Tool result: `{name}`\n\n"));
                let tool_content = content_to_markdown(msg.get("content"));
                push_fenced(&mut out, "", tool_content.trim());
            }
            other => {
                out.push_str(&format!("\n## {other}\n\n"));
                push_block(&mut out, &content_to_markdown(msg.get("content")));
            }
        }
    }
    out
}

pub(crate) fn push_block(out: &mut String, text: &str) {
    out.push_str(text.trim_end());
    out.push('\n');
}

/// Choose a Markdown code fence that can wrap `content` verbatim: one backtick
/// longer than the longest run of backticks inside it (CommonMark), min 3. This
/// keeps tool results / file contents that themselves contain ``` from breaking
/// out of the exported code block.
pub(crate) fn code_fence(content: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for c in content.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

/// Push a fenced code block (optionally language-tagged) whose fence is sized to
/// contain `content` without being terminated early.
pub(crate) fn push_fenced(out: &mut String, lang: &str, content: &str) {
    let fence = code_fence(content);
    out.push_str(&fence);
    out.push_str(lang);
    out.push('\n');
    out.push_str(content);
    if !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push('\n');
}

pub(crate) fn count_roles(messages: &[Value]) -> (usize, usize, usize) {
    let (mut users, mut assistants, mut tools) = (0, 0, 0);
    for msg in messages {
        match msg.get("role").and_then(|v| v.as_str()) {
            Some("user") => users += 1,
            Some("assistant") => assistants += 1,
            Some("tool") => tools += 1,
            _ => {}
        }
    }
    (users, assistants, tools)
}

/// Flatten an OpenAI-shape `content` field (string, or a multimodal block array)
/// into Markdown text. Inline image blocks become a `[image]` placeholder so an
/// export never dumps base64 data.
pub(crate) fn content_to_markdown(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => {
            let mut parts = Vec::new();
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("image_url") {
                    parts.push("[image]".to_string());
                } else if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
            parts.join("\n\n")
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

// ── /review · /security-review · /verify ─────────────────────────────────────
// These return a canned prompt that the REPL runs as a normal turn, so the agent
// drives the work with its tools (shell for git diff / tests, read_file, …). A
// trailing argument narrows the scope.

pub(crate) fn scope_suffix(arg: &str) -> String {
    let arg = arg.trim();
    if arg.is_empty() {
        String::new()
    } else {
        format!("\n\nScope/focus for this pass: {arg}")
    }
}

pub(crate) fn review_prompt(arg: &str) -> String {
    format!(
        "Review the current code changes for correctness and quality.\n\n\
         1. Run `git --no-pager diff` and `git --no-pager diff --staged` via the shell tool to see the working-tree changes. If there are none, review the most recently discussed code instead.\n\
         2. For each issue, report: file:line, severity (blocker/major/minor/nit), what is wrong, and a concrete fix.\n\
         3. Focus on real bugs, edge cases, error handling, and unintended behavior changes — skip pure style nits unless they hide a bug.\n\
         4. End with a one-line verdict: is the change safe to ship?{}",
        scope_suffix(arg)
    )
}

pub(crate) fn security_review_prompt(arg: &str) -> String {
    format!(
        "Perform a security review of the current changes.\n\n\
         1. Run `git --no-pager diff` via the shell tool to see what changed. If nothing changed, review the areas that handle untrusted input.\n\
         2. Look for: command/SQL/path injection, unsafe deserialization, missing input validation, secrets in code or logs, auth/authorization gaps, and unsafe file or network operations.\n\
         3. For each finding, report: file:line, severity (critical/high/medium/low), the vulnerability, a short exploit sketch, and the fix.\n\
         4. If you find nothing exploitable, say so explicitly and note any residual risks.{}",
        scope_suffix(arg)
    )
}

pub(crate) fn verify_prompt(arg: &str) -> String {
    format!(
        "Verify that the recent change actually works — do not assume, observe.\n\n\
         1. Identify what changed (`git --no-pager diff`) and what it should do.\n\
         2. Run the relevant build and tests via the shell tool (prefer the narrowest command that exercises the change, e.g. a single test).\n\
         3. Report the actual command output, pass or fail. On failure, show the error and the likely cause.\n\
         4. End with a clear verdict: VERIFIED (with evidence) or NOT VERIFIED (with the blocking failure).{}",
        scope_suffix(arg)
    )
}

// ── /docx · /pdf · /pptx · /xlsx ─────────────────────────────────────────────
// Document handling is delegated to a backend the agent drives through the shell
// tool (these formats are binary/zip containers — never hand-parse them). The
// command injects a prompt naming the right library and the read/edit/create
// workflow; the backend itself is a runtime dependency (the agent confirms it is
// installed and asks before installing).

pub(crate) fn doc_backend_hint(fmt: &str) -> &'static str {
    match fmt {
        "docx" => "the `python-docx` library (`import docx`) to read or write, or `pandoc` for format conversion",
        "pdf" => "`pdfplumber` or `pypdf` to read/extract text and tables, `reportlab` to create PDFs, or `pandoc` for conversion",
        "pptx" => "the `python-pptx` library (`import pptx`) to read or build slide decks",
        "xlsx" => "`openpyxl` (or `pandas`) to read, edit, or create spreadsheets",
        _ => "an appropriate document library",
    }
}

pub(crate) fn doc_prompt(fmt: &str, arg: &str) -> String {
    let target = arg.trim();
    let file_line = if target.is_empty() {
        format!("Work with the .{fmt} file the user names next (ask which file if it is unclear).")
    } else {
        format!("Target file: {target}")
    };
    format!(
        "Help the user work with a .{fmt} document. {file_line}\n\n\
         1. Decide the operation: extract/read, edit, or create.\n\
         2. Use {backend} via the shell tool. First confirm the backend is importable (e.g. `python -c \"import <lib>\"`); if it is missing, give the exact `pip install` command and ask before installing.\n\
         3. Perform the operation with a short Python script (or pandoc) run through the shell tool — do not hand-parse the binary format.\n\
         4. Report what you did and show the result (extracted text, the written path, etc.).",
        backend = doc_backend_hint(fmt),
    )
}

pub(crate) fn config_text(cfg: &ncx_config::Config, arg: &str) -> String {
    let path = ConfigPaths::default().nanocodex;
    config_text_at(cfg, arg, &path)
}

pub(crate) fn config_text_at(cfg: &ncx_config::Config, arg: &str, path: &Path) -> String {
    let arg = arg.trim();
    if arg.is_empty() {
        return render_config_overview(cfg, path);
    }

    let (key, value) = match parse_config_assignment(arg) {
        Ok(pair) => pair,
        Err(e) => return format!("usage: /config key=value\n{e}"),
    };
    if !WRITABLE_KEYS.contains(&key.as_str()) {
        return format!(
            "Unknown writable config key: {key}\nWritable keys: {}",
            WRITABLE_KEYS.join(", ")
        );
    }

    let mut updates: HashMap<&str, &str> = HashMap::new();
    updates.insert(key.as_str(), value.as_str());
    match write_nanocodex_config(&updates, path) {
        Ok(()) => {
            let shown = if key.contains("key") {
                "<redacted>"
            } else {
                value.as_str()
            };
            format!(
                "Saved config: {key} = {shown}\npath: {}\nRestart the REPL for provider, model, sandbox, or budget changes to affect the active session.",
                path.display()
            )
        }
        Err(e) => format!("config write failed: {e}"),
    }
}

pub(crate) fn render_config_overview(cfg: &ncx_config::Config, path: &Path) -> String {
    let red = cfg.redacted();
    format!(
        "config path: {}\nmodel:     {}\nbase_url:  {}\nsandbox:   {}\napproval:  {}\napi_key:   {}\nwritable keys: {}",
        path.display(),
        cfg.model,
        cfg.base_url,
        cfg.sandbox_mode,
        cfg.approval_policy,
        red.get("api_key").cloned().unwrap_or_default(),
        WRITABLE_KEYS.join(", ")
    )
}

pub(crate) fn parse_config_assignment(arg: &str) -> Result<(String, String), String> {
    let Some((key, value)) = arg.split_once('=') else {
        return Err("missing '='; example: /config model=deepseek-chat".into());
    };
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
        return Err("config key is empty".into());
    }
    if key.chars().any(char::is_whitespace) {
        return Err("config key cannot contain whitespace".into());
    }
    if value.is_empty() {
        return Err("config value is empty".into());
    }
    Ok((key.to_string(), value.to_string()))
}
