//! Read-only Harness-style queries over the existing session index and snapshots.

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::session_index::{SessionIndex, SessionSummary};
use crate::tools::{Tool, ToolContext};

const MAX_RESULTS: usize = 100;

pub fn session_query_tools() -> Vec<Box<dyn Tool>> {
    [
        "session_search",
        "session_trace",
        "session_event_read",
        "session_event_search",
        "session_event_trace",
    ]
    .into_iter()
    .map(|name| Box::new(SessionQueryTool::new(name, None)) as Box<dyn Tool>)
    .collect()
}

struct SessionQueryTool {
    name: &'static str,
    index_path: Option<PathBuf>,
}

impl SessionQueryTool {
    fn new(name: &'static str, index_path: Option<PathBuf>) -> Self {
        Self { name, index_path }
    }

    fn index(&self) -> SessionIndex {
        self.index_path
            .clone()
            .map(SessionIndex::new)
            .unwrap_or_default()
    }
}

#[async_trait(?Send)]
impl Tool for SessionQueryTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        match self.name {
            "session_search" => {
                "Search saved session titles, snippets, workspaces, and recent tools."
            }
            "session_trace" => "Return metadata and message counts for one saved session.",
            "session_event_read" => {
                "Read a bounded page of redacted messages from one saved session."
            }
            "session_event_search" => "Search redacted messages inside one saved session.",
            _ => "Return a compact role and tool-call trace for one saved session.",
        }
    }

    fn parameters(&self) -> Value {
        match self.name {
            "session_search" => json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS}
                },
                "required": ["query"]
            }),
            "session_event_search" => json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS}
                },
                "required": ["session_id", "query"]
            }),
            "session_event_read" => json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "offset": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": MAX_RESULTS}
                },
                "required": ["session_id"]
            }),
            _ => json!({
                "type": "object",
                "properties": {"session_id": {"type": "string"}},
                "required": ["session_id"]
            }),
        }
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        match self.name {
            "session_search" => search_sessions(&self.index(), args),
            "session_trace" => session_trace(&self.index(), args),
            "session_event_read" => read_events(&self.index(), args),
            "session_event_search" => search_events(&self.index(), args),
            _ => event_trace(&self.index(), args),
        }
    }
}

fn search_sessions(index: &SessionIndex, args: &Value) -> String {
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return "Error: 'query' is required and must be a string.".into();
    };
    let query = query.to_ascii_lowercase();
    let limit = limit(args);
    let rows = index
        .entries()
        .into_iter()
        .filter(|entry| summary_text(entry).to_ascii_lowercase().contains(&query))
        .take(limit)
        .map(summary_json)
        .collect::<Vec<_>>();
    json!({"sessions": rows}).to_string()
}

fn session_trace(index: &SessionIndex, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    match index.get(id) {
        Some(summary) => summary_json(summary.clone()).to_string(),
        None => format!("Error: unknown session '{id}'."),
    }
}

fn read_events(index: &SessionIndex, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let Some(messages) = index.load_snapshot(id) else {
        return format!("Error: session snapshot not found: {id}");
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let page = messages
        .into_iter()
        .skip(offset)
        .take(limit(args))
        .collect::<Vec<_>>();
    json!({"session_id": id, "offset": offset, "messages": page}).to_string()
}

fn search_events(index: &SessionIndex, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return "Error: 'query' is required and must be a string.".into();
    };
    let Some(messages) = index.load_snapshot(id) else {
        return format!("Error: session snapshot not found: {id}");
    };
    let query = query.to_ascii_lowercase();
    let matches = messages
        .into_iter()
        .enumerate()
        .filter(|(_, message)| message.to_string().to_ascii_lowercase().contains(&query))
        .take(limit(args))
        .map(|(index, message)| json!({"index": index, "message": message}))
        .collect::<Vec<_>>();
    json!({"session_id": id, "matches": matches}).to_string()
}

fn event_trace(index: &SessionIndex, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let Some(messages) = index.load_snapshot(id) else {
        return format!("Error: session snapshot not found: {id}");
    };
    let trace = messages.iter().enumerate().map(|(index, message)| json!({
        "index": index,
        "role": message.get("role").and_then(Value::as_str).unwrap_or("unknown"),
        "tool_calls": message.get("tool_calls").and_then(Value::as_array).map(Vec::len).unwrap_or(0)
    })).collect::<Vec<_>>();
    json!({"session_id": id, "trace": trace}).to_string()
}

fn summary_json(entry: SessionSummary) -> Value {
    json!({"session_id": entry.session_id, "workspace": entry.workspace, "title": entry.title,
        "snippet": entry.snippet, "user_messages": entry.user_messages,
        "assistant_messages": entry.assistant_messages, "tool_calls": entry.tool_calls,
        "recent_tools": entry.recent_tools, "updated_at": entry.updated_at, "archived": entry.archived})
}

fn summary_text(entry: &SessionSummary) -> String {
    format!(
        "{} {} {} {}",
        entry.title,
        entry.snippet,
        entry.workspace,
        entry.recent_tools.join(" ")
    )
}

fn session_id(args: &Value) -> Option<&str> {
    args.get("session_id").and_then(Value::as_str)
}
fn missing_session_id() -> String {
    "Error: 'session_id' is required and must be a string.".into()
}
fn limit(args: &Value) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, MAX_RESULTS as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};

    #[tokio::test]
    async fn searches_and_reads_redacted_snapshots() {
        let root = std::env::temp_dir().join("ncx_session_query_tools");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("index.jsonl");
        let mut index = SessionIndex::new(path.clone());
        let mut session = Session::new("system");
        session.add_user(Value::String("find the widget".into()));
        session.add_assistant("widget located", None, "");
        index.record_turn("s1", &root, &session, &root.join("session.jsonl"));
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));

        let search = SessionQueryTool::new("session_search", Some(path.clone()))
            .execute(&ctx, &json!({"query": "widget"}))
            .await;
        assert!(search.contains("s1"), "{search}");
        let events = SessionQueryTool::new("session_event_search", Some(path))
            .execute(&ctx, &json!({"session_id": "s1", "query": "located"}))
            .await;
        assert!(events.contains("widget located"), "{events}");
    }
}
