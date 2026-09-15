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

use crate::instruction_state::{self, Origin, Polarity};
use crate::memory::MemoryStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteStatus {
    Active,
    /// Contradicted by a later, higher-authority instruction.
    Superseded,
}

impl NoteStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
        }
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
                superseded_by TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memory_workspace_status
                ON memory_index(workspace, status);
            CREATE INDEX IF NOT EXISTS idx_memory_created
                ON memory_index(created_at);",
        )
        .map_err(|e| e.to_string())?;
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
        if matches!(evidence, Evidence::SessionSummary { .. }) {
            return Err(PromotionRefused::UnverifiedSummary);
        }
        let note_id = format!(
            "m-{}",
            crate::context_trace::content_hash(&format!("{workspace}|{text}"))
        );
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT note_id FROM memory_index WHERE note_id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| PromotionRefused::Storage(e.to_string()))?;
        if existing.is_some() {
            return Err(PromotionRefused::Duplicate);
        }
        let keywords = instruction_state::parse(text).1.join(" ");
        self.conn
            .execute(
                "INSERT INTO memory_index (
                    note_id, workspace, text, tags, keywords, source_ref,
                    evidence_kind, status, created_at, superseded_by
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
                params![
                    note_id,
                    workspace,
                    text,
                    tags.join(","),
                    keywords,
                    evidence.source_ref(),
                    evidence.kind(),
                    NoteStatus::Active.as_str(),
                    created_at as i64,
                ],
            )
            .map_err(|e| PromotionRefused::Storage(e.to_string()))?;
        Ok(note_id)
    }

    /// Recall notes for one workspace, newest first. Superseded notes are never
    /// returned: a note the user has since contradicted is not a lead.
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
                "DELETE FROM memory_index WHERE workspace = ?1",
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
        Ok(stored)
    }
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedNote> {
    let tags: String = row.get("tags")?;
    let status: String = row.get("status")?;
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
        status: if status == "superseded" {
            NoteStatus::Superseded
        } else {
            NoteStatus::Active
        },
        created_at: row.get::<_, i64>("created_at")? as u64,
        superseded_by: row.get("superseded_by")?,
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
}
