use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ncx_video_agent::{
    assert_artifacts_passed, build_rough_cut, estimate_seedance_cost_cny,
    export_project_shot_trace, export_project_trace, persist_seedance_video_artifact,
    record_job_latency_ms, record_validation, resolve_paid_seedance_prereqs, seedance_cost_cny,
    settle_budget, submit_seedance_job_once, validate_video_file_l0, ArkClient, Database,
    P1ExternalConfig, RenderedShot, ReqwestArkTransport, ReqwestTosTransport,
    ReqwestVideoDownloader, SeedanceArtifactInput, SeedancePollOutcome, SeedanceSubmitInput,
    TosClient, TosConfig, ValidationInput, ARK_BASE_URL,
};
use rusqlite::params;
use serde_json::{json, Value};
use temporalio_client::{
    envconfig::LoadClientConfigProfileOptions, Client, ClientOptions, Connection,
    WorkflowGetResultOptions, WorkflowSignalOptions, WorkflowStartOptions,
};
use temporalio_common::telemetry::TelemetryOptions;
use temporalio_macros::{activities, workflow, workflow_methods};
use temporalio_sdk::{
    activities::{ActivityContext, ActivityError},
    ActivityOptions, ApplicationFailure, SyncWorkflowContext, Worker, WorkerOptions,
    WorkflowContext, WorkflowContextView, WorkflowResult, WorkflowTermination,
};
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions};

const DEFAULT_TASK_QUEUE: &str = "video-agent-p1-probe";
const DEFAULT_WORKFLOW_ID: &str = "video-agent-p1-probe-workflow";
const DEFAULT_DRY_RUN_WORKFLOW_ID: &str = "video-agent-p1-dry-run-workflow";
const DEFAULT_LIVE_WORKFLOW_ID: &str = "video-agent-p1-live-workflow";
const DEFAULT_SHOT_ID: &str = "shot_01";
const LIVE_PROJECT_ID: &str = "p1_temporal_seedance_tos";
const LIVE_CHAPTER_ID: &str = "chapter_01";
const LIVE_SCENE_ID: &str = "scene_01";
const LIVE_SHOT_ID: &str = "shot_01";
const LIVE_MODEL: &str = "doubao-seedance-2-0-fast-260128";
const LIVE_DURATION_S: f64 = 5.0;
const LIVE_MAX_POLLS: u32 = 60;
const LIVE_POLL_INTERVAL_S: u64 = 6;

pub struct P1ProbeActivities;

#[activities]
impl P1ProbeActivities {
    #[activity]
    pub async fn submit_video_job(
        _ctx: ActivityContext,
        shot_id: String,
    ) -> Result<String, ActivityError> {
        Ok(format!("dry-temporal-job-{shot_id}"))
    }

    #[activity]
    pub async fn poll_video_job(
        _ctx: ActivityContext,
        input: (String, u32),
    ) -> Result<bool, ActivityError> {
        let (_job_id, attempt) = input;
        Ok(attempt >= 2)
    }

    #[activity]
    pub async fn prepare_p1_dry_run(
        _ctx: ActivityContext,
        out_dir: String,
    ) -> Result<String, ActivityError> {
        let out_dir_path = PathBuf::from(&out_dir);
        std::fs::create_dir_all(&out_dir_path).map_err(activity_error)?;
        std::fs::write(
            out_dir_path.join("temporal_prepare_marker.txt"),
            "prepared\n",
        )
        .map_err(activity_error)?;
        Ok(out_dir)
    }

    #[activity]
    pub async fn run_p1_dry_run(
        _ctx: ActivityContext,
        out_dir: String,
    ) -> Result<String, ActivityError> {
        let output = ncx_video_agent::run_local_p1_dry_run(&out_dir).map_err(activity_error)?;
        let first_shot_trace = output
            .shot_trace_paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Ok(format!(
            "rough_cut={};trace={};shot_trace={};db={}",
            output.rough_cut_path.display(),
            output.trace_path.display(),
            first_shot_trace,
            output.db_path.display()
        ))
    }

    #[activity]
    pub async fn submit_live_seedance_job(
        _ctx: ActivityContext,
        out_dir: String,
    ) -> Result<String, ActivityError> {
        submit_live_seedance_job_activity(&out_dir).map_err(activity_error)
    }

    #[activity]
    pub async fn poll_live_seedance_job(
        _ctx: ActivityContext,
        state_json: String,
    ) -> Result<String, ActivityError> {
        poll_live_seedance_job_activity(&state_json).map_err(activity_error)
    }

