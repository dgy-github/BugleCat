//! Queryable metadata index over the Markdown memory store.
//!
//! The Markdown file stays the source of truth — it is human-readable and
//! human-editable, and this index is always rebuildable from it. What the index
//! adds is the ability to filter recall by workspace, time and validity instead
//! of scoring every note by keyword overlap on every turn.
//!
//! Two safety properties live here rather than in the caller:
//!
//! * A model-written session summary is never sufficient evidence to become a
//!   long-term memory. Summaries are claims about a conversation; memories are
//!   claims about the project.
//! * A note contradicted by a current-session instruction is marked superseded
//!   and withheld from recall, instead of being deleted. The original text and
//!   its `source_ref` survive so the override can be audited.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::instruction_state::{self, Origin, Polarity};
use crate::memory::MemoryStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteStatus {
    Active,
    /// Contradicted by a later, higher-authority instruction.
    Superseded,
    /// Automatically demoted due to low empirical acceptance (`injected >= 8 && p < 0.25`).
    Demoted,
    /// Experience candidate paired from failure -> verified recovery, awaiting confirmation.
    Candidate,
}

impl NoteStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Demoted => "demoted",
            Self::Candidate => "candidate",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "superseded" => Self::Superseded,
            "demoted" => Self::Demoted,
            "candidate" => Self::Candidate,
            _ => Self::Active,
        }
    }
}

/// Empirical usage and outcome attribution statistics for a single memory note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryFeedbackStats {
    pub note_id: String,
    pub workspace: String,
    pub injected: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub last_used: u64,
    /// Persistent demotion flag. Only set in feedback mode `on`; survives
    /// `rebuild_from_markdown` and can be cleared by `--memory-review restore`.
    pub demoted: bool,
}

impl MemoryFeedbackStats {
    /// Beta(1, 1) Laplace-smoothed posterior acceptance rate: `(accepted + 1) / (injected + 2)`.
    pub fn beta_posterior(&self) -> f64 {
        (self.accepted as f64 + 1.0) / (self.injected as f64 + 2.0)
    }

    /// Bounded score multiplier in `[0.8, 1.2]` (`±20%` around neutral `1.0` at `p = 0.5`).
    pub fn bounded_multiplier(&self) -> f64 {
        (0.8 + 0.4 * self.beta_posterior()).clamp(0.8, 1.2)
    }

    /// Whether this note has enough negative evidence (`injected >= 8 && p < 0.25`) to be demoted.
    pub fn should_demote(&self) -> bool {
        self.injected >= 8 && self.beta_posterior() < 0.25
    }
}

/// Where a candidate memory came from. The variant, not the caller's assurance,
/// decides whether it may be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evidence {
    /// A tool ran and its result was recorded.
    VerifiedToolResult { source_ref: String },
    /// The user stated it directly.
    UserStatement { source_ref: String },
    /// A model-written checkpoint summary. Never sufficient on its own.
    SessionSummary { source_ref: String },
}

