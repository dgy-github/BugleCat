use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use ncx_video_agent::{
    assert_artifacts_passed, build_rough_cut, estimate_seedance_cost_cny,
    export_project_shot_trace, export_project_trace, persist_seedance_video_artifact,
    record_job_latency_ms, record_validation, resolve_paid_seedance_prereqs, seedance_cost_cny,
    settle_budget, submit_seedance_job_once, validate_video_file_l0, ArkClient, Database,
    P1ExternalConfig, RenderedShot, ReqwestArkTransport, ReqwestTosTransport,
    ReqwestVideoDownloader, SeedanceArtifactInput, SeedancePollOutcome, SeedanceSubmitInput,
    TosClient, TosConfig, ValidationInput, ARK_BASE_URL,
};
use serde_json::json;

const MODEL: &str = "doubao-seedance-2-0-fast-260128";
const PROJECT_ID: &str = "p1_seedance_tos_smoke";
const SHOT_ID: &str = "shot_01";

fn main() -> ExitCode {
    if !std::env::args().any(|arg| arg == "--submit-real-ark-job") {
        eprintln!("This smoke submits a real Seedance job and may spend CNY.");
        eprintln!(
            "Run explicitly: cargo run -p ncx-video-agent --bin p1_seedance_tos_smoke -- --submit-real-ark-job [out_dir]"
        );
        return ExitCode::FAILURE;
    }

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("p1 Seedance/TOS smoke failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> ncx_video_agent::Result<()> {
    let (tos_config, ark_key) = resolve_paid_seedance_prereqs(TosConfig::from_env, || {
        P1ExternalConfig::load()
            .ark_api_key
            .map(|setting| setting.value)
            .ok_or_else(|| {
                ncx_video_agent::VideoAgentError::Ark(
                    "missing ARK_API_KEY, NANOCODEX_ARK_API_KEY, or ncx-config ark_api_key"
                        .to_string(),
                )
            })
    })?;
    let out_dir = std::env::args()
        .skip_while(|arg| arg != "--submit-real-ark-job")
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ncx-video-agent-p1-seedance-tos-smoke"));
    std::fs::create_dir_all(&out_dir)
        .map_err(|err| ncx_video_agent::VideoAgentError::Ffmpeg(err.to_string()))?;
    let db_path = out_dir.join(format!("video_agent_{}.sqlite", std::process::id()));
    let mut db = Database::open(&db_path)?;
    seed_db(&db)?;

    let mut tos = TosClient::new(tos_config, ReqwestTosTransport::new()?);
    let mut ark = ArkClient::with_base_url(ark_key, ARK_BASE_URL, ReqwestArkTransport::new()?)?;
    let payload = json!({
        "model": MODEL,
        "content": [{
            "type": "text",
            "text": "A clean 5-second establishing shot of a quiet modern studio, soft daylight, no text overlays"
        }],
        "ratio": "16:9",
        "duration": 5,
        "watermark": false
    });
    let reserve_cost = estimate_seedance_cost_cny(5.0, false) * 1.25;
    let job_started = Instant::now();
    let job = submit_seedance_job_once(
        &mut db,
        &mut ark,
        &SeedanceSubmitInput {
            project_id: PROJECT_ID.to_string(),
            shot_id: SHOT_ID.to_string(),
            attempt: 0,
            model: MODEL.to_string(),
            payload: payload.clone(),
            reserve_cost,
        },
    )?;
    let task_id =
        job.record.provider_job_id.clone().ok_or_else(|| {
            ncx_video_agent::VideoAgentError::Ark("job has no task id".to_string())
        })?;
    println!("submitted Seedance task: {task_id}");

    let status = poll_until_succeeded(&mut db, &mut ark, &job.record.id, &task_id, job_started)?;
    let SeedancePollOutcome::Succeeded { video_url, usage } = status else {
        unreachable!("poll_until_succeeded only returns succeeded outcomes")
    };
    record_job_latency_ms(db.connection(), &job.record.id, elapsed_ms(job_started))?;
    if let Some(cost) = seedance_cost_cny(&usage, false) {
        settle_budget(
            db.connection_mut(),
            PROJECT_ID,
            &job.record.id,
            cost,
            total_tokens(&usage),
        )?;
        println!("settled Seedance cost: CNY {cost:.4}");
    } else {
        println!("Seedance usage had no positive total_tokens; budget left reserved for audit");
    }

    let mut downloader = ReqwestVideoDownloader::new()?;
    let tos_key = format!(
        "ncx-video-agent/p1-seedance-tos-smoke/{}-{}.mp4",
        std::process::id(),
        task_id
    );
    let artifact = persist_seedance_video_artifact(
        &db,
        &mut tos,
        &mut downloader,
        &SeedanceArtifactInput {
            artifact_id: "artifact_shot_01_seedance".to_string(),
            shot_id: SHOT_ID.to_string(),
            tos_key: tos_key.clone(),
            ark_task_id: task_id.clone(),
            video_url,
            usage,
            params_json: payload,
        },
    )?;
    let local_tos_copy = out_dir.join("seedance_tos_roundtrip.mp4");
    let tos_bytes = tos.get_object(&tos_key)?;
    std::fs::write(&local_tos_copy, tos_bytes).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "write TOS roundtrip video {} failed: {err}",
            local_tos_copy.display()
        ))
    })?;
    let media_report = validate_video_file_l0(&local_tos_copy, Some(5.0), 1.0, false)?;
    let verdict = if media_report.passed {
        "pass"
    } else {
        "repair"
    };
    record_validation(
        db.connection(),
        &ValidationInput {
            id: "validation_shot_01_seedance_media_l0".to_string(),
            artifact_id: artifact.artifact_id.clone(),
            stage: "seedance_media_l0".to_string(),
            gate_version: "p1-real-smoke".to_string(),
            verdict: verdict.to_string(),
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
            "Seedance TOS artifact failed media L0: {}",
            media_report.reasons.join("; ")
        )));
    }
    assert_artifacts_passed(db.connection(), &[&artifact.artifact_id])?;
    let rough = build_rough_cut(
        &[RenderedShot {
            shot_id: SHOT_ID.to_string(),
            clip_path: Some(local_tos_copy.clone()),
            subtitle_path: None,
            audio_path: None,
            rerun_context: json!({
                "ark_task_id": task_id,
                "seedance_artifact_id": artifact.artifact_id,
            }),
        }],
        &out_dir,
    )?;
    let rough_cut_path = rough.rough_cut_path.ok_or_else(|| {
        ncx_video_agent::VideoAgentError::Ffmpeg(
            "live Seedance/TOS smoke produced no rough_cut".to_string(),
        )
    })?;
    let rough_media_report = validate_video_file_l0(&rough_cut_path, Some(5.0), 1.0, false)?;
    let rough_bytes = std::fs::read(&rough_cut_path).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "read rough_cut {} failed: {err}",
            rough_cut_path.display()
        ))
    })?;
    let rough_tos_key = format!(
        "ncx-video-agent/p1-seedance-tos-smoke/{}-rough-cut.mp4",
        std::process::id()
    );
    let rough_object = tos.put_object(&rough_tos_key, &rough_bytes, "video/mp4")?;
    let rough_artifact_id = "artifact_p1_seedance_rough_cut";
    db.create_project_artifact(
        rough_artifact_id,
        PROJECT_ID,
        "rough_cut",
        &rough_object.uri,
        &rough_object.content_hash,
        &json!({
            "seedance_video_artifact_id": "artifact_shot_01_seedance",
            "assembly_manifest": rough.assembly_manifest_path.to_string_lossy(),
            "failed_shots": rough.failed_shots_path.to_string_lossy(),
            "partial_delivery": true,
            "source_tos_key": tos_key,
        })
        .to_string(),
    )?;
    let rough_verdict = if rough_media_report.passed {
        "pass"
    } else {
        "repair"
    };
    record_validation(
        db.connection(),
        &ValidationInput {
            id: "validation_p1_seedance_rough_cut_media_l0".to_string(),
            artifact_id: rough_artifact_id.to_string(),
            stage: "rough_cut_media_l0".to_string(),
            gate_version: "p1-real-smoke".to_string(),
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
            "live Seedance rough_cut failed media L0: {}",
            rough_media_report.reasons.join("; ")
        )));
    }
    assert_artifacts_passed(db.connection(), &[rough_artifact_id])?;
    let trace = export_project_trace(db.connection(), PROJECT_ID)?;
    let trace_path = out_dir.join("trace.json");
    std::fs::write(&trace_path, serde_json::to_string_pretty(&trace)?).map_err(|err| {
        ncx_video_agent::VideoAgentError::Ffmpeg(format!(
            "write trace.json {} failed: {err}",
            trace_path.display()
        ))
    })?;
    let shot_trace = export_project_shot_trace(db.connection(), PROJECT_ID, SHOT_ID)?;
    let shot_trace_path = out_dir.join(format!("trace_{SHOT_ID}.json"));
    std::fs::write(&shot_trace_path, serde_json::to_string_pretty(&shot_trace)?).map_err(
        |err| {
            ncx_video_agent::VideoAgentError::Ffmpeg(format!(
                "write shot trace {} failed: {err}",
                shot_trace_path.display()
            ))
        },
    )?;
    println!("db: {}", db_path.display());
    println!("artifact: {}", artifact.artifact_id);
    println!("tos: {}", artifact.tos_uri);
    println!("content_hash: {}", artifact.content_hash);
    println!("size_bytes: {}", artifact.size_bytes);
    println!("media_l0: pass");
    println!("tos_roundtrip_video: {}", local_tos_copy.display());
    println!("rough_cut: {}", rough_cut_path.display());
    println!("rough_cut_tos: {}", rough_object.uri);
    println!("failed_shots: {}", rough.failed_shots_path.display());
    println!(
        "assembly_manifest: {}",
        rough.assembly_manifest_path.display()
    );
    println!("trace: {}", trace_path.display());
    println!("shot_trace: {}", shot_trace_path.display());
    Ok(())
}

