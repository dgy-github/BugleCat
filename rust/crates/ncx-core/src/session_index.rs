//! Browsable session history and frozen snapshots.
//!
//! The workspace JSONL log is for `--resume`; this global index is for a
//! human-facing directory of conversations. Each conversation has one summary
//! row keyed by a session id, plus a snapshot file with the full transcript.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::session::{redact_image_data, Session};

const TITLE_MAX: usize = 120;
const SNIPPET_MAX: usize = 200;

static SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: String,
    pub workspace: String,
    pub title: String,
    pub snippet: String,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub recent_tools: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub log_path: String,
    pub has_snapshot: bool,
    pub archived: bool,
}

impl SessionSummary {
    fn to_value(&self) -> Value {
        json!({
            "session_id": self.session_id,
            "workspace": self.workspace,
            "title": self.title,
            "snippet": self.snippet,
            "user_messages": self.user_messages,
            "assistant_messages": self.assistant_messages,
            "tool_calls": self.tool_calls,
            "recent_tools": self.recent_tools,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "log_path": self.log_path,
            "has_snapshot": self.has_snapshot,
            "archived": self.archived,
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        let workspace = value
            .get("workspace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let session_id = value
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                if workspace.is_empty() {
                    None
                } else {
                    Some(format!("legacy:{workspace}"))
                }
            })?;
        Some(SessionSummary {
            session_id,
            workspace,
            title: string_field(value, "title"),
            snippet: string_field(value, "snippet"),
            user_messages: usize_field(value, "user_messages"),
            assistant_messages: usize_field(value, "assistant_messages"),
            tool_calls: usize_field(value, "tool_calls"),
            recent_tools: value
                .get("recent_tools")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            created_at: string_field(value, "created_at"),
            updated_at: string_field(value, "updated_at"),
            log_path: string_field(value, "log_path"),
            has_snapshot: value
                .get("has_snapshot")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            archived: value.get("archived").and_then(|v| v.as_bool()).unwrap_or(false),
        })
    }
}

pub struct SessionIndex {
    path: PathBuf,
    snapshots_dir: PathBuf,
    by_id: HashMap<String, SessionSummary>,
}

impl Default for SessionIndex {
    fn default() -> Self {
        Self::new(default_index_path())
    }
}

impl SessionIndex {
    pub fn new(path: PathBuf) -> Self {
        let snapshots_dir = path
            .parent()
            .map(|p| p.join("snapshots"))
            .unwrap_or_else(|| PathBuf::from("snapshots"));
        let mut index = SessionIndex {
            path,
            snapshots_dir,
            by_id: HashMap::new(),
        };
        index.load();
        index
    }

    pub fn entries(&self) -> Vec<SessionSummary> {
        let mut out = self.by_id.values().cloned().collect::<Vec<_>>();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out
    }

    pub fn get(&self, session_id: &str) -> Option<&SessionSummary> {
        self.by_id.get(session_id)
    }

    pub fn record(&mut self, summary: SessionSummary) {
        self.by_id.insert(summary.session_id.clone(), summary);
        self.save();
    }

    pub fn record_turn(
        &mut self,
        session_id: &str,
        workspace: &Path,
        session: &Session,
        log_path: &Path,
    ) -> SessionSummary {
        let prior_created = self.by_id.get(session_id).map(|s| s.created_at.clone());
        let prior_archived = self.by_id.get(session_id).map(|s| s.archived).unwrap_or(false);
        let saved = self.save_snapshot(session_id, session);
        let mut summary = summarize(
            session_id,
            &workspace.display().to_string(),
            &session.full_messages(),
            &log_path.display().to_string(),
            Some(now_stamp()),
            prior_created.as_deref(),
            saved,
        );
        summary.archived = prior_archived; // archiving survives new turns
        self.record(summary.clone());
        summary
    }

    /// Set a session's archived flag (persists). Returns false if unknown.
    pub fn set_archived(&mut self, session_id: &str, archived: bool) -> bool {
        match self.by_id.get_mut(session_id) {
            Some(s) => {
                s.archived = archived;
                self.save();
                true
            }
            None => false,
        }
    }

    pub fn snapshot_path(&self, session_id: &str) -> PathBuf {
        self.snapshots_dir
            .join(format!("{}.json", safe_file_stem(session_id)))
    }

    pub fn save_snapshot(&self, session_id: &str, session: &Session) -> bool {
        let payload = json!({
            "session_id": session_id,
            "messages": redact_messages(&session.full_messages(), "[image omitted from snapshot]"),
        });
        if fs::create_dir_all(&self.snapshots_dir).is_err() {
            return false;
        }
        serde_json::to_string(&payload)
            .ok()
            .and_then(|text| fs::write(self.snapshot_path(session_id), text).ok())
            .is_some()
    }

    pub fn load_snapshot(&self, session_id: &str) -> Option<Vec<Value>> {
        let text = fs::read_to_string(self.snapshot_path(session_id)).ok()?;
        let value = serde_json::from_str::<Value>(&text).ok()?;
        value.get("messages")?.as_array().cloned()
    }

    fn load(&mut self) {
        let Ok(text) = fs::read_to_string(&self.path) else {
            return;
        };
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            let Some(summary) = SessionSummary::from_value(&value) else {
                continue;
            };
            self.by_id.insert(summary.session_id.clone(), summary);
        }
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let lines = self
            .entries()
            .iter()
            .filter_map(|s| serde_json::to_string(&s.to_value()).ok())
            .collect::<Vec<_>>();
        let text = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        let _ = fs::write(&self.path, text);
    }
}

pub fn new_session_id() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{n:x}{:x}{seq:x}", std::process::id())
}

pub fn default_index_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".nanocodex").join("sessions.jsonl")
}

