use crate::{AppServer, AppServerAdapter, AppServerError, DispatchOutcome};
use ncx_protocol::{ClientRequest, ResponsePayload, ThreadId};
use ncx_thread_store::ThreadStore;

pub(crate) fn set_harness_profile<S: ThreadStore>(
    server: &AppServer<S>,
    runtime: &dyn AppServerAdapter,
    thread_id: ThreadId,
    harness_profile: String,
) -> Result<DispatchOutcome, AppServerError> {
    runtime
        .validate_harness_profile(&harness_profile)
        .map_err(AppServerError::Runtime)?;
    if !server.read_thread(&thread_id)?.turns.is_empty() {
        return Err(AppServerError::InvalidRequest(
            "Harness Profile is locked after the first turn".to_string(),
        ));
    }
    server.update_thread_metadata(thread_id, |metadata| {
        metadata.harness_profile = harness_profile
    })
}

pub(crate) fn dispatch(
    request: ClientRequest,
    runtime: &dyn AppServerAdapter,
) -> Result<ResponsePayload, String> {
    match request {
        ClientRequest::MemoryList => runtime.list_memory().map(ResponsePayload::MemoryNotes),
        ClientRequest::MemoryAdd { note, tags } => {
            runtime.add_memory(note, tags).map(ResponsePayload::Bool)
        }
        ClientRequest::MemoryConsolidate => {
            runtime.consolidate_memory().map(ResponsePayload::Count)
        }
        ClientRequest::MemoryMergeStart => runtime
            .start_memory_merge()
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::MemoryMergeStatusRead => runtime
            .memory_merge_status()
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::MemoryMergeCancel => runtime
            .cancel_memory_merge()
            .map(ResponsePayload::MemoryMergeOperation),
        ClientRequest::ForgeRuntimeStatusRead => runtime
            .forge_runtime_status()
            .map(ResponsePayload::ForgeRuntime),
        ClientRequest::ForgeJobStart {
            rounds,
            repeats,
            timeout_s,
            budget_s,
            teacher,
            accept_margin,
        } => runtime
            .start_forge_job(rounds, repeats, timeout_s, budget_s, teacher, accept_margin)
            .map(ResponsePayload::ForgeJob),
        ClientRequest::ForgeJobStatusRead => {
            runtime.forge_job_status().map(ResponsePayload::ForgeJob)
        }
        ClientRequest::ForgeJobCancel => runtime.cancel_forge_job().map(ResponsePayload::ForgeJob),
        _ => Err("request is not a memory or Forge runtime operation".to_string()),
    }
}
