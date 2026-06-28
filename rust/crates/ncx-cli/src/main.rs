//! ncx — nanocodex CLI (Rust). Entry point + REPL.
//!
//! Rust port of the runnable surface of `nanocodex/cli.py`: argument parsing,
//! config resolution, building the provider + tool registry + turn loop, a
//! one-shot mode (`ncx "do X"`) and an interactive REPL with slash commands.
//!
//! Kept dependency-light (hand-rolled arg parsing, no clap) in line with the
//! rewrite's goal: fast startup and a small single binary.

mod args;
mod runner;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ncx_config::{
    load_config, load_mcp_servers, permission_mode_to_knobs, write_nanocodex_config, Config,
    ConfigPaths, Overrides, VALID_PERMISSION_MODES, WRITABLE_KEYS,
};
use ncx_core::slash::{is_known, parse_slash, SLASH_HELP};
use std::rc::Rc;

use ncx_core::{
    discover_skills, expand_file_mentions, load_project_instructions, new_session_id,
    register_mcp_server, skills_index_block, AgentLoop, CheckpointMeta, CheckpointStore,
    ContextEditPolicy, MemoryStore, Orchestrator, OrchestratorConfig, Provider, Session,
    SessionIndex, SessionSummary, TaskBudget, ToolContext, ToolRegistry, TurnResult,
};
use ncx_provider::DeepSeekProvider;
use ncx_sandbox::SandboxPolicy;
use serde_json::json;

use args::{parse_args, Args};
use runner::{LiveRunner, LiveSummarizer};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use the provided tools \
    (read_file, apply_patch, update_plan) to inspect and edit the workspace. Prefer apply_patch \
    for edits. Keep responses concise.";

/// Injected into the system prompt under `--permission-mode plan`.
const PLAN_MODE_NOTE: &str = "You are in PLAN MODE. Do NOT modify files or run state-changing \
    commands — apply_patch is disabled and write/escalating shell commands are blocked. \
    Investigate (read files, run read-only commands) and produce a concrete plan for the user \
    to approve; make no changes.";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ncx: {e}\n");
            eprintln!("{}", args::USAGE);
            std::process::exit(2);
        }
    };

    if args.help {
        println!("{}", args::USAGE);
        return;
    }
    if args.version {
        println!("ncx {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Build a current-thread runtime: the loop and tools are `!Send` by design.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio current-thread runtime builds");

    std::process::exit(rt.block_on(run(args)));
}

