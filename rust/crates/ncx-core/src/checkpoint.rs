//! Workspace checkpoints for reversible agent work.
//!
//! Checkpoints are stored outside Git history under `.nanocodex/checkpoints`.
//! They are intentionally coarse-grained snapshots: before a prompt runs, the
//! CLI can save the current file state and later restore it with `/restore`.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const MAX_FILES: usize = 5_000;
const MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

static CHECKPOINT_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointMeta {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub files: Vec<String>,
    pub skipped_paths: Vec<String>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub checkpoint_id: String,
    pub safety_checkpoint_id: Option<String>,
    pub restored_files: usize,
    pub deleted_files: usize,
}

pub struct CheckpointStore {
    workspace: PathBuf,
    root: PathBuf,
}

impl CheckpointStore {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let root = workspace.join(".nanocodex").join("checkpoints");
        CheckpointStore { workspace, root }
    }

    pub fn create(&self, label: &str) -> io::Result<CheckpointMeta> {
        fs::create_dir_all(&self.root)?;
        let id = new_checkpoint_id();
        let dir = self.root.join(&id);
        let files_dir = dir.join("files");
        fs::create_dir_all(&files_dir)?;

        let mut entries = list_workspace_files(&self.workspace)?;
        entries.sort();

        let mut files = Vec::new();
        let mut skipped_paths = Vec::new();
        let mut total_bytes = 0u64;

        for rel in entries {
            let rel_key = rel_to_key(&rel);
            let src = self.workspace.join(&rel);
            let Ok(meta) = fs::metadata(&src) else {
                skipped_paths.push(rel_key);
                continue;
            };
            let len = meta.len();
            let over_limits = files.len() >= MAX_FILES
                || len > MAX_FILE_BYTES
                || total_bytes.saturating_add(len) > MAX_TOTAL_BYTES;
            if over_limits {
                skipped_paths.push(rel_key);
                continue;
            }
            let dst = files_dir.join(&rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::copy(&src, &dst) {
                Ok(_) => {
                    total_bytes = total_bytes.saturating_add(len);
                    files.push(rel_key);
                }
                Err(_) => skipped_paths.push(rel_key),
            }
        }

        let meta = CheckpointMeta {
            id,
            label: label.trim().to_string(),
            created_at: now_stamp(),
            files,
            skipped_paths,
            total_bytes,
        };
        write_meta(&dir, &meta)?;
        Ok(meta)
    }

    pub fn list(&self) -> Vec<CheckpointMeta> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out = entries
            .filter_map(Result::ok)
            .filter_map(|entry| read_meta(&entry.path()).ok())
            .collect::<Vec<_>>();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        out
    }

    pub fn get(&self, id: &str) -> Option<CheckpointMeta> {
        let id = safe_checkpoint_id(id)?;
        read_meta(&self.root.join(id)).ok()
    }

    pub fn restore(&self, id: &str) -> io::Result<RestoreReport> {
        let id = safe_checkpoint_id(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid checkpoint id"))?;
        let checkpoint_dir = self.root.join(&id);
        let meta = read_meta(&checkpoint_dir)?;
        let safety = self.create(&format!("safety before restore {id}")).ok();

        let captured: HashSet<String> = meta.files.iter().cloned().collect();
        let preserve: HashSet<String> = meta
            .files
            .iter()
            .chain(meta.skipped_paths.iter())
            .cloned()
            .collect();
        let mut deleted_files = 0usize;
        for rel in list_workspace_files(&self.workspace)? {
            let rel_key = rel_to_key(&rel);
            if preserve.contains(&rel_key) {
                continue;
            }
            let target = self.workspace.join(&rel);
            if inside_workspace(&self.workspace, &target) && fs::remove_file(target).is_ok() {
                deleted_files += 1;
            }
        }

        let mut restored_files = 0usize;
        for rel_key in &captured {
            let rel = key_to_path(rel_key)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad checkpoint path"))?;
            let src = checkpoint_dir.join("files").join(&rel);
            let dst = self.workspace.join(&rel);
            if !inside_workspace(&self.workspace, &dst) {
                continue;
            }
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src, dst)?;
            restored_files += 1;
        }

        Ok(RestoreReport {
            checkpoint_id: id,
            safety_checkpoint_id: safety.map(|m| m.id),
            restored_files,
            deleted_files,
        })
    }
}

