//! REPL slash-command parsing — Rust port of `nanocodex/agent/slash.py`.
//!
//! Pure string logic so the dispatcher is unit-tested without a console: this
//! module only recognizes and splits a slash line; the REPL maps the parsed
//! command to an action.

/// Default `/loop` interval when none is given (matches Claude Code: 10m).
pub const DEFAULT_LOOP_INTERVAL_S: u64 = 600;

/// command -> one-line help, in display order. Mirrors `SLASH_HELP`.
pub const SLASH_HELP: &[(&str, &str)] = &[
    ("/help", "Show this help."),
    (
        "/status",
        "Show model, sandbox, approval, workspace, and token usage.",
    ),
    (
        "/usage",
        "Show token usage for the last turn and current REPL session.",
    ),
    (
        "/cost",
        "Alias for /usage (raw token usage; no price table).",
    ),
    (
        "/config",
        "Show the config path, or persist a setting: /config key=value.",
    ),
    (
        "/model",
        "Show the current model, or switch it: /model <name>.",
    ),
    (
        "/approvals",
        "Show the approval policy, or set it: /approvals <untrusted|on-failure|on-request|never>.",
    ),
    ("/diff", "Show the working-tree git diff."),
    ("/plan", "Show the current step plan."),
    ("/skills", "List available agent skills (name + when to use)."),
    ("/history", "Show recent saved sessions."),
    ("/checkpoint", "Create a workspace checkpoint."),
    ("/checkpoints", "List recent workspace checkpoints."),
    (
        "/restore",
        "Restore files from a checkpoint: /restore <id>.",
    ),
    (
        "/loop",
        "Repeat a prompt on an interval: /loop [5m] <prompt> (Ctrl+C stops).",
    ),
    ("/compact", "Fold the conversation now to the token budget."),
    ("/clear", "Start a fresh conversation (keep settings)."),
    ("/exit", "Quit the REPL (also /quit)."),
];

/// True if `cmd` is a known slash command.
pub fn is_known(cmd: &str) -> bool {
    SLASH_HELP.iter().any(|(c, _)| *c == cmd)
}

/// Parse a human duration into whole seconds, or None if it isn't one.
///
/// Accepts `30s` / `5m` / `1h` and a bare number (seconds). Zero/negative
/// durations are rejected.
pub fn parse_duration(text: &str) -> Option<u64> {
    let s = text.trim();
    if s.is_empty() {
        return None;
    }
    // Split into numeric part and an optional single unit suffix.
    let (num_part, unit) = match s.chars().last() {
        Some(c) if c == 's' || c == 'm' || c == 'h' || c == 'S' || c == 'M' || c == 'H' => {
            (&s[..s.len() - 1], c.to_ascii_lowercase())
        }
        _ => (s, '\0'),
    };
    let num_part = num_part.trim();
    if num_part.is_empty() {
        return None;
    }
    // Reject embedded whitespace ("5 m extra" must not parse).
    if num_part.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    let value: f64 = num_part.parse().ok()?;
    let factor = match unit {
        '\0' | 's' => 1.0,
        'm' => 60.0,
        'h' => 3600.0,
        _ => return None,
    };
    let secs = (value * factor) as i64;
    if secs > 0 {
        Some(secs as u64)
    } else {
        None
    }
}

/// Split a `/loop` argument into `(interval_seconds, prompt)`.
///
/// A leading token that parses as a duration becomes the interval; otherwise the
/// whole argument is the prompt at `default_s`.
pub fn split_loop_arg(arg: &str, default_s: u64) -> (u64, String) {
    let trimmed = arg.trim_start();
    if let Some(sp) = trimmed.find(char::is_whitespace) {
        let head = &trimmed[..sp];
        let rest = trimmed[sp..].trim();
        if let Some(dur) = parse_duration(head) {
            return (dur, rest.to_string());
        }
    }
    (default_s, arg.trim().to_string())
}

