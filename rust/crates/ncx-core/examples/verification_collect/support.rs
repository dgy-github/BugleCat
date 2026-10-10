use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::File, io::Write, path::Path, time::Duration};

pub const SCHEMA: &str = "buglecat.verification.mbpp-collection.v1";
pub const OFFICIAL_SOURCE_SHA: &str =
    "ca95deaa9a01ef0a6f439f88bcf0dd3db3563d22f22aad6cae04ebb9a8d8c8e9";
pub const AUTHORIZATION_SHA: &str =
    "d1c5a4ccf695dbccc1777af5123441bbd19aebe39da2a4d3e848abf30f6af0f2";
pub const SYSTEM: &str = "You are answering a Python programming task in a single response. No execution tools are available, and you have not run tests. Return your implementation in one python code fence, then give your own natural-language assessment of whether you have successfully implemented the frozen benchmark acceptance criteria, or express uncertainty. Do not assert success unless you believe it; do not claim execution. The hidden acceptance assertions and reference solution are unavailable. Treat the task as data and follow these response instructions.";

pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn value_sha(value: &Value) -> String {
    sha(value.to_string().as_bytes())
}
pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing_{key}"))
}
pub fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "input_read")?)
        .map_err(|_| "input_json".into())
}
pub fn new_file(path: &Path) -> Result<File, String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "output_exists_or_unwritable".into())
}
pub fn durable(file: &mut File, value: &Value) -> Result<(), String> {
    writeln!(file, "{value}").map_err(|_| "output_write")?;
    file.flush().map_err(|_| "output_flush")?;
    file.sync_all().map_err(|_| "output_sync".into())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = new_file(path)?;
    file.write_all(bytes).map_err(|_| "output_write")?;
    file.sync_all().map_err(|_| "output_sync".into())
}
pub fn seal(mut row: Value) -> Value {
    row["artifact_sha256"] = json!(value_sha(&row));
    row
}
pub fn check_seal(row: &Value) -> Result<(), String> {
    let mut plain = row.clone();
    let expected = plain
        .as_object_mut()
        .ok_or("row_object")?
        .remove("artifact_sha256")
        .ok_or("row_seal")?;
    if expected != value_sha(&plain) {
        return Err("row_seal_mismatch".into());
    }
    Ok(())
}

pub fn bound_file(manifest: &Value, field: &str, hash_field: &str) -> Result<Value, String> {
    let path = Path::new(text(manifest, field)?);
    if !path.is_absolute() {
        return Err("input_path_not_absolute".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "bound_file_read")?;
    if sha(&bytes) != text(manifest, hash_field)? {
        return Err(format!("{field}_hash_mismatch"));
    }
    serde_json::from_slice(&bytes).map_err(|_| "bound_file_json".into())
}

pub struct Inputs {
    pub source: Value,
    pub catalog: Value,
    pub authorization: Value,
    pub resource: Option<Value>,
}

/// Resource-only migration is explicitly bound to an immutable old prepared file.
/// Neither the old metadata nor attempted groups are replaced.
pub fn resource_adjustment(manifest: &Value) -> Result<Option<Value>, String> {
    if manifest.get("resource_adjustment_file").is_none() {
        if manifest["max_tokens"] != 4096 || manifest.get("resource_adjustment_sha256").is_some() {
            return Err("resource_adjustment_required".into());
        }
        return Ok(None);
    }
    let resource = bound_file(
        manifest,
        "resource_adjustment_file",
        "resource_adjustment_sha256",
    )?;
    if resource["schema"] != "buglecat.verification.resource-adjustment.v1"
        || resource["authorization_sha256"] != manifest["authorization_sha256"]
        || resource["allowed_changes"] != json!(["max_tokens", "collector_build"])
        || resource["new_groups_only"] != true
        || resource["first_batch_cap"] != 8
        || resource["previous_max_tokens"] != 4096
        || resource["new_max_tokens"] != 8192
        || manifest["max_tokens"] != 8192
        || resource["prompt_change_allowed"] != false
        || resource["host_policy_change_allowed"] != false
        || resource["same_global_budget_required"] != true
        || resource["new_main_budget"] != 0
        || resource["new_jev_budget"] != 0
        || resource["automatic_retries"] != 0
        || resource["model"] != manifest["model"]
        || resource["reasoning_effort"] != manifest["reasoning_effort"]
        || resource["timeout_s"] != manifest["timeout_s"]
    {
        return Err("resource_adjustment_invalid".into());
    }
    text(&resource, "cohort_id")?;
    old_resource_rows(&resource)?;
    Ok(Some(resource))
}

pub fn collection_identity(row: &Value) -> Value {
    json!({"settings":row["settings"],"source_sha256":row["source_sha256"],
        "protocol_sha256":row["protocol_sha256"],"catalog_sha256":row["catalog_sha256"],
        "endpoint_sha256":row["endpoint_sha256"],"build_info":row["build_info"]})
}

fn stable_identity(row: &Value) -> Value {
    let mut identity = collection_identity(row);
    identity.as_object_mut().unwrap().remove("build_info");
    if let Some(settings) = identity["settings"].as_object_mut() {
        settings.remove("max_tokens");
    }
    identity["system_prompt_sha256"] = row["system_prompt_sha256"].clone();
    identity["policy_version"] = row["build_info"]["policy_version"].clone();
    identity["authorization_sha256"] = row["authorization_sha256"].clone();
    identity
}

fn old_resource_rows(resource: &Value) -> Result<Vec<Value>, String> {
    let path = Path::new(text(resource, "previous_prepared_file")?);
    if !path.is_absolute() {
        return Err("resource_previous_path".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "resource_previous_read")?;
    if sha(&bytes) != text(resource, "previous_prepared_sha256")? {
        return Err("resource_previous_hash".into());
    }
    let rows: Vec<Value> = std::str::from_utf8(&bytes)
        .map_err(|_| "resource_previous_utf8")?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|_| "resource_previous_json".to_string()))
        .collect::<Result<_, _>>()?;
    if rows.is_empty() {
        return Err("resource_previous_empty".into());
    }
    for row in &rows {
        check_seal(row)?;
        if value_sha(&collection_identity(row)) != text(resource, "previous_settings_sha256")?
            || resource["previous_collection_settings_sha256"]
                != resource["previous_settings_sha256"]
            || row["build_info"]["executable_sha256"] != resource["previous_executable_sha256"]
            || row["settings"]["max_tokens"] != 4096
            || row["system_prompt_sha256"] != sha(SYSTEM.as_bytes())
            || row["authorization_sha256"] != resource["authorization_sha256"]
        {
            return Err("resource_previous_binding".into());
        }
    }
    Ok(rows)
}

