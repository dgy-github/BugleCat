//! Formal gate: no transport before frozen source, original starts and labels join.
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_collect/support.rs"]
mod collector;
use crate::support::*;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};

pub fn source_join(row: &Value, manifest: &Value) -> Result<(), String> {
    let inputs = collector::validate(manifest)?;
    let cohort = bound(
        row,
        "artifact_cohort_file",
        "artifact_cohort_sha256",
        16_000_000,
    )?;
    check_seal(&cohort, "artifact_hash")?;
    if cohort != inputs.cohort
        || row["artifact_cohort_hash"] != cohort["artifact_hash"]
        || row["artifact_cohort_sha256"] != manifest["artifact_cohort_sha256"]
    {
        return Err("formal_cohort_join".into());
    }
    let receipt = bound(row, "main_receipt_file", "main_receipt_sha256", 1_000_000)?;
    for field in ["artifact_cohort_sha256", "artifact_cohort_hash"] {
        if receipt[field] != row[field] {
            return Err("main_receipt_has_no_formal_cohort".into());
        }
    }
    let ledger = Path::new(text(&inputs.authorization["main_model"], "ledger_dir")?)
        .join("main-attempts.sqlite3");
    let db = Connection::open_with_flags(ledger, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "formal_ledger_read")?;
    let meta: String = db
        .query_row(
            "SELECT value FROM metadata WHERE key='authorization_sha256'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "formal_ledger_auth")?;
    if meta != ORIGINAL_AUTH {
        return Err("formal_ledger_auth".into());
    }
    let raw: String = db
        .query_row(
            "SELECT started_json FROM attempts WHERE group_id=?1",
            [text(row, "group_id")?],
            |r| r.get(0),
        )
        .map_err(|_| "original_formal_start_missing")?;
    let start: Value = serde_json::from_str(&raw).map_err(|_| "original_start_json")?;
    if start["authorization_sha256"] != AUTH
        || start["id"] != row["id"]
        || start["request_hash"] != receipt["request_hash"]
        || start["artifact_cohort_sha256"] != row["artifact_cohort_sha256"]
        || start["artifact_cohort_hash"] != row["artifact_cohort_hash"]
    {
        return Err("original_formal_start_unbound".into());
    }
    Ok(())
}

pub fn registration(
    path: &Path,
    prepared_path: &Path,
    selected: &Value,
) -> Result<(Value, String), String> {
    let raw = read(path, 8_000_000)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|_| "registration_json")?;
    check_seal(&value, "artifact_hash")?;
    if value["schema"] != "buglecat.verification.artifact-formal-registration.v1"
        || value["registered_before_jev"] != true
        || value["holdout_opened"] != false
        || value["production_authorized"] != false
        || value["label_counts"]["supported"]
            .as_u64()
            .is_none_or(|n| n < 150)
        || value["label_counts"]["unsupported"]
            .as_u64()
            .is_none_or(|n| n < 50)
        || value["protocol"]["min_supported"] != 150
        || value["protocol"]["min_unsupported"] != 50
    {
        return Err("formal_independent_label_quotas_required".into());
    }
    let data = bound(&value, "dataset_file", "dataset_file_sha256", 64_000_000)?;
    check_seal(&data, "artifact_hash")?;
    let frozen = read(Path::new(text(&value, "prepared_file")?), 64_000_000)?;
    if sha(&frozen) != text(&value, "prepared_file_sha256")?
        || Path::new(text(&value, "prepared_file")?)
            .canonicalize()
            .map_err(|_| "prepared_path")?
            != prepared_path.canonicalize().map_err(|_| "prepared_path")?
    {
        return Err("registered_prepared_changed".into());
    }
    let rows: Vec<Value> = std::str::from_utf8(&frozen)
        .map_err(|_| "prepared_utf8")?
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|s| serde_json::from_str(s).map_err(|_| "prepared_json".to_string()))
        .collect::<Result<_, _>>()?;
    if value["dataset_hash"] != digest(&data)
        || value["prepared_hash"] != digest(&json!(rows))
        || data["schema"] != "buglecat.verification.artifact-formal-dataset.v1"
        || data["scores_hidden"] != true
        || data["jev_probabilities_read"] != false
        || !rows.contains(selected)
    {
        return Err("registered_original_contents".into());
    }
    let mut groups = HashSet::new();
    let mut ids = HashSet::new();
    let mut supported = 0u64;
    let mut unsupported = 0u64;
    let cases = data["cases"].as_array().ok_or("dataset_cases")?;
    let mut all_ids = HashSet::new();
    let mut all_groups = HashSet::new();
    for case in cases {
        if !all_ids.insert(text(case, "id")?)
            || !all_groups.insert(text(case, "group_id")?)
            || case["split"] != "calibration"
            || case["author_exposed"] != false
            || case["source_kind"] != "source_preregistered_original_model_code"
        {
            return Err("formal_case_source".into());
        }
    }
    for row in &rows {
        validate_row(row)?;
        if !groups.insert(text(row, "group_id")?) || !ids.insert(text(row, "id")?) {
            return Err("formal_duplicate_group".into());
        }
        for field in ["artifact_cohort_sha256", "artifact_cohort_hash"] {
            if row[field] != value[field] || data[field] != value[field] {
                return Err("formal_registered_cohort".into());
            }
        }
        let protocol = bound(row, "protocol_file", "protocol_sha256", 128_000)?;
        if protocol != value["protocol"] {
            return Err("formal_protocol_changed".into());
        }
        let case = cases
            .iter()
            .find(|c| c["id"] == row["id"])
            .ok_or("formal_case_absent")?;
        let oracle = bound(
            row,
            "oracle_receipt_file",
            "oracle_receipt_sha256",
            1_000_000,
        )?;
        if case["group_id"] != row["group_id"]
            || case["candidate_sha256"] != row["candidate_sha256"]
            || case["main_receipt_sha256"] != row["main_receipt_sha256"]
            || case["oracle"] != oracle
            || case["call_eligible"] != true
        {
            return Err("formal_label_original_oracle".into());
        }
        match (case["label"].as_str(), oracle["result"]["status"].as_str()) {
            (Some("supported"), Some("passed")) => supported += 1,
            (Some("unsupported"), Some("failed")) => unsupported += 1,
            _ => return Err("formal_binary_label_original_oracle".into()),
        }
    }
    let mut sorted: Vec<_> = ids.into_iter().collect();
    sorted.sort();
    let eligible: HashSet<_> = cases
        .iter()
        .filter(|c| c["call_eligible"] == true)
        .map(|c| c["id"].clone())
        .collect();
    if eligible != rows.iter().map(|r| r["id"].clone()).collect()
        || value["case_ids"] != json!(sorted)
        || value["label_counts"] != json!({"supported":supported,"unsupported":unsupported})
        || supported < 150
        || unsupported < 50
    {
        return Err("formal_registration_counts_or_selection".into());
    }
    Ok((value, sha(&raw)))
}

