//! Durable fact-event store for one workspace's agent sessions.
//!
//! Complements, and does not replace, the two existing stores: the session
//! JSONL log stays the verbatim wire history, and `context_trace` stays the
//! append-only provenance excerpt file. This store adds what neither offers —
//! indexed, terminal-state-aware queries over facts produced across sessions,
//! turns and sub-agents, so a caller can ask "did this tool call finish?"
//! without replaying a log.
//!
//! Writes are idempotent by `event_id`: a crash between the side effect and the
//! record, followed by a replay, must not create a second event.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// What a recorded fact is about. Kept deliberately small; anything more
/// specific belongs in `payload_json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    UserMessage,
    ToolCall,
    ToolResult,
    StateTransition,
    ApprovalDecision,
    WriteProbe,
    Checkpoint,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::StateTransition => "state_transition",
            Self::ApprovalDecision => "approval_decision",
            Self::WriteProbe => "write_probe",
            Self::Checkpoint => "checkpoint",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "user_message" => Self::UserMessage,
            "tool_call" => Self::ToolCall,
            "tool_result" => Self::ToolResult,
            "state_transition" => Self::StateTransition,
            "approval_decision" => Self::ApprovalDecision,
            "write_probe" => Self::WriteProbe,
            "checkpoint" => Self::Checkpoint,
            _ => return None,
        })
    }
}

/// Lifecycle of the fact itself, independent of whether the agent succeeded.
/// `Unknown` is a real, queryable state: a write whose outcome was never
/// observed must not be silently treated as either done or not done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Started,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

impl EventStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "started" => Self::Started,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }

    /// True once the fact can no longer change on its own.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// One recorded fact. `sequence` is per-session and assigned by the store, so
/// concurrent sub-agents in different sessions cannot collide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEvent {
    pub event_id: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<u64>,
    pub sequence: i64,
    pub timestamp: u64,
    pub event_type: EventType,
    pub status: EventStatus,
    pub tool_call_id: Option<String>,
    pub workspace: Option<String>,
    pub payload_json: String,
    /// Points back at the verbatim record: a JSONL line, a file path, a commit.
    pub source_ref: Option<String>,
}

impl ContextEvent {
    /// Minimal constructor; `sequence` is overwritten on append.
    pub fn new(
        event_id: impl Into<String>,
        session_id: impl Into<String>,
        event_type: EventType,
        status: EventStatus,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            session_id: session_id.into(),
            task_id: None,
            agent_id: None,
            parent_agent_id: None,
            thread_id: None,
            turn_id: None,
            sequence: 0,
            timestamp: now_millis(),
            event_type,
            status,
            tool_call_id: None,
            workspace: None,
            payload_json: "{}".into(),
            source_ref: None,
        }
    }

    pub fn with_turn(mut self, turn_id: Option<u64>) -> Self {
        self.turn_id = turn_id;
        self
    }

    pub fn with_task(mut self, task_id: Option<String>) -> Self {
        self.task_id = task_id;
        self
    }

    pub fn with_agent(mut self, agent_id: Option<String>, parent: Option<String>) -> Self {
        self.agent_id = agent_id;
        self.parent_agent_id = parent;
        self
    }

    pub fn with_tool_call(mut self, tool_call_id: Option<String>) -> Self {
        self.tool_call_id = tool_call_id;
        self
    }

    pub fn with_workspace(mut self, workspace: Option<String>) -> Self {
        self.workspace = workspace;
        self
    }

    pub fn with_payload(mut self, payload: &serde_json::Value) -> Self {
        self.payload_json = payload.to_string();
        self
    }

    pub fn with_source_ref(mut self, source_ref: Option<String>) -> Self {
        self.source_ref = source_ref;
        self
    }
}

pub struct ContextStore {
    conn: Connection,
}

impl std::fmt::Debug for ContextStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContextStore")
    }
}

