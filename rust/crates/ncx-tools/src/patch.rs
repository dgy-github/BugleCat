//! Codex V4A patch format — Rust port of `nanocodex/tools/patch.py`.
//!
//! ```text
//! *** Begin Patch
//! *** Add File: path/to/new.py
//! +line one
//! *** Update File: path/to/existing.py
//! @@ optional_context_header
//!  unchanged context line
//! -removed line
//! +added line
//! *** Delete File: path/to/gone.py
//! *** Move to: path/to/renamed.py
//! *** End Patch
//! ```
//!
//! Context is matched with three fallbacks (exact, rstrip, full-strip) and the
//! patch applies atomically: if any hunk fails to locate, nothing is written.

use std::path::{Path, PathBuf};

/// Raised when a patch cannot be parsed or applied.
#[derive(Debug, Clone, PartialEq)]
pub struct PatchError(pub String);

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PatchError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionType {
    Add,
    Update,
    Delete,
}

/// A contiguous change inside an Update hunk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chunk {
    pub del_lines: Vec<String>,
    pub ins_lines: Vec<String>,
    /// Locator (`@@`) lines that must precede this chunk, outermost first.
    pub locators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileAction {
    pub action: ActionType,
    pub path: String,
    pub new_lines: Vec<String>,
    pub chunks: Vec<Chunk>,
    pub move_to: Option<String>,
}

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const UPDATE: &str = "*** Update File: ";
const DELETE: &str = "*** Delete File: ";
const MOVE: &str = "*** Move to: ";
const HUNK_AT: &str = "@@";

fn err<T>(msg: impl Into<String>) -> Result<T, PatchError> {
    Err(PatchError(msg.into()))
}

/// Parse a V4A patch envelope into structured file actions.
pub fn parse_patch(text: &str) -> Result<Vec<FileAction>, PatchError> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if lines.is_empty() || lines[0].trim() != BEGIN {
        return err("patch must start with '*** Begin Patch'");
    }
    let Some(end_idx) = lines.iter().position(|l| l.trim() == END) else {
        return err("patch must end with '*** End Patch'");
    };

    let body = &lines[1..end_idx];
    let n = body.len();
    let mut actions: Vec<FileAction> = Vec::new();
    let mut i = 0usize;

    while i < n {
        let line = body[i];
        if let Some(path) = line.strip_prefix(ADD) {
            let path = path.trim().to_string();
            i += 1;
            let mut new_lines: Vec<String> = Vec::new();
            while i < n && !body[i].starts_with("*** ") {
                let content = body[i];
                if !content.is_empty() {
                    let first = content.chars().next().unwrap();
                    if first != '+' && first != ' ' {
                        return err(format!(
                            "Add File '{path}': every line must start with '+' (got {content:?})"
                        ));
                    }
                }
                // strip the leading marker byte (ASCII '+'/' ')
                new_lines.push(if content.is_empty() {
                    String::new()
                } else {
                    content[1..].to_string()
                });
                i += 1;
            }
            actions.push(FileAction {
                action: ActionType::Add,
                path,
                new_lines,
                chunks: vec![],
                move_to: None,
            });
            continue;
        }
        if let Some(path) = line.strip_prefix(DELETE) {
            actions.push(FileAction {
                action: ActionType::Delete,
                path: path.trim().to_string(),
                new_lines: vec![],
                chunks: vec![],
                move_to: None,
            });
            i += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix(UPDATE) {
            let path = path.trim().to_string();
            i += 1;
            let mut move_to = None;
            if i < n {
                if let Some(dst) = body[i].strip_prefix(MOVE) {
                    move_to = Some(dst.trim().to_string());
                    i += 1;
                }
            }
            let (chunks, next_i) = parse_update_body(body, i, n, &path)?;
            i = next_i;
            actions.push(FileAction {
                action: ActionType::Update,
                path,
                new_lines: vec![],
                chunks,
                move_to,
            });
            continue;
        }
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        return err(format!("unexpected line in patch: {line:?}"));
    }

    if actions.is_empty() {
        return err("patch contained no file actions");
    }
    Ok(actions)
}

