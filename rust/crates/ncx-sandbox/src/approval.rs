//! Codex-style approval state machine — Rust port of `nanocodex/sandbox/approval.py`.
//!
//! Four approval policies:
//!
//! * `untrusted`  — auto-run only known-safe (read-only-ish) commands; ask for the rest
//! * `on-failure` — run sandboxed first; if it fails, ask to retry unsandboxed
//! * `on-request` — model decides when to ask; escalation requests are honored (default)
//! * `never`      — never ask; anything needing approval is denied and reported
//!
//! [`Approver::classify`] is the pure decision (auto-approve / ask / auto-deny).
//! The actual yes/no prompt is an upper-layer (CLI/GUI) concern, so it is not in
//! this crate; [`ApprovalRequest`] is the context such a prompt would receive.

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::policy::{
    is_protected_write_path, is_sensitive_read_path, SandboxPolicy, DANGER_FULL_ACCESS,
};

pub const UNTRUSTED: &str = "untrusted";
pub const ON_FAILURE: &str = "on-failure";
pub const ON_REQUEST: &str = "on-request";
pub const NEVER: &str = "never";

/// The pure decision for whether a command may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run it (sandboxed).
    AutoApprove,
    /// Must prompt the user.
    Ask,
    /// Refuse without asking (policy = never).
    AutoDeny,
}

/// Context handed to an approval prompt for a human decision.
#[derive(Debug, Clone, Default)]
pub struct ApprovalRequest {
    pub command: String,
    pub reason: String,
    pub cwd: String,
    pub escalated: bool,
    /// Optional extra context shown verbatim — e.g. apply_patch passes the full
    /// patch (a diff) so the user reviews the change, not just file names.
    pub details: String,
}

/// Commands that MODIFY state — under "confirm each step" these always prompt.
pub const WRITE_TOOLS: &[&str] = &["shell", "apply_patch"];

/// Layer per-step confirmation on top of the sandbox-escalation decision.
///
/// When the user turned OFF auto-approve (`require_step_approval`), any WRITE
/// action that would otherwise auto-run is upgraded to [`Decision::Ask`].
/// [`Decision::AutoDeny`] is never softened; an existing [`Decision::Ask`] stays.
pub fn step_decision(base: Decision, is_write: bool, require_step_approval: bool) -> Decision {
    if base == Decision::AutoDeny {
        return base;
    }
    if require_step_approval && is_write && base == Decision::AutoApprove {
        return Decision::Ask;
    }
    base
}

/// Commands considered strictly read-only (leading prefix of each chained segment).
const READ_ONLY_PREFIXES: &[&str] = &[
    "ls",
    "cat",
    "pwd",
    "echo",
    "head",
    "tail",
    "wc",
    "grep",
    "rg",
    "find",
    "which",
    "type",
    "file",
    "stat",
    "tree",
    "git status",
    "git log",
    "git diff",
    "git show",
    "git branch",
    "dir",
    "where",
    "date",
    "whoami",
    "true",
    "false",
    // NB: `python -c` / `node -e` are intentionally NOT here — they run arbitrary code.
];

/// Commands considered safe to run without approval under `untrusted` (first
/// token of each segment). Conservative, read-only-ish allowlist.
const TRUSTED_COMMANDS: &[&str] = &[
    "ls", "cat", "pwd", "echo", "head", "tail", "wc", "grep", "rg", "find", "which", "type",
    "file", "stat", "tree", "date", "whoami", "env", "printenv", "git", "python", "python3",
    "node", "pytest", "ruff", "true", "false", "dir", "where",
];

/// git subcommands that are NOT read-only -> still need approval under untrusted.
const GIT_WRITE_SUBCMDS: &[&str] = &[
    "push",
    "fetch",
    "pull",
    "clone",
    "commit",
    "reset",
    "rebase",
    "merge",
    "clean",
    "checkout",
    "branch",
    "tag",
    "stash",
    "rm",
    "mv",
    "cherry-pick",
    "revert",
    "am",
];