    #[activity]
    pub async fn persist_live_seedance_outputs(
        _ctx: ActivityContext,
        state_json: String,
    ) -> Result<String, ActivityError> {
        persist_live_seedance_outputs_activity(&state_json).map_err(activity_error)
    }
}

fn activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(10))
}

fn dry_run_activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(90))
}

fn live_submit_activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(45))
}

fn live_poll_activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(45))
}

fn live_persist_activity_opts() -> ActivityOptions {
    ActivityOptions::start_to_close_timeout(Duration::from_secs(300))
}

fn activity_error(err: impl ToString) -> ActivityError {
    ActivityError::application(ApplicationFailure::non_retryable(err.to_string()))
}

fn workflow_error(err: impl ToString) -> WorkflowTermination {
    ApplicationFailure::non_retryable(err.to_string()).into()
}

#[workflow]
#[derive(Default)]
pub struct P1ProbeWorkflow {
    approved: bool,
}

#[workflow]
#[derive(Default)]
pub struct P1DryRunWorkflow;

#[workflow]
#[derive(Default)]
pub struct P1LiveSeedanceWorkflow;

#[workflow_methods]
impl P1DryRunWorkflow {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, out_dir: String) -> WorkflowResult<String> {
        let prepared_out_dir = ctx
            .start_activity(
                P1ProbeActivities::prepare_p1_dry_run,
                out_dir,
                activity_opts(),
            )
            .await?;

        ctx.timer(Duration::from_secs(3)).await;

        let summary = ctx
            .start_activity(
                P1ProbeActivities::run_p1_dry_run,
                prepared_out_dir,
                dry_run_activity_opts(),
            )
            .await?;
        Ok(summary)
    }
}

#[workflow_methods]
impl P1LiveSeedanceWorkflow {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, out_dir: String) -> WorkflowResult<String> {
        let mut state = ctx
            .start_activity(
                P1ProbeActivities::submit_live_seedance_job,
                out_dir,
                live_submit_activity_opts(),
            )
            .await?;

        for _ in 1..=LIVE_MAX_POLLS {
            state = ctx
                .start_activity(
                    P1ProbeActivities::poll_live_seedance_job,
                    state,
                    live_poll_activity_opts(),
                )
                .await?;

            match state_kind(&state).as_deref() {
                Some("succeeded") => {
                    let summary = ctx
                        .start_activity(
                            P1ProbeActivities::persist_live_seedance_outputs,
                            state,
                            live_persist_activity_opts(),
                        )
                        .await?;
                    return Ok(summary);
                }
                Some("failed") => {
                    return Err(workflow_error(state_reason(&state).unwrap_or_else(|| {
                        "live Seedance job failed without provider reason".to_string()
                    })));
                }
                _ => {
                    ctx.timer(Duration::from_secs(LIVE_POLL_INTERVAL_S)).await;
                }
            }
        }

        Err(workflow_error(format!(
            "live Seedance task did not complete after {LIVE_MAX_POLLS} polls"
        )))
    }
}

#[workflow_methods]
impl P1ProbeWorkflow {
    #[run]
    pub async fn run(ctx: &mut WorkflowContext<Self>, shot_id: String) -> WorkflowResult<String> {
        let job_id = ctx
            .start_activity(
                P1ProbeActivities::submit_video_job,
                shot_id.clone(),
                activity_opts(),
            )
            .await?;

        for attempt in 1..=3u32 {
            let ready = ctx
                .start_activity(
                    P1ProbeActivities::poll_video_job,
                    (job_id.clone(), attempt),
                    activity_opts(),
                )
                .await?;
            if ready {
                break;
            }
            ctx.timer(Duration::from_secs(1)).await;
        }

        ctx.wait_condition(|state| state.approved).await;
        Ok(format!("{shot_id}:{job_id}:approved"))
    }

    #[signal]
    pub fn approve(&mut self, _ctx: &mut SyncWorkflowContext<Self>) {
        self.approved = true;
    }

    #[query]
    pub fn is_approved(&self, _ctx: &WorkflowContextView) -> bool {
        self.approved
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());
    match mode.as_str() {
        "worker" => run_worker().await?,
        "start" => start_workflow().await?,
        "signal" => signal_approval().await?,
        "result" => wait_result().await?,
        "dry-start" => start_dry_run_workflow().await?,
        "dry-result" => wait_dry_run_result().await?,
        "live-start" => start_live_seedance_workflow().await?,
        "live-result" => wait_live_seedance_result().await?,
        _ => print_help(),
    }
    Ok(())
}