fn parse_update_body(
    body: &[&str],
    mut i: usize,
    n: usize,
    path: &str,
) -> Result<(Vec<Chunk>, usize), PatchError> {
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut pending_locators: Vec<String> = Vec::new();
    let mut current: Option<Chunk> = None;

    // flush: commit current chunk if it has any change lines.
    macro_rules! flush {
        () => {{
            if let Some(c) = current.take() {
                if !c.del_lines.is_empty() || !c.ins_lines.is_empty() {
                    chunks.push(c);
                }
            }
        }};
    }

    while i < n && !body[i].starts_with("*** ") {
        let raw = body[i];
        if let Some(rest) = raw.strip_prefix(HUNK_AT) {
            flush!();
            let locator = rest.trim();
            if !locator.is_empty() {
                pending_locators.push(locator.to_string());
            }
            i += 1;
            continue;
        }

        let (marker, content) = match raw.chars().next() {
            Some(c) => (c, &raw[1..]),
            None => (' ', ""),
        };
        match marker {
            ' ' => {
                flush!();
                i += 1;
            }
            '+' | '-' => {
                if current.is_none() {
                    current = Some(Chunk {
                        locators: std::mem::take(&mut pending_locators),
                        ..Default::default()
                    });
                }
                let c = current.as_mut().unwrap();
                if marker == '-' {
                    c.del_lines.push(content.to_string());
                } else {
                    c.ins_lines.push(content.to_string());
                }
                i += 1;
            }
            _ => {
                return err(format!(
                    "Update File '{path}': line must start with ' ', '+', '-', or '@@' (got {raw:?})"
                ));
            }
        }
    }
    flush!();
    if chunks.is_empty() {
        return err(format!("Update File '{path}': no changes found"));
    }
    Ok((chunks, i))
}

// ── application ───────────────────────────────────────────────────────────────

/// Find *needle* in *haystack* at/after *start*; return index or None.
/// Three-level fallback: exact, rstrip, then full-strip equality.
fn match_at(haystack: &[String], needle: &[String], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    let normalizers: [fn(&str) -> &str; 3] = [|s| s, |s| s.trim_end(), |s| s.trim()];
    for norm in normalizers {
        let nn: Vec<&str> = needle.iter().map(|s| norm(s)).collect();
        for idx in start..=(haystack.len() - needle.len()) {
            let window: Vec<&str> = (0..needle.len())
                .map(|k| norm(&haystack[idx + k]))
                .collect();
            if window == nn {
                return Some(idx);
            }
        }
    }
    None
}

fn apply_update(original: &str, action: &FileAction) -> Result<String, PatchError> {
    let had_trailing_nl = original.ends_with('\n');
    let lines: Vec<String> = original.split('\n').map(|s| s.to_string()).collect();
    // Python's str.splitlines() drops a trailing empty element from a final \n;
    // mimic that so cursor math matches.
    let mut result: Vec<String> = lines;
    if had_trailing_nl {
        result.pop();
    }

    let mut cursor = 0usize;
    for chunk in &action.chunks {
        let mut search_from = cursor;
        for locator in &chunk.locators {
            let loc_needle = vec![locator.clone()];
            match match_at(&result, &loc_needle, search_from) {
                Some(idx) => search_from = idx + 1,
                None => {
                    return err(format!(
                        "Update File '{}': locator {:?} not found",
                        action.path, locator
                    ))
                }
            }
        }

        if !chunk.del_lines.is_empty() {
            let Some(idx) = match_at(&result, &chunk.del_lines, search_from) else {
                return err(format!(
                    "Update File '{}': could not locate the lines to replace:\n{}",
                    action.path,
                    chunk.del_lines.join("\n")
                ));
            };
            result.splice(
                idx..idx + chunk.del_lines.len(),
                chunk.ins_lines.iter().cloned(),
            );
            cursor = idx + chunk.ins_lines.len();
        } else {
            let insert_at = search_from;
            result.splice(insert_at..insert_at, chunk.ins_lines.iter().cloned());
            cursor = insert_at + chunk.ins_lines.len();
        }
    }

    let mut text = result.join("\n");
    if had_trailing_nl || (!action.chunks.is_empty() && !result.is_empty()) {
        text.push('\n');
    }
    Ok(text)
}

/// What changed, for reporting back to the model and CLI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplyOutcome {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    pub moved: Vec<(String, String)>,
}

impl ApplyOutcome {
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for p in &self.added {
            parts.push(format!("  A {p}"));
        }
        for (src, dst) in &self.moved {
            parts.push(format!("  R {src} -> {dst}"));
        }
        for p in &self.updated {
            parts.push(format!("  M {p}"));
        }
        for p in &self.deleted {
            parts.push(format!("  D {p}"));
        }
        parts.join("\n")
    }
}