pub fn summarize(
    session_id: &str,
    workspace: &str,
    messages: &[Value],
    log_path: &str,
    now: Option<String>,
    created_at: Option<&str>,
    has_snapshot: bool,
) -> SessionSummary {
    let mut title = String::new();
    let mut snippet = String::new();
    let mut user_messages = 0;
    let mut assistant_messages = 0;
    let mut tool_calls = 0;
    let mut recent_tools = Vec::new();

    for msg in messages {
        match msg.get("role").and_then(|v| v.as_str()) {
            Some("user") => {
                user_messages += 1;
                if title.is_empty() {
                    let text = first_text(msg.get("content"));
                    if !text.is_empty() && !text.starts_with("[Earlier conversation") {
                        title = clip(&text, TITLE_MAX);
                    }
                }
            }
            Some("assistant") => {
                assistant_messages += 1;
                let text = first_text(msg.get("content"));
                if !text.is_empty() {
                    snippet = clip(&text, SNIPPET_MAX);
                }
                if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    tool_calls += calls.len();
                    for call in calls {
                        if let Some(name) = call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                        {
                            recent_tools.push(name.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if recent_tools.len() > 8 {
        recent_tools = recent_tools[recent_tools.len() - 8..].to_vec();
    }
    let now = now.unwrap_or_else(now_stamp);
    SessionSummary {
        session_id: session_id.to_string(),
        workspace: workspace.to_string(),
        title: if title.is_empty() {
            "(no prompt yet)".into()
        } else {
            title
        },
        snippet,
        user_messages,
        assistant_messages,
        tool_calls,
        recent_tools,
        created_at: created_at.unwrap_or(&now).to_string(),
        updated_at: now,
        log_path: log_path.to_string(),
        has_snapshot,
        archived: false,
    }
}

fn first_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn clip(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let take = limit.saturating_sub(3);
    format!("{}...", collapsed.chars().take(take).collect::<String>())
}

fn redact_messages(messages: &[Value], placeholder: &str) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| redact_image_data(msg, placeholder))
        .collect()
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn usize_field(value: &Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(|v| v.as_u64())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

fn safe_file_stem(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:013}", d.as_millis()))
        .unwrap_or_else(|_| "0000000000000".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ncx_session_index_{name}_{}", now_stamp()))
    }

    fn msgs() -> Vec<Value> {
        vec![
            json!({"role": "system", "content": "sys"}),
            json!({"role": "user", "content": "fix login"}),
            json!({"role": "assistant", "content": "looking", "tool_calls": [
                {"id": "1", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}
            ]}),
            json!({"role": "tool", "tool_call_id": "1", "name": "read_file", "content": "..."}),
            json!({"role": "assistant", "content": "fixed"}),
        ]
    }

    #[test]
    fn summarize_pulls_title_snippet_counts_and_tools() {
        let s = summarize(
            "sid",
            "/proj",
            &msgs(),
            "/proj/.nanocodex/session.jsonl",
            Some("2026-06-01T10:00:00".into()),
            None,
            true,
        );
        assert_eq!(s.title, "fix login");
        assert_eq!(s.snippet, "fixed");
        assert_eq!(s.user_messages, 1);
        assert_eq!(s.assistant_messages, 2);
        assert_eq!(s.tool_calls, 1);
        assert_eq!(s.recent_tools, vec!["read_file"]);
        assert_eq!(s.created_at, "2026-06-01T10:00:00");
        assert!(s.has_snapshot);
    }

    #[test]
    fn index_upserts_and_sorts_newest_first() {
        let path = tmp_path("sort").join("sessions.jsonl");
        let mut idx = SessionIndex::new(path);
        idx.record(summarize(
            "old",
            "/p",
            &msgs(),
            "",
            Some("2026-06-01T09:00:00".into()),
            None,
            false,
        ));
        idx.record(summarize(
            "new",
            "/p",
            &msgs(),
            "",
            Some("2026-06-01T11:00:00".into()),
            None,
            false,
        ));
        idx.record(summarize(
            "old",
            "/p",
            &msgs(),
            "",
            Some("2026-06-01T12:00:00".into()),
            Some("2026-06-01T09:00:00"),
            false,
        ));

        let entries = idx.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id, "old");
        assert_eq!(entries[0].created_at, "2026-06-01T09:00:00");
        assert_eq!(entries[1].session_id, "new");
    }

    #[test]
    fn persists_and_loads_legacy_rows() {
        let dir = tmp_path("legacy");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.jsonl");
        std::fs::write(
            &path,
            "{\"workspace\":\"/old\",\"title\":\"legacy\",\"updated_at\":\"2026\"}\nnot json\n",
        )
        .unwrap();

        let idx = SessionIndex::new(path);
        let entries = idx.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, "legacy:/old");
        assert_eq!(entries[0].title, "legacy");
    }

    #[test]
    fn snapshot_round_trip_redacts_image_data() {
        let dir = tmp_path("snapshot");
        let mut idx = SessionIndex::new(dir.join("sessions.jsonl"));
        let mut session = Session::new("sys");
        session.add_user(json!([
            {"type": "text", "text": "describe"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ]));

        idx.record_turn(
            "sid",
            Path::new("/p"),
            &session,
            Path::new("/p/.nanocodex/session.jsonl"),
        );
        let loaded = idx.load_snapshot("sid").unwrap();
        let text = serde_json::to_string(&loaded).unwrap();
        assert!(text.contains("[image omitted from snapshot]"));
        assert!(!text.contains("data:image"));
        assert!(idx.get("sid").unwrap().has_snapshot);
    }

    #[test]
    fn session_ids_are_unique() {
        assert_ne!(new_session_id(), new_session_id());
    }
}
