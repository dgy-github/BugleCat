//! In-process app-server boundary shared by desktop, CLI and future SDK clients.

use ncx_protocol::{
    ClientRequest, Event, EventEnvelope, ResponsePayload, ServerResponse, Thread, ThreadMetadata,
    Turn, TurnStatus, PROTOCOL_VERSION,
};
use ncx_thread_store::{ThreadStore, ThreadStoreError};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Runtime side effects that are deliberately outside durable thread storage.
///
/// Desktop, CLI, and future transports implement this boundary instead of
/// matching protocol requests themselves. This keeps request routing owned by
/// the app-server while allowing each host to choose its agent scheduler.
pub trait AppServerAdapter {
    fn create_thread(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn activate_thread(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn fork_thread(
        &self,
        source_id: &ncx_protocol::ThreadId,
        target_id: &ncx_protocol::ThreadId,
    ) -> Result<(), String>;
    fn submit_turn(
        &self,
        thread_id: &ncx_protocol::ThreadId,
        text: String,
        images: Vec<String>,
    ) -> Result<(), String>;
    fn interrupt_latest(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn runtime_status(&self) -> Result<serde_json::Value, String>;
    fn refresh_ready(&self) -> Result<(), String>;
    fn set_workspace(&self, path: String) -> Result<String, String>;
    fn approve(
        &self,
        thread_id: Option<&ncx_protocol::ThreadId>,
        id: u64,
        decision: String,
    ) -> Result<(), String>;
    fn answer(
        &self,
        thread_id: Option<&ncx_protocol::ThreadId>,
        id: u64,
        answer: Option<String>,
    ) -> Result<(), String>;
    fn read_settings(&self) -> Result<serde_json::Value, String>;
    fn update_settings(
        &self,
        updates: std::collections::BTreeMap<String, String>,
    ) -> Result<(), String>;
    fn set_model(&self, model: String) -> Result<(), String>;
    fn set_permission_mode(&self, mode: String) -> Result<(), String>;
    fn read_model_catalog(&self) -> Result<serde_json::Value, String>;
    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String>;
    fn harness_diagnostics(&self) -> Result<serde_json::Value, String>;
    fn list_external_plugins(&self) -> Result<serde_json::Value, String>;
    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String>;
    fn list_codex_plugins(&self) -> Result<serde_json::Value, String>;
    fn install_codex_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
    fn set_codex_plugin_enabled(&self, name: String, enabled: bool) -> Result<(), String>;
    fn uninstall_codex_plugin(&self, name: String) -> Result<(), String>;
    fn list_marketplaces(&self) -> Result<serde_json::Value, String>;
    fn install_marketplace_plugin(
        &self,
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
}

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
        match request {
            request @ (ClientRequest::ThreadCreate { .. }
            | ClientRequest::ThreadImport { .. }
            | ClientRequest::ThreadsImport { .. }) => self.dispatch_thread_creation(request),
            request @ (ClientRequest::ThreadList { .. }
            | ClientRequest::ThreadRead { .. }
            | ClientRequest::ThreadReadVisible { .. }
            | ClientRequest::ThreadArchive { .. }
            | ClientRequest::ThreadRename { .. }
            | ClientRequest::ThreadFork { .. }) => self.dispatch_thread_metadata(request),
            request @ (ClientRequest::ThreadModelContextRead { .. }
            | ClientRequest::ThreadModelContextReplace { .. }) => {
                self.dispatch_model_context(request)
            }
            request @ (ClientRequest::TurnStart { .. }
            | ClientRequest::TurnInterrupt { .. }
            | ClientRequest::TurnComplete { .. }) => self.dispatch_turn(request),
            ClientRequest::ItemAppend {
                thread_id,
                turn_id,
                item,
            } => self.dispatch_item(thread_id, turn_id, item),
            ClientRequest::ThreadCreateActivate { .. }
            | ClientRequest::ThreadForkActivate { .. }
            | ClientRequest::ThreadActivate { .. }
            | ClientRequest::TurnSubmit { .. }
            | ClientRequest::TurnInterruptLatest { .. }
            | ClientRequest::RuntimeStatusRead
            | ClientRequest::RuntimeReadyRefresh
            | ClientRequest::WorkspaceSet { .. }
            | ClientRequest::InteractionApprove { .. }
            | ClientRequest::InteractionAnswer { .. }
            | ClientRequest::SettingsRead
            | ClientRequest::SettingsUpdate { .. }
            | ClientRequest::RuntimeModelSet { .. }
            | ClientRequest::RuntimePermissionModeSet { .. }
            | ClientRequest::ModelCatalogRead
            | ClientRequest::ModelPresetApply { .. }
            | ClientRequest::HarnessDiagnosticsRead
            | ClientRequest::ExternalPluginList
            | ClientRequest::ExternalPluginInstall { .. }
            | ClientRequest::ExternalPluginSetEnabled { .. } => Err(AppServerError::InvalidRequest(
                "request requires a runtime adapter".to_string(),
            )),
            ClientRequest::CodexPluginList
            | ClientRequest::CodexPluginInstall { .. }
            | ClientRequest::CodexPluginSetEnabled { .. }
            | ClientRequest::CodexPluginUninstall { .. }
            | ClientRequest::MarketplaceList
            | ClientRequest::MarketplacePluginInstall { .. } => {
                Err(AppServerError::InvalidRequest(
                    "plugin requests require a host adapter".to_string(),
                ))
            }
        }
    }

    fn dispatch_thread_creation(
        &self,
        request: ClientRequest,
    ) -> Result<DispatchOutcome, AppServerError> {
        let mut events = Vec::new();
        let payload = match request {
            ClientRequest::ThreadCreate {
                thread_id,
                workspace,
                title,
            } => {
                let now = (self.clock)();
                let id = thread_id.unwrap_or(ncx_protocol::ThreadId::new(format!(
                    "thread-{now}-{}",
                    self.sequence.fetch_add(1, Ordering::Relaxed)
                ))?);
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
            ClientRequest::ThreadImport { thread } => {
                ensure_import_is_idle(std::slice::from_ref(&thread))?;
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
                ensure_import_is_idle(&threads)?;
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
            _ => unreachable!("thread creation dispatcher received another request"),
        };
        Ok(self.outcome(payload, events))
    }

    fn dispatch_thread_metadata(
        &self,
        request: ClientRequest,
    ) -> Result<DispatchOutcome, AppServerError> {
        match request {
            ClientRequest::ThreadList { include_archived } => Ok(self.outcome(
                ResponsePayload::Threads(self.store.list(include_archived)?),
                Vec::new(),
            )),
            ClientRequest::ThreadRead { thread_id } => {
                let thread = self.read_thread(&thread_id)?;
                Ok(self.outcome(ResponsePayload::Thread(thread), Vec::new()))
            }
            ClientRequest::ThreadReadVisible { thread_id } => {
                let thread = self.read_thread(&thread_id)?;
                Ok(self.outcome(ResponsePayload::Thread(thread.into_visible()), Vec::new()))
            }
            ClientRequest::ThreadArchive {
                thread_id,
                archived,
            } => self.update_thread_metadata(thread_id, |metadata| metadata.archived = archived),
            ClientRequest::ThreadRename { thread_id, title } => {
                let title = title.trim();
                if title.is_empty() {
                    return Err(AppServerError::InvalidRequest(
                        "thread title must not be empty".to_string(),
                    ));
                }
                self.update_thread_metadata(thread_id, |metadata| {
                    metadata.title = title.to_string()
                })
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
                let event = self.event(
                    new_thread_id,
                    None,
                    Event::ThreadCreated {
                        metadata: thread.metadata.clone(),
                    },
                );
                Ok(self.outcome(ResponsePayload::Thread(thread), vec![event]))
            }
            _ => unreachable!("thread metadata dispatcher received another request"),
        }
    }

    fn dispatch_model_context(
        &self,
        request: ClientRequest,
    ) -> Result<DispatchOutcome, AppServerError> {
        match request {
            ClientRequest::ThreadModelContextRead { thread_id } => {
                self.read_thread(&thread_id)?;
                Ok(self.outcome(
                    ResponsePayload::ModelContext(self.store.read_model_context(&thread_id)?),
                    Vec::new(),
                ))
            }
            ClientRequest::ThreadModelContextReplace {
                thread_id,
                messages,
            } => {
                let message_count = messages.len();
                self.store
                    .replace_model_context(&thread_id, messages, (self.clock)())?;
                let event = self.event(
                    thread_id,
                    None,
                    Event::ModelContextUpdated { message_count },
                );
                Ok(self.outcome(ResponsePayload::Ack, vec![event]))
            }
            _ => unreachable!("model context dispatcher received another request"),
        }
    }

    fn dispatch_turn(&self, request: ClientRequest) -> Result<DispatchOutcome, AppServerError> {
        let (payload, event) = match request {
            ClientRequest::TurnStart { thread_id, turn_id } => {
                self.store.claim_turn(
                    &thread_id,
                    Turn {
                        id: turn_id.clone(),
                        status: TurnStatus::Running,
                        items: Vec::new(),
                        started_at: (self.clock)(),
                        completed_at: None,
                        error: None,
                        usage: Default::default(),
                    },
                )?;
                (
                    ResponsePayload::Ack,
                    self.event(
                        thread_id,
                        Some(turn_id),
                        Event::TurnStarted {
                            status: TurnStatus::Running,
                        },
                    ),
                )
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
                (
                    ResponsePayload::Ack,
                    self.event(
                        thread_id,
                        Some(turn_id),
                        Event::TurnCompleted {
                            status: TurnStatus::Cancelled,
                            error: None,
                        },
                    ),
                )
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
                (
                    ResponsePayload::Ack,
                    self.event(
                        thread_id,
                        Some(turn_id),
                        Event::TurnCompleted { status, error },
                    ),
                )
            }
            _ => unreachable!("turn dispatcher received another request"),
        };
        Ok(self.outcome(payload, vec![event]))
    }

    fn dispatch_item(
        &self,
        thread_id: ncx_protocol::ThreadId,
        turn_id: ncx_protocol::TurnId,
        item: ncx_protocol::ThreadItem,
    ) -> Result<DispatchOutcome, AppServerError> {
        self.store
            .append_item(&thread_id, &turn_id, item.clone(), (self.clock)())?;
        let event = self.event(thread_id, Some(turn_id), Event::ItemAdded { item });
        Ok(self.outcome(ResponsePayload::Ack, vec![event]))
    }

    fn update_thread_metadata(
        &self,
        thread_id: ncx_protocol::ThreadId,
        update: impl FnOnce(&mut ThreadMetadata),
    ) -> Result<DispatchOutcome, AppServerError> {
        let mut thread = self.read_thread(&thread_id)?;
        update(&mut thread.metadata);
        thread.metadata.updated_at = (self.clock)();
        self.store.update_metadata(thread.metadata.clone())?;
        let event = self.event(
            thread_id,
            None,
            Event::ThreadUpdated {
                metadata: thread.metadata,
            },
        );
        Ok(self.outcome(ResponsePayload::Ack, vec![event]))
    }

    fn read_thread(&self, thread_id: &ncx_protocol::ThreadId) -> Result<Thread, AppServerError> {
        self.store
            .read(thread_id)?
            .ok_or_else(|| AppServerError::NotFound(thread_id.to_string()))
    }

    fn outcome(&self, payload: ResponsePayload, events: Vec<EventEnvelope>) -> DispatchOutcome {
        DispatchOutcome {
            response: ServerResponse {
                protocol_version: PROTOCOL_VERSION,
                payload,
            },
            events,
        }
    }

    /// Dispatch the complete public protocol, delegating only host-specific
    /// scheduler effects to `runtime`.
    pub fn dispatch_with_runtime(
        &self,
        request: ClientRequest,
        runtime: &dyn AppServerAdapter,
    ) -> Result<DispatchOutcome, AppServerError> {
        match request {
            ClientRequest::ThreadCreateActivate {
                thread_id,
                workspace,
                title,
            } => {
                let outcome = self.dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(thread_id.clone()),
                    workspace,
                    title,
                })?;
                runtime
                    .create_thread(&thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(outcome)
            }
            ClientRequest::ThreadActivate { thread_id } => {
                self.dispatch(ClientRequest::ThreadRead {
                    thread_id: thread_id.clone(),
                })?;
                runtime
                    .activate_thread(&thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::ThreadForkActivate {
                thread_id,
                new_thread_id,
            } => {
                let outcome = self.dispatch(ClientRequest::ThreadFork {
                    thread_id: thread_id.clone(),
                    new_thread_id: new_thread_id.clone(),
                })?;
                runtime
                    .fork_thread(&thread_id, &new_thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(outcome)
            }
            ClientRequest::TurnSubmit {
                thread_id,
                text,
                images,
            } => {
                runtime
                    .submit_turn(&thread_id, text, images)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::TurnInterruptLatest { thread_id } => {
                runtime
                    .interrupt_latest(&thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::RuntimeStatusRead => runtime
                .runtime_status()
                .map(ResponsePayload::RuntimeStatus)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::RuntimeReadyRefresh => {
                runtime.refresh_ready().map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::WorkspaceSet { path } => runtime
                .set_workspace(path)
                .map(ResponsePayload::Workspace)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::InteractionApprove {
                thread_id,
                id,
                decision,
            } => {
                runtime
                    .approve(thread_id.as_ref(), id, decision)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::InteractionAnswer {
                thread_id,
                id,
                answer,
            } => {
                runtime
                    .answer(thread_id.as_ref(), id, answer)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::SettingsRead => runtime
                .read_settings()
                .map(ResponsePayload::Settings)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::SettingsUpdate { updates } => {
                runtime
                    .update_settings(updates)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::RuntimeModelSet { model } => {
                runtime.set_model(model).map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::RuntimePermissionModeSet { mode } => {
                runtime
                    .set_permission_mode(mode)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::ModelCatalogRead => runtime
                .read_model_catalog()
                .map(ResponsePayload::ModelCatalog)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::ModelPresetApply {
                provider_id,
                model_id,
            } => runtime
                .apply_model_preset(provider_id, model_id)
                .map(ResponsePayload::ModelPreset)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::HarnessDiagnosticsRead => runtime
                .harness_diagnostics()
                .map(ResponsePayload::HarnessDiagnostics)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::ExternalPluginList => runtime
                .list_external_plugins()
                .map(ResponsePayload::ExternalPlugins)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::ExternalPluginInstall { source, upgrade } => runtime
                .install_external_plugin(source, upgrade)
                .map(ResponsePayload::ExternalPlugin)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::ExternalPluginSetEnabled { id, enabled } => {
                runtime
                    .set_external_plugin_enabled(id, enabled)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::CodexPluginList => runtime
                .list_codex_plugins()
                .map(ResponsePayload::CodexPlugins)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::CodexPluginInstall { source, upgrade } => runtime
                .install_codex_plugin(source, upgrade)
                .map(ResponsePayload::CodexPlugin)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::CodexPluginSetEnabled { name, enabled } => {
                runtime
                    .set_codex_plugin_enabled(name, enabled)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::CodexPluginUninstall { name } => {
                runtime
                    .uninstall_codex_plugin(name)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::MarketplaceList => runtime
                .list_marketplaces()
                .map(ResponsePayload::Marketplaces)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::MarketplacePluginInstall {
                marketplace_path,
                plugin_name,
                upgrade,
            } => runtime
                .install_marketplace_plugin(marketplace_path, plugin_name, upgrade)
                .map(ResponsePayload::CodexPlugin)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            request => self.dispatch(request),
        }
    }

    pub fn ack(&self) -> DispatchOutcome {
        self.response(ResponsePayload::Ack)
    }

    fn response(&self, payload: ResponsePayload) -> DispatchOutcome {
        self.outcome(payload, Vec::new())
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

fn ensure_import_is_idle(threads: &[Thread]) -> Result<(), AppServerError> {
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
            Self::InvalidRequest(message) => message.fmt(formatter),
            Self::Runtime(message) => message.fmt(formatter),
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
#[path = "tests.rs"]
mod tests;