async fn temporal_client() -> Result<Client, Box<dyn std::error::Error>> {
    let (conn_opts, client_opts) =
        ClientOptions::load_from_config(LoadClientConfigProfileOptions::default())?;
    let connection = Connection::connect(conn_opts).await?;
    Ok(Client::new(connection, client_opts)?)
}

async fn run_worker() -> Result<(), Box<dyn std::error::Error>> {
    let task_queue = task_queue();
    let runtime = CoreRuntime::new_assume_tokio(
        RuntimeOptions::builder()
            .telemetry_options(TelemetryOptions::builder().build())
            .build()?,
    )?;
    let client = temporal_client().await?;
    let worker_options = WorkerOptions::new(&task_queue)
        .register_workflow::<P1ProbeWorkflow>()?
        .register_workflow::<P1DryRunWorkflow>()?
        .register_workflow::<P1LiveSeedanceWorkflow>()?
        .register_activities(P1ProbeActivities)
        .build();
    let mut worker = Worker::new(&runtime, client, worker_options)?;
    println!("P1 Temporal probe worker started on task queue: {task_queue}");
    worker.run().await?;
    Ok(())
}

async fn start_dry_run_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let task_queue = task_queue();
    let workflow_id = dry_run_workflow_id();
    let out_dir = dry_run_out_dir();
    let client = temporal_client().await?;
    let handle = client
        .start_workflow(
            P1DryRunWorkflow::run,
            out_dir.clone(),
            WorkflowStartOptions::new(&task_queue, &workflow_id).build(),
        )
        .await?;
    println!(
        "Started P1 dry-run workflow {workflow_id} for {out_dir}, run_id: {:?}",
        handle.run_id()
    );
    Ok(())
}

async fn wait_dry_run_result() -> Result<(), Box<dyn std::error::Error>> {
    let workflow_id = dry_run_workflow_id();
    let client = temporal_client().await?;
    let handle = client.get_workflow_handle::<P1DryRunWorkflow>(&workflow_id);
    let result = handle
        .get_result(WorkflowGetResultOptions::default())
        .await?;
    println!("Dry-run workflow result: {result}");
    Ok(())
}

async fn start_live_seedance_workflow() -> Result<(), Box<dyn std::error::Error>> {
    require_live_opt_in()?;
    let task_queue = task_queue();
    let workflow_id = live_workflow_id();
    let out_dir = live_out_dir();
    let client = temporal_client().await?;
    let handle = client
        .start_workflow(
            P1LiveSeedanceWorkflow::run,
            out_dir.clone(),
            WorkflowStartOptions::new(&task_queue, &workflow_id).build(),
        )
        .await?;
    println!(
        "Started P1 live Seedance/TOS workflow {workflow_id} for {out_dir}, run_id: {:?}",
        handle.run_id()
    );
    Ok(())
}

async fn wait_live_seedance_result() -> Result<(), Box<dyn std::error::Error>> {
    let workflow_id = live_workflow_id();
    let client = temporal_client().await?;
    let handle = client.get_workflow_handle::<P1LiveSeedanceWorkflow>(&workflow_id);
    let result = handle
        .get_result(WorkflowGetResultOptions::default())
        .await?;
    println!("Live Seedance/TOS workflow result: {result}");
    Ok(())
}

async fn start_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let task_queue = task_queue();
    let workflow_id = workflow_id();
    let shot_id = shot_id();
    let client = temporal_client().await?;
    let handle = client
        .start_workflow(
            P1ProbeWorkflow::run,
            shot_id.clone(),
            WorkflowStartOptions::new(&task_queue, &workflow_id).build(),
        )
        .await?;
    println!(
        "Started P1 probe workflow {workflow_id} for {shot_id}, run_id: {:?}",
        handle.run_id()
    );
    Ok(())
}

async fn signal_approval() -> Result<(), Box<dyn std::error::Error>> {
    let workflow_id = workflow_id();
    let client = temporal_client().await?;
    let handle = client.get_workflow_handle::<P1ProbeWorkflow>(&workflow_id);
    handle
        .signal(
            P1ProbeWorkflow::approve,
            (),
            WorkflowSignalOptions::default(),
        )
        .await?;
    println!("Sent approval signal to workflow {workflow_id}");
    Ok(())
}

