//! New STDIN guard. Only immutable durability/hash/prompt primitives are reused.
#[allow(dead_code, unused_imports)]
#[path = "../verification_collect/support.rs"]
mod shared;
pub use shared::{
    check_seal, durable, new_file, read_json, seal, sha, text, value_sha, write_new, SYSTEM,
};
#[path = "ledger.rs"]
mod ledger;
pub use ledger::Ledger;
use serde_json::{json, Value};
use std::path::Path;

pub struct Inputs {
    pub source: Value,
    pub catalog: Value,
    pub authorization: Value,
    pub cohort: Value,
}
pub const APPS_BINDINGS: &[&str] = &[
    "source_proposal_file",
    "source_proposal_sha256",
    "source_proposal_hash",
    "source_snapshot_file",
    "source_snapshot_sha256",
    "source_authority_file",
    "source_authority_sha256",
    "oracle_runtime_binding",
    "holdout_opening_file",
    "holdout_opening_sha256",
    "holdout_opening_hash",
];

pub fn profile(value: &Value) -> Result<Value, String> {
    let settings = json!({"model":"deepseek-flash","reasoning_effort":"high","max_tokens":16384,
        "timeout_s":120,"max_retries":0,"provider_protocol":"openai"});
    if [
        "model",
        "reasoning_effort",
        "max_tokens",
        "timeout_s",
        "max_retries",
        "provider_protocol",
    ]
    .iter()
    .any(|key| value[*key] != settings[*key])
    {
        return Err("fixed_main_profile_required".into());
    }
    Ok(json!({"main_collection_settings":settings}))
}

fn python(action: &str, path: &Path) -> Result<Value, String> {
    let mut command = std::process::Command::new("python");
    command.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    command
        .args([
            "-m",
            "evals.verification.apps_stdin_v1.holdout_collection",
            action,
            "--file",
        ])
        .arg(path);
    for (key, _) in std::env::vars_os() {
        if ["KEY", "TOKEN", "SECRET"]
            .iter()
            .any(|part| key.to_string_lossy().to_ascii_uppercase().contains(part))
        {
            command.env_remove(key);
        }
    }
    let output = command.output().map_err(|_| "stdin_proof_unavailable")?;
    if !output.status.success() {
        return Err("stdin_proof_rejected".into());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "stdin_proof_invalid_output".into())
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    let mut plain = manifest.clone();
    plain
        .as_object_mut()
        .ok_or("manifest_object")?
        .remove("_actual_manifest_path");
    let proof = python(
        "validate-manifest",
        Path::new(text(manifest, "_actual_manifest_path")?),
    )?;
    let mut cohort = read_json(Path::new(text(manifest, "artifact_cohort_file")?))?;
    if manifest["schema"] != "buglecat.verification.stdin-holdout-main-manifest.v1"
        || manifest["split"] != "holdout"
        || cohort["collection_authorized_split"] != "development"
        || proof["status"] != "stdin_holdout_source_verified"
        || proof["hash"] != manifest["artifact_hash"]
        || proof["paid_calls"] != 0
        || cohort["formal_registration"] != false
        || cohort["holdout_opened"] != false
        || cohort["production_authorized"] != false
        || manifest["cases"]
            .as_array()
            .is_none_or(|cases| cases.is_empty() || cases.len() > 8)
    {
        return Err("genuine_stdin_holdout_only".into());
    }
    plain.as_object_mut().unwrap().remove("artifact_hash");
    if value_sha(&plain) != manifest["artifact_hash"] {
        return Err("manifest_seal".into());
    }
    let opening = read_json(Path::new(text(manifest, "holdout_opening_file")?))?;
    if opening["schema"] != "buglecat.verification.stdin-once-holdout-opening.v1"
        || opening["artifact_hash"] != manifest["holdout_opening_hash"]
        || sha(&std::fs::read(text(manifest, "holdout_opening_file")?).map_err(|_| "opening_read")?)
            != manifest["holdout_opening_sha256"]
        || opening["cohort_hash"] != cohort["artifact_hash"]
        || opening["opened_split"] != "holdout"
        || opening["holdout_opened"] != true
        || manifest["collector_build_info"] != super::provenance::build_info()?
    {
        return Err("genuine_holdout_opening_required".into());
    }
    // Private phase guard: authenticated historical accounting and the fixed
    // 400 allocation, never a replacement current ledger or persisted cohort.
    cohort["ledger_snapshot"] = opening["original_accounting_image"]["snapshot"].clone();
    cohort["original_finished"] = opening["original_accounting_image"]["finished"].clone();
    cohort["holdout_original_sql_image"] =
        opening["original_accounting_image"]["sql_image"].clone();
    cohort["cases"] = opening["cases"].clone();
    cohort["collection_authorized_split"] = json!("holdout");
    cohort["bindings"] = manifest["phase_proof_bindings"].clone();
    for field in [
        "holdout_opening_file",
        "holdout_opening_sha256",
        "holdout_opening_hash",
    ] {
        cohort[field] = manifest[field].clone();
    }
    super::provenance::check_current(&cohort)?;
    profile(manifest)?;
    let auth = read_json(Path::new(text(manifest, "authorization_file")?))?;
    if auth["schema"] != "buglecat.verification.stdin-approved-budget-extension.v1"
        || auth["main_model"]["max_attempts"] != 1642
        || auth["jev"]["max_attempts"] != 1037
    {
        return Err("genuine_approved_stdin_funding_required".into());
    }
    Ok(Inputs {
        source: manifest["public_tasks"].clone(),
        catalog: json!({"tasks":manifest["cases"].as_array().unwrap()
        .iter().map(|case| json!({"task_id":case["task_id"],"task_sha256":case["task_sha256"]})).collect::<Vec<_>>()}),
        authorization: auth,
        cohort,
    })
}

