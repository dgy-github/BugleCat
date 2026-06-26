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

use std::sync::OnceLock;

use regex::Regex;

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

/// Commands considered safe to run without approval under `untrusted` (first
/// token of the command). Conservative, read-only-ish allowlist.
const TRUSTED_COMMANDS: &[&str] = &[
    "ls", "cat", "pwd", "echo", "head", "tail", "wc", "grep", "rg", "find", "which", "type",
    "file", "stat", "tree", "date", "whoami", "env", "printenv", "git", "python", "python3",
    "node", "pytest", "ruff", "true", "false", "dir", "where",
];

/// git subcommands that are NOT read-only -> still need approval under untrusted.
const GIT_WRITE_SUBCMDS: &[&str] = &[
    "push",
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

/// Patterns that always require approval regardless of the leading token.
fn dangerous_patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        [
            r"\brm\s+-[rf]",
            r"\bdel\s+/[fq]",
            r"\brmdir\s+/s",
            r"(?:^|[;&|]\s*)format\b",
            r"\b(mkfs|diskpart|dd)\b",
            r"\b(shutdown|reboot|poweroff)\b",
            r":\(\)\s*\{.*\};\s*:",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static dangerous-pattern regex is valid"))
        .collect()
    })
}

fn first_token(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// Whether a command is in the read-only-ish allowlist (untrusted policy).
fn is_trusted(command: &str) -> bool {
    if dangerous_patterns().iter().any(|re| re.is_match(command)) {
        return false;
    }
    let head = first_token(command);
    // Strip a path prefix and a trailing .exe so "C:\\...\\git.exe" -> "git".
    let base = head.rsplit(['/', '\\']).next().unwrap_or(&head);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    if !TRUSTED_COMMANDS.contains(&base) {
        return false;
    }
    if base == "git" {
        let sub = command
            .split_whitespace()
            .skip(1)
            .find(|t| !t.starts_with('-'))
            .unwrap_or("")
            .to_lowercase();
        if GIT_WRITE_SUBCMDS.contains(&sub.as_str()) {
            return false;
        }
    }
    true
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
        // A space-free path prefix + .exe suffix must normalize to "git".
        // (An unquoted path with spaces would split on the space — same as the
        // Python shlex behavior — so that's not a realistic trusted invocation.)
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
}