/// Direct network-initiating binaries.
const NETWORK_COMMANDS: &[&str] = &[
    "curl",
    "wget",
    "invoke-webrequest",
    "invoke-restmethod",
    "iwr",
    "irm",
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "nc",
    "ncat",
    "netcat",
    "telnet",
    "ftp",
    "ping",
    "nslookup",
    "dig",
    "tracert",
    "traceroute",
];

/// Metacharacters that can hide a write even with a read-only leading token:
/// redirection, command substitution, process substitution.
const WRITE_OR_SUBSHELL: &[&str] = &[">", "`", "$(", "<(", ">("];

/// Patterns that always require approval regardless of the leading token.
pub fn matches_dangerous_pattern(command: &str) -> bool {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    let pats = PATS.get_or_init(|| {
        [
            r"(?i)\brm\s+-[a-z]*[rf]",
            r"(?i)\bdel\s+/[fq]",
            r"(?i)\brmdir\s+/s",
            r"(?i)(?:^|[;&|]\s*)format\b",
            r"(?i)\b(mkfs|diskpart|dd)\b",
            r"(?i)\b(shutdown|reboot|poweroff)\b",
            r":\(\)\s*\{.*\};\s*:",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static dangerous-pattern regex is valid"))
        .collect()
    });
    pats.iter().any(|re| re.is_match(command))
}

/// Split a command line into chained segments (`&&`, `||`, `;`, `|`, `&`, newline).
pub fn split_chain(s: &str) -> Vec<String> {
    let mut tmp = s
        .replace("2>&1", " ")
        .replace("1>&2", " ")
        .replace("&&", "\u{1}")
        .replace("||", "\u{1}");
    for ch in [';', '|', '&', '\n'] {
        tmp = tmp.replace(ch, "\u{1}");
    }
    tmp.split('\u{1}').map(|seg| seg.to_string()).collect()
}

fn normalize_executable_token(token: &str) -> String {
    let lower = token
        .trim_matches(|c: char| "\"'()".contains(c))
        .to_ascii_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    base.strip_suffix(".exe").unwrap_or(base).to_string()
}

fn normalize_segment_head(segment: &str) -> String {
    let mut iter = segment.split_whitespace();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let head = normalize_executable_token(first);
    let rest: Vec<&str> = iter.collect();
    if rest.is_empty() {
        head
    } else {
        format!("{head} {}", rest.join(" ").to_ascii_lowercase())
    }
}

/// True only if EVERY chained segment is a known read-only command and the
/// command contains no redirection, command substitution, or dangerous pattern.
pub fn looks_read_only(command: &str) -> bool {
    let stripped = command.trim();
    if stripped.is_empty() {
        return false;
    }
    if WRITE_OR_SUBSHELL.iter().any(|tok| stripped.contains(tok)) {
        return false;
    }
    if matches_dangerous_pattern(stripped) {
        return false;
    }
    for segment in split_chain(stripped) {
        let seg = normalize_segment_head(segment.trim());
        if seg.is_empty() {
            continue;
        }
        let ok = READ_ONLY_PREFIXES.iter().any(|p| {
            seg == *p || seg.starts_with(&format!("{p} ")) || seg.starts_with(&format!("{p}\t"))
        });
        if !ok {
            return false;
        }
    }
    true
}

/// Whether a command is in the read-only-ish allowlist (`untrusted` policy).
pub fn is_trusted(command: &str) -> bool {
    let stripped = command.trim();
    if stripped.is_empty() {
        return false;
    }
    if WRITE_OR_SUBSHELL.iter().any(|tok| stripped.contains(tok)) {
        return false;
    }
    if matches_dangerous_pattern(stripped) {
        return false;
    }
    let mut saw_segment = false;
    for segment in split_chain(stripped) {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        saw_segment = true;
        let mut tokens = seg.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };
        let base = normalize_executable_token(first);
        if !TRUSTED_COMMANDS.contains(&base.as_str()) {
            return false;
        }
        if base == "git" {
            let sub = tokens
                .find(|t| !t.starts_with('-'))
                .unwrap_or("")
                .to_ascii_lowercase();
            if GIT_WRITE_SUBCMDS.contains(&sub.as_str()) {
                return false;
            }
        }
    }
    saw_segment
}