async fn run(args: Args) -> i32 {
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let overrides = Overrides {
        workspace: Some(workspace.clone()),
        model: args.model.clone(),
        sandbox_mode: args.sandbox.clone(),
        approval_policy: args.approval.clone(),
        max_iterations: args.max_iterations,
        max_tool_calls: args.max_tool_calls,
        context_edit_enabled: if args.disable_context_edit {
            Some(false)
        } else {
            None
        },
        context_edit_max_chars: args.context_edit_max_chars,
        context_edit_keep_recent_messages: args.context_edit_keep_recent_messages,
        context_edit_max_tool_result_chars: args.context_edit_max_tool_result_chars,
        profile: args.profile.clone(),
        ..Default::default()
    };

    let cfg = match load_config(overrides) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ncx: config error: {e}");
            return 1;
        }
    };
    if args.history {
        println!("{}", render_history(&SessionIndex::default().entries(), 20));
        return 0;
    }
    if let Err(e) = cfg.validate() {
        eprintln!("ncx: {e}");
        return 1;
    }
    if let Some(pm) = args.permission_mode.as_deref() {
        if !VALID_PERMISSION_MODES.contains(&pm) {
            eprintln!(
                "ncx: invalid --permission-mode {pm:?}; expected one of {VALID_PERMISSION_MODES:?}"
            );
            return 1;
        }
    }

    // Maintenance: LLM-fold near-duplicate memory notes, then exit.
    if args.memory_merge {
        let mem = MemoryStore::new(cfg.workspace.join(".ncx").join("memory"));
        let summarizer = LiveSummarizer::new(cfg.clone());
        return match mem.summarize_consolidate(&summarizer, 0.85).await {
            Ok(n) => {
                println!("memory: folded {n} near-duplicate note(s) via the LLM.");
                0
            }
            Err(e) => {
                eprintln!("memory merge failed: {e}");
                1
            }
        };
    }

    let provider = DeepSeekProvider::with_opts(
        cfg.api_key.clone(),
        &cfg.base_url,
        cfg.model.clone(),
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    );
    // --permission-mode (when given) is the single source of gating, overriding
    // --sandbox/--approval; otherwise keep the classic sandbox+approval behavior.
    let (sandbox_mode, approval_policy, require_edit, plan_mode) =
        match args.permission_mode.as_deref() {
            Some(pm) => {
                let (s, a, r, p) = permission_mode_to_knobs(pm);
                (s.to_string(), a.to_string(), r, p)
            }
            None => (cfg.sandbox_mode.clone(), cfg.approval_policy.clone(), false, false),
        };
    let network = sandbox_mode == "danger-full-access";
    let policy = SandboxPolicy::new(sandbox_mode, &cfg.workspace).with_network_access(network);
    // Project memory: recalled into the system prompt as leads; the `remember`
    // tool lets the agent append verified notes (it gets smarter on THIS repo).
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    // Periodic consolidation: fold near-duplicate notes on every start (cheap,
    // idempotent) so the store stays tidy as it grows.
    let _ = memory.consolidate(0.85);
    let recall_query = args.prompt.as_deref().unwrap_or("");
    let recall = memory.recall(recall_query, 8, 4000);
    let instructions = load_project_instructions(&cfg.workspace, 16_000);
    // Agent Skills: inject only the name+description index (progressive
    // disclosure); the `skill` tool loads a full SKILL.md body on demand.
    let skills = discover_skills(&cfg.workspace);
    let skills_index = skills_index_block(&skills);
    let plan_note = if plan_mode {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let system_prompt =
        compose_system_prompt(SYSTEM_PROMPT, &[instructions, recall, skills_index, plan_note]);
    let ctx = ToolContext::new(cfg.workspace.clone(), policy)
        .with_approval_policy(approval_policy)
        .with_require_edit_approval(require_edit)
        .with_plan_mode(plan_mode)
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(cfg.hooks.clone())
        .with_skills(skills);
    let mut tools = ToolRegistry::new(ctx);
    for srv in load_mcp_servers() {
        match register_mcp_server(&mut tools, &srv.name, &srv.command, &srv.args, &srv.env).await {
            Ok(n) => eprintln!("mcp({}): {} tool(s) registered", srv.name, n),
            Err(e) => eprintln!("mcp({}): connect failed: {e}", srv.name),
        }
    }
    let log_path = session_log_path(&cfg.workspace);
    let session_id = new_session_id();
    let session = if args.resume {
        Session::resume(system_prompt, Some(log_path.clone()))
    } else {
        Session::with_log(system_prompt, Some(log_path.clone()))
    };
    let restored_count = session.restored_count;
    let mut agent = AgentLoop::new(Box::new(provider), tools, session)
        .with_task_budget(task_budget_from_config(&cfg))
        .with_context_edit(context_edit_from_config(&cfg))
        .with_vision_provider(build_vision_provider(&cfg));
    let mut recorder = SessionRecorder::new(session_id, cfg.workspace.clone(), log_path);

    if args.resume {
        if restored_count > 0 {
            eprintln!("resumed {restored_count} message(s) from the workspace session log.");
        } else {
            eprintln!("no previous workspace session log found; starting fresh.");
        }
    }

    // One-shot mode: run the prompt and exit.
    if let Some(prompt) = &args.prompt {
        let expanded = expand_file_mentions(prompt, &cfg.workspace);
        checkpoint_before_turn(&cfg.workspace, &expanded);
        if args.orchestrate {
            if !args.images.is_empty() {
                eprintln!("ncx: --image is ignored with --orchestrate (text-only path).");
            }
            return run_orchestrated(cfg, &expanded).await;
        }
        let user_input = match build_image_user_input(&expanded, &args.images) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ncx: {e}");
                return 1;
            }
        };
        let result = agent.run_turn(user_input, None).await;
        recorder.record(&agent.session);
        println!("{}", result.final_text);
        return if result.stop_reason == "error" { 1 } else { 0 };
    }

    repl(&mut agent, &cfg, &mut recorder).await;
    0
}

/// Run a single prompt through the tiered flash/pro orchestrator and print the
/// outcome (complexity, verify status, final text).
async fn run_orchestrated(cfg: Config, prompt: &str) -> i32 {
    let fast = if cfg.fast_model.is_empty() {
        cfg.model.clone()
    } else {
        cfg.fast_model.clone()
    };
    eprintln!("[orchestrator] main={}  fast={}", cfg.model, fast);
    let runner = LiveRunner::new(cfg);
    let orch = Orchestrator::new(&runner, OrchestratorConfig::default());
    let outcome = orch.handle(prompt).await;
    eprintln!(
        "[orchestrator] complexity={:?}  verify={}  rounds={}  best_worker={}",
        outcome.complexity,
        if outcome.verify_passed {
            "PASS"
        } else {
            "UNVERIFIED"
        },
        outcome.verify_rounds,
        outcome.best_worker,
    );
    println!("{}", outcome.final_text);
    if outcome.verify_passed {
        0
    } else {
        1
    }
}

fn compose_system_prompt(base: &str, blocks: &[String]) -> String {
    let mut out = base.to_string();
    for block in blocks {
        if !block.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(block.trim());
        }
    }
    out
}

