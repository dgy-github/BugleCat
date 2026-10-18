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

    /// Items awaiting human review: unconfirmed experience candidates and
    /// notes demoted by outcome feedback. Nothing here is recalled until a
    /// human confirms (candidate) or restores (demoted) it.
    pub fn review_items(&self) -> Vec<MemoryReviewItem> {
        let Ok(index) = crate::memory_index::MemoryIndex::for_store(self) else {
            return Vec::new();
        };
        let mut items: Vec<MemoryReviewItem> = index
            .candidate_notes(&self.workspace_key())
            .into_iter()
            .map(|note| MemoryReviewItem {
                kind: MemoryReviewKind::Candidate,
                id: note.note_id,
                text: note.text,
                detail: if note.occurrences > 1 {
                    format!(
                        "命中 {} 次 · evidence={} source={}",
                        note.occurrences, note.evidence_kind, note.source_ref
                    )
                } else {
                    format!("evidence={} source={}", note.evidence_kind, note.source_ref)
                },
            })
            .collect();
        let entries = self.entries();
        for stat in index.demoted_feedback() {
            let text = entries
                .iter()
                .find(|entry| MemoryStore::note_id(&entry.text) == stat.note_id)
                .map(|entry| entry.text.clone())
                .unwrap_or_else(|| "(note no longer in LEARNINGS.md)".to_string());
            items.push(MemoryReviewItem {
                kind: MemoryReviewKind::Demoted,
                detail: format!(
                    "injected={} accepted={} rejected={} p={:.2}",
                    stat.injected,
                    stat.accepted,
                    stat.rejected,
                    stat.beta_posterior()
                ),
                id: stat.note_id,
                text,
            });
        }
        items
    }

    /// Human confirmation of one candidate: write it to `LEARNINGS.md` and mark
    /// the index row active. Returns `false` when no such candidate exists.
    pub fn confirm_candidate(&self, note_id: &str) -> std::io::Result<bool> {
        let index = crate::memory_index::MemoryIndex::for_store(self).map_err(Error::other)?;
        let Some(candidate) = index
            .candidate_notes(&self.workspace_key())
            .into_iter()
            .find(|note| note.note_id == note_id)
        else {
            return Ok(false);
        };
        self.remember(&candidate.text, &candidate.tags, candidate.created_at)?;
        index.confirm_candidate(note_id).map_err(Error::other)
    }

    /// Human rejection of one candidate: the row is deleted.
    pub fn reject_candidate(&self, note_id: &str) -> std::io::Result<bool> {
        crate::memory_index::MemoryIndex::for_store(self)
            .and_then(|index| index.reject_candidate(note_id))
            .map_err(Error::other)
    }

    /// Clear the demotion flag of one note so it is recalled again.
    pub fn restore_demoted(&self, note_id: &str) -> std::io::Result<bool> {
        crate::memory_index::MemoryIndex::for_store(self)
            .and_then(|index| index.set_note_demoted(note_id, false))
            .map_err(Error::other)
    }

    /// Text front-end shared by `ncx --memory-review` and the GUI:
    /// `list` (default) | `confirm <id>` | `reject <id>` | `restore <id>`.
    pub fn review_command(&self, arg: &str) -> String {
        let mut parts = arg.split_whitespace();
        let action = parts.next().unwrap_or("list").to_ascii_lowercase();
        let id = parts.next().unwrap_or("");
        let needs_id = |verb: &str| format!("usage: {verb} <id>  (ids come from `list`)");
        let result = match action.as_str() {
            "list" | "" => return format_review_items(&self.review_items()),
            "json" => {
                return serde_json::to_string(&self.review_items())
                    .unwrap_or_else(|_| "[]".to_string())
            }
            "confirm" if id.is_empty() => return needs_id("confirm"),
            "reject" if id.is_empty() => return needs_id("reject"),
            "restore" if id.is_empty() => return needs_id("restore"),
            "confirm" => self
                .confirm_candidate(id)
                .map(|ok| (ok, "confirmed candidate")),
            "reject" => self
                .reject_candidate(id)
                .map(|ok| (ok, "rejected candidate")),
            "restore" => self
                .restore_demoted(id)
                .map(|ok| (ok, "restored demoted note")),
            _ => return "usage: list | confirm <id> | reject <id> | restore <id>".to_string(),
        };
        match result {
            Ok((true, what)) => format!("{what} {id}"),
            Ok((false, _)) => format!("no reviewable memory item with id {id}"),
            Err(error) => format!("memory review failed: {error}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryReviewKind {
    Candidate,
    Demoted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryReviewItem {
    pub kind: MemoryReviewKind,
    pub id: String,
    pub text: String,
    pub detail: String,
}

fn format_review_items(items: &[MemoryReviewItem]) -> String {
    if items.is_empty() {
        return "memory review: nothing pending (no candidates, no demoted notes).".to_string();
    }
    let mut out = vec![format!("memory review: {} item(s)", items.len())];
    for item in items {
        let kind = match item.kind {
            MemoryReviewKind::Candidate => "candidate",
            MemoryReviewKind::Demoted => "demoted",
        };
        out.push(format!(
            "- [{kind}] {}\n    {}\n    {}",
            item.id, item.text, item.detail
        ));
    }
    out.push(
        "actions: confirm <id> | reject <id> (candidates), restore <id> (demoted)".to_string(),
    );
    out.join("\n")
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