async fn wait_result() -> Result<(), Box<dyn std::error::Error>> {
    let workflow_id = workflow_id();
    let client = temporal_client().await?;
    let handle = client.get_workflow_handle::<P1ProbeWorkflow>(&workflow_id);
    let result = handle
        .get_result(WorkflowGetResultOptions::default())
        .await?;
    println!("Workflow result: {result}");
    Ok(())
}

fn task_queue() -> String {
    env_or_default("P1_TEMPORAL_TASK_QUEUE", DEFAULT_TASK_QUEUE)
}

fn workflow_id() -> String {
    env_or_default("P1_TEMPORAL_WORKFLOW_ID", DEFAULT_WORKFLOW_ID)
}

fn dry_run_workflow_id() -> String {
    env_or_default(
        "P1_TEMPORAL_DRY_RUN_WORKFLOW_ID",
        DEFAULT_DRY_RUN_WORKFLOW_ID,
    )
}

fn live_workflow_id() -> String {
    env_or_default("P1_TEMPORAL_LIVE_WORKFLOW_ID", DEFAULT_LIVE_WORKFLOW_ID)
}

fn dry_run_out_dir() -> String {
    std::env::var("P1_TEMPORAL_DRY_RUN_OUT_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join(dry_run_workflow_id())
                .to_string_lossy()
                .to_string()
        })
}

fn live_out_dir() -> String {
    std::env::var("P1_TEMPORAL_LIVE_OUT_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join(live_workflow_id())
                .to_string_lossy()
                .to_string()
        })
}

fn shot_id() -> String {
    env_or_default("P1_TEMPORAL_SHOT_ID", DEFAULT_SHOT_ID)
}

fn env_or_default(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn print_help() {
    println!("P1 Temporal proof probe");
    println!("Usage:");
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- worker"
    );
    println!("  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- start");
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- signal"
    );
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- result"
    );
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- dry-start"
    );
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- dry-result"
    );
    println!(
        "  $env:P1_TEMPORAL_ALLOW_REAL_ARK='1'; cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- live-start"
    );
    println!(
        "  cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- live-result"
    );
    println!();
    println!("Optional env:");
    println!("  P1_TEMPORAL_TASK_QUEUE   default: {DEFAULT_TASK_QUEUE}");
    println!("  P1_TEMPORAL_WORKFLOW_ID  default: {DEFAULT_WORKFLOW_ID}");
    println!("  P1_TEMPORAL_DRY_RUN_WORKFLOW_ID  default: {DEFAULT_DRY_RUN_WORKFLOW_ID}");
    println!("  P1_TEMPORAL_DRY_RUN_OUT_DIR      default: temp dir named after dry-run workflow");
    println!("  P1_TEMPORAL_LIVE_WORKFLOW_ID     default: {DEFAULT_LIVE_WORKFLOW_ID}");
    println!("  P1_TEMPORAL_LIVE_OUT_DIR         default: temp dir named after live workflow");
    println!("  P1_TEMPORAL_ALLOW_REAL_ARK       set to 1 to submit the paid live Seedance job");
    println!("  P1_TEMPORAL_SHOT_ID      default: {DEFAULT_SHOT_ID}");
    println!();
    println!("Run this against `temporal server start-dev`; kill/restart the worker before `signal` to verify recovery.");
    println!(
        "`live-start` is paid and also requires ARK config plus TOS env credentials; the workflow polls with Temporal timers."
    );
}

fn submit_live_seedance_job_activity(out_dir: &str) -> ncx_video_agent::Result<String> {
    require_live_opt_in()?;
    let (_tos_config, ark_key) =
        resolve_paid_seedance_prereqs(TosConfig::from_env, resolve_ark_api_key)?;
    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| ncx_video_agent::VideoAgentError::Ffmpeg(err.to_string()))?;
    let mut db = Database::open(live_db_path(&out_dir))?;
    seed_live_db(&db)?;

    let payload = live_seedance_payload();
    let reserve_cost = estimate_seedance_cost_cny(LIVE_DURATION_S, false) * 1.25;
    let mut ark = ArkClient::with_base_url(ark_key, ARK_BASE_URL, ReqwestArkTransport::new()?)?;
    let job = submit_seedance_job_once(
        &mut db,
        &mut ark,
        &SeedanceSubmitInput {
            project_id: LIVE_PROJECT_ID.to_string(),
            shot_id: LIVE_SHOT_ID.to_string(),
            attempt: 0,
            model: LIVE_MODEL.to_string(),
            payload: payload.clone(),
            reserve_cost,
        },
    )?;
    let task_id = job.record.provider_job_id.clone().ok_or_else(|| {
        ncx_video_agent::VideoAgentError::Ark("live Seedance job has no task id".to_string())
    })?;

    let state = json!({
        "kind": "submitted",
        "out_dir": out_dir.to_string_lossy(),
        "project_id": LIVE_PROJECT_ID,
        "shot_id": LIVE_SHOT_ID,
        "job_id": job.record.id,
        "task_id": task_id,
        "model": LIVE_MODEL,
        "payload": payload,
        "submitted_to_provider": job.submitted_to_provider,
        "submitted_at_unix_ms": now_unix_ms(),
    });
    write_live_marker(&out_dir, "temporal_live_submit_marker.json", &state)?;
    Ok(state.to_string())
}

