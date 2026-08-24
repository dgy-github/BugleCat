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
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ncx_app_server::AppServer;
use ncx_config::{
    load_config, load_mcp_servers, write_nanocodex_config, Config, ConfigPaths, McpServerConfig,
    Overrides, VALID_PERMISSION_MODES, WRITABLE_KEYS,
};
use ncx_core::slash::{is_known, parse_slash, SLASH_HELP};
use std::rc::Rc;

use ncx_core::{
    custom_command_prompt, discover_codex_hooks, discover_codex_mcp_servers, discover_skills,
    expand_file_mentions, install_llm_provider_factory, list_custom_commands,
    load_project_instructions, new_session_id, prepare_mcp_server_tools, skills_index_block,
    AgentLoop, AgentRuntimeProfile, CheckpointMeta, CheckpointStore, Genome, HarnessRuntimeBuilder,
    McpServiceDescriptor, MemoryStore, Orchestrator, OrchestratorConfig, PromptAssembler, Session,
    TextContextFragment, Tool, ToolContext, TurnResult,
};
use ncx_protocol::{
    ClientRequest, ItemId, ResponsePayload, Thread, ThreadId, ThreadItem, ThreadMetadata, TurnId,
    TurnStatus, TurnUsage as ProtocolTurnUsage,
};
use ncx_thread_store::{default_thread_store_path, JsonThreadStore};
use serde_json::{json, Value};

