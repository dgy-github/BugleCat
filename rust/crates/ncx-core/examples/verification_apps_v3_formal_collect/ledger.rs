//! Approved APPS caps on the original SQLite schema/root/lock; original owner unchanged.
use super::{new_file, read_json, text};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};
pub struct Ledger {
    connection: Connection,
    cap: u64,
    directory: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "apps-formal-ledger-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn same_original_root_pending_failure_and_cross_instance_unique() {
        let root = root("prefix");
        let mut first = Ledger::open(&root, "original", 750).unwrap();
        let pending = first
            .reserve(
                "pending",
                &json!({"group_id":"pending","split":"development"}),
            )
            .unwrap();
        let failed = first
            .reserve(
                "failed",
                &json!({"group_id":"failed","split":"development"}),
            )
            .unwrap();
        first
            .finish(failed, &json!({"call_status":"transport_error"}))
            .unwrap();
        let original: Vec<(i64, String, String, Option<String>)> = first
            .connection
            .prepare(
                "SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut second = Ledger::open(&root, "original", 1022).unwrap();
        assert_eq!(second.count().unwrap(), 2);
        assert_eq!(pending, 1);
        assert!(second
            .reserve("pending", &json!({"split":"calibration"}))
            .is_err());
        assert_eq!(
            second
                .reserve("new", &json!({"group_id":"new","split":"calibration"}))
                .unwrap(),
            3
        );
        let prefix: Vec<(i64,String,String,Option<String>)> = second.connection.prepare(
            "SELECT ordinal,group_id,started_json,finished_json FROM attempts WHERE ordinal<=2 ORDER BY ordinal"
        ).unwrap().query_map([], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap()
            .collect::<Result<_,_>>().unwrap();
        assert_eq!(prefix, original);
        assert!(Ledger::open(&root, "reset-auth", 1022).is_err());
        drop(second);
        drop(first);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn original_lock_not_cleared_and_reserved_union_consumes_cap() {
        let root = root("cap");
        let mut ledger = Ledger::open(&root, "original", 1022).unwrap();
        ledger
            .reserve("already", &json!({"group_id":"already"}))
            .unwrap();
        let lock = root.join("reservation.lock");
        std::fs::write(&lock, "old-interrupted-owner").unwrap();
        assert!(ledger
            .reserve("fresh", &json!({"group_id":"fresh"}))
            .is_err());
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            "old-interrupted-owner"
        );
        std::fs::remove_file(&lock).unwrap(); // Fixture cleanup only, never runtime recovery.
        let slots = root.join("reservations");
        std::fs::create_dir(&slots).unwrap();
        for index in 0..1022 {
            let group = if index == 0 {
                "already".to_string()
            } else {
                format!("reserved-{index}")
            };
            std::fs::write(
                slots.join(format!("{index}.json")),
                json!({"group_id":group}).to_string(),
            )
            .unwrap();
        }
        assert!(ledger
            .reserve("fresh", &json!({"group_id":"fresh"}))
            .is_err());
        assert_eq!(ledger.count().unwrap(), 1);
        drop(ledger);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_supported_caps_and_original_metadata() {
        let root = root("caps");
        for cap in [0, 500, 751, 1023, 1151] {
            assert!(Ledger::open(&root, "original", cap).is_err());
        }
        assert!(!root.join("main-attempts.sqlite3").exists());
        let ledger = Ledger::open(&root, "original", 1150).unwrap();
        let metadata: String = ledger
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key='authorization_sha256'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(metadata, "original");
        drop(ledger);
        std::fs::remove_dir_all(root).unwrap();
    }
}
impl Ledger {
    pub fn open(directory: &Path, authorization_hash: &str, cap: u64) -> Result<Self, String> {
        if ![750, 1022, 1150].contains(&cap) {
            return Err("budget_cap".into());
        }
        std::fs::create_dir_all(directory).map_err(|_| "ledger_directory")?;
        let connection =
            Connection::open(directory.join("main-attempts.sqlite3")).map_err(|_| "ledger_open")?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(|_| "ledger_timeout")?;
        connection.execute_batch("PRAGMA synchronous=FULL;CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);CREATE TABLE IF NOT EXISTS attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT NOT NULL UNIQUE,split TEXT NOT NULL,started_json TEXT NOT NULL,finished_json TEXT);").map_err(|_| "ledger_schema")?;
        // In production this must already be the original bound SQLite ledger.
        connection
            .execute(
                "INSERT OR IGNORE INTO metadata VALUES('authorization_sha256',?1)",
                [authorization_hash],
            )
            .map_err(|_| "ledger_metadata")?;
        let actual: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='authorization_sha256'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "ledger_metadata")?;
        if actual != authorization_hash {
            return Err("ledger_authorization_changed".into());
        }
        Ok(Self {
            connection,
            cap,
            directory: directory.to_path_buf(),
        })
    }
    pub fn count(&self) -> Result<u64, String> {
        self.connection
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, i64>(0))
            .map(|count| count as u64)
            .map_err(|_| "ledger_count".into())
    }
    pub fn bind(&self, key: &str, value: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO metadata VALUES(?1,?2)",
                params![key, value],
            )
            .map_err(|_| "ledger_bind")?;
        let bound: String = self
            .connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .map_err(|_| "ledger_bind")?;
        if bound != value {
            return Err("cohort_build_settings_changed".into());
        }
        Ok(())
    }
    pub fn reserve(&mut self, group: &str, record: &Value) -> Result<u64, String> {
        let lock_path = self.directory.join("reservation.lock");
        let lock = new_file(&lock_path).map_err(|_| "reservation_in_progress")?;
        let result = self.reserve_locked(group, record);
        drop(lock);
        std::fs::remove_file(lock_path).map_err(|_| "reservation_lock_release")?;
        result
    }
    fn reserve_locked(&mut self, group: &str, record: &Value) -> Result<u64, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "ledger_lock")?;
        let mut groups: HashSet<String> = tx
            .prepare("SELECT group_id FROM attempts")
            .map_err(|_| "ledger_query")?
            .query_map([], |r| r.get(0))
            .map_err(|_| "ledger_query")?
            .collect::<Result<_, _>>()
            .map_err(|_| "ledger_rows")?;
        let slots = self.directory.join("reservations");
        if slots.exists() {
            for entry in std::fs::read_dir(slots).map_err(|_| "reservations_read")? {
                let path = entry.map_err(|_| "reservation_path")?.path();
                if path.extension().is_some_and(|e| e == "json") {
                    groups.insert(text(&read_json(&path)?, "group_id")?.into());
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
            params![
                group,
                record["split"].as_str().unwrap_or("development"),
                record.to_string()
            ],
        )
        .map_err(|_| "group_already_attempted")?;
        let ordinal = tx.last_insert_rowid() as u64;
        tx.commit().map_err(|_| "ledger_commit")?;
        Ok(ordinal)
    }
    pub fn finish(&self, ordinal: u64, record: &Value) -> Result<(), String> {
        let n = self
            .connection
            .execute(
                "UPDATE attempts SET finished_json=?1 WHERE ordinal=?2 AND finished_json IS NULL",
                params![record.to_string(), ordinal as i64],
            )
            .map_err(|_| "ledger_finish")?;
        if n != 1 {
            return Err("duplicate_finish".into());
        }
        Ok(())
    }
    pub fn reject_existing(&self, groups: &[&str]) -> Result<(), String> {
        for group in groups {
            let existing: Option<i64> = self
                .connection
                .query_row(
                    "SELECT ordinal FROM attempts WHERE group_id=?1",
                    [group],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| "ledger_query")?;
            if existing.is_some() {
                return Err("group_already_attempted".into());
            }
        }
        Ok(())
    }
    pub fn validate_baseline(&self, cohort: &Value) -> Result<(), String> {
        for row in cohort["ledger_snapshot"]["main_started"]
            .as_array()
            .ok_or("cohort_ledger_snapshot")?
        {
            let raw: String = self
                .connection
                .query_row(
                    "SELECT started_json FROM attempts WHERE ordinal=?1 AND group_id=?2",
                    params![
                        row["ordinal"].as_u64().ok_or("ordinal")? as i64,
                        text(row, "group_id")?
                    ],
                    |r| r.get(0),
                )
                .map_err(|_| "original_start_removed")?;
            let started: Value = serde_json::from_str(&raw).map_err(|_| "started_json")?;
            if started != row["started"] {
                return Err("original_start_changed".into());
            }
        }
        if self.count()?
            < cohort["ledger_snapshot"]["main_started"]
                .as_array()
                .unwrap()
                .len() as u64
        {
            return Err("ledger_reset".into());
        }
        Ok(())
    }
    pub fn require_next_cases(&self, cohort: &Value, cases: &[Value]) -> Result<(), String> {
        let mut next = vec![];
        for case in cohort["cases"].as_array().ok_or("cohort_cases")? {
            let attempted: bool = self
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM attempts WHERE group_id=?1)",
                    [text(case, "group_id")?],
                    |r| r.get(0),
                )
                .map_err(|_| "ledger_query")?;
            if !attempted {
                next.push(case.clone());
                if next.len() == cases.len() {
                    break;
                }
            }
        }
        if next != cases {
            return Err("batch_not_next_frozen_cases".into());
        }
        Ok(())
    }
}