fn seed_db(db: &Database) -> ncx_video_agent::Result<()> {
    db.create_project(PROJECT_ID, 20.0)?;
    db.create_chapter("chapter_01", PROJECT_ID, "{\"duration_s\":5}")?;
    db.create_scene("scene_01", "chapter_01", "{\"duration_s\":5}")?;
    db.create_shot(
        SHOT_ID,
        "scene_01",
        "{\"duration_s\":5}",
        Some("start"),
        Some("end"),
        false,
        "standard",
    )?;
    Ok(())
}

fn poll_until_succeeded(
    db: &mut Database,
    ark: &mut ArkClient<ReqwestArkTransport>,
    job_id: &str,
    task_id: &str,
    started: Instant,
) -> ncx_video_agent::Result<SeedancePollOutcome> {
    for attempt in 1..=60 {
        let outcome =
            ncx_video_agent::poll_seedance_job_once(db, ark, PROJECT_ID, job_id, task_id)?;
        match &outcome {
            SeedancePollOutcome::Succeeded { .. } => {
                println!("poll {attempt}: succeeded");
                return Ok(outcome);
            }
            SeedancePollOutcome::Failed { status, reason } => {
                println!("poll {attempt}: {status}");
                record_job_latency_ms(db.connection(), job_id, elapsed_ms(started))?;
                return Err(ncx_video_agent::VideoAgentError::Ark(format!(
                    "task {task_id} failed: {reason}"
                )));
            }
            SeedancePollOutcome::Running { status } => {
                println!("poll {attempt}: {status}");
                thread::sleep(Duration::from_secs(6));
            }
        }
    }
    record_job_latency_ms(db.connection(), job_id, elapsed_ms(started))?;
    Err(ncx_video_agent::VideoAgentError::Ark(format!(
        "task {task_id} still running after 60 polls"
    )))
}

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().min(i64::MAX as u128) as i64
}

fn total_tokens(usage: &serde_json::Value) -> i64 {
    usage
        .get("total_tokens")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0)
}
