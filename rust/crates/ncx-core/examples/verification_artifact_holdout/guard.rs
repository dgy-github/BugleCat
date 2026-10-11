//! Original owner composition, checked before any holdout transport or reservation.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn owners() -> Vec<(&'static str, &'static str, &'static [u8])> {
    vec![
        (
            "source_binding_owner_source_sha256",
            "evals/verification/source_bindings.py",
            include_bytes!("../../../../../evals/verification/source_bindings.py"),
        ),
        (
            "holdout_transport_guard_source_sha256",
            "evals/verification/artifact_holdout_transport_guard.py",
            include_bytes!("../../../../../evals/verification/artifact_holdout_transport_guard.py"),
        ),
        (
            "holdout_rust_guard_source_sha256",
            "rust/crates/ncx-core/examples/verification_artifact_holdout/guard.rs",
            include_bytes!("guard.rs"),
        ),
        (
            "holdout_gate_owner_source_sha256",
            "evals/verification/artifact_holdout.py",
            include_bytes!("../../../../../evals/verification/artifact_holdout.py"),
        ),
        (
            "formal_calibration_owner_source_sha256",
            "evals/verification/artifact_formal.py",
            include_bytes!("../../../../../evals/verification/artifact_formal.py"),
        ),
        (
            "formal_intake_owner_source_sha256",
            "evals/verification/artifact_formal_intake.py",
            include_bytes!("../../../../../evals/verification/artifact_formal_intake.py"),
        ),
        (
            "formal_observations_owner_source_sha256",
            "evals/verification/artifact_formal_observations.py",
            include_bytes!("../../../../../evals/verification/artifact_formal_observations.py"),
        ),
        (
            "artifact_numeric_gate_source_sha256",
            "evals/verification/artifact_gate.py",
            include_bytes!("../../../../../evals/verification/artifact_gate.py"),
        ),
        (
            "profile_collector_validator_source_sha256",
            "rust/crates/ncx-core/examples/verification_artifact_profile_collect/support.rs",
            include_bytes!("../verification_artifact_profile_collect/support.rs"),
        ),
    ]
}
pub fn build_fields() -> Value {
    let mut fields = json!({});
    for (field, _, bytes) in owners() {
        fields[field] = json!(sha(bytes));
    }
    fields
}
pub fn run(arguments: &[&str]) -> Result<Value, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("repo_identity")?;
    for (_, path, bytes) in owners() {
        if sha(&std::fs::read(root.join(path)).map_err(|_| "holdout_owner_read")?) != sha(bytes) {
            return Err("holdout_compiled_owner_changed".into());
        }
    }
    // File-backed stdout prevents a large child write from deadlocking a pipe.
    // No inherited API credentials are available to the proof subprocess.
    let dir = tempfile::tempdir().map_err(|_| "guard_temp")?;
    let path = dir.path().join("proof.json");
    let file = std::fs::File::create(&path).map_err(|_| "guard_stdout")?;
    let mut command = Command::new("python");
    command.env_clear().env("PYTHONUTF8", "1");
    for key in [
        "PATH",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .current_dir(root)
        .args(["-m", "evals.verification.artifact_holdout_transport_guard"])
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::from(file))
        .spawn()
        .map_err(|_| "holdout_guard_spawn")?;
    let started = Instant::now();
    loop {
        if std::fs::metadata(&path)
            .map_err(|_| "guard_stdout_metadata")?
            .len()
            > 128_000
            || started.elapsed() > Duration::from_secs(300)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("holdout_guard_budget".into());
        }
        if let Some(status) = child.try_wait().map_err(|_| "guard_wait")? {
            if !status.success() {
                return Err("qualified_original_holdout_gate_required".into());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|_| "guard_read")?)
        .map_err(|_| "guard_proof_json")?;
    let seal = value
        .as_object_mut()
        .ok_or("guard_object")?
        .remove("artifact_hash")
        .ok_or("guard_seal")?;
    if seal != sha(value.to_string().as_bytes())
        || value["schema"] != "buglecat.verification.artifact-holdout-transport-proof.v1"
        || value["split"] != "holdout"
        || value["qualified_original_calibration"] != true
        || value["threshold_retuning_allowed"] != false
        || value["production_authorized"] != false
        || value["http_calls"] != 0
        || value["paid_calls"] != 0
    {
        return Err("guard_proof_contract".into());
    }
    value["artifact_hash"] = seal;
    Ok(value)
}
