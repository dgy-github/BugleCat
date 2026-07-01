//! AI video-agent production framework primitives.
//!
//! This crate starts with the P1 hard foundation from the runbook: SQLite
//! schema/WAL, job idempotency, budget accounting, validation-contract checks,
//! and the local L0 gate surface. Cloud providers and Temporal workers plug
//! into these primitives instead of bypassing them.

pub mod ark;
pub mod db;
pub mod dry_run;
pub mod edit;
pub mod jobs;
pub mod l0;
pub mod media;
pub mod node;
pub mod preflight;
pub mod pricing;
pub mod render;
pub mod runtime_config;
pub mod structured;
pub mod text_separation;
pub mod tos;
pub mod trace;
pub mod validation;

pub use ark::{ArkClient, ArkTaskStatus, ArkTransport, ReqwestArkTransport, ARK_BASE_URL};
pub use db::{init_schema, open_db, require_json1, Database};
pub use dry_run::{run_local_p1_dry_run, LocalDryRunOutput};
pub use edit::{build_rough_cut, FailedShot, RenderedShot, RoughCutResult};
pub use jobs::{
    fail_job_and_release_budget, idempotency_key, mark_job_status, record_job_latency_ms,
    settle_budget, submit_job_once, JobRecord, JobSubmitOutcome,
};
pub use l0::{
    validate_scene_l0, FastTextCliDetector, FastTextModelDetector, HeuristicLanguageDetector,
    L0Report, L0Verdict, LanguageDetector,
};
pub use media::{validate_video_file_l0, MediaL0Report, MediaProbe};
pub use node::{
    assert_context_packet_admissible, p1_agent_node_spec, AgentReasoningMode, ContextPacket,
    NodeKind, NodeSpec, P1AgentNode,
};
pub use preflight::resolve_paid_seedance_prereqs;
pub use pricing::{estimate_seedance_cost_cny, seedance_cost_cny, SEEDANCE_PRICING_AS_OF};
pub use render::{
    persist_seedance_video_artifact, poll_seedance_job_once, submit_seedance_job_once,
    ReqwestVideoDownloader, SeedanceArtifactInput, SeedanceArtifactOutput, SeedancePollOutcome,
    SeedanceSubmitInput, VideoDownloader,
};
pub use runtime_config::{P1ExternalConfig, ResolvedSetting};
pub use structured::{
    chapter_budgets_from_artifact, insert_project_artifact, json_content_hash,
    record_structured_agent_validation_if_pass, record_structured_validation_if_pass,
    shot_ids_from_artifact, validate_assets_artifact, validate_brief_artifact,
    validate_chapters_artifact, validate_shots_artifact, AgentArtifactKind,
    StructuredValidationReport,
};
pub use text_separation::{
    separate_text_and_voice, write_srt, OverlayText, SeparatedShot, ShotTextSpec, TtsRequest,
};
pub use tos::{ReqwestTosTransport, TosClient, TosConfig, TosObjectRef, TosTransport};
pub use trace::{export_project_shot_trace, export_project_trace, ShotTrace};
pub use validation::{assert_artifacts_passed, record_validation, ValidationInput};

#[derive(Debug, thiserror::Error)]
pub enum VideoAgentError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "budget exhausted for project {project_id}: requested {requested}, available {available}"
    )]
    BudgetExhausted {
        project_id: String,
        requested: f64,
        available: f64,
    },
    #[error("job submission failed: {0}")]
    JobSubmission(String),
    #[error("ARK error: {0}")]
    Ark(String),
    #[error("TOS error: {0}")]
    Tos(String),
    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),
    #[error("node contract error: {0}")]
    NodeContract(String),
    #[error("artifact {artifact_id} has no passing validation record")]
    MissingPassingValidation { artifact_id: String },
    #[error("L0 rejected artifact: {0}")]
    L0Rejected(String),
}

pub type Result<T> = std::result::Result<T, VideoAgentError>;

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

    pub fn temp_db_path(name: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ncx-video-agent-{name}-{}-{nanos}-{id}.sqlite",
            std::process::id(),
        ))
    }
}
