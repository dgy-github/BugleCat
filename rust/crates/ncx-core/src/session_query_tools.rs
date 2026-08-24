//! Read-only Harness-style queries over the versioned Thread/Turn store.

use std::path::PathBuf;
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ncx_protocol::{Thread, ThreadId, ThreadItem, ThreadMetadata};
use ncx_thread_store::{default_thread_store_path, JsonThreadStore, ThreadStore};
use serde_json::{json, Value};

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
    store_path: Option<PathBuf>,
}

impl SessionQueryTool {
    fn new(name: &'static str, store_path: Option<PathBuf>) -> Self {
        Self { name, store_path }
    }

    fn store(&self) -> Result<JsonThreadStore, String> {
        let path = self
            .store_path
            .clone()
            .unwrap_or_else(default_thread_store_path);
        JsonThreadStore::open(path).map_err(|error| error.to_string())
    }
}

#[async_trait(?Send)]
impl Tool for SessionQueryTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        match self.name {
            "session_search" => "Search saved thread titles, workspaces, and visible messages.",
            "session_trace" => "Return metadata and turn/item counts for one saved thread.",
            "session_event_read" => {
                "Read a bounded page containing only user messages and final answers."
            }
            "session_event_search" => "Search visible messages inside one saved thread.",
            _ => "Return a compact visible role trace for one saved thread.",
        }
    }

    fn parameters(&self) -> Value {
        match self.name {
            "session_search" => {
                json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":MAX_RESULTS}},"required":["query"]})
            }
            "session_event_search" => {
                json!({"type":"object","properties":{"session_id":{"type":"string"},"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":MAX_RESULTS}},"required":["session_id","query"]})
            }
            "session_event_read" => {
                json!({"type":"object","properties":{"session_id":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":MAX_RESULTS}},"required":["session_id"]})
            }
            _ => {
                json!({"type":"object","properties":{"session_id":{"type":"string"}},"required":["session_id"]})
            }
        }
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        let store = match self.store() {
            Ok(store) => store,
            Err(error) => return format!("Error: thread store unavailable: {error}"),
        };
        match self.name {
            "session_search" => search_sessions(&store, args),
            "session_trace" => session_trace(&store, args),
            "session_event_read" => read_events(&store, args),
            "session_event_search" => search_events(&store, args),
            _ => event_trace(&store, args),
        }
    }
}

fn search_sessions(store: &JsonThreadStore, args: &Value) -> String {
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return "Error: 'query' is required and must be a string.".into();
    };
    let query = query.to_ascii_lowercase();
    let rows = match list_threads(store) {
        Ok(rows) => rows,
        Err(error) => return error,
    };
    let matches = rows
        .into_iter()
        .filter_map(|metadata| {
            let thread = read_visible_thread(store, metadata.id.as_str()).ok()?;
            summary_text(&thread)
                .to_ascii_lowercase()
                .contains(&query)
                .then(|| summary_json(&thread))
        })
        .take(limit(args))
        .collect::<Vec<_>>();
    json!({"sessions":matches}).to_string()
}

fn session_trace(store: &JsonThreadStore, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    match read_visible_thread(store, id) {
        Ok(thread) => summary_json(&thread).to_string(),
        Err(_) => format!("Error: unknown session '{id}'."),
    }
}

fn read_events(store: &JsonThreadStore, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let messages = match visible_messages(store, id) {
        Ok(messages) => messages,
        Err(error) => return error,
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let page = messages
        .into_iter()
        .skip(offset)
        .take(limit(args))
        .collect::<Vec<_>>();
    json!({"session_id":id,"offset":offset,"messages":page}).to_string()
}

fn search_events(store: &JsonThreadStore, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return "Error: 'query' is required and must be a string.".into();
    };
    let messages = match visible_messages(store, id) {
        Ok(messages) => messages,
        Err(error) => return error,
    };
    let query = query.to_ascii_lowercase();
    let matches = messages
        .into_iter()
        .enumerate()
        .filter(|(_, message)| message.to_string().to_ascii_lowercase().contains(&query))
        .take(limit(args))
        .map(|(index, message)| json!({"index":index,"message":message}))
        .collect::<Vec<_>>();
    json!({"session_id":id,"matches":matches}).to_string()
}

fn event_trace(store: &JsonThreadStore, args: &Value) -> String {
    let Some(id) = session_id(args) else {
        return missing_session_id();
    };
    let messages = match visible_messages(store, id) {
        Ok(messages) => messages,
        Err(error) => return error,
    };
    let trace = messages.iter().enumerate().map(|(index,message)| json!({"index":index,"role":message.get("role").and_then(Value::as_str).unwrap_or("unknown")})).collect::<Vec<_>>();
    json!({"session_id":id,"trace":trace}).to_string()
}

