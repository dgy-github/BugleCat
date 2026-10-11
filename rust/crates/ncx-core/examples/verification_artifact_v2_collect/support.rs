//! New development source validator; frozen engines remain read-only.
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_profile_collect/support.rs"]
mod profile_support;
pub use profile_support::{
    check_resource_identity, check_seal, durable, new_file, original_auth, read_json,
    reject_reserved, seal, sha, text, user_prompt, value_sha, write_new, Inputs, SYSTEM,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};
const OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_development_cohort.py");
const BUDGET_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/budget_authorization_v2.py");

pub fn profile(value: &Value) -> Result<Value, String> {
    profile_support::profile(value)
}
pub fn profile_path(path: &Path) -> Result<Value, String> {
    profile_support::profile_path(path)
}

fn proof(path: &Path) -> Result<Value, String> {
    let output = std::process::Command::new("python")
        .args([
            "-m",
            "evals.verification.artifact_v2_development_cohort",
            "validate-manifest",
            "--manifest",
        ])
        .arg(path)
        .env("NCX_JEV_API_KEY", "")
        .env("DEEPSEEK_API_KEY", "")
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "development_proof_unavailable")?;
    let value: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "development_proof_rejected")?;
    if !output.status.success()
        || value["status"] != "development_source_verified"
        || value["new_paid_calls"] != 0
        || value["formal_registration"] != false
    {
        return Err("development_proof_rejected".into());
    }
    Ok(value)
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    let cohort = read_json(Path::new(text(manifest, "artifact_cohort_file")?))?;
    if manifest["schema"] != "buglecat.verification.artifact-development-collection.v2"
        || manifest["split"] != "development"
        || manifest["formal_registration"] != false
        || cohort["schema"] != "buglecat.verification.artifact-development-cohort.v2"
        || cohort["formal_registration"] != false
        || cohort["holdout_opened"] != false
        || manifest["cases"]
            .as_array()
            .is_none_or(|v| v.is_empty() || v.len() > 8)
    {
        return Err("development_only_contract".into());
    }
    for (role, compiled) in [
        ("development_cohort_owner", OWNER),
        ("budget_v2_owner", BUDGET_OWNER),
    ] {
        let owner_path = if role == "budget_v2_owner" {
            Path::new(text(
                cohort["bindings"]
                    .as_array()
                    .ok_or("bindings")?
                    .iter()
                    .find(|b| b["role"] == "development_cohort_owner")
                    .ok_or("owner")?,
                "file",
            )?)
            .with_file_name("budget_authorization_v2.py")
        } else {
            std::path::PathBuf::from(text(
                cohort["bindings"]
                    .as_array()
                    .ok_or("bindings")?
                    .iter()
                    .find(|b| b["role"] == role)
                    .ok_or("owner")?,
                "file",
            )?)
        };
        if sha(&std::fs::read(owner_path).map_err(|_| "owner_read")?) != sha(compiled) {
            return Err("compiled_owner_changed".into());
        }
    }
    // No temporary proof file is written: caller supplies the actual manifest
    // path via the source-bound field, injected by prepare/live in this engine.
    let proof_path = Path::new(text(manifest, "_actual_manifest_path")?);
    let mut original = manifest.clone();
    original
        .as_object_mut()
        .unwrap()
        .remove("_actual_manifest_path");
    let checked = proof(proof_path)?;
    if checked["hash"]
        != value_sha(&{
            let mut plain = original.clone();
            plain.as_object_mut().unwrap().remove("artifact_hash");
            plain
        })
    {
        return Err("proof_manifest_hash".into());
    }
    profile(&original)?;
    let source = read_json(Path::new(text(&original, "source_file")?))?;
    let catalog = read_json(Path::new(text(&original, "catalog_file")?))?;
    let authorization = read_json(Path::new(text(&original, "authorization_file")?))?;
    Ok(Inputs {
        source,
        catalog,
        authorization,
        cohort,
        resource: None,
    })
}

pub fn revalidate_row(row: &Value) -> Result<(), String> {
    let path = Path::new(text(row, "manifest_file")?);
    let mut manifest = read_json(path)?;
    manifest["_actual_manifest_path"] = json!(path);
    validate(&manifest)?;
    Ok(())
}

pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    resource: Option<&Value>,
    hash: Option<&str>,
) -> Result<(), String> {
    let _ = (resource, hash);
    let first = rows.first().ok_or("empty_batch")?;
    let key = format!(
        "artifact_v2_development_collection:{}",
        text(first, "artifact_cohort_sha256")?
    );
    ledger.bind(&key, &value_sha(&json!({"settings":first["settings"], "build":first["build_info"], "endpoint":first["endpoint_sha256"],"cohort":first["artifact_cohort_sha256"],"system":sha(SYSTEM.as_bytes())})))
}

// Same original SQLite schema/BEGIN IMMEDIATE, cap extended in this new owner only.
pub struct Ledger {
    connection: Connection,
    cap: u64,
    directory: PathBuf,
}
impl Ledger {
    pub fn open(directory: &Path, authorization_hash: &str, cap: u64) -> Result<Self, String> {
        if cap > 750 || cap == 0 {
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