impl Evidence {
    fn source_ref(&self) -> &str {
        match self {
            Self::VerifiedToolResult { source_ref }
            | Self::UserStatement { source_ref }
            | Self::SessionSummary { source_ref } => source_ref,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::VerifiedToolResult { .. } => "verified_tool_result",
            Self::UserStatement { .. } => "user_statement",
            Self::SessionSummary { .. } => "session_summary",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionRefused {
    /// A summary was offered as the only evidence.
    UnverifiedSummary,
    /// The same note already exists for this workspace.
    Duplicate,
    Storage(String),
}

impl std::fmt::Display for PromotionRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnverifiedSummary => f.write_str(
                "拒绝入库：会话摘要不是已验证事实，需要工具结果或用户明确陈述作为来源。",
            ),
            Self::Duplicate => f.write_str("已存在相同记忆，未重复写入。"),
            Self::Storage(error) => write!(f, "记忆索引写入失败：{error}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNote {
    pub note_id: String,
    pub workspace: String,
    pub text: String,
    pub tags: Vec<String>,
    pub source_ref: String,
    pub evidence_kind: String,
    pub status: NoteStatus,
    pub created_at: u64,
    pub superseded_by: Option<String>,
    pub occurrences: u64,
}

pub struct MemoryIndex {
    conn: Connection,
}

impl std::fmt::Debug for MemoryIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryIndex")
    }
}

impl MemoryIndex {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Self::init(Connection::open(path).map_err(|e| e.to_string())?)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(|e| e.to_string())?)
    }

    /// Index living beside the Markdown store it describes.
    pub fn for_store(store: &MemoryStore) -> Result<Self, String> {
        let dir: PathBuf = store
            .markdown_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        Self::open(dir.join("index.sqlite3"))
    }

    fn init(conn: Connection) -> Result<Self, String> {
        let _: Result<String, _> = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0));
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_index (
                note_id       TEXT PRIMARY KEY,
                workspace     TEXT NOT NULL,
                text          TEXT NOT NULL,
                tags          TEXT NOT NULL,
                keywords      TEXT NOT NULL,
                source_ref    TEXT NOT NULL,
                evidence_kind TEXT NOT NULL,
                status        TEXT NOT NULL,
                created_at    INTEGER NOT NULL,
                superseded_by TEXT,
                occurrences   INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_memory_workspace_status
                ON memory_index(workspace, status);
            CREATE INDEX IF NOT EXISTS idx_memory_created
                ON memory_index(created_at);
            CREATE TABLE IF NOT EXISTS memory_feedback (
                note_id   TEXT NOT NULL,
                workspace TEXT NOT NULL,
                injected  INTEGER NOT NULL DEFAULT 0,
                accepted  INTEGER NOT NULL DEFAULT 0,
                rejected  INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER NOT NULL DEFAULT 0,
                demoted   INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (note_id, workspace)
            );",
        )
        .map_err(|e| e.to_string())?;
        // Databases created before the columns existed: add them, ignore "duplicate column".
        let _ = conn.execute(
            "ALTER TABLE memory_feedback ADD COLUMN demoted INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE memory_index ADD COLUMN occurrences INTEGER NOT NULL DEFAULT 1",
            [],
        );
        Ok(Self { conn })
    }

    /// Store a candidate memory. Refuses summaries, refuses duplicates, and
    /// always keeps the original text and its `source_ref`.
    pub fn promote(
        &self,
        workspace: &str,
        text: &str,
        tags: &[String],
        evidence: &Evidence,
        created_at: u64,
    ) -> Result<String, PromotionRefused> {
        self.insert_with_status(
            workspace,
            text,
            tags,
            evidence,
            NoteStatus::Active,
            created_at,
        )
    }

    /// Store an experience candidate (from within-turn tool failure -> verified recovery).
    /// Candidates are never recalled until explicitly confirmed via `confirm_candidate`.
    pub fn record_candidate(
        &self,
        workspace: &str,
        text: &str,
        tags: &[String],
        evidence: &Evidence,
        created_at: u64,
    ) -> Result<String, PromotionRefused> {
        self.insert_with_status(
            workspace,
            text,
            tags,
            evidence,
            NoteStatus::Candidate,
            created_at,
        )
    }

    fn insert_with_status(
        &self,
        workspace: &str,
        text: &str,
        tags: &[String],
        evidence: &Evidence,
        status: NoteStatus,
        created_at: u64,
    ) -> Result<String, PromotionRefused> {
        if matches!(evidence, Evidence::SessionSummary { .. }) {
            return Err(PromotionRefused::UnverifiedSummary);
        }
        let note_id = format!(
            "m-{}",
            crate::context_trace::content_hash(&format!("{workspace}|{text}"))
        );
        let existing_status: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM memory_index WHERE note_id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| PromotionRefused::Storage(e.to_string()))?;
        if let Some(existing_status) = existing_status {
            if status == NoteStatus::Candidate && existing_status == "candidate" {
                self.conn
                    .execute(
                        "UPDATE memory_index
                         SET occurrences = occurrences + 1,
                             created_at = MAX(created_at, ?2),
                             source_ref = ?3
                         WHERE note_id = ?1",
                        params![note_id, created_at as i64, evidence.source_ref()],
                    )
                    .map_err(|e| PromotionRefused::Storage(e.to_string()))?;
                return Ok(note_id);
            }
            return Err(PromotionRefused::Duplicate);
        }
        let keywords = instruction_state::parse(text).1.join(" ");
        self.conn
            .execute(
                "INSERT INTO memory_index (
                    note_id, workspace, text, tags, keywords, source_ref,
                    evidence_kind, status, created_at, superseded_by, occurrences
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,1)",
                params![
                    note_id,
                    workspace,
                    text,
                    tags.join(","),
                    keywords,
                    evidence.source_ref(),
                    evidence.kind(),
                    status.as_str(),
                    created_at as i64,
                ],
            )
            .map_err(|e| PromotionRefused::Storage(e.to_string()))?;
        Ok(note_id)
    }

    /// Promote a `Candidate` note to `Active` after human review (`--memory-merge`).
    pub fn confirm_candidate(&self, note_id: &str) -> Result<bool, String> {
        let changed = self
            .conn
            .execute(
                "UPDATE memory_index SET status = 'active' WHERE note_id = ?1 AND status = 'candidate'",
                params![note_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(changed > 0)
    }

    /// Delete an unconfirmed candidate after human review rejected it.
    pub fn reject_candidate(&self, note_id: &str) -> Result<bool, String> {
        let changed = self
            .conn
            .execute(
                "DELETE FROM memory_index WHERE note_id = ?1 AND status = 'candidate'",
                params![note_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(changed > 0)
    }

    /// Delete any indexed note by id (used when a turn's outcome is explicitly rejected).
    pub fn remove_note(&self, note_id: &str) -> Result<bool, String> {
        let changed = self
            .conn
            .execute(
                "DELETE FROM memory_index WHERE note_id = ?1",
                params![note_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(changed > 0)
    }

    /// Return all notes in `workspace` whose `source_ref` begins with `prefix`.
    pub fn notes_for_source_prefix(&self, workspace: &str, prefix: &str) -> Vec<IndexedNote> {
        self.all_notes(workspace)
            .into_iter()
            .filter(|note| note.source_ref.starts_with(prefix))
            .collect()
    }

    /// Score one resolved primary task against the notes it recalled. The
    /// injection and its verdict are counted together, so `injected` only ever
    /// holds resolved tasks (worker, goal and unresolved turns never inflate it).
    /// A note is demoted only when `allow_demote` (feedback mode `on`) and this
    /// verdict is a rejection. Returns the newly demoted ids.
    pub fn record_outcome_feedback(
        &self,
        workspace: &str,
        note_ids: &[String],
        accepted: bool,
        timestamp_ms: u64,
        allow_demote: bool,
    ) -> Vec<String> {
        let (acc_inc, rej_inc) = if accepted { (1i64, 0i64) } else { (0i64, 1i64) };
        for id in note_ids {
            let _ = self.conn.execute(
                "INSERT INTO memory_feedback (note_id, workspace, injected, accepted, rejected, last_used)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5)
                 ON CONFLICT(note_id, workspace) DO UPDATE SET
                    injected = injected + 1,
                    accepted = accepted + ?3,
                    rejected = rejected + ?4,
                    last_used = excluded.last_used",
                params![id, workspace, acc_inc, rej_inc, timestamp_ms as i64],
            );
        }
        if accepted || !allow_demote {
            return Vec::new();
        }
        self.demote_if_due(workspace, note_ids)
    }

    /// Record an injection where the task succeeded only at high cost and the
    /// recalled note was not attributed to any touched file (P3 cost-aware
    /// discounting: increments `injected` without crediting `accepted`, nudging
    /// the note's Beta posterior downward).
    pub fn record_unhelpful_injection(
        &self,
        workspace: &str,
        note_ids: &[String],
        timestamp_ms: u64,
    ) {
        for id in note_ids {
            let _ = self.conn.execute(
                "INSERT INTO memory_feedback (note_id, workspace, injected, accepted, rejected, last_used)
                 VALUES (?1, ?2, 1, 0, 0, ?3)
                 ON CONFLICT(note_id, workspace) DO UPDATE SET
                    injected = injected + 1,
                    last_used = excluded.last_used",
                params![id, workspace, timestamp_ms as i64],
            );
        }
    }

    /// Flip one already-counted verdict (a later human label or rollback
    /// disagreed with the earlier automatic verdict). `injected` is unchanged.
    pub fn revise_outcome_feedback(
        &self,
        workspace: &str,
        note_ids: &[String],
        now_accepted: bool,
        timestamp_ms: u64,
        allow_demote: bool,
    ) -> Vec<String> {
        let (acc_delta, rej_delta) = if now_accepted {
            (1i64, -1i64)
        } else {
            (-1i64, 1i64)
        };
        for id in note_ids {
            let _ = self.conn.execute(
                "UPDATE memory_feedback SET
                    accepted = MAX(0, accepted + ?3),
                    rejected = MAX(0, rejected + ?4),
                    last_used = ?5
                 WHERE note_id = ?1 AND workspace = ?2",
                params![id, workspace, acc_delta, rej_delta, timestamp_ms as i64],
            );
        }
        if now_accepted || !allow_demote {
            return Vec::new();
        }
        self.demote_if_due(workspace, note_ids)
    }

    fn demote_if_due(&self, workspace: &str, note_ids: &[String]) -> Vec<String> {
        let mut newly = Vec::new();
        for id in note_ids {
            let Some(stats) = self.feedback_stat(workspace, id) else {
                continue;
            };
            if !stats.demoted && stats.should_demote() {
                let _ = self.conn.execute(
                    "UPDATE memory_feedback SET demoted = 1 WHERE note_id = ?1 AND workspace = ?2",
                    params![id, workspace],
                );
                newly.push(id.clone());
            }
        }
        if !newly.is_empty() {
            self.apply_feedback_demotions();
        }
        newly
    }

    /// Set or clear the persistent demotion flag of a Markdown note
    /// (`MemoryStore::note_id`). Returns whether a feedback row existed.
    pub fn set_note_demoted(&self, note_id: &str, demoted: bool) -> Result<bool, String> {
        let changed = self
            .conn
            .execute(
                "UPDATE memory_feedback SET demoted = ?2 WHERE note_id = ?1",
                params![note_id, i64::from(demoted)],
            )
            .map_err(|e| e.to_string())?;
        self.apply_feedback_demotions();
        Ok(changed > 0)
    }

    /// Mirror the persistent feedback flags onto `memory_index.status`. Index
    /// rows use their own id scheme, so the match is by Markdown note id of the
    /// row text. `rebuild_from_markdown` calls this again, so a demotion
    /// survives an index rebuild.
    pub fn apply_feedback_demotions(&self) {
        let flagged: std::collections::HashSet<String> = self
            .conn
            .prepare("SELECT note_id FROM memory_feedback WHERE demoted = 1")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(0))
                    .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default();
        let rows: Vec<(String, String, String)> = self
            .conn
            .prepare(
                "SELECT note_id, text, status FROM memory_index
                 WHERE status IN ('active', 'demoted')",
            )
            .and_then(|mut stmt| {
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map(|rows| rows.filter_map(Result::ok).collect())
            })
            .unwrap_or_default();
        for (index_id, text, status) in rows {
            let want_demoted = flagged.contains(&MemoryStore::note_id(&text));
            let next = match (status.as_str(), want_demoted) {
                ("active", true) => NoteStatus::Demoted,
                ("demoted", false) => NoteStatus::Active,
                _ => continue,
            };
            let _ = self.conn.execute(
                "UPDATE memory_index SET status = ?1 WHERE note_id = ?2",
                params![next.as_str(), index_id],
            );
        }
    }

    /// Feedback rows whose persistent demotion flag is set.
    pub fn demoted_feedback(&self) -> Vec<MemoryFeedbackStats> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT note_id, workspace, injected, accepted, rejected, last_used, demoted
             FROM memory_feedback WHERE demoted = 1",
        ) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map([], row_to_feedback) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    pub fn feedback_stat(&self, workspace: &str, note_id: &str) -> Option<MemoryFeedbackStats> {
        self.conn
            .query_row(
                "SELECT note_id, workspace, injected, accepted, rejected, last_used, demoted
                 FROM memory_feedback WHERE workspace = ?1 AND note_id = ?2",
                params![workspace, note_id],
                row_to_feedback,
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn feedback_stats_for_workspace(
        &self,
        workspace: &str,
    ) -> std::collections::HashMap<String, MemoryFeedbackStats> {
        let mut out = std::collections::HashMap::new();
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT note_id, workspace, injected, accepted, rejected, last_used, demoted
             FROM memory_feedback WHERE workspace = ?1",
        ) else {
            return out;
        };
        let Ok(rows) = stmt.query_map(params![workspace], row_to_feedback) else {
            return out;
        };
        for stat in rows.flatten() {
            out.insert(stat.note_id.clone(), stat);
        }
        out
    }

    /// Recall notes for one workspace, newest first. Superseded, Demoted, and Candidate
    /// notes are never returned.
    pub fn recall(&self, workspace: &str, query: &str, limit: usize) -> Vec<IndexedNote> {
        let terms = instruction_state::parse(query).1;
        let mut notes = self.active_notes(workspace);
        if !terms.is_empty() {
            notes.retain(|note| {
                let note_terms = instruction_state::parse(&note.text).1;
                instruction_state::subjects_overlap(&note_terms, &terms)
                    || note
                        .tags
                        .iter()
                        .any(|tag| terms.contains(&tag.to_lowercase()))
            });
        }
        notes.sort_by_key(|note| std::cmp::Reverse(note.created_at));
        notes.truncate(limit);
        notes
    }

    pub fn active_notes(&self, workspace: &str) -> Vec<IndexedNote> {
        self.query_notes(
            "SELECT * FROM memory_index WHERE workspace = ?1 AND status = 'active'",
            workspace,
        )
    }

    pub fn demoted_notes(&self, workspace: &str) -> Vec<IndexedNote> {
        self.query_notes(
            "SELECT * FROM memory_index WHERE workspace = ?1 AND status = 'demoted'",
            workspace,
        )
    }

    pub fn candidate_notes(&self, workspace: &str) -> Vec<IndexedNote> {
        self.query_notes(
            "SELECT * FROM memory_index WHERE workspace = ?1 AND status = 'candidate' ORDER BY occurrences DESC, created_at DESC",
            workspace,
        )
    }

    pub fn all_notes(&self, workspace: &str) -> Vec<IndexedNote> {
        self.query_notes("SELECT * FROM memory_index WHERE workspace = ?1", workspace)
    }

    fn query_notes(&self, sql: &str, workspace: &str) -> Vec<IndexedNote> {
        let Ok(mut stmt) = self.conn.prepare(sql) else {
            return Vec::new();
        };
        let Ok(rows) = stmt.query_map(params![workspace], row_to_note) else {
            return Vec::new();
        };
        rows.filter_map(Result::ok).collect()
    }

    /// Mark every stored note that a current-session instruction contradicts.
    /// Returns the ids that were withdrawn. The rows stay, so the override chain
    /// remains auditable.
    pub fn supersede_conflicting(
        &self,
        workspace: &str,
        instruction: &str,
        instruction_id: &str,
    ) -> Vec<String> {
        let (polarity, subject) = instruction_state::parse(instruction);
        if subject.is_empty() {
            return Vec::new();
        }
        let mut withdrawn = Vec::new();
        for note in self.active_notes(workspace) {
            let (note_polarity, note_subject) = instruction_state::parse(&note.text);
            if !instruction_state::subjects_overlap(&note_subject, &subject) {
                continue;
            }
            // Only an actual contradiction withdraws a note. Agreement, and a
            // note that merely mentions the same topic, are left alone.
            let contradicts = matches!(
                (note_polarity, polarity),
                (Polarity::Allow, Polarity::Forbid)
                    | (Polarity::Forbid, Polarity::Allow)
                    | (_, Polarity::Revoke)
            );
            if !contradicts {
                continue;
            }
            let updated = self.conn.execute(
                "UPDATE memory_index SET status = ?1, superseded_by = ?2 WHERE note_id = ?3",
                params![
                    NoteStatus::Superseded.as_str(),
                    instruction_id,
                    note.note_id
                ],
            );
            if updated.is_ok() {
                withdrawn.push(note.note_id);
            }
        }
        withdrawn
    }

    /// Rebuild the index from the Markdown store. Existing rows for this
    /// workspace are replaced, so a hand-edited Markdown file wins.
    ///
    /// Notes recovered this way carry the Markdown file as their `source_ref`
    /// and are treated as user statements: a human wrote or approved the file.
    pub fn rebuild_from_markdown(
        &self,
        store: &MemoryStore,
        workspace: &str,
    ) -> Result<usize, String> {
        self.conn
            .execute(
                "DELETE FROM memory_index WHERE workspace = ?1 AND status != 'candidate'",
                params![workspace],
            )
            .map_err(|e| e.to_string())?;
        let source = store.markdown_path().display().to_string();
        let mut stored = 0;
        for entry in store.entries() {
            let evidence = Evidence::UserStatement {
                source_ref: source.clone(),
            };
            if self
                .promote(workspace, &entry.text, &entry.tags, &evidence, entry.ts)
                .is_ok()
            {
                stored += 1;
            }
        }
        self.apply_feedback_demotions();
        Ok(stored)
    }
}

fn row_to_feedback(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryFeedbackStats> {
    Ok(MemoryFeedbackStats {
        note_id: row.get(0)?,
        workspace: row.get(1)?,
        injected: row.get::<_, i64>(2)?.max(0) as u64,
        accepted: row.get::<_, i64>(3)?.max(0) as u64,
        rejected: row.get::<_, i64>(4)?.max(0) as u64,
        last_used: row.get::<_, i64>(5)?.max(0) as u64,
        demoted: row.get::<_, i64>(6)? != 0,
    })
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedNote> {
    let tags: String = row.get("tags")?;
    let status: String = row.get("status")?;
    let occurrences = row
        .get::<_, Option<i64>>("occurrences")
        .ok()
        .flatten()
        .unwrap_or(1)
        .max(1) as u64;
    Ok(IndexedNote {
        note_id: row.get("note_id")?,
        workspace: row.get("workspace")?,
        text: row.get("text")?,
        tags: tags
            .split(',')
            .filter(|tag| !tag.trim().is_empty())
            .map(str::to_string)
            .collect(),
        source_ref: row.get("source_ref")?,
        evidence_kind: row.get("evidence_kind")?,
        status: NoteStatus::parse(&status),
        created_at: row.get::<_, i64>("created_at")? as u64,
        superseded_by: row.get("superseded_by")?,
        occurrences,
    })
}

/// Convenience for callers holding an `InstructionLedger`: withdraw every note
/// contradicted by the ledger's current-session instructions.
pub fn apply_ledger(
    index: &MemoryIndex,
    workspace: &str,
    ledger: &instruction_state::InstructionLedger,
) -> Vec<String> {
    let mut withdrawn = Vec::new();
    for instruction in ledger.active() {
        if instruction.origin != Origin::CurrentSession {
            continue;
        }
        withdrawn.extend(index.supersede_conflicting(
            workspace,
            &instruction.text,
            &instruction.id,
        ));
    }
    withdrawn
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified(reference: &str) -> Evidence {
        Evidence::VerifiedToolResult {
            source_ref: reference.into(),
        }
    }

    #[test]
    fn a_session_summary_cannot_become_a_long_term_memory() {
        let index = MemoryIndex::open_in_memory().unwrap();
        let refused = index
            .promote(
                "/ws",
                "构建总是在 5 分钟内完成",
                &[],
                &Evidence::SessionSummary {
                    source_ref: "s.jsonl#40".into(),
                },
                1,
            )
            .unwrap_err();
        assert_eq!(refused, PromotionRefused::UnverifiedSummary);
        assert!(index.all_notes("/ws").is_empty());

        // The same claim backed by a recorded tool result is acceptable.
        index
            .promote(
                "/ws",
                "构建总是在 5 分钟内完成",
                &[],
                &verified("s.jsonl#41"),
                1,
            )
            .unwrap();
        assert_eq!(index.active_notes("/ws").len(), 1);
    }

    #[test]
    fn promotion_keeps_the_original_text_and_source_reference() {
        let index = MemoryIndex::open_in_memory().unwrap();
        let id = index
            .promote(
                "/ws",
                "发布脚本需要 PROTOC 环境变量",
                &["build".to_string()],
                &verified("events#7"),
                10,
            )
            .unwrap();
        let note = &index.active_notes("/ws")[0];
        assert_eq!(note.note_id, id);
        assert_eq!(note.text, "发布脚本需要 PROTOC 环境变量");
        assert_eq!(note.source_ref, "events#7");
        assert_eq!(note.evidence_kind, "verified_tool_result");
        assert_eq!(note.tags, vec!["build".to_string()]);
    }

    #[test]
    fn the_same_note_is_not_stored_twice() {
        let index = MemoryIndex::open_in_memory().unwrap();
        index
            .promote("/ws", "使用 MSVC target", &[], &verified("a"), 1)
            .unwrap();
        let again = index
            .promote("/ws", "使用 MSVC target", &[], &verified("b"), 2)
            .unwrap_err();
        assert_eq!(again, PromotionRefused::Duplicate);
        assert_eq!(index.all_notes("/ws").len(), 1);
    }

    #[test]
    fn notes_are_scoped_to_their_workspace() {
        let index = MemoryIndex::open_in_memory().unwrap();
        index
            .promote("/a", "A 项目用 pnpm", &[], &verified("x"), 1)
            .unwrap();
        index
            .promote("/b", "B 项目用 cargo", &[], &verified("y"), 1)
            .unwrap();

        assert_eq!(index.active_notes("/a").len(), 1);
        assert_eq!(index.active_notes("/a")[0].text, "A 项目用 pnpm");
        // Switching workspace must not leak the other project's notes.
        assert_eq!(index.active_notes("/b").len(), 1);
        assert!(index.recall("/b", "pnpm", 10).is_empty());
    }

    #[test]
    fn a_current_instruction_withdraws_the_note_it_contradicts_but_keeps_the_row() {
        let index = MemoryIndex::open_in_memory().unwrap();
        index
            .promote("/ws", "可以直接提交代码", &[], &verified("old"), 1)
            .unwrap();
        index
            .promote("/ws", "测试用 pytest 运行", &[], &verified("other"), 1)
            .unwrap();

        let withdrawn = index.supersede_conflicting("/ws", "不要提交代码", "i9");
        assert_eq!(withdrawn.len(), 1);

        // Recall no longer offers the contradicted note...
        let active = index.active_notes("/ws");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].text, "测试用 pytest 运行");
        // ...but the row and its override link survive for audit.
        let all = index.all_notes("/ws");
        assert_eq!(all.len(), 2);
        let superseded = all
            .iter()
            .find(|note| note.status == NoteStatus::Superseded)
            .unwrap();
        assert_eq!(superseded.superseded_by.as_deref(), Some("i9"));
        assert_eq!(superseded.text, "可以直接提交代码");
    }

    #[test]
    fn an_agreeing_instruction_does_not_withdraw_anything() {
        let index = MemoryIndex::open_in_memory().unwrap();
        index
            .promote("/ws", "不要提交代码", &[], &verified("old"), 1)
            .unwrap();
        assert!(index
            .supersede_conflicting("/ws", "不要提交代码到 main", "i1")
            .is_empty());
        assert_eq!(index.active_notes("/ws").len(), 1);
    }

    #[test]
    fn a_ledger_withdraws_only_via_current_session_instructions() {
        let index = MemoryIndex::open_in_memory().unwrap();
        index
            .promote("/ws", "可以删除缓存目录", &[], &verified("old"), 1)
            .unwrap();
        let mut ledger = instruction_state::InstructionLedger::new();
        // A remembered instruction must not withdraw a stored note by itself.
        ledger.record("不要删除缓存目录", None, Origin::LongTermMemory, 5);
        assert!(apply_ledger(&index, "/ws", &ledger).is_empty());
        assert_eq!(index.active_notes("/ws").len(), 1);

        let mut current = instruction_state::InstructionLedger::new();
        current.record("不要删除缓存目录", None, Origin::CurrentSession, 6);
        assert_eq!(apply_ledger(&index, "/ws", &current).len(), 1);
        assert!(index.active_notes("/ws").is_empty());
    }

    #[test]
    fn rebuilding_from_markdown_replaces_only_this_workspace() {
        let dir = crate::test_support::unique_temp_dir("memory_index_rebuild");
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::new(&dir);
        store
            .remember("构建需要 MSVC", &["build".into()], 10)
            .unwrap();
        store.remember("测试用 pytest", &[], 20).unwrap();

        let index = MemoryIndex::for_store(&store).unwrap();
        index
            .promote("/other", "别的项目的记忆", &[], &verified("keep"), 1)
            .unwrap();
        let stored = index.rebuild_from_markdown(&store, "/ws").unwrap();

        assert_eq!(stored, 2);
        assert_eq!(index.active_notes("/ws").len(), 2);
        assert_eq!(index.active_notes("/other").len(), 1);
        // The Markdown file remains the source of truth for its rows.
        assert!(index.active_notes("/ws")[0]
            .source_ref
            .contains("LEARNINGS.md"));
    }

    #[test]
    fn a_failed_rebuild_leaves_the_markdown_store_untouched() {
        let dir = crate::test_support::unique_temp_dir("memory_index_failure");
        std::fs::create_dir_all(&dir).unwrap();
        let store = MemoryStore::new(&dir);
        store.remember("只读事实", &[], 1).unwrap();
        let before = store.entries();

        let index = MemoryIndex::open_in_memory().unwrap();
        index.rebuild_from_markdown(&store, "/ws").unwrap();
        // Indexing never writes back to the Markdown store.
        assert_eq!(store.entries(), before);
    }

    fn feedback_store(label: &str, mode: &str, notes: &[&str]) -> MemoryStore {
        let dir = crate::test_support::unique_temp_dir(label);
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = MemoryStore::new(dir.join(".ncx").join("memory"));
        store.set_feedback_mode(mode);
        for (i, text) in notes.iter().enumerate() {
            store.remember(text, &[], i as u64 + 1).unwrap();
        }
        store
    }

    fn reject_many(store: &MemoryStore, id: &str, times: usize, allow_demote: bool) {
        let index = MemoryIndex::for_store(store).unwrap();
        for _ in 0..times {
            index.record_outcome_feedback(
                &store.workspace_key(),
                &[id.to_string()],
                false,
                1,
                allow_demote,
            );
        }
    }

    #[test]
    fn shadow_recall_injects_exactly_what_off_injects() {
        let notes = [
            "cargo build needs the MSVC linker on windows",
            "cargo test must run with --locked",
        ];
        let off = feedback_store("memory_fb_off", "off", &notes);
        let shadow = feedback_store("memory_fb_shadow", "shadow", &notes);
        // Strong negative evidence in shadow's feedback table must not change
        // what gets injected.
        let target = MemoryStore::note_id(notes[1]);
        reject_many(&shadow, &target, 12, false);
        let q = "how do I run cargo test";
        let a = off.recall_with_feedback(q, 5, 4000);
        let b = shadow.recall_with_feedback(q, 5, 4000);
        assert_eq!(a.text, b.text);
        assert_eq!(a.note_ids, b.note_ids);
        assert_eq!(a.rank_changed, None);
        assert!(b.rank_changed.is_some());
    }

    #[test]
    fn on_mode_withholds_flagged_notes_and_reports_the_rank_change() {
        let notes = ["always run cargo fmt", "cargo clippy before pushing"];
        let store = feedback_store("memory_fb_on", "on", &notes);
        let target = MemoryStore::note_id(notes[0]);
        reject_many(&store, &target, 9, true);
        let recall = store.recall_with_feedback("cargo", 5, 4000);
        assert!(!recall.note_ids.contains(&target));
        assert_eq!(recall.rank_changed, Some(true));

        // Restoring clears the flag and the note is recalled again.
        let index = MemoryIndex::for_store(&store).unwrap();
        assert!(index.set_note_demoted(&target, false).unwrap());
        let recall = store.recall_with_feedback("cargo", 5, 4000);
        assert!(recall.note_ids.contains(&target));
    }

    #[test]
    fn demotion_flag_survives_an_index_rebuild_and_matches_by_note_text() {
        let notes = ["prefer apply_patch over write_file"];
        let store = feedback_store("memory_fb_rebuild", "on", &notes);
        let index = MemoryIndex::for_store(&store).unwrap();
        let ws = store.workspace_key();
        index.rebuild_from_markdown(&store, &ws).unwrap();
        assert_eq!(index.active_notes(&ws).len(), 1);

        reject_many(&store, &MemoryStore::note_id(notes[0]), 9, true);
        assert_eq!(index.active_notes(&ws).len(), 0);
        assert_eq!(index.demoted_notes(&ws).len(), 1);

        // A rebuild re-creates rows as active; the persistent flag re-demotes them.
        index.rebuild_from_markdown(&store, &ws).unwrap();
        assert_eq!(index.active_notes(&ws).len(), 0);
        assert_eq!(index.demoted_notes(&ws).len(), 1);
    }

    #[test]
    fn candidates_wait_for_explicit_review() {
        let store = feedback_store("memory_review_flow", "shadow", &[]);
        let index = MemoryIndex::for_store(&store).unwrap();
        let ws = store.workspace_key();
        let keep = index
            .record_candidate(
                &ws,
                "retry cargo build after clearing target/",
                &[],
                &verified("e#1"),
                5,
            )
            .unwrap();
        let drop = index
            .record_candidate(
                &ws,
                "shell on `ls` failed then succeeded",
                &[],
                &verified("e#2"),
                6,
            )
            .unwrap();
        // Candidates are neither recalled nor written to Markdown by themselves.
        assert!(index.recall(&ws, "cargo build", 5).is_empty());
        assert!(store.entries().is_empty());
        assert_eq!(store.review_items().len(), 2);

        assert!(store
            .review_command(&format!("confirm {keep}"))
            .starts_with("confirmed"));
        assert!(store
            .review_command(&format!("reject {drop}"))
            .starts_with("rejected"));
        assert_eq!(store.entries().len(), 1);
        assert!(store.review_items().is_empty());
        assert!(store
            .review_command("confirm m-missing")
            .starts_with("no reviewable"));
    }
}
