//! Same original SQLite and exclusive reservation lock; no new fee store.
use super::{new_file, sha, text};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Ledger {
    connection: Connection,
    directory: PathBuf,
    origin: String,
    authorization: String,
    cohort: Option<Value>,
    cap: u64,
}

impl Ledger {
    pub fn open(directory: &Path, origin: &str, cap: u64, cohort: &Value) -> Result<Self, String> {
        if cap != 1642
            || cohort["ledger_snapshot"]["authorization_chain"]["authorization"]["main_model"]
                ["max_attempts"]
                != 1642
            || cohort["ledger_snapshot"]["authorization_chain"]["bindings"]
                .as_array()
                .is_none_or(|v| v.len() != 5)
            || cohort["collection_authorized_split"] != "calibration"
        {
            return Err("actual_approved_five_chain_required".into());
        }
        let directory = directory
            .canonicalize()
            .map_err(|_| "original_root_missing")?;
        let path = directory.join("main-attempts.sqlite3");
        if !path.is_file() {
            return Err("original_ledger_missing".into());
        }
        let connection = Connection::open(path).map_err(|_| "ledger_open")?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(|_| "ledger_timeout")?;
        connection
            .execute_batch("PRAGMA synchronous=FULL")
            .map_err(|_| "durable_ledger")?;
        let actual: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='authorization_sha256'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "original_metadata")?;
        if actual != origin {
            return Err("original_metadata_changed".into());
        }
        Ok(Self {
            connection,
            directory,
            origin: origin.into(),
            authorization: text(cohort, "authorization_sha256")?.into(),
            cohort: Some(cohort.clone()),
            cap,
        })
    }

    pub fn fixture(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir(directory).map_err(|_| "fixture_root_exists")?;
        let connection = Connection::open(directory.join("main-attempts.sqlite3"))
            .map_err(|_| "fixture_open")?;
        connection.execute_batch("PRAGMA synchronous=FULL;CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT NOT NULL UNIQUE,split TEXT NOT NULL,started_json TEXT NOT NULL,finished_json TEXT);INSERT INTO metadata VALUES('authorization_sha256','offline');").map_err(|_| "fixture_schema")?;
        Ok(Self {
            connection,
            directory: directory.into(),
            origin: "offline".into(),
            authorization: "offline".into(),
            cohort: None,
            cap: 1642,
        })
    }
    pub fn count(&self) -> Result<u64, String> {
        self.connection
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .map(|count| count as u64)
            .map_err(|_| "ledger_count".into())
    }
    pub fn reject_existing(&self, groups: &[&str]) -> Result<(), String> {
        for group in groups {
            if self
                .connection
                .query_row(
                    "SELECT ordinal FROM attempts WHERE group_id=?1",
                    [group],
                    |r| r.get::<_, i64>(0),
                )
                .optional()
                .map_err(|_| "ledger_query")?
                .is_some()
            {
                return Err("group_already_attempted".into());
            }
        }
        Ok(())
    }
    pub fn reserve(&mut self, group: &str, record: &Value) -> Result<u64, String> {
        let path = self.directory.join("reservation.lock");
        let file = new_file(&path).map_err(|_| "reservation_in_progress")?;
        file.sync_all().map_err(|_| "reservation_lock_sync")?;
        let result = self.reserve_locked(group, record);
        drop(file);
        std::fs::remove_file(path).map_err(|_| "reservation_lock_release")?;
        result
    }
    fn reserve_locked(&mut self, group: &str, record: &Value) -> Result<u64, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "ledger_lock")?;
        let actual: String = tx
            .query_row(
                "SELECT value FROM metadata WHERE key='authorization_sha256'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "metadata_missing")?;
        if actual != self.origin {
            return Err("original_metadata_changed".into());
        }
        let rows: Vec<(i64, String, String, Option<String>)> = tx
            .prepare(
                "SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal",
            )
            .map_err(|_| "ledger_query")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(|_| "ledger_rows")?
            .collect::<Result<_, _>>()
            .map_err(|_| "ledger_rows")?;
        let sequence: Option<i64> = tx
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name='attempts'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| "ledger_sequence")?;
        if rows
            .iter()
            .enumerate()
            .any(|(index, row)| row.0 != index as i64 + 1)
            || sequence.unwrap_or(0) != rows.len() as i64
        {
            return Err("original_ordinal_deleted_or_reset".into());
        }
        let mut groups: HashSet<String> = rows.iter().map(|row| row.1.clone()).collect();
        if let Some(cohort) = &self.cohort {
            let metadata: Vec<(String, String)> = tx
                .prepare("SELECT key,value FROM metadata ORDER BY key")
                .map_err(|_| "metadata_read")?
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|_| "metadata_read")?
                .collect::<Result<_, _>>()
                .map_err(|_| "metadata_read")?;
            if serde_json::to_value(metadata).map_err(|_| "metadata_json")?
                != cohort["ledger_snapshot"]["main_metadata"]
            {
                return Err("complete_original_metadata_changed".into());
            }
            for binding in cohort["bindings"].as_array().ok_or("bindings")? {
                if sha(&std::fs::read(text(binding, "file")?).map_err(|_| "bound_read")?)
                    != text(binding, "sha256")?
                {
                    return Err("original_source_changed_under_lock".into());
                }
            }
            let original = cohort["ledger_snapshot"]["main_started"]
                .as_array()
                .ok_or("original_prefix")?;
            if rows.len() < original.len() {
                return Err("original_prefix_deleted".into());
            }
            for (index, before) in original.iter().enumerate() {
                let raw = &cohort["calibration_original_sql_image"][1][index];
                if raw[0] != rows[index].0
                    || raw[1] != rows[index].1
                    || raw[3] != rows[index].2
                    || raw[4]
                        != serde_json::to_value(&rows[index].3).map_err(|_| "raw_history_json")?
                {
                    return Err("original_sql_bytes_changed".into());
                }
                let start: Value =
                    serde_json::from_str(&rows[index].2).map_err(|_| "original_start_json")?;
                let finish: Value = rows[index]
                    .3
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| "original_finish_json")?
                    .unwrap_or(Value::Null);
                if rows[index].1 != text(before, "group_id")?
                    || start != before["started"]
                    || finish != cohort["original_finished"][&rows[index].1]
                {
                    return Err("original_start_or_finish_changed".into());
                }
            }
            let fixed: Vec<&Value> = cohort["cases"]
                .as_array()
                .ok_or("cases")?
                .iter()
                .filter(|case| case["split"] == "calibration")
                .collect();
            let own = &rows[original.len()..];
            if own.len() >= fixed.len() {
                return Err("development_exhausted".into());
            }
            for (index, row) in own.iter().enumerate() {
                let start: Value = serde_json::from_str(&row.2).map_err(|_| "new_start_json")?;
                if row.1 != text(fixed[index], "group_id")?
                    || start["artifact_cohort_hash"] != cohort["artifact_hash"]
                    || row.3.is_none()
                    || start["authorization_sha256"] != self.authorization
                    || [
                        "id",
                        "group_id",
                        "task_id",
                        "task_sha256",
                        "source_kind",
                        "split",
                    ]
                    .iter()
                    .any(|key| start[*key] != fixed[index][*key])
                    || [
                        "calibration_opening_file",
                        "calibration_opening_sha256",
                        "calibration_opening_hash",
                    ]
                    .iter()
                    .any(|key| start[*key] != cohort[*key])
                {
                    return Err("new_pending_or_foreign_scope".into());
                }
            }
            if group != text(fixed[own.len()], "group_id")?
                || record["authorization_sha256"] != self.authorization
                || record["artifact_cohort_hash"] != cohort["artifact_hash"]
                || record["split"] != "calibration"
                || [
                    "calibration_opening_file",
                    "calibration_opening_sha256",
                    "calibration_opening_hash",
                ]
                .iter()
                .any(|key| record[*key] != cohort[*key])
                || [
                    "id",
                    "group_id",
                    "task_id",
                    "task_sha256",
                    "source_kind",
                    "split",
                ]
                .iter()
                .any(|key| record[*key] != fixed[own.len()][*key])
            {
                return Err("genuine_next_funded_group_required".into());
            }
        }
        let reservations = self.directory.join("reservations");
        if reservations.exists() {
            for entry in std::fs::read_dir(reservations).map_err(|_| "reservations_read")? {
                let path = entry.map_err(|_| "reservation_path")?.path();
                if path.extension().is_some_and(|value| value == "json") {
                    let value: Value = serde_json::from_slice(
                        &std::fs::read(path).map_err(|_| "reservation_read")?,
                    )
                    .map_err(|_| "reservation_json")?;
                    groups.insert(text(&value, "group_id")?.into());
                }
            }
        }
        if groups.contains(group) {
            return Err("group_already_attempted_or_reserved".into());
        }
        if groups.len() as u64 >= self.cap {
            return Err("main_budget_exhausted".into());
        }
        tx.execute(
            "INSERT INTO attempts(group_id,split,started_json) VALUES(?1,?2,?3)",
            params![group, text(record, "split")?, record.to_string()],
        )
        .map_err(|_| "group_already_attempted")?;
        let ordinal = tx.last_insert_rowid() as u64;
        tx.commit().map_err(|_| "ledger_commit")?;
        Ok(ordinal)
    }
    pub fn finish(&self, ordinal: u64, record: &Value) -> Result<(), String> {
        let count = self
            .connection
            .execute(
                "UPDATE attempts SET finished_json=?1 WHERE ordinal=?2 AND finished_json IS NULL",
                params![record.to_string(), ordinal as i64],
            )
            .map_err(|_| "ledger_finish")?;
        if count != 1 {
            return Err("duplicate_finish".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "stdin-ledger-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn separate_instances_preserve_failure_and_enforce_spent_cap() {
        let path = fixture_root("two-instances");
        let mut first = Ledger::fixture(&path).unwrap();
        first.cap = 2;
        let ordinal = first
            .reserve("failed", &json!({"split":"calibration"}))
            .unwrap();
        first
            .finish(ordinal, &json!({"call_status":"transport_error"}))
            .unwrap();
        let mut second = Ledger {
            connection: Connection::open(path.join("main-attempts.sqlite3")).unwrap(),
            directory: path.clone(),
            origin: "offline".into(),
            authorization: "offline".into(),
            cohort: None,
            cap: 2,
        };
        assert!(second
            .reserve("failed", &json!({"split":"calibration"}))
            .is_err());
        second
            .reserve("pending", &json!({"split":"calibration"}))
            .unwrap();
        assert!(first
            .reserve("third", &json!({"split":"calibration"}))
            .is_err());
        assert_eq!(first.count().unwrap(), 2);
        let failure: String = first
            .connection
            .query_row(
                "SELECT finished_json FROM attempts WHERE group_id='failed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&failure).unwrap(),
            json!({"call_status":"transport_error"})
        );
        drop(first);
        drop(second);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn deleted_ordinal_and_changed_origin_do_not_reset_budget() {
        let path = fixture_root("no-reset");
        let mut ledger = Ledger::fixture(&path).unwrap();
        ledger
            .reserve("pending", &json!({"split":"calibration"}))
            .unwrap();
        ledger
            .connection
            .execute("DELETE FROM attempts WHERE ordinal=1", [])
            .unwrap();
        assert_eq!(
            ledger
                .reserve("new", &json!({"split":"calibration"}))
                .unwrap_err(),
            "original_ordinal_deleted_or_reset"
        );
        ledger
            .connection
            .execute("UPDATE metadata SET value='forged'", [])
            .unwrap();
        assert_eq!(
            ledger
                .reserve("new", &json!({"split":"calibration"}))
                .unwrap_err(),
            "original_metadata_changed"
        );
        drop(ledger);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn pending_and_failed_survive_duplicate_and_stale_lock() {
        let path = std::env::temp_dir().join(format!(
            "stdin-ledger-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut ledger = Ledger::fixture(&path).unwrap();
        assert_eq!(
            ledger
                .reserve("pending", &json!({"split":"calibration"}))
                .unwrap(),
            1
        );
        let ordinal = ledger
            .reserve("failed", &json!({"split":"calibration"}))
            .unwrap();
        ledger
            .finish(ordinal, &json!({"call_status":"transport_error"}))
            .unwrap();
        assert!(ledger
            .reserve("pending", &json!({"split":"calibration"}))
            .is_err());
        std::fs::write(path.join("reservation.lock"), b"original stale marker").unwrap();
        assert!(ledger
            .reserve("new", &json!({"split":"calibration"}))
            .is_err());
        assert_eq!(
            std::fs::read(path.join("reservation.lock")).unwrap(),
            b"original stale marker"
        );
        assert_eq!(ledger.count().unwrap(), 2);
        drop(ledger);
        std::fs::remove_dir_all(path).unwrap(); // Temporary fixture only.
    }
    #[test]
    fn authenticated_phase_prefix_preserves_raw_old_history_and_new_failure() {
        let path = fixture_root("calibration-phase-prefix");
        let mut seed = Ledger::fixture(&path).unwrap();
        seed.reserve("old-pending", &json!({"split":"development"}))
            .unwrap();
        let failed = seed
            .reserve("old-failed", &json!({"split":"development"}))
            .unwrap();
        seed.finish(failed, &json!({"call_status":"transport_error"}))
            .unwrap();
        let raw: Vec<(i64,String,String,String,Option<String>)> = seed.connection.prepare(
            "SELECT ordinal,group_id,split,started_json,finished_json FROM attempts ORDER BY ordinal"
        ).unwrap().query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))
            .unwrap().collect::<Result<_,_>>().unwrap();
        let original: Vec<Value> = raw
            .iter()
            .map(|r| {
                json!({"ordinal":r.0,"group_id":r.1,
            "started":serde_json::from_str::<Value>(&r.3).unwrap()})
            })
            .collect();
        let cases: Vec<Value> = (0..3).map(|i| json!({"id":format!("cal-{i}"),"group_id":format!("cal-group-{i}"),
            "task_id":i,"task_sha256":"fixture-task","source_kind":"apps_stdin_restricted_script","split":"calibration"})).collect();
        let coho = json!({"artifact_hash":"fixture-cohort","authorization_sha256":"fixture-funded",
            "collection_authorized_split":"calibration","bindings":[],"cases":cases,
            "calibration_opening_file":"fixture-marker","calibration_opening_sha256":"fixture-sha","calibration_opening_hash":"fixture-hash",
            "ledger_snapshot":{"main_metadata":[["authorization_sha256","offline"]],"main_started":original,"main_reservations":[],
                "authorization_chain":{"authorization":{"main_model":{"max_attempts":1642}},"bindings":[null,null,null,null,null]}},
            "original_finished":{"old-pending":null,"old-failed":{"call_status":"transport_error"}},
            "calibration_original_sql_image":[[["authorization_sha256","offline"]],raw,[[2]]]});
        drop(seed);
        let mut ledger = Ledger::open(&path, "offline", 1642, &coho).unwrap();
        let record = |i: usize| {
            let mut value = coho["cases"][i].clone();
            for key in [
                "calibration_opening_file",
                "calibration_opening_sha256",
                "calibration_opening_hash",
            ] {
                value[key] = coho[key].clone();
            }
            value["authorization_sha256"] = json!("fixture-funded");
            value["artifact_cohort_hash"] = json!("fixture-cohort");
            value
        };
        let first = ledger.reserve("cal-group-0", &record(0)).unwrap();
        ledger
            .finish(first, &json!({"call_status":"http_400"}))
            .unwrap();
        assert!(ledger.reserve("cal-group-0", &record(0)).is_err());
        ledger.reserve("cal-group-1", &record(1)).unwrap();
        assert_eq!(
            ledger.reserve("cal-group-2", &record(2)).unwrap_err(),
            "new_pending_or_foreign_scope"
        );
        ledger
            .connection
            .execute(
                "UPDATE attempts SET started_json=started_json || ' ' WHERE ordinal=1",
                [],
            )
            .unwrap();
        assert_eq!(
            ledger.reserve("cal-group-2", &record(2)).unwrap_err(),
            "original_sql_bytes_changed"
        );
        assert_eq!(ledger.count().unwrap(), 4);
        drop(ledger);
        std::fs::remove_dir_all(path).unwrap(); // Self-made temporary fixture only.
    }
}
