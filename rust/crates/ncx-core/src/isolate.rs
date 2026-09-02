//! Workspace isolation for parallel sub-agent workers.
//!
//! The orchestrator fans out N workers in parallel; if they all write to the
//! same workspace they collide (observed: two `apply_patch`es racing to Add the
//! same file). The fix: only the primary worker (index 0, whose result is the
//! synthesized answer) writes to the real workspace; the other "exploratory"
//! workers run against a throwaway COPY, so their writes can't corrupt it. Their
//! text results still feed the verifier.
//!
//! [`copy_tree`] makes that copy, skipping VCS/build/dep dirs so it's cheap.

use std::path::Path;

/// Dirs never copied into an isolated workspace (huge / generated / irrelevant).
pub(crate) const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".ncx",
    "dist",
    ".venv",
    "__pycache__",
];

pub(crate) fn is_skipped_dir(name: &std::ffi::OsStr) -> bool {
    SKIP_DIRS.contains(&name.to_string_lossy().as_ref())
}

/// Recursively copy `src` into `dst` (created if absent), skipping [`SKIP_DIRS`].
/// Returns the number of files copied.
pub fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut copied = 0;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        for entry in std::fs::read_dir(&s)?.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            let name = entry.file_name();
            let from = entry.path();
            let to = d.join(&name);
            if ft.is_dir() {
                if is_skipped_dir(&name) {
                    continue;
                }
                std::fs::create_dir_all(&to)?;
                stack.push((from, to));
            } else if ft.is_file() {
                std::fs::copy(&from, &to)?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = crate::test_support::unique_temp_dir(&format!("ncx_isolate_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn copies_files_and_skips_ignored() {
        let src = tmp("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::create_dir_all(src.join("target")).unwrap();
        std::fs::write(src.join("a.txt"), "A").unwrap();
        std::fs::write(src.join("sub/b.txt"), "B").unwrap();
        std::fs::write(src.join("target/junk.o"), "junk").unwrap();

        let dst = tmp("dst");
        let n = copy_tree(&src, &dst).unwrap();
        assert_eq!(n, 2, "a.txt + sub/b.txt; target/ skipped");
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "A");
        assert_eq!(std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "B");
        assert!(
            !dst.join("target").exists(),
            "ignored dir must not be copied"
        );
    }

    #[test]
    fn isolated_copy_is_independent() {
        let src = tmp("src2");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("f.txt"), "orig").unwrap();
        let dst = tmp("dst2");
        copy_tree(&src, &dst).unwrap();
        // Mutating the copy must not touch the source.
        std::fs::write(dst.join("f.txt"), "changed").unwrap();
        assert_eq!(std::fs::read_to_string(src.join("f.txt")).unwrap(), "orig");
    }
}