/// Interactive REPL. Slash commands are dispatched without a model call; any
/// other line becomes a turn (with `@file` mention expansion).
async fn repl(agent: &mut AgentLoop, cfg: &ncx_config::Config, recorder: &mut SessionRecorder) {
    println!(
        "nanocodex (ncx) — model {}, sandbox {}. /help for commands, /exit to quit. \
         (attach images inline: `--image <path> your question`)",
        cfg.model, cfg.sandbox_mode
    );
    let stdin = io::stdin();
    let mut usage = UsageTracker::default();
    loop {
        print!("\n› ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() {
            continue;
        }

        let (cmd, arg) = parse_slash(line);
        if let Some(cmd) = cmd {
            match dispatch_slash(&cmd, &arg, agent, cfg, recorder, &usage) {
                SlashOutcome::Exit => break,
                SlashOutcome::Printed(text) => println!("{text}"),
                SlashOutcome::Prompt(text) => {
                    run_one_turn(agent, &text, cfg, recorder, &mut usage).await
                }
            }
            continue;
        }

        run_one_turn(agent, line, cfg, recorder, &mut usage).await;
    }
    println!("bye.");
}

async fn run_one_turn(
    agent: &mut AgentLoop,
    prompt: &str,
    cfg: &ncx_config::Config,
    recorder: &mut SessionRecorder,
    usage: &mut UsageTracker,
) {
    // Inline `--image <path>` tokens attach images (vision turn); the rest is text.
    let (text, images) = split_inline_images(prompt);
    let expanded = expand_file_mentions(&text, &cfg.workspace);
    checkpoint_before_turn(&cfg.workspace, &expanded);
    let user_input = match build_image_user_input(&expanded, &images) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ncx: {e}");
            return;
        }
    };
    let result = agent.run_turn(user_input, None).await;
    recorder.record(&agent.session);
    usage.record(&result);
    println!("{}", result.final_text);
}

/// Pull inline `--image <path>` pairs out of a REPL line, returning the
/// remaining prompt text and the collected image paths (mirrors the one-shot
/// `--image` flag so the REPL can also drive vision turns).
fn split_inline_images(line: &str) -> (String, Vec<PathBuf>) {
    let mut images = Vec::new();
    let mut words = Vec::new();
    let mut it = line.split_whitespace();
    while let Some(w) = it.next() {
        if w == "--image" {
            if let Some(p) = it.next() {
                images.push(PathBuf::from(p));
            }
        } else {
            words.push(w);
        }
    }
    (words.join(" "), images)
}

enum SlashOutcome {
    Exit,
    Printed(String),
    Prompt(String),
}

/// Handle a slash command that doesn't require a model call. Returns the text to
/// print, an exit signal, or (for unknown commands) treats the line as a prompt.
fn dispatch_slash(
    cmd: &str,
    arg: &str,
    agent: &mut AgentLoop,
    cfg: &ncx_config::Config,
    recorder: &mut SessionRecorder,
    usage: &UsageTracker,
) -> SlashOutcome {
    match cmd {
        "/exit" => SlashOutcome::Exit,
        "/help" => SlashOutcome::Printed(render_help_for_workspace(&cfg.workspace)),
        "/status" => SlashOutcome::Printed(render_status(cfg)),
        "/usage" | "/cost" => SlashOutcome::Printed(usage.render()),
        "/config" => SlashOutcome::Printed(config_text(cfg, arg)),
        "/history" => SlashOutcome::Printed(render_history(&SessionIndex::default().entries(), 20)),
        "/checkpoint" => SlashOutcome::Printed(create_checkpoint_text(&cfg.workspace, arg)),
        "/checkpoints" => SlashOutcome::Printed(render_checkpoints(
            &CheckpointStore::new(&cfg.workspace).list(),
            20,
        )),
        "/restore" => SlashOutcome::Printed(restore_checkpoint_text(&cfg.workspace, arg)),
        "/compact" => SlashOutcome::Printed(compact_session_text(agent, recorder)),
        "/model" => {
            if arg.is_empty() {
                SlashOutcome::Printed(format!("model: {}", cfg.model))
            } else {
                SlashOutcome::Printed(format!(
                    "(model switch requires restart in this build; current: {})",
                    cfg.model
                ))
            }
        }
        "/skills" => SlashOutcome::Printed(render_skills(&agent.tools.ctx.skills)),
        "/plan" => {
            let plan = agent.tools.ctx.plan.borrow();
            if plan.is_empty() {
                SlashOutcome::Printed("(no plan yet)".into())
            } else {
                let mut out = String::from("Plan:");
                for step in plan.iter() {
                    let s = step.get("step").and_then(|v| v.as_str()).unwrap_or("?");
                    let st = step.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    out.push_str(&format!("\n  [{st}] {s}"));
                }
                SlashOutcome::Printed(out)
            }
        }
        other if is_known(other) => {
            SlashOutcome::Printed(format!("({other} is not available in this build yet)"))
        }
        other => match custom_command_prompt(&cfg.workspace, other, arg) {
            Ok(Some(prompt)) => SlashOutcome::Prompt(prompt),
            Ok(None) => SlashOutcome::Printed(format!("Unknown command {other}. Try /help.")),
            Err(e) => SlashOutcome::Printed(format!("Custom command failed: {e}")),
        },
    }
}

