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
            ClientRequest::ThreadFork {
                thread_id,
                new_thread_id,
            } => ResponsePayload::Thread(self.store.fork(&thread_id, new_thread_id)?),
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
            ClientRequest::TurnInterrupt { thread_id, turn_id } => {
                self.store.finish_turn(
                    &thread_id,
                    &turn_id,
                    TurnStatus::Cancelled,
                    (self.clock)(),
                    None,
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
            ClientRequest::TurnComplete {
                thread_id,
                turn_id,
                status,
                error,
            } => {
                self.store.finish_turn(
                    &thread_id,
                    &turn_id,
                    status,
                    (self.clock)(),
                    error.clone(),
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
                self.store.append_item(&thread_id, &turn_id, item.clone())?;
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
}

impl fmt::Display for AppServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::NotFound(id) => write!(formatter, "{id} was not found"),
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
    use ncx_protocol::{ClientRequest, ResponsePayload, ThreadId, TurnId};
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
}