pub fn user_prompt(task: &Value, case: &Value) -> Result<String, String> {
    if task["task_id"] != case["task_id"]
        || task["source_kind"] != "apps_stdin_restricted_script"
        || case["source_kind"] != task["source_kind"]
    {
        return Err("public_stdin_task_identity".into());
    }
    Ok(text(task, "main_prompt")?.into())
}

pub fn static_row(row: &Value) -> Result<(), String> {
    if row["split"] != "holdout" {
        return Err("holdout_holdout_denied".into());
    }
    for (file, hash) in [
        ("manifest_file", "manifest_sha256"),
        ("artifact_cohort_file", "artifact_cohort_sha256"),
    ] {
        if sha(&std::fs::read(text(row, file)?).map_err(|_| "source_read")?) != text(row, hash)? {
            return Err("immutable_source_changed_before_request".into());
        }
    }
    let manifest = read_json(Path::new(text(row, "manifest_file")?))?;
    super::provenance::check_current(&json!({"bindings":manifest["phase_proof_bindings"]}))?;
    if sha(&std::fs::read(text(row, "holdout_opening_file")?).map_err(|_| "opening_read")?)
        != row["holdout_opening_sha256"]
    {
        return Err("opening_changed_before_request".into());
    }
    for bound in manifest["phase_proof_bindings"]
        .as_array()
        .ok_or("bindings")?
    {
        if sha(&std::fs::read(text(bound, "file")?).map_err(|_| "bound_read")?)
            != text(bound, "sha256")?
        {
            return Err("source_changed_before_request".into());
        }
    }
    Ok(())
}

pub fn revalidate_row(row: &Value) -> Result<(), String> {
    static_row(row)?;
    let proof = python(
        "validate-row",
        Path::new(text(row, "case_directory")?)
            .join("prepared-row.json")
            .as_path(),
    )?;
    if proof["status"] != "stdin_next_holdout_group_verified"
        || proof["row_hash"] != row["artifact_sha256"]
        || proof["paid_calls"] != 0
    {
        return Err("next_original_group_proof_rejected".into());
    }
    Ok(())
}

pub fn close_batch(row: &Value) -> Result<(), String> {
    static_row(row)?;
    let path = Path::new(text(row, "case_directory")?).join("prepared-row.json");
    let proof = python("close-batch", &path)?;
    if proof["status"] != "stdin_original_holdout_batch_closure_verified"
        || proof["paid_calls"] != 0
    {
        return Err("batch_closure_rejected".into());
    }
    Ok(())
}

pub fn bind_collection(rows: &[Value]) -> Result<(), String> {
    let first = rows.first().ok_or("empty_batch")?;
    let auth = read_json(Path::new(text(first, "authorization_file")?))?;
    let directory = Path::new(text(&auth["main_model"], "ledger_dir")?)
        .join("stdin-holdout-collections")
        .join(text(first, "artifact_cohort_hash")?);
    // Reject a link/junction before following a fixed marker namespace.
    let root = Path::new(text(&auth["main_model"], "ledger_dir")?)
        .canonicalize()
        .map_err(|_| "actual_main_root")?;
    for path in [root.join("stdin-holdout-collections"), directory.clone()] {
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("collection_namespace_link".into());
                }
            }
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err("collection_namespace_link".into());
            }
        }
        std::fs::create_dir_all(&path).map_err(|_| "collection_marker_directory")?;
        if !path
            .canonicalize()
            .map_err(|_| "namespace_identity")?
            .starts_with(&root)
        {
            return Err("collection_namespace_escape".into());
        }
    }
    write_new(
        &directory.join(format!("{}.started", text(first, "manifest_sha256")?)),
        value_sha(&json!({"cohort":first["artifact_cohort_sha256"],"build":first["build_info"]}))
            .as_bytes(),
    )
}