use args::{parse_args, Args};
use runner::{LiveRunner, LiveSummarizer};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use native workspace tools \
    (find_files, grep, glob, list_directory, path_info, read_file) for recursive discovery and \
    inspection, and prefer them over shell commands. Use apply_patch for edits and update_plan for \
    multi-step work. If a path is incomplete, search recursively instead of guessing. Keep responses concise.";

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
        max_parallel_tool_calls: args.max_parallel_tool_calls,
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
        return match protocol_history(20) {
            Ok(history) => {
                println!("{history}");
                0
            }
            Err(error) => {
                eprintln!("ncx: history error: {error}");
                1
            }
        };
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

    let runtime_profile = runtime_profile_for_args(&cfg, &args);
    let policy = runtime_profile.sandbox_policy(&cfg.workspace);
    // Project memory: recalled per prompt by AgentLoop (query-scoped); the
    // `remember` tool lets the agent append verified notes (smarter on THIS repo).
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    // Periodic consolidation: fold near-duplicate notes on every start (cheap,
    // idempotent) so the store stays tidy as it grows.
    let _ = memory.consolidate(0.85);
    let instructions = load_project_instructions(&cfg.workspace, 16_000);
    // Agent Skills: inject only the name+description index (progressive
    // disclosure); the `skill` tool loads a full SKILL.md body on demand.
    let skills = discover_skills(&cfg.workspace);
    let skills_index = skills_index_block(&skills);
    // Training-time harness overrides (NCX_GENOME). Empty/unset => no-op: the
    // base prompt stays SYSTEM_PROMPT and tool descriptions are untouched.
    let genome = Genome::from_env();
    if !genome.is_empty() {
        eprintln!(
            "[ncx] NCX_GENOME active: system_prompt={}, tool_desc overrides={}",
            genome.system_prompt.is_some(),
            genome.tool_desc.len()
        );
    }
    let base_prompt = genome.base_system_prompt(SYSTEM_PROMPT).to_string();
    // Plan-mode steering note (feat/gui permission model): appended to the
    // composed prompt so the agent knows it is in read-only/plan mode.
    let plan_note = if runtime_profile.permissions.plan_mode {
        PLAN_MODE_NOTE.to_string()
    } else {
        String::new()
    };
    let mut prompt = PromptAssembler::new(base_prompt.clone());
    let instruction_fragment =
        TextContextFragment::new("project_instructions", instructions, 16_000);
    let skills_fragment = TextContextFragment::new("skills", skills_index, 32_000);
    let plan_fragment = TextContextFragment::new("plan_mode", plan_note, 4_000);
    prompt
        .upsert_fragment(10, &instruction_fragment)
        .upsert_fragment(20, &skills_fragment)
        .upsert_fragment(30, &plan_fragment);
    let system_prompt = prompt.build();
    let mut hooks = cfg.hooks.clone();
    match discover_codex_hooks(&cfg.workspace) {
        Ok(plugin_hooks) => hooks.extend(plugin_hooks),
        Err(error) => {
            eprintln!("ncx: Codex 插件 Hooks 加载失败: {error}");
            return 1;
        }
    }
    let ctx = runtime_profile
        .apply_tool_context(ToolContext::new(cfg.workspace.clone(), policy))
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(hooks)
        .with_skills(skills)
        .with_genome(genome);
    let mut tools = match HarnessRuntimeBuilder::configured(&cfg.workspace) {
        Ok(builder) => builder.build(ctx),
        Err(error) => {
            eprintln!("ncx: Harness 配置错误: {error}");
            return 1;
        }
    };
    install_llm_provider_factory(&mut tools, cfg.clone(), cfg.model.clone());
    // ncx-forge: emit the default harness genome (base prompt + core tool
    // descriptions) as TOML and exit. Done BEFORE MCP registration so the dump
    // contains only the evolvable core surface, not server-provided tools.
    if args.dump_genome {
        print!(
            "{}",
            dump_genome_toml(&base_prompt, &tools.ctx.tool_catalog.borrow())
        );
        return 0;
    }
    // MCP is off by default: only connect servers (spawning subprocesses outside
    // the sandbox) when the user opts in with --mcp. Keeps startup fast and quiet.
    let mut mcp_tool_names = Vec::new();
    if args.mcp {
        let mut servers = load_mcp_servers();
        match discover_codex_mcp_servers(&cfg.workspace) {
            Ok(plugin_servers) => servers.extend(plugin_servers),
            Err(error) => {
                eprintln!("mcp: Codex 插件资源加载失败: {error}");
                return 1;
            }
        }
        if servers.is_empty() {
            eprintln!("mcp: --mcp set but no servers found in ~/.nanocodex/mcp.toml");
        }
        match prepare_configured_mcp_tools(&servers).await {
            Ok(prepared) => match tools.replace_tools(&[], prepared) {
                Ok(names) => {
                    mcp_tool_names = names;
                    tools.replace_service(
                        "mcp",
                        Rc::new(McpServiceDescriptor {
                            enabled: true,
                            configured_servers: servers.len(),
                            active_tools: mcp_tool_names.len(),
                        }),
                    );
                    eprintln!(
                        "mcp: {} server(s), {} tool(s) registered",
                        servers.len(),
                        mcp_tool_names.len()
                    );
                }
                Err(error) => eprintln!("mcp: registration rejected: {error}"),
            },
            Err(error) => eprintln!("mcp: load failed: {error}"),
        }
    }
    let mut recorder = match SessionRecorder::open(cfg.workspace.clone(), args.resume) {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("ncx: thread store error: {error}");
            return 1;
        }
    };
    let log_path = recorder.log_path();
    let seed = recorder.model_context();
    let session = match seed {
        Some(messages) => Session::fork(system_prompt, messages, Some(log_path)),
        None => Session::with_log(system_prompt, Some(log_path)),
    };
    let restored_count = session.restored_count;
    let mut agent = runtime_profile
        .apply(AgentLoop::from_runtime_services(tools, session).expect("LLM factory service"));
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
            return run_orchestrated(cfg, &expanded, &mut recorder).await;
        }
        if let Err(error) = validate_attachments(&agent.tools, &args.images) {
            eprintln!("ncx: {error}");
            return 1;
        }
        let user_input = match build_image_user_input(&expanded, &args.images) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("ncx: {e}");
                return 1;
            }
        };
        let turn_id = match recorder.start_turn(&expanded) {
            Ok(turn_id) => turn_id,
            Err(error) => {
                eprintln!("ncx: cannot start turn: {error}");
                return 1;
            }
        };
        let result = agent.run_turn(user_input, None).await;
        if let Err(error) = recorder.finish_turn(&turn_id, &result, &agent) {
            eprintln!("ncx: cannot persist turn: {error}");
            return 1;
        }
        println!("{}", result.final_text);
        // Emit a stable, parseable token-usage line on stderr so external tools
        // (e.g. the ncx-forge evaluator's Pareto cost axis) can read real token
        // cost rather than wall-clock. Always printed in one-shot mode.
        emit_usage_line(&result.usage);
        return if result.stop_reason == "error" { 1 } else { 0 };
    }

    repl(&mut agent, &cfg, &mut recorder, &mut mcp_tool_names).await;
    0
}

/// Run a single prompt through the tiered flash/pro orchestrator and print the
/// outcome (complexity, verify status, final text).
async fn run_orchestrated(cfg: Config, prompt: &str, recorder: &mut SessionRecorder) -> i32 {
    let fast = if cfg.fast_model.is_empty() {
        cfg.model.clone()
    } else {
        cfg.fast_model.clone()
    };
    eprintln!("[orchestrator] main={}  fast={}", cfg.model, fast);
    let turn_id = match recorder.start_turn(prompt) {
        Ok(turn_id) => turn_id,
        Err(error) => {
            eprintln!("ncx: cannot start orchestrated turn: {error}");
            return 1;
        }
    };
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
    let status = if outcome.verify_passed {
        TurnStatus::Completed
    } else {
        TurnStatus::Failed
    };
    if let Err(error) = recorder.finish_external_turn(
        &turn_id,
        prompt,
        &outcome.final_text,
        status,
        (!outcome.verify_passed).then(|| "orchestrator verification failed".to_string()),
    ) {
        eprintln!("ncx: cannot persist orchestrated turn: {error}");
        return 1;
    }
    println!("{}", outcome.final_text);
    if outcome.verify_passed {
        0
    } else {
        1
    }
}

