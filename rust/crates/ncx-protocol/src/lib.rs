//! Versioned client/server contracts for nanocodex threads and turns.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

pub const PROTOCOL_VERSION: u32 = 2;

macro_rules! durable_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ProtocolError::InvalidId($label));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

durable_id!(ThreadId, "threadId");
durable_id!(TurnId, "turnId");
durable_id!(ItemId, "itemId");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMetadata {
    pub id: ThreadId,
    pub workspace: String,
    pub title: String,
    pub archived: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ThreadItem {
    UserMessage {
        id: ItemId,
        text: String,
    },
    AssistantMessage {
        id: ItemId,
        text: String,
    },
    Reasoning {
        id: ItemId,
        summary: String,
    },
    ToolCall {
        id: ItemId,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: ItemId,
        call_id: ItemId,
        output: String,
        success: bool,
    },
    ContextCompaction {
        id: ItemId,
        summary: String,
        dropped_items: u32,
    },
}

impl ThreadItem {
    pub fn id(&self) -> &ItemId {
        match self {
            Self::UserMessage { id, .. }
            | Self::AssistantMessage { id, .. }
            | Self::Reasoning { id, .. }
            | Self::ToolCall { id, .. }
            | Self::ToolResult { id, .. }
            | Self::ContextCompaction { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    #[serde(default)]
    pub tokens: BTreeMap<String, i64>,
    #[serde(default)]
    pub estimated_cost: Option<f64>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: TurnId,
    pub status: TurnStatus,
    pub items: Vec<ThreadItem>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    #[serde(default)]
    pub usage: TurnUsage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub metadata: ThreadMetadata,
    pub turns: Vec<Turn>,
}

impl Thread {
    /// Project a durable thread to the transcript safe for history clients.
    /// Every turn retains its user request and only its final assistant answer;
    /// tool traffic, reasoning, compaction details, and intermediate answers are removed.
    pub fn into_visible(mut self) -> Self {
        for turn in &mut self.turns {
            let mut visible = turn
                .items
                .iter()
                .filter(|item| matches!(item, ThreadItem::UserMessage { .. }))
                .cloned()
                .collect::<Vec<_>>();
            if let Some(answer) = turn
                .items
                .iter()
                .rev()
                .find(|item| matches!(item, ThreadItem::AssistantMessage { .. }))
                .cloned()
            {
                visible.push(answer);
            }
            turn.items = visible;
        }
        self
    }
}

/// Provider-facing conversation state kept separately from the user-visible
/// Thread/Turn transcript. Compaction may replace this value without deleting
/// durable user messages or final answers from the transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredModelContext {
    pub thread_id: ThreadId,
    pub messages: Vec<Value>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum ClientRequest {
    ThreadCreate {
        thread_id: Option<ThreadId>,
        workspace: String,
        title: String,
    },
    ThreadCreateActivate {
        thread_id: ThreadId,
        workspace: String,
        title: String,
    },
    ThreadImport {
        thread: Thread,
    },
    ThreadsImport {
        threads: Vec<Thread>,
    },
    ThreadList {
        include_archived: bool,
    },
    ThreadRead {
        thread_id: ThreadId,
    },
    ThreadReadVisible {
        thread_id: ThreadId,
    },
    ThreadModelContextRead {
        thread_id: ThreadId,
    },
    ThreadModelContextReplace {
        thread_id: ThreadId,
        messages: Vec<Value>,
    },
    ThreadArchive {
        thread_id: ThreadId,
        archived: bool,
    },
    ThreadRename {
        thread_id: ThreadId,
        title: String,
    },
    ThreadFork {
        thread_id: ThreadId,
        new_thread_id: ThreadId,
    },
    ThreadForkActivate {
        thread_id: ThreadId,
        new_thread_id: ThreadId,
    },
    ThreadActivate {
        thread_id: ThreadId,
    },
    TurnStart {
        thread_id: ThreadId,
        turn_id: TurnId,
    },
    TurnSubmit {
        thread_id: ThreadId,
        text: String,
        #[serde(default)]
        images: Vec<String>,
    },
    TurnInterrupt {
        thread_id: ThreadId,
        turn_id: TurnId,
    },
    TurnInterruptLatest {
        thread_id: ThreadId,
    },
    TurnComplete {
        thread_id: ThreadId,
        turn_id: TurnId,
        status: TurnStatus,
        error: Option<String>,
        #[serde(default)]
        usage: TurnUsage,
    },
    ItemAppend {
        thread_id: ThreadId,
        turn_id: TurnId,
        item: ThreadItem,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum ResponsePayload {
    Ack,
    Thread(Thread),
    Threads(Vec<ThreadMetadata>),
    ModelContext(Option<StoredModelContext>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerResponse {
    pub protocol_version: u32,
    pub payload: ResponsePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum Event {
    ThreadCreated {
        metadata: ThreadMetadata,
    },
    ThreadUpdated {
        metadata: ThreadMetadata,
    },
    TurnStarted {
        status: TurnStatus,
    },
    TurnCompleted {
        status: TurnStatus,
        error: Option<String>,
    },
    ItemAdded {
        item: ThreadItem,
    },
    ModelContextUpdated {
        message_count: usize,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub protocol_version: u32,
    pub sequence: u64,
    pub thread_id: ThreadId,
    pub turn_id: Option<TurnId>,
    pub event: Event,
}

impl EventEnvelope {
    pub fn new(sequence: u64, thread_id: ThreadId, turn_id: Option<TurnId>, event: Event) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            sequence,
            thread_id,
            turn_id,
            event,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidId(&'static str),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(name) => write!(formatter, "{name} must not be empty"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_round_trip_preserves_thread_and_turn_ownership() {
        let event = EventEnvelope::new(
            7,
            ThreadId::new("thread-1").unwrap(),
            Some(TurnId::new("turn-2").unwrap()),
            Event::TurnStarted {
                status: TurnStatus::Running,
            },
        );
        let json = serde_json::to_string(&event).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
        assert!(json.contains("\"threadId\":\"thread-1\""));
        assert!(json.contains("\"turnId\":\"turn-2\""));
    }

    #[test]
    fn durable_ids_reject_empty_values() {
        assert!(ThreadId::new("  ").is_err());
        assert!(TurnId::new("").is_err());
        assert!(ItemId::new("item").is_ok());
    }

    #[test]
    fn visible_projection_keeps_each_request_and_only_the_final_answer() {
        let thread = Thread {
            metadata: ThreadMetadata {
                id: ThreadId::new("thread-1").unwrap(),
                workspace: "workspace".into(),
                title: "title".into(),
                archived: false,
                created_at: 1,
                updated_at: 2,
            },
            turns: vec![Turn {
                id: TurnId::new("turn-1").unwrap(),
                status: TurnStatus::Completed,
                items: vec![
                    ThreadItem::UserMessage {
                        id: ItemId::new("u").unwrap(),
                        text: "request".into(),
                    },
                    ThreadItem::AssistantMessage {
                        id: ItemId::new("a1").unwrap(),
                        text: "progress".into(),
                    },
                    ThreadItem::ToolResult {
                        id: ItemId::new("r").unwrap(),
                        call_id: ItemId::new("c").unwrap(),
                        output: "secret".into(),
                        success: true,
                    },
                    ThreadItem::AssistantMessage {
                        id: ItemId::new("a2").unwrap(),
                        text: "done".into(),
                    },
                ],
                started_at: 1,
                completed_at: Some(2),
                error: None,
                usage: TurnUsage::default(),
            }],
        };
        let visible = thread.into_visible();
        assert_eq!(visible.turns[0].items.len(), 2);
        assert!(
            matches!(&visible.turns[0].items[1], ThreadItem::AssistantMessage { text, .. } if text == "done")
        );
    }
}