fn poll_live_seedance_job_activity(state_json: &str) -> ncx_video_agent::Result<String> {
    require_live_opt_in()?;
    let state = parse_state(state_json)?;
    let out_dir = required_string(&state, "out_dir")?;
    let project_id = required_string(&state, "project_id")?;
    let shot_id = required_string(&state, "shot_id")?;
    let job_id = required_string(&state, "job_id")?;
    let task_id = required_string(&state, "task_id")?;
    let model = required_string(&state, "model")?;
    let submitted_at_unix_ms = state
        .get("submitted_at_unix_ms")
        .and_then(Value::as_i64)
        .unwrap_or_else(now_unix_ms);
    let payload = state
        .get("payload")
        .cloned()
        .unwrap_or_else(live_seedance_payload);

    let mut db = Database::open(live_db_path(Path::new(&out_dir)))?;
    let ark_key = resolve_ark_api_key()?;
    let mut ark = ArkClient::with_base_url(ark_key, ARK_BASE_URL, ReqwestArkTransport::new()?)?;
    match ncx_video_agent::poll_seedance_job_once(
        &mut db,
        &mut ark,
        &project_id,
        &job_id,
        &task_id,
    )? {
        SeedancePollOutcome::Running { status } => {
            let state = json!({
            "kind": "running",
            "out_dir": out_dir,
            "project_id": project_id,
            "shot_id": shot_id,
            "job_id": job_id,
            "task_id": task_id,
            "model": model,
            "payload": payload,
            "provider_status": status,
            "submitted_at_unix_ms": submitted_at_unix_ms,
            });
            write_live_marker(
                Path::new(state["out_dir"].as_str().unwrap_or_default()),
                "temporal_live_poll_marker.json",
                &state,
            )?;
            Ok(state.to_string())
        }
        SeedancePollOutcome::Succeeded { video_url, usage } => {
            record_job_latency_ms(
                db.connection(),
                &job_id,
                elapsed_since_unix_ms(submitted_at_unix_ms),
            )?;
            if let Some(cost) = seedance_cost_cny(&usage, false) {
                settle_budget(
                    db.connection_mut(),
                    &project_id,
                    &job_id,
                    cost,
                    total_tokens(&usage),
                )?;
            }
            let state = json!({
                "kind": "succeeded",
                "out_dir": out_dir,
                "project_id": project_id,
                "shot_id": shot_id,
                "job_id": job_id,
                "task_id": task_id,
                "model": model,
                "payload": payload,
                "video_url": video_url,
                "usage": usage,
                "submitted_at_unix_ms": submitted_at_unix_ms,
            });
            write_live_marker(
                Path::new(state["out_dir"].as_str().unwrap_or_default()),
                "temporal_live_poll_marker.json",
                &state,
            )?;
            Ok(state.to_string())
        }
        SeedancePollOutcome::Failed { status, reason } => {
            record_job_latency_ms(
                db.connection(),
                &job_id,
                elapsed_since_unix_ms(submitted_at_unix_ms),
            )?;
            let state = json!({
            "kind": "failed",
            "out_dir": out_dir,
            "project_id": project_id,
            "shot_id": shot_id,
            "job_id": job_id,
            "task_id": task_id,
            "model": model,
            "payload": payload,
            "provider_status": status,
            "reason": reason,
            "submitted_at_unix_ms": submitted_at_unix_ms,
            });
            write_live_marker(
                Path::new(state["out_dir"].as_str().unwrap_or_default()),
                "temporal_live_poll_marker.json",
                &state,
            )?;
            Ok(state.to_string())
        }
    }
}

