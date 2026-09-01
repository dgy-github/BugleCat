//! In-process app-server boundary shared by desktop, CLI and future SDK clients.
mod adapter;
mod goal_driver;
mod goal_operations;
mod outcome;
mod runtime_operations;
mod thread_operations;
pub use adapter::AppServerAdapter;
pub use goal_driver::{GoalRoundDriveOutcome, GoalRoundDriver};
use ncx_protocol::{
    ClientRequest, Event, EventEnvelope, ResponsePayload, ServerResponse, Thread, ThreadMetadata,
    PROTOCOL_VERSION,
};
use ncx_thread_store::ThreadStore;
pub use outcome::{AppServerError, DispatchOutcome};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
pub struct AppServer<S: ThreadStore> {
    store: Arc<S>,
    sequence: AtomicU64,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// Thread IDs explicitly armed in this exact process lifecycle. This is
    /// intentionally absent from Thread Store persistence.
    goal_activations: Mutex<HashSet<String>>,
}

impl<S: ThreadStore> AppServer<S> {
    pub fn new(store: Arc<S>, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            store,
            sequence: AtomicU64::new(1),
            clock: Arc::new(clock),
            goal_activations: Mutex::new(HashSet::new()),
        }
    }

    pub fn dispatch(&self, request: ClientRequest) -> Result<DispatchOutcome, AppServerError> {
        if runtime_operations::requires_runtime_adapter(&request) {
            return Err(AppServerError::InvalidRequest(
                "request requires a runtime adapter".to_string(),
            ));
        }
        match request {
            request @ (ClientRequest::ThreadCreate { .. }
            | ClientRequest::ThreadImport { .. }
            | ClientRequest::ThreadsImport { .. }) => self.dispatch_thread_creation(request),
            request @ (ClientRequest::ThreadList { .. }
            | ClientRequest::ThreadRead { .. }
            | ClientRequest::ThreadReadVisible { .. }
            | ClientRequest::ThreadArchive { .. }
            | ClientRequest::ThreadRename { .. }
            | ClientRequest::ThreadFork { .. }) => {
                thread_operations::dispatch_metadata(self, request)
            }
            request @ (ClientRequest::ThreadModelContextRead { .. }
            | ClientRequest::ThreadModelContextReplace { .. }) => {
                thread_operations::dispatch_model_context(self, request)
            }
            request @ (ClientRequest::GoalRead { .. }
            | ClientRequest::GoalCreate { .. }
            | ClientRequest::GoalEdit { .. }
            | ClientRequest::GoalPause { .. }
            | ClientRequest::GoalResume { .. }
            | ClientRequest::GoalBlock { .. }
            | ClientRequest::GoalComplete { .. }
            | ClientRequest::GoalClear { .. }
            | ClientRequest::GoalRoundStart { .. }) => goal_operations::dispatch(self, request),
            request @ (ClientRequest::TurnStart { .. }
            | ClientRequest::TurnInterrupt { .. }
            | ClientRequest::TurnComplete { .. }) => {
                thread_operations::dispatch_turn(self, request)
            }
            ClientRequest::ItemAppend {
                thread_id,
                turn_id,
                item,
            } => thread_operations::dispatch_item(self, thread_id, turn_id, item),
            ClientRequest::CodexPluginList
            | ClientRequest::CodexPluginInstall { .. }
            | ClientRequest::CodexPluginSetEnabled { .. }
            | ClientRequest::CodexPluginUninstall { .. }
            | ClientRequest::MarketplaceList
            | ClientRequest::MarketplacePluginInstall { .. }
            | ClientRequest::DshMarketplaceSearch { .. }
            | ClientRequest::DshMarketplacePreview { .. }
            | ClientRequest::DshMarketplaceInstall { .. } => Err(AppServerError::InvalidRequest(
                "plugin requests require a host adapter".to_string(),
            )),
            _ => unreachable!("runtime request was not intercepted above"),
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
                harness_profile,
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
                    harness_profile,
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
                outcome::ensure_import_is_idle(std::slice::from_ref(&thread))?;
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
                outcome::ensure_import_is_idle(&threads)?;
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

    pub(crate) fn update_thread_metadata(
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

    /// Update the Harness Profile only while the durable Thread has never
    /// admitted a turn. The store combines that predicate and mutation in one
    /// transaction, preventing a first TurnStart from racing this update.
    pub(crate) fn set_harness_profile_if_idle(
        &self,
        thread_id: ncx_protocol::ThreadId,
        harness_profile: String,
    ) -> Result<Option<DispatchOutcome>, AppServerError> {
        let Some(metadata) =
            self.store
                .set_harness_profile_if_idle(&thread_id, harness_profile, (self.clock)())?
        else {
            return Ok(None);
        };
        let event = self.event(thread_id, None, Event::ThreadUpdated { metadata });
        Ok(Some(self.outcome(ResponsePayload::Ack, vec![event])))
    }

    pub(crate) fn read_thread(
        &self,
        thread_id: &ncx_protocol::ThreadId,
    ) -> Result<Thread, AppServerError> {
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
                harness_profile,
            } => {
                runtime
                    .validate_harness_profile(&harness_profile, &workspace)
                    .map_err(AppServerError::Runtime)?;
                let outcome = self.dispatch(ClientRequest::ThreadCreate {
                    thread_id: Some(thread_id.clone()),
                    workspace,
                    title,
                    harness_profile,
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
                self.disarm_goal(&thread_id)?;
                runtime
                    .activate_thread(&thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::ThreadHarnessProfileSet {
                thread_id,
                harness_profile,
            } => runtime_operations::set_harness_profile(self, runtime, thread_id, harness_profile),
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
                execution_mode,
            } => {
                runtime
                    .submit_turn(&thread_id, text, images, execution_mode)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::TurnInterruptLatest { thread_id } => {
                runtime
                    .interrupt_latest(&thread_id)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::GoalResume { thread_id, goal } => {
                let outcome = self.dispatch(ClientRequest::GoalResume {
                    thread_id: thread_id.clone(),
                    goal,
                })?;
                if let Err(error) = runtime.continue_goal(&thread_id) {
                    // Durable phase may remain active, but process-local authority
                    // must fail closed when the host did not accept the work.
                    self.disarm_goal(&thread_id)?;
                    return Err(AppServerError::Runtime(error));
                }
                Ok(outcome)
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
            ClientRequest::RuntimePermissionModeSet { thread_id, mode } => {
                self.dispatch(ClientRequest::ThreadRead {
                    thread_id: thread_id.clone(),
                })?;
                runtime
                    .set_permission_mode(&thread_id, mode)
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
            ClientRequest::CustomProviderList => runtime
                .list_custom_providers()
                .map(ResponsePayload::CustomProviders)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::CustomProviderSave {
                id,
                name,
                protocol,
                base_url,
                api_key,
                models,
            } => runtime
                .save_custom_provider(id, name, protocol, base_url, api_key, models)
                .map(ResponsePayload::CustomProvider)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::CustomProviderDelete { id } => {
                runtime
                    .delete_custom_provider(id)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::CustomProviderModelsDiscover { id } => runtime
                .discover_custom_provider_models(id)
                .map(ResponsePayload::Models)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::CustomProviderActivate { id, model } => {
                runtime
                    .activate_custom_provider(id, model)
                    .map_err(AppServerError::Runtime)?;
                Ok(self.ack())
            }
            ClientRequest::CustomProviderChatProbe { id, model } => runtime
                .probe_custom_provider_chat(id, model)
                .map(ResponsePayload::ProviderChatProbe)
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
            request @ (ClientRequest::MemoryList { .. }
            | ClientRequest::MemoryAdd { .. }
            | ClientRequest::MemoryConsolidate { .. }
            | ClientRequest::MemoryMergeStart { .. }
            | ClientRequest::MemoryMergeStatusRead { .. }
            | ClientRequest::MemoryMergeCancel { .. }
            | ClientRequest::ForgeRuntimeStatusRead
            | ClientRequest::ForgeJobStart { .. }
            | ClientRequest::ForgeJobStatusRead { .. }
            | ClientRequest::ForgeJobCancel { .. }) => {
                runtime_operations::dispatch(request, runtime)
                    .map(|payload| self.response(payload))
                    .map_err(AppServerError::Runtime)
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
            ClientRequest::DshMarketplaceSearch {
                source,
                manifest_url,
                query,
            } => runtime
                .search_dsh_marketplace(source, manifest_url, query)
                .map(ResponsePayload::DshMarketplace)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::DshMarketplacePreview { item } => runtime
                .preview_dsh_marketplace_plugin(item)
                .map(ResponsePayload::DshMarketplacePreview)
                .map(|payload| self.response(payload))
                .map_err(AppServerError::Runtime),
            ClientRequest::DshMarketplaceInstall { item, upgrade } => runtime
                .install_dsh_marketplace_plugin(item, upgrade)
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

    pub(crate) fn goal_view(
        &self,
        thread_id: &ncx_protocol::ThreadId,
        goal: ncx_protocol::GoalSnapshot,
    ) -> Result<ncx_protocol::GoalView, AppServerError> {
        let armed = self
            .goal_activations
            .lock()
            .map_err(|_| AppServerError::Runtime("goal activation lock is poisoned".into()))?
            .contains(thread_id.as_str());
        Ok(ncx_protocol::GoalView {
            goal,
            activation: if armed {
                ncx_protocol::GoalActivation::Armed
            } else {
                ncx_protocol::GoalActivation::Disarmed
            },
        })
    }

    pub(crate) fn arm_goal(
        &self,
        thread_id: &ncx_protocol::ThreadId,
    ) -> Result<(), AppServerError> {
        self.goal_activations
            .lock()
            .map_err(|_| AppServerError::Runtime("goal activation lock is poisoned".into()))?
            .insert(thread_id.as_str().to_string());
        Ok(())
    }

    /// Revoke process-local continuation authority without mutating the
    /// durable Goal definition. Hosts call this when their accepted worker
    /// cannot be created or its durability fence fails.
    pub fn disarm_goal(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), AppServerError> {
        self.goal_activations
            .lock()
            .map_err(|_| AppServerError::Runtime("goal activation lock is poisoned".into()))?
            .remove(thread_id.as_str());
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
