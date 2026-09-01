use crate::{AppServer, AppServerError, DispatchOutcome};
use ncx_protocol::{
    ClientRequest, Event, ResponsePayload, ThreadId, ThreadItem, Turn, TurnId, TurnStatus,
};
use ncx_thread_store::ThreadStore;

/// Dispatches durable thread metadata requests that do not require a host runtime.
pub(crate) fn dispatch_metadata<S: ThreadStore>(
    server: &AppServer<S>,
    request: ClientRequest,
) -> Result<DispatchOutcome, AppServerError> {
    match request {
        ClientRequest::ThreadList { include_archived } => Ok(server.outcome(
            ResponsePayload::Threads(server.store.list(include_archived)?),
            Vec::new(),
        )),
        ClientRequest::ThreadRead { thread_id } => {
            let thread = server.read_thread(&thread_id)?;
            Ok(server.outcome(ResponsePayload::Thread(thread), Vec::new()))
        }
        ClientRequest::ThreadReadVisible { thread_id } => {
            let thread = server.read_thread(&thread_id)?;
            Ok(server.outcome(ResponsePayload::Thread(thread.into_visible()), Vec::new()))
        }
        ClientRequest::ThreadArchive {
            thread_id,
            archived,
        } => server.update_thread_metadata(thread_id, |metadata| metadata.archived = archived),
        ClientRequest::ThreadRename { thread_id, title } => rename(server, thread_id, title),
        ClientRequest::ThreadFork {
            thread_id,
            new_thread_id,
        } => fork(server, thread_id, new_thread_id),
        _ => unreachable!("thread metadata dispatcher received another request"),
    }
}

fn rename<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    title: String,
) -> Result<DispatchOutcome, AppServerError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppServerError::InvalidRequest(
            "thread title must not be empty".to_string(),
        ));
    }
    server.update_thread_metadata(thread_id, |metadata| metadata.title = title.to_string())
}

fn fork<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    new_thread_id: ThreadId,
) -> Result<DispatchOutcome, AppServerError> {
    let now = (server.clock)();
    let mut thread = server.store.fork(&thread_id, new_thread_id.clone())?;
    thread.metadata.created_at = now;
    thread.metadata.updated_at = now;
    server.store.update_metadata(thread.metadata.clone())?;
    let event = server.event(
        new_thread_id,
        None,
        Event::ThreadCreated {
            metadata: thread.metadata.clone(),
        },
    );
    Ok(server.outcome(ResponsePayload::Thread(thread), vec![event]))
}

/// Dispatches persisted model context reads and replacements.
pub(crate) fn dispatch_model_context<S: ThreadStore>(
    server: &AppServer<S>,
    request: ClientRequest,
) -> Result<DispatchOutcome, AppServerError> {
    match request {
        ClientRequest::ThreadModelContextRead { thread_id } => {
            server.read_thread(&thread_id)?;
            Ok(server.outcome(
                ResponsePayload::ModelContext(server.store.read_model_context(&thread_id)?),
                Vec::new(),
            ))
        }
        ClientRequest::ThreadModelContextReplace {
            thread_id,
            messages,
        } => {
            let message_count = messages.len();
            server
                .store
                .replace_model_context(&thread_id, messages, (server.clock)())?;
            let event = server.event(
                thread_id,
                None,
                Event::ModelContextUpdated { message_count },
            );
            Ok(server.outcome(ResponsePayload::Ack, vec![event]))
        }
        _ => unreachable!("model context dispatcher received another request"),
    }
}

/// Dispatches the durable turn lifecycle, keeping event emission coupled to
/// the successful store transition.
pub(crate) fn dispatch_turn<S: ThreadStore>(
    server: &AppServer<S>,
    request: ClientRequest,
) -> Result<DispatchOutcome, AppServerError> {
    let (payload, event) = match request {
        ClientRequest::TurnStart {
            thread_id,
            turn_id,
            execution_mode,
        } => start_turn(server, thread_id, turn_id, execution_mode)?,
        ClientRequest::TurnInterrupt { thread_id, turn_id } => finish_turn(
            server,
            thread_id,
            turn_id,
            TurnStatus::Cancelled,
            None,
            Default::default(),
        )?,
        ClientRequest::TurnComplete {
            thread_id,
            turn_id,
            status,
            error,
            usage,
        } => finish_turn(server, thread_id, turn_id, status, error, usage)?,
        _ => unreachable!("turn dispatcher received another request"),
    };
    Ok(server.outcome(payload, vec![event]))
}

fn start_turn<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    turn_id: TurnId,
    execution_mode: ncx_protocol::ExecutionMode,
) -> Result<(ResponsePayload, ncx_protocol::EventEnvelope), AppServerError> {
    server.store.claim_turn(
        &thread_id,
        Turn {
            id: turn_id.clone(),
            status: TurnStatus::Running,
            execution_mode,
            items: Vec::new(),
            started_at: (server.clock)(),
            completed_at: None,
            error: None,
            usage: Default::default(),
        },
    )?;
    Ok((
        ResponsePayload::Ack,
        server.event(
            thread_id,
            Some(turn_id),
            Event::TurnStarted {
                status: TurnStatus::Running,
            },
        ),
    ))
}

fn finish_turn<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    turn_id: TurnId,
    status: TurnStatus,
    error: Option<String>,
    usage: ncx_protocol::TurnUsage,
) -> Result<(ResponsePayload, ncx_protocol::EventEnvelope), AppServerError> {
    server.store.finish_turn(
        &thread_id,
        &turn_id,
        status,
        (server.clock)(),
        error.clone(),
        usage,
    )?;
    Ok((
        ResponsePayload::Ack,
        server.event(
            thread_id,
            Some(turn_id),
            Event::TurnCompleted { status, error },
        ),
    ))
}

pub(crate) fn dispatch_item<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    turn_id: TurnId,
    item: ThreadItem,
) -> Result<DispatchOutcome, AppServerError> {
    server
        .store
        .append_item(&thread_id, &turn_id, item.clone(), (server.clock)())?;
    let event = server.event(thread_id, Some(turn_id), Event::ItemAdded { item });
    Ok(server.outcome(ResponsePayload::Ack, vec![event]))
}
