//! Read-only command heuristic — Rust port of `_looks_read_only` in
//! `nanocodex/tools/shell.py`.
//!
//! Under `read-only` sandbox mode a command classified read-only runs WITHOUT
//! an approval prompt, so this must be conservative: a write hidden behind
//! `&&`, a redirect, or command substitution must NOT pass.

/// Commands considered read-only (leading token of each chained segment).
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
    // NB: `python -c` / `node -e` are intentionally NOT here — they run arbitrary code.
];

/// Metacharacters that can hide a write even with a read-only leading token:
/// redirection, command substitution, process substitution.
const WRITE_OR_SUBSHELL: &[&str] = &[">", "`", "$(", "<(", ">("];

/// Split a command line into chained segments (`&&`, `||`, `;`, `|`, `&`, newline).
fn split_chain(s: &str) -> Vec<String> {
    // Replace the two-char operators with a sentinel first, then split on the
    // single-char set. Mirrors the Python regex `&&|\|\||[;|&\n]`.
    let mut tmp = s.replace("&&", "\u{1}").replace("||", "\u{1}");
    for ch in [';', '|', '&', '\n'] {
        tmp = tmp.replace(ch, "\u{1}");
    }
    tmp.split('\u{1}').map(|seg| seg.to_string()).collect()
}

/// True only if EVERY chained segment is a known read-only command.
///
/// Conservative on purpose — when in doubt returns false so the action goes
/// through the approval/escalation path.
pub fn looks_read_only(command: &str) -> bool {
    let stripped = command.trim();
    if stripped.is_empty() {
        return false;
    }
    if WRITE_OR_SUBSHELL.iter().any(|tok| stripped.contains(tok)) {
        return false;
    }
    for segment in split_chain(stripped) {
        let seg = segment.trim().to_lowercase();
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

// ── tests (mirror tests/test_shell_readonly.py) ───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_read_only_commands_pass() {
        for cmd in [
            "ls",
            "ls -la",
            "cat foo.py",
            "git status",
            "git diff HEAD~1",
            "rg pattern src",
            "pwd",
            "git log --oneline",
        ] {
            assert!(looks_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn plain_writes_do_not_pass() {
        for cmd in [
            "rm -rf build",
            "mkdir x",
            "git commit -m x",
            "pip install foo",
        ] {
            assert!(!looks_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn command_chain_with_write_does_not_pass() {
        for cmd in [
            "ls && rm -rf x",
            "cat a; rm b",
            "pwd || mkdir hack",
            "ls & rm -rf x",
            "ls\nrm -rf x",
        ] {
            assert!(!looks_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn all_segments_read_only_passes() {
        assert!(looks_read_only("git log --oneline | head"));
        assert!(looks_read_only("cat a && ls && pwd"));
        assert!(looks_read_only("grep foo . | wc -l"));
    }

    #[test]
    fn redirection_does_not_pass() {
        for cmd in [
            "cat a > out.txt",
            "echo hi >> log",
            "ls > files.txt",
            "cat a &> b",
        ] {
            assert!(!looks_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn command_substitution_does_not_pass() {
        for cmd in ["echo $(rm -rf x)", "cat `rm x`", "diff <(ls) <(rm y)"] {
            assert!(!looks_read_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn arbitrary_code_runners_not_assumed_read_only() {
        assert!(!looks_read_only("python -c \"open('x','w').write('1')\""));
        assert!(!looks_read_only(
            "node -e \"require('fs').writeFileSync('x','1')\""
        ));
    }

    #[test]
    fn prefix_lookalike_does_not_pass() {
        assert!(!looks_read_only("lsof -i :8080"));
        assert!(!looks_read_only("catalog-build"));
    }

    #[test]
    fn empty_is_not_read_only() {
        assert!(!looks_read_only(""));
        assert!(!looks_read_only("   "));
    }
}