/// Emit the default harness genome as TOML for the ncx-forge trainer: the base
/// system prompt + each registered (core) tool's description. Single-line basic
/// strings with `\n`/`\"` escapes so it round-trips through any TOML parser.
fn dump_genome_toml(system_prompt: &str, catalog: &[ncx_core::tools::ToolCatalogEntry]) -> String {
    let mut out = String::new();
    out.push_str("# Default nanocodex harness genome (ncx --dump-genome).\n");
    out.push_str("# Edit system_prompt and tool_desc.* to evolve the agent.\n\n");
    out.push_str(&format!(
        "system_prompt = \"{}\"\n\n",
        toml_escape(system_prompt)
    ));
    out.push_str("[tool_desc]\n");
    for entry in catalog {
        out.push_str(&format!(
            "{} = \"{}\"\n",
            entry.name,
            toml_escape(&entry.description)
        ));
    }
    out
}

/// Escape a string for a TOML single-line basic string (the content between the
/// surrounding quotes): backslash, double-quote, and the common control chars.
fn toml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Interactive REPL. Slash commands are dispatched without a model call; any
/// other line becomes a turn (with `@file` mention expansion).
async fn repl(
    agent: &mut AgentLoop,
    cfg: &ncx_config::Config,
    recorder: &mut SessionRecorder,
    mcp_tool_names: &mut Vec<String>,
) {
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
                SlashOutcome::ReloadMcp => {
                    println!("{}", reload_mcp_tools(agent, mcp_tool_names).await)
                }
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
    if let Err(error) = validate_attachments(&agent.tools, &images) {
        eprintln!("ncx: {error}");
        return;
    }
    let user_input = match build_image_user_input(&expanded, &images) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("ncx: {e}");
            return;
        }
    };
    let turn_id = match recorder.start_turn(&expanded) {
        Ok(turn_id) => turn_id,
        Err(error) => {
            eprintln!("ncx: cannot start turn: {error}");
            return;
        }
    };
    let result = agent.run_turn(user_input, None).await;
    if let Err(error) = recorder.finish_turn(&turn_id, &result, agent) {
        eprintln!("ncx: cannot persist turn: {error}");
    }
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
    ReloadMcp,
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
        "/usage" | "/cost" | "/usage-credits" => SlashOutcome::Printed(usage.render()),
        "/config" | "/update-config" => SlashOutcome::Printed(config_text(cfg, arg)),
        "/history" => SlashOutcome::Printed(
            protocol_history(20).unwrap_or_else(|error| format!("history error: {error}")),
        ),
        "/checkpoint" => SlashOutcome::Printed(create_checkpoint_text(&cfg.workspace, arg)),
        "/checkpoints" => SlashOutcome::Printed(render_checkpoints(
            &CheckpointStore::new(&cfg.workspace).list(),
            20,
        )),
        "/restore" => SlashOutcome::Printed(restore_checkpoint_text(&cfg.workspace, arg)),
        "/export" => SlashOutcome::Printed(export_session_text(
            &agent.session,
            cfg,
            recorder.session_id(),
            arg,
        )),
        "/review" => SlashOutcome::Prompt(review_prompt(arg)),
        "/security-review" => SlashOutcome::Prompt(security_review_prompt(arg)),
        "/verify" => SlashOutcome::Prompt(verify_prompt(arg)),
        "/docx" => SlashOutcome::Prompt(doc_prompt("docx", arg)),
        "/pdf" => SlashOutcome::Prompt(doc_prompt("pdf", arg)),
        "/pptx" => SlashOutcome::Prompt(doc_prompt("pptx", arg)),
        "/xlsx" => SlashOutcome::Prompt(doc_prompt("xlsx", arg)),
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
        "/mcp" => {
            if arg.eq_ignore_ascii_case("reload") {
                SlashOutcome::ReloadMcp
            } else {
                SlashOutcome::Printed("Usage: /mcp reload".to_string())
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

async fn reload_mcp_tools(agent: &mut AgentLoop, current_names: &mut Vec<String>) -> String {
    let servers = load_mcp_servers();
    let server_count = servers.len();
    let prepared = match prepare_configured_mcp_tools(&servers).await {
        Ok(prepared) => prepared,
        Err(error) => {
            return format!(
                "MCP reload failed: {error}. Existing {} MCP tool(s) retained.",
                current_names.len()
            );
        }
    };

    let old_count = current_names.len();
    match agent.tools.replace_tools(current_names, prepared) {
        Ok(new_names) => {
            let new_count = new_names.len();
            *current_names = new_names;
            agent.tools.replace_service(
                "mcp",
                Rc::new(McpServiceDescriptor {
                    enabled: true,
                    configured_servers: server_count,
                    active_tools: new_count,
                }),
            );
            format!(
                "MCP reload complete: {server_count} server(s), {new_count} tool(s) active; replaced {old_count}."
            )
        }
        Err(error) => {
            format!("MCP reload rejected: {error}. Existing {old_count} MCP tool(s) retained.")
        }
    }
}

async fn prepare_configured_mcp_tools(
    servers: &[McpServerConfig],
) -> Result<Vec<Box<dyn Tool>>, String> {
    let mut prepared = Vec::new();
    for server in servers {
        let mut tools =
            prepare_mcp_server_tools(&server.name, &server.command, &server.args, &server.env)
                .await
                .map_err(|error| format!("server '{}': {error}", server.name))?;
        prepared.append(&mut tools);
    }
    Ok(prepared)
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
fn export_session_text(
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
fn export_target_path(workspace: &Path, session_id: &str, arg: &str) -> PathBuf {
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
fn render_session_markdown(
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

fn push_block(out: &mut String, text: &str) {
    out.push_str(text.trim_end());
    out.push('\n');
}

/// Choose a Markdown code fence that can wrap `content` verbatim: one backtick
/// longer than the longest run of backticks inside it (CommonMark), min 3. This
/// keeps tool results / file contents that themselves contain ``` from breaking
/// out of the exported code block.
fn code_fence(content: &str) -> String {
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
fn push_fenced(out: &mut String, lang: &str, content: &str) {
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

fn count_roles(messages: &[Value]) -> (usize, usize, usize) {
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
fn content_to_markdown(content: Option<&Value>) -> String {
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

fn scope_suffix(arg: &str) -> String {
    let arg = arg.trim();
    if arg.is_empty() {
        String::new()
    } else {
        format!("\n\nScope/focus for this pass: {arg}")
    }
}

fn review_prompt(arg: &str) -> String {
    format!(
        "Review the current code changes for correctness and quality.\n\n\
         1. Run `git --no-pager diff` and `git --no-pager diff --staged` via the shell tool to see the working-tree changes. If there are none, review the most recently discussed code instead.\n\
         2. For each issue, report: file:line, severity (blocker/major/minor/nit), what is wrong, and a concrete fix.\n\
         3. Focus on real bugs, edge cases, error handling, and unintended behavior changes — skip pure style nits unless they hide a bug.\n\
         4. End with a one-line verdict: is the change safe to ship?{}",
        scope_suffix(arg)
    )
}

fn security_review_prompt(arg: &str) -> String {
    format!(
        "Perform a security review of the current changes.\n\n\
         1. Run `git --no-pager diff` via the shell tool to see what changed. If nothing changed, review the areas that handle untrusted input.\n\
         2. Look for: command/SQL/path injection, unsafe deserialization, missing input validation, secrets in code or logs, auth/authorization gaps, and unsafe file or network operations.\n\
         3. For each finding, report: file:line, severity (critical/high/medium/low), the vulnerability, a short exploit sketch, and the fix.\n\
         4. If you find nothing exploitable, say so explicitly and note any residual risks.{}",
        scope_suffix(arg)
    )
}

fn verify_prompt(arg: &str) -> String {
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

fn doc_backend_hint(fmt: &str) -> &'static str {
    match fmt {
        "docx" => "the `python-docx` library (`import docx`) to read or write, or `pandoc` for format conversion",
        "pdf" => "`pdfplumber` or `pypdf` to read/extract text and tables, `reportlab` to create PDFs, or `pandoc` for conversion",
        "pptx" => "the `python-pptx` library (`import pptx`) to read or build slide decks",
        "xlsx" => "`openpyxl` (or `pandas`) to read, edit, or create spreadsheets",
        _ => "an appropriate document library",
    }
}

fn doc_prompt(fmt: &str, arg: &str) -> String {
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
    server: AppServer<JsonThreadStore>,
    thread_id: ThreadId,
    workspace: PathBuf,
    model_context: Option<Vec<Value>>,
}

impl SessionRecorder {
    fn open(workspace: PathBuf, resume: bool) -> Result<Self, String> {
        Self::open_at(workspace, resume, default_thread_store_path())
    }

    fn open_at(workspace: PathBuf, resume: bool, store_path: PathBuf) -> Result<Self, String> {
        let store = Arc::new(JsonThreadStore::open(store_path).map_err(|e| e.to_string())?);
        let server = AppServer::new(store, now_epoch_millis);
        let workspace_text = workspace.display().to_string();
        let existing = if resume {
            match server
                .dispatch(ClientRequest::ThreadList {
                    include_archived: false,
                })
                .map_err(|e| e.to_string())?
                .response
                .payload
            {
                ResponsePayload::Threads(threads) => threads
                    .into_iter()
                    .find(|metadata| metadata.workspace == workspace_text),
                _ => None,
            }
        } else {
            None
        };
        let thread_id = if let Some(metadata) = existing {
            metadata.id
        } else {
            let thread_id = ThreadId::new(new_session_id()).map_err(|e| e.to_string())?;
            server
                .dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(thread_id.clone()),
                    workspace: workspace_text,
                    title: "(no prompt yet)".to_string(),
                })
                .map_err(|e| e.to_string())?;
            thread_id
        };
        let model_context = match server
            .dispatch(ClientRequest::ThreadModelContextRead {
                thread_id: thread_id.clone(),
            })
            .map_err(|e| e.to_string())?
            .response
            .payload
        {
            ResponsePayload::ModelContext(Some(context)) => Some(context.messages),
            _ if resume => Some(read_protocol_messages(&server, &thread_id)?),
            _ => None,
        };
        Ok(Self {
            server,
            thread_id,
            workspace,
            model_context,
        })
    }

    fn model_context(&self) -> Option<Vec<Value>> {
        self.model_context
            .clone()
            .filter(|messages| !messages.is_empty())
    }

    fn log_path(&self) -> PathBuf {
        self.workspace
            .join(".nanocodex")
            .join("sessions")
            .join(format!(
                "{}.jsonl",
                safe_thread_file_stem(self.thread_id.as_str())
            ))
    }

    fn start_turn(&mut self, user_text: &str) -> Result<TurnId, String> {
        let turn_id =
            TurnId::new(format!("turn-{}", new_session_id())).map_err(|error| error.to_string())?;
        self.server
            .dispatch(ClientRequest::TurnStart {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::UserMessage {
                    id: ItemId::new(format!("user-{}", new_session_id()))
                        .map_err(|error| error.to_string())?,
                    text: user_text.to_string(),
                },
            })
            .map_err(|error| error.to_string())?;
        let current = self
            .server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: self.thread_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        if matches!(current.response.payload, ResponsePayload::Thread(Thread { metadata: ThreadMetadata { ref title, .. }, .. }) if title == "(no prompt yet)")
        {
            self.server
                .dispatch(ClientRequest::ThreadRename {
                    thread_id: self.thread_id.clone(),
                    title: clipped_label(user_text, 80),
                })
                .map_err(|error| error.to_string())?;
        }
        Ok(turn_id)
    }

    fn session_id(&self) -> &str {
        self.thread_id.as_str()
    }

    fn finish_turn(
        &mut self,
        turn_id: &TurnId,
        result: &TurnResult,
        agent: &AgentLoop,
    ) -> Result<(), String> {
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::AssistantMessage {
                    id: ItemId::new(format!("assistant-{}", new_session_id()))
                        .map_err(|error| error.to_string())?,
                    text: result.final_text.clone(),
                },
            })
            .map_err(|error| error.to_string())?;
        let messages = agent.session.full_messages();
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.model_context = Some(messages);
        let estimated = agent.estimated_cost(result);
        let (currency, estimated_cost) = estimated
            .map(|(currency, amount)| (Some(currency), Some(amount)))
            .unwrap_or((None, None));
        let status = if result.stop_reason == "error" {
            TurnStatus::Failed
        } else {
            TurnStatus::Completed
        };
        self.server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                status,
                error: (status == TurnStatus::Failed).then(|| result.final_text.clone()),
                usage: ProtocolTurnUsage {
                    tokens: result.usage.clone(),
                    estimated_cost,
                    currency,
                },
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn replace_model_context(&mut self, session: &Session) -> Result<(), String> {
        let messages = session.full_messages();
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|error| error.to_string())?;
        self.model_context = Some(messages);
        Ok(())
    }

    fn finish_external_turn(
        &mut self,
        turn_id: &TurnId,
        user_text: &str,
        final_text: &str,
        status: TurnStatus,
        error: Option<String>,
    ) -> Result<(), String> {
        self.server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                item: ThreadItem::AssistantMessage {
                    id: ItemId::new(format!("assistant-{}", new_session_id()))
                        .map_err(|failure| failure.to_string())?,
                    text: final_text.to_string(),
                },
            })
            .map_err(|failure| failure.to_string())?;
        let mut messages = self.model_context.take().unwrap_or_default();
        messages.push(json!({"role": "user", "content": user_text}));
        messages.push(json!({"role": "assistant", "content": final_text}));
        self.server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: self.thread_id.clone(),
                messages: messages.clone(),
            })
            .map_err(|failure| failure.to_string())?;
        self.model_context = Some(messages);
        self.server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: self.thread_id.clone(),
                turn_id: turn_id.clone(),
                status,
                error,
                usage: ProtocolTurnUsage::default(),
            })
            .map_err(|failure| failure.to_string())?;
        Ok(())
    }
}

