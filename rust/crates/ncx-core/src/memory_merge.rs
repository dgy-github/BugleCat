use std::io::{Error, ErrorKind};

use crate::memory::{jaccard, parse_entries, word_set};
use crate::{MemoryEntry, MemoryStore, Summarizer};

/// A conflict-safe LLM merge result. Preparing it never changes project files;
/// commit succeeds only while the source bytes still match the captured base.
#[derive(Debug)]
pub struct MemoryMergeDraft {
    baseline: Vec<u8>,
    entries: Vec<MemoryEntry>,
    pub removed: usize,
}

impl MemoryStore {
    /// Compute an LLM merge without changing the real memory file.
    pub async fn prepare_summarize_consolidate(
        &self,
        summarizer: &dyn Summarizer,
        threshold: f64,
    ) -> std::io::Result<MemoryMergeDraft> {
        self.prepare_summarize_consolidate_cancellable(summarizer, threshold, || false)
            .await
    }

    /// Prepare a merge while checking cancellation between model calls. A
    /// cancelled preparation returns `Interrupted` and has no file side effect.
    pub async fn prepare_summarize_consolidate_cancellable(
        &self,
        summarizer: &dyn Summarizer,
        threshold: f64,
        cancelled: impl Fn() -> bool,
    ) -> std::io::Result<MemoryMergeDraft> {
        let baseline = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        let entries = parse_entries(&String::from_utf8_lossy(&baseline));
        let before = entries.len();
        let entries = merge_entries(entries, summarizer, threshold, &cancelled).await?;
        Ok(MemoryMergeDraft {
            baseline,
            removed: before.saturating_sub(entries.len()),
            entries,
        })
    }

    /// Promote a prepared merge if no user or other process changed memory.
    pub fn commit_summarize_consolidate(&self, draft: MemoryMergeDraft) -> std::io::Result<usize> {
        if draft.removed == 0 {
            return Ok(0);
        }
        let current = std::fs::read(&self.path).unwrap_or_default();
        if current != draft.baseline {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                "project memory changed while the LLM merge was running",
            ));
        }
        self.write_all(&draft.entries)?;
        Ok(draft.removed)
    }

    /// Compatibility path for hosts that intentionally await the whole merge.
    pub async fn summarize_consolidate(
        &self,
        summarizer: &dyn Summarizer,
        threshold: f64,
    ) -> std::io::Result<usize> {
        let draft = self
            .prepare_summarize_consolidate(summarizer, threshold)
            .await?;
        self.commit_summarize_consolidate(draft)
    }
}

async fn merge_entries(
    mut entries: Vec<MemoryEntry>,
    summarizer: &dyn Summarizer,
    threshold: f64,
    cancelled: &dyn Fn() -> bool,
) -> std::io::Result<Vec<MemoryEntry>> {
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.ts));
    let mut clusters: Vec<Vec<MemoryEntry>> = Vec::new();
    let mut reps = Vec::new();
    for entry in entries {
        let words = word_set(&entry.text);
        if let Some(index) = reps
            .iter()
            .position(|rep| jaccard(rep, &words) >= threshold)
        {
            clusters[index].push(entry);
        } else {
            reps.push(words);
            clusters.push(vec![entry]);
        }
    }

    let mut merged = Vec::new();
    for cluster in clusters {
        if cancelled() {
            return Err(Error::new(ErrorKind::Interrupted, "memory merge cancelled"));
        }
        merged.push(merge_cluster(cluster, summarizer).await);
        if cancelled() {
            return Err(Error::new(ErrorKind::Interrupted, "memory merge cancelled"));
        }
    }
    merged.sort_by_key(|entry| entry.ts);
    if merged.len() > crate::memory::MAX_ENTRIES {
        merged.drain(0..merged.len() - crate::memory::MAX_ENTRIES);
    }
    Ok(merged)
}

async fn merge_cluster(cluster: Vec<MemoryEntry>, summarizer: &dyn Summarizer) -> MemoryEntry {
    if cluster.len() == 1 {
        return cluster.into_iter().next().unwrap();
    }
    let newest = cluster.iter().max_by_key(|entry| entry.ts).unwrap();
    let mut tags = Vec::new();
    for tag in cluster.iter().flat_map(|entry| &entry.tags) {
        if !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    let facts = cluster
        .iter()
        .map(|entry| entry.text.clone())
        .collect::<Vec<_>>();
    match summarizer.merge(&facts).await {
        Some(text) if !text.trim().is_empty() => MemoryEntry {
            ts: newest.ts,
            tags,
            text: text.trim().to_string(),
        },
        _ => newest.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;

    use super::*;

    struct FixedMerger;

    #[async_trait(?Send)]
    impl Summarizer for FixedMerger {
        async fn merge(&self, _facts: &[String]) -> Option<String> {
            Some("merged fact".into())
        }
    }

    fn store(name: &str) -> MemoryStore {
        let root = crate::test_support::unique_temp_dir(&format!("ncx-memory-draft-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        MemoryStore::new(root)
    }

    #[tokio::test]
    async fn draft_does_not_write_until_commit() {
        let store = store("commit");
        store.remember("alpha beta gamma delta", &[], 1).unwrap();
        store
            .remember("alpha beta gamma delta epsilon", &[], 2)
            .unwrap();
        let before = std::fs::read(&store.path).unwrap();
        let draft = store
            .prepare_summarize_consolidate(&FixedMerger, 0.8)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
        assert_eq!(store.commit_summarize_consolidate(draft).unwrap(), 1);
        assert_eq!(store.entries()[0].text, "merged fact");
    }

    #[tokio::test]
    async fn concurrent_change_rejects_the_whole_draft() {
        let store = store("conflict");
        store.remember("alpha beta gamma delta", &[], 1).unwrap();
        store
            .remember("alpha beta gamma delta epsilon", &[], 2)
            .unwrap();
        let draft = store
            .prepare_summarize_consolidate(&FixedMerger, 0.8)
            .await
            .unwrap();
        store
            .remember("user added this while merge ran", &[], 3)
            .unwrap();
        let changed = std::fs::read(&store.path).unwrap();
        let error = store.commit_summarize_consolidate(draft).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::WouldBlock);
        assert_eq!(std::fs::read(&store.path).unwrap(), changed);
    }

    #[tokio::test]
    async fn cancellation_discards_the_prepared_result() {
        let store = store("cancel");
        store.remember("alpha beta gamma delta", &[], 1).unwrap();
        store
            .remember("alpha beta gamma delta epsilon", &[], 2)
            .unwrap();
        let before = std::fs::read(&store.path).unwrap();
        let cancelled = AtomicBool::new(true);
        let error = store
            .prepare_summarize_consolidate_cancellable(&FixedMerger, 0.8, || {
                cancelled.load(Ordering::SeqCst)
            })
            .await
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Interrupted);
        assert_eq!(std::fs::read(&store.path).unwrap(), before);
    }
}
