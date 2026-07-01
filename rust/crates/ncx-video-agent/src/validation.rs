use rusqlite::params;
use serde_json::Value;

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone)]
pub struct ValidationInput {
    pub id: String,
    pub artifact_id: String,
    pub stage: String,
    pub gate_version: String,
    pub verdict: String,
    pub confidence: Option<f64>,
    pub aesthetic_score: Option<f64>,
    pub layers_json: Value,
    pub escalate_reason: Option<String>,
}

pub fn record_validation(conn: &rusqlite::Connection, input: &ValidationInput) -> Result<()> {
    conn.execute(
        "INSERT INTO validation_records(
            id, artifact_id, stage, gate_version, verdict, confidence,
            aesthetic_score, layers_json, escalate_reason
         )
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.id,
            input.artifact_id,
            input.stage,
            input.gate_version,
            input.verdict,
            input.confidence,
            input.aesthetic_score,
            input.layers_json.to_string(),
            input.escalate_reason,
        ],
    )?;
    Ok(())
}

pub fn assert_artifacts_passed(conn: &rusqlite::Connection, artifact_ids: &[&str]) -> Result<()> {
    for artifact_id in artifact_ids {
        let pass_count: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM validation_records
             WHERE artifact_id=?1 AND verdict='pass'",
            params![artifact_id],
            |row| row.get(0),
        )?;
        if pass_count == 0 {
            return Err(VideoAgentError::MissingPassingValidation {
                artifact_id: (*artifact_id).to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::Database;
    use crate::test_support::temp_db_path;

    fn db_with_artifact(name: &str) -> (std::path::PathBuf, Database) {
        let path = temp_db_path(name);
        let db = Database::open(&path).expect("open db");
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
        db.create_artifact("a", Some("shot"), "brief", "tos://a", "hash", "{}")
            .unwrap();
        (path, db)
    }

    #[test]
    fn downstream_contract_rejects_missing_and_non_pass_records() {
        let (path, db) = db_with_artifact("contract");

        assert!(matches!(
            assert_artifacts_passed(db.connection(), &["a"]),
            Err(VideoAgentError::MissingPassingValidation { .. })
        ));

        record_validation(
            db.connection(),
            &ValidationInput {
                id: "v1".to_string(),
                artifact_id: "a".to_string(),
                stage: "l0".to_string(),
                gate_version: "v1".to_string(),
                verdict: "repair".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({"reason": "bad_reference"}),
                escalate_reason: None,
            },
        )
        .unwrap();
        assert!(matches!(
            assert_artifacts_passed(db.connection(), &["a"]),
            Err(VideoAgentError::MissingPassingValidation { .. })
        ));

        record_validation(
            db.connection(),
            &ValidationInput {
                id: "v2".to_string(),
                artifact_id: "a".to_string(),
                stage: "l0".to_string(),
                gate_version: "v1".to_string(),
                verdict: "pass".to_string(),
                confidence: Some(1.0),
                aesthetic_score: None,
                layers_json: json!({}),
                escalate_reason: None,
            },
        )
        .unwrap();
        assert_artifacts_passed(db.connection(), &["a"]).unwrap();

        let _ = std::fs::remove_file(path);
    }
}
