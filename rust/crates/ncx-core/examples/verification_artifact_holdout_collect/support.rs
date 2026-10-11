//! Shared source/profile validator and frozen budget engine reuse.
pub use high_profile::{chain, original_auth, reject_reserved, Inputs, Ledger};
pub use high_profile::{
    check_resource_identity, check_seal, durable, new_file, read_json, seal, sha, text,
    user_prompt, value_sha, write_new, SYSTEM,
};
use serde_json::{json, Value};
use std::{collections::HashSet, path::Path};
const EXPANDED_SOURCE: &str = "2875e6f093d50c4f10709666562b8f14018d932f31dc5dbda9ff81c02f4af9ce";
const SCHEMA: &str = "buglecat.verification.mbpp-collection.v1";
fn bound_file(value: &Value, file: &str, hash: &str) -> Result<Value, String> {
    let path = Path::new(text(value, file)?);
    if !path.is_absolute() {
        return Err("bound_path_absolute".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "bound_read")?;
    if sha(&bytes) != text(value, hash)? {
        return Err("bound_file_hash_mismatch".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "bound_json".into())
}
fn cohort_seal(value: &Value) -> Result<(), String> {
    let mut plain = value.clone();
    let expected = plain
        .as_object_mut()
        .ok_or("sealed_object")?
        .remove("artifact_hash")
        .ok_or("artifact_hash")?;
    if expected != value_sha(&plain) {
        return Err("artifact_seal_mismatch".into());
    }
    Ok(())
}
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_profile_collect/support.rs"]
mod high_profile;
pub use high_profile::{profile, profile_path};
pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    resource: Option<&Value>,
    hash: Option<&str>,
) -> Result<(), String> {
    high_profile::bind_collection(ledger, rows, resource, hash)?;
    let first = rows.first().ok_or("empty_batch")?;
    let key = format!(
        "main_profile:{}:{}",
        text(first, "main_resource_profile_sha256")?,
        text(first, "artifact_cohort_sha256")?
    );
    ledger.bind(&key, &value_sha(&json!({"profile":first["main_resource_profile_sha256"],"profile_hash":first["main_resource_profile_hash"],"cohort":first["artifact_cohort_sha256"],"settings":first["settings"],"build":first["build_info"],"endpoint":first["endpoint_sha256"]})))
}
pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    let selected = profile(manifest)?;
    if manifest["schema"] != SCHEMA
        || manifest["model"] != "deepseek-flash"
        || manifest["max_retries"] != 0
        || manifest["provider_protocol"] != "openai"
        || manifest["reasoning_effort"] != selected["main_collection_settings"]["reasoning_effort"]
        || manifest["max_tokens"] != selected["main_collection_settings"]["max_tokens"]
        || manifest["timeout_s"] != selected["main_collection_settings"]["timeout_s"]
    {
        return Err("formal_manifest_settings".into());
    }
    let source = bound_file(manifest, "source_file", "source_sha256")?;
    let catalog = bound_file(manifest, "catalog_file", "catalog_sha256")?;
    let authorization = bound_file(manifest, "authorization_file", "authorization_sha256")?;
    let protocol = bound_file(manifest, "protocol_file", "protocol_sha256")?;
    let cohort = bound_file(manifest, "artifact_cohort_file", "artifact_cohort_sha256")?;
    chain(&authorization, text(manifest, "authorization_sha256")?)?;
    cohort_seal(&cohort)?;
    for field in [
        "main_resource_profile_file",
        "main_resource_profile_sha256",
        "main_resource_profile_hash",
    ] {
        if cohort[field] != manifest[field] {
            return Err("profile_cohort_binding".into());
        }
    }
    if cohort["schema"] != "buglecat.verification.artifact-cohort.v1"
        || cohort["artifact_hash"] != manifest["artifact_cohort_hash"]
        || cohort["formal_registration"] != true
        || cohort["fixture_only"] != false
        || cohort["status"] != "registered"
        || cohort["collection_authorized"] != false
        || cohort["collection_authorization_status"] != "new_holdout_transport_entry_required"
        || cohort["full_campaign_source_gate"] != true
        || cohort["before_main_requests"] != true
        || cohort["scores_hidden"] != true
        || cohort["jev_probabilities_read"] != false
        || cohort["author_exposed"] != false
        || cohort["split"] != "holdout"
        || cohort["holdout_opened"] != true
        || cohort["cases"]
            .as_array()
            .is_none_or(|c| c.len() < 200 || c.len() > 500)
    {
        return Err("formal_cohort_required".into());
    }
    let bindings = cohort["bindings"].as_array().ok_or("cohort_bindings")?;
    if cohort["holdout_max_groups"] != cohort["cases"].as_array().unwrap().len()
        || cohort["holdout_lock"]["threshold_retuning_allowed"] != false
        || cohort["holdout_lock"]["production_authorized"] != false
    {
        return Err("holdout_locked_allocation".into());
    }
    for bound in bindings {
        let path = Path::new(text(bound, "file")?);
        if !path.is_absolute()
            || sha(&std::fs::read(path).map_err(|_| "cohort_bound_read")?) != text(bound, "sha256")?
        {
            return Err("cohort_bound_file_changed".into());
        }
    }
    for name in [
        "source",
        "catalog",
        "authorization",
        "protocol",
        "main_resource_profile",
    ] {
        if !bindings.iter().any(|b| {
            b["role"] == name
                && b["file"] == manifest[format!("{name}_file")]
                && b["sha256"] == manifest[format!("{name}_sha256")]
        }) {
            return Err("cohort_manifest_binding".into());
        }
    }
    let expected_settings = selected["main_collection_settings"].clone();
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
            || case["split"] != "holdout"
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
            || entry["reserved_split"] != "holdout"
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
        || manifest["main_resource_profile_file"] != row["main_resource_profile_file"]
        || manifest["main_resource_profile_sha256"] != row["main_resource_profile_sha256"]
        || manifest["main_resource_profile_hash"] != row["main_resource_profile_hash"]
    {
        return Err("pre_http_cohort_changed".into());
    }
    let inputs = validate_path(
        Path::new(text(row, "manifest_file")?),
        &manifest,
        Some(text(row, "id")?),
    )?;
    reject_reserved(&inputs, std::slice::from_ref(row))?;
    Ok(())
}

pub fn validate_path(
    path: &Path,
    manifest: &Value,
    selected: Option<&str>,
) -> Result<Inputs, String> {
    let inputs = validate(manifest)?;
    let raw = path.to_str().ok_or("manifest_utf8_path")?;
    let arguments = if let Some(id) = selected {
        vec!["main", raw, id]
    } else {
        vec!["main", raw]
    };
    let proof = crate::guard::run(&arguments)?;
    if proof["artifact_cohort_hash"] != manifest["artifact_cohort_hash"]
        || proof["artifact_cohort_sha256"] != manifest["artifact_cohort_sha256"]
        || proof["manifest_sha256"] != sha(&std::fs::read(path).map_err(|_| "manifest_read")?)
    {
        return Err("holdout_guard_manifest_join".into());
    }
    Ok(inputs)
}
