//! Hand-rolled argument parsing (no clap — keeps startup fast and the binary
//! small). Mirrors the flags `nanocodex/cli.py` exposes.

use std::path::PathBuf;

pub const USAGE: &str = "\
ncx — nanocodex coding agent

USAGE:
    ncx [OPTIONS] [PROMPT]

ARGS:
    PROMPT                  Run a single turn with this prompt, print the reply, exit.
                            Omit to start the interactive REPL.

OPTIONS:
    -w, --workspace <DIR>   Workspace root (default: current directory).
    -m, --model <NAME>      Model to use (overrides config).
    -p, --profile <NAME>    Config profile from ~/.nanocodex/config.toml.
    -s, --sandbox <MODE>    read-only | workspace-write | danger-full-access.
    -a, --approval <POLICY> untrusted | on-failure | on-request | never.
        --permission-mode <MODE>
                            plan | default | accept-edits | bypass (Claude-Code
                            style; when set, overrides --sandbox / --approval).
        --max-iterations <N>
                            Max model calls for one task (default: config/60).
        --max-tool-calls <N>
                            Max tool calls for one task (default: config/120).
        --context-edit-max-chars <N>
                            Provider-view context character budget.
        --context-edit-keep-recent <N>
                            Messages kept before older prefixes may be dropped.
        --context-edit-tool-result-chars <N>
                            Max chars for compressed old tool results.
        --disable-context-edit
                            Send full history without runtime context editing.
        --image <PATH>      Attach an image to the prompt (repeatable). Routes the
                            turn to the configured vision model. One-shot only.
    -r, --resume            Resume the workspace session log before starting.
        --history           List recent saved sessions, then exit.
    -o, --orchestrate       Run the prompt through the tiered flash/pro orchestrator
                            (classify → plan → parallel workers → verify). One-shot only.
        --memory-merge      Maintenance: LLM-fold near-duplicate project memory notes, then exit.
        --dump-genome       Print the default harness genome (system_prompt + core tool
                            descriptions) as TOML, then exit. Used by the ncx-forge trainer.
    -h, --help              Show this help.
    -V, --version           Show version.";

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub prompt: Option<String>,
    pub workspace: Option<PathBuf>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub sandbox: Option<String>,
    pub approval: Option<String>,
    /// Claude-Code permission mode (plan/default/accept-edits/bypass). When set,
    /// derives sandbox + approval + edit/plan gating, overriding --sandbox/--approval.
    pub permission_mode: Option<String>,
    pub max_iterations: Option<i64>,
    pub max_tool_calls: Option<i64>,
    pub context_edit_max_chars: Option<i64>,
    pub context_edit_keep_recent_messages: Option<i64>,
    pub context_edit_max_tool_result_chars: Option<i64>,
    pub disable_context_edit: bool,
    /// Image files to attach to the one-shot prompt (multimodal / vision turn).
    pub images: Vec<PathBuf>,
    pub resume: bool,
    pub history: bool,
    pub orchestrate: bool,
    pub memory_merge: bool,
    /// Print the default genome (system_prompt + core tool descriptions) as TOML and exit.
    pub dump_genome: bool,
    pub help: bool,
    pub version: bool,
}

