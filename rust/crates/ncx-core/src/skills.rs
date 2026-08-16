//! Agent Skills — progressive-disclosure capabilities authored as `SKILL.md`
//! files, mirroring Anthropic's Agent Skills convention.
//!
//! A *skill* is a directory containing a `SKILL.md` file with YAML frontmatter:
//!
//! ```text
//! ---
//! name: pdf-forms
//! description: Fill, read, and flatten PDF form fields. Use when the user mentions PDFs or forms.
//! ---
//!
//! # How to fill a PDF form
//! ...detailed instructions, optionally referencing sibling files in this dir...
//! ```
//!
//! Progressive disclosure keeps the prompt cheap:
//!
//! * **Always on (level 1):** only each skill's `name` + `description` is
//!   injected into the system prompt as a compact index — see [`skills_index_block`].
//! * **On demand (level 2):** when the model needs one, it calls the `skill`
//!   tool with the name; that returns the full `SKILL.md` body plus the skill's
//!   directory so it can `read_file` any bundled resources (level 3).
//!
//! This is the same two-tier shape as `tool_search`: a small catalog that is
//! always visible, with the heavyweight content fetched only when relevant.

use std::path::{Path, PathBuf};

const INDEX_HEADER: &str = "Available skills (author-provided playbooks; when a task matches a \
description, call the `skill` tool with its name to load the full instructions before proceeding):";

/// A discovered skill. The body is loaded lazily (on `skill` invocation) so the
/// always-on index stays cheap even with many large skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Stable identifier (frontmatter `name:`, falling back to the directory name).
    pub name: String,
    /// One-line "when to use this" summary (frontmatter `description:`).
    pub description: String,
    /// Path to the `SKILL.md` file (a synthetic `<builtin>` path for builtins).
    pub path: PathBuf,
    /// The skill's directory (holds `SKILL.md` and any bundled resource files).
    pub dir: PathBuf,
    /// For builtin skills compiled into the binary: the body, already parsed.
    /// `None` for filesystem skills (the body is read from `path` on demand).
    pub embedded: Option<String>,
}

impl Skill {
    /// Read the full markdown body (everything after the frontmatter block).
    /// Builtins return their embedded body without touching the filesystem.
    pub fn load_body(&self) -> std::io::Result<String> {
        if let Some(body) = &self.embedded {
            return Ok(body.clone());
        }
        let text = std::fs::read_to_string(&self.path)?;
        Ok(strip_frontmatter(&text).trim().to_string())
    }

    /// True for skills compiled into the binary (no on-disk directory).
    pub fn is_builtin(&self) -> bool {
        self.embedded.is_some()
    }
}

/// Builtin skills compiled into the binary via `include_str!`. These are always
/// available; a filesystem skill of the same name shadows the builtin so users
/// can customize them.
pub fn builtin_skills() -> Vec<Skill> {
    // (name, SKILL.md contents). Add new builtins here.
    const BUILTINS: &[&str] = &[include_str!("../builtin_skills/commit-message/SKILL.md")];
    let mut out = Vec::new();
    for text in BUILTINS {
        let (name, description) = parse_frontmatter(text);
        let Some(name) = name.filter(|n| !n.trim().is_empty()) else {
            continue; // a builtin without a name is a packaging bug; skip defensively.
        };
        out.push(Skill {
            name: name.trim().to_string(),
            description: description.unwrap_or_default().trim().to_string(),
            path: PathBuf::from("<builtin>"),
            dir: PathBuf::from("<builtin>"),
            embedded: Some(strip_frontmatter(text).trim().to_string()),
        });
    }
    out
}

/// Discover skills under the user-global and workspace skill roots.
///
/// Search order (later roots override earlier same-named skills, so a project
/// can shadow a global skill):
/// 1. `~/.ncx/skills/*/SKILL.md`
/// 2. `<workspace>/.ncx/skills/*/SKILL.md`
///
/// Returns skills sorted by name. Unreadable dirs / malformed files are skipped.
pub fn discover_skills(workspace: &Path) -> Vec<Skill> {
    discover_skills_with_home(workspace, home_dir().as_deref())
}

fn discover_skills_with_home(workspace: &Path, home: Option<&Path>) -> Vec<Skill> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = home {
        roots.push(home.join(".ncx").join("skills"));
    }
    roots.push(workspace.join(".ncx").join("skills"));

    // Keyed by name. Builtins seed the map first; a later filesystem root with
    // the same name shadows it (home then workspace), so users can override.
    let mut by_name: std::collections::BTreeMap<String, Skill> = std::collections::BTreeMap::new();
    for skill in builtin_skills() {
        by_name.insert(skill.name.clone(), skill);
    }
    for root in roots {
        for skill in scan_root(&root) {
            by_name.insert(skill.name.clone(), skill);
        }
    }
    by_name.into_values().collect()
}

/// Scan one `skills/` root for `*/SKILL.md` files.
fn scan_root(root: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest = dir.join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let (name, description) = parse_frontmatter(&text);
        // Fall back to the directory name when frontmatter omits `name:`.
        let name = name.unwrap_or_else(|| {
            dir.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        if name.trim().is_empty() {
            continue;
        }
        out.push(Skill {
            name: name.trim().to_string(),
            description: description.unwrap_or_default().trim().to_string(),
            path: manifest,
            dir,
            embedded: None,
        });
    }
    out
}

/// Build the always-on system-prompt index (level-1 disclosure). Empty string
/// when no skills are discovered.
pub fn skills_index_block(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from(INDEX_HEADER);
    for s in skills {
        if s.description.is_empty() {
            out.push_str(&format!("\n- {}", s.name));
        } else {
            out.push_str(&format!("\n- {}: {}", s.name, s.description));
        }
    }
    out
}

/// Parse `name:` and `description:` from a leading `---`-fenced YAML block.
/// Returns `(name, description)`; either may be `None` if absent.
fn parse_frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut description = None;
    for line in frontmatter_lines(text) {
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(unquote(v.trim()));
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(unquote(v.trim()));
        }
    }
    (name, description)
}

