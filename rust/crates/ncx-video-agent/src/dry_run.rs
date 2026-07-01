use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db::Database;
use crate::edit::{build_rough_cut, RenderedShot};
use crate::jobs::{record_job_latency_ms, settle_budget, submit_job_once};
use crate::media::validate_video_file_l0;
use crate::node::{p1_agent_node_spec, ContextPacket, P1AgentNode};
use crate::structured::{
    chapter_budgets_from_artifact, insert_project_artifact,
    record_structured_agent_validation_if_pass, shot_ids_from_artifact, validate_assets_artifact,
    validate_brief_artifact, validate_chapters_artifact, validate_shots_artifact,
    AgentArtifactKind, StructuredValidationReport,
};
use crate::text_separation::{separate_text_and_voice, OverlayText, ShotTextSpec};
use crate::trace::{export_project_shot_trace, export_project_trace};
use crate::validation::{record_validation, ValidationInput};
use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct LocalDryRunOutput {
    pub db_path: PathBuf,
    pub rough_cut_path: PathBuf,
    pub failed_shots_path: PathBuf,
    pub assembly_manifest_path: PathBuf,
    pub trace_path: PathBuf,
    pub shot_trace_paths: Vec<PathBuf>,
}

pub fn run_local_p1_dry_run(out_dir: impl AsRef<Path>) -> Result<LocalDryRunOutput> {
    let out_dir = out_dir.as_ref();
    std::fs::create_dir_all(out_dir)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("create dry-run dir failed: {err}")))?;

    let db_path = out_dir.join("video_agent.sqlite");
    remove_previous_sqlite(&db_path)?;
    let mut db = Database::open(&db_path)?;
    seed_project(&db)?;
    let shot_specs = vec![
        ShotTextSpec {
            shot_id: "shot_01".to_string(),
            visual_prompt: "A clean establishing shot of a quiet studio".to_string(),
            duration_s: 0.4,
            overlays: vec![OverlayText {
                text: "第一幕".to_string(),
                start_s: 0.05,
                end_s: 0.35,
            }],
            dialogue: vec!["旁白：这是本地 dry-run".to_string()],
        },
        ShotTextSpec {
            shot_id: "shot_02".to_string(),
            visual_prompt: "A close detail shot with no visible writing".to_string(),
            duration_s: 0.4,
            overlays: vec![],
            dialogue: vec![],
        },
    ];
    seed_agent_artifacts(&db, &shot_specs)?;

    let mut rendered = Vec::new();
    for (idx, spec) in shot_specs.iter().enumerate() {
        let plan_json = json!({
            "duration_s": spec.duration_s,
            "requires_chinese": !spec.overlays.is_empty() || !spec.dialogue.is_empty(),
            "subtitle": spec.overlays.first().map(|o| o.text.as_str()).unwrap_or(""),
        });
        db.create_shot(
            &spec.shot_id,
            "scene_01",
            &plan_json.to_string(),
            (idx == 0).then_some("start"),
            (idx + 1 == shot_specs.len()).then_some("end"),
            idx == 0,
            if idx == 0 { "hero" } else { "standard" },
        )?;

        let separated = separate_text_and_voice(spec, out_dir.join("text"))?;
        let tts_audio_path = if separated.tts_requests.is_empty() {
            None
        } else {
            let path = out_dir
                .join("text")
                .join(format!("{}_tts_placeholder.wav", spec.shot_id));
            make_local_tts_placeholder_audio(&path, spec.duration_s)?;
            Some(path)
        };
        let params = json!({
            "prompt": separated.clean_generation_prompt,
            "duration_s": spec.duration_s,
            "dry_run": true,
        });
        let job_started = Instant::now();
        let job = submit_job_once(
            db.connection_mut(),
            "project_p1_dry_run",
            &spec.shot_id,
            0,
            &params,
            "dry-run",
            "ffmpeg-color",
            0.0,
            || Ok(format!("dry-run-{}", spec.shot_id)),
        )?;
        settle_budget(
            db.connection_mut(),
            "project_p1_dry_run",
            &job.record.id,
            0.0,
            0,
        )?;

        let clip_path = out_dir.join(format!("{}.mp4", spec.shot_id));
        make_color_clip(
            &clip_path,
            if idx == 0 { "green" } else { "blue" },
            spec.duration_s,
        )?;
        let media_report = validate_video_file_l0(&clip_path, Some(spec.duration_s), 0.15, false)?;
        if !media_report.passed {
            return Err(VideoAgentError::L0Rejected(format!(
                "dry-run clip {} failed media L0: {}",
                spec.shot_id,
                media_report.reasons.join("; ")
            )));
        }
        record_job_latency_ms(db.connection(), &job.record.id, elapsed_ms(job_started))?;
        let artifact_id = format!("artifact_{}", spec.shot_id);
        db.create_artifact(
            &artifact_id,
            Some(&spec.shot_id),
            "video",
            &format!("local://dry-run/{}.mp4", spec.shot_id),
            &local_file_hash_marker(&clip_path),
            &params.to_string(),
        )?;
        record_validation(
            db.connection(),
            &ValidationInput {
                id: format!("validation_{}", spec.shot_id),
                artifact_id,
                stage: "dry_run_l0".to_string(),
                gate_version: "p1-dry-run".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({
                    "dry_run": true,
                    "media_l0": media_report.layers_json(),
                }),
                escalate_reason: None,
            },
        )?;

        rendered.push(RenderedShot {
            shot_id: spec.shot_id.clone(),
            clip_path: Some(clip_path),
            subtitle_path: separated.srt_path,
            audio_path: tts_audio_path.clone(),
            rerun_context: json!({
                "shot_text_spec": {
                    "visual_prompt": spec.visual_prompt,
                    "duration_s": spec.duration_s,
                },
                "tts_audio_path": tts_audio_path.as_ref().map(|path| path.to_string_lossy().to_string()),
                "tts_placeholder": tts_audio_path.is_some(),
                "tts_requests": separated.tts_requests.iter().map(|req| req.text.clone()).collect::<Vec<_>>(),
            }),
        });
    }

    let rough = build_rough_cut(&rendered, out_dir)?;
    let rough_cut_path = rough
        .rough_cut_path
        .ok_or_else(|| VideoAgentError::Ffmpeg("dry-run produced no rough_cut".to_string()))?;
    let rough_media_report = validate_video_file_l0(
        &rough_cut_path,
        Some(0.8),
        0.2,
        rendered.iter().any(|shot| shot.audio_path.is_some()),
    )?;
    if !rough_media_report.passed {
        return Err(VideoAgentError::L0Rejected(format!(
            "dry-run rough_cut failed media L0: {}",
            rough_media_report.reasons.join("; ")
        )));
    }
    db.create_project_artifact(
        "artifact_rough_cut",
        "project_p1_dry_run",
        "rough_cut",
        "local://dry-run/rough_cut.mp4",
        &sha256_file_hash_marker(&rough_cut_path)?,
        &json!({
            "assembly_manifest": rough.assembly_manifest_path.to_string_lossy(),
            "failed_shots": rough.failed_shots_path.to_string_lossy(),
            "partial_delivery": true,
        })
        .to_string(),
    )?;
    record_validation(
        db.connection(),
        &ValidationInput {
            id: "validation_rough_cut_media_l0".to_string(),
            artifact_id: "artifact_rough_cut".to_string(),
            stage: "rough_cut_media_l0".to_string(),
            gate_version: "p1-dry-run".to_string(),
            verdict: "pass".to_string(),
            confidence: Some(1.0),
            aesthetic_score: None,
            layers_json: json!({
                "dry_run": true,
                "media_l0": rough_media_report.layers_json(),
            }),
            escalate_reason: None,
        },
    )?;
    let trace = export_project_trace(db.connection(), "project_p1_dry_run")?;
    let trace_path = out_dir.join("trace.json");
    std::fs::write(&trace_path, serde_json::to_string_pretty(&trace)?)
        .map_err(|err| VideoAgentError::Ffmpeg(format!("write trace.json failed: {err}")))?;
    let mut shot_trace_paths = Vec::new();
    for spec in &shot_specs {
        let shot_trace =
            export_project_shot_trace(db.connection(), "project_p1_dry_run", &spec.shot_id)?;
        let shot_trace_path = out_dir.join(format!("trace_{}.json", spec.shot_id));
        std::fs::write(&shot_trace_path, serde_json::to_string_pretty(&shot_trace)?).map_err(
            |err| {
                VideoAgentError::Ffmpeg(format!(
                    "write shot trace {} failed: {err}",
                    shot_trace_path.display()
                ))
            },
        )?;
        shot_trace_paths.push(shot_trace_path);
    }

    Ok(LocalDryRunOutput {
        db_path,
        rough_cut_path,
        failed_shots_path: rough.failed_shots_path,
        assembly_manifest_path: rough.assembly_manifest_path,
        trace_path,
        shot_trace_paths,
    })
}

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().min(i64::MAX as u128) as i64
}

