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

use crate::session::{redact_image_data, Session, COMPACTED_HISTORY_PREFIX};

const TITLE_MAX: usize = 36;
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
            archived: value
                .get("archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
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
        // Sort by parsed epoch-ms, not raw string: the index mixes 13-digit
        // ms-epoch timestamps with legacy ISO strings, and comparing those as
        // strings mis-sorted the (older) ISO entries to the top.
        out.sort_by_key(|entry| std::cmp::Reverse(parse_ts_ms(&entry.updated_at)));
        out
    }

    pub fn get(&self, session_id: &str) -> Option<&SessionSummary> {
        self.by_id.get(session_id)
    }

    /// Return the newest unarchived snapshot that belongs to `workspace`.
    /// Used by the desktop app to continue the visible conversation after a
    /// restart instead of silently creating a second, identically titled task.
    pub fn latest_resumable_for_workspace(
        &self,
        workspace: &Path,
    ) -> Option<(SessionSummary, Vec<Value>)> {
        let wanted = normalized_workspace(workspace);
        self.entries().into_iter().find_map(|summary| {
            if summary.archived
                || !summary.has_snapshot
                || normalized_workspace(Path::new(&summary.workspace)) != wanted
            {
                return None;
            }
            self.load_snapshot(&summary.session_id)
                .map(|messages| (summary, messages))
        })
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
        self.record_turn_with_title(session_id, workspace, session, log_path, None)
    }

    pub fn record_turn_with_title(
        &mut self,
        session_id: &str,
        workspace: &Path,
        session: &Session,
        log_path: &Path,
        title_override: Option<&str>,
    ) -> SessionSummary {
        let prior_created = self.by_id.get(session_id).map(|s| s.created_at.clone());
        let prior_title = self.by_id.get(session_id).and_then(|summary| {
            (summary.user_messages > 0 && !summary.title.trim().is_empty())
                .then(|| summary.title.clone())
        });
        let prior_archived = self
            .by_id
            .get(session_id)
            .map(|s| s.archived)
            .unwrap_or(false);
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
        if let Some(title) = title_override
            .filter(|title| !title.trim().is_empty())
            .map(|title| clip(title, TITLE_MAX))
            .or(prior_title)
        {
            summary.title = title;
        }
        summary.archived = prior_archived; // archiving survives new turns
        self.record(summary.clone());
        summary
    }

    pub fn set_title(&mut self, session_id: &str, title: &str) -> bool {
        let title = clip(title, TITLE_MAX);
        if title.is_empty() {
            return false;
        }
        match self.by_id.get_mut(session_id) {
            Some(summary) => {
                summary.title = title;
                self.save();
                true
            }
            None => false,
        }
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

fn normalized_workspace(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let without_verbatim = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let normalized = PathBuf::from(without_verbatim)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(without_verbatim))
        .to_string_lossy()
        .replace('\\', "/");
    if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    }
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
                let text = first_text(msg.get("content"));
                if text.starts_with(COMPACTED_HISTORY_PREFIX) {
                    continue;
                }
                user_messages += 1;
                if title.is_empty()
                    && !text.is_empty()
                    && !text.starts_with("[Earlier conversation")
                {
                    title = fallback_title(&text);
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

fn fallback_title(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let markers = [
        "可以帮我",
        "请帮我",
        "请你",
        "帮我",
        "麻烦",
        "能否",
        "我需要",
        "我想",
        "请",
    ];
    let request = markers
        .iter()
        .filter_map(|marker| collapsed.rfind(marker).map(|index| (index, *marker)))
        .max_by_key(|(index, _)| *index)
        .map(|(index, marker)| collapsed[index + marker.len()..].trim())
        .filter(|request| !request.is_empty())
        .unwrap_or(&collapsed);
    clip(request, TITLE_MAX)
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

/// Parse a stored timestamp to epoch milliseconds for ordering. Handles the
/// current 13-digit ms-epoch strings AND legacy ISO `YYYY-MM-DDTHH:MM:SS` values
/// (comparing the two as raw strings mis-sorted the ISO entries to the top).
fn parse_ts_ms(s: &str) -> i64 {
    let s = s.trim();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse::<i64>().unwrap_or(0);
    }
    // Legacy ISO: pull out the numeric fields (Y M D [H M S]).
    let f: Vec<i64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| (!p.is_empty()).then(|| p.parse::<i64>().ok()).flatten())
        .collect();
    if f.len() < 3 {
        return 0;
    }
    let (y, mo, d) = (f[0], f[1], f[2]);
    let (h, mi, sec) = (
        f.get(3).copied().unwrap_or(0),
        f.get(4).copied().unwrap_or(0),
        f.get(5).copied().unwrap_or(0),
    );
    // days_from_civil (Howard Hinnant), proleptic Gregorian, epoch 1970-01-01.
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = (if yy >= 0 { yy } else { yy - 399 }) / 400;
    let yoe = yy - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    (days * 86400 + h * 3600 + mi * 60 + sec) * 1000
}

#[cfg(test)]
#[path = "session_index_tests.rs"]
mod tests;