/// The lines inside the leading `---` … `---` fence (empty if there is none).
fn frontmatter_lines(text: &str) -> Vec<&str> {
    let mut lines = text.lines();
    // The very first non-empty content must be the opening fence.
    match lines.next() {
        Some(l) if l.trim() == "---" => {}
        _ => return Vec::new(),
    }
    let mut out = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return out;
        }
        out.push(line);
    }
    // No closing fence -> treat as malformed (no frontmatter).
    Vec::new()
}

/// Everything after the leading frontmatter fence; the whole text if there is none.
fn strip_frontmatter(text: &str) -> &str {
    let trimmed_start = text.trim_start_matches(['\u{feff}']);
    let mut rest = trimmed_start;
    if let Some(after_open) = rest.strip_prefix("---") {
        // Require the opening fence to be its own line.
        if after_open.starts_with(['\n', '\r']) {
            if let Some(idx) = find_closing_fence(after_open) {
                rest = &after_open[idx..];
            }
        }
    }
    rest
}

/// Byte offset just past a line that is exactly `---`, searching `s`.
fn find_closing_fence(s: &str) -> Option<usize> {
    let mut offset = 0;
    for line in s.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']).trim() == "---" {
            return Some(offset + line.len());
        }
        offset += line.len();
    }
    None
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
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
        let d = std::env::temp_dir().join(format!("ncx_skills_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_skill(root: &Path, dir: &str, contents: &str) {
        let d = root.join(".ncx").join("skills").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), contents).unwrap();
    }

    /// Skills discovered from disk only (drop builtins) for exact-count asserts.
    fn fs_only(skills: Vec<Skill>) -> Vec<Skill> {
        skills.into_iter().filter(|s| !s.is_builtin()).collect()
    }

    #[test]
    fn discovers_and_parses_frontmatter() {
        let ws = tmp("discover");
        write_skill(
            &ws,
            "pdf",
            "---\nname: pdf-forms\ndescription: \"Fill PDF forms. Use for PDFs.\"\n---\n\n# Body\nDetails here.",
        );
        let skills = fs_only(discover_skills_with_home(&ws, None));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "pdf-forms");
        assert_eq!(skills[0].description, "Fill PDF forms. Use for PDFs.");
        assert_eq!(skills[0].load_body().unwrap(), "# Body\nDetails here.");
    }

    #[test]
    fn name_falls_back_to_dir() {
        let ws = tmp("fallback");
        write_skill(&ws, "my-tool", "---\ndescription: no name field\n---\nbody");
        let skills = fs_only(discover_skills_with_home(&ws, None));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-tool");
    }

    #[test]
    fn builtins_are_always_present_and_loadable() {
        let ws = tmp("builtins");
        let skills = discover_skills_with_home(&ws, None);
        let cm = skills
            .iter()
            .find(|s| s.name == "commit-message")
            .expect("commit-message builtin present");
        assert!(cm.is_builtin());
        assert!(cm.load_body().unwrap().contains("Conventional Commits"));
    }

    #[test]
    fn filesystem_skill_shadows_builtin() {
        let ws = tmp("shadow_builtin");
        write_skill(
            &ws,
            "commit-message",
            "---\nname: commit-message\ndescription: custom override\n---\nmy rules",
        );
        let skills = discover_skills_with_home(&ws, None);
        let cm = skills.iter().find(|s| s.name == "commit-message").unwrap();
        assert!(!cm.is_builtin(), "filesystem skill should win");
        assert_eq!(cm.description, "custom override");
    }

    #[test]
    fn workspace_shadows_home_same_name() {
        let home = tmp("home");
        let ws = tmp("ws");
        write_skill(
            &home,
            "shared",
            "---\nname: shared\ndescription: from home\n---\nx",
        );
        write_skill(
            &ws,
            "shared",
            "---\nname: shared\ndescription: from workspace\n---\ny",
        );
        let skills = fs_only(discover_skills_with_home(&ws, Some(&home)));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "from workspace");
    }

    #[test]
    fn index_block_lists_name_and_description() {
        let ws = tmp("index");
        write_skill(
            &ws,
            "a",
            "---\nname: alpha\ndescription: do alpha things\n---\nbody",
        );
        let skills = discover_skills_with_home(&ws, None);
        let block = skills_index_block(&skills);
        assert!(block.contains("call the `skill` tool"));
        assert!(block.contains("alpha: do alpha things"));
    }

    #[test]
    fn empty_when_no_filesystem_skills() {
        let ws = tmp("none");
        // Only builtins remain when nothing is on disk.
        assert!(fs_only(discover_skills_with_home(&ws, None)).is_empty());
        assert_eq!(skills_index_block(&[]), "");
    }

    #[test]
    fn malformed_frontmatter_skipped_or_dir_named() {
        let ws = tmp("malformed");
        // No frontmatter fence at all -> name falls back to dir, body is whole file.
        write_skill(&ws, "raw", "just a plain body, no frontmatter");
        let skills = fs_only(discover_skills_with_home(&ws, None));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "raw");
        assert_eq!(skills[0].description, "");
        assert_eq!(
            skills[0].load_body().unwrap(),
            "just a plain body, no frontmatter"
        );
    }
}
