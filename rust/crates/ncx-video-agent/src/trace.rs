use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct ShotTrace {
    pub shot_id: String,
    pub jobs: Vec<Value>,
    pub artifacts: Vec<Value>,
    pub validations: Vec<Value>,
}

pub fn export_project_trace(conn: &rusqlite::Connection, project_id: &str) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT sh.id
         FROM shots sh
         JOIN scenes sc ON sc.id = sh.scene_id
         JOIN chapters ch ON ch.id = sc.chapter_id
         WHERE ch.project_id=?1
         ORDER BY sh.id",
    )?;
    let shot_ids = stmt
        .query_map(params![project_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut traces = Vec::new();
    for shot_id in shot_ids {
        let trace = export_shot_trace(conn, &shot_id)?;
        traces.push(json!({
            "shot_id": trace.shot_id,
            "jobs": trace.jobs,
            "artifacts": trace.artifacts,
            "validations": trace.validations,
        }));
    }
    let project_artifacts = export_project_artifacts(conn, project_id)?;
    let budget = export_project_budget(conn, project_id)?;
    Ok(json!({
        "project_id": project_id,
        "budget": budget,
        "project_artifacts": project_artifacts,
        "shots": traces,
    }))
}

fn export_project_budget(conn: &rusqlite::Connection, project_id: &str) -> Result<Value> {
    let (budget_total, budget_reserved, budget_spent): (f64, f64, f64) = conn.query_row(
        "SELECT budget_total, budget_reserved, budget_spent FROM projects WHERE id=?1",
        params![project_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let job_cost_total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(j.cost), 0)
         FROM jobs j
         JOIN shots sh ON sh.id = j.shot_id
         JOIN scenes sc ON sc.id = sh.scene_id
         JOIN chapters ch ON ch.id = sc.chapter_id
         WHERE ch.project_id=?1",
        params![project_id],
        |row| row.get(0),
    )?;
    let job_reserved_total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN j.budget_settled = 0 THEN j.budget_reserved ELSE 0 END), 0)
         FROM jobs j
         JOIN shots sh ON sh.id = j.shot_id
         JOIN scenes sc ON sc.id = sh.scene_id
         JOIN chapters ch ON ch.id = sc.chapter_id
         WHERE ch.project_id=?1",
        params![project_id],
        |row| row.get(0),
    )?;
    Ok(json!({
        "budget_total": budget_total,
        "budget_reserved": budget_reserved,
        "budget_spent": budget_spent,
        "job_cost_total": job_cost_total,
        "job_reserved_total": job_reserved_total,
    }))
}

