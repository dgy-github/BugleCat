//! Formal artifact-source collection guards. All legacy inputs remain read-only.
#[allow(dead_code)]
#[path = "../verification_collect/support.rs"]
mod legacy;
pub use legacy::{
    check_resource_identity, check_seal, durable, new_file, read_json, seal, sha, text,
    user_prompt, value_sha, write_new, SYSTEM,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

const ORIGINAL_AUTH: &str = "d1c5a4ccf695dbccc1777af5123441bbd19aebe39da2a4d3e848abf30f6af0f2";
const EXTENDED_AUTH: &str = "11749a365bb3943fabccae8bc7eefdcdea685353508ae5427bba4e00ae3200e2";
const EXPANDED_SOURCE: &str = "2875e6f093d50c4f10709666562b8f14018d932f31dc5dbda9ff81c02f4af9ce";

pub struct Inputs {
    pub source: Value,
    pub catalog: Value,
    pub authorization: Value,
    pub cohort: Value,
    pub resource: Option<Value>,
}

fn cohort_seal(value: &Value) -> Result<(), String> {
    let mut plain = value.clone();
    let expected = plain
        .as_object_mut()
        .ok_or("cohort_object")?
        .remove("artifact_hash")
        .ok_or("cohort_hash")?;
    if expected != value_sha(&plain) {
        return Err("cohort_seal_mismatch".into());
    }
    Ok(())
}

pub fn chain(authorization: &Value, current: &str) -> Result<(), String> {
    if current != EXTENDED_AUTH
        || authorization["schema"] != "buglecat.verification.paid-budget-extension.v1"
        || authorization["automatic_retries"] != 0
        || authorization["per_group_max_attempts"] != 1
        || authorization["additional_main_attempt_cap"] != 100
        || authorization["additional_jev_attempt_cap"] != 100
        || authorization["old_attempts_and_failures_count"] != true
        || authorization["main_model"]["max_attempts"] != 500
        || authorization["jev"]["max_attempts"] != 500
        || authorization["previous_authorization_sha256"] != ORIGINAL_AUTH
    {
        return Err("authorization_extension_invalid".into());
    }
    let original = legacy::bound_file(
        authorization,
        "previous_authorization_file",
        "previous_authorization_sha256",
    )?;
    for service in ["main_model", "jev"] {
        if original[service]["max_attempts"] != 400
            || Path::new(text(&original[service], "ledger_dir")?)
                .canonicalize()
                .map_err(|_| "ledger_root")?
                != Path::new(text(&authorization[service], "ledger_dir")?)
                    .canonicalize()
                    .map_err(|_| "ledger_root")?
        {
            return Err("authorization_root_reset".into());
        }
    }
    if original["automatic_retries"] != 0 {
        return Err("original_retry".into());
    }
    Ok(())
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    if manifest["schema"] != legacy::SCHEMA
        || manifest["model"] != "deepseek-flash"
        || manifest["reasoning_effort"] != "ultra"
        || manifest["max_tokens"] != 8192
        || manifest["timeout_s"] != 60
    {
        return Err("formal_manifest_settings".into());
    }
    let source = legacy::bound_file(manifest, "source_file", "source_sha256")?;
    let catalog = legacy::bound_file(manifest, "catalog_file", "catalog_sha256")?;
    let authorization = legacy::bound_file(manifest, "authorization_file", "authorization_sha256")?;
    let protocol = legacy::bound_file(manifest, "protocol_file", "protocol_sha256")?;
    let cohort = legacy::bound_file(manifest, "artifact_cohort_file", "artifact_cohort_sha256")?;
    chain(&authorization, text(manifest, "authorization_sha256")?)?;
    cohort_seal(&cohort)?;
    if cohort["schema"] != "buglecat.verification.artifact-cohort.v1"
        || cohort["artifact_hash"] != manifest["artifact_cohort_hash"]
        || cohort["formal_registration"] != true
        || cohort["fixture_only"] != false
        || cohort["status"] != "registered"
        || cohort["collection_authorized"] != true
        || cohort["full_campaign_source_gate"] != true
        || cohort["before_main_requests"] != true
        || cohort["scores_hidden"] != true
        || cohort["jev_probabilities_read"] != false
        || cohort["author_exposed"] != false
        || cohort["split"] != "calibration"
        || cohort["holdout_opened"] != false
        || cohort["blockers"] != json!([])
        || cohort["cases"]
            .as_array()
            .is_none_or(|c| c.len() < 200 || c.len() > 300)
    {
        return Err("formal_cohort_required".into());
    }
    let bindings = cohort["bindings"].as_array().ok_or("cohort_bindings")?;
    if cohort["main_allocation"]["calibration_max_groups"]
        != cohort["cases"].as_array().unwrap().len()
        || cohort["main_allocation"]["holdout_min_groups_reserved"] != 200
    {
        return Err("cohort_calibration_allocation".into());
    }
    for bound in bindings {
        let path = Path::new(text(bound, "file")?);
        if !path.is_absolute()
            || sha(&std::fs::read(path).map_err(|_| "cohort_bound_read")?) != text(bound, "sha256")?
        {
            return Err("cohort_bound_file_changed".into());
        }
    }
    for name in ["source", "catalog", "authorization", "protocol"] {
        if !bindings.iter().any(|b| {
            b["role"] == name
                && b["file"] == manifest[format!("{name}_file")]
                && b["sha256"] == manifest[format!("{name}_sha256")]
        }) {
            return Err("cohort_manifest_binding".into());
        }
    }
    let expected_settings = json!({"model":"deepseek-flash","reasoning_effort":"ultra","max_tokens":8192,"timeout_s":60,"max_retries":0,"provider_protocol":"openai"});
    if cohort["main_collection_settings"] != expected_settings
        || cohort["main_system_prompt_sha256"] != sha(SYSTEM.as_bytes())
    {
        return Err("cohort_main_settings_or_prompt".into());
    }
    if manifest["source_sha256"] != EXPANDED_SOURCE
        || catalog["source_sha256"] != EXPANDED_SOURCE
        || catalog["catalog_extension"] != "official-mbpp-preserve-original-split-v1"
        || protocol["policy_version"] != "artifact-acceptance-v1"
        || protocol["template_version"] != "jev-artifact-acceptance-v1"
        || protocol["categories"] != json!(["artifact_acceptance"])
        || protocol["min_supported"] != 150
        || protocol["min_unsupported"] != 50
    {
        return Err("formal_source_protocol_binding".into());
    }
    let cases = manifest["cases"].as_array().ok_or("cases")?;
    if cases.is_empty() || cases.len() > 8 {
        return Err("batch_size_1_to_8".into());
    }
    let mut groups = HashSet::new();
    for case in cases {
        if !cohort["cases"].as_array().unwrap().contains(case)
            || case["split"] != "calibration"
            || !groups.insert(text(case, "group_id")?)
            || case["id"] != format!("mbpp-main-{}", case["task_id"])
            || case["source_kind"] != "external_mbpp_fresh_collection"
            || case["label"] != "unlabeled"
            || case["author_exposed"] != false
        {
            return Err("case_not_formally_preregistered".into());
        }
        let task = source
            .as_array()
            .ok_or("source_tasks")?
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("task")?;
        let entry = catalog["tasks"]
            .as_array()
            .ok_or("catalog_tasks")?
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("entry")?;
        if case["task_sha256"] != value_sha(task)
            || entry["task_sha256"] != value_sha(task)
            || entry["group_id"] != case["group_id"]
            || entry["reserved_split"] != "calibration"
            || case["public_interface_sha256"] != value_sha(&case["public_interface"])
        {
            return Err("case_source_binding".into());
        }
    }
    Ok(Inputs {
        source,
        catalog,
        authorization,
        cohort,
        resource: None,
    })
}

pub fn revalidate_row(row: &Value) -> Result<(), String> {
    let manifest = read_json(Path::new(text(row, "manifest_file")?))?;
    if sha(&std::fs::read(text(row, "manifest_file")?).map_err(|_| "manifest_read")?)
        != text(row, "manifest_sha256")?
        || manifest["artifact_cohort_sha256"] != row["artifact_cohort_sha256"]
        || manifest["artifact_cohort_hash"] != row["artifact_cohort_hash"]
        || manifest["artifact_cohort_file"] != row["artifact_cohort_file"]
    {
        return Err("pre_http_cohort_changed".into());
    }
    let inputs = validate(&manifest)?;
    reject_reserved(&inputs, std::slice::from_ref(row))?;
    Ok(())
}

/// Same SQLite table/counter, new immutable metadata key; old bindings untouched.
pub struct Ledger {
    connection: Connection,
    cap: u64,
    directory: PathBuf,
}
impl Ledger {
    pub fn open(directory: &Path, authorization_hash: &str, cap: u64) -> Result<Self, String> {
        if cap > 500 || cap == 0 {
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

pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    _resource: Option<&Value>,
    _hash: Option<&str>,
) -> Result<(), String> {
    let first = rows.first().ok_or("empty_batch")?;
    let identity = json!({"settings":first["settings"],"build":first["build_info"],"endpoint":first["endpoint_sha256"],"cohort":first["artifact_cohort_sha256"],"system":sha(SYSTEM.as_bytes())});
    let key = format!(
        "artifact_collection:{}",
        text(first, "artifact_cohort_sha256")?
    );
    ledger.bind(&key, &value_sha(&identity))
}
pub fn original_auth() -> &'static str {
    ORIGINAL_AUTH
}

pub fn reject_reserved(inputs: &Inputs, cases: &[Value]) -> Result<(), String> {
    for service in ["main_model", "jev"] {
        let root = PathBuf::from(text(&inputs.authorization[service], "ledger_dir")?);
        if root.join("reservation.lock").exists() {
            return Err("reservation_in_progress".into());
        }
        let dir = root.join("reservations");
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir).map_err(|_| "reservations_read")? {
            let path = entry.map_err(|_| "reservation_path")?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let slot = read_json(&path)?;
                if cases
                    .iter()
                    .any(|case| case["group_id"] == slot["group_id"])
                {
                    return Err("group_already_reserved".into());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_cohort_cannot_reset_old_binding_or_reuse_started_group() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(dir.path(), "original", 500).unwrap();
        ledger
            .bind("collection_settings_sha256", "old-binding")
            .unwrap();
        let record =
            json!({"group_id":"old","split":"calibration","artifact_cohort_sha256":"legacy"});
        assert_eq!(ledger.reserve("old", &record).unwrap(), 1);
        let cohort = json!({"ledger_snapshot":{"main_started":[{"ordinal":1,"group_id":"old","started":record}]},"cases":[{"group_id":"old"},{"group_id":"next"}]});
        ledger.validate_baseline(&cohort).unwrap();
        assert!(ledger.reserve("old", &json!({})).is_err());
        assert!(Ledger::open(dir.path(), "new-auth", 500).is_err());
        ledger
            .bind("artifact_collection:new-cohort", "new-build")
            .unwrap();
        let old: String = ledger
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key='collection_settings_sha256'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, "old-binding");
        ledger
            .require_next_cases(&cohort, &[json!({"group_id":"next"})])
            .unwrap();
        assert!(ledger
            .require_next_cases(&cohort, &[json!({"group_id":"old"})])
            .is_err());
        ledger
            .connection
            .execute("DELETE FROM attempts WHERE group_id='old'", [])
            .unwrap();
        assert!(ledger.validate_baseline(&cohort).is_err());
    }
    #[test]
    fn cap_and_duplicate_finish_keep_failed_slot() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(dir.path(), "fixture", 1).unwrap();
        assert_eq!(
            ledger
                .reserve("a", &json!({"split":"development"}))
                .unwrap(),
            1
        );
        ledger.finish(1, &json!({"call_status":"failed"})).unwrap();
        assert!(ledger.finish(1, &json!({})).is_err());
        assert!(ledger.reserve("b", &json!({})).is_err());
        assert_eq!(ledger.count().unwrap(), 1);
        assert!(Ledger::open(dir.path(), "fixture", 501).is_err());
    }
}