/// Parse and apply a V4A patch under *root*, gating every touched path through
/// `can_write`. Staged fully in memory first; if any hunk fails or any path is
/// unwritable, nothing is written to disk.
pub fn apply_patch<F>(text: &str, root: &Path, can_write: F) -> Result<ApplyOutcome, PatchError>
where
    F: Fn(&Path) -> bool,
{
    let actions = parse_patch(text)?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

    let resolve = |rel: &str| -> PathBuf {
        let joined = root.join(rel);
        joined.canonicalize().unwrap_or(joined)
    };

    let mut staged_writes: Vec<(PathBuf, String)> = Vec::new();
    let mut staged_deletes: Vec<PathBuf> = Vec::new();
    let mut staged_moves: Vec<PathBuf> = Vec::new();
    let mut outcome = ApplyOutcome::default();

    for action in &actions {
        let target = resolve(&action.path);
        if !can_write(&target) {
            return err(format!(
                "path is outside the writable sandbox: {}",
                action.path
            ));
        }
        match action.action {
            ActionType::Add => {
                if target.exists() {
                    return err(format!("Add File: {} already exists", action.path));
                }
                let mut content = action.new_lines.join("\n");
                if !action.new_lines.is_empty() {
                    content.push('\n');
                }
                staged_writes.push((target, content));
                outcome.added.push(action.path.clone());
            }
            ActionType::Delete => {
                if !target.is_file() {
                    return err(format!("Delete File: {} not found", action.path));
                }
                staged_deletes.push(target);
                outcome.deleted.push(action.path.clone());
            }
            ActionType::Update => {
                if !target.is_file() {
                    return err(format!("Update File: {} not found", action.path));
                }
                let original = std::fs::read_to_string(&target)
                    .map_err(|e| PatchError(format!("read {}: {e}", action.path)))?
                    .replace("\r\n", "\n");
                let new_text = apply_update(&original, action)?;
                if let Some(move_to) = &action.move_to {
                    let dest = resolve(move_to);
                    if !can_write(&dest) {
                        return err(format!(
                            "Move target is outside the writable sandbox: {move_to}"
                        ));
                    }
                    staged_writes.push((dest, new_text));
                    staged_moves.push(target);
                    outcome.moved.push((action.path.clone(), move_to.clone()));
                } else {
                    staged_writes.push((target, new_text));
                    outcome.updated.push(action.path.clone());
                }
            }
        }
    }

    // commit
    for (path, content) in &staged_writes {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PatchError(format!("mkdir {}: {e}", parent.display())))?;
        }
        std::fs::write(path, content)
            .map_err(|e| PatchError(format!("write {}: {e}", path.display())))?;
    }
    for src in &staged_moves {
        if src.exists() {
            let _ = std::fs::remove_file(src);
        }
    }
    for path in &staged_deletes {
        std::fs::remove_file(path)
            .map_err(|e| PatchError(format!("delete {}: {e}", path.display())))?;
    }

    Ok(outcome)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_TMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn tmpdir(name: &str) -> PathBuf {
        for _ in 0..100 {
            let d = std::env::temp_dir().join(format!(
                "ncx_patch_{name}_{}_{}",
                std::process::id(),
                TEST_TMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&d) {
                Ok(()) => return d,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory {}: {error}", d.display()),
            }
        }
        panic!("could not create a unique test directory for {name}");
    }

    fn allow_all(_p: &Path) -> bool {
        true
    }

    #[test]
    fn tmpdir_creates_unique_directories() {
        let first = tmpdir("unique");
        let second = tmpdir("unique");

        assert_ne!(first, second);
        assert!(first.is_dir());
        assert!(second.is_dir());

        std::fs::remove_dir(first).unwrap();
        std::fs::remove_dir(second).unwrap();
    }

    #[test]
    fn parse_requires_begin_and_end() {
        assert!(parse_patch("nope").is_err());
        assert!(parse_patch("*** Begin Patch\n*** Add File: a\n+x").is_err()); // no end
    }

    #[test]
    fn parse_add_file() {
        let actions =
            parse_patch("*** Begin Patch\n*** Add File: a.txt\n+hello\n+world\n*** End Patch")
                .unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, ActionType::Add);
        assert_eq!(actions[0].new_lines, vec!["hello", "world"]);
    }

    #[test]
    fn parse_rejects_add_line_without_plus() {
        let e = parse_patch("*** Begin Patch\n*** Add File: a\nbad\n*** End Patch").unwrap_err();
        assert!(e.to_string().contains("must start with '+'"));
    }

    #[test]
    fn parse_update_with_locator_and_change() {
        let patch = "*** Begin Patch\n*** Update File: a.py\n@@ def main():\n-    print(\"hi\")\n+    print(\"hello\")\n*** End Patch";
        let actions = parse_patch(patch).unwrap();
        assert_eq!(actions[0].action, ActionType::Update);
        let chunk = &actions[0].chunks[0];
        assert_eq!(chunk.locators, vec!["def main():"]);
        assert_eq!(chunk.del_lines, vec!["    print(\"hi\")"]);
        assert_eq!(chunk.ins_lines, vec!["    print(\"hello\")"]);
    }

    #[test]
    fn add_file_writes_to_disk() {
        let root = tmpdir("add");
        let outcome = apply_patch(
            "*** Begin Patch\n*** Add File: new.txt\n+line1\n+line2\n*** End Patch",
            &root,
            allow_all,
        )
        .unwrap();
        assert_eq!(outcome.added, vec!["new.txt"]);
        let written = std::fs::read_to_string(root.join("new.txt")).unwrap();
        assert_eq!(written, "line1\nline2\n");
    }

    #[test]
    fn add_file_rejects_existing() {
        let root = tmpdir("add_exist");
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let e = apply_patch(
            "*** Begin Patch\n*** Add File: a.txt\n+y\n*** End Patch",
            &root,
            allow_all,
        )
        .unwrap_err();
        assert!(e.to_string().contains("already exists"));
    }

    #[test]
    fn update_replaces_matched_lines() {
        let root = tmpdir("update");
        std::fs::write(
            root.join("a.py"),
            "def main():\n    print(\"hi\")\n    return 0\n",
        )
        .unwrap();
        let patch = "*** Begin Patch\n*** Update File: a.py\n-    print(\"hi\")\n+    print(\"hello\")\n*** End Patch";
        let outcome = apply_patch(patch, &root, allow_all).unwrap();
        assert_eq!(outcome.updated, vec!["a.py"]);
        let written = std::fs::read_to_string(root.join("a.py")).unwrap();
        assert_eq!(written, "def main():\n    print(\"hello\")\n    return 0\n");
    }

    #[test]
    fn update_uses_locator_to_disambiguate() {
        let root = tmpdir("locator");
        std::fs::write(root.join("a.py"), "x = 1\nfoo()\nx = 1\nbar()\n").unwrap();
        // locator 'bar()' must steer the match to the SECOND 'x = 1'... but the
        // change targets the line after the locator: replace 'bar()'.
        let patch =
            "*** Begin Patch\n*** Update File: a.py\n@@ foo()\n-x = 1\n+x = 99\n*** End Patch";
        apply_patch(patch, &root, allow_all).unwrap();
        let written = std::fs::read_to_string(root.join("a.py")).unwrap();
        assert_eq!(written, "x = 1\nfoo()\nx = 99\nbar()\n");
    }

    #[test]
    fn update_with_whitespace_fallback() {
        let root = tmpdir("ws");
        std::fs::write(root.join("a.txt"), "  spaced line  \n").unwrap();
        // del line lacks the trailing spaces -> rstrip/strip fallback matches.
        let patch =
            "*** Begin Patch\n*** Update File: a.txt\n-spaced line\n+changed\n*** End Patch";
        apply_patch(patch, &root, allow_all).unwrap();
        let written = std::fs::read_to_string(root.join("a.txt")).unwrap();
        assert_eq!(written, "changed\n");
    }

    #[test]
    fn update_failure_to_locate_is_atomic() {
        let root = tmpdir("atomic");
        std::fs::write(root.join("a.txt"), "real content\n").unwrap();
        let patch = "*** Begin Patch\n*** Update File: a.txt\n-nonexistent line\n+x\n*** End Patch";
        let e = apply_patch(patch, &root, allow_all).unwrap_err();
        assert!(e.to_string().contains("could not locate"));
        // file untouched
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "real content\n"
        );
    }

    #[test]
    fn delete_file() {
        let root = tmpdir("delete");
        std::fs::write(root.join("gone.txt"), "bye").unwrap();
        let outcome = apply_patch(
            "*** Begin Patch\n*** Delete File: gone.txt\n*** End Patch",
            &root,
            allow_all,
        )
        .unwrap();
        assert_eq!(outcome.deleted, vec!["gone.txt"]);
        assert!(!root.join("gone.txt").exists());
    }

    #[test]
    fn move_file_writes_dest_removes_source() {
        let root = tmpdir("move");
        std::fs::write(root.join("old.txt"), "a\nb\n").unwrap();
        let patch = "*** Begin Patch\n*** Update File: old.txt\n*** Move to: new.txt\n-a\n+A\n*** End Patch";
        let outcome = apply_patch(patch, &root, allow_all).unwrap();
        assert_eq!(
            outcome.moved,
            vec![("old.txt".to_string(), "new.txt".to_string())]
        );
        assert!(!root.join("old.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            "A\nb\n"
        );
    }

    #[test]
    fn unwritable_path_blocks_whole_patch() {
        let root = tmpdir("unwritable");
        let outcome = apply_patch(
            "*** Begin Patch\n*** Add File: a.txt\n+x\n*** End Patch",
            &root,
            |_p| false,
        );
        assert!(outcome.is_err());
        assert!(!root.join("a.txt").exists());
    }

    #[test]
    fn summary_orders_a_r_m_d() {
        let o = ApplyOutcome {
            added: vec!["a".into()],
            updated: vec!["m".into()],
            deleted: vec!["d".into()],
            moved: vec![("s".into(), "t".into())],
        };
        assert_eq!(o.summary(), "  A a\n  R s -> t\n  M m\n  D d");
    }
}