impl ContextStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        // WAL keeps a reader (a retrieval query) from blocking the writer (the
        // live turn). Failure is not fatal: an in-memory database has no WAL.
        let _: Result<String, _> = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0));
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS context_events (
                event_id        TEXT PRIMARY KEY,
                session_id      TEXT NOT NULL,
                task_id         TEXT,
                agent_id        TEXT,
                parent_agent_id TEXT,
                thread_id       TEXT,
                turn_id         INTEGER,
                sequence        INTEGER NOT NULL,
                timestamp       INTEGER NOT NULL,
                event_type      TEXT NOT NULL,
                status          TEXT NOT NULL,
                tool_call_id    TEXT,
                workspace       TEXT,
                payload_json    TEXT NOT NULL,
                source_ref      TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_events_session_sequence
                ON context_events(session_id, sequence);
            CREATE INDEX IF NOT EXISTS idx_events_task_status
                ON context_events(task_id, status);
            CREATE INDEX IF NOT EXISTS idx_events_tool_call
                ON context_events(tool_call_id);
            CREATE INDEX IF NOT EXISTS idx_events_workspace
                ON context_events(workspace);
            CREATE INDEX IF NOT EXISTS idx_events_agent
                ON context_events(agent_id);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// Append one event, assigning the next per-session sequence. Re-appending a
    /// known `event_id` is a no-op that returns the stored event, so a replayed
    /// write cannot be double-counted.
    pub fn append(&self, event: &ContextEvent) -> Result<ContextEvent, String> {
        if let Some(existing) = self.event(&event.event_id)? {
            return Ok(existing);
        }
        let mut stored = event.clone();
        // Sequence assignment and insert share one transaction, so two writers
        // cannot observe the same "next" value.
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let next: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM context_events WHERE session_id = ?1",
                params![stored.session_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        stored.sequence = next;
        tx.execute(
            "INSERT INTO context_events (
                event_id, session_id, task_id, agent_id, parent_agent_id, thread_id,
                turn_id, sequence, timestamp, event_type, status, tool_call_id,
                workspace, payload_json, source_ref
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                stored.event_id,
                stored.session_id,
                stored.task_id,
                stored.agent_id,
                stored.parent_agent_id,
                stored.thread_id,
                stored.turn_id.map(|v| v as i64),
                stored.sequence,
                stored.timestamp as i64,
                stored.event_type.as_str(),
                stored.status.as_str(),
                stored.tool_call_id,
                stored.workspace,
                stored.payload_json,
                stored.source_ref,
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(stored)
    }

    pub fn event(&self, event_id: &str) -> Result<Option<ContextEvent>, String> {
        self.conn
            .query_row(
                "SELECT * FROM context_events WHERE event_id = ?1",
                params![event_id],
                row_to_event,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Incremental read for a reconnecting reader: everything after the last
    /// sequence it already has, in order.
    pub fn since(
        &self,
        session_id: &str,
        after_sequence: i64,
    ) -> Result<Vec<ContextEvent>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT * FROM context_events
                 WHERE session_id = ?1 AND sequence > ?2
                 ORDER BY sequence",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![session_id, after_sequence], row_to_event)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Tool calls that were started but never reached a terminal state. These
    /// are exactly the calls that must block automatic compaction and must not
    /// be silently re-executed.
    pub fn unresolved_tool_calls(&self, session_id: &str) -> Result<Vec<ContextEvent>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT * FROM context_events
                 WHERE session_id = ?1
                   AND event_type IN ('tool_call', 'tool_result')
                   AND status IN ('started', 'unknown')
                   AND (tool_call_id IS NULL OR tool_call_id NOT IN (
                        SELECT tool_call_id FROM context_events
                        WHERE session_id = ?1 AND tool_call_id IS NOT NULL
                          AND status IN ('completed', 'failed', 'cancelled')))
                 ORDER BY sequence",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![session_id], row_to_event)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Latest status recorded for one tool call, across retries and substitutes.
    pub fn tool_call_status(&self, tool_call_id: &str) -> Result<Option<EventStatus>, String> {
        self.conn
            .query_row(
                "SELECT status FROM context_events
                 WHERE tool_call_id = ?1 ORDER BY sequence DESC LIMIT 1",
                params![tool_call_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())
            .map(|text| text.as_deref().and_then(EventStatus::parse))
    }

    pub fn session_events(&self, session_id: &str) -> Result<Vec<ContextEvent>, String> {
        self.since(session_id, 0)
    }

    /// Everything recorded about one tool call, gathered from its start and
    /// outcome events. `status` is `Unknown` when no outcome was ever recorded —
    /// that is a real answer, not a missing one.
    pub fn tool_call_detail(&self, tool_call_id: &str) -> Result<Option<ToolCallDetail>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM context_events WHERE tool_call_id = ?1 ORDER BY sequence")
            .map_err(|e| e.to_string())?;
        let events: Vec<ContextEvent> = stmt
            .query_map(params![tool_call_id], row_to_event)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if events.is_empty() {
            return Ok(None);
        }
        let field = |key: &str| -> Option<serde_json::Value> {
            events.iter().rev().find_map(|event| {
                serde_json::from_str::<serde_json::Value>(&event.payload_json)
                    .ok()?
                    .get(key)
                    .filter(|value| !value.is_null())
                    .cloned()
            })
        };
        let outcome = events
            .iter()
            .rev()
            .find(|event| event.event_type == EventType::ToolResult);
        Ok(Some(ToolCallDetail {
            tool_call_id: tool_call_id.to_string(),
            session_id: events[0].session_id.clone(),
            turn_id: events[0].turn_id,
            tool: field("tool").and_then(|value| value.as_str().map(str::to_string)),
            status: outcome
                .map(|event| event.status)
                .unwrap_or(EventStatus::Unknown),
            exit_code: field("exit_code")
                .and_then(|value| value.as_i64())
                .map(|v| v as i32),
            result_hash: field("result_hash").and_then(|value| value.as_str().map(str::to_string)),
            workspace_revision: field("workspace_revision")
                .and_then(|value| value.as_str().map(str::to_string)),
            source_ref: outcome.and_then(|event| event.source_ref.clone()),
        }))
    }
}

/// Query answer for "what happened with this tool call".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallDetail {
    pub tool_call_id: String,
    pub session_id: String,
    pub turn_id: Option<u64>,
    pub tool: Option<String>,
    pub status: EventStatus,
    /// Present only when the tool reported one; absence is not success.
    pub exit_code: Option<i32>,
    pub result_hash: Option<String>,
    pub workspace_revision: Option<String>,
    pub source_ref: Option<String>,
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextEvent> {
    let event_type: String = row.get("event_type")?;
    let status: String = row.get("status")?;
    Ok(ContextEvent {
        event_id: row.get("event_id")?,
        session_id: row.get("session_id")?,
        task_id: row.get("task_id")?,
        agent_id: row.get("agent_id")?,
        parent_agent_id: row.get("parent_agent_id")?,
        thread_id: row.get("thread_id")?,
        turn_id: row.get::<_, Option<i64>>("turn_id")?.map(|v| v as u64),
        sequence: row.get("sequence")?,
        timestamp: row.get::<_, i64>("timestamp")? as u64,
        // An unreadable discriminant becomes `Unknown` rather than dropping the
        // row: losing a fact is worse than losing its label.
        event_type: EventType::parse(&event_type).unwrap_or(EventType::Checkpoint),
        status: EventStatus::parse(&status).unwrap_or(EventStatus::Unknown),
        tool_call_id: row.get("tool_call_id")?,
        workspace: row.get("workspace")?,
        payload_json: row.get("payload_json")?,
        source_ref: row.get("source_ref")?,
    })
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(id: &str, session: &str, kind: EventType, status: EventStatus) -> ContextEvent {
        ContextEvent::new(id, session, kind, status)
    }

    #[test]
    fn assigns_per_session_sequences_and_reads_incrementally() {
        let store = ContextStore::open_in_memory().unwrap();
        for id in ["a1", "a2", "a3"] {
            store
                .append(&event(
                    id,
                    "s1",
                    EventType::UserMessage,
                    EventStatus::Completed,
                ))
                .unwrap();
        }
        store
            .append(&event(
                "b1",
                "s2",
                EventType::UserMessage,
                EventStatus::Completed,
            ))
            .unwrap();

        let s1 = store.session_events("s1").unwrap();
        assert_eq!(
            s1.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // A second session starts its own sequence; sub-agents cannot collide.
        assert_eq!(store.session_events("s2").unwrap()[0].sequence, 1);
        let tail = store.since("s1", 1).unwrap();
        assert_eq!(
            tail.iter().map(|e| e.event_id.as_str()).collect::<Vec<_>>(),
            vec!["a2", "a3"]
        );
    }

    #[test]
    fn replaying_the_same_event_id_does_not_create_a_second_fact() {
        let store = ContextStore::open_in_memory().unwrap();
        let first = store
            .append(&event("w1", "s", EventType::ToolCall, EventStatus::Started))
            .unwrap();
        // A crash-and-replay resends the same id with a different status. The
        // original fact wins; a write must not be counted twice.
        let replay = store
            .append(&event(
                "w1",
                "s",
                EventType::ToolCall,
                EventStatus::Completed,
            ))
            .unwrap();
        assert_eq!(replay.sequence, first.sequence);
        assert_eq!(replay.status, EventStatus::Started);
        assert_eq!(store.session_events("s").unwrap().len(), 1);
    }

    #[test]
    fn unresolved_tool_calls_exclude_calls_that_reached_a_terminal_state() {
        let store = ContextStore::open_in_memory().unwrap();
        store
            .append(
                &event("c1", "s", EventType::ToolCall, EventStatus::Started)
                    .with_tool_call(Some("call-done".into())),
            )
            .unwrap();
        store
            .append(
                &event("c2", "s", EventType::ToolResult, EventStatus::Completed)
                    .with_tool_call(Some("call-done".into())),
            )
            .unwrap();
        store
            .append(
                &event("c3", "s", EventType::ToolCall, EventStatus::Started)
                    .with_tool_call(Some("call-open".into())),
            )
            .unwrap();
        store
            .append(
                &event("c4", "s", EventType::ToolCall, EventStatus::Unknown)
                    .with_tool_call(Some("call-unknown".into())),
            )
            .unwrap();

        let open = store.unresolved_tool_calls("s").unwrap();
        let ids: Vec<_> = open
            .iter()
            .filter_map(|e| e.tool_call_id.as_deref())
            .collect();
        assert_eq!(ids, vec!["call-open", "call-unknown"]);
        assert_eq!(
            store.tool_call_status("call-done").unwrap(),
            Some(EventStatus::Completed)
        );
        assert_eq!(
            store.tool_call_status("call-unknown").unwrap(),
            Some(EventStatus::Unknown)
        );
    }

    #[test]
    fn out_of_order_writes_keep_a_readable_order_and_survive_reopen() {
        let dir = crate::test_support::unique_temp_dir("context_store_reopen");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.sqlite3");
        {
            let store = ContextStore::open(&path).unwrap();
            store
                .append(
                    &event("late", "s", EventType::ToolResult, EventStatus::Completed)
                        .with_payload(&json!({"note": "arrived first"})),
                )
                .unwrap();
            store
                .append(&event(
                    "early",
                    "s",
                    EventType::ToolCall,
                    EventStatus::Started,
                ))
                .unwrap();
        }
        let reopened = ContextStore::open(&path).unwrap();
        let events = reopened.session_events("s").unwrap();
        // Order is the store's arrival sequence, not the caller's guess.
        assert_eq!(
            events
                .iter()
                .map(|e| e.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["late", "early"]
        );
        assert!(events[0].payload_json.contains("arrived first"));
        // A later append continues the same sequence after reopen.
        let next = reopened
            .append(&event(
                "third",
                "s",
                EventType::Checkpoint,
                EventStatus::Completed,
            ))
            .unwrap();
        assert_eq!(next.sequence, 3);
    }

    #[test]
    fn events_from_different_agents_stay_separable() {
        let store = ContextStore::open_in_memory().unwrap();
        store
            .append(
                &event("p1", "s", EventType::ToolCall, EventStatus::Completed)
                    .with_agent(Some("parent".into()), None),
            )
            .unwrap();
        store
            .append(
                &event("w1", "s", EventType::ToolCall, EventStatus::Completed)
                    .with_agent(Some("worker".into()), Some("parent".into())),
            )
            .unwrap();

        let events = store.session_events("s").unwrap();
        let worker = events.iter().find(|e| e.event_id == "w1").unwrap();
        assert_eq!(worker.agent_id.as_deref(), Some("worker"));
        assert_eq!(worker.parent_agent_id.as_deref(), Some("parent"));
    }
}
