//! Layered project instructions for the Rust runtime.
//!
//! This mirrors the ecosystem convention around `AGENTS.md` (Codex-style) and
//! `CLAUDE.md` (Claude Code-style): read durable, human-authored project
//! guidance at session startup and inject it as context.

use std::path::{Path, PathBuf};

const HEADER: &str =
    "Project instructions (from AGENTS.md / CLAUDE.md files; follow unless contradicted by the user):";
const TRUNCATED: &str = "\n\n[project instructions truncated to fit the configured limit]";

/// Load user + workspace instruction files into one capped system-note block.
///
/// Search order:
/// 1. `~/.codex/AGENTS.md`
/// 2. `~/.claude/CLAUDE.md`
/// 3. `AGENTS.md`, `CLAUDE.md`, and `.claude/CLAUDE.md` from repository root
///    down to `workspace`
pub fn load_project_instructions(workspace: &Path, max_chars: usize) -> String {
    load_project_instructions_with_home(workspace, home_dir().as_deref(), max_chars)
}

/// Like [`load_project_instructions`] but WITHOUT the user's global
/// `~/.codex` / `~/.claude` files — only the workspace's own AGENTS.md/CLAUDE.md.
///
/// The GUI uses this so an end-user chat is driven by the OPENED PROJECT's
/// guidance, not the developer's personal Claude Code config (e.g. a handoff
/// protocol that makes the agent read HANDOFF.md on every first message — the
/// "why does 'hi' run a bunch of tools" surprise).
pub fn load_workspace_instructions(workspace: &Path, max_chars: usize) -> String {
    load_project_instructions_with_home(workspace, None, max_chars)
}

fn load_project_instructions_with_home(
    workspace: &Path,
    home: Option<&Path>,
    max_chars: usize,
) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    for path in instruction_paths(workspace, home) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        parts.push(format!("## {}\n{}", path.display(), text));
    }
    if parts.is_empty() {
        return String::new();
    }
    cap_block(&format!("{HEADER}\n\n{}", parts.join("\n\n")), max_chars)
}

fn instruction_paths(workspace: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home {
        out.push(home.join(".codex").join("AGENTS.md"));
        out.push(home.join(".claude").join("CLAUDE.md"));
    }

    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let root = repo_root(&workspace);
    let mut dirs = Vec::new();
    let mut cur = Some(workspace.as_path());
    while let Some(dir) = cur {
        dirs.push(dir.to_path_buf());
        if dir == root {
            break;
        }
        cur = dir.parent();
    }
    dirs.reverse();

    for dir in dirs {
        out.push(dir.join("AGENTS.md"));
        out.push(dir.join("CLAUDE.md"));
        out.push(dir.join(".claude").join("CLAUDE.md"));
    }
    out
}

fn repo_root(workspace: &Path) -> PathBuf {
    let mut cur = Some(workspace);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    workspace.to_path_buf()
}

fn cap_block(block: &str, max_chars: usize) -> String {
    if block.chars().count() <= max_chars {
        return block.to_string();
    }
    if max_chars <= TRUNCATED.chars().count() {
        return TRUNCATED.chars().take(max_chars).collect();
    }
    let keep = max_chars.saturating_sub(TRUNCATED.chars().count());
    let mut out: String = block.chars().take(keep).collect();
    out.push_str(TRUNCATED);
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ncx_instructions_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn loads_home_and_layered_workspace_files() {
        let home = tmp("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::write(home.join(".codex").join("AGENTS.md"), "global codex").unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude").join("CLAUDE.md"), "global claude").unwrap();

        let repo = tmp("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("AGENTS.md"), "repo agents").unwrap();
        let nested = repo.join("crates").join("core");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("CLAUDE.md"), "nested claude").unwrap();
        std::fs::create_dir_all(nested.join(".claude")).unwrap();
        std::fs::write(
            nested.join(".claude").join("CLAUDE.md"),
            "nested dot claude",
        )
        .unwrap();

        let block = load_project_instructions_with_home(&nested, Some(&home), 10_000);

        assert!(block.contains("global codex"));
        assert!(block.contains("global claude"));
        assert!(block.contains("repo agents"));
        assert!(block.contains("nested claude"));
        assert!(block.contains("nested dot claude"));
        assert!(
            block.find("repo agents").unwrap() < block.find("nested claude").unwrap(),
            "parent instructions should precede nested instructions"
        );
    }

    #[test]
    fn empty_when_no_files() {
        let repo = tmp("empty");
        assert_eq!(load_project_instructions_with_home(&repo, None, 10_000), "");
    }

    #[test]
    fn caps_large_instruction_block() {
        let repo = tmp("cap");
        std::fs::write(repo.join("AGENTS.md"), "x".repeat(500)).unwrap();

        let block = load_project_instructions_with_home(&repo, None, 120);

        assert!(block.chars().count() <= 120);
        assert!(block.contains("truncated"));
    }
}
