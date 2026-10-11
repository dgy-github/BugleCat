//! New mixed-source proof, retaining frozen ledger and provider implementations.
#[allow(dead_code, unused_imports)]
#[path = "../verification_artifact_v2_collect/support.rs"]
mod development;
pub use development::{
    check_resource_identity, check_seal, durable, new_file, original_auth, read_json,
    reject_reserved, seal, sha, text, user_prompt, value_sha, write_new, Inputs, Ledger, SYSTEM,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
const COHORT_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_cohort.py");
const INTAKE_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_intake.py");
const BUDGET_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/budget_authorization_v2.py");
const HOLDOUT_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_holdout.py");

pub fn profile(value: &Value) -> Result<Value, String> {
    development::profile(value)
}
pub fn profile_path(path: &Path) -> Result<Value, String> {
    development::profile_path(path)
}

pub fn validate(manifest: &Value) -> Result<Inputs, String> {
    if manifest["schema"] != "buglecat.verification.artifact-mixed-collection.formal.v2"
        || ![json!("calibration"), json!("holdout")].contains(&manifest["split"])
        || manifest["formal_registration"] != true
        || manifest["cases"]
            .as_array()
            .is_none_or(|rows| rows.is_empty() || rows.len() > 8)
    {
        return Err("formal_calibration_only_contract".into());
    }
    let cohort = read_json(Path::new(text(manifest, "artifact_cohort_file")?))?;
    if cohort["schema"] != "buglecat.verification.artifact-mixed-cohort.formal.v2"
        || cohort["holdout_opened"] != false
    {
        return Err("formal_v2_cohort_required".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../evals/verification");
    for (name, embedded) in [
        ("artifact_v2_formal_cohort.py", COHORT_OWNER),
        ("artifact_v2_formal_intake.py", INTAKE_OWNER),
        ("budget_authorization_v2.py", BUDGET_OWNER),
        ("artifact_v2_formal_holdout.py", HOLDOUT_OWNER),
    ] {
        if sha(&std::fs::read(root.join(name)).map_err(|_| "formal_owner_read")?) != sha(embedded) {
            return Err("compiled_formal_owner_changed".into());
        }
    }
    let proof_path = Path::new(text(manifest, "_actual_manifest_path")?);
    let result = std::process::Command::new("python")
        .args([
            "-m",
            "evals.verification.artifact_v2_formal_cohort",
            "validate-manifest",
            "--manifest",
        ])
        .arg(proof_path)
        .env("DEEPSEEK_API_KEY", "")
        .env("NCX_JEV_API_KEY", "")
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "formal_proof_unavailable")?;
    let proof: Value =
        serde_json::from_slice(&result.stdout).map_err(|_| "formal_proof_rejected")?;
    if !result.status.success()
        || proof["status"] != "formal_mixed_source_verified"
        || proof["new_paid_calls"] != 0
        || proof["hash"] != manifest["artifact_hash"]
        || proof["holdout_opened"] != (manifest["split"] == "holdout")
    {
        return Err("formal_proof_rejected".into());
    }
    profile(manifest)?;
    Ok(Inputs {
        source: read_json(Path::new(text(manifest, "source_file")?))?,
        catalog: read_json(Path::new(text(manifest, "catalog_file")?))?,
        authorization: read_json(Path::new(text(manifest, "authorization_file")?))?,
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

pub fn ledger_baseline(cohort: &Value) -> Value {
    json!({"ledger_snapshot":cohort["history_baseline"],"cases":cohort["cases"]})
}

pub fn bind_collection(
    ledger: &Ledger,
    rows: &[Value],
    _resource: Option<&Value>,
    _hash: Option<&str>,
) -> Result<(), String> {
    let first = rows.first().ok_or("empty_batch")?;
    ledger.bind(&format!("artifact_v2_formal_collection:{}",text(first,"artifact_cohort_sha256")?),
        &value_sha(&json!({"cohort":first["artifact_cohort_sha256"],"settings":first["settings"],
            "build":first["build_info"],"endpoint":first["endpoint_sha256"],"system":sha(SYSTEM.as_bytes())})))
}