fn safe_thread_file_stem(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn now_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// Print a stable, parseable token-usage line to stderr (one-shot mode).
/// Format: `[ncx-usage] prompt_tokens=P completion_tokens=C total_tokens=T`.
/// `total_tokens` is P+C (the provider does not report a total directly).
fn emit_usage_line(usage: &std::collections::BTreeMap<String, i64>) {
    let prompt = usage.get("prompt_tokens").copied().unwrap_or(0);
    let completion = usage.get("completion_tokens").copied().unwrap_or(0);
    eprintln!(
        "[ncx-usage] prompt_tokens={prompt} completion_tokens={completion} total_tokens={}",
        prompt + completion
    );
}

fn protocol_history(limit: usize) -> Result<String, String> {
    let store =
        Arc::new(JsonThreadStore::open(default_thread_store_path()).map_err(|e| e.to_string())?);
    let server = AppServer::new(store, now_epoch_millis);
    let entries = match server
        .dispatch(ClientRequest::ThreadList {
            include_archived: false,
        })
        .map_err(|e| e.to_string())?
        .response
        .payload
    {
        ResponsePayload::Threads(entries) => entries,
        _ => return Err("threadList returned an unexpected response".to_string()),
    };
    Ok(render_history(&entries, limit))
}

fn render_history(entries: &[ThreadMetadata], limit: usize) -> String {
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
            "\n  {}  {}  {}",
            summary.updated_at, summary.id, title,
        ));
    }
    out
}

