use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{Result, VideoAgentError};

#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub id: String,
    pub shot_id: String,
    pub attempt: i64,
    pub idempotency_key: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub provider_job_id: Option<String>,
    pub params_json: Value,
    pub latency_ms: Option<i64>,
    pub failure_reason: Option<String>,
    pub cost: f64,
    pub budget_reserved: f64,
    pub budget_settled: bool,
    pub candidate_set: Option<String>,
    pub is_chosen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobSubmitOutcome {
    pub record: JobRecord,
    pub submitted_to_provider: bool,
}

pub fn idempotency_key(shot_id: &str, attempt: i64, params: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(shot_id.as_bytes());
    hasher.update([0]);
    hasher.update(attempt.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(canonical_json(params).as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn submit_job_once<F>(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    shot_id: &str,
    attempt: i64,
    params: &Value,
    provider: &str,
    model: &str,
    reserve_cost: f64,
    mut submit_to_provider: F,
) -> Result<JobSubmitOutcome>
where
    F: FnMut() -> std::result::Result<String, String>,
{
    let key = idempotency_key(shot_id, attempt, params);
    if let Some(existing) = load_job_by_key(conn, &key)? {
        if existing.provider_job_id.is_none() {
            return Err(VideoAgentError::JobSubmission(format!(
                "idempotent job {} already exists with status '{}' but no provider_job_id; refusing to resubmit because provider submission state is ambiguous",
                existing.id, existing.status
            )));
        }
        return Ok(JobSubmitOutcome {
            record: existing,
            submitted_to_provider: false,
        });
    }

    let job_id = format!("job_{key}");
    reserve_and_insert_job(
        conn,
        project_id,
        shot_id,
        attempt,
        &key,
        &job_id,
        provider,
        model,
        params,
        reserve_cost,
    )?;

    match submit_to_provider() {
        Ok(provider_job_id) => {
            conn.execute(
                "UPDATE jobs SET status='submitted', provider_job_id=?1 WHERE id=?2",
                params![provider_job_id, job_id],
            )?;
            Ok(JobSubmitOutcome {
                record: load_job(conn, &job_id)?.expect("inserted job can be loaded"),
                submitted_to_provider: true,
            })
        }
        Err(err) => {
            release_failed_reservation(conn, project_id, &job_id)?;
            Err(VideoAgentError::JobSubmission(err))
        }
    }
}

pub fn settle_budget(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    actual_cost: f64,
    token_used: i64,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (reserved, settled): (f64, i64) = tx.query_row(
        "SELECT budget_reserved, budget_settled FROM jobs WHERE id=?1",
        params![job_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if settled == 1 {
        tx.commit()?;
        return Ok(());
    }
    tx.execute(
        "UPDATE projects
         SET budget_reserved = budget_reserved - ?1,
             budget_spent = budget_spent + ?2
         WHERE id=?3",
        params![reserved, actual_cost, project_id],
    )?;
    tx.execute(
        "UPDATE jobs
         SET token_used=?1, cost=?2, budget_settled=1, status='settled'
         WHERE id=?3",
        params![token_used, actual_cost, job_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn mark_job_status(conn: &rusqlite::Connection, job_id: &str, status: &str) -> Result<()> {
    if status.trim().is_empty() {
        return Err(VideoAgentError::JobSubmission(
            "job status must not be empty".to_string(),
        ));
    }
    conn.execute(
        "UPDATE jobs SET status=?1, failure_reason=NULL WHERE id=?2",
        params![status, job_id],
    )?;
    Ok(())
}

pub fn record_job_latency_ms(
    conn: &rusqlite::Connection,
    job_id: &str,
    latency_ms: i64,
) -> Result<()> {
    if latency_ms < 0 {
        return Err(VideoAgentError::JobSubmission(format!(
            "job latency must be non-negative, got {latency_ms}"
        )));
    }
    conn.execute(
        "UPDATE jobs SET latency_ms=?1 WHERE id=?2",
        params![latency_ms, job_id],
    )?;
    Ok(())
}

pub fn fail_job_and_release_budget(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    failure_reason: &str,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (reserved, settled): (f64, i64) = tx.query_row(
        "SELECT budget_reserved, budget_settled FROM jobs WHERE id=?1",
        params![job_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if settled == 1 {
        tx.commit()?;
        return Ok(());
    }
    tx.execute(
        "UPDATE projects
         SET budget_reserved = budget_reserved - ?1
         WHERE id=?2",
        params![reserved, project_id],
    )?;
    tx.execute(
        "UPDATE jobs
         SET status='failed',
             failure_reason=?1,
             budget_settled=1,
             cost=0
         WHERE id=?2",
        params![failure_reason, job_id],
    )?;
    tx.commit()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn reserve_and_insert_job(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    shot_id: &str,
    attempt: i64,
    key: &str,
    job_id: &str,
    provider: &str,
    model: &str,
    params_json: &Value,
    reserve_cost: f64,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (total, reserved, spent): (f64, f64, f64) = tx.query_row(
        "SELECT budget_total, budget_reserved, budget_spent FROM projects WHERE id=?1",
        params![project_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let available = total - reserved - spent;
    if reserve_cost > available + f64::EPSILON {
        return Err(VideoAgentError::BudgetExhausted {
            project_id: project_id.to_string(),
            requested: reserve_cost,
            available,
        });
    }

    tx.execute(
        "UPDATE projects
         SET budget_reserved = budget_reserved + ?1
         WHERE id=?2",
        params![reserve_cost, project_id],
    )?;
    tx.execute(
        "INSERT INTO jobs(
            id, shot_id, attempt, idempotency_key, provider, model, status,
            params_json, budget_reserved
         )
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'reserved', ?7, ?8)",
        params![
            job_id,
            shot_id,
            attempt,
            key,
            provider,
            model,
            canonical_json(params_json),
            reserve_cost
        ],
    )?;
    tx.commit()?;
    Ok(())
}

fn release_failed_reservation(
    conn: &mut rusqlite::Connection,
    project_id: &str,
    job_id: &str,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let reserved: f64 = tx.query_row(
        "SELECT budget_reserved FROM jobs WHERE id=?1",
        params![job_id],
        |row| row.get(0),
    )?;
    tx.execute(
        "UPDATE projects
         SET budget_reserved = budget_reserved - ?1
         WHERE id=?2",
        params![reserved, project_id],
    )?;
    tx.execute(
        "UPDATE jobs
         SET status='submit_failed', budget_settled=1, cost=0
         WHERE id=?1",
        params![job_id],
    )?;
    tx.commit()?;
    Ok(())
}

fn load_job_by_key(conn: &rusqlite::Connection, key: &str) -> Result<Option<JobRecord>> {
    conn.query_row(
        "SELECT id, shot_id, attempt, idempotency_key, provider, model, status,
                provider_job_id, params_json, latency_ms, failure_reason,
                cost, budget_reserved, budget_settled,
                candidate_set, is_chosen
         FROM jobs WHERE idempotency_key=?1",
        params![key],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

fn load_job(conn: &rusqlite::Connection, id: &str) -> Result<Option<JobRecord>> {
    conn.query_row(
        "SELECT id, shot_id, attempt, idempotency_key, provider, model, status,
                provider_job_id, params_json, latency_ms, failure_reason,
                cost, budget_reserved, budget_settled,
                candidate_set, is_chosen
         FROM jobs WHERE id=?1",
        params![id],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
    let params_raw: String = row.get(8)?;
    let params_json = serde_json::from_str(&params_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let budget_settled: i64 = row.get(13)?;
    let is_chosen: i64 = row.get(15)?;
    Ok(JobRecord {
        id: row.get(0)?,
        shot_id: row.get(1)?,
        attempt: row.get(2)?,
        idempotency_key: row.get(3)?,
        provider: row.get(4)?,
        model: row.get(5)?,
        status: row.get(6)?,
        provider_job_id: row.get(7)?,
        params_json,
        latency_ms: row.get(9)?,
        failure_reason: row.get(10)?,
        cost: row.get(11)?,
        budget_reserved: row.get(12)?,
        budget_settled: budget_settled == 1,
        candidate_set: row.get(14)?,
        is_chosen: is_chosen == 1,
    })
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => serde_json::to_string(v).expect("string serialization cannot fail"),
        Value::Array(items) => {
            let body = items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| {
                    let k = serde_json::to_string(key).expect("key serialization cannot fail");
                    let v = canonical_json(&map[key]);
                    format!("{k}:{v}")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use serde_json::json;

    use super::*;
    use crate::db::Database;
    use crate::test_support::temp_db_path;

    fn seeded_db(name: &str, budget: f64) -> (std::path::PathBuf, Database) {
        let path = temp_db_path(name);
        let db = Database::open(&path).expect("open db");
        db.create_project("p", budget).unwrap();
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
        (path, db)
    }

    #[test]
    fn idempotency_key_canonicalizes_json_object_order() {
        let a = json!({"duration": 2, "params": {"b": true, "a": 1}});
        let b = json!({"params": {"a": 1, "b": true}, "duration": 2});
        assert_eq!(
            idempotency_key("shot", 0, &a),
            idempotency_key("shot", 0, &b)
        );
        assert_ne!(
            idempotency_key("shot", 0, &a),
            idempotency_key("shot", 1, &a)
        );
    }

    #[test]
    fn submit_job_is_idempotent_and_reserves_once() {
        let (path, mut db) = seeded_db("submit", 100.0);
        let mut api_calls = 0;
        let params = json!({"prompt": "no text overlays", "duration_s": 1});

        let first = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &params,
            "ark",
            "seedance",
            20.0,
            || {
                api_calls += 1;
                Ok("ark-job-1".to_string())
            },
        )
        .unwrap();
        assert!(first.submitted_to_provider);
        assert_eq!(api_calls, 1);

        let second = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &params,
            "ark",
            "seedance",
            20.0,
            || {
                api_calls += 1;
                Ok("ark-job-duplicate".to_string())
            },
        )
        .unwrap();
        assert!(!second.submitted_to_provider);
        assert_eq!(api_calls, 1, "provider should not be called twice");
        assert_eq!(first.record.id, second.record.id);

        let reserved: f64 = db
            .connection()
            .query_row(
                "SELECT budget_reserved FROM projects WHERE id='p'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reserved, 20.0);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn submit_job_refuses_to_resubmit_ambiguous_existing_job_without_provider_id() {
        let (path, mut db) = seeded_db("ambiguous-submit", 100.0);
        let params = json!({"prompt": "no text overlays", "duration_s": 1});
        let key = idempotency_key("shot", 0, &params);
        db.connection()
            .execute("UPDATE projects SET budget_reserved=20 WHERE id='p'", [])
            .unwrap();
        db.connection()
            .execute(
                "INSERT INTO jobs(
                id, shot_id, attempt, idempotency_key, provider, model, status,
                params_json, budget_reserved
             )
             VALUES(?1, 'shot', 0, ?2, 'ark', 'seedance', 'reserved', ?3, 20)",
                params![format!("job_{key}"), key, canonical_json(&params)],
            )
            .unwrap();

        let mut api_calls = 0;
        let err = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &params,
            "ark",
            "seedance",
            20.0,
            || {
                api_calls += 1;
                Ok("ark-job-duplicate".to_string())
            },
        )
        .expect_err("ambiguous existing submit must require reconciliation");

        assert_eq!(api_calls, 0, "provider must not be called again");
        assert!(err.to_string().contains("no provider_job_id"));
        assert!(err.to_string().contains("refusing to resubmit"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn submit_job_refuses_to_retry_submit_failed_job_without_provider_id() {
        let (path, mut db) = seeded_db("submit-failed-retry", 100.0);
        let params = json!({"prompt": "no text overlays", "duration_s": 1});
        let first = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &params,
            "ark",
            "seedance",
            20.0,
            || Err("transport timed out after request body was sent".to_string()),
        )
        .expect_err("first submit should surface transport error");
        assert!(first.to_string().contains("transport timed out"));

        let mut api_calls = 0;
        let second = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &params,
            "ark",
            "seedance",
            20.0,
            || {
                api_calls += 1;
                Ok("ark-job-duplicate".to_string())
            },
        )
        .expect_err("submit_failed retry must not silently resubmit");

        assert_eq!(api_calls, 0, "provider must not be retried ambiguously");
        assert!(second.to_string().contains("submit_failed"));
        assert!(second.to_string().contains("no provider_job_id"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settle_budget_reconciles_project_and_job_once() {
        let (path, mut db) = seeded_db("settle", 100.0);
        let outcome = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &json!({"duration_s": 1}),
            "ark",
            "seedance",
            25.0,
            || Ok("ark-job-1".to_string()),
        )
        .unwrap();

        settle_budget(db.connection_mut(), "p", &outcome.record.id, 18.5, 0).unwrap();
        settle_budget(db.connection_mut(), "p", &outcome.record.id, 18.5, 0).unwrap();

        let (reserved, spent): (f64, f64) = db
            .connection()
            .query_row(
                "SELECT budget_reserved, budget_spent FROM projects WHERE id='p'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(reserved, 0.0);
        assert_eq!(spent, 18.5);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn concurrent_reservations_never_exceed_project_budget() {
        let (path, db) = seeded_db("concurrent", 100.0);
        drop(db);

        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();
        for idx in 0..8 {
            let path = path.clone();
            let barrier = barrier.clone();
            handles.push(thread::spawn(move || {
                let mut db = Database::open(&path).expect("open db in thread");
                barrier.wait();
                submit_job_once(
                    db.connection_mut(),
                    "p",
                    "shot",
                    idx,
                    &json!({"duration_s": 1, "seed": idx}),
                    "ark",
                    "seedance",
                    15.0,
                    || Ok(format!("ark-job-{idx}")),
                )
            }));
        }

        let mut accepted = 0;
        let mut exhausted = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => accepted += 1,
                Err(VideoAgentError::BudgetExhausted { .. }) => exhausted += 1,
                Err(err) => panic!("unexpected error: {err}"),
            }
        }
        assert_eq!(accepted, 6);
        assert_eq!(exhausted, 2);

        let db = Database::open(&path).unwrap();
        let reserved: f64 = db
            .connection()
            .query_row(
                "SELECT budget_reserved FROM projects WHERE id='p'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reserved, 90.0);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_provider_job_releases_reserved_budget_once() {
        let (path, mut db) = seeded_db("fail-release", 100.0);
        let outcome = submit_job_once(
            db.connection_mut(),
            "p",
            "shot",
            0,
            &json!({"duration_s": 1}),
            "ark",
            "seedance",
            25.0,
            || Ok("ark-job-1".to_string()),
        )
        .unwrap();

        fail_job_and_release_budget(db.connection_mut(), "p", &outcome.record.id, "task failed")
            .unwrap();
        fail_job_and_release_budget(
            db.connection_mut(),
            "p",
            &outcome.record.id,
            "task failed again",
        )
        .unwrap();

        let (reserved, spent): (f64, f64) = db
            .connection()
            .query_row(
                "SELECT budget_reserved, budget_spent FROM projects WHERE id='p'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let (status, reason, settled): (String, Option<String>, i64) = db
            .connection()
            .query_row(
                "SELECT status, failure_reason, budget_settled FROM jobs WHERE id=?1",
                params![outcome.record.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(reserved, 0.0);
        assert_eq!(spent, 0.0);
        assert_eq!(status, "failed");
        assert_eq!(reason.as_deref(), Some("task failed"));
        assert_eq!(settled, 1);

        let _ = std::fs::remove_file(path);
    }
}
