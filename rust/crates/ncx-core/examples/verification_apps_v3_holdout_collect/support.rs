//! APPS-only source proof and public prompt; original SQLite owner reused.
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_v2_collect/support.rs"]
mod shared;
use serde_json::{json, Value};
pub use shared::{
    check_resource_identity, check_seal, durable, new_file, original_auth, profile, profile_path,
    read_json, reject_reserved, seal, sha, text, value_sha, write_new, Inputs, SYSTEM,
};
#[path = "../verification_apps_v3_formal_collect/ledger.rs"]
#[allow(dead_code)]
mod ledger;
pub use ledger::Ledger;
use std::path::Path;
const OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/development.py");
const PACKET: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/packet.py");
const BUDGET: &[u8] =
    include_bytes!("../../../../../evals/verification/budget_authorization_v2.py");
const GUARD: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/admission_guard.py");
const RESOURCES: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/resources.py");
const AUTHORITY: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/source_authority.py");
const PROTOCOL: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/protocol.py");

const FORMAL_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/formal_collection.py");
const INTAKE_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/formal_intake.py");
const SAMPLING: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/sampling_proposal.py");
const CONTRACT: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/formal_contract.py");
const PROSPECTIVE: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/prospective_protocol.py");
const APPROVED_BUDGET: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/approved_budget.py");
const FORMAL_LEDGER: &[u8] = include_bytes!("../verification_apps_v3_formal_collect/ledger.rs");

const HOLDOUT_COLLECTION: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/holdout_collection.py");
const HOLDOUT_INTAKE: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/holdout_intake.py");
const HOLDOUT_SOURCE: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/formal_holdout.py");
const ARCHIVE: &[u8] =
    include_bytes!("../../../../../evals/verification/apps_campaign_v3/formal_archive.py");

pub fn user_prompt(task: &Value, case: &Value) -> Result<String, String> {
    if task["task_id"] != case["task_id"] || task["source_kind"] != case["source_kind"] {
        return Err("public_task_identity".into());
    }
    Ok(text(task, "main_prompt")?.into())
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    let path = Path::new(text(manifest, "_actual_manifest_path")?);
    let cohort = read_json(Path::new(text(manifest, "artifact_cohort_file")?))?;
    if manifest["schema"] != "buglecat.verification.apps-holdout-main-collection.v3"
        || cohort["schema"] != "buglecat.verification.apps-formal-main-cohort.v3"
        || manifest["split"] != "holdout"
        || cohort["collection_authorized_split"] != "calibration"
        || manifest["formal_registration"] != false
        || cohort["formal_registration"] != false
        || cohort["holdout_opened"] != false
        || cohort["production_authorized"] != false
        || manifest["cases"]
            .as_array()
            .is_none_or(|v| v.is_empty() || v.len() > 8)
    {
        return Err("apps_development_only".into());
    }
    for (role, compiled) in [
        ("development_owner", OWNER),
        ("formal_collection_owner", FORMAL_OWNER),
        ("formal_intake_owner", INTAKE_OWNER),
        ("sampling_proposal_owner", SAMPLING),
        ("formal_contract_owner", CONTRACT),
        ("prospective_protocol_owner", PROSPECTIVE),
        ("approved_budget_owner", APPROVED_BUDGET),
        ("formal_ledger_source", FORMAL_LEDGER),
        ("apps_call_based_free_function:packet", PACKET),
        ("shared_python:budget_authorization_v2.py", BUDGET),
        ("admission_guard", GUARD),
        ("resources_owner", RESOURCES),
        ("authority_owner", AUTHORITY),
        ("protocol_owner", PROTOCOL),
    ] {
        let bound = cohort["bindings"]
            .as_array()
            .ok_or("bindings")?
            .iter()
            .find(|b| b["role"] == role)
            .ok_or("compiled_binding_missing")?;
        let python_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/verification");
        let actual = match role {
            "formal_collection_owner" => python_root.join("apps_campaign_v3/formal_collection.py"),
            "formal_intake_owner" => python_root.join("apps_campaign_v3/formal_intake.py"),
            "sampling_proposal_owner" => python_root.join("apps_campaign_v3/sampling_proposal.py"),
            "formal_contract_owner" => python_root.join("apps_campaign_v3/formal_contract.py"),
            "prospective_protocol_owner" => {
                python_root.join("apps_campaign_v3/prospective_protocol.py")
            }
            "approved_budget_owner" => python_root.join("apps_campaign_v3/approved_budget.py"),
            "formal_ledger_source" => Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/verification_apps_v3_formal_collect/ledger.rs"),
            "shared_python:budget_authorization_v2.py" => {
                python_root.join("budget_authorization_v2.py")
            }
            "apps_call_based_free_function:packet" => {
                python_root.join("apps_campaign_v3/packet.py")
            }
            "admission_guard" => python_root.join("apps_campaign_v3/admission_guard.py"),
            "resources_owner" => python_root.join("apps_campaign_v3/resources.py"),
            "authority_owner" => python_root.join("apps_campaign_v3/source_authority.py"),
            "protocol_owner" => python_root.join("apps_campaign_v3/protocol.py"),
            _ => python_root.join("apps_campaign_v3/development.py"),
        };
        if sha(&std::fs::read(text(bound, "file")?).map_err(|_| "compiled_owner_read")?)
            != sha(compiled)
            || bound["sha256"] != sha(compiled)
            || sha(&std::fs::read(actual).map_err(|_| "actual_owner_read")?) != sha(compiled)
        {
            return Err("compiled_owner_changed".into());
        }
    }

    for (name, compiled) in [
        ("holdout_collection.py", HOLDOUT_COLLECTION),
        ("holdout_intake.py", HOLDOUT_INTAKE),
        ("formal_holdout.py", HOLDOUT_SOURCE),
        ("formal_archive.py", ARCHIVE),
    ] {
        let bound = manifest["holdout_owner_bindings"]
            .as_array()
            .ok_or("holdout_owners")?
            .iter()
            .find(|b| {
                Path::new(b["file"].as_str().unwrap_or(""))
                    .file_name()
                    .is_some_and(|s| s == name)
            })
            .ok_or("holdout_compiled_owner_missing")?;
        let actual = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../evals/verification/apps_campaign_v3")
            .join(name);
        if bound["sha256"] != sha(compiled)
            || sha(&std::fs::read(text(bound, "file")?).map_err(|_| "holdout_owner_read")?)
                != sha(compiled)
            || sha(&std::fs::read(actual).map_err(|_| "holdout_current_owner_read")?)
                != sha(compiled)
        {
            return Err("holdout_compiled_owner_changed".into());
        }
    }
    let mut command = std::process::Command::new("python");
    command.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    command
        .args([
            "-m",
            "evals.verification.apps_campaign_v3.holdout_collection",
            "validate-manifest",
            "--manifest",
        ])
        .arg(path);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy().to_ascii_uppercase();
        if ["KEY", "TOKEN", "SECRET"]
            .iter()
            .any(|part| name.contains(part))
        {
            command.env_remove(key);
        }
    }
    let output = command.output().map_err(|_| "apps_proof_unavailable")?;
    let proof: Value = serde_json::from_slice(&output.stdout).map_err(|_| "apps_proof_rejected")?;
    let mut original = manifest.clone();
    original
        .as_object_mut()
        .ok_or("manifest_object")?
        .remove("_actual_manifest_path");
    let expected = original
        .as_object_mut()
        .ok_or("manifest_object")?
        .remove("artifact_hash")
        .ok_or("manifest_seal")?;
    if !output.status.success()
        || proof["status"] != "apps_original_holdout_source_verified"
        || proof["new_paid_calls"] != 0
        || proof["formal_registration"] != false
        || proof["hash"] != expected
        || expected != value_sha(&original)
    {
        return Err("apps_source_proof_rejected".into());
    }
    profile(manifest)?;
    let catalog = json!({"tasks": manifest["cases"].as_array().unwrap().iter().map(|c|
        json!({"task_id":c["task_id"],"task_sha256":c["task_sha256"]})).collect::<Vec<_>>()});
    Ok(Inputs {
        source: manifest["public_tasks"].clone(),
        catalog,
        authorization: read_json(Path::new(text(manifest, "authorization_file")?))?,
        cohort,
        resource: None,
    })
}

