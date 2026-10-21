//! Project memory — a tiny "self-evolution" store so the agent gets more
//! project-fit with use. NOT raised IQ: it's accumulated, verified experience
//! (conventions, gotchas, solutions) recalled into the prompt as *leads*.
//!
//! Stored as one human-readable markdown file `.ncx/memory/LEARNINGS.md`, one
//! entry per verified note with a parseable comment header:
//!
//! ```text
//! <!-- ts:1719300000 tags:build,windows -->
//! The GNU linker overflows on cdylib; use crate-type=["lib"].
//! ```
//!
//! On write: deduplicate (normalized text) and cap to the newest [`MAX_ENTRIES`].
//! On recall: score by a lightweight semantic lexical ranker (keywords, tags,
//! phrases, Jaccard, and a tiny domain synonym map), tie-break by recency, and
//! return a capped block to prepend to the system prompt.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

/// Merges several same-topic facts into one concise note (the LLM-backed
/// consolidation). `None` = couldn't summarize → caller keeps the newest.
/// Supplied by the CLI/GUI (uses the fast model); tests use a mock.
#[async_trait(?Send)]
pub trait Summarizer {
    async fn merge(&self, facts: &[String]) -> Option<String>;
}

/// Hard cap so the store can't grow unbounded; oldest are dropped first.
pub const MAX_ENTRIES: usize = 200;
const RECALL_HEADER: &str =
    "Project memory (verified notes from past work — treat as leads, verify before acting):";

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    /// UNIX epoch seconds when recorded (recency).
    pub ts: u64,
    pub tags: Vec<String>,
    pub text: String,
}

/// Append-only-ish markdown fact store under a project's `.ncx/memory/`.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    pub(crate) path: PathBuf,
    pub(crate) feedback_mode: String,
}

/// Result of one prompt-scoped recall.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryRecall {
    /// Recall block for the prompt; empty when nothing was selected.
    pub text: String,
    /// `note_id`s of the injected notes, for outcome attribution.
    pub note_ids: Vec<String>,
    /// Whether plain and feedback-weighted ranking select different notes
    /// (`None` when feedback is off or nothing was selectable).
    pub rank_changed: Option<bool>,
}

fn select_recall(
    scored: Vec<(i64, String, &MemoryEntry)>,
    max_entries: usize,
    max_chars: usize,
) -> (String, Vec<String>) {
    let mut out = String::from(RECALL_HEADER);
    let mut used = out.len();
    let mut ids = Vec::new();
    for (_, nid, e) in scored {
        if ids.len() >= max_entries {
            break;
        }
        let line = format!("\n- {}", e.text.replace('\n', " "));
        if used + line.len() > max_chars {
            break;
        }
        out.push_str(&line);
        used += line.len();
        ids.push(nid);
    }
    if ids.is_empty() {
        (String::new(), Vec::new())
    } else {
        (out, ids)
    }
}