/// Heuristic: does any chained segment of `command` initiate network access?
pub fn command_uses_network(command: &str) -> bool {
    for segment in split_chain(command) {
        let mut tokens = segment.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };
        let base = normalize_executable_token(first);
        if NETWORK_COMMANDS.contains(&base.as_str()) {
            return true;
        }
        let sub = tokens
            .find(|t| !t.starts_with('-'))
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_net_subcmd = match base.as_str() {
            "git" => matches!(
                sub.as_str(),
                "push" | "fetch" | "pull" | "clone" | "ls-remote" | "submodule"
            ),
            "cargo" => matches!(sub.as_str(), "publish" | "install" | "login"),
            "npm" | "pnpm" | "yarn" => {
                matches!(sub.as_str(), "publish" | "install" | "add" | "ci" | "login")
            }
            "pip" | "pip3" => matches!(sub.as_str(), "install" | "download"),
            "uv" => matches!(sub.as_str(), "publish" | "pip"),
            "poetry" => matches!(sub.as_str(), "publish" | "add" | "install"),
            _ => false,
        };
        if is_net_subcmd {
            return true;
        }
    }
    false
}

/// Evaluate whether `command` running in `workdir` requires escalation beyond
/// `policy`.
///
/// Covers:
/// 1. `read-only` mode: any non-read-only command, network command (when
///    `!policy.network_access`), or command referencing sensitive credential paths.
/// 2. `workspace-write` mode:
///    - `workdir` outside `writable_roots`,
///    - destructive patterns (`rm -rf`, `del /f`, `format`, `mkfs`, etc.),
///    - network commands when `!policy.network_access`,
///    - path arguments or redirections that escape `writable_roots` (`../...`,
///      `~/...`, absolute paths outside workspace) or touch protected targets
///      (`.git/hooks/**`, `.git/config`, `.env*`, private keys).
pub fn command_needs_escalation(policy: &SandboxPolicy, command: &str, workdir: &Path) -> bool {
    if policy.mode == DANGER_FULL_ACCESS {
        return false;
    }
    if matches_dangerous_pattern(command) {
        return true;
    }
    if !policy.network_access && command_uses_network(command) {
        return true;
    }
    if !policy.writes_allowed() {
        if !looks_read_only(command) {
            return true;
        }
        return command_touches_disallowed_paths(policy, command, workdir, false);
    }
    if !policy.can_write(workdir) {
        return true;
    }
    let check_writes = !looks_read_only(command);
    command_touches_disallowed_paths(policy, command, workdir, check_writes)
}

fn command_touches_disallowed_paths(
    policy: &SandboxPolicy,
    command: &str,
    workdir: &Path,
    check_writes: bool,
) -> bool {
    for raw_token in command.split_whitespace() {
        let token = clean_path_token(raw_token);
        if token.is_empty() || is_benign_token(token) {
            continue;
        }
        let candidate_path = Path::new(token);
        if is_sensitive_read_path(candidate_path)
            || (check_writes && is_protected_write_path(candidate_path))
        {
            return true;
        }
        if token == "~" || token.starts_with("~/") || token.starts_with(r"~\") {
            return true;
        }
        if has_parent_traversal(token) {
            let resolved = workdir.join(candidate_path);
            if !policy.can_read(&resolved) || (check_writes && !policy.can_write(&resolved)) {
                return true;
            }
        } else if looks_like_explicit_abs_path(token) {
            if !policy.can_read(candidate_path)
                || (check_writes && !policy.can_write(candidate_path))
            {
                return true;
            }
        }
    }
    false
}