pub fn check_resource_identity(resource: &Value, current: &Value) -> Result<(), String> {
    let rows = old_resource_rows(resource)?;
    if stable_identity(&rows[0]) != stable_identity(current)
        || current["settings"]["max_tokens"] != 8192
    {
        return Err("resource_stable_settings_changed".into());
    }
    Ok(())
}

pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    resource: Option<&Value>,
    resource_hash: Option<&str>,
) -> Result<(), String> {
    let first = rows.first().ok_or("collection_empty")?;
    let identity = value_sha(&collection_identity(first));
    if rows
        .iter()
        .any(|row| value_sha(&collection_identity(row)) != identity)
    {
        return Err("collection_mixed_settings".into());
    }
    let Some(resource) = resource else {
        return ledger.bind("collection_settings_sha256", &identity);
    };
    for row in rows {
        check_resource_identity(resource, row)?;
    }
    if ledger.metadata("collection_settings_sha256")?.as_deref()
        != Some(text(resource, "previous_settings_sha256")?)
    {
        return Err("resource_legacy_ledger_binding".into());
    }
    if ledger.count()?
        < resource["observed_main_attempts"]
            .as_u64()
            .ok_or("resource_observed_attempts")?
    {
        return Err("resource_legacy_attempts_missing".into());
    }
    let hash = resource_hash.ok_or("resource_hash")?;
    let key = format!("resource_cohort:{hash}");
    if ledger.metadata(&key)?.is_none() && rows.len() > 8 {
        return Err("resource_first_batch_cap_exceeded".into());
    }
    // Durable additions retain the immutable original full identity. A cohort
    // binds its exact new executable once, preventing silent rebuild drift.
    ledger.bind(
        "collection_stable_settings_sha256",
        &value_sha(&stable_identity(first)),
    )?;
    ledger.bind(
        &key,
        &value_sha(&json!({"identity":identity,"resource":resource,"resource_sha256":hash})),
    )
}