fn read_protocol_messages(
    server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
) -> Result<Vec<Value>, String> {
    let thread = match server
        .dispatch(ClientRequest::ThreadReadVisible {
            thread_id: thread_id.clone(),
        })
        .map_err(|error| error.to_string())?
        .response
        .payload
    {
        ResponsePayload::Thread(thread) => thread,
        _ => return Err("threadReadVisible returned an unexpected response".to_string()),
    };
    Ok(thread
        .turns
        .into_iter()
        .flat_map(|turn| turn.items)
        .filter_map(|item| match item {
            ThreadItem::UserMessage { text, .. } => Some(json!({"role": "user", "content": text})),
            ThreadItem::AssistantMessage { text, .. } => {
                Some(json!({"role": "assistant", "content": text}))
            }
            _ => None,
        })
        .collect())
}

fn compact_session_text(agent: &mut AgentLoop, recorder: &mut SessionRecorder) -> String {
    let stats = agent.session.compact(&agent.context_edit);
    if let Err(error) = recorder.replace_model_context(&agent.session) {
        return format!("Compaction succeeded but persistence failed: {error}");
    }
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

fn runtime_profile_for_args(cfg: &Config, args: &Args) -> AgentRuntimeProfile {
    match args.permission_mode.as_deref() {
        Some(mode) => AgentRuntimeProfile::from_permission_mode(cfg, mode),
        None if args.sandbox.is_some() || args.approval.is_some() => {
            AgentRuntimeProfile::from_legacy_permissions(cfg)
        }
        None => AgentRuntimeProfile::from_config(cfg),
    }
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
        let bytes = std::fs::read(path)
            .map_err(|e| format!("cannot read image {}: {e}", path.display()))?;
        let url = format!("data:{};base64,{}", image_mime(path), base64_encode(&bytes));
        content.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Ok(serde_json::Value::Array(content))
}

fn validate_attachments(tools: &ncx_core::ToolRegistry, images: &[PathBuf]) -> Result<(), String> {
    if images.is_empty() {
        return Ok(());
    }
    let service = tools
        .service::<ncx_core::AttachmentServiceDescriptor>("attachment")
        .ok_or_else(|| "当前 Harness Profile 未启用附件插件".to_string())?;
    for path in images {
        let extension = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !service
            .extensions
            .iter()
            .any(|allowed| allowed == &extension)
        {
            return Err(format!("附件格式 .{extension} 未被当前插件允许"));
        }
        let size = std::fs::metadata(path)
            .map_err(|e| format!("cannot read image {}: {e}", path.display()))?
            .len();
        if size > service.max_bytes {
            return Err(format!(
                "附件 {} 超过 {} 字节限制",
                path.display(),
                service.max_bytes
            ));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_core::vision_provider_from_config;

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
        assert!(vision_provider_from_config(&cfg).is_none());
        // vl_model set -> a dedicated vision provider is constructed.
        cfg.vl_model = "qwen-vl-max".into();
        assert!(vision_provider_from_config(&cfg).is_some());
    }

    #[test]
    fn cli_and_gui_use_equivalent_runtime_profiles_for_same_config() {
        let cfg = Config {
            permission_mode: "default".into(),
            max_iterations: 9,
            max_tool_calls: 21,
            max_parallel_tool_calls: 4,
            context_edit_max_chars: 42_000,
            context_edit_keep_recent_messages: 12,
            context_edit_max_tool_result_chars: 888,
            ..Default::default()
        };

        let cli_profile = runtime_profile_for_args(&cfg, &Args::default());
        let gui_profile = AgentRuntimeProfile::from_config(&cfg);

        assert_eq!(cli_profile, gui_profile);
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
        let rows = vec![ThreadMetadata {
            id: ThreadId::new("sid").unwrap(),
            workspace: "/p".into(),
            title: "fix bug".into(),
            created_at: 1,
            updated_at: 2,
            archived: false,
        }];
        let out = render_history(&rows, 10);
        assert!(out.contains("sid"));
        assert!(out.contains("fix bug"));
        assert!(out.contains("  2  "));
    }

    #[test]
    fn cli_recorder_uses_protocol_store_for_turn_ownership_and_resume() {
        let root = std::env::temp_dir().join(format!("ncx_cli_thread_{}", new_session_id()));
        let workspace = root.join("workspace");
        let store_path = root.join("threads-v2.json");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut recorder =
            SessionRecorder::open_at(workspace.clone(), false, store_path.clone()).unwrap();
        let original_id = recorder.thread_id.clone();
        let turn_id = recorder.start_turn("修复历史恢复").unwrap();
        let thread = match recorder
            .server
            .dispatch(ClientRequest::ThreadRead {
                thread_id: original_id.clone(),
            })
            .unwrap()
            .response
            .payload
        {
            ResponsePayload::Thread(thread) => thread,
            _ => panic!("expected thread"),
        };
        assert_eq!(thread.metadata.title, "修复历史恢复");
        assert_eq!(thread.turns[0].status, TurnStatus::Running);
        assert!(matches!(
            thread.turns[0].items.first(),
            Some(ThreadItem::UserMessage { text, .. }) if text == "修复历史恢复"
        ));

        let messages = vec![
            json!({"role": "user", "content": "修复历史恢复"}),
            json!({"role": "assistant", "content": "已完成"}),
        ];
        recorder
            .server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: original_id.clone(),
                messages: messages.clone(),
            })
            .unwrap();
        recorder
            .server
            .dispatch(ClientRequest::TurnComplete {
                thread_id: original_id.clone(),
                turn_id,
                status: TurnStatus::Completed,
                error: None,
                usage: ProtocolTurnUsage::default(),
            })
            .unwrap();
        drop(recorder);

        let resumed = SessionRecorder::open_at(workspace, true, store_path).unwrap();
        assert_eq!(resumed.thread_id, original_id);
        assert_eq!(resumed.model_context(), Some(messages));

        let _ = std::fs::remove_dir_all(root);
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

    #[test]
    fn export_renders_user_assistant_tool_markdown() {
        let mut s = Session::new("system instructions");
        s.add_user_text("fix the bug");
        s.add_assistant(
            "looking into it",
            Some(vec![json!({
                "id": "c1",
                "type": "function",
                "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"}
            })]),
            "thinking step by step",
        );
        s.add_tool_result("c1", "shell", "file.rs");

        let md = render_session_markdown(&s.system, &s.messages, "deepseek-chat", Path::new("/ws"));

        assert!(md.starts_with("# nanocodex session export"));
        assert!(md.contains("model: `deepseek-chat`"));
        assert!(md.contains("messages: 3 (user 1, assistant 1, tool 1)"));
        assert!(md.contains("## System prompt"));
        assert!(md.contains("## User"));
        assert!(md.contains("fix the bug"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("<details><summary>reasoning</summary>"));
        assert!(md.contains("thinking step by step"));
        assert!(md.contains("tool call: `shell`"));
        assert!(md.contains("### Tool result: `shell`"));
        assert!(md.contains("file.rs"));
    }

    #[test]
    fn export_flattens_multimodal_and_hides_image_data() {
        let mut s = Session::new("");
        s.add_user(json!([
            {"type": "text", "text": "what is this"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ]));

        let md = render_session_markdown(&s.system, &s.messages, "m", Path::new("/ws"));

        assert!(md.contains("what is this"));
        assert!(md.contains("[image]"));
        assert!(!md.contains("AAAA"));
        // An empty system prompt is omitted entirely.
        assert!(!md.contains("## System prompt"));
    }

    #[test]
    fn export_writes_markdown_file_to_explicit_path() {
        let dir = std::env::temp_dir().join(format!("ncx_export_{}", new_session_id()));
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let mut s = Session::new("sys");
        s.add_user_text("hello world");
        let target = dir.join("out.md");

        let status = export_session_text(&s, &cfg, "sid", target.to_str().unwrap());

        assert!(status.contains("Exported 1 message(s)"));
        assert!(status.contains("out.md"));
        let written = std::fs::read_to_string(&target).unwrap();
        assert!(written.contains("hello world"));
    }

    #[test]
    fn export_refuses_to_overwrite_existing_explicit_file() {
        let dir = std::env::temp_dir().join(format!("ncx_export_clob_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let existing = dir.join("keep.md");
        std::fs::write(&existing, "IMPORTANT").unwrap();
        let mut s = Session::new("sys");
        s.add_user_text("hi");

        let status = export_session_text(&s, &cfg, "sid", existing.to_str().unwrap());

        assert!(status.contains("already exists"), "{status}");
        // The original file is untouched.
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "IMPORTANT");
    }

    #[test]
    fn export_refuses_directory_arg_with_clear_message() {
        let dir = std::env::temp_dir().join(format!("ncx_export_dir_{}", new_session_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = ncx_config::Config {
            workspace: dir.clone(),
            model: "m".into(),
            ..Default::default()
        };
        let mut s = Session::new("sys");
        s.add_user_text("hi");

        let status = export_session_text(&s, &cfg, "sid", dir.to_str().unwrap());

        assert!(status.contains("is a directory"), "{status}");
    }

    #[test]
    fn export_default_path_uses_session_id_under_exports() {
        let p = export_target_path(Path::new("/ws"), "abc123", "");
        let s = p.to_string_lossy();
        assert!(s.contains("exports"), "{s}");
        assert!(s.ends_with("abc123.md"), "{s}");

        // A relative arg resolves against the workspace; absolute is taken as-is.
        let rel = export_target_path(Path::new("/ws"), "abc123", "notes/out.md");
        assert!(rel.ends_with(Path::new("notes/out.md")));
    }

    #[test]
    fn export_uses_longer_fence_when_content_has_backticks() {
        assert_eq!(code_fence("no backticks"), "```");
        assert_eq!(code_fence("inline `code`"), "```");
        assert_eq!(code_fence("a ``` b"), "````");
        assert_eq!(code_fence("````x"), "`````");

        let mut s = Session::new("");
        s.add_tool_result("c1", "read_file", "here:\n```rust\nfn main() {}\n```\n");
        let md = render_session_markdown(&s.system, &s.messages, "m", Path::new("/ws"));
        // The wrapping fence is longer than the inner triple backticks, and the
        // inner content survives verbatim.
        assert!(md.contains("````\n"), "{md}");
        assert!(md.contains("```rust"));
        assert!(md.contains("fn main() {}"));
    }

    #[test]
    fn review_verify_prompts_reference_diff_and_scope() {
        let review = review_prompt("src/main.rs");
        assert!(review.contains("git --no-pager diff"));
        assert!(review.contains("Scope/focus for this pass: src/main.rs"));

        let sec = security_review_prompt("");
        assert!(sec.to_lowercase().contains("injection"));
        assert!(!sec.contains("Scope/focus"));

        let verify = verify_prompt("the parser");
        assert!(verify.contains("VERIFIED"));
        assert!(verify.contains("the parser"));
    }

    #[test]
    fn doc_prompts_name_format_file_and_backend() {
        let d = doc_prompt("docx", "report.docx");
        assert!(d.contains(".docx"));
        assert!(d.contains("Target file: report.docx"));
        assert!(d.to_lowercase().contains("python-docx"));

        // No file arg -> the agent is told to ask which file.
        let x = doc_prompt("xlsx", "");
        assert!(x.contains(".xlsx"));
        assert!(x.to_lowercase().contains("openpyxl"));
        assert!(x.contains("names next"));

        let p = doc_prompt("pdf", "a.pdf");
        let pl = p.to_lowercase();
        assert!(pl.contains("pdfplumber") || pl.contains("pypdf"));

        let pptx = doc_prompt("pptx", "deck.pptx");
        assert!(pptx.to_lowercase().contains("python-pptx"));
    }
}
