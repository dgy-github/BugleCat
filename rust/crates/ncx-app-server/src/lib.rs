//! In-process app-server boundary shared by desktop, CLI and future SDK clients.

use ncx_protocol::{
    ClientRequest, Event, EventEnvelope, ResponsePayload, ServerResponse, Thread, ThreadMetadata,
    Turn, TurnStatus, PROTOCOL_VERSION,
};
use ncx_thread_store::{ThreadStore, ThreadStoreError};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct AppServer<S: ThreadStore> {
    store: Arc<S>,
    sequence: AtomicU64,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl<S: ThreadStore> AppServer<S> {
    pub fn new(store: Arc<S>, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            store,
            sequence: AtomicU64::new(1),
            clock: Arc::new(clock),
        }
    }

    pub fn dispatch(&self, request: ClientRequest) -> Result<DispatchOutcome, AppServerError> {
        let mut events = Vec::new();
        let payload = match request {
            ClientRequest::ThreadCreate {
                thread_id,
                workspace,
                title,
            } => {
                let now = (self.clock)();
                let id = match thread_id {
                    Some(id) => id,
                    None => ncx_protocol::ThreadId::new(format!(
                        "thread-{now}-{}",
                        self.sequence.fetch_add(1, Ordering::Relaxed)
                    ))?,
                };
                let metadata = ThreadMetadata {
                    id: id.clone(),
                    workspace,
                    title,
                    archived: false,
                    created_at: now,
                    updated_at: now,
                };
                let thread = Thread {
                    metadata: metadata.clone(),
                    turns: Vec::new(),
                };
                self.store.create(thread.clone())?;
                events.push(self.event(id, None, Event::ThreadCreated { metadata }));
                ResponsePayload::Thread(thread)
            }
            ClientRequest::ThreadCreateActivate { .. }
            | ClientRequest::ThreadForkActivate { .. }
            | ClientRequest::ThreadActivate { .. } => {
                return Err(AppServerError::InvalidRequest(
                    "thread activation must be handled by the runtime adapter".to_string(),
                ));
            }
            ClientRequest::ThreadImport { thread } => {
                if thread
                    .turns
                    .iter()
                    .any(|turn| matches!(turn.status, TurnStatus::Queued | TurnStatus::Running))
                {
                    return Err(AppServerError::InvalidRequest(
                        "imported threads cannot contain active turns".to_string(),
                    ));
                }
                self.store.create(thread.clone())?;
                events.push(self.event(
                    thread.metadata.id.clone(),
                    None,
                    Event::ThreadCreated {
                        metadata: thread.metadata.clone(),
                    },
                ));
                ResponsePayload::Thread(thread)
            }
            ClientRequest::ThreadsImport { threads } => {
                if threads
                    .iter()
                    .flat_map(|thread| &thread.turns)
                    .any(|turn| matches!(turn.status, TurnStatus::Queued | TurnStatus::Running))
                {
                    return Err(AppServerError::InvalidRequest(
                        "imported threads cannot contain active turns".to_string(),
                    ));
                }
                self.store.create_many(threads.clone())?;
                for thread in &threads {
                    events.push(self.event(
                        thread.metadata.id.clone(),
                        None,
                        Event::ThreadCreated {
                            metadata: thread.metadata.clone(),
                        },
                    ));
                }
                ResponsePayload::Threads(
                    threads.into_iter().map(|thread| thread.metadata).collect(),
                )
            }
            ClientRequest::ThreadList { include_archived } => {
                ResponsePayload::Threads(self.store.list(include_archived)?)
            }
            ClientRequest::ThreadRead { thread_id } => {
                let thread = self
                    .store
                    .read(&thread_id)?
                    .ok_or_else(|| AppServerError::NotFound(thread_id.to_string()))?;
                ResponsePayload::Thread(thread)
            }
            ClientRequest::ThreadReadVisible { thread_id } => {
                let thread = self
                    .store
                    .read(&thread_id)?
                    .ok_or_else(|| AppServerError::NotFound(thread_id.to_string()))?;
                ResponsePayload::Thread(thread.into_visible())
            }
            ClientRequest::ThreadModelContextRead { thread_id } => {
                if self.store.read(&thread_id)?.is_none() {
                    return Err(AppServerError::NotFound(thread_id.to_string()));
                }
                ResponsePayload::ModelContext(self.store.read_model_context(&thread_id)?)
            }
            ClientRequest::ThreadModelContextReplace {
                thread_id,
                messages,
            } => {
                let message_count = messages.len();
                self.store
                    .replace_model_context(&thread_id, messages, (self.clock)())?;
                events.push(self.event(
                    thread_id,
                    None,
                    Event::ModelContextUpdated { message_count },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::ThreadArchive {
                thread_id,
                archived,
            } => {
                let mut thread = self
                    .store
                    .read(&thread_id)?
                    .ok_or_else(|| AppServerError::NotFound(thread_id.to_string()))?;
                thread.metadata.archived = archived;
                thread.metadata.updated_at = (self.clock)();
                self.store.update_metadata(thread.metadata.clone())?;
                events.push(self.event(
                    thread_id,
                    None,
                    Event::ThreadUpdated {
                        metadata: thread.metadata,
                    },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::ThreadRename { thread_id, title } => {
                let mut thread = self
                    .store
                    .read(&thread_id)?
                    .ok_or_else(|| AppServerError::NotFound(thread_id.to_string()))?;
                let title = title.trim();
                if title.is_empty() {
                    return Err(AppServerError::InvalidRequest(
                        "thread title must not be empty".to_string(),
                    ));
                }
                thread.metadata.title = title.to_string();
                thread.metadata.updated_at = (self.clock)();
                self.store.update_metadata(thread.metadata.clone())?;
                events.push(self.event(
                    thread_id,
                    None,
                    Event::ThreadUpdated {
                        metadata: thread.metadata,
                    },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::ThreadFork {
                thread_id,
                new_thread_id,
            } => {
                let now = (self.clock)();
                let mut thread = self.store.fork(&thread_id, new_thread_id.clone())?;
                thread.metadata.created_at = now;
                thread.metadata.updated_at = now;
                self.store.update_metadata(thread.metadata.clone())?;
                events.push(self.event(
                    new_thread_id,
                    None,
                    Event::ThreadCreated {
                        metadata: thread.metadata.clone(),
                    },
                ));
                ResponsePayload::Thread(thread)
            }
            ClientRequest::TurnStart { thread_id, turn_id } => {
                let now = (self.clock)();
                self.store.claim_turn(
                    &thread_id,
                    Turn {
                        id: turn_id.clone(),
                        status: TurnStatus::Running,
                        items: Vec::new(),
                        started_at: now,
                        completed_at: None,
                        error: None,
                        usage: Default::default(),
                    },
                )?;
                events.push(self.event(
                    thread_id,
                    Some(turn_id),
                    Event::TurnStarted {
                        status: TurnStatus::Running,
                    },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::TurnSubmit { .. } => {
                return Err(AppServerError::InvalidRequest(
                    "turnSubmit must be handled by the runtime adapter".to_string(),
                ));
            }
            ClientRequest::TurnInterrupt { thread_id, turn_id } => {
                self.store.finish_turn(
                    &thread_id,
                    &turn_id,
                    TurnStatus::Cancelled,
                    (self.clock)(),
                    None,
                    Default::default(),
                )?;
                events.push(self.event(
                    thread_id,
                    Some(turn_id),
                    Event::TurnCompleted {
                        status: TurnStatus::Cancelled,
                        error: None,
                    },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::TurnInterruptLatest { .. } => {
                return Err(AppServerError::InvalidRequest(
                    "turnInterruptLatest must be handled by the runtime adapter".to_string(),
                ));
            }
            ClientRequest::TurnComplete {
                thread_id,
                turn_id,
                status,
                error,
                usage,
            } => {
                self.store.finish_turn(
                    &thread_id,
                    &turn_id,
                    status,
                    (self.clock)(),
                    error.clone(),
                    usage,
                )?;
                events.push(self.event(
                    thread_id,
                    Some(turn_id),
                    Event::TurnCompleted { status, error },
                ));
                ResponsePayload::Ack
            }
            ClientRequest::ItemAppend {
                thread_id,
                turn_id,
                item,
            } => {
                self.store
                    .append_item(&thread_id, &turn_id, item.clone(), (self.clock)())?;
                events.push(self.event(thread_id, Some(turn_id), Event::ItemAdded { item }));
                ResponsePayload::Ack
            }
        };
        Ok(DispatchOutcome {
            response: ServerResponse {
                protocol_version: PROTOCOL_VERSION,
                payload,
            },
            events,
        })
    }

    pub fn ack(&self) -> DispatchOutcome {
        DispatchOutcome {
            response: ServerResponse {
                protocol_version: PROTOCOL_VERSION,
                payload: ResponsePayload::Ack,
            },
            events: Vec::new(),
        }
    }

    fn event(
        &self,
        thread_id: ncx_protocol::ThreadId,
        turn_id: Option<ncx_protocol::TurnId>,
        event: Event,
    ) -> EventEnvelope {
        EventEnvelope::new(
            self.sequence.fetch_add(1, Ordering::Relaxed),
            thread_id,
            turn_id,
            event,
        )
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DispatchOutcome {
    pub response: ServerResponse,
    pub events: Vec<EventEnvelope>,
}

#[derive(Debug)]
pub enum AppServerError {
    Protocol(ncx_protocol::ProtocolError),
    Store(ThreadStoreError),
    NotFound(String),
    InvalidRequest(String),
}

impl fmt::Display for AppServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::NotFound(id) => write!(formatter, "{id} was not found"),
            Self::InvalidRequest(message) => message.fmt(formatter),
        }
    }
}

impl std::error::Error for AppServerError {}

impl From<ncx_protocol::ProtocolError> for AppServerError {
    fn from(error: ncx_protocol::ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

impl From<ThreadStoreError> for AppServerError {
    fn from(error: ThreadStoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_protocol::{ClientRequest, ItemId, ResponsePayload, ThreadId, ThreadItem, TurnId};
    use ncx_thread_store::JsonThreadStore;
    use std::sync::atomic::AtomicU64;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn server() -> AppServer<JsonThreadStore> {
        let path = std::env::temp_dir().join(format!(
            "ncx-app-server-{}-{}.json",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        AppServer::new(Arc::new(JsonThreadStore::open(path).unwrap()), || 100)
    }

    #[test]
    fn create_and_start_turn_emit_owned_v2_events() {
        let server = server();
        let created = server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: None,
                workspace: "workspace".into(),
                title: "title".into(),
            })
            .unwrap();
        let ResponsePayload::Thread(thread) = created.response.payload else {
            panic!("expected created thread");
        };
        assert_eq!(created.events[0].thread_id, thread.metadata.id);
        assert_eq!(created.events[0].protocol_version, PROTOCOL_VERSION);

        let turn_id = TurnId::new("turn-1").unwrap();
        let started = server
            .dispatch(ClientRequest::TurnStart {
                thread_id: thread.metadata.id.clone(),
                turn_id: turn_id.clone(),
            })
            .unwrap();
        assert_eq!(started.events[0].thread_id, thread.metadata.id);
        assert_eq!(started.events[0].turn_id, Some(turn_id));
    }

    #[test]
    fn second_concurrent_turn_is_rejected() {
        let server = server();
        let created = server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: None,
                workspace: "workspace".into(),
                title: "title".into(),
            })
            .unwrap();
        let ResponsePayload::Thread(thread) = created.response.payload else {
            panic!("expected thread");
        };
        server
            .dispatch(ClientRequest::TurnStart {
                thread_id: thread.metadata.id.clone(),
                turn_id: TurnId::new("one").unwrap(),
            })
            .unwrap();
        assert!(server
            .dispatch(ClientRequest::TurnStart {
                thread_id: ThreadId::new(thread.metadata.id.as_str()).unwrap(),
                turn_id: TurnId::new("two").unwrap(),
            })
            .is_err());
    }

    #[test]
    fn rename_and_fork_are_owned_by_the_app_server() {
        let server = server();
        let thread_id = ThreadId::new("source").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(thread_id.clone()),
                workspace: "workspace".into(),
                title: "old".into(),
            })
            .unwrap();
        let renamed = server
            .dispatch(ClientRequest::ThreadRename {
                thread_id: thread_id.clone(),
                title: "new title".into(),
            })
            .unwrap();
        assert!(matches!(
            renamed.events[0].event,
            Event::ThreadUpdated { .. }
        ));
        let forked = server
            .dispatch(ClientRequest::ThreadFork {
                thread_id,
                new_thread_id: ThreadId::new("target").unwrap(),
            })
            .unwrap();
        let ResponsePayload::Thread(forked_thread) = forked.response.payload else {
            panic!("expected forked thread");
        };
        assert_eq!(forked_thread.metadata.title, "new title");
        assert_eq!(forked.events[0].thread_id.as_str(), "target");
    }

    #[test]
    fn model_context_is_replaced_without_rewriting_visible_turns() {
        let server = server();
        let thread_id = ThreadId::new("thread").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(thread_id.clone()),
                workspace: "workspace".into(),
                title: "title".into(),
            })
            .unwrap();
        let updated = server
            .dispatch(ClientRequest::ThreadModelContextReplace {
                thread_id: thread_id.clone(),
                messages: vec![serde_json::json!({"role":"user","content":"compact"})],
            })
            .unwrap();
        assert!(matches!(
            updated.events[0].event,
            Event::ModelContextUpdated { message_count: 1 }
        ));
        let read = server
            .dispatch(ClientRequest::ThreadModelContextRead { thread_id })
            .unwrap();
        let ResponsePayload::ModelContext(Some(context)) = read.response.payload else {
            panic!("expected stored model context");
        };
        assert_eq!(context.messages[0]["content"], "compact");
    }

    #[test]
    fn visible_thread_never_returns_tool_logs_or_intermediate_assistant_text() {
        let server = server();
        let thread_id = ThreadId::new("visible").unwrap();
        let turn_id = TurnId::new("turn").unwrap();
        server
            .dispatch(ClientRequest::ThreadCreate {
                thread_id: Some(thread_id.clone()),
                workspace: "workspace".into(),
                title: "title".into(),
            })
            .unwrap();
        server
            .dispatch(ClientRequest::TurnStart {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
            })
            .unwrap();
        for item in [
            ThreadItem::UserMessage {
                id: ItemId::new("user").unwrap(),
                text: "请求".into(),
            },
            ThreadItem::AssistantMessage {
                id: ItemId::new("intermediate").unwrap(),
                text: "正在执行".into(),
            },
            ThreadItem::ToolResult {
                id: ItemId::new("result").unwrap(),
                call_id: ItemId::new("call").unwrap(),
                output: "secret tool log".into(),
                success: true,
            },
            ThreadItem::AssistantMessage {
                id: ItemId::new("final").unwrap(),
                text: "最终结论".into(),
            },
        ] {
            server
                .dispatch(ClientRequest::ItemAppend {
                    thread_id: thread_id.clone(),
                    turn_id: turn_id.clone(),
                    item,
                })
                .unwrap();
        }
        let visible = server
            .dispatch(ClientRequest::ThreadReadVisible { thread_id })
            .unwrap();
        let ResponsePayload::Thread(thread) = visible.response.payload else {
            panic!("expected visible thread");
        };
        assert_eq!(thread.turns[0].items.len(), 2);
        assert!(matches!(
            &thread.turns[0].items[1],
            ThreadItem::AssistantMessage { text, .. } if text == "最终结论"
        ));
    }
}