fn render_skills(skills: &[ncx_core::Skill]) -> String {
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

fn render_help() -> String {
    let mut out = String::from("Commands:");
    for (cmd, help) in SLASH_HELP {
        out.push_str(&format!("\n  {cmd:<12} {help}"));
    }
    out
}

fn render_help_for_workspace(workspace: &Path) -> String {
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

fn render_status(cfg: &ncx_config::Config) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CustomCommandSummary {
    scope: &'static str,
    name: String,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CustomCommandQuery {
    scope: Option<&'static str>,
    name: String,
}

fn custom_command_prompt(
    workspace: &Path,
    slash_cmd: &str,
    arg: &str,
) -> Result<Option<String>, String> {
    let Some(query) = parse_custom_command_query(slash_cmd) else {
        return Ok(None);
    };
    let Some(cmd) = resolve_custom_command(workspace, &query) else {
        return Ok(None);
    };
    let template = std::fs::read_to_string(&cmd.path).map_err(|e| {
        format!(
            "could not read custom command {} from {}: {e}",
            slash_cmd,
            cmd.path.display()
        )
    })?;
    Ok(Some(expand_custom_command_template(
        strip_frontmatter(&template),
        arg,
    )))
}

fn parse_custom_command_query(slash_cmd: &str) -> Option<CustomCommandQuery> {
    let body = slash_cmd.strip_prefix('/')?;
    if body.is_empty() {
        return None;
    }
    let (scope, name) = if let Some((scope, name)) = body.split_once(':') {
        let scope = match scope {
            "project" => "project",
            "user" => "user",
            _ => return None,
        };
        (Some(scope), name)
    } else {
        (None, body)
    };
    if !valid_custom_command_name(name) {
        return None;
    }
    Some(CustomCommandQuery {
        scope,
        name: name.to_string(),
    })
}

fn resolve_custom_command(
    workspace: &Path,
    query: &CustomCommandQuery,
) -> Option<CustomCommandSummary> {
    custom_command_roots(workspace)
        .into_iter()
        .filter(|root| query.scope.is_none_or(|s| s == root.scope))
        .find_map(|root| {
            let path = root.dir.join(format!("{}.md", query.name));
            if path.is_file() {
                Some(CustomCommandSummary {
                    scope: root.scope,
                    name: query.name.clone(),
                    path,
                })
            } else {
                None
            }
        })
}

fn list_custom_commands(workspace: &Path) -> Vec<CustomCommandSummary> {
    let mut out: Vec<CustomCommandSummary> = Vec::new();
    let mut seen: Vec<(&'static str, String)> = Vec::new();
    for root in custom_command_roots(workspace) {
        let Ok(entries) = std::fs::read_dir(&root.dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !valid_custom_command_name(name) {
                continue;
            }
            let name = name.to_string();
            if seen
                .iter()
                .any(|(scope, n)| *scope == root.scope && n == &name)
            {
                continue;
            }
            seen.push((root.scope, name.clone()));
            out.push(CustomCommandSummary {
                scope: root.scope,
                name,
                path,
            });
        }
    }
    out.sort_by(|a, b| (a.scope, &a.name).cmp(&(b.scope, &b.name)));
    out
}

struct CustomCommandRoot {
    scope: &'static str,
    dir: PathBuf,
}

fn custom_command_roots(workspace: &Path) -> Vec<CustomCommandRoot> {
    let mut roots = vec![
        CustomCommandRoot {
            scope: "project",
            dir: workspace.join(".nanocodex").join("commands"),
        },
        CustomCommandRoot {
            scope: "project",
            dir: workspace.join(".claude").join("commands"),
        },
    ];
    if let Some(home) = home_dir() {
        roots.push(CustomCommandRoot {
            scope: "user",
            dir: home.join(".nanocodex").join("commands"),
        });
        roots.push(CustomCommandRoot {
            scope: "user",
            dir: home.join(".claude").join("commands"),
        });
    }
    roots
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn valid_custom_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn strip_frontmatter(template: &str) -> &str {
    let Some(rest) = template
        .strip_prefix("---\n")
        .or_else(|| template.strip_prefix("---\r\n"))
    else {
        return template.trim();
    };
    let mut offset = template.len() - rest.len();
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return template[offset + line.len()..].trim();
        }
        offset += line.len();
    }
    template.trim()
}

fn expand_custom_command_template(template: &str, arg: &str) -> String {
    let args = split_custom_args(arg);
    let mut out = template.to_string();
    for i in 0..10 {
        let value = args.get(i).map(String::as_str).unwrap_or("");
        out = out.replace(&format!("$ARGUMENTS[{i}]"), value);
        out = out.replace(&format!("${i}"), value);
    }
    out = out.replace("$ARGUMENTS", arg.trim());
    if !arg.trim().is_empty()
        && !template.contains("$ARGUMENTS")
        && !(0..10).any(|i| template.contains(&format!("${i}")))
    {
        out.push_str("\n\nArguments: ");
        out.push_str(arg.trim());
    }
    out.trim().to_string()
}

fn split_custom_args(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in arg.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn config_text(cfg: &ncx_config::Config, arg: &str) -> String {
    let path = ConfigPaths::default().nanocodex;
    config_text_at(cfg, arg, &path)
}

fn config_text_at(cfg: &ncx_config::Config, arg: &str, path: &Path) -> String {
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

fn render_config_overview(cfg: &ncx_config::Config, path: &Path) -> String {
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

fn parse_config_assignment(arg: &str) -> Result<(String, String), String> {
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

#[derive(Debug, Default, Clone)]
struct UsageTracker {
    last: Option<TurnUsage>,
    total: BTreeMap<String, i64>,
    total_model_calls: usize,
    total_tool_calls: usize,
}

#[derive(Debug, Clone)]
struct TurnUsage {
    usage: BTreeMap<String, i64>,
    model_calls: usize,
    tool_calls: usize,
    stop_reason: String,
}

impl UsageTracker {
    fn record(&mut self, result: &TurnResult) {
        self.total_model_calls += result.iterations;
        self.total_tool_calls += result.tools_used.len();
        add_usage(&mut self.total, &result.usage);
        self.last = Some(TurnUsage {
            usage: result.usage.clone(),
            model_calls: result.iterations,
            tool_calls: result.tools_used.len(),
            stop_reason: result.stop_reason.clone(),
        });
    }

    fn render(&self) -> String {
        let Some(last) = &self.last else {
            return "No token usage recorded yet.".into();
        };
        format!(
            "Last turn:\n{}\n\nSession total:\n{}\n\nCost: raw token usage only; no Rust price table is configured.",
            format_usage_block(
                last.model_calls,
                last.tool_calls,
                Some(&last.stop_reason),
                &last.usage
            ),
            format_usage_block(
                self.total_model_calls,
                self.total_tool_calls,
                None,
                &self.total
            )
        )
    }
}

fn add_usage(total: &mut BTreeMap<String, i64>, usage: &BTreeMap<String, i64>) {
    for (key, value) in usage {
        *total.entry(key.clone()).or_insert(0) += *value;
    }
}

fn format_usage_block(
    model_calls: usize,
    tool_calls: usize,
    stop_reason: Option<&str>,
    usage: &BTreeMap<String, i64>,
) -> String {
    let prompt = usage_value(usage, "prompt_tokens");
    let completion = usage_value(usage, "completion_tokens");
    let hit = usage_value(usage, "prompt_cache_hit_tokens");
    let miss = usage_value(usage, "prompt_cache_miss_tokens");
    let total = prompt + completion;
    let mut lines = vec![
        format!("model_calls: {model_calls}"),
        format!("tool_calls:  {tool_calls}"),
    ];
    if let Some(reason) = stop_reason {
        lines.push(format!("stop_reason: {reason}"));
    }
    lines.push(format!("prompt_tokens:     {prompt}"));
    lines.push(format!("completion_tokens: {completion}"));
    lines.push(format!("total_tokens:      {total}"));
    if hit > 0 || miss > 0 {
        lines.push(format!("prompt_cache_hit_tokens:  {hit}"));
        lines.push(format!("prompt_cache_miss_tokens: {miss}"));
    }
    lines.join("\n")
}

fn usage_value(usage: &BTreeMap<String, i64>, key: &str) -> i64 {
    usage.get(key).copied().unwrap_or(0)
}

struct SessionRecorder {
    index: SessionIndex,
    session_id: String,
    workspace: PathBuf,
    log_path: PathBuf,
}

impl SessionRecorder {
    fn new(session_id: String, workspace: PathBuf, log_path: PathBuf) -> Self {
        SessionRecorder {
            index: SessionIndex::default(),
            session_id,
            workspace,
            log_path,
        }
    }

    fn record(&mut self, session: &Session) {
        let _ = self
            .index
            .record_turn(&self.session_id, &self.workspace, session, &self.log_path);
    }
}

fn session_log_path(workspace: &Path) -> PathBuf {
    workspace.join(".nanocodex").join("session.jsonl")
}

fn render_history(entries: &[SessionSummary], limit: usize) -> String {
    if entries.is_empty() {
        return "No saved sessions.".into();
    }
    let mut out = String::from("Saved sessions:");
    for summary in entries.iter().take(limit) {
        let title = if summary.title.trim().is_empty() {
            "(no prompt yet)"
        } else {
            summary.title.as_str()
        };
        out.push_str(&format!(
            "\n  {}  {}  {}  users={} assistants={} tools={}",
            summary.updated_at,
            summary.session_id,
            title,
            summary.user_messages,
            summary.assistant_messages,
            summary.tool_calls
        ));
    }
    out
}

fn compact_session_text(agent: &mut AgentLoop, recorder: &mut SessionRecorder) -> String {
    let stats = agent.session.compact(&agent.context_edit);
    recorder.record(&agent.session);
    format!(
        "Compacted session: chars {} -> {}; compressed_tool_results={} dropped_messages={}",
        stats.original_chars,
        stats.edited_chars,
        stats.compressed_tool_results,
        stats.dropped_messages
    )
}

fn checkpoint_before_turn(workspace: &Path, prompt: &str) {
    let label = format!("auto: {}", clipped_label(prompt, 80));
    match CheckpointStore::new(workspace).create(&label) {
        Ok(meta) => eprintln!(
            "checkpoint {} saved ({} file(s), {} skipped).",
            meta.id,
            meta.files.len(),
            meta.skipped_paths.len()
        ),
        Err(e) => eprintln!("checkpoint warning: {e}"),
    }
}

fn create_checkpoint_text(workspace: &Path, label: &str) -> String {
    let label = if label.trim().is_empty() {
        "manual checkpoint"
    } else {
        label.trim()
    };
    match CheckpointStore::new(workspace).create(label) {
        Ok(meta) => format_checkpoint_saved(&meta),
        Err(e) => format!("checkpoint failed: {e}"),
    }
}

fn restore_checkpoint_text(workspace: &Path, id: &str) -> String {
    if id.trim().is_empty() {
        return "usage: /restore <checkpoint-id>".into();
    }
    match CheckpointStore::new(workspace).restore(id) {
        Ok(report) => {
            let safety = report
                .safety_checkpoint_id
                .map(|id| format!("\nsafety checkpoint: {id}"))
                .unwrap_or_else(|| "\nsafety checkpoint: failed".into());
            format!(
                "restored checkpoint {}\nrestored_files: {}\ndeleted_files: {}{}",
                report.checkpoint_id, report.restored_files, report.deleted_files, safety
            )
        }
        Err(e) => format!("restore failed: {e}"),
    }
}

fn format_checkpoint_saved(meta: &CheckpointMeta) -> String {
    format!(
        "checkpoint: {}\nlabel: {}\nfiles: {}  skipped: {}  bytes: {}",
        meta.id,
        meta.label,
        meta.files.len(),
        meta.skipped_paths.len(),
        meta.total_bytes
    )
}

fn render_checkpoints(entries: &[CheckpointMeta], limit: usize) -> String {
    if entries.is_empty() {
        return "No checkpoints.".into();
    }
    let mut out = String::from("Checkpoints:");
    for meta in entries.iter().take(limit) {
        out.push_str(&format!(
            "\n  {}  {}  {}  files={} skipped={}",
            meta.created_at,
            meta.id,
            if meta.label.is_empty() {
                "(unlabeled)"
            } else {
                meta.label.as_str()
            },
            meta.files.len(),
            meta.skipped_paths.len()
        ));
    }
    out
}

fn clipped_label(text: &str, limit: usize) -> String {
    let s = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= limit {
        s
    } else {
        format!(
            "{}...",
            s.chars().take(limit.saturating_sub(3)).collect::<String>()
        )
    }
}

fn positive_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn nonnegative_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value).ok().unwrap_or(fallback)
}

fn task_budget_from_config(cfg: &ncx_config::Config) -> TaskBudget {
    TaskBudget {
        max_model_calls: positive_usize(cfg.max_iterations, 60),
        max_tool_calls: nonnegative_usize(cfg.max_tool_calls, 120),
    }
}

/// Build a dedicated vision provider from the `vl_*` config, or `None` when no
/// vision model is configured (image turns then stay on the main provider).
///
/// Only `vl_model` is required; `vl_base_url` / `vl_api_key` fall back to the
/// main `base_url` / `api_key`, so a user can either point at a separate VL
/// endpoint (e.g. DashScope) or just name a vision model on the same endpoint.
fn build_vision_provider(cfg: &ncx_config::Config) -> Option<Box<dyn Provider>> {
    if cfg.vl_model.trim().is_empty() {
        return None;
    }
    let base_url = if cfg.vl_base_url.trim().is_empty() {
        &cfg.base_url
    } else {
        &cfg.vl_base_url
    };
    let api_key = if cfg.vl_api_key.trim().is_empty() {
        cfg.api_key.clone()
    } else {
        cfg.vl_api_key.clone()
    };
    Some(Box::new(DeepSeekProvider::with_opts(
        api_key,
        base_url,
        cfg.vl_model.clone(),
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    )))
}

/// Build the one-shot user input. With no images it is just the prompt text;
/// with `--image` paths it becomes an OpenAI-style multimodal `content` array
/// (`text` block + one `image_url` block per file, each a base64 `data:` URL),
/// which trips [`AgentLoop`]'s image detection and routes to the vision model.
fn build_image_user_input(text: &str, images: &[PathBuf]) -> Result<serde_json::Value, String> {
    if images.is_empty() {
        return Ok(json!(text));
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    for path in images {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read image {}: {e}", path.display()))?;
        let url = format!("data:{};base64,{}", image_mime(path), base64_encode(&bytes));
        content.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Ok(serde_json::Value::Array(content))
}

/// Guess an image MIME type from the file extension (defaults to PNG).
fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

/// Standard base64 encoding (RFC 4648, with `=` padding). Hand-rolled to avoid a
/// new crate dependency for the single image-attachment use site.
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn context_edit_from_config(cfg: &ncx_config::Config) -> ContextEditPolicy {
    ContextEditPolicy {
        enabled: cfg.context_edit_enabled,
        max_chars: positive_usize(cfg.context_edit_max_chars, 120_000),
        keep_recent_messages: positive_usize(cfg.context_edit_keep_recent_messages, 30),
        max_tool_result_chars: positive_usize(cfg.context_edit_max_tool_result_chars, 4_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_all_commands() {
        let help = render_help();
        for (cmd, _) in SLASH_HELP {
            assert!(help.contains(cmd), "{cmd}");
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 §10 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn image_input_builds_multimodal_content() {
        let dir = std::env::temp_dir().join(format!("ncx_img_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.jpg");
        std::fs::write(&img, b"foobar").unwrap();

        // No images -> plain text string.
        assert_eq!(build_image_user_input("hi", &[]).unwrap(), json!("hi"));

        // With an image -> [text, image_url(data: URL)].
        let v = build_image_user_input("describe", &[img]).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr[0], json!({"type": "text", "text": "describe"}));
        assert_eq!(arr[1]["type"], "image_url");
        assert_eq!(
            arr[1]["image_url"]["url"].as_str().unwrap(),
            "data:image/jpeg;base64,Zm9vYmFy"
        );

        // A missing file is a clean error, not a panic.
        assert!(build_image_user_input("x", &[dir.join("nope.png")]).is_err());
    }

    #[test]
    fn inline_images_split_from_prompt() {
        // No flag -> all text, no images.
        let (t, imgs) = split_inline_images("what is this");
        assert_eq!(t, "what is this");
        assert!(imgs.is_empty());

        // Flags anywhere are pulled out; remaining words form the prompt.
        let (t, imgs) = split_inline_images("--image a.png compare these --image b.jpg now");
        assert_eq!(t, "compare these now");
        assert_eq!(imgs, vec![PathBuf::from("a.png"), PathBuf::from("b.jpg")]);
    }

    #[test]
    fn vision_provider_only_built_when_vl_model_set() {
        let mut cfg = ncx_config::Config::default();
        // No vl_model -> image turns stay on the main provider.
        assert!(build_vision_provider(&cfg).is_none());
        // vl_model set -> a dedicated vision provider is constructed.
        cfg.vl_model = "qwen-vl-max".into();
        assert!(build_vision_provider(&cfg).is_some());
    }

    #[test]
    fn custom_command_expands_project_prompt_template() {
        let ws = std::env::temp_dir().join(format!("ncx_custom_cmd_{}", new_session_id()));
        let dir = ws.join(".nanocodex").join("commands");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("review.md"),
            "---\ndescription: Review a file\n---\nReview $ARGUMENTS[0] with $0. Full: $ARGUMENTS",
        )
        .unwrap();

        let out = custom_command_prompt(&ws, "/review", "src/main.rs extra")
            .unwrap()
            .unwrap();

        assert_eq!(
            out,
            "Review src/main.rs with src/main.rs. Full: src/main.rs extra"
        );
        assert!(!out.contains("description"));
    }

    #[test]
    fn custom_command_supports_claude_compatible_project_dir() {
        let ws = std::env::temp_dir().join(format!("ncx_custom_claude_{}", new_session_id()));
        let dir = ws.join(".claude").join("commands");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("audit.md"), "Audit the current change.").unwrap();

        let out = custom_command_prompt(&ws, "/project:audit", "focus tests")
            .unwrap()
            .unwrap();

        assert_eq!(out, "Audit the current change.\n\nArguments: focus tests");
    }

    #[test]
    fn help_lists_custom_project_commands() {
        let ws = std::env::temp_dir().join(format!("ncx_custom_help_{}", new_session_id()));
        let dir = ws.join(".nanocodex").join("commands");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ship.md"), "Prepare release notes.").unwrap();

        let help = render_help_for_workspace(&ws);

        assert!(help.contains("Custom commands"));
        assert!(help.contains("/project:ship"));
    }

    #[test]
    fn custom_command_parser_rejects_unknown_scope_or_bad_name() {
        assert!(parse_custom_command_query("/project:review").is_some());
        assert!(parse_custom_command_query("/user:review").is_some());
        assert!(parse_custom_command_query("/team:review").is_none());
        assert!(parse_custom_command_query("/bad/name").is_none());
        assert!(parse_custom_command_query("/bad name").is_none());
    }

    #[test]
    fn split_custom_args_honors_simple_quotes() {
        assert_eq!(
            split_custom_args(r#"one "two words" 'three words'"#),
            vec!["one", "two words", "three words"]
        );
    }

    #[test]
    fn parse_config_assignment_accepts_trimmed_key_value() {
        assert_eq!(
            parse_config_assignment(" model = deepseek-chat ").unwrap(),
            ("model".into(), "deepseek-chat".into())
        );
        assert!(parse_config_assignment("model").is_err());
        assert!(parse_config_assignment("bad key=value").is_err());
        assert!(parse_config_assignment("model=").is_err());
    }

    #[test]
    fn usage_tracker_renders_last_and_total_usage() {
        let mut tracker = UsageTracker::default();
        assert_eq!(tracker.render(), "No token usage recorded yet.");

        let mut first_usage = BTreeMap::new();
        first_usage.insert("prompt_tokens".into(), 100);
        first_usage.insert("completion_tokens".into(), 20);
        first_usage.insert("prompt_cache_hit_tokens".into(), 80);
        first_usage.insert("prompt_cache_miss_tokens".into(), 20);
        tracker.record(&TurnResult {
            final_text: "ok".into(),
            iterations: 2,
            stop_reason: "completed".into(),
            tools_used: vec!["read_file".into()],
            usage: first_usage,
        });

        let mut second_usage = BTreeMap::new();
        second_usage.insert("prompt_tokens".into(), 10);
        second_usage.insert("completion_tokens".into(), 5);
        tracker.record(&TurnResult {
            final_text: "ok".into(),
            iterations: 1,
            stop_reason: "completed".into(),
            tools_used: vec![],
            usage: second_usage,
        });

        let rendered = tracker.render();
        assert!(rendered.contains("Last turn"));
        assert!(rendered.contains("Session total"));
        assert!(rendered.contains("model_calls: 3"));
        assert!(rendered.contains("tool_calls:  1"));
        assert!(rendered.contains("prompt_tokens:     110"));
        assert!(rendered.contains("completion_tokens: 25"));
        assert!(rendered.contains("prompt_cache_hit_tokens:  80"));
        assert!(rendered.contains("raw token usage only"));
    }

    #[test]
    fn config_text_writes_known_key_to_path() {
        let dir = std::env::temp_dir().join(format!("ncx_config_slash_{}", new_session_id()));
        let path = dir.join("config.toml");
        let cfg = ncx_config::Config::default();
        let out = config_text_at(&cfg, "model=deepseek-chat", &path);

        assert!(out.contains("Saved config"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("model = \"deepseek-chat\""), "{text}");
    }

    #[test]
    fn config_text_rejects_unknown_key() {
        let dir = std::env::temp_dir().join(format!("ncx_config_slash_bad_{}", new_session_id()));
        let path = dir.join("config.toml");
        let cfg = ncx_config::Config::default();
        let out = config_text_at(&cfg, "bogus=value", &path);

        assert!(out.contains("Unknown writable config key"));
        assert!(!path.exists());
    }

    #[test]
    fn status_masks_api_key() {
        let cfg = ncx_config::Config {
            api_key: "sk-secret1234".into(),
            ..Default::default()
        };
        let status = render_status(&cfg);
        assert!(status.contains("****1234"));
        assert!(!status.contains("secret"));
    }

    #[test]
    fn history_renders_saved_sessions() {
        let rows = vec![SessionSummary {
            session_id: "sid".into(),
            workspace: "/p".into(),
            title: "fix bug".into(),
            snippet: "done".into(),
            user_messages: 1,
            assistant_messages: 2,
            tool_calls: 3,
            recent_tools: vec!["read_file".into()],
            created_at: "2026-06-01T09:00:00".into(),
            updated_at: "2026-06-01T10:00:00".into(),
            log_path: "/p/.nanocodex/session.jsonl".into(),
            has_snapshot: true,
        }];
        let out = render_history(&rows, 10);
        assert!(out.contains("sid"));
        assert!(out.contains("fix bug"));
        assert!(out.contains("tools=3"));
    }

    #[test]
    fn checkpoints_render_saved_entries() {
        let rows = vec![CheckpointMeta {
            id: "cp1".into(),
            label: "before edit".into(),
            created_at: "2026-06-01T10:00:00".into(),
            files: vec!["a.txt".into()],
            skipped_paths: vec!["target/big".into()],
            total_bytes: 12,
        }];
        let out = render_checkpoints(&rows, 10);
        assert!(out.contains("cp1"));
        assert!(out.contains("before edit"));
        assert!(out.contains("skipped=1"));
    }
}