fn persist_live_seedance_outputs_activity(state_json: &str) -> ncx_video_agent::Result<String> {
    require_live_opt_in()?;
    let state = parse_state(state_json)?;
    if state.get("kind").and_then(Value::as_str) != Some("succeeded") {
        return Err(ncx_video_agent::VideoAgentError::Ark(
            "persist requested before live Seedance job succeeded".to_string(),
        ));
    }

    let out_dir = PathBuf::from(required_string(&state, "out_dir")?);
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| ncx_video_agent::VideoAgentError::Ffmpeg(err.to_string()))?;
    let db_path = live_db_path(&out_dir);
    let db = Database::open(&db_path)?;
    let project_id = required_string(&state, "project_id")?;
    let shot_id = required_string(&state, "shot_id")?;
    let task_id = required_string(&state, "task_id")?;
    let video_url = required_string(&state, "video_url")?;
    let usage = state.get("usage").cloned().unwrap_or_else(|| json!({}));
    let payload = state
        .get("payload")
        .cloned()
        .unwrap_or_else(live_seedance_payload);

    let tos_config = TosConfig::from_env()?;
    let mut tos = TosClient::new(tos_config, ReqwestTosTransport::new()?);
    let tos_key = live_video_tos_key(&task_id);
    let artifact_id = live_video_artifact_id(&task_id);
    if !artifact_exists(db.connection(), &artifact_id)? {
        let mut downloader = ReqwestVideoDownloader::new()?;
        persist_seedance_video_artifact(
            &db,
            &mut tos,
            &mut downloader,
            &SeedanceArtifactInput {
                artifact_id: artifact_id.clone(),
                shot_id: shot_id.clone(),
                tos_key: tos_key.clone(),
                ark_task_id: task_id.clone(),
                video_url,
                usage,
                params_json: payload,
            },
        )?;
    }

    let local_tos_copy = out_dir.join("seedance_tos_roundtrip.mp4");
    let tos_bytes = tos.get_object(&tos_key)?;
    std::fs::write(&local_tos_copy, tos_bytes).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "write TOS roundtrip video {} failed: {err}",
            local_tos_copy.display()
        ))
    })?;

    let media_report = validate_video_file_l0(&local_tos_copy, Some(LIVE_DURATION_S), 1.0, false)?;
    let media_verdict = if media_report.passed {
        "pass"
    } else {
        "repair"
    };
    record_validation_once(
        &db,
        &ValidationInput {
            id: live_video_validation_id(&task_id),
            artifact_id: artifact_id.clone(),
            stage: "seedance_media_l0".to_string(),
            gate_version: "p1-temporal-live".to_string(),
            verdict: media_verdict.to_string(),
            confidence: Some(1.0),
            aesthetic_score: None,
            layers_json: json!({
                "tos_roundtrip_path": local_tos_copy.to_string_lossy(),
                "media_l0": media_report.layers_json(),
            }),
            escalate_reason: (!media_report.passed).then(|| media_report.reasons.join("; ")),
        },
    )?;
    if !media_report.passed {
        return Err(ncx_video_agent::VideoAgentError::L0Rejected(format!(
            "live Temporal Seedance TOS artifact failed media L0: {}",
            media_report.reasons.join("; ")
        )));
    }

    assert_artifacts_passed(db.connection(), &[&artifact_id])?;
    let rough = build_rough_cut(
        &[RenderedShot {
            shot_id: shot_id.clone(),
            clip_path: Some(local_tos_copy.clone()),
            subtitle_path: None,
            audio_path: None,
            rerun_context: json!({
                "ark_task_id": task_id.clone(),
                "seedance_artifact_id": artifact_id.clone(),
                "temporal_workflow": true,
            }),
        }],
        &out_dir,
    )?;
    let rough_cut_path = rough.rough_cut_path.ok_or_else(|| {
        ncx_video_agent::VideoAgentError::Ffmpeg(
            "live Temporal Seedance/TOS workflow produced no rough_cut".to_string(),
        )
    })?;
    let rough_media_report =
        validate_video_file_l0(&rough_cut_path, Some(LIVE_DURATION_S), 1.0, false)?;
    let rough_bytes = std::fs::read(&rough_cut_path).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "read rough_cut {} failed: {err}",
            rough_cut_path.display()
        ))
    })?;
    let rough_tos_key = live_rough_tos_key(&task_id);
    let rough_object = tos.put_object(&rough_tos_key, &rough_bytes, "video/mp4")?;
    let rough_artifact_id = live_rough_artifact_id(&task_id);
    if !artifact_exists(db.connection(), &rough_artifact_id)? {
        db.create_project_artifact(
            &rough_artifact_id,
            &project_id,
            "rough_cut",
            &rough_object.uri,
            &rough_object.content_hash,
            &json!({
                "seedance_video_artifact_id": artifact_id,
                "assembly_manifest": rough.assembly_manifest_path.to_string_lossy(),
                "failed_shots": rough.failed_shots_path.to_string_lossy(),
                "partial_delivery": true,
                "source_tos_key": tos_key,
                "temporal_workflow": true,
            })
            .to_string(),
        )?;
    }

    let rough_verdict = if rough_media_report.passed {
        "pass"
    } else {
        "repair"
    };
    record_validation_once(
        &db,
        &ValidationInput {
            id: live_rough_validation_id(&task_id),
            artifact_id: rough_artifact_id.clone(),
            stage: "rough_cut_media_l0".to_string(),
            gate_version: "p1-temporal-live".to_string(),
            verdict: rough_verdict.to_string(),
            confidence: Some(1.0),
            aesthetic_score: None,
            layers_json: json!({
                "rough_cut_path": rough_cut_path.to_string_lossy(),
                "media_l0": rough_media_report.layers_json(),
            }),
            escalate_reason: (!rough_media_report.passed)
                .then(|| rough_media_report.reasons.join("; ")),
        },
    )?;
    if !rough_media_report.passed {
        return Err(ncx_video_agent::VideoAgentError::L0Rejected(format!(
            "live Temporal Seedance rough_cut failed media L0: {}",
            rough_media_report.reasons.join("; ")
        )));
    }

    assert_artifacts_passed(db.connection(), &[&rough_artifact_id])?;
    let trace = export_project_trace(db.connection(), &project_id)?;
    let trace_path = out_dir.join("trace.json");
    std::fs::write(&trace_path, serde_json::to_string_pretty(&trace)?).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "write trace.json {} failed: {err}",
            trace_path.display()
        ))
    })?;
    let shot_trace = export_project_shot_trace(db.connection(), &project_id, &shot_id)?;
    let shot_trace_path = out_dir.join(format!("trace_{shot_id}.json"));
    std::fs::write(&shot_trace_path, serde_json::to_string_pretty(&shot_trace)?).map_err(
        |err| {
            ncx_video_agent::VideoAgentError::Ffmpeg(format!(
                "write shot trace {} failed: {err}",
                shot_trace_path.display()
            ))
        },
    )?;
    let video_tos_uri = artifact_tos_uri(db.connection(), &artifact_id)?;

    Ok(format!(
        "rough_cut={};trace={};shot_trace={};db={};tos={};rough_cut_tos={}",
        rough_cut_path.display(),
        trace_path.display(),
        shot_trace_path.display(),
        db_path.display(),
        video_tos_uri,
        rough_object.uri
    ))
}