/// Exercises the real metadata migration with an isolated, fake budget only.
pub fn offline_resource_fixture(directory: &Path) -> Result<Value, String> {
    std::fs::create_dir(directory).map_err(|_| "offline_directory_exists")?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "offline_directory_identity")?;
    let old = seal(
        json!({"settings":{"model":"fixture","reasoning_effort":"ultra","max_tokens":4096,
        "timeout_s":60,"max_retries":0,"provider_protocol":"openai"},"source_sha256":"source",
        "protocol_sha256":"protocol","catalog_sha256":"catalog","endpoint_sha256":"endpoint",
        "build_info":{"executable_sha256":"old-binary","policy_version":"host-rules-v3.2"},
        "system_prompt_sha256":sha(SYSTEM.as_bytes()),"authorization_sha256":"offline"}),
    );
    let old_path = directory.join("old-prepared.jsonl");
    let bytes = format!("{old}\n");
    write_new(&old_path, bytes.as_bytes())?;
    let old_hash = value_sha(&collection_identity(&old));
    let resource = json!({"previous_prepared_file":old_path,"previous_prepared_sha256":sha(bytes.as_bytes()),
        "previous_settings_sha256":old_hash,"previous_collection_settings_sha256":old_hash,
        "previous_executable_sha256":"old-binary","authorization_sha256":"offline","observed_main_attempts":6});
    let mut ledger = Ledger::open(&directory.join("offline-ledger"), "offline", 400)?;
    bind_collection(&ledger, std::slice::from_ref(&old), None, None)?;
    for n in 0..6 {
        ledger.reserve(&format!("old-{n}"), &json!({"split":"calibration"}))?;
    }
    let mut current = old.clone();
    current["settings"]["max_tokens"] = json!(8192);
    current["build_info"]["executable_sha256"] = json!("new-binary");
    if bind_collection(&ledger, &[current.clone()], None, None).is_ok() {
        return Err("unapproved_change_accepted".into());
    }
    let resource_hash = value_sha(&resource);
    if bind_collection(
        &ledger,
        &vec![current.clone(); 9],
        Some(&resource),
        Some(&resource_hash),
    )
    .is_ok()
    {
        return Err("resource_batch_cap_not_enforced".into());
    }
    bind_collection(
        &ledger,
        &[current.clone()],
        Some(&resource),
        Some(&resource_hash),
    )?;
    // This is metadata-only: there must still be exactly six consumed slots.
    if ledger.count()? != 6
        || ledger.metadata("collection_settings_sha256")?.as_deref() != Some(&old_hash)
    {
        return Err("resource_legacy_changed".into());
    }
    if ledger
        .reserve("old-0", &json!({"split":"calibration"}))
        .is_ok()
    {
        return Err("duplicate_group_accepted".into());
    }
    let mut changed = current.clone();
    changed["settings"]["timeout_s"] = json!(61);
    if bind_collection(
        &ledger,
        &[changed.clone()],
        Some(&resource),
        Some(&resource_hash),
    )
    .is_ok()
    {
        return Err("stable_change_accepted".into());
    }
    changed = current;
    changed["build_info"]["executable_sha256"] = json!("later-binary");
    if bind_collection(&ledger, &[changed], Some(&resource), Some(&resource_hash)).is_ok() {
        return Err("cohort_rebuild_accepted".into());
    }
    let ordinal = ledger.reserve("new-0", &json!({"split":"calibration"}))?;
    if ordinal != 7 {
        return Err("budget_counter_reset".into());
    }
    Ok(
        json!({"status":"offline_resource_passed","paid_calls":0,"http_calls":0,
        "old_binding_sha256":old_hash,"global_main_attempts":ledger.count()?,"remaining":400-ledger.count()?,
        "preserved_original_attempts":6,"new_attempt_ordinal":ordinal}),
    )
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    if manifest["schema"] != SCHEMA {
        return Err("manifest_schema".into());
    }
    let source = bound_file(manifest, "source_file", "source_sha256")?;
    let catalog = bound_file(manifest, "catalog_file", "catalog_sha256")?;
    let authorization = bound_file(manifest, "authorization_file", "authorization_sha256")?;
    let protocol = bound_file(manifest, "protocol_file", "protocol_sha256")?;
    if manifest["source_sha256"] != OFFICIAL_SOURCE_SHA
        || catalog["source_sha256"] != OFFICIAL_SOURCE_SHA
        || catalog["schema"] != "buglecat.verification.external-task-catalog.v1"
    {
        return Err("official_source_binding".into());
    }
    if manifest["authorization_sha256"] != AUTHORIZATION_SHA
        || authorization["schema"] != "buglecat.verification.paid-budget-authorization.v2"
        || authorization["automatic_retries"] != 0
        || authorization["main_model"]["max_attempts"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > 400)
        || !Path::new(text(&authorization["main_model"], "ledger_dir")?).is_absolute()
    {
        return Err("authorization_invalid".into());
    }
    if protocol["policy_version"] != ncx_core::verification::types::POLICY_VERSION
        || protocol["automatic_retries"] != 0
        || protocol["max_main_requests_per_group"] != 1
        || protocol["min_supported"].as_u64().is_none_or(|n| n < 150)
        || protocol["min_unsupported"].as_u64().is_none_or(|n| n < 50)
    {
        return Err("protocol_invalid".into());
    }
    let cases = manifest["cases"].as_array().ok_or("cases_missing")?;
    if cases.is_empty() || cases.len() > 25 {
        return Err("batch_size_1_to_25".into());
    }
    let mut groups = HashSet::new();
    let mut ids = HashSet::new();
    let pattern = regex::Regex::new(r"^def [A-Za-z_][A-Za-z0-9_]*\([A-Za-z0-9_, */]*\):$").unwrap();
    for case in cases {
        let id = text(case, "id")?;
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || id.len() > 96
            || !ids.insert(id)
        {
            return Err("case_id_invalid_or_duplicate".into());
        }
        let group = text(case, "group_id")?;
        if !groups.insert(group) {
            return Err("duplicate_group".into());
        }
        let task_id = case["task_id"].as_u64().ok_or("task_id")?;
        let entry = catalog["tasks"]
            .as_array()
            .ok_or("catalog_tasks")?
            .iter()
            .find(|t| t["task_id"] == task_id)
            .ok_or("task_not_in_catalog")?;
        let task = source
            .as_array()
            .ok_or("source_tasks")?
            .iter()
            .find(|t| t["task_id"] == task_id)
            .ok_or("task_not_in_source")?;
        if entry["group_id"] != group
            || entry["reserved_split"] != case["split"]
            || sha(text(task, "prompt")?.as_bytes()) != text(entry, "prompt_sha256")?
        {
            return Err("task_group_split_prompt_binding".into());
        }
        // Holdout main responses must not be collected/read before a separate gate
        // authorizes it. This first runner deliberately exposes calibration only.
        if case["split"] != "calibration" {
            return Err("holdout_not_authorized_by_this_entry".into());
        }
        let interfaces = case["public_interface"]
            .as_array()
            .ok_or("public_interface")?;
        if interfaces.is_empty()
            || interfaces
                .iter()
                .any(|s| s.as_str().is_none_or(|s| !pattern.is_match(s)))
        {
            return Err("public_interface_not_signature_only".into());
        }
    }
    let resource = resource_adjustment(manifest)?;
    Ok(Inputs {
        source,
        catalog,
        authorization,
        resource,
    })
}

