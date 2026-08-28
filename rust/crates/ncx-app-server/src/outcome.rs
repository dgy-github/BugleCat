use ncx_protocol::{EventEnvelope, ServerResponse, Thread, TurnStatus};
use ncx_thread_store::ThreadStoreError;
use std::fmt;

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
    Runtime(String),
}

impl fmt::Display for AppServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::NotFound(id) => write!(formatter, "{id} was not found"),
            Self::InvalidRequest(message) | Self::Runtime(message) => message.fmt(formatter),
        }
    }
}

pub(crate) fn ensure_import_is_idle(threads: &[Thread]) -> Result<(), AppServerError> {
    if threads
        .iter()
        .flat_map(|thread| &thread.turns)
        .any(|turn| matches!(turn.status, TurnStatus::Queued | TurnStatus::Running))
    {
        return Err(AppServerError::InvalidRequest(
            "imported threads cannot contain active turns".to_string(),
        ));
    }
    Ok(())
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
