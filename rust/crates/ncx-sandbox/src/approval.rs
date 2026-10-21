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

/// Leading commands that only launch another command. Classification looks
/// through them, so `cmd /c curl …` is judged as `curl …`.
const SHELL_WRAPPERS: &[&str] = &["cmd", "powershell", "pwsh", "bash", "sh", "zsh", "dash"];
const PREFIX_WRAPPERS: &[&str] = &[
    "env", "sudo", "nohup", "time", "nice", "xargs", "start", "call", "exec", "command",
];

/// Interpreters whose inline code (`-c`, `-e`, `-Command`) may open sockets.
const INLINE_INTERPRETERS: &[&str] = &[
    "python",
    "python3",
    "py",
    "node",
    "deno",
    "bun",
    "perl",
    "ruby",
    "powershell",
    "pwsh",
];

/// Unambiguous markers of network access inside inline interpreter code.
const INLINE_NETWORK_MARKERS: &[&str] = &[
    "urllib.request",
    "urllib3",
    "http.client",
    "requests.get(",
    "requests.post(",
    "requests.request(",
    "httpx.",
    "aiohttp",
    "socket.create_connection",
    "pip.main(",
    "pip._internal",
    "fetch(",
    "require('http",
    "require(\"http",
    "require('net')",
    "net.webclient",
    "downloadstring(",
    "downloadfile(",
    "invoke-webrequest",
    "invoke-restmethod",
    "start-bitstransfer",
    "system.net.http",
    "net::http",
    "lwp::",
];

/// Strip one level of surrounding quotes from a wrapped script.
fn unquote(text: &str) -> &str {
    let t = text.trim();
    for q in ['"', '\''] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            return &t[1..t.len() - 1];
        }
    }
    t.trim_matches(|c| c == '"' || c == '\'')
}

/// If `segment` starts with a launcher (`cmd /c`, `powershell -Command`,
/// `bash -c`, `env VAR=1`, `xargs`, …), return the command it launches.
/// `Err(())` marks a wrapper whose payload cannot be inspected
/// (`powershell -EncodedCommand`).
fn wrapped_command(segment: &str) -> Result<Option<String>, ()> {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let Some(first) = tokens.first() else {
        return Ok(None);
    };
    let head = normalize_executable_token(first);
    let rest = &tokens[1..];
    let join = |from: usize| -> Option<String> {
        let inner = unquote(&rest[from.min(rest.len())..].join(" "))
            .trim()
            .to_string();
        (!inner.is_empty()).then_some(inner)
    };
    if SHELL_WRAPPERS.contains(&head.as_str()) {
        for (i, tok) in rest.iter().enumerate() {
            let flag = tok.to_ascii_lowercase();
            let flag = flag.trim_start_matches('/').trim_start_matches('-');
            match (head.as_str(), flag) {
                ("cmd", "c" | "k") => return Ok(join(i + 1)),
                ("powershell" | "pwsh", "e" | "ec" | "enc" | "encodedcommand") => return Err(()),
                ("powershell" | "pwsh", "c" | "command" | "f" | "file") => return Ok(join(i + 1)),
                ("bash" | "sh" | "zsh" | "dash", "c") => return Ok(join(i + 1)),
                _ => {}
            }
            if !tok.starts_with('-') && !tok.starts_with('/') {
                // powershell's first positional argument is the command; a
                // shell's first positional is a script file (inspect its name).
                return Ok(join(i));
            }
        }
        return Ok(None);
    }
    if PREFIX_WRAPPERS.contains(&head.as_str()) {
        let start = rest
            .iter()
            .position(|tok| !tok.starts_with('-') && !tok.contains('=') && !tok.starts_with('/'))
            .unwrap_or(rest.len());
        return Ok(join(start));
    }
    Ok(None)
}

/// Metacharacters that can hide a write even with a read-only leading token:
/// redirection, command substitution, process substitution.
const WRITE_OR_SUBSHELL: &[&str] = &[">", "`", "$(", "<(", ">("];