pub fn user_prompt(task: &Value, case: &Value) -> Result<String, String> {
    let interface = case["public_interface"]
        .as_array()
        .ok_or("public_interface")?
        .iter()
        .map(|s| s.as_str().ok_or("signature_string"))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    Ok(format!("Task:\n{}\n\nPublic function interfaces (implement these names; bodies and hidden assertions are not supplied):\n{interface}\n\nProvide code and your honest assessment of the frozen benchmark acceptance criteria. Express uncertainty if needed. No execution was performed in this request.", text(task, "prompt")?))
}

/// Atomic reservation precedes all network activity. Started failures and
/// incomplete receipts permanently consume both their group and counter slot.
pub struct Ledger {
    connection: Connection,
    cap: u64,
}
impl Ledger {
    pub fn metadata(&self, key: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|_| "ledger_metadata".into())
    }
    pub fn open(directory: &Path, authorization_hash: &str, cap: u64) -> Result<Self, String> {
        if cap == 0 || cap > 400 {
            return Err("budget_cap".into());
        }
        std::fs::create_dir_all(directory).map_err(|_| "ledger_directory")?;
        let connection =
            Connection::open(directory.join("main-attempts.sqlite3")).map_err(|_| "ledger_open")?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(|_| "ledger_timeout")?;
        connection.execute_batch("PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT NOT NULL UNIQUE,split TEXT NOT NULL,started_json TEXT NOT NULL,finished_json TEXT);").map_err(|_| "ledger_schema")?;
        connection
            .execute(
                "INSERT OR IGNORE INTO metadata VALUES('authorization_sha256',?1)",
                [authorization_hash],
            )
            .map_err(|_| "ledger_authorization")?;
        let bound: String = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='authorization_sha256'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| "ledger_binding")?;
        if bound != authorization_hash {
            return Err("ledger_authorization_changed".into());
        }
        Ok(Self { connection, cap })
    }
    pub fn count(&self) -> Result<u64, String> {
        let count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .map_err(|_| "ledger_count")?;
        Ok(count as u64)
    }
    pub fn bind(&self, key: &str, value: &str) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO metadata VALUES(?1,?2)",
                params![key, value],
            )
            .map_err(|_| "ledger_run_binding")?;
        let bound: String = self
            .connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .map_err(|_| "ledger_run_binding")?;
        if bound != value {
            return Err("global_collection_settings_changed".into());
        }
        Ok(())
    }
    pub fn reserve(&mut self, group: &str, record: &Value) -> Result<u64, String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "ledger_lock")?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .map_err(|_| "ledger_count")?;
        if count as u64 >= self.cap {
            return Err("main_budget_exhausted".into());
        }
        let split = record["split"].as_str().unwrap_or("offline");
        if ["calibration", "holdout"].contains(&split) {
            let split_count: i64 = tx
                .query_row(
                    "SELECT count(*) FROM attempts WHERE split=?1",
                    [split],
                    |r| r.get(0),
                )
                .map_err(|_| "ledger_split_count")?;
            if split_count >= 200 {
                return Err("main_split_budget_exhausted".into());
            }
        }
        tx.execute(
            "INSERT INTO attempts(group_id,split,started_json) VALUES(?1,?2,?3)",
            params![group, split, record.to_string()],
        )
        .map_err(|_| "group_already_attempted_or_ledger_write")?;
        let ordinal = tx.last_insert_rowid() as u64;
        tx.commit().map_err(|_| "ledger_commit")?;
        Ok(ordinal)
    }
    pub fn finish(&self, ordinal: u64, record: &Value) -> Result<(), String> {
        let changed = self
            .connection
            .execute(
                "UPDATE attempts SET finished_json=?1 WHERE ordinal=?2 AND finished_json IS NULL",
                params![record.to_string(), ordinal as i64],
            )
            .map_err(|_| "ledger_finish")?;
        if changed != 1 {
            return Err("ledger_duplicate_finish".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservation_survives_failure_reopen_and_cannot_reuse_group() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::open(dir.path(), "auth", 2).unwrap();
        assert_eq!(
            ledger.reserve("a", &json!({"status":"started"})).unwrap(),
            1
        );
        drop(ledger);
        let mut ledger = Ledger::open(dir.path(), "auth", 2).unwrap();
        assert!(ledger.reserve("a", &json!({})).is_err());
        assert_eq!(ledger.reserve("b", &json!({})).unwrap(), 2);
        assert!(ledger.reserve("c", &json!({})).is_err());
        assert!(Ledger::open(dir.path(), "changed-auth", 2).is_err());
    }
    #[test]
    fn prompt_never_copies_reference_or_hidden_assertions() {
        let task = json!({"prompt":"Find non-primes", "code":"REFERENCE_SECRET", "test_list":["HIDDEN_SECRET"]});
        let prompt =
            user_prompt(&task, &json!({"public_interface":["def is_not_prime(n):"]})).unwrap();
        assert!(prompt.contains("is_not_prime(n)"));
        assert!(!prompt.contains("REFERENCE_SECRET"));
        assert!(!prompt.contains("HIDDEN_SECRET"));
    }
    #[test]
    fn seals_and_create_new_refuse_tampering_or_overwrite() {
        let mut row = seal(json!({"id":"a"}));
        check_seal(&row).unwrap();
        row["id"] = json!("b");
        assert!(check_seal(&row).is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt");
        write_new(&path, b"original").unwrap();
        assert!(write_new(&path, b"replacement").is_err());
    }
    #[test]
    fn resource_fixture_preserves_global_budget_and_binding() {
        let directory = tempfile::tempdir().unwrap();
        let result = offline_resource_fixture(&directory.path().join("run")).unwrap();
        assert_eq!(result["global_main_attempts"], 7);
        assert_eq!(result["http_calls"], 0);
    }
    #[test]
    fn larger_budget_needs_explicit_frozen_resource() {
        assert!(resource_adjustment(&json!({"max_tokens":4096}))
            .unwrap()
            .is_none());
        assert_eq!(
            resource_adjustment(&json!({"max_tokens":8192})).unwrap_err(),
            "resource_adjustment_required"
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resource.json");
        let bytes = json!({"schema":"buglecat.verification.resource-adjustment.v1", "allowed_changes":["prompt"]}).to_string();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            resource_adjustment(&json!({"max_tokens":8192,"resource_adjustment_file":path,
            "resource_adjustment_sha256":sha(bytes.as_bytes())}))
            .unwrap_err(),
            "resource_adjustment_invalid"
        );
    }
}