impl MemoryStore {
    /// `dir` is the `.ncx/memory` directory (created on first write).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        MemoryStore {
            path: dir.into().join("LEARNINGS.md"),
            feedback_mode: "shadow".to_string(),
        }
    }

    /// Set online memory feedback mode (`"off" | "shadow" | "on"`).
    pub fn set_feedback_mode(&mut self, mode: impl Into<String>) {
        self.feedback_mode = mode.into();
    }

    pub fn feedback_mode(&self) -> &str {
        &self.feedback_mode
    }

    /// Deterministic note identifier (`content_hash(&normalize(text))`).
    pub fn note_id(text: &str) -> String {
        crate::context_trace::content_hash(&normalize(text))
    }

    /// Workspace key associated with this memory store directory.
    pub fn workspace_key(&self) -> String {
        self.path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."))
            .display()
            .to_string()
    }

    /// Record a verified note. Returns `Ok(false)` if it duplicates an existing
    /// entry (skipped). `now` is the epoch-seconds timestamp (caller supplies it
    /// so this is deterministic / testable).
    pub fn remember(&self, text: &str, tags: &[String], now: u64) -> std::io::Result<bool> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(false);
        }
        let mut entries = self.entries();
        let norm = normalize(text);
        if entries.iter().any(|e| normalize(&e.text) == norm) {
            return Ok(false); // dedup
        }
        entries.push(MemoryEntry {
            ts: now,
            tags: tags.to_vec(),
            text: text.to_string(),
        });
        // Cap: keep the newest MAX_ENTRIES (stable by ts ascending, drop front).
        entries.sort_by_key(|e| e.ts);
        if entries.len() > MAX_ENTRIES {
            let drop = entries.len() - MAX_ENTRIES;
            entries.drain(0..drop);
        }
        self.write_all(&entries)?;
        Ok(true)
    }

    /// Remove any entry matching `text` (after normalization) from the Markdown store.
    pub fn forget_text(&self, text: &str) -> std::io::Result<bool> {
        let norm = normalize(text.trim());
        if norm.is_empty() {
            return Ok(false);
        }
        let mut entries = self.entries();
        let before = entries.len();
        entries.retain(|e| normalize(&e.text) != norm);
        if entries.len() == before {
            return Ok(false);
        }
        self.write_all(&entries)?;
        Ok(true)
    }

    /// Path of the Markdown file this store owns. The metadata index is built
    /// from it and lives beside it.
    pub fn markdown_path(&self) -> &Path {
        &self.path
    }

    /// Parse all stored entries (empty if the file is absent / unreadable).
    pub fn entries(&self) -> Vec<MemoryEntry> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        parse_entries(&text)
    }

    /// Build a recall block for the system prompt along with the recalled `note_id`s
    /// for outcome attribution.
    ///
    /// - `off`: plain lexical + recency ranking, no feedback lookup.
    /// - `shadow`: plain ranking is what gets injected; the feedback-weighted
    ///   ranking is computed only to report whether it would have differed.
    /// - `on`: notes flagged demoted are withheld and the rest get a bounded
    ///   Beta-posterior multiplier (`[0.8, 1.2]`).
    pub fn recall_with_feedback(
        &self,
        query: &str,
        max_entries: usize,
        max_chars: usize,
    ) -> MemoryRecall {
        let entries = self.entries();
        if entries.is_empty() {
            return MemoryRecall::default();
        }
        let feedback_map = if self.feedback_mode == "off" {
            std::collections::HashMap::new()
        } else {
            crate::memory_index::MemoryIndex::for_store(self)
                .ok()
                .map(|idx| idx.feedback_stats_for_workspace(&self.workspace_key()))
                .unwrap_or_default()
        };

        let qwords = expanded_keywords(query);
        let qset = word_set(query);
        let qphrases = phrases(query);
        let mut plain: Vec<(i64, String, &MemoryEntry)> = Vec::new();
        let mut weighted: Vec<(i64, String, &MemoryEntry)> = Vec::new();
        for e in &entries {
            let nid = Self::note_id(&e.text);
            let overlap = semantic_score(e, &qwords, &qset, &qphrases);
            // Pack recency into the low bits so higher ts wins ties.
            let recency = e.ts.min(999_999) as i64;
            plain.push((overlap * 1_000_000 + recency, nid.clone(), e));
            if self.feedback_mode == "off" || (self.feedback_mode == "on" && overlap < 8) {
                continue;
            }
            let stat = feedback_map.get(&nid);
            if stat.is_some_and(|s| s.demoted) {
                continue;
            }
            let multiplier = stat.map_or(1.0, |s| s.bounded_multiplier());
            let score = (overlap as f64 * multiplier * 100.0).round() as i64;
            weighted.push((score * 1_000_000 + recency, nid, e));
        }
        plain.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        weighted.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        let (plain_text, plain_ids) = select_recall(plain, max_entries, max_chars);
        if self.feedback_mode == "off" {
            return MemoryRecall {
                text: plain_text,
                note_ids: plain_ids,
                rank_changed: None,
            };
        }
        let (weighted_text, weighted_ids) = select_recall(weighted, max_entries, max_chars);
        let rank_changed =
            (!plain_ids.is_empty() || !weighted_ids.is_empty()).then(|| plain_ids != weighted_ids);
        if self.feedback_mode == "on" {
            MemoryRecall {
                text: weighted_text,
                note_ids: weighted_ids,
                rank_changed,
            }
        } else {
            MemoryRecall {
                text: plain_text,
                note_ids: plain_ids,
                rank_changed,
            }
        }
    }

    /// Build a recall block for the system prompt: entries scored by keyword
    /// overlap with `query` (tie-break recency), capped by count and chars.
    /// Empty string when nothing is stored.
    pub fn recall(&self, query: &str, max_entries: usize, max_chars: usize) -> String {
        self.recall_with_feedback(query, max_entries, max_chars)
            .text
    }

    /// Periodic consolidation: drop NEAR-duplicates (not just exact), keeping
    /// the newest of each similar cluster, then re-cap. Idempotent (running it
    /// again removes nothing). Returns how many entries were removed. Cheap to
    /// call on every session start.
    pub fn consolidate(&self, similarity_threshold: f64) -> std::io::Result<usize> {
        let mut entries = self.entries();
        if entries.len() < 2 {
            return Ok(0);
        }
        // Newest first, so the kept representative of a cluster is the latest.
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.ts));
        let mut kept: Vec<(MemoryEntry, std::collections::HashSet<String>)> = Vec::new();
        let mut removed = 0usize;
        for e in entries {
            let ws = word_set(&e.text);
            let dup = kept
                .iter()
                .any(|(_, kws)| jaccard(kws, &ws) >= similarity_threshold);
            if dup {
                removed += 1;
            } else {
                kept.push((e, ws));
            }
        }
        let mut out: Vec<MemoryEntry> = kept.into_iter().map(|(e, _)| e).collect();
        out.sort_by_key(|e| e.ts);
        if out.len() > MAX_ENTRIES {
            let drop = out.len() - MAX_ENTRIES;
            out.drain(0..drop);
            removed += drop;
        }
        if removed > 0 {
            self.write_all(&out)?;
        }
        Ok(removed)
    }

    pub(crate) fn write_all(&self, entries: &[MemoryEntry]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut s = String::from("# Project memory (nanocodex)\n\n");
        for e in entries {
            s.push_str(&format!(
                "<!-- ts:{} tags:{} -->\n{}\n\n",
                e.ts,
                e.tags.join(","),
                e.text
            ));
        }
        std::fs::write(&self.path, s)
    }
}

