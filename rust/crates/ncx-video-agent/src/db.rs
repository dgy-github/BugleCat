use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection};
use serde_json::json;

use crate::Result;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = open_db(path)?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn create_project(&self, id: &str, budget_total: f64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects(id, brief_json, status, budget_total)
             VALUES(?1, ?2, 'new', ?3)",
            params![id, json!({}).to_string(), budget_total],
        )?;
        Ok(())
    }

    pub fn create_chapter(&self, id: &str, project_id: &str, plan_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chapters(id, project_id, plan_json, status)
             VALUES(?1, ?2, ?3, 'new')",
            params![id, project_id, plan_json],
        )?;
        Ok(())
    }

    pub fn create_scene(&self, id: &str, chapter_id: &str, plan_json: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO scenes(id, chapter_id, plan_json, status)
             VALUES(?1, ?2, ?3, 'new')",
            params![id, chapter_id, plan_json],
        )?;
        Ok(())
    }

    pub fn create_shot(
        &self,
        id: &str,
        scene_id: &str,
        plan_json: &str,
        continuity_in: Option<&str>,
        continuity_out: Option<&str>,
        is_hero: bool,
        tier: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO shots(
                id, scene_id, plan_json, status, continuity_in, continuity_out,
                risk_level, is_hero, tier
             )
             VALUES(?1, ?2, ?3, 'new', ?4, ?5, 'normal', ?6, ?7)",
            params![
                id,
                scene_id,
                plan_json,
                continuity_in,
                continuity_out,
                i64::from(is_hero),
                tier
            ],
        )?;
        Ok(())
    }

    pub fn create_artifact(
        &self,
        id: &str,
        shot_id: Option<&str>,
        kind: &str,
        tos_key: &str,
        content_hash: &str,
        params_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO artifacts(id, shot_id, kind, tos_key, content_hash, params_json)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, shot_id, kind, tos_key, content_hash, params_json],
        )?;
        Ok(())
    }

    pub fn create_project_artifact(
        &self,
        id: &str,
        project_id: &str,
        kind: &str,
        tos_key: &str,
        content_hash: &str,
        params_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO artifacts(id, project_id, shot_id, kind, tos_key, content_hash, params_json)
             VALUES(?1, ?2, NULL, ?3, ?4, ?5, ?6)",
            params![id, project_id, kind, tos_key, content_hash, params_json],
        )?;
        Ok(())
    }
}

pub fn open_db(path: impl AsRef<Path>) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    init_schema(&conn)?;
    require_json1(&conn)?;
    Ok(conn)
}