/// Return `(Some(command), arg)` for a slash line, or `(None, "")` if not one.
///
/// The command token is lower-cased; the remainder (trimmed) is the argument.
/// `/quit` normalizes to `/exit`.
pub fn parse_slash(text: &str) -> (Option<String>, String) {
    let s = text.trim();
    if !s.starts_with('/') {
        return (None, String::new());
    }
    let (cmd_tok, arg) = match s.find(char::is_whitespace) {
        Some(sp) => (&s[..sp], s[sp..].trim()),
        None => (s, ""),
    };
    let mut cmd = cmd_tok.to_lowercase();
    if cmd == "/quit" {
        cmd = "/exit".to_string();
    }
    (Some(cmd), arg.to_string())
}

// ── tests (mirror tests/test_slash.py) ────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ps(text: &str) -> (Option<String>, String) {
        parse_slash(text)
    }

    #[test]
    fn plain_text_is_not_a_command() {
        assert_eq!(ps("fix the bug in foo.py"), (None, String::new()));
        assert_eq!(ps(""), (None, String::new()));
    }

    #[test]
    fn bare_command() {
        assert_eq!(ps("/status"), (Some("/status".into()), String::new()));
        assert_eq!(ps("  /help  "), (Some("/help".into()), String::new()));
    }

    #[test]
    fn command_with_argument() {
        assert_eq!(
            ps("/model deepseek-chat"),
            (Some("/model".into()), "deepseek-chat".into())
        );
        assert_eq!(
            ps("/approvals never"),
            (Some("/approvals".into()), "never".into())
        );
    }

    #[test]
    fn quit_normalizes_to_exit() {
        assert_eq!(ps("/quit"), (Some("/exit".into()), String::new()));
    }

    #[test]
    fn case_insensitive_command() {
        assert_eq!(ps("/MODEL Foo"), (Some("/model".into()), "Foo".into()));
    }

    #[test]
    fn help_table_covers_core_commands() {
        for c in [
            "/help",
            "/config",
            "/usage",
            "/cost",
            "/model",
            "/approvals",
            "/diff",
            "/skills",
            "/history",
            "/checkpoint",
            "/checkpoints",
            "/restore",
            "/loop",
            "/compact",
            "/clear",
            "/exit",
        ] {
            assert!(is_known(c), "{c}");
        }
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s"), Some(30));
        assert_eq!(parse_duration("5m"), Some(300));
        assert_eq!(parse_duration("1h"), Some(3600));
        assert_eq!(parse_duration("90"), Some(90));
        assert_eq!(parse_duration("1.5m"), Some(90));
    }

    #[test]
    fn parse_duration_rejects_non_durations() {
        for bad in ["", "abc", "run", "5x", "0", "-3", "5 m extra"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    #[test]
    fn split_loop_arg_with_leading_interval() {
        assert_eq!(
            split_loop_arg("5m run the tests", DEFAULT_LOOP_INTERVAL_S),
            (300, "run the tests".into())
        );
        assert_eq!(
            split_loop_arg("30s /diff", DEFAULT_LOOP_INTERVAL_S),
            (30, "/diff".into())
        );
    }

    #[test]
    fn split_loop_arg_without_interval_uses_default() {
        assert_eq!(
            split_loop_arg("run the tests", DEFAULT_LOOP_INTERVAL_S),
            (DEFAULT_LOOP_INTERVAL_S, "run the tests".into())
        );
        assert_eq!(
            split_loop_arg("check status now", DEFAULT_LOOP_INTERVAL_S),
            (DEFAULT_LOOP_INTERVAL_S, "check status now".into())
        );
    }

    #[test]
    fn split_loop_arg_empty() {
        assert_eq!(
            split_loop_arg("", DEFAULT_LOOP_INTERVAL_S),
            (DEFAULT_LOOP_INTERVAL_S, String::new())
        );
    }
}
