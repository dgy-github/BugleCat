use crate::{AppServer, AppServerAdapter, AppServerError, DispatchOutcome};
use ncx_protocol::{ClientRequest, ResponsePayload, ThreadId};
use ncx_thread_store::ThreadStore;

/// Reports whether this request has host-side effects and therefore cannot be
/// served by the durable App Server alone.
pub(crate) fn requires_runtime_adapter(request: &ClientRequest) -> bool {
    matches!(
        request,
        ClientRequest::ThreadCreateActivate { .. }
            | ClientRequest::ThreadHarnessProfileSet { .. }
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
            | ClientRequest::CustomProviderList
            | ClientRequest::CustomProviderSave { .. }
            | ClientRequest::CustomProviderDelete { .. }
            | ClientRequest::CustomProviderModelsDiscover { .. }
            | ClientRequest::CustomProviderActivate { .. }
            | ClientRequest::CustomProviderChatProbe { .. }
            | ClientRequest::HarnessDiagnosticsRead
            | ClientRequest::ExternalPluginList
            | ClientRequest::ExternalPluginInstall { .. }
            | ClientRequest::ExternalPluginSetEnabled { .. }
            | ClientRequest::MemoryList { .. }
            | ClientRequest::MemoryAdd { .. }
            | ClientRequest::MemoryConsolidate { .. }
            | ClientRequest::MemoryMergeStart { .. }
            | ClientRequest::MemoryMergeStatusRead { .. }
            | ClientRequest::MemoryMergeCancel { .. }
            | ClientRequest::ForgeRuntimeStatusRead
            | ClientRequest::ForgeJobStart { .. }
            | ClientRequest::ForgeJobStatusRead { .. }
            | ClientRequest::ForgeJobCancel { .. }
    )
}

pub(crate) fn set_harness_profile<S: ThreadStore>(
    server: &AppServer<S>,
    runtime: &dyn AppServerAdapter,
    thread_id: ThreadId,
    harness_profile: String,
) -> Result<DispatchOutcome, AppServerError> {
    let thread = server.read_thread(&thread_id)?;
    runtime
        .validate_harness_profile(&harness_profile, &thread.metadata.workspace)
        .map_err(AppServerError::Runtime)?;
    server
        .set_harness_profile_if_idle(thread_id, harness_profile)?
        .ok_or_else(|| {
            AppServerError::InvalidRequest(
                "Harness Profile is locked after the first turn".to_string(),
            )
        })
}

pub(crate) fn dispatch(
    request: ClientRequest,
    runtime: &dyn AppServerAdapter,
) -> Result<ResponsePayload, String> {
    match request {
        ClientRequest::MemoryList { workspace } => runtime
            .list_memory(workspace)
            .map(ResponsePayload::MemoryNotes),
        ClientRequest::MemoryAdd {
            note,
            tags,
            workspace,
        } => runtime
            .add_memory(note, tags, workspace)
            .map(ResponsePayload::Bool),
        ClientRequest::MemoryConsolidate { workspace } => runtime
            .consolidate_memory(workspace)
            .map(ResponsePayload::Count),
        ClientRequest::MemoryMergeStart { workspace } => runtime
            .start_memory_merge(workspace)
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::MemoryMergeStatusRead {
            workspace,
            generation,
        } => runtime
            .memory_merge_status(workspace, generation)
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::MemoryMergeCancel {
            workspace,
            generation,
        } => runtime
            .cancel_memory_merge(workspace, generation)
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::ForgeRuntimeStatusRead => runtime
            .forge_runtime_status()
            .map(ResponsePayload::ForgeRuntime),
        ClientRequest::ForgeJobStart {
            workspace,
            rounds,
            repeats,
            timeout_s,
            budget_s,
            teacher,
            accept_margin,
        } => runtime
            .start_forge_job(
                workspace,
                rounds,
                repeats,
                timeout_s,
                budget_s,
                teacher,
                accept_margin,
            )
            .map(ResponsePayload::ForgeJob),
        ClientRequest::ForgeJobStatusRead {
            workspace,
            generation,
        } => runtime
            .forge_job_status(workspace, generation)
            .map(ResponsePayload::ForgeJob),
        ClientRequest::ForgeJobCancel {
            workspace,
            generation,
        } => runtime
            .cancel_forge_job(workspace, generation)
            .map(ResponsePayload::ForgeJob),
        _ => Err("request is not a memory or Forge runtime operation".to_string()),
    }
}