fn list_workspace_files(workspace: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(workspace, workspace, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };
        if should_exclude(rel) {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(root, &path, out)?;
        } else if ft.is_file() {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

fn should_exclude(rel: &Path) -> bool {
    let parts = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return false;
    }
    matches!(
        parts[0],
        ".git"
            | ".hg"
            | ".svn"
            | ".nanocodex"
            | ".ncx"
            | "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | "dist"
            | "build"
    )
}

fn write_meta(dir: &Path, meta: &CheckpointMeta) -> io::Result<()> {
    let value = json!({
        "id": meta.id,
        "label": meta.label,
        "created_at": meta.created_at,
        "files": meta.files.clone(),
        "skipped_paths": meta.skipped_paths.clone(),
        "total_bytes": meta.total_bytes,
    });
    let text = serde_json::to_string_pretty(&value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(dir.join("manifest.json"), text)
}

fn read_meta(dir: &Path) -> io::Result<CheckpointMeta> {
    let text = fs::read_to_string(dir.join("manifest.json"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(CheckpointMeta {
        id: string_field(&value, "id"),
        label: string_field(&value, "label"),
        created_at: string_field(&value, "created_at"),
        files: string_array(&value, "files"),
        skipped_paths: string_array(&value, "skipped_paths"),
        total_bytes: value
            .get("total_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .filter(|s| key_to_path(s).is_some())
                .collect()
        })
        .unwrap_or_default()
}

fn rel_to_key(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn key_to_path(key: &str) -> Option<PathBuf> {
    if key.trim().is_empty() || key.contains('\\') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in key.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

fn safe_checkpoint_id(id: &str) -> Option<String> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn inside_workspace(workspace: &Path, path: &Path) -> bool {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let lexical = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let candidate = if lexical.exists() {
        lexical.canonicalize().unwrap_or_else(|_| lexical.clone())
    } else {
        path.parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.join(path.file_name().unwrap_or_default()))
            .unwrap_or_else(|| lexical.clone())
    };
    candidate.starts_with(&ws) || lexical.starts_with(workspace)
}

fn new_checkpoint_id() -> String {
    let seq = CHECKPOINT_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{}-{:04}", now_stamp(), seq)
}

fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:013}", d.as_millis()))
        .unwrap_or_else(|_| "0000000000000".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws(name: &str) -> PathBuf {
        let dir = crate::test_support::unique_temp_dir(&format!("ncx_checkpoint_{name}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_and_restore_reverts_modified_and_new_files() {
        let ws = tmp_ws("restore");
        fs::write(ws.join("a.txt"), "old").unwrap();
        fs::create_dir_all(ws.join("src")).unwrap();
        fs::write(ws.join("src").join("b.txt"), "keep").unwrap();
        let store = CheckpointStore::new(&ws);
        let meta = store.create("before turn").unwrap();

        fs::write(ws.join("a.txt"), "new").unwrap();
        fs::write(ws.join("created.txt"), "new file").unwrap();
        let report = store.restore(&meta.id).unwrap();

        assert_eq!(fs::read_to_string(ws.join("a.txt")).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(ws.join("src").join("b.txt")).unwrap(),
            "keep"
        );
        assert!(!ws.join("created.txt").exists());
        assert_eq!(report.restored_files, 2);
        assert_eq!(report.deleted_files, 1);
        assert!(report.safety_checkpoint_id.is_some());
    }

    #[test]
    fn list_returns_newest_first() {
        let ws = tmp_ws("list");
        fs::write(ws.join("a.txt"), "one").unwrap();
        let store = CheckpointStore::new(&ws);
        let first = store.create("first").unwrap();
        fs::write(ws.join("a.txt"), "two").unwrap();
        let second = store.create("second").unwrap();

        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, second.id);
        assert_eq!(list[1].id, first.id);
    }

    #[test]
    fn checkpoint_paths_reject_traversal() {
        assert!(key_to_path("../x").is_none());
        assert!(key_to_path("a\\b").is_none());
        assert!(safe_checkpoint_id("../x").is_none());
    }
}
