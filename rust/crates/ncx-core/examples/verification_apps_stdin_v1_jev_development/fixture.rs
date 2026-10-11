//! Synthetic provenance, isolated output, never accepted by live validation.
use super::{engine, provenance, support::*};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn prepare(output: &Path) -> Result<(Value, PathBuf, PathBuf), String> {
    let parent = output
        .parent()
        .ok_or("fixture_parent")?
        .canonicalize()
        .map_err(|_| "fixture_parent")?;
    if !parent.starts_with(
        std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "temp_path")?,
    ) {
        return Err("fixture_requires_temp_root".into());
    }
    std::fs::create_dir(output).map_err(|_| "fresh_fixture_directory_required")?;
    let root = output.canonicalize().map_err(|_| "fixture_path")?;
    if !root.starts_with(
        std::env::temp_dir()
            .canonicalize()
            .map_err(|_| "temp_path")?,
    ) {
        return Err("fixture_requires_temp_root".into());
    }
    let attempt = root.join("attempt");
    std::fs::create_dir(&attempt).map_err(|_| "fixture_attempt")?;
    let body = r#"{"model":"jev-latest","questions":{"acceptance_violation":{"instructions":"synthetic fixture; no source authority","type":"noul"}},"state":{"code":"print('你好')\n","numbers":[1e-07,1.0,1],"schema":"synthetic-only"}}"#;
    let prepared = root.join("fixture-prepared.json");
    let reservation = root.join("fixture-reservation.json");
    let row = json!({"schema":"buglecat.verification.stdin-development-jev-fixture.v1","id":"synthetic-case","group_id":"synthetic-group",
        "request_json":body,"request":serde_json::from_str::<Value>(body).unwrap(),"request_hash":sha(body.as_bytes()),"sender_build_info":provenance::build()?,
        "endpoint_sha256":sha(serde_json::to_string(engine::ENDPOINT).unwrap().as_bytes()),"registration_file":"synthetic-only","registration_sha256":"0".repeat(64),
        "registration_hash":"0".repeat(64),"cohort_hash":"0".repeat(64),"candidate_code_sha256":"0".repeat(64),"oracle_sha256":"0".repeat(64),
        "output_directory":attempt,"template_version":engine::TEMPLATE,"development_only":true,"formal_registration":false,"holdout_opened":false,
        "production_authorized":false,"formal_certification":false,"certified_threshold":null});
    durable(&prepared, &row)?;
    durable(
        &reservation,
        &json!({"reservation_index":1,"group_id":"synthetic-group","request_hash":row["request_hash"],"synthetic_provenance":true}),
    )?;
    durable(
        &reservation.with_extension("started"),
        &json!({"request_hash":row["request_hash"],"prepared_sha256":sha(&read(&prepared,2_000_000)?),
        "reservation_sha256":sha(&read(&reservation,2_000_000)?),"output_directory":attempt,"synthetic_provenance":true}),
    )?;
    Ok((row, prepared, reservation))
}
