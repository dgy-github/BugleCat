//! ncx — nanocodex CLI (Rust). Entry point + REPL.
//!
//! Rust port of the runnable surface of `nanocodex/cli.py`: argument parsing,
//! config resolution, building the provider + tool registry + turn loop, a
//! one-shot mode (`ncx "do X"`) and an interactive REPL with slash commands.
//!
//! Kept dependency-light (hand-rolled arg parsing, no clap) in line with the
//! rewrite's goal: fast startup and a small single binary.

mod args;
mod cli_app;
mod command_support;
mod runner;
mod runtime_support;
mod session_recorder;
mod usage;

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
    expand_file_mentions, list_custom_commands, load_project_instructions, new_session_id,
    prepare_mcp_server_tools, AgentLoop, AgentRuntimeProfile, CheckpointMeta, CheckpointStore,
    ConfiguredHarnessRuntime, ContextServiceDescriptor, Genome, HarnessAgentRunner,
    McpServiceDescriptor, MemoryStore, Orchestrator, OrchestratorConfig, RuntimeContextSources,
    RuntimeHostBindings, Session, Tool, TurnResult,
};
use ncx_protocol::{
    ClientRequest, ItemId, ResponsePayload, Thread, ThreadId, ThreadItem, ThreadMetadata, TurnId,
    TurnStatus, TurnUsage as ProtocolTurnUsage,
};
use ncx_thread_store::{default_thread_store_path, JsonThreadStore};
use serde_json::{json, Value};

use args::{parse_args, Args};
use cli_app::run;
use runner::memory_summarizer;

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

/// Run a single prompt through the tiered flash/pro orchestrator and print the
/// outcome (complexity, verify status, final text).
async fn run_orchestrated(cfg: Config, prompt: &str, recorder: &mut SessionRecorder) -> i32 {
    let fast = if cfg.fast_model.is_empty() {
        cfg.model.clone()
    } else {
        cfg.fast_model.clone()
    };
    eprintln!("[orchestrator] main={}  fast={}", cfg.model, fast);
    let turn_id =
        match recorder.start_turn_with_mode(prompt, ncx_protocol::ExecutionMode::Orchestrator) {
            Ok(turn_id) => turn_id,
            Err(error) => {
                eprintln!("ncx: cannot start orchestrated turn: {error}");
                return 1;
            }
        };
    let orchestrator_config = OrchestratorConfig::from_runtime_config(&cfg);
    let runner = HarnessAgentRunner::new(cfg);
    let orch = Orchestrator::new(&runner, orchestrator_config);
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
    let status = if outcome.cancelled {
        TurnStatus::Cancelled
    } else if outcome.verify_passed {
        TurnStatus::Completed
    } else {
        TurnStatus::Failed
    };
    if let Err(error) = recorder.finish_external_turn(
        &turn_id,
        prompt,
        &outcome.final_text,
        status,
        (!outcome.verify_passed && !outcome.cancelled)
            .then(|| "orchestrator verification failed".to_string()),
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
    let prepared = prepare_configured_mcp_tools(&servers).await;
    report_mcp_server_failures(&prepared.failures);
    if !servers.is_empty() && prepared.successful_servers == 0 {
        return format!(
            "MCP reload failed: all {server_count} configured server(s) failed. Existing {} MCP tool(s) retained.",
            current_names.len()
        );
    }

    let old_count = current_names.len();
    let skipped_server_count = prepared.failures.len();
    match agent.tools.replace_tools(current_names, prepared.tools) {
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
            let skipped = if skipped_server_count > 0 {
                format!("; {skipped_server_count} server(s) skipped")
            } else {
                String::new()
            };
            format!(
                "MCP reload complete: {server_count} server(s), {new_count} tool(s) active; replaced {old_count}{skipped}."
            )
        }
        Err(error) => {
            format!("MCP reload rejected: {error}. Existing {old_count} MCP tool(s) retained.")
        }
    }
}

struct PreparedMcpTools {
    tools: Vec<Box<dyn Tool>>,
    successful_servers: usize,
    failures: Vec<(String, String)>,
}

async fn prepare_configured_mcp_tools(servers: &[McpServerConfig]) -> PreparedMcpTools {
    prepare_configured_mcp_tools_with(servers, |server| {
        let name = server.name.clone();
        let command = server.command.clone();
        let args = server.args.clone();
        let env = server.env.clone();
        async move { prepare_mcp_server_tools(&name, &command, &args, &env).await }
    })
    .await
}

async fn prepare_configured_mcp_tools_with<F, Fut>(
    servers: &[McpServerConfig],
    mut prepare: F,
) -> PreparedMcpTools
where
    F: FnMut(&McpServerConfig) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<Box<dyn Tool>>, String>>,
{
    let mut tools = Vec::new();
    let mut successful_servers = 0;
    let mut failures = Vec::new();
    for server in servers {
        match prepare(server).await {
            Ok(mut server_tools) => {
                successful_servers += 1;
                tools.append(&mut server_tools);
            }
            Err(error) => failures.push((server.name.clone(), error)),
        }
    }
    PreparedMcpTools {
        tools,
        successful_servers,
        failures,
    }
}

fn report_mcp_server_failures(failures: &[(String, String)]) {
    for (name, error) in failures {
        eprintln!("mcp: skipped server '{name}': {error}");
    }
}

use command_support::*;
use runtime_support::*;
#[cfg(test)]
use session_recorder::render_history;
use session_recorder::{emit_usage_line, protocol_history, SessionRecorder};
use usage::UsageTracker;

#[cfg(test)]
mod main_tests;
