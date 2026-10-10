use super::types::FileSnapshot;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

/// Bounded host snapshot, including uncommitted/untracked files. Never follows
/// symlinks. Incomplete reads cannot support a freshness certification.
pub fn snapshot(root: &Path) -> FileSnapshot {
    snapshot_excluding(root, &[])
}
pub fn snapshot_excluding(root: &Path, excluded: &[PathBuf]) -> FileSnapshot {
    let mut result = FileSnapshot {
        hashes: BTreeMap::new(),
        complete: true,
    };
    let mut remaining_bytes = 16 * 1024 * 1024;
    let mut remaining_entries = 4096;
    walk(
        root,
        root,
        &mut result,
        &mut remaining_bytes,
        0,
        excluded,
        &mut remaining_entries,
    );
    result
}
fn walk(
    root: &Path,
    dir: &Path,
    result: &mut FileSnapshot,
    budget: &mut u64,
    depth: usize,
    excluded: &[PathBuf],
    entry_budget: &mut usize,
) {
    if depth > 24 {
        result.complete = false;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        result.complete = false;
        return;
    };
    let mut entries: Vec<_> = entries
        .take(*entry_budget + 1)
        .filter_map(|e| match e {
            Ok(e) => Some(e),
            Err(_) => {
                result.complete = false;
                None
            }
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if *entry_budget == 0 {
            result.complete = false;
            return;
        }
        *entry_budget -= 1;
        if excluded.contains(&entry.path()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if [
            ".git",
            "target",
            "node_modules",
            ".venv",
            "__pycache__",
            "dist",
        ]
        .contains(&name.as_ref())
        {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            result.complete = false;
            continue;
        };
        if meta.file_type().is_symlink() {
            result.complete = false;
            continue;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                result.complete = false;
                continue;
            }
        }
        if meta.is_dir() {
            walk(
                root,
                &entry.path(),
                result,
                budget,
                depth + 1,
                excluded,
                entry_budget,
            );
            continue;
        }
        if !meta.is_file() {
            result.complete = false;
            continue;
        }
        if result.hashes.len() >= 2048 || meta.len() > *budget {
            result.complete = false;
            continue;
        }
        // Hashes stay local. Only a freshness boolean is sent to Jev.
        let mut bytes = Vec::new();
        let Ok(file) = std::fs::File::open(entry.path()) else {
            result.complete = false;
            continue;
        };
        if file.take(*budget + 1).read_to_end(&mut bytes).is_err() {
            result.complete = false;
            continue;
        }
        let Ok(after) = std::fs::symlink_metadata(entry.path()) else {
            result.complete = false;
            continue;
        };
        if meta.len() != after.len()
            || meta.modified().ok() != after.modified().ok()
            || after.file_type().is_symlink()
        {
            result.complete = false;
            continue;
        }
        if bytes.len() as u64 > *budget {
            result.complete = false;
            continue;
        }
        *budget -= bytes.len() as u64;
        let hash = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        result.hashes.insert(path, hash);
    }
}

pub fn fresh(before: &FileSnapshot, after: &FileSnapshot, current: &FileSnapshot) -> bool {
    before.complete && after.complete && current.complete && before == after && after == current
}