fn require_live_opt_in() -> ncx_video_agent::Result<()> {
    let allowed = std::env::var("P1_TEMPORAL_ALLOW_REAL_ARK")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes"));
    if allowed {
        Ok(())
    } else {
        Err(ncx_video_agent::VideoAgentError::Ark(
            "live Temporal Seedance workflow is paid; set P1_TEMPORAL_ALLOW_REAL_ARK=1 to submit"
                .to_string(),
        ))
    }
}

fn resolve_ark_api_key() -> ncx_video_agent::Result<String> {
    P1ExternalConfig::load()
        .ark_api_key
        .map(|setting| setting.value)
        .ok_or_else(|| {
            ncx_video_agent::VideoAgentError::Ark(
                "missing ARK_API_KEY, NANOCODEX_ARK_API_KEY, or ncx-config ark_api_key".to_string(),
            )
        })
}

fn seed_live_db(db: &Database) -> ncx_video_agent::Result<()> {
    db.connection().execute(
        "INSERT OR IGNORE INTO projects(id, brief_json, status, budget_total)
         VALUES(?1, ?2, 'new', ?3)",
        params![LIVE_PROJECT_ID, json!({}).to_string(), 20.0],
    )?;
    db.connection().execute(
        "INSERT OR IGNORE INTO chapters(id, project_id, plan_json, status)
         VALUES(?1, ?2, ?3, 'new')",
        params![
            LIVE_CHAPTER_ID,
            LIVE_PROJECT_ID,
            json!({"duration_s": LIVE_DURATION_S}).to_string()
        ],
    )?;
    db.connection().execute(
        "INSERT OR IGNORE INTO scenes(id, chapter_id, plan_json, status)
         VALUES(?1, ?2, ?3, 'new')",
        params![
            LIVE_SCENE_ID,
            LIVE_CHAPTER_ID,
            json!({"duration_s": LIVE_DURATION_S}).to_string()
        ],
    )?;
    db.connection().execute(
        "INSERT OR IGNORE INTO shots(
            id, scene_id, plan_json, status, continuity_in, continuity_out,
            risk_level, is_hero, tier
         )
         VALUES(?1, ?2, ?3, 'new', 'start', 'end', 'normal', 0, 'standard')",
        params![
            LIVE_SHOT_ID,
            LIVE_SCENE_ID,
            json!({"duration_s": LIVE_DURATION_S}).to_string()
        ],
    )?;
    Ok(())
}