fn clean_path_token(raw: &str) -> &str {
    let trimmed = raw.trim_matches(|c: char| ";|&()\"'".contains(c));
    let after_redir = trimmed
        .strip_prefix("1>>")
        .or_else(|| trimmed.strip_prefix("2>>"))
        .or_else(|| trimmed.strip_prefix("&>>"))
        .or_else(|| trimmed.strip_prefix(">>"))
        .or_else(|| trimmed.strip_prefix("1>"))
        .or_else(|| trimmed.strip_prefix("2>"))
        .or_else(|| trimmed.strip_prefix("&>"))
        .or_else(|| trimmed.strip_prefix('>'))
        .or_else(|| trimmed.strip_prefix('<'))
        .unwrap_or(trimmed);
    let after_flag = if after_redir.starts_with('-') {
        after_redir
            .split_once('=')
            .map_or("", |(_, val)| val)
    } else {
        after_redir
    };
    after_flag.trim_matches(|c: char| "\"'".contains(c))
}

fn is_benign_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if lower == "nul" || lower == "/dev/null" {
        return true;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return true;
    }
    // Short Windows cmd switches like `/c`, `/d`, `/q`, `/s`
    if token.starts_with('/') && !token[1..].contains('/') && token.len() <= 4 {
        return true;
    }
    false
}