fn is_cjk_char(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x30FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
    )
}

/// Extract lexical tokens from mixed ASCII/CJK text:
/// - ASCII/Latin alphanumeric runs with length >= 3
/// - Contiguous CJK runs decomposed into character bigrams (2-grams) so
///   unsegmented Chinese/Japanese text has meaningful Jaccard and keyword overlap.
fn tokenize_mixed(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push_unique = |tok: String| {
        if !out.contains(&tok) {
            out.push(tok);
        }
    };
    for raw in s.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        if !raw.chars().any(is_cjk_char) {
            if raw.len() >= 3 {
                push_unique(raw.to_string());
            }
            continue;
        }
        let mut ascii_buf = String::new();
        let mut cjk_buf: Vec<char> = Vec::new();
        let flush_ascii = |buf: &mut String, push: &mut dyn FnMut(String)| {
            if buf.len() >= 3 {
                push(std::mem::take(buf));
            } else {
                buf.clear();
            }
        };
        let flush_cjk = |buf: &mut Vec<char>, push: &mut dyn FnMut(String)| {
            if buf.len() == 1 {
                buf.clear();
            } else if buf.len() == 2 {
                push(buf.iter().collect());
                buf.clear();
            } else if buf.len() >= 3 {
                for pair in buf.windows(2) {
                    push(pair.iter().collect());
                }
                buf.clear();
            }
        };
        for ch in raw.chars() {
            if is_cjk_char(ch) {
                flush_ascii(&mut ascii_buf, &mut push_unique);
                cjk_buf.push(ch);
            } else {
                flush_cjk(&mut cjk_buf, &mut push_unique);
                ascii_buf.push(ch);
            }
        }
        flush_ascii(&mut ascii_buf, &mut push_unique);
        flush_cjk(&mut cjk_buf, &mut push_unique);
    }
    out
}

/// Words worth matching on: lowercased, length ≥ 3 (plus CJK bigrams), deduped.
fn keywords(s: &str) -> Vec<String> {
    tokenize_mixed(s)
}

fn expanded_keywords(s: &str) -> Vec<String> {
    let mut out = keywords(s);
    for w in out.clone() {
        for alias in semantic_aliases(&w) {
            if !out.iter().any(|x| x == alias) {
                out.push(alias.to_string());
            }
        }
    }
    out
}

