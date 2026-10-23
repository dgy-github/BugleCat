//! Three-way promotion of an isolated worker workspace into the live workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::isolate::is_skipped_dir;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileFingerprint {
    len: u64,
    sha256: [u8; 32],
}

pub(crate) type WorkspaceSnapshot = BTreeMap<PathBuf, FileFingerprint>;

pub(crate) fn snapshot(root: &Path) -> io::Result<WorkspaceSnapshot> {
    let mut result = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if !is_skipped_dir(&entry.file_name()) {
                    stack.push(entry.path());
                }
            } else if file_type.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_path_buf();
                result.insert(relative, fingerprint(&entry.path())?);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) fn promote(
    baseline: &WorkspaceSnapshot,
    worker_root: &Path,
    live_root: &Path,
) -> Result<usize, String> {
    promote_with_paths(baseline, worker_root, live_root).map(|paths| paths.len())
}

pub(crate) fn promote_with_paths(
    baseline: &WorkspaceSnapshot,
    worker_root: &Path,
    live_root: &Path,
) -> Result<Vec<String>, String> {
    compose_with_paths(&[(baseline, worker_root)], live_root)
}

/// Harness² Propose–Probe–Compose: compose non-conflicting file changes across
/// one or more isolated worker workspaces ordered by priority (`workers[0]` is
/// the primary `BEST` worker; subsequent workers contribute changes only on
/// paths untouched by earlier workers or with identical SHA-256 fingerprints).
pub(crate) fn compose_with_paths(
    workers: &[(&WorkspaceSnapshot, &Path)],
    live_root: &Path,
) -> Result<Vec<String>, String> {
    let Some(&(primary_baseline, _)) = workers.first() else {
        return Ok(Vec::new());
    };
    let live = snapshot(live_root).map_err(|error| format!("读取当前工作区失败：{error}"))?;
    let mut chosen_source: BTreeMap<PathBuf, Option<PathBuf>> = BTreeMap::new();
    let mut chosen_fp: BTreeMap<PathBuf, Option<FileFingerprint>> = BTreeMap::new();

    for &(baseline, worker_root) in workers {
        let worker =
            snapshot(worker_root).map_err(|error| format!("读取 Worker 结果失败：{error}"))?;
        let changed = baseline
            .keys()
            .chain(worker.keys())
            .filter(|path| baseline.get(*path) != worker.get(*path))
            .cloned()
            .collect::<BTreeSet<_>>();
        for relative in changed {
            let new_fp = worker.get(&relative).cloned();
            if let Some(existing_fp) = chosen_fp.get(&relative) {
                // Primary/earlier worker already modified this file; only keep if identical,
                // never overwrite the primary worker's version on overlapping files.
                let _ = existing_fp;
                continue;
            }
            let src = new_fp.as_ref().map(|_| worker_root.join(&relative));
            chosen_source.insert(relative.clone(), src);
            chosen_fp.insert(relative, new_fp);
        }
    }

    let conflicts = chosen_source
        .keys()
        .filter(|path| live.get(*path) != primary_baseline.get(*path))
        .take(5)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Err(format!(
            "工作区在 Worker 运行期间发生冲突，未提升任何文件：{}",
            conflicts.join("、")
        ));
    }

    let mut promoted_paths = Vec::with_capacity(chosen_source.len());
    for (relative, source_file) in &chosen_source {
        let destination = live_root.join(relative);
        if let Some(src) = source_file {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("创建目录 {} 失败：{error}", parent.display()))?;
            }
            std::fs::copy(src, &destination)
                .map_err(|error| format!("提升 {} 失败：{error}", relative.display()))?;
        } else if destination.exists() {
            std::fs::remove_file(&destination)
                .map_err(|error| format!("删除 {} 失败：{error}", relative.display()))?;
            remove_empty_parents(destination.parent(), live_root);
        }
        promoted_paths.push(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(promoted_paths)
}

fn fingerprint(path: &Path) -> io::Result<FileFingerprint> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(FileFingerprint {
        len,
        sha256: hasher.finalize().into(),
    })
}

fn remove_empty_parents(mut current: Option<&Path>, root: &Path) {
    while let Some(directory) = current {
        if directory == root || std::fs::remove_dir(directory).is_err() {
            break;
        }
        current = directory.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        crate::test_support::unique_temp_dir(&format!("ncx_promotion_{name}"))
    }

    fn setup(name: &str) -> (PathBuf, PathBuf, WorkspaceSnapshot) {
        let live = temp(&format!("{name}_live"));
        let worker = temp(&format!("{name}_worker"));
        let _ = std::fs::remove_dir_all(&live);
        let _ = std::fs::remove_dir_all(&worker);
        std::fs::create_dir_all(live.join("nested")).unwrap();
        std::fs::write(live.join("keep.txt"), "keep").unwrap();
        std::fs::write(live.join("nested/remove.txt"), "remove").unwrap();
        crate::isolate::copy_tree(&live, &worker).unwrap();
        let baseline = snapshot(&live).unwrap();
        (live, worker, baseline)
    }

    #[test]
    fn promotes_add_modify_and_delete() {
        let (live, worker, baseline) = setup("changes");
        std::fs::write(worker.join("keep.txt"), "changed").unwrap();
        std::fs::write(worker.join("added.txt"), "added").unwrap();
        std::fs::remove_file(worker.join("nested/remove.txt")).unwrap();
        assert_eq!(promote(&baseline, &worker, &live).unwrap(), 3);
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "changed"
        );
        assert_eq!(
            std::fs::read_to_string(live.join("added.txt")).unwrap(),
            "added"
        );
        assert!(!live.join("nested/remove.txt").exists());
        assert!(
            !live.join("nested").exists(),
            "empty directories are cleaned"
        );
    }

    #[test]
    fn conflict_fails_before_any_change_is_applied() {
        let (live, worker, baseline) = setup("conflict");
        std::fs::write(worker.join("keep.txt"), "worker").unwrap();
        std::fs::write(worker.join("added.txt"), "added").unwrap();
        std::fs::write(live.join("keep.txt"), "user").unwrap();
        let error = promote(&baseline, &worker, &live).unwrap_err();
        assert!(error.contains("冲突"), "{error}");
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "user"
        );
        assert!(!live.join("added.txt").exists());
    }

    #[test]
    fn compose_merges_disjoint_worker_changes_preserving_primary_on_overlap() {
        let (live, w1, baseline) = setup("compose");
        let w2 = temp("compose_worker2");
        let _ = std::fs::remove_dir_all(&w2);
        crate::isolate::copy_tree(&live, &w2).unwrap();

        // Worker 1 (primary) edits keep.txt and adds w1_only.py
        std::fs::write(w1.join("keep.txt"), "from_w1").unwrap();
        std::fs::write(w1.join("w1_only.py"), "def a(): pass").unwrap();

        // Worker 2 (secondary) also edits keep.txt (overlap -> ignored in favor of w1)
        // and adds tests/test_a.py (disjoint -> composed!)
        std::fs::write(w2.join("keep.txt"), "from_w2").unwrap();
        std::fs::write(w2.join("test_a.py"), "def test_a(): pass").unwrap();

        let paths = compose_with_paths(&[(&baseline, &w1), (&baseline, &w2)], &live).unwrap();
        assert_eq!(paths.len(), 3);
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "from_w1"
        );
        assert_eq!(
            std::fs::read_to_string(live.join("w1_only.py")).unwrap(),
            "def a(): pass"
        );
        assert_eq!(
            std::fs::read_to_string(live.join("test_a.py")).unwrap(),
            "def test_a(): pass"
        );
    }
}
