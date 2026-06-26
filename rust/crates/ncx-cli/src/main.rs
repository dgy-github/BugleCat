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

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ncx_config::{load_config, Config, Overrides};
use ncx_core::slash::{is_known, parse_slash, SLASH_HELP};
use std::rc::Rc;

use ncx_core::{
    expand_file_mentions, new_session_id, AgentLoop, CheckpointMeta, CheckpointStore,
    ContextEditPolicy, MemoryStore, Orchestrator, OrchestratorConfig, Session, SessionIndex,
    SessionSummary, TaskBudget, ToolContext, ToolRegistry,
};
use ncx_provider::DeepSeekProvider;
use ncx_sandbox::SandboxPolicy;
use serde_json::json;

use args::{parse_args, Args};
use runner::{LiveRunner, LiveSummarizer};

const SYSTEM_PROMPT: &str = "You are nanocodex, a precise coding agent. Use the provided tools \
    (read_file, apply_patch, update_plan) to inspect and edit the workspace. Prefer apply_patch \
    for edits. Keep responses concise.";

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
    let policy = SandboxPolicy::new(cfg.sandbox_mode.clone(), &cfg.workspace)
        .with_network_access(cfg.network_access);
    // Project memory: recalled into the system prompt as leads; the `remember`
    // tool lets the agent append verified notes (it gets smarter on THIS repo).
    let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
    // Periodic consolidation: fold near-duplicate notes on every start (cheap,
    // idempotent) so the store stays tidy as it grows.
    let _ = memory.consolidate(0.85);
    let recall_query = args.prompt.as_deref().unwrap_or("");
    let recall = memory.recall(recall_query, 8, 4000);
    let system_prompt = if recall.is_empty() {
        SYSTEM_PROMPT.to_string()
    } else {
        format!("{SYSTEM_PROMPT}\n\n{recall}")
    };
    let ctx = ToolContext::new(cfg.workspace.clone(), policy)
        .with_approval_policy(cfg.approval_policy.clone())
        .with_timeout(cfg.timeout_s as u64)
        .with_search(cfg.search_provider.clone(), cfg.search_api_key.clone())
        .with_memory(memory)
        .with_hooks(cfg.hooks.clone());
    let tools = ToolRegistry::new(ctx);
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
        .with_context_edit(context_edit_from_config(&cfg));
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
            return run_orchestrated(cfg, &expanded).await;
        }
        let result = agent.run_turn(json!(expanded), None).await;
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

/// Interactive REPL. Slash commands are dispatched without a model call; any
/// other line becomes a turn (with `@file` mention expansion).
async fn repl(agent: &mut AgentLoop, cfg: &ncx_config::Config, recorder: &mut SessionRecorder) {
    println!(
        "nanocodex (ncx) — model {}, sandbox {}. /help for commands, /exit to quit.",
        cfg.model, cfg.sandbox_mode
    );
    let stdin = io::stdin();
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
            match dispatch_slash(&cmd, &arg, agent, cfg) {
                SlashOutcome::Exit => break,
                SlashOutcome::Printed(text) => println!("{text}"),
            }
            continue;
        }

        run_one_turn(agent, line, cfg, recorder).await;
    }
    println!("bye.");
}

async fn run_one_turn(
    agent: &mut AgentLoop,
    prompt: &str,
    cfg: &ncx_config::Config,
    recorder: &mut SessionRecorder,
) {
    let expanded = expand_file_mentions(prompt, &cfg.workspace);
    checkpoint_before_turn(&cfg.workspace, &expanded);
    let result = agent.run_turn(json!(expanded), None).await;
    recorder.record(&agent.session);
    println!("{}", result.final_text);
}

enum SlashOutcome {
    Exit,
    Printed(String),
}

/// Handle a slash command that doesn't require a model call. Returns the text to
/// print, an exit signal, or (for unknown commands) treats the line as a prompt.
fn dispatch_slash(
    cmd: &str,
    arg: &str,
    agent: &AgentLoop,
    cfg: &ncx_config::Config,
) -> SlashOutcome {
    match cmd {
        "/exit" => SlashOutcome::Exit,
        "/help" => SlashOutcome::Printed(render_help()),
        "/status" => SlashOutcome::Printed(render_status(cfg)),
        "/history" => SlashOutcome::Printed(render_history(&SessionIndex::default().entries(), 20)),
        "/checkpoint" => SlashOutcome::Printed(create_checkpoint_text(&cfg.workspace, arg)),
        "/checkpoints" => SlashOutcome::Printed(render_checkpoints(
            &CheckpointStore::new(&cfg.workspace).list(),
            20,
        )),
        "/restore" => SlashOutcome::Printed(restore_checkpoint_text(&cfg.workspace, arg)),
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
        other => SlashOutcome::Printed(format!("Unknown command {other}. Try /help.")),
    }
}

fn render_help() -> String {
    let mut out = String::from("Commands:");
    for (cmd, help) in SLASH_HELP {
        out.push_str(&format!("\n  {cmd:<12} {help}"));
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