/// Parse argv (without the program name). Unknown flags and missing values are
/// errors; the first non-flag argument (and anything after) becomes the prompt.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut prompt_parts: Vec<String> = Vec::new();
    let mut i = 0;
    let mut positional_only = false;

    while i < argv.len() {
        let a = &argv[i];
        if positional_only || !a.starts_with('-') {
            prompt_parts.push(a.clone());
            i += 1;
            continue;
        }
        match a.as_str() {
            "--" => positional_only = true,
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "-o" | "--orchestrate" => args.orchestrate = true,
            "-r" | "--resume" => args.resume = true,
            "--history" => args.history = true,
            "--memory-merge" => args.memory_merge = true,
            "--dump-genome" => args.dump_genome = true,
            "--disable-context-edit" => args.disable_context_edit = true,
            "-w" | "--workspace" => {
                args.workspace = Some(PathBuf::from(take_value(argv, &mut i, a)?));
            }
            "--image" => args.images.push(PathBuf::from(take_value(argv, &mut i, a)?)),
            "-m" | "--model" => args.model = Some(take_value(argv, &mut i, a)?),
            "-p" | "--profile" => args.profile = Some(take_value(argv, &mut i, a)?),
            "-s" | "--sandbox" => args.sandbox = Some(take_value(argv, &mut i, a)?),
            "-a" | "--approval" => args.approval = Some(take_value(argv, &mut i, a)?),
            "--permission-mode" => args.permission_mode = Some(take_value(argv, &mut i, a)?),
            "--max-iterations" => args.max_iterations = Some(take_i64(argv, &mut i, a)?),
            "--max-tool-calls" => args.max_tool_calls = Some(take_i64(argv, &mut i, a)?),
            "--context-edit-max-chars" => {
                args.context_edit_max_chars = Some(take_i64(argv, &mut i, a)?);
            }
            "--context-edit-keep-recent" => {
                args.context_edit_keep_recent_messages = Some(take_i64(argv, &mut i, a)?);
            }
            "--context-edit-tool-result-chars" => {
                args.context_edit_max_tool_result_chars = Some(take_i64(argv, &mut i, a)?);
            }
            other => return Err(format!("unknown option '{other}'")),
        }
        i += 1;
    }

    if !prompt_parts.is_empty() {
        args.prompt = Some(prompt_parts.join(" "));
    }
    Ok(args)
}

/// Consume the next argv element as a flag's value, advancing the cursor.
fn take_value(argv: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    let next = argv
        .get(*i + 1)
        .ok_or_else(|| format!("option '{flag}' needs a value"))?;
    *i += 1;
    Ok(next.clone())
}

fn take_i64(argv: &[String], i: &mut usize, flag: &str) -> Result<i64, String> {
    let raw = take_value(argv, i, flag)?;
    raw.parse::<i64>()
        .map_err(|_| format!("option '{flag}' needs an integer value"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Result<Args, String> {
        parse_args(&parts.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn empty_is_repl_mode() {
        let a = args(&[]).unwrap();
        assert!(a.prompt.is_none());
        assert!(!a.help);
    }

    #[test]
    fn help_and_version_flags() {
        assert!(args(&["-h"]).unwrap().help);
        assert!(args(&["--help"]).unwrap().help);
        assert!(args(&["-V"]).unwrap().version);
    }

    #[test]
    fn options_with_values() {
        let a = args(&[
            "-m",
            "deepseek-chat",
            "-s",
            "read-only",
            "-p",
            "fast",
            "--max-iterations",
            "7",
            "--max-tool-calls",
            "9",
        ])
        .unwrap();
        assert_eq!(a.model.as_deref(), Some("deepseek-chat"));
        assert_eq!(a.sandbox.as_deref(), Some("read-only"));
        assert_eq!(a.profile.as_deref(), Some("fast"));
        assert_eq!(a.max_iterations, Some(7));
        assert_eq!(a.max_tool_calls, Some(9));
    }

    #[test]
    fn positional_becomes_prompt() {
        let a = args(&["fix", "the", "bug"]).unwrap();
        assert_eq!(a.prompt.as_deref(), Some("fix the bug"));
    }

    #[test]
    fn flags_then_prompt() {
        let a = args(&["-m", "x", "do", "something"]).unwrap();
        assert_eq!(a.model.as_deref(), Some("x"));
        assert_eq!(a.prompt.as_deref(), Some("do something"));
    }

    #[test]
    fn double_dash_forces_positional() {
        let a = args(&["--", "-m", "literal"]).unwrap();
        assert_eq!(a.prompt.as_deref(), Some("-m literal"));
        assert!(a.model.is_none());
    }

    #[test]
    fn missing_value_errors() {
        assert!(args(&["--model"]).is_err());
    }

    #[test]
    fn numeric_flags_validate_integer_values() {
        assert!(args(&["--max-tool-calls", "abc"]).is_err());
        let a = args(&["--disable-context-edit"]).unwrap();
        assert!(a.disable_context_edit);
    }

    #[test]
    fn resume_and_history_flags() {
        let a = args(&["--resume", "--history"]).unwrap();
        assert!(a.resume);
        assert!(a.history);
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn dump_genome_flag() {
        assert!(args(&["--dump-genome"]).unwrap().dump_genome);
        assert!(!args(&[]).unwrap().dump_genome);
    }
}