pub fn output_within_ledger(directory: &Path, auth: &Value) -> Result<(), String> {
    let root = Path::new(text(&auth["jev"], "ledger_dir")?)
        .canonicalize()
        .map_err(|_| "output_ledger_root")?;
    let parent = directory
        .parent()
        .ok_or("output_parent")?
        .canonicalize()
        .map_err(|_| "output_parent")?;
    if !parent.starts_with(&root) || directory.exists() || directory.file_name().is_none() {
        return Err("formal_output_outside_ledger_or_exists".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unqualified_registration_rejected_before_reading_dataset_or_any_probability() {
        let dir = tempfile::tempdir().unwrap();
        for (supported, unsupported) in [(149, 50), (150, 49)] {
            let mut row = json!({"schema":"buglecat.verification.artifact-formal-registration.v1","registered_before_jev":true,"holdout_opened":false,"production_authorized":false,"label_counts":{"supported":supported,"unsupported":unsupported},"protocol":{"min_supported":150,"min_unsupported":50}});
            row["artifact_hash"] = json!(digest(&row));
            let path = dir.path().join(format!("{supported}-{unsupported}.json"));
            write_new(&path, row.to_string().as_bytes()).unwrap();
            assert_eq!(
                registration(&path, &dir.path().join("missing"), &json!({})).unwrap_err(),
                "formal_independent_label_quotas_required"
            );
        }
    }
}