pub fn revalidate_row(row: &Value) -> Result<(), String> {
    let path = Path::new(text(row, "manifest_file")?);
    if sha(&std::fs::read(path).map_err(|_| "manifest_read")?) != text(row, "manifest_sha256")? {
        return Err("manifest_changed_before_request".into());
    }
    let cohort_path = Path::new(text(row, "artifact_cohort_file")?);
    if sha(&std::fs::read(cohort_path).map_err(|_| "cohort_read")?)
        != text(row, "artifact_cohort_sha256")?
    {
        return Err("cohort_changed_before_request".into());
    }
    let cohort = read_json(cohort_path)?;
    for bound in cohort["bindings"].as_array().ok_or("bindings")? {
        if sha(&std::fs::read(text(bound, "file")?).map_err(|_| "source_read")?)
            != text(bound, "sha256")?
        {
            return Err("source_changed_before_request".into());
        }
    }
    let marker_path = Path::new(text(row, "holdout_source_file")?);
    if sha(&std::fs::read(marker_path).map_err(|_| "holdout_marker_read")?)
        != text(row, "holdout_source_sha256")?
        || read_json(marker_path)?["artifact_hash"] != row["holdout_source_hash"]
    {
        return Err("holdout_marker_changed_before_request".into());
    }
    let source_marker = read_json(marker_path)?;
    for bound in source_marker["source_bindings"]
        .as_array()
        .ok_or("original_holdout_proof_bindings")?
    {
        if sha(&std::fs::read(text(bound, "file")?).map_err(|_| "original_holdout_proof_read")?)
            != text(bound, "sha256")?
        {
            return Err("original_holdout_proof_changed_before_request".into());
        }
    }
    if row["split"] != "holdout" {
        return Err("holdout_denied".into());
    }
    Ok(())
}

pub const APPS_BINDINGS: &[&str] = &[
    "source_authority_file",
    "source_authority_sha256",
    "resource_inventory_file",
    "resource_inventory_sha256",
    "packet_source_sha256",
    "oracle_runtime_binding",
    "sampling_proposal_file",
    "sampling_proposal_sha256",
    "sampling_proposal_hash",
    "holdout_source_file",
    "holdout_source_sha256",
    "holdout_source_hash",
];

pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    _resource: Option<&Value>,
    _hash: Option<&str>,
) -> Result<(), String> {
    let first = rows.first().ok_or("empty_batch")?;
    let _ = ledger;
    // Original metadata stays byte-identical; exclusive per-batch compiled marker below.
    let authorization = read_json(Path::new(text(first, "authorization_file")?))?;
    let directory = Path::new(text(&authorization["main_model"], "ledger_dir")?)
        .join("apps-holdout-collections")
        .join(text(first, "artifact_cohort_hash")?);
    std::fs::create_dir_all(&directory).map_err(|_| "collection_marker_directory")?;
    write_new(
        &directory.join(format!("{}.started", text(first, "manifest_sha256")?)),
        value_sha(&json!({"cohort":first["artifact_cohort_sha256"],"build":first["build_info"]}))
            .as_bytes(),
    )
}
