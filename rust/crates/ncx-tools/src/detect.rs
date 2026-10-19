//! Read-only command heuristic — re-exported from [`ncx_sandbox::looks_read_only`]
//! so `ncx-sandbox` and `ncx-tools` share a single command classifier.

pub use ncx_sandbox::looks_read_only;

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