fn live_seedance_payload() -> Value {
    json!({
        "model": LIVE_MODEL,
        "content": [{
            "type": "text",
            "text": "A clean 5-second establishing shot of a quiet modern studio, soft daylight, no text overlays"
        }],
        "ratio": "16:9",
        "duration": LIVE_DURATION_S as i64,
        "watermark": false
    })
}

fn live_db_path(out_dir: &Path) -> PathBuf {
    out_dir.join("video_agent_temporal_live.sqlite")
}

fn write_live_marker(out_dir: &Path, name: &str, value: &Value) -> ncx_video_agent::Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|err| ncx_video_agent::VideoAgentError::Ffmpeg(err.to_string()))?;
    std::fs::write(out_dir.join(name), serde_json::to_string_pretty(value)?)
        .map_err(|err| ncx_video_agent::VideoAgentError::Ffmpeg(err.to_string()))?;
    Ok(())
}

fn parse_state(state_json: &str) -> ncx_video_agent::Result<Value> {
    serde_json::from_str(state_json).map_err(Into::into)
}

fn required_string(state: &Value, key: &str) -> ncx_video_agent::Result<String> {
    state
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            ncx_video_agent::VideoAgentError::Ark(format!("live workflow state missing {key}"))
        })
}

fn state_kind(state_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(state_json)
        .ok()
        .and_then(|value| {
            value
                .get("kind")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn state_reason(state_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(state_json)
        .ok()
        .and_then(|value| {
            value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn artifact_exists(
    conn: &rusqlite::Connection,
    artifact_id: &str,
) -> ncx_video_agent::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE id=?1",
        params![artifact_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn artifact_tos_uri(
    conn: &rusqlite::Connection,
    artifact_id: &str,
) -> ncx_video_agent::Result<String> {
    conn.query_row(
        "SELECT tos_key FROM artifacts WHERE id=?1",
        params![artifact_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn validation_exists(
    conn: &rusqlite::Connection,
    validation_id: &str,
) -> ncx_video_agent::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM validation_records WHERE id=?1",
        params![validation_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn record_validation_once(db: &Database, input: &ValidationInput) -> ncx_video_agent::Result<()> {
    if validation_exists(db.connection(), &input.id)? {
        Ok(())
    } else {
        record_validation(db.connection(), input)
    }
}

fn live_video_tos_key(task_id: &str) -> String {
    format!(
        "ncx-video-agent/p1-temporal-live/{}/shot_01.mp4",
        sanitize_id(task_id)
    )
}

fn live_rough_tos_key(task_id: &str) -> String {
    format!(
        "ncx-video-agent/p1-temporal-live/{}/rough_cut.mp4",
        sanitize_id(task_id)
    )
}

fn live_video_artifact_id(task_id: &str) -> String {
    format!("artifact_shot_01_seedance_{}", sanitize_id(task_id))
}

fn live_rough_artifact_id(task_id: &str) -> String {
    format!(
        "artifact_p1_temporal_seedance_rough_cut_{}",
        sanitize_id(task_id)
    )
}

fn live_video_validation_id(task_id: &str) -> String {
    format!(
        "validation_shot_01_seedance_media_l0_{}",
        sanitize_id(task_id)
    )
}

fn live_rough_validation_id(task_id: &str) -> String {
    format!(
        "validation_p1_temporal_seedance_rough_cut_media_l0_{}",
        sanitize_id(task_id)
    )
}

fn sanitize_id(value: &str) -> String {
    let clean = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if clean.trim_matches('_').is_empty() {
        "task".to_string()
    } else {
        clean
    }
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn elapsed_since_unix_ms(started_unix_ms: i64) -> i64 {
    now_unix_ms().saturating_sub(started_unix_ms).max(0)
}

fn total_tokens(usage: &Value) -> i64 {
    usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}
