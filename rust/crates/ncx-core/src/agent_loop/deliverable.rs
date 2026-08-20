use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::search::walk_files;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub(super) struct DeliverableRequirement {
    baseline: HashMap<PathBuf, Fingerprint>,
}

impl DeliverableRequirement {
    pub(super) fn detect(query: &str, workspace: &Path) -> Option<Self> {
        let lower = query.to_ascii_lowercase();
        let mentions_pdf = lower.contains("pdf");
        let requests_creation = [
            "生成",
            "制作",
            "整理成",
            "转换",
            "导出",
            "创建",
            "写成",
            "generate",
            "create",
            "convert",
            "export",
            "produce",
            "make",
        ]
        .iter()
        .any(|verb| lower.contains(verb));
        if !(mentions_pdf && requests_creation) {
            return None;
        }
        Some(Self {
            baseline: pdf_fingerprints(workspace),
        })
    }

    pub(super) fn completed_path(&self, workspace: &Path) -> Option<PathBuf> {
        pdf_fingerprints(workspace)
            .into_iter()
            .find(|(path, current)| {
                self.baseline.get(path) != Some(current) && has_pdf_signature(path)
            })
            .map(|(path, _)| path)
    }
}

fn pdf_fingerprints(workspace: &Path) -> HashMap<PathBuf, Fingerprint> {
    walk_files(workspace)
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        })
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            Some((
                path,
                Fingerprint {
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            ))
        })
        .collect()
}

fn has_pdf_signature(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0u8; 5];
    if file.read_exact(&mut header).is_err() || header != *b"%PDF-" {
        return false;
    }
    let Ok(len) = file.metadata().map(|metadata| metadata.len()) else {
        return false;
    };
    let tail_start = len.saturating_sub(1_024);
    if file.seek(SeekFrom::Start(tail_start)).is_err() {
        return false;
    }
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).is_ok() && tail.windows(5).any(|window| window == b"%%EOF")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_pdf_container_is_not_a_valid_deliverable() {
        let root = std::env::temp_dir().join(format!(
            "ncx_pdf_container_{}_{}",
            std::process::id(),
            crate::session_index::new_session_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let incomplete = root.join("incomplete.pdf");
        let complete = root.join("complete.pdf");
        std::fs::write(&incomplete, b"%PDF-1.4\nunfinished").unwrap();
        std::fs::write(&complete, b"%PDF-1.4\n/Type /Page\n%%EOF\n").unwrap();

        assert!(!has_pdf_signature(&incomplete));
        assert!(has_pdf_signature(&complete));

        let _ = std::fs::remove_dir_all(root);
    }
}