pub fn require_json1(conn: &Connection) -> Result<()> {
    let value: i64 = conn.query_row("SELECT json_extract('{\"x\":7}', '$.x')", [], |row| {
        row.get(0)
    })?;
    debug_assert_eq!(value, 7);
    Ok(())
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS projects(
            id TEXT PRIMARY KEY,
            brief_json TEXT NOT NULL CHECK(json_valid(brief_json)),
            status TEXT NOT NULL,
            budget_total REAL NOT NULL CHECK(budget_total >= 0),
            budget_reserved REAL NOT NULL DEFAULT 0 CHECK(budget_reserved >= 0),
            budget_spent REAL NOT NULL DEFAULT 0 CHECK(budget_spent >= 0),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS chapters(
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            plan_json TEXT NOT NULL CHECK(json_valid(plan_json)),
            status TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS scenes(
            id TEXT PRIMARY KEY,
            chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
            plan_json TEXT NOT NULL CHECK(json_valid(plan_json)),
            status TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS shots(
            id TEXT PRIMARY KEY,
            scene_id TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
            plan_json TEXT NOT NULL CHECK(json_valid(plan_json)),
            status TEXT NOT NULL,
            continuity_in TEXT,
            continuity_out TEXT,
            risk_level TEXT NOT NULL DEFAULT 'normal',
            is_hero INTEGER NOT NULL DEFAULT 0 CHECK(is_hero IN (0, 1)),
            tier TEXT NOT NULL DEFAULT 'standard'
        );

        CREATE TABLE IF NOT EXISTS artifacts(
            id TEXT PRIMARY KEY,
            project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
            shot_id TEXT REFERENCES shots(id) ON DELETE SET NULL,
            kind TEXT NOT NULL,
            tos_key TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            params_json TEXT NOT NULL CHECK(json_valid(params_json)),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS jobs(
            id TEXT PRIMARY KEY,
            shot_id TEXT NOT NULL REFERENCES shots(id) ON DELETE CASCADE,
            attempt INTEGER NOT NULL CHECK(attempt >= 0),
            idempotency_key TEXT NOT NULL UNIQUE,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            status TEXT NOT NULL,
            provider_job_id TEXT,
            params_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(params_json)),
            token_used INTEGER NOT NULL DEFAULT 0 CHECK(token_used >= 0),
            cost REAL NOT NULL DEFAULT 0 CHECK(cost >= 0),
            latency_ms INTEGER CHECK(latency_ms IS NULL OR latency_ms >= 0),
            failure_reason TEXT,
            budget_reserved REAL NOT NULL DEFAULT 0 CHECK(budget_reserved >= 0),
            budget_settled INTEGER NOT NULL DEFAULT 0 CHECK(budget_settled IN (0, 1)),
            candidate_set TEXT,
            is_chosen INTEGER NOT NULL DEFAULT 0 CHECK(is_chosen IN (0, 1)),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS validation_records(
            id TEXT PRIMARY KEY,
            artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
            stage TEXT NOT NULL,
            gate_version TEXT NOT NULL,
            verdict TEXT NOT NULL CHECK(verdict IN ('pass', 'repair', 'escalate')),
            confidence REAL CHECK(confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
            aesthetic_score REAL,
            layers_json TEXT NOT NULL CHECK(json_valid(layers_json)),
            escalate_reason TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS golden_cases(
            id TEXT PRIMARY KEY,
            stage TEXT NOT NULL,
            failure_type TEXT NOT NULL,
            tos_key TEXT NOT NULL,
            human_verdict TEXT NOT NULL,
            human_score REAL,
            is_exemplar INTEGER NOT NULL DEFAULT 0 CHECK(is_exemplar IN (0, 1)),
            source TEXT NOT NULL,
            added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS gate_metrics(
            stage TEXT NOT NULL,
            gate_version TEXT NOT NULL,
            pass_precision REAL,
            escalate_rate REAL,
            judge_cost REAL,
            human_agreement REAL,
            PRIMARY KEY(stage, gate_version)
        );

        CREATE TABLE IF NOT EXISTS model_metrics(
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            task TEXT NOT NULL,
            pass_rate REAL,
            avg_cost REAL,
            avg_latency REAL,
            PRIMARY KEY(provider, model, task)
        );
        "#,
    )?;
    ensure_artifact_project_id(conn)?;
    Ok(())
}

fn ensure_artifact_project_id(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(artifacts)")?;
    let has_project_id = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "project_id");
    if !has_project_id {
        conn.execute(
            "ALTER TABLE artifacts ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE CASCADE",
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temp_db_path;

    #[test]
    fn schema_creates_tables_wal_and_json1() {
        let path = temp_db_path("schema");
        let conn = open_db(&path).expect("open db");

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");
        assert_eq!(mode.to_lowercase(), "wal");

        require_json1(&conn).expect("JSON1 is available");

        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table'
                   AND name IN (
                    'projects','chapters','scenes','shots','artifacts','jobs',
                    'validation_records','golden_cases','gate_metrics','model_metrics'
                   )",
                [],
                |row| row.get(0),
            )
            .expect("count tables");
        assert_eq!(table_count, 10);

        let _: i64 = conn
            .query_row(
                "SELECT json_extract('{\"plan\":{\"seconds\":3}}', '$.plan.seconds')",
                [],
                |row| row.get(0),
            )
            .expect("json_extract works");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn duplicate_idempotency_key_is_rejected() {
        let path = temp_db_path("job-unique");
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

        db.connection()
            .execute(
                "INSERT INTO jobs(id, shot_id, attempt, idempotency_key, provider, model, status)
                 VALUES('j1', 'shot', 0, 'same-key', 'ark', 'seedance', 'submitted')",
                [],
            )
            .unwrap();

        let err = db
            .connection()
            .execute(
                "INSERT INTO jobs(id, shot_id, attempt, idempotency_key, provider, model, status)
                 VALUES('j2', 'shot', 1, 'same-key', 'ark', 'seedance', 'submitted')",
                [],
            )
            .expect_err("UNIQUE should reject duplicate idempotency key");
        assert!(matches!(
            err,
            rusqlite::Error::SqliteFailure(_, Some(_)) | rusqlite::Error::SqliteFailure(_, None)
        ));

        let _ = std::fs::remove_file(path);
    }
}
