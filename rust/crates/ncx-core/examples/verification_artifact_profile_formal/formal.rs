//! Formal gate: no transport before frozen source, original starts and labels join.
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_profile_collect/support.rs"]
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
    for field in [
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
    ] {
        if row[field] != cohort[field] || row[field] != manifest[field] {
            return Err("formal_main_profile_join".into());
        }
    }
    let receipt = bound(row, "main_receipt_file", "main_receipt_sha256", 1_000_000)?;
    for field in [
        "artifact_cohort_sha256",
        "artifact_cohort_hash",
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
    ] {
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
    for field in [
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
    ] {
        if start[field] != row[field] {
            return Err("original_main_start_profile_unbound".into());
        }
    }
    let finish_raw: String = db
        .query_row(
            "SELECT finished_json FROM attempts WHERE group_id=?1",
            [text(row, "group_id")?],
            |r| r.get(0),
        )
        .map_err(|_| "original_finish_missing")?;
    let finish: Value = serde_json::from_str(&finish_raw).map_err(|_| "original_finish_json")?;
    if finish["event"] != "attempt_finished"
        || finish["call_status"] != "ok"
        || finish["receipt_sha256"] != row["main_receipt_sha256"]
        || finish["request_hash"] != receipt["request_hash"]
    {
        return Err("original_main_finish_unbound".into());
    }
    for field in [
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
    ] {
        if finish[field] != row[field] {
            return Err("original_main_finish_profile_unbound".into());
        }
    }
    let receipt_path = Path::new(text(row, "main_receipt_file")?);
    let case_dir = receipt_path.parent().ok_or("main_case_directory")?;
    let request_bytes = read(&case_dir.join("request.json"), 128_000)?;
    let response_bytes = read(&case_dir.join("raw-response.json"), 512 * 1024)?;
    let request: Value =
        serde_json::from_slice(&request_bytes).map_err(|_| "original_main_request_json")?;
    let response: Value =
        serde_json::from_slice(&response_bytes).map_err(|_| "original_main_response_json")?;
    let candidate = text(&receipt["result"], "candidate")?;
    if sha(&request_bytes) != text(&receipt, "request_hash")?
        || sha(&response_bytes) != text(&receipt, "response_sha256")?
        || receipt["result"]["response_sha256"] != receipt["response_sha256"]
        || response["choices"]
            .as_array()
            .is_none_or(|rows| rows.len() != 1)
        || response["choices"][0]["message"]["content"] != candidate
        || response["model"] != "deepseek-flash"
        || response["usage"] != receipt["result"]["usage"]
        || sha(&read(&case_dir.join("candidate.txt"), 512 * 1024)?) != row["candidate_sha256"]
        || request["model"] != "deepseek-flash"
        || request["reasoning_effort"] != "high"
        || request["max_tokens"] != 16384
        || request["messages"][0]["content"] != collector::SYSTEM
    {
        return Err("original_main_raw_transport_join".into());
    }
    let manifest_path = Path::new(text(row, "collection_manifest_file")?);
    let prepared_path = manifest_path.with_file_name("prepared.jsonl");
    let prepared_raw = read(&prepared_path, 16_000_000)?;
    let originals: Vec<Value> = std::str::from_utf8(&prepared_raw)
        .map_err(|_| "original_prepared_utf8")?
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|s| serde_json::from_str(s).map_err(|_| "original_prepared_json".to_string()))
        .collect::<Result<_, _>>()?;
    let matches: Vec<_> = originals.iter().filter(|p| p["id"] == row["id"]).collect();
    if matches.len() != 1 {
        return Err("original_prepared_identity".into());
    }
    let prepared = matches[0];
    check_seal(prepared, "artifact_sha256")?;
    for field in [
        "request_hash",
        "settings",
        "build_info",
        "group_id",
        "task_id",
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
        "artifact_cohort_sha256",
        "artifact_cohort_hash",
    ] {
        if prepared[field] != receipt[field]
            || prepared[field] != start[field]
            || prepared[field] != finish[field]
        {
            return Err("original_main_prepared_events_join".into());
        }
    }
    if prepared["body"] != request
        || prepared["settings"] != cohort["main_collection_settings"]
        || prepared["manifest_sha256"] != row["collection_manifest_sha256"]
    {
        return Err("original_main_prepared_body_join".into());
    }
    let task = inputs
        .source
        .as_array()
        .ok_or("original_source_tasks")?
        .iter()
        .find(|t| t["task_id"] == row["task_id"])
        .ok_or("original_task")?;
    let case = manifest["cases"]
        .as_array()
        .ok_or("original_cases")?
        .iter()
        .find(|c| c["id"] == row["id"])
        .ok_or("original_case")?;
    let expected_prompt = collector::user_prompt(task, case)?;
    if prepared["prompt"] != expected_prompt
        || request["messages"].as_array().is_none_or(|m| m.len() != 2)
        || request["messages"][1]["content"] != expected_prompt
        || !request["tools"].is_null()
    {
        return Err("original_main_public_prompt_join".into());
    }
    for (field, expected) in [
        (
            "runner_source_sha256",
            sha(include_bytes!(
                "../verification_artifact_profile_collect.rs"
            )),
        ),
        (
            "support_source_sha256",
            sha(include_bytes!(
                "../verification_artifact_profile_collect/support.rs"
            )),
        ),
        (
            "collection_engine_source_sha256",
            sha(include_bytes!(
                "../verification_artifact_profile_collect/engine.rs"
            )),
        ),
        (
            "frozen_collector_support_sha256",
            sha(include_bytes!(
                "../verification_artifact_collect/support.rs"
            )),
        ),
        (
            "request_builder_source_sha256",
            sha(include_bytes!("../../../ncx-provider/src/request.rs")),
        ),
        (
            "response_parser_source_sha256",
            sha(include_bytes!("../../../ncx-provider/src/response.rs")),
        ),
    ] {
        if prepared["build_info"][field] != expected {
            return Err("original_main_compiled_source_join".into());
        }
    }
    let event_bytes = read(
        &manifest_path.with_file_name("attempt-events.jsonl"),
        32_000_000,
    )?;
    let events: Vec<Value> = std::str::from_utf8(&event_bytes)
        .map_err(|_| "original_events_utf8")?
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|s| serde_json::from_str(s).map_err(|_| "original_event_json".to_string()))
        .collect::<Result<_, _>>()?;
    let events: Vec<_> = events.iter().filter(|e| e["id"] == row["id"]).collect();
    if events.len() != 3
        || events[0]["event"] != "attempt_started"
        || events[1]["event"] != "request_submitted"
        || events[2] != &finish
        || events[1]["actual_request_hash"] != receipt["request_hash"]
    {
        return Err("original_main_event_sequence".into());
    }
    let mut original_event = events[0].clone();
    original_event
        .as_object_mut()
        .ok_or("original_start_object")?
        .remove("attempt_ordinal");
    if original_event != start {
        return Err("original_main_sql_start_event_join".into());
    }
    if sha(&read(
        Path::new(text(&receipt["result"], "session_log")?),
        2_000_000,
    )?) != text(&receipt["result"], "session_sha256")?
    {
        return Err("original_main_session_changed".into());
    }
    let session = Path::new(text(&receipt["result"], "session_log")?);
    if sha(&read(&session.with_extension("sources.jsonl"), 2_000_000)?)
        != text(&receipt["result"], "sources_sha256")?
        || sha(&read(&case_dir.join("packet.json"), 4_000_000)?)
            != text(&receipt["result"], "packet_sha256")?
    {
        return Err("original_main_sources_packet_changed".into());
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
        for field in [
            "artifact_cohort_sha256",
            "artifact_cohort_hash",
            "main_resource_profile_file",
            "main_resource_profile_sha256",
            "main_resource_profile_hash",
        ] {
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