fn has_parent_traversal(token: &str) -> bool {
    token == ".."
        || token.starts_with("../")
        || token.starts_with(r"..\")
        || token.contains("/../")
        || token.contains(r"\..\")
        || token.ends_with("/..")
        || token.ends_with(r"\..")
}

fn looks_like_explicit_abs_path(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    if token.starts_with(r"\\") {
        return true;
    }
    if token.starts_with('/') && token[1..].contains('/') {
        return true;
    }
    false
}

/// Decide whether a shell command may run under a given policy.
#[derive(Debug, Clone)]
pub struct Approver {
    pub policy: String,
}

impl Approver {
    pub fn new(policy: impl Into<String>) -> Self {
        Approver {
            policy: policy.into(),
        }
    }

    /// Pure decision: can this run automatically, must we ask, or auto-deny?
    ///
    /// `needs_escalation` is true when the command wants something the sandbox
    /// forbids (e.g. writing outside the workspace, or network access).
    pub fn classify(&self, command: &str, needs_escalation: bool) -> Decision {
        match self.policy.as_str() {
            NEVER => {
                if needs_escalation {
                    Decision::AutoDeny
                } else {
                    Decision::AutoApprove
                }
            }
            ON_REQUEST => {
                if needs_escalation {
                    Decision::Ask
                } else {
                    Decision::AutoApprove
                }
            }
            // Run first; approval is only sought after a sandboxed failure.
            ON_FAILURE => Decision::AutoApprove,
            UNTRUSTED => {
                if is_trusted(command) && !needs_escalation {
                    Decision::AutoApprove
                } else {
                    Decision::Ask
                }
            }
            _ => Decision::Ask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{READ_ONLY, WORKSPACE_WRITE};

    // Mirrors the approval tests in tests/test_sandbox.py.

    #[test]
    fn never_policy_auto_denies_escalation() {
        let a = Approver::new(NEVER);
        assert_eq!(a.classify("rm -rf /", true), Decision::AutoDeny);
        assert_eq!(a.classify("ls", false), Decision::AutoApprove);
    }

    #[test]
    fn on_request_asks_only_on_escalation() {
        let a = Approver::new(ON_REQUEST);
        assert_eq!(a.classify("ls", false), Decision::AutoApprove);
        assert_eq!(a.classify("curl example.com", true), Decision::Ask);
    }

    #[test]
    fn on_failure_runs_first() {
        let a = Approver::new(ON_FAILURE);
        assert_eq!(a.classify("anything", true), Decision::AutoApprove);
    }

    #[test]
    fn untrusted_auto_approves_safe_commands() {
        let a = Approver::new(UNTRUSTED);
        assert_eq!(a.classify("ls -la", false), Decision::AutoApprove);
        assert_eq!(a.classify("git status", false), Decision::AutoApprove);
        assert_eq!(a.classify("cat file.txt", false), Decision::AutoApprove);
    }

    #[test]
    fn untrusted_asks_for_unknown_or_write_commands() {
        let a = Approver::new(UNTRUSTED);
        assert_eq!(a.classify("npm install", false), Decision::Ask);
        assert_eq!(a.classify("git push", false), Decision::Ask);
        assert_eq!(a.classify("rm -rf build", false), Decision::Ask);
        assert_eq!(a.classify("ls && npm install", false), Decision::Ask);
        assert_eq!(a.classify("echo hi > out.txt", false), Decision::Ask);
    }

    #[test]
    fn untrusted_blocks_dangerous_even_if_leading_token_trusted() {
        let a = Approver::new(UNTRUSTED);
        // 'git clean -fd' leads with trusted 'git' but is a write subcommand.
        assert_eq!(a.classify("git clean -fd", false), Decision::Ask);
    }

    #[test]
    fn dangerous_pattern_blocks_trusted_leading_token() {
        // `rm -rf` is dangerous regardless; not trusted even bare.
        let a = Approver::new(UNTRUSTED);
        assert_eq!(a.classify("rm -rf /tmp/x", false), Decision::Ask);
    }

    #[test]
    fn step_decision_upgrades_writes_when_confirming_each_step() {
        // auto-approve write -> ask when per-step confirmation is on.
        assert_eq!(
            step_decision(Decision::AutoApprove, true, true),
            Decision::Ask
        );
        // not a write, or confirmation off -> unchanged.
        assert_eq!(
            step_decision(Decision::AutoApprove, false, true),
            Decision::AutoApprove
        );
        assert_eq!(
            step_decision(Decision::AutoApprove, true, false),
            Decision::AutoApprove
        );
        // auto-deny is never softened.
        assert_eq!(
            step_decision(Decision::AutoDeny, true, true),
            Decision::AutoDeny
        );
    }

    #[test]
    fn git_exe_with_path_prefix_is_normalized() {
        let a = Approver::new(UNTRUSTED);
        assert_eq!(
            a.classify("/usr/bin/git status", false),
            Decision::AutoApprove
        );
        assert_eq!(
            a.classify(r"C:\tools\git.exe status", false),
            Decision::AutoApprove
        );
    }

    #[test]
    fn workspace_write_escalation_catches_network_escape_and_protected_targets() {
        let ws = std::env::temp_dir().join("ncx_esc_ws");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        // Normal workspace commands do not escalate:
        assert!(!command_needs_escalation(&policy, "echo hello", &ws));
        assert!(!command_needs_escalation(&policy, "cargo test", &ws));
        assert!(!command_needs_escalation(&policy, "git diff HEAD~1", &ws));
        // Network commands escalate when network_access == false:
        assert!(command_needs_escalation(&policy, "curl https://example.com", &ws));
        assert!(command_needs_escalation(&policy, "git push origin main", &ws));
        // With network_access == true, curl does not escalate:
        let net_policy = policy.clone().with_network_access(true);
        assert!(!command_needs_escalation(&net_policy, "curl https://example.com", &ws));
        // Destructive commands escalate even inside workspace:
        assert!(command_needs_escalation(&policy, "rm -rf build", &ws));
        // Escaping paths & sensitive files escalate:
        assert!(command_needs_escalation(&policy, "echo x > ../outside.txt", &ws));
        assert!(command_needs_escalation(&policy, "cat ~/.ssh/id_rsa", &ws));
        assert!(command_needs_escalation(&policy, "cat .env", &ws));
        assert!(command_needs_escalation(&policy, "echo hook > .git/hooks/pre-commit", &ws));
        // Read-only mode escalates any write command:
        let ro = SandboxPolicy::new(READ_ONLY, &ws);
        assert!(command_needs_escalation(&ro, "mkdir sub", &ws));
        assert!(!command_needs_escalation(&ro, "git status", &ws));
    }
}
