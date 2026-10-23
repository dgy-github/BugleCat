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

#[cfg(test)]
pub(crate) fn promote_with_paths(
    baseline: &WorkspaceSnapshot,
    worker_root: &Path,
    live_root: &Path,
) -> Result<Vec<String>, String> {
    compose_with_paths(&[(baseline, worker_root)], live_root)
}

/// Harness² Propose–Probe–Compose: compose non-conflicting file changes across
/// one or more isolated worker workspaces ordered by priority (`workers[0]` is
/// the primary `BEST` worker).
///
/// Secondary workers are composed **atomically per worker**: if any file a
/// secondary worker changed was also changed by an earlier (accepted) worker
/// with *different* content, the whole secondary worker is dropped so no
/// half-applied patch reaches the live workspace. Identical overlapping edits
/// (same SHA-256 fingerprint) are fine. Every promoted path is checked against
/// the baseline of the worker that supplied it, so a live workspace that
/// drifted from *any* contributing worker's baseline aborts the whole promotion.
#[cfg(test)]
pub(crate) fn compose_with_paths(
    workers: &[(&WorkspaceSnapshot, &Path)],
    live_root: &Path,
) -> Result<Vec<String>, String> {
    compose_with_report(workers, live_root).map(|report| report.promoted)
}

/// Result of [`compose_with_report`].
#[derive(Debug, Default)]
pub(crate) struct ComposeReport {
    pub promoted: Vec<String>,
    /// Indices (into `workers`) of secondary workers dropped due to
    /// overlapping edits whose content differs from an earlier worker.
    pub dropped_workers: Vec<usize>,
}

pub(crate) fn compose_with_report(
    workers: &[(&WorkspaceSnapshot, &Path)],
    live_root: &Path,
) -> Result<ComposeReport, String> {
    if workers.is_empty() {
        return Ok(ComposeReport::default());
    }
    let live = snapshot(live_root).map_err(|error| format!("读取当前工作区失败：{error}"))?;
    // path -> (source file or None for delete, new fingerprint, baseline fingerprint)
    type Chosen = (
        Option<PathBuf>,
        Option<FileFingerprint>,
        Option<FileFingerprint>,
    );
    let mut chosen: BTreeMap<PathBuf, Chosen> = BTreeMap::new();
    let mut dropped_workers = Vec::new();

    for (index, &(baseline, worker_root)) in workers.iter().enumerate() {
        let worker =
            snapshot(worker_root).map_err(|error| format!("读取 Worker 结果失败：{error}"))?;
        let changed = baseline
            .keys()
            .chain(worker.keys())
            .filter(|path| baseline.get(*path) != worker.get(*path))
            .cloned()
            .collect::<BTreeSet<_>>();
        let conflicting = changed.iter().any(|relative| {
            chosen
                .get(relative)
                .is_some_and(|(_, existing_fp, _)| existing_fp.as_ref() != worker.get(relative))
        });
        if conflicting {
            // Never half-promote a secondary worker (index 0 never conflicts: `chosen` is empty).
            dropped_workers.push(index);
            continue;
        }
        for relative in changed {
            if chosen.contains_key(&relative) {
                continue; // identical edit already chosen
            }
            let new_fp = worker.get(&relative).cloned();
            let src = new_fp.as_ref().map(|_| worker_root.join(&relative));
            let base_fp = baseline.get(&relative).cloned();
            chosen.insert(relative, (src, new_fp, base_fp));
        }
    }

    let conflicts = chosen
        .iter()
        .filter(|(path, (_, _, base_fp))| live.get(*path) != base_fp.as_ref())
        .take(5)
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Err(format!(
            "工作区在 Worker 运行期间发生冲突，未提升任何文件：{}",
            conflicts.join("、")
        ));
    }

    let mut promoted_paths = Vec::with_capacity(chosen.len());
    for (relative, (source_file, _, _)) in &chosen {
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
    Ok(ComposeReport {
        promoted: promoted_paths,
        dropped_workers,
    })
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
    fn compose_merges_disjoint_worker_changes() {
        let (live, w1, baseline) = setup("compose");
        let w2 = temp("compose_worker2");
        let _ = std::fs::remove_dir_all(&w2);
        crate::isolate::copy_tree(&live, &w2).unwrap();
        std::fs::write(w1.join("keep.txt"), "from_w1").unwrap();
        std::fs::write(w1.join("w1_only.py"), "def a(): pass").unwrap();
        // Identical overlapping edit is allowed; disjoint addition is composed.
        std::fs::write(w2.join("keep.txt"), "from_w1").unwrap();
        std::fs::write(w2.join("test_a.py"), "def test_a(): pass").unwrap();

        let report = compose_with_report(&[(&baseline, &w1), (&baseline, &w2)], &live).unwrap();
        assert!(report.dropped_workers.is_empty());
        assert_eq!(report.promoted.len(), 3);
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "from_w1"
        );
        assert_eq!(
            std::fs::read_to_string(live.join("test_a.py")).unwrap(),
            "def test_a(): pass"
        );
    }

    #[test]
    fn compose_drops_whole_secondary_worker_on_overlapping_different_content() {
        let (live, w1, baseline) = setup("compose_overlap");
        let w2 = temp("compose_overlap_worker2");
        let _ = std::fs::remove_dir_all(&w2);
        crate::isolate::copy_tree(&live, &w2).unwrap();
        std::fs::write(w1.join("keep.txt"), "from_w1").unwrap();
        std::fs::write(w1.join("w1_only.py"), "def a(): pass").unwrap();
        std::fs::write(w2.join("keep.txt"), "from_w2").unwrap();
        std::fs::write(w2.join("test_a.py"), "def test_a(): pass").unwrap();

        let report = compose_with_report(&[(&baseline, &w1), (&baseline, &w2)], &live).unwrap();
        assert_eq!(report.dropped_workers, vec![1]);
        assert_eq!(report.promoted.len(), 2);
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "from_w1"
        );
        assert!(
            !live.join("test_a.py").exists(),
            "conflicting secondary worker must not be half-promoted"
        );
    }

    #[test]
    fn compose_checks_live_against_each_contributing_worker_baseline() {
        let (live, w1, baseline1) = setup("compose_base");
        std::fs::write(w1.join("keep.txt"), "from_w1").unwrap();
        // Worker 2 started from a different baseline of `nested/remove.txt`.
        let w2 = temp("compose_base_worker2");
        let _ = std::fs::remove_dir_all(&w2);
        crate::isolate::copy_tree(&live, &w2).unwrap();
        std::fs::write(w2.join("nested/remove.txt"), "w2_base").unwrap();
        let baseline2 = snapshot(&w2).unwrap();
        std::fs::write(w2.join("nested/remove.txt"), "w2_edit").unwrap();

        let error = compose_with_paths(&[(&baseline1, &w1), (&baseline2, &w2)], &live).unwrap_err();
        assert!(error.contains("冲突"), "{error}");
        assert_eq!(
            std::fs::read_to_string(live.join("keep.txt")).unwrap(),
            "keep"
        );
    }
}
