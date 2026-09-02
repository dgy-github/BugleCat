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

pub(crate) fn promote(
    baseline: &WorkspaceSnapshot,
    worker_root: &Path,
    live_root: &Path,
) -> Result<usize, String> {
    let worker = snapshot(worker_root).map_err(|error| format!("读取 Worker 结果失败：{error}"))?;
    let live = snapshot(live_root).map_err(|error| format!("读取当前工作区失败：{error}"))?;
    let changed = baseline
        .keys()
        .chain(worker.keys())
        .filter(|path| baseline.get(*path) != worker.get(*path))
        .cloned()
        .collect::<BTreeSet<_>>();

    let conflicts = changed
        .iter()
        .filter(|path| live.get(*path) != baseline.get(*path))
        .take(5)
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Err(format!(
            "工作区在 Worker 运行期间发生冲突，未提升任何文件：{}",
            conflicts.join("、")
        ));
    }

    for relative in &changed {
        let destination = live_root.join(relative);
        if worker.contains_key(relative) {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("创建目录 {} 失败：{error}", parent.display()))?;
            }
            std::fs::copy(worker_root.join(relative), &destination)
                .map_err(|error| format!("提升 {} 失败：{error}", relative.display()))?;
        } else if destination.exists() {
            std::fs::remove_file(&destination)
                .map_err(|error| format!("删除 {} 失败：{error}", relative.display()))?;
            remove_empty_parents(destination.parent(), live_root);
        }
    }
    Ok(changed.len())
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
}