pub fn export_project_shot_trace(
    conn: &rusqlite::Connection,
    project_id: &str,
    shot_id: &str,
) -> Result<Value> {
    let belongs = conn
        .query_row(
            "SELECT 1
             FROM shots sh
             JOIN scenes sc ON sc.id = sh.scene_id
             JOIN chapters ch ON ch.id = sc.chapter_id
             WHERE ch.project_id=?1 AND sh.id=?2
             LIMIT 1",
            params![project_id, shot_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !belongs {
        return Err(VideoAgentError::NodeContract(format!(
            "shot {shot_id} does not belong to project {project_id}"
        )));
    }

    let trace = export_shot_trace(conn, shot_id)?;
    Ok(json!({
        "project_id": project_id,
        "shot_id": trace.shot_id,
        "jobs": trace.jobs,
        "artifacts": trace.artifacts,
        "validations": trace.validations,
    }))
}

fn export_project_artifacts(conn: &rusqlite::Connection, project_id: &str) -> Result<Vec<Value>> {
    let artifacts = query_json_rows_without_shot(
        conn,
        "SELECT id, kind, tos_key, content_hash, params_json, created_at
         FROM artifacts
         WHERE shot_id IS NULL AND project_id=?1
         ORDER BY created_at, id",
        &[project_id],
        |row| {
            let id: String = row.get(0)?;
            let params_raw: String = row.get(4)?;
            let params_json: Value = serde_json::from_str(&params_raw).unwrap_or(Value::Null);
            Ok(json!({
                "id": id,
                "kind": row.get::<_, String>(1)?,
                "tos_key": row.get::<_, String>(2)?,
                "content_hash": row.get::<_, String>(3)?,
                "params": params_json,
                "created_at": row.get::<_, String>(5)?,
                "validations": export_artifact_validations_for_row(conn, &id)?,
            }))
        },
    )?;
    Ok(artifacts)
}

fn export_shot_trace(conn: &rusqlite::Connection, shot_id: &str) -> Result<ShotTrace> {
    let jobs = query_json_rows(
        conn,
        "SELECT id, attempt, provider, model, status, params_json,
                token_used, cost, latency_ms, failure_reason, candidate_set, is_chosen
         FROM jobs WHERE shot_id=?1 ORDER BY attempt, created_at",
        shot_id,
        |row| {
            let params_raw: String = row.get(5)?;
            let params_json: Value = serde_json::from_str(&params_raw).unwrap_or(Value::Null);
            let is_chosen: i64 = row.get(11)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "attempt": row.get::<_, i64>(1)?,
                "provider": row.get::<_, String>(2)?,
                "model": row.get::<_, String>(3)?,
                "status": row.get::<_, String>(4)?,
                "params": params_json,
                "token_used": row.get::<_, i64>(6)?,
                "cost": row.get::<_, f64>(7)?,
                "latency_ms": row.get::<_, Option<i64>>(8)?,
                "failure_reason": row.get::<_, Option<String>>(9)?,
                "candidate_set": row.get::<_, Option<String>>(10)?,
                "is_chosen": is_chosen == 1,
            }))
        },
    )?;

    let artifacts = query_json_rows(
        conn,
        "SELECT id, kind, tos_key, content_hash, params_json, created_at
         FROM artifacts WHERE shot_id=?1 ORDER BY created_at, id",
        shot_id,
        |row| {
            let params_raw: String = row.get(4)?;
            let params_json: Value = serde_json::from_str(&params_raw).unwrap_or(Value::Null);
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "kind": row.get::<_, String>(1)?,
                "tos_key": row.get::<_, String>(2)?,
                "content_hash": row.get::<_, String>(3)?,
                "params": params_json,
                "created_at": row.get::<_, String>(5)?,
            }))
        },
    )?;

    let validations = query_json_rows(
        conn,
        "SELECT vr.id, vr.artifact_id, vr.stage, vr.gate_version, vr.verdict,
                vr.confidence, vr.aesthetic_score, vr.layers_json,
                vr.escalate_reason, vr.created_at
         FROM validation_records vr
         JOIN artifacts a ON a.id = vr.artifact_id
         WHERE a.shot_id=?1
         ORDER BY vr.created_at, vr.id",
        shot_id,
        |row| {
            let layers_raw: String = row.get(7)?;
            let layers_json: Value = serde_json::from_str(&layers_raw).unwrap_or(Value::Null);
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "artifact_id": row.get::<_, String>(1)?,
                "stage": row.get::<_, String>(2)?,
                "gate_version": row.get::<_, String>(3)?,
                "verdict": row.get::<_, String>(4)?,
                "confidence": row.get::<_, Option<f64>>(5)?,
                "aesthetic_score": row.get::<_, Option<f64>>(6)?,
                "layers": layers_json,
                "escalate_reason": row.get::<_, Option<String>>(8)?,
                "created_at": row.get::<_, String>(9)?,
            }))
        },
    )?;

    Ok(ShotTrace {
        shot_id: shot_id.to_string(),
        jobs,
        artifacts,
        validations,
    })
}

fn export_artifact_validations_for_row(
    conn: &rusqlite::Connection,
    artifact_id: &str,
) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, artifact_id, stage, gate_version, verdict,
                confidence, aesthetic_score, layers_json,
                escalate_reason, created_at
         FROM validation_records
         WHERE artifact_id=?1
         ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![artifact_id], |row| {
        let layers_raw: String = row.get(7)?;
        let layers_json: Value = serde_json::from_str(&layers_raw).unwrap_or(Value::Null);
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "artifact_id": row.get::<_, String>(1)?,
            "stage": row.get::<_, String>(2)?,
            "gate_version": row.get::<_, String>(3)?,
            "verdict": row.get::<_, String>(4)?,
            "confidence": row.get::<_, Option<f64>>(5)?,
            "aesthetic_score": row.get::<_, Option<f64>>(6)?,
            "layers": layers_json,
            "escalate_reason": row.get::<_, Option<String>>(8)?,
            "created_at": row.get::<_, String>(9)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
}