fn list_threads(store: &JsonThreadStore) -> Result<Vec<ThreadMetadata>, String> {
    store.list(false).map_err(|error| format!("Error: {error}"))
}

fn read_visible_thread(store: &JsonThreadStore, id: &str) -> Result<Thread, String> {
    let thread_id = ThreadId::new(id.to_string()).map_err(|error| format!("Error: {error}"))?;
    store
        .read(&thread_id)
        .map_err(|error| format!("Error: {error}"))?
        .map(Thread::into_visible)
        .ok_or_else(|| format!("Error: unknown session '{id}'."))
}

fn visible_messages(store: &JsonThreadStore, id: &str) -> Result<Vec<Value>, String> {
    Ok(read_visible_thread(store, id)?
        .turns
        .into_iter()
        .flat_map(|turn| turn.items)
        .filter_map(|item| match item {
            ThreadItem::UserMessage { text, .. } => Some(json!({"role":"user","content":text})),
            ThreadItem::AssistantMessage { text, .. } => {
                Some(json!({"role":"assistant","content":text}))
            }
            _ => None,
        })
        .collect())
}

fn summary_json(thread: &Thread) -> Value {
    let items = thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .collect::<Vec<_>>();
    let user_messages = items
        .iter()
        .filter(|item| matches!(item, ThreadItem::UserMessage { .. }))
        .count();
    let assistant_messages = items
        .iter()
        .filter(|item| matches!(item, ThreadItem::AssistantMessage { .. }))
        .count();
    json!({"session_id":thread.metadata.id,"workspace":thread.metadata.workspace,"title":thread.metadata.title,"turns":thread.turns.len(),"user_messages":user_messages,"assistant_messages":assistant_messages,"updated_at":thread.metadata.updated_at,"archived":thread.metadata.archived})
}

fn summary_text(thread: &Thread) -> String {
    let content = thread
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter_map(|item| match item {
            ThreadItem::UserMessage { text, .. } | ThreadItem::AssistantMessage { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{} {} {}",
        thread.metadata.title, thread.metadata.workspace, content
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
fn now_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_protocol::{ItemId, Turn, TurnId, TurnStatus, TurnUsage};
    use ncx_sandbox::{SandboxPolicy, READ_ONLY};
    use ncx_thread_store::ThreadStore;

    #[tokio::test]
    async fn searches_visible_thread_projection_without_tool_logs() {
        let root = std::env::temp_dir().join(format!("ncx_session_query_{}", now_epoch_millis()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("threads-v2.json");
        let store = JsonThreadStore::open(&path).unwrap();
        store
            .create(Thread {
                metadata: ThreadMetadata {
                    id: ThreadId::new("s1").unwrap(),
                    workspace: root.display().to_string(),
                    title: "find widget".into(),
                    archived: false,
                    created_at: 1,
                    updated_at: 2,
                },
                turns: vec![Turn {
                    id: TurnId::new("t1").unwrap(),
                    status: TurnStatus::Completed,
                    items: vec![
                        ThreadItem::UserMessage {
                            id: ItemId::new("u1").unwrap(),
                            text: "find the widget".into(),
                        },
                        ThreadItem::ToolResult {
                            id: ItemId::new("r1").unwrap(),
                            call_id: ItemId::new("c1").unwrap(),
                            output: "SECRET_TOOL_LOG".into(),
                            success: true,
                        },
                        ThreadItem::AssistantMessage {
                            id: ItemId::new("a1").unwrap(),
                            text: "widget located".into(),
                        },
                    ],
                    started_at: 1,
                    completed_at: Some(2),
                    error: None,
                    usage: TurnUsage::default(),
                }],
            })
            .unwrap();
        let ctx = ToolContext::new(root.clone(), SandboxPolicy::new(READ_ONLY, &root));
        let search = SessionQueryTool::new("session_search", Some(path.clone()))
            .execute(&ctx, &json!({"query":"widget"}))
            .await;
        assert!(search.contains("s1"), "{search}");
        let events = SessionQueryTool::new("session_event_search", Some(path))
            .execute(&ctx, &json!({"session_id":"s1","query":"located"}))
            .await;
        assert!(events.contains("widget located"), "{events}");
        assert!(!events.contains("SECRET_TOOL_LOG"), "{events}");
        let _ = std::fs::remove_dir_all(root);
    }
}