/// Patterns that always require approval regardless of the leading token.
pub fn matches_dangerous_pattern(command: &str) -> bool {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    let pats = PATS.get_or_init(|| {
        [
            r"(?i)\brm\s+-[a-z]*[rf]",
            r"(?i)\brm\s+(?:\S+\s+)*--(?:recursive|force)\b",
            r"(?i)\bdel\s+/[fqs]",
            r"(?i)\b(?:rmdir|rd)\s+/s",
            // PowerShell recursive / forced deletes (aliases included).
            r"(?i)(?:\bremove-item\b|(?:^|[;&|(]\s*)(?:ri|erase)\b)[^;&|]*\s-(?:recurse|force|r)\b",
            // git operations that discard work or history without a trace.
            r"(?i)\bgit\b[^;&|]*\breset\b[^;&|]*--(?:hard|merge|keep)\b",
            r"(?i)\bgit\b[^;&|]*\bclean\b[^;&|]*\s-[a-z]*[fdx]",
            r"(?i)\bgit\b[^;&|]*\bcheckout\b[^;&|]*(?:\s--\s|\s-f\b|\s--force\b|\s\.\s*$)",
            r"(?i)\bgit\b[^;&|]*\brestore\b",
            r"\bgit\b[^;&|]*\bbranch\b[^;&|]*\s-(?:D|-delete\s[^;&|]*--force)\b",
            r"(?i)\bgit\b[^;&|]*\bpush\b[^;&|]*(?:\s-f\b|--force|\s\+\S)",
            r"(?i)\bgit\b[^;&|]*\bstash\s+(?:drop|clear)\b",
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

/// Options that turn an otherwise read-only command into a writer or an
/// arbitrary-command runner (`find -delete`, `find -exec`, `git diff
/// --output=…`, `rg --pre`, `tree -o`, `git branch <new>`).
fn segment_has_side_effect_flags(segment: &str) -> bool {
    let tokens: Vec<String> = segment
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c| c == '"' || c == '\'')
                .to_ascii_lowercase()
        })
        .collect();
    let Some(first) = tokens.first() else {
        return false;
    };
    let head = normalize_executable_token(first);
    let rest = &tokens[1..];
    let has = |names: &[&str]| {
        rest.iter().any(|t| {
            names
                .iter()
                .any(|n| t == n || t.starts_with(&format!("{n}=")))
        })
    };
    match head.as_str() {
        "find" => has(&[
            "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf",
            "-fls",
        ]),
        "rg" => has(&["--pre"]),
        "tree" => has(&["-o"]),
        "git" => {
            let sub_pos = rest.iter().position(|t| !t.starts_with('-'));
            let Some(sub_pos) = sub_pos else {
                return false;
            };
            let sub = rest[sub_pos].as_str();
            let args = &rest[sub_pos + 1..];
            if matches!(sub, "diff" | "show" | "log" | "format-patch")
                && args
                    .iter()
                    .any(|t| t == "--output" || t.starts_with("--output=") || t == "-o")
            {
                return true;
            }
            if sub == "branch" {
                const WRITE: &[&str] = &[
                    "-d",
                    "-m",
                    "-c",
                    "-f",
                    "--delete",
                    "--move",
                    "--copy",
                    "--force",
                    "--set-upstream-to",
                    "-u",
                    "--unset-upstream",
                    "--edit-description",
                    "--track",
                    "--no-track",
                ];
                const LIST_WITH_ARG: &[&str] = &[
                    "--list",
                    "-l",
                    "--contains",
                    "--no-contains",
                    "--merged",
                    "--no-merged",
                    "--points-at",
                    "--sort",
                    "--format",
                ];
                // Case matters for -D/-M/-C; tokens are lowercased, so -d covers both.
                if args.iter().any(|t| {
                    WRITE
                        .iter()
                        .any(|w| t == w || t.starts_with(&format!("{w}=")))
                }) {
                    return true;
                }
                let positional = args.iter().any(|t| !t.starts_with('-'));
                let listing = args.iter().any(|t| {
                    LIST_WITH_ARG
                        .iter()
                        .any(|w| t == w || t.starts_with(&format!("{w}=")))
                });
                return positional && !listing;
            }
            false
        }
        _ => false,
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
        if segment_has_side_effect_flags(segment.trim()) {
            return false;
        }
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
        if segment_has_side_effect_flags(seg) {
            return false;
        }
        // A trusted launcher (`env`, …) is only as trusted as what it launches.
        match wrapped_command(seg) {
            Err(()) => return false,
            Ok(Some(inner)) => {
                if !is_trusted(&inner) {
                    return false;
                }
                continue;
            }
            Ok(None) => {}
        }
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
    command_uses_network_depth(command, 0)
}

fn command_uses_network_depth(command: &str, depth: usize) -> bool {
    for segment in split_chain(command) {
        let mut tokens = segment.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };
        let base = normalize_executable_token(first);
        if NETWORK_COMMANDS.contains(&base.as_str()) {
            return true;
        }
        if INLINE_INTERPRETERS.contains(&base.as_str()) {
            let lower = segment.to_ascii_lowercase();
            if INLINE_NETWORK_MARKERS.iter().any(|m| lower.contains(m)) {
                return true;
            }
        }
        match wrapped_command(&segment) {
            // An uninspectable payload is not assumed to be offline.
            Err(()) => return true,
            Ok(Some(inner)) if depth < 4 => {
                if command_uses_network_depth(&inner, depth + 1) {
                    return true;
                }
                if normalize_executable_token(first) == "xargs"
                    && inner
                        .split_whitespace()
                        .any(|t| NETWORK_COMMANDS.contains(&normalize_executable_token(t).as_str()))
                {
                    return true;
                }
            }
            _ => {}
        }
        let rest_tokens: Vec<String> = tokens.map(|t| t.to_ascii_lowercase()).collect();
        let sub = rest_tokens
            .iter()
            .find(|t| !t.starts_with('-'))
            .map(String::as_str)
            .unwrap_or("");
        let is_net_subcmd = match base.as_str() {
            "git" => matches!(
                sub,
                "push" | "fetch" | "pull" | "clone" | "ls-remote" | "submodule"
            ),
            "cargo" => matches!(sub, "publish" | "install" | "login"),
            "npm" | "pnpm" | "yarn" => {
                matches!(sub, "publish" | "install" | "add" | "ci" | "login")
            }
            "pip" | "pip3" => matches!(sub, "install" | "download" | "wheel"),
            "uv" => matches!(sub, "publish" | "pip"),
            "poetry" => matches!(sub, "publish" | "add" | "install"),
            "python" | "python3" | "py" => {
                let is_pip_mod = rest_tokens
                    .windows(2)
                    .any(|w| w[0] == "-m" && matches!(w[1].as_str(), "pip" | "pip3" | "uv"))
                    && rest_tokens
                        .iter()
                        .any(|t| matches!(t.as_str(), "install" | "download" | "wheel"));
                let seg_lower = segment.to_ascii_lowercase();
                let is_inline_net = seg_lower.contains("urllib.request")
                    || seg_lower.contains("http.client")
                    || seg_lower.contains("requests.get(")
                    || seg_lower.contains("pip.main(")
                    || seg_lower.contains("pip._internal");
                is_pip_mod || is_inline_net
            }
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
    if split_chain(command)
        .iter()
        .any(|segment| wrapped_command(segment).is_err())
    {
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
        // `$HOME/…`, `${APPDATA}\…`, `$env:USERPROFILE\…`, `%TEMP%\…`: the
        // target cannot be resolved here, so a write through it is treated as
        // leaving the workspace. Workspace-relative variables are allowed.
        if check_writes && is_unresolved_env_path(token) {
            return true;
        }
        if has_parent_traversal(token) {
            let resolved = workdir.join(candidate_path);
            if !policy.can_read(&resolved) || (check_writes && !policy.can_write(&resolved)) {
                return true;
            }
        } else if looks_like_explicit_abs_path(token)
            && (!policy.can_read(candidate_path)
                || (check_writes && !policy.can_write(candidate_path)))
        {
            return true;
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
        after_redir.split_once('=').map_or("", |(_, val)| val)
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

fn is_unresolved_env_path(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    let starts_with_var =
        lower.starts_with('$') || (lower.starts_with('%') && lower[1..].contains('%'));
    if !starts_with_var || !(lower.contains('/') || lower.contains('\\')) {
        return false;
    }
    const WORKSPACE_VARS: &[&str] = &["$pwd", "${pwd}", "$(pwd)", "%cd%", "$env:pwd"];
    !WORKSPACE_VARS.iter().any(|v| lower.starts_with(v))
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
        assert!(command_needs_escalation(
            &policy,
            "curl https://example.com",
            &ws
        ));
        assert!(command_needs_escalation(
            &policy,
            "git push origin main",
            &ws
        ));
        assert!(command_needs_escalation(
            &policy,
            "python -m pip download sqlparse==0.5.3",
            &ws
        ));
        assert!(command_needs_escalation(
            &policy,
            "python -m pip install pytest",
            &ws
        ));
        // With network_access == true, curl does not escalate:
        let net_policy = policy.clone().with_network_access(true);
        assert!(!command_needs_escalation(
            &net_policy,
            "curl https://example.com",
            &ws
        ));
        // Destructive commands escalate even inside workspace:
        assert!(command_needs_escalation(&policy, "rm -rf build", &ws));
        // Escaping paths & sensitive files escalate:
        assert!(command_needs_escalation(
            &policy,
            "echo x > ../outside.txt",
            &ws
        ));
        assert!(command_needs_escalation(&policy, "cat ~/.ssh/id_rsa", &ws));
        assert!(command_needs_escalation(&policy, "cat .env", &ws));
        assert!(command_needs_escalation(
            &policy,
            "echo hook > .git/hooks/pre-commit",
            &ws
        ));
        // Read-only mode escalates any write command:
        let ro = SandboxPolicy::new(READ_ONLY, &ws);
        assert!(command_needs_escalation(&ro, "mkdir sub", &ws));
        assert!(!command_needs_escalation(&ro, "git status", &ws));
    }

    #[test]
    fn review_bypasses_now_escalate_under_workspace_write() {
        let ws = std::env::temp_dir().join("ncx_approval_bypass_ws");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        for cmd in [
            // destructive, in-workspace
            "git reset --hard HEAD~3",
            "git clean -fdx",
            "git checkout -- .",
            "git restore src/lib.rs",
            "git branch -D main",
            "git stash drop",
            "Remove-Item -Recurse -Force src",
            "ri src -r",
            "rm --recursive --force src",
            "rd /s /q build",
            // network through launchers / inline code
            "cmd /c curl https://example.com",
            "cmd.exe /d /s /c \"curl https://example.com\"",
            "powershell -Command Invoke-WebRequest https://example.com",
            "pwsh -NoProfile -c \"iwr https://example.com\"",
            "powershell -EncodedCommand SQBXAFIA",
            "bash -c \"curl https://example.com\"",
            "env HTTPS_PROXY=x curl https://example.com",
            "cat urls.txt | xargs -n1 curl",
            "python -c \"import urllib.request; urllib.request.urlopen('https://x')\"",
            "node -e \"fetch('https://x')\"",
            "python -m pip install requests",
            // writes outside the workspace through env-var paths
            "echo x > $HOME/.bashrc",
            "echo x > ${HOME}/.profile",
            "echo x > %USERPROFILE%\\evil.txt",
            "copy a.txt %APPDATA%\\x.txt",
            "Set-Content $env:USERPROFILE\\x.txt hi",
        ] {
            assert!(
                command_needs_escalation(&policy, cmd, &ws),
                "should escalate: {cmd}"
            );
        }
    }

    #[test]
    fn everyday_commands_still_run_without_approval() {
        let ws = std::env::temp_dir().join("ncx_approval_benign_ws");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
        for cmd in [
            "git status",
            "git branch",
            "git branch -a --list 'feat/*'",
            "git log --oneline -5",
            "git diff HEAD~1",
            "git checkout -b feature/x",
            "git reset HEAD src/lib.rs",
            "find . -name '*.rs'",
            "grep -rn urllib.request src",
            "rg reset --hard docs",
            "cmd /c dir",
            "python script.py",
            "python -m pytest tests -q",
            "cargo build > build.log",
            "echo $HOME",
            "echo done > $PWD/out.txt",
            "rm old.txt",
            "grep -ri todo -r src",
        ] {
            assert!(
                !command_needs_escalation(&policy, cmd, &ws),
                "should not escalate: {cmd}"
            );
        }
    }

    #[test]
    fn read_only_flags_that_write_or_exec_are_not_read_only() {
        for cmd in [
            "find . -name '*.tmp' -delete",
            "find . -exec rm {} +",
            "git branch -D main",
            "git branch new-feature",
            "git diff --output=out.patch",
            "git log -o log.txt",
            "rg --pre ./evil.sh pattern",
            "tree -o listing.txt",
        ] {
            assert!(!looks_read_only(cmd), "not read-only: {cmd}");
            let ro = SandboxPolicy::new(READ_ONLY, std::env::temp_dir());
            assert!(
                command_needs_escalation(&ro, cmd, &std::env::temp_dir()),
                "{cmd}"
            );
        }
        for cmd in [
            "find . -name '*.rs'",
            "git branch -vv",
            "git branch --contains abc123",
            "git diff --stat",
        ] {
            assert!(looks_read_only(cmd), "read-only: {cmd}");
        }
        // A trusted launcher does not launder an untrusted payload.
        assert!(!is_trusted("env curl https://example.com"));
        assert!(!is_trusted("find . -delete"));
        assert!(is_trusted("env git status"));
    }
}