fn query_json_rows<F>(
    conn: &rusqlite::Connection,
    sql: &str,
    shot_id: &str,
    mut mapper: F,
) -> Result<Vec<Value>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![shot_id], |row| mapper(row))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_json_rows_without_shot<F>(
    conn: &rusqlite::Connection,
    sql: &str,
    values: &[&str],
    mut mapper: F,
) -> Result<Vec<Value>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| mapper(row))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::Database;
    use crate::jobs::{settle_budget, submit_job_once};
    use crate::structured::{
        insert_project_artifact, record_structured_validation_if_pass, validate_brief_artifact,
        AgentArtifactKind,
    };
    use crate::test_support::temp_db_path;
    use crate::validation::{record_validation, ValidationInput};

    #[test]
    fn trace_exports_jobs_artifacts_and_validation_by_shot() {
        let path = temp_db_path("trace");
        let mut db = Database::open(&path).expect("open db");
        db.create_project("p", 100.0).unwrap();
        db.create_chapter("c", "p", "{}").unwrap();
        db.create_scene("s", "c", "{}").unwrap();
        db.create_shot(
            "shot",
            "s",
            "{\"duration_s\":1}",
            None,
            None,
            false,
            "standard",
        )
        .unwrap();
        let brief = json!({"brief": "make a short video", "duration_s": 1.0, "language": "zh"});
        insert_project_artifact(
            db.connection(),
            "brief_artifact",
            "p",
            AgentArtifactKind::Brief,
            &brief,
        )
        .unwrap();
        let report = validate_brief_artifact(&brief);
        record_structured_validation_if_pass(db.connection(), "brief_artifact", "p1-test", &report)
            .unwrap();

        let job = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &json!({"duration_s": 1, "prompt": "no text overlays"}),
            "ark",
            "seedance",
            4.0,
            || Ok("ark-job".to_string()),
        )
        .unwrap();
        settle_budget(db.connection_mut(), "p", &job.record.id, 3.5, 123).unwrap();
        db.create_artifact(
            "a",
            Some("shot"),
            "video",
            "tos://clip",
            "hash",
            "{\"codec\":\"h264\"}",
        )
        .unwrap();
        record_validation(
            db.connection(),
            &ValidationInput {
                id: "v".to_string(),
                artifact_id: "a".to_string(),
                stage: "l0".to_string(),
                gate_version: "v1".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({"l0": "ok"}),
                escalate_reason: None,
            },
        )
        .unwrap();

        let trace = export_project_trace(db.connection(), "p").unwrap();
        assert_eq!(trace["project_id"], "p");
        assert_eq!(trace["budget"]["budget_spent"], 3.5);
        assert_eq!(trace["budget"]["job_cost_total"], 3.5);
        assert_eq!(trace["budget"]["job_reserved_total"], 0.0);
        assert_eq!(trace["project_artifacts"][0]["id"], "brief_artifact");
        assert_eq!(
            trace["project_artifacts"][0]["validations"][0]["verdict"],
            "pass"
        );
        assert_eq!(trace["shots"][0]["shot_id"], "shot");
        assert_eq!(trace["shots"][0]["jobs"][0]["model"], "seedance");
        assert_eq!(
            trace["shots"][0]["jobs"][0]["params"]["prompt"],
            "no text overlays"
        );
        assert_eq!(trace["shots"][0]["jobs"][0]["cost"], 3.5);
        assert_eq!(trace["shots"][0]["artifacts"][0]["tos_key"], "tos://clip");
        assert_eq!(trace["shots"][0]["validations"][0]["verdict"], "pass");

        let shot_trace = export_project_shot_trace(db.connection(), "p", "shot").unwrap();
        assert_eq!(shot_trace["project_id"], "p");
        assert_eq!(shot_trace["shot_id"], "shot");
        assert_eq!(shot_trace["jobs"][0]["attempt"], 0);
        assert_eq!(shot_trace["jobs"][0]["cost"], 3.5);
        assert!(shot_trace["jobs"][0].get("latency_ms").is_some());
        assert!(shot_trace["jobs"][0].get("failure_reason").is_some());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn trace_exports_only_project_owned_artifacts() {
        let path = temp_db_path("trace-project-artifact-isolation");
        let db = Database::open(&path).expect("open db");
        db.create_project("p1", 100.0).unwrap();
        db.create_project("p2", 100.0).unwrap();

        db.create_project_artifact(
            "artifact-p1-rough-cut",
            "p1",
            "rough_cut",
            "local://p1/rough_cut.mp4",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            "{}",
        )
        .unwrap();
        db.create_project_artifact(
            "artifact-p2-rough-cut",
            "p2",
            "rough_cut",
            "local://p2/rough_cut.mp4",
            "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            "{}",
        )
        .unwrap();
        db.create_artifact(
            "artifact-unowned-rough-cut",
            None,
            "rough_cut",
            "local://legacy/rough_cut.mp4",
            "sha256:3333333333333333333333333333333333333333333333333333333333333333",
            "{}",
        )
        .unwrap();

        let trace = export_project_trace(db.connection(), "p1").unwrap();
        let artifact_ids = trace["project_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|artifact| artifact["id"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(artifact_ids, vec!["artifact-p1-rough-cut"]);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn project_shot_trace_rejects_cross_project_shot() {
        let path = temp_db_path("trace-shot-membership");
        let db = Database::open(&path).expect("open db");
        db.create_project("p1", 100.0).unwrap();
        db.create_project("p2", 100.0).unwrap();
        db.create_chapter("c1", "p1", "{}").unwrap();
        db.create_chapter("c2", "p2", "{}").unwrap();
        db.create_scene("s1", "c1", "{}").unwrap();
        db.create_scene("s2", "c2", "{}").unwrap();
        db.create_shot("shot-p1", "s1", "{}", None, None, false, "standard")
            .unwrap();
        db.create_shot("shot-p2", "s2", "{}", None, None, false, "standard")
            .unwrap();

        let err = export_project_shot_trace(db.connection(), "p1", "shot-p2").unwrap_err();
        assert!(err
            .to_string()
            .contains("shot shot-p2 does not belong to project p1"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn trace_exports_live_seedance_tos_shape_for_strict_verifier() {
        let path = temp_db_path("trace-live-shape");
        let mut db = Database::open(&path).expect("open db");
        db.create_project("p-live", 100.0).unwrap();
        db.create_chapter("c-live", "p-live", "{\"duration_s\":5}")
            .unwrap();
        db.create_scene("s-live", "c-live", "{\"duration_s\":5}")
            .unwrap();
        db.create_shot(
            "shot-live",
            "s-live",
            "{\"duration_s\":5}",
            Some("start"),
            Some("end"),
            false,
            "standard",
        )
        .unwrap();

        let params = json!({
            "model": "doubao-seedance-2-0-fast-260128",
            "content": [{
                "type": "text",
                "text": "A clean 5-second studio shot, no text overlays"
            }],
            "duration": 5,
            "watermark": false
        });
        let job = submit_job_once(
            db.connection_mut(),
            "p-live",
            "shot-live",
            0,
            &params,
            "ark",
            "doubao-seedance-2-0-fast-260128",
            4.0,
            || Ok("ark-task-live".to_string()),
        )
        .unwrap();
        settle_budget(db.connection_mut(), "p-live", &job.record.id, 3.25, 512).unwrap();

        db.create_artifact(
            "artifact-live-video",
            Some("shot-live"),
            "video",
            "tos://bucket/ncx-video-agent/live/shot.mp4",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            &params.to_string(),
        )
        .unwrap();
        record_validation(
            db.connection(),
            &ValidationInput {
                id: "validation-live-video-l0".to_string(),
                artifact_id: "artifact-live-video".to_string(),
                stage: "seedance_media_l0".to_string(),
                gate_version: "p1-real-smoke".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({"media_l0": {"passed": true}}),
                escalate_reason: None,
            },
        )
        .unwrap();

        db.create_project_artifact(
            "artifact-live-rough-cut",
            "p-live",
            "rough_cut",
            "tos://bucket/ncx-video-agent/live/rough_cut.mp4",
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            &json!({
                "seedance_video_artifact_id": "artifact-live-video",
                "assembly_manifest": "assembly_manifest.json",
                "failed_shots": "failed_shots.json",
                "partial_delivery": true,
                "source_tos_key": "ncx-video-agent/live/shot.mp4"
            })
            .to_string(),
        )
        .unwrap();
        record_validation(
            db.connection(),
            &ValidationInput {
                id: "validation-live-rough-cut-l0".to_string(),
                artifact_id: "artifact-live-rough-cut".to_string(),
                stage: "rough_cut_media_l0".to_string(),
                gate_version: "p1-real-smoke".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({"media_l0": {"passed": true}}),
                escalate_reason: None,
            },
        )
        .unwrap();

        let trace = export_project_trace(db.connection(), "p-live").unwrap();
        let shot = &trace["shots"][0];
        assert_eq!(shot["jobs"][0]["provider"], "ark");
        assert!(shot["jobs"][0]["model"]
            .as_str()
            .unwrap()
            .contains("seedance"));
        assert!(shot["jobs"][0]["params"]
            .to_string()
            .to_ascii_lowercase()
            .contains("no text overlays"));
        assert_eq!(shot["artifacts"][0]["kind"], "video");
        assert!(shot["artifacts"][0]["tos_key"]
            .as_str()
            .unwrap()
            .starts_with("tos://"));
        assert!(shot["artifacts"][0]["content_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert!(shot["validations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["artifact_id"] == "artifact-live-video"
                    && record["stage"] == "seedance_media_l0"
                    && record["verdict"] == "pass"
            }));

        let rough = trace["project_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["kind"] == "rough_cut")
            .expect("rough_cut project artifact");
        assert!(rough["tos_key"].as_str().unwrap().starts_with("tos://"));
        assert!(rough["content_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(rough["params"]["partial_delivery"], true);
        assert!(rough["validations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| {
                record["artifact_id"] == "artifact-live-rough-cut"
                    && record["stage"] == "rough_cut_media_l0"
                    && record["verdict"] == "pass"
            }));

        let _ = std::fs::remove_file(path);
    }
}