fn seed_project(db: &Database) -> Result<()> {
    db.create_project("project_p1_dry_run", 1.0)?;
    db.create_chapter("chapter_01", "project_p1_dry_run", "{\"duration_s\":0.8}")?;
    db.create_scene("scene_01", "chapter_01", "{\"duration_s\":0.8}")?;
    Ok(())
}

fn seed_agent_artifacts(db: &Database, shot_specs: &[ShotTextSpec]) -> Result<()> {
    let brief_packet = ContextPacket::new(
        "brief",
        vec![],
        json!({
            "project_id": "project_p1_dry_run",
            "duration_s": 0.8,
            "language": "zh",
        }),
    )?;
    let brief = json!({
        "brief": "local P1 dry-run video",
        "duration_s": 0.8,
        "language": "zh",
    });
    insert_project_artifact(
        db.connection(),
        "artifact_agent_brief",
        "project_p1_dry_run",
        AgentArtifactKind::Brief,
        &brief,
    )?;
    write_structured_agent_pass(
        db,
        "artifact_agent_brief",
        P1AgentNode::Requirements,
        &brief_packet,
        &validate_brief_artifact(&brief),
    )?;

    let chapters_packet = ContextPacket::new(
        "chapters",
        vec!["artifact_agent_brief".to_string()],
        json!({
            "project_id": "project_p1_dry_run",
            "target_duration_s": 0.8,
        }),
    )?;
    let chapters = json!({
        "chapters": [{
            "chapter_id": "chapter_01",
            "title": "Local dry-run",
            "duration_s": 0.8,
        }]
    });
    insert_project_artifact(
        db.connection(),
        "artifact_agent_chapters",
        "project_p1_dry_run",
        AgentArtifactKind::Chapters,
        &chapters,
    )?;
    write_structured_agent_pass(
        db,
        "artifact_agent_chapters",
        P1AgentNode::ScriptChapters,
        &chapters_packet,
        &validate_chapters_artifact(&chapters, 0.8),
    )?;

    let shots_packet = ContextPacket::new(
        "shots",
        vec!["artifact_agent_chapters".to_string()],
        json!({
            "project_id": "project_p1_dry_run",
            "chapter_ids": ["chapter_01"],
            "target_duration_s": 0.8,
        }),
    )?;
    let shots = json!({
        "shots": shot_specs.iter().enumerate().map(|(idx, spec)| {
            let continuity_in = if idx == 0 {
                "start".to_string()
            } else {
                shot_specs[idx - 1].shot_id.clone()
            };
            let continuity_out = if idx + 1 == shot_specs.len() {
                "end".to_string()
            } else {
                shot_specs[idx + 1].shot_id.clone()
            };
            json!({
                "shot_id": spec.shot_id.clone(),
                "chapter_id": "chapter_01",
                "duration_s": spec.duration_s,
                "continuity_in": continuity_in,
                "continuity_out": continuity_out,
                "is_hero": idx == 0,
                "tier": if idx == 0 { "hero" } else { "standard" },
                "requires_chinese": !spec.overlays.is_empty() || !spec.dialogue.is_empty(),
            })
        }).collect::<Vec<_>>()
    });
    insert_project_artifact(
        db.connection(),
        "artifact_agent_shots",
        "project_p1_dry_run",
        AgentArtifactKind::Shots,
        &shots,
    )?;
    let chapter_budgets = chapter_budgets_from_artifact(&chapters);
    write_structured_agent_pass(
        db,
        "artifact_agent_shots",
        P1AgentNode::Storyboard,
        &shots_packet,
        &validate_shots_artifact(&shots, &chapter_budgets),
    )?;

    let assets_packet = ContextPacket::new(
        "assets",
        vec!["artifact_agent_shots".to_string()],
        json!({
            "project_id": "project_p1_dry_run",
            "shot_ids": shot_specs.iter().map(|spec| spec.shot_id.clone()).collect::<Vec<_>>(),
        }),
    )?;
    let assets = json!({
        "assets": [{
            "asset_id": "asset_studio",
            "type": "environment",
            "shot_ids": shot_specs.iter().map(|spec| spec.shot_id.clone()).collect::<Vec<_>>(),
            "description": "local deterministic dry-run studio plate"
        }]
    });
    insert_project_artifact(
        db.connection(),
        "artifact_agent_assets",
        "project_p1_dry_run",
        AgentArtifactKind::Assets,
        &assets,
    )?;
    write_structured_agent_pass(
        db,
        "artifact_agent_assets",
        P1AgentNode::VisualAssets,
        &assets_packet,
        &validate_assets_artifact(&assets, &shot_ids_from_artifact(&shots)),
    )?;
    Ok(())
}