fn semantic_aliases(w: &str) -> &'static [&'static str] {
    match w {
        "desktop" | "native" | "window" => &["gui", "tauri"],
        "package" | "packaging" | "installer" | "bundle" | "release" => {
            &["build", "tauri", "msi", "exe", "distribution"]
        }
        "memory" | "remember" => &["recall", "learning", "learnings"],
        "search" => &["web", "tavily", "duckduckgo", "grep", "glob"],
        "sandbox" => &["approval", "policy", "permission"],
        "parallel" | "worker" => &["orchestrator", "isolate", "verifier"],
        "rust" => &["cargo", "crate", "gnu"],
        _ => &[],
    }
}

/// Hybrid lexical-semantic score of an entry (text + tags) against the query.
fn semantic_score(
    e: &MemoryEntry,
    qwords: &[String],
    qset: &std::collections::HashSet<String>,
    qphrases: &[String],
) -> i64 {
    if qwords.is_empty() {
        return 0;
    }
    let hay = format!(
        "{} {}",
        e.text.to_lowercase(),
        e.tags.join(" ").to_lowercase()
    );
    let entry_set = word_set(&hay);
    let mut score = 0i64;
    for w in qwords {
        if e.tags.iter().any(|t| t.eq_ignore_ascii_case(w)) {
            score += 8;
        } else if hay.contains(w.as_str()) {
            score += 4;
        }
    }
    for p in qphrases {
        if hay.contains(p.as_str()) {
            score += 6;
        }
    }
    score += (jaccard(qset, &entry_set) * 20.0).round() as i64;
    score
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Significant words of an entry (lowercased ASCII >= 3 or CJK bigrams), as a set.
pub(crate) fn word_set(text: &str) -> std::collections::HashSet<String> {
    tokenize_mixed(text).into_iter().collect()
}

fn phrases(text: &str) -> Vec<String> {
    let words = keywords(text);
    let mut out = Vec::new();
    for pair in words.windows(2) {
        let p = format!("{} {}", pair[0], pair[1]);
        if !out.iter().any(|x| x == &p) {
            out.push(p);
        }
    }
    out
}

/// Jaccard similarity of two word sets (|∩| / |∪|); 0 when both empty.
pub(crate) fn jaccard(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

pub(crate) fn parse_entries(text: &str) -> Vec<MemoryEntry> {
    let mut out: Vec<MemoryEntry> = Vec::new();
    let mut cur: Option<(u64, Vec<String>)> = None;
    let mut body: Vec<String> = Vec::new();

    let flush = |cur: &mut Option<(u64, Vec<String>)>,
                 body: &mut Vec<String>,
                 out: &mut Vec<MemoryEntry>| {
        if let Some((ts, tags)) = cur.take() {
            let txt = body.join("\n").trim().to_string();
            if !txt.is_empty() {
                out.push(MemoryEntry {
                    ts,
                    tags,
                    text: txt,
                });
            }
        }
        body.clear();
    };

    for line in text.lines() {
        if let Some(header) = line
            .trim()
            .strip_prefix("<!-- ")
            .and_then(|s| s.strip_suffix(" -->"))
        {
            flush(&mut cur, &mut body, &mut out);
            // header = "ts:<n> tags:<a,b>"
            let mut ts = 0u64;
            let mut tags: Vec<String> = Vec::new();
            for tok in header.split_whitespace() {
                if let Some(v) = tok.strip_prefix("ts:") {
                    ts = v.parse().unwrap_or(0);
                } else if let Some(v) = tok.strip_prefix("tags:") {
                    tags = v
                        .split(',')
                        .filter(|t| !t.is_empty())
                        .map(|t| t.to_string())
                        .collect();
                }
            }
            cur = Some((ts, tags));
        } else if cur.is_some() {
            body.push(line.to_string());
        }
    }
    flush(&mut cur, &mut body, &mut out);
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> MemoryStore {
        let d = crate::test_support::unique_temp_dir(&format!("ncx_mem_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        MemoryStore::new(d)
    }

    #[test]
    fn remember_then_round_trips() {
        let s = store("rt");
        assert!(
            s.remember(
                "Use crate-type=lib on the gnu toolchain",
                &["build".into()],
                100
            )
            .unwrap()
        );
        let es = s.entries();
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].ts, 100);
        assert_eq!(es[0].tags, vec!["build"]);
        assert!(es[0].text.contains("crate-type"));
    }

    #[test]
    fn dedup_skips_identical() {
        let s = store("dedup");
        assert!(s.remember("same fact", &[], 1).unwrap());
        // different whitespace/case -> still a duplicate
        assert!(!s.remember("  SAME   fact ", &[], 2).unwrap());
        assert_eq!(s.entries().len(), 1);
    }

    #[test]
    fn empty_is_not_stored() {
        let s = store("empty");
        assert!(!s.remember("   ", &[], 1).unwrap());
        assert!(s.entries().is_empty());
    }

    #[test]
    fn cap_drops_oldest() {
        let s = store("cap");
        for i in 0..(MAX_ENTRIES as u64 + 5) {
            s.remember(&format!("fact number {i}"), &[], i).unwrap();
        }
        let es = s.entries();
        assert_eq!(es.len(), MAX_ENTRIES);
        // oldest (ts 0..4) dropped; newest kept.
        assert!(es.iter().all(|e| e.ts >= 5));
    }

    #[test]
    fn recall_scores_by_keyword_overlap() {
        let s = store("recall");
        s.remember(
            "Windows GNU linker overflows on cdylib",
            &["build".into()],
            1,
        )
        .unwrap();
        s.remember(
            "The storyboard panel renders thumbnails",
            &["gui".into()],
            2,
        )
        .unwrap();
        let block = s.recall("how do I fix the linker on windows", 5, 4000);
        assert!(block.contains("treat as leads"));
        assert!(block.contains("GNU linker"));
        // The unrelated entry ranks below; with both fitting it still appears,
        // but the linker note must be present and first after the header.
        let first_bullet = block.lines().nth(1).unwrap_or("");
        assert!(first_bullet.contains("linker"), "got: {first_bullet}");
    }

    #[test]
    fn recall_uses_semantic_aliases_and_tags() {
        let s = store("semantic");
        s.remember(
            "Tauri desktop shell builds compact Windows bundles",
            &["gui".into()],
            1,
        )
        .unwrap();
        s.remember("Use cargo test for the Rust workspace", &["test".into()], 2)
            .unwrap();
        let block = s.recall("native installer release package", 1, 4000);
        assert!(block.contains("Tauri desktop"), "got: {block}");
        assert!(!block.contains("cargo test"), "got: {block}");
    }

    #[test]
    fn recall_empty_store_is_blank() {
        let s = store("blank");
        assert_eq!(s.recall("anything", 5, 4000), "");
    }

    #[test]
    fn consolidate_merges_near_duplicates() {
        let s = store("consolidate");
        // Two near-identical notes (≈0.86 Jaccard) + one distinct.
        s.remember("the gnu toolchain is used for the build", &[], 1)
            .unwrap();
        s.remember("the gnu toolchain used for the build here", &[], 2)
            .unwrap();
        s.remember("the storyboard panel renders thumbnails", &[], 3)
            .unwrap();
        let removed = s.consolidate(0.8).unwrap();
        assert_eq!(removed, 1, "one near-dup should be merged away");
        let es = s.entries();
        assert_eq!(es.len(), 2);
        // The newer of the near-dup cluster (ts 2) is the one kept.
        assert!(es.iter().any(|e| e.ts == 2));
        assert!(es.iter().any(|e| e.text.contains("storyboard")));
    }

    struct FixedMerger(&'static str);
    #[async_trait(?Send)]
    impl Summarizer for FixedMerger {
        async fn merge(&self, _facts: &[String]) -> Option<String> {
            if self.0.is_empty() {
                None
            } else {
                Some(self.0.to_string())
            }
        }
    }

    #[tokio::test]
    async fn summarize_merges_cluster_into_one() {
        let s = store("llm_merge");
        s.remember(
            "the gnu toolchain is used for the build",
            &["build".into()],
            1,
        )
        .unwrap();
        s.remember("the gnu toolchain used for the build here", &[], 2)
            .unwrap();
        s.remember("the storyboard panel renders thumbnails", &[], 3)
            .unwrap();
        let removed = s
            .summarize_consolidate(&FixedMerger("gnu toolchain, no MSVC"), 0.8)
            .await
            .unwrap();
        assert_eq!(removed, 1); // the 2-entry cluster folds to one
        let es = s.entries();
        assert_eq!(es.len(), 2);
        let merged = es
            .iter()
            .find(|e| e.text == "gnu toolchain, no MSVC")
            .unwrap();
        assert_eq!(merged.ts, 2, "keeps newest ts of the cluster");
        assert!(
            merged.tags.contains(&"build".to_string()),
            "keeps union of tags"
        );
        assert!(es.iter().any(|e| e.text.contains("storyboard")));
    }

    #[tokio::test]
    async fn summarize_falls_back_to_newest_when_merge_none() {
        let s = store("llm_none");
        s.remember("the gnu toolchain is used for the build", &[], 1)
            .unwrap();
        s.remember("the gnu toolchain used for the build here", &[], 2)
            .unwrap();
        // empty merger → None → fallback keeps the newest of the cluster.
        let removed = s
            .summarize_consolidate(&FixedMerger(""), 0.8)
            .await
            .unwrap();
        assert_eq!(removed, 1);
        let es = s.entries();
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].ts, 2);
    }

    #[test]
    fn consolidate_is_idempotent() {
        let s = store("idem");
        s.remember("alpha beta gamma delta", &[], 1).unwrap();
        s.remember("alpha beta gamma delta epsilon", &[], 2)
            .unwrap();
        let first = s.consolidate(0.8).unwrap();
        assert!(first >= 1);
        let second = s.consolidate(0.8).unwrap();
        assert_eq!(second, 0, "re-running removes nothing");
    }

    #[test]
    fn recall_respects_entry_cap() {
        let s = store("reccap");
        for i in 0..10 {
            s.remember(&format!("note {i} about widgets"), &[], i)
                .unwrap();
        }
        let block = s.recall("widgets", 3, 4000);
        assert_eq!(block.lines().filter(|l| l.starts_with("- ")).count(), 3);
    }

    #[test]
    fn cjk_bigrams_enable_chinese_consolidation_and_recall() {
        let s = store("cjk_bigram");
        s.remember(
            "在 Windows 环境下编译桌面端 Tauri 应用需要启用 GNU 工具链配置",
            &[],
            1,
        )
        .unwrap();
        s.remember(
            "在 Windows 环境下编译桌面端 Tauri 应用需要启用 GNU 工具链设置",
            &[],
            2,
        )
        .unwrap();
        s.remember("前端状态管理使用 Svelte runes 响应式存储", &[], 3)
            .unwrap();

        let removed = s.consolidate(0.75).unwrap();
        assert_eq!(removed, 1, "Chinese near-duplicate notes should merge");
        assert_eq!(s.entries().len(), 2);

        let recalled = s.recall("如何配置 Windows 编译工具链", 1, 4000);
        assert!(recalled.contains("GNU 工具链"), "got: {recalled}");
        assert!(!recalled.contains("Svelte"), "got: {recalled}");
    }

    #[test]
    fn review_command_json_returns_structured_candidates() {
        use crate::memory_index::{Evidence, MemoryIndex};
        let s = store("review_json");
        let idx = MemoryIndex::for_store(&s).unwrap();
        idx.record_candidate(
            &s.workspace_key(),
            "single-hit candidate",
            &["experience".into()],
            &Evidence::VerifiedToolResult {
                source_ref: "ev-0".to_string(),
            },
            20,
        )
        .unwrap();
        for t in [10, 30, 40] {
            idx.record_candidate(
                &s.workspace_key(),
                "cargo check failed (`error[E0433]`) — resolved on retry",
                &["experience".into()],
                &Evidence::VerifiedToolResult {
                    source_ref: format!("ev-{t}"),
                },
                t,
            )
            .unwrap();
        }
        let raw = s.review_command("json");
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // The 3-hit candidate ranks before the 1-hit candidate.
        assert_eq!(arr[0]["kind"], "candidate");
        assert!(arr[0]["text"].as_str().unwrap().contains("E0433"));
        assert!(
            arr[0]["detail"].as_str().unwrap().contains("命中 3 次"),
            "got: {}",
            arr[0]["detail"]
        );
        assert_eq!(arr[1]["text"], "single-hit candidate");
    }
}