fn write_structured_agent_pass(
    db: &Database,
    artifact_id: &str,
    node: P1AgentNode,
    packet: &ContextPacket,
    report: &StructuredValidationReport,
) -> Result<()> {
    let spec = p1_agent_node_spec(node);
    let wrote = record_structured_agent_validation_if_pass(
        db.connection(),
        artifact_id,
        "p1-dry-run",
        report,
        &spec,
        packet,
    )?;
    if wrote {
        Ok(())
    } else {
        Err(VideoAgentError::NodeContract(format!(
            "dry-run structured artifact {artifact_id} failed validation: {}",
            report.reasons.join("; ")
        )))
    }
}

fn remove_previous_sqlite(db_path: &Path) -> Result<()> {
    for path in [
        db_path.to_path_buf(),
        db_path.with_extension("sqlite-wal"),
        db_path.with_extension("sqlite-shm"),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(VideoAgentError::Ffmpeg(format!(
                    "remove previous dry-run sqlite file {} failed: {err}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn make_color_clip(path: &Path, color: &str, duration_s: f64) -> Result<()> {
    let input = format!("color=c={color}:s=160x90:d={duration_s}:r=10");
    let output = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", &input, "-pix_fmt", "yuv420p", "-an",
        ])
        .arg(path)
        .output()
        .map_err(|err| VideoAgentError::Ffmpeg(format!("failed to launch ffmpeg: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(VideoAgentError::Ffmpeg(format!(
            "dry-run clip generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn make_local_tts_placeholder_audio(path: &Path, duration_s: f64) -> Result<()> {
    let input = format!(
        "sine=frequency=440:duration={}:sample_rate=48000",
        duration_s.max(0.1)
    );
    let output = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i", &input])
        .args(["-ac", "2", "-ar", "48000"])
        .arg(path)
        .output()
        .map_err(|err| VideoAgentError::Ffmpeg(format!("failed to launch ffmpeg: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(VideoAgentError::Ffmpeg(format!(
            "local TTS placeholder generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn local_file_hash_marker(path: &Path) -> String {
    let len = path.metadata().map(|meta| meta.len()).unwrap_or(0);
    format!("local-size-{len}")
}

fn sha256_file_hash_marker(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|err| {
        VideoAgentError::Ffmpeg(format!(
            "read file for hash {} failed: {err}",
            path.display()
        ))
    })?;
    let digest = Sha256::digest(&bytes);
    Ok(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use serde_json::Value;

    use super::*;
    use crate::test_support::temp_db_path;

    #[test]
    fn local_p1_dry_run_produces_rough_cut_and_trace() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg not available; skipping local dry-run smoke");
            return;
        }

        let out_dir = temp_db_path("p1-dry-run");
        let output = run_local_p1_dry_run(&out_dir).unwrap();
        assert!(output.db_path.is_file());
        assert!(output.rough_cut_path.is_file());
        assert!(output.rough_cut_path.metadata().unwrap().len() > 0);
        assert!(output.failed_shots_path.is_file());
        assert!(output.assembly_manifest_path.is_file());
        assert!(output.trace_path.is_file());
        assert_eq!(output.shot_trace_paths.len(), 2);
        assert!(output.shot_trace_paths.iter().all(|path| path.is_file()));

        let trace: Value =
            serde_json::from_str(&std::fs::read_to_string(&output.trace_path).unwrap()).unwrap();
        assert_eq!(trace["project_id"], "project_p1_dry_run");
        assert_eq!(trace["shots"].as_array().unwrap().len(), 2);
        assert_eq!(trace["shots"][0]["jobs"][0]["provider"], "dry-run");
        assert_eq!(trace["shots"][0]["validations"][0]["verdict"], "pass");
        assert_eq!(
            trace["shots"][0]["validations"][0]["layers"]["media_l0"]["passed"],
            true
        );
        let rough_cut_artifact = trace["project_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["kind"] == "rough_cut")
            .expect("rough_cut project artifact");
        assert_eq!(rough_cut_artifact["id"], "artifact_rough_cut");
        assert_eq!(rough_cut_artifact["validations"][0]["verdict"], "pass");
        assert_eq!(
            rough_cut_artifact["validations"][0]["layers"]["media_l0"]["passed"],
            true
        );
        let shot_trace: Value =
            serde_json::from_str(&std::fs::read_to_string(&output.shot_trace_paths[0]).unwrap())
                .unwrap();
        assert_eq!(shot_trace["project_id"], "project_p1_dry_run");
        assert_eq!(shot_trace["shot_id"], "shot_01");
        assert_eq!(shot_trace["jobs"][0]["attempt"], 0);
        assert_eq!(shot_trace["jobs"][0]["provider"], "dry-run");
        assert!(shot_trace["jobs"][0].get("cost").is_some());
        assert!(shot_trace["jobs"][0].get("latency_ms").is_some());
        assert!(shot_trace["jobs"][0].get("failure_reason").is_some());

        let manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&output.assembly_manifest_path).unwrap())
                .unwrap();
        assert!(manifest["shots"][0]["subtitle_path"].as_str().is_some());
        assert_eq!(manifest["shots"][0]["subtitle_burned_in"], true);
        assert_eq!(manifest["shots"][0]["audio_muxed"], true);
        assert!(manifest["shots"][0]["audio_path"].as_str().is_some());
        assert_eq!(
            manifest["shots"][0]["rerun_context"]["tts_placeholder"],
            true
        );
        assert_eq!(manifest["shots"][1]["silent_audio_inserted"], true);

        let _ = std::fs::remove_dir_all(out_dir);
    }

    #[test]
    fn local_p1_dry_run_can_be_repeated_in_same_output_dir() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("ffmpeg not available; skipping repeated dry-run smoke");
            return;
        }

        let out_dir = temp_db_path("p1-dry-run-repeat");
        let first = run_local_p1_dry_run(&out_dir).unwrap();
        let second = run_local_p1_dry_run(&out_dir).unwrap();

        assert!(first.rough_cut_path.is_file());
        assert!(second.rough_cut_path.is_file());
        assert!(second.trace_path.is_file());

        let _ = std::fs::remove_dir_all(out_dir);
    }
}
