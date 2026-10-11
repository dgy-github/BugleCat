//! One-use development transport. Never qualifies a threshold or opens a holdout.
#[allow(dead_code)]
#[path = "verification_artifact_profile_formal/support.rs"]
mod frozen;
use frozen as support;
#[allow(dead_code)]
#[path = "verification_artifact_profile_formal/formal.rs"]
mod formal;
use async_trait::async_trait;
use frozen::{digest, durable, json_file, new_file, read, sha, text, write_new};
use ncx_provider::jev::{self, JevTransport};
use serde_json::{Value, json};
use std::{cell::Cell, path::Path, time::Duration};

const SOURCE: &[u8] = include_bytes!("verification_artifact_v2_development.rs");
const PROVIDER: &[u8] = include_bytes!("../../ncx-provider/src/jev.rs");
const INTAKE: &[u8] = include_bytes!("../../../../evals/verification/artifact_v2_development.py");
const BUILDER: &[u8] = include_bytes!("../../../../evals/verification/artifact_request_v2.py");
const FROZEN_SUPPORT: &[u8] = include_bytes!("verification_artifact_profile_formal/support.rs");
const FROZEN_FORMAL: &[u8] = include_bytes!("verification_artifact_profile_formal/formal.rs");
const DEV_COHORT: &[u8] =
    include_bytes!("../../../../evals/verification/artifact_v2_development_cohort.py");
const TEMPLATE: &str = "jev-artifact-assertion-specific-v2-development";

fn validate(path: &Path) -> Result<Value, String> {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (path, compiled) in [
        (
            source_root.join("examples/verification_artifact_v2_development.rs"),
            SOURCE,
        ),
        (source_root.join("../ncx-provider/src/jev.rs"), PROVIDER),
        (
            source_root.join("examples/verification_artifact_profile_formal/support.rs"),
            FROZEN_SUPPORT,
        ),
        (
            source_root.join("examples/verification_artifact_profile_formal/formal.rs"),
            FROZEN_FORMAL,
        ),
    ] {
        if sha(&read(&path, 128_000)?) != sha(compiled) {
            return Err("compiled_owner_changed".into());
        }
    }
    let row = json_file(path, 128_000)?;
    frozen::check_seal(&row, "artifact_hash")?;
    if row["schema"] != "buglecat.verification.artifact-v2-development-prepared.v1"
        || row["split"] != "development"
        || row["template_version"] != TEMPLATE
        || row["registered_for_formal_evaluation"] != false
        || row["production_authorized"] != false
        || digest(&row["request"]) != text(&row, "request_hash")?
        || row["request"].to_string().len() > jev::MAX_INPUT_BYTES
    {
        return Err("development_packet_contract".into());
    }
    let bindings = row["source_bindings"].as_array().ok_or("source_bindings")?;
    for (suffix, compiled) in [
        ("artifact_v2_development.py", INTAKE),
        ("artifact_request_v2.py", BUILDER),
    ] {
        let binding = bindings
            .iter()
            .find(|b| b["file"].as_str().is_some_and(|s| s.ends_with(suffix)))
            .ok_or("missing_compiled_python_owner")?;
        if binding["sha256"] != sha(compiled)
            || sha(&read(Path::new(text(binding, "file")?), 128_000)?) != sha(compiled)
        {
            return Err("compiled_python_owner_changed".into());
        }
    }
    if row["source_kind"] == "fresh_development_collection" {
        let binding = bindings
            .iter()
            .find(|b| {
                b["file"]
                    .as_str()
                    .is_some_and(|s| s.ends_with("artifact_v2_development_cohort.py"))
            })
            .ok_or("missing_compiled_cohort_owner")?;
        if binding["sha256"] != sha(DEV_COHORT)
            || sha(&read(Path::new(text(binding, "file")?), 128_000)?) != sha(DEV_COHORT)
        {
            return Err("compiled_cohort_owner_changed".into());
        }
    }
    let output = std::process::Command::new("python")
        .args([
            "-m",
            "evals.verification.artifact_v2_development",
            "validate",
            "--prepared",
        ])
        .arg(path)
        .env("NCX_JEV_API_KEY", "")
        .env("DEEPSEEK_API_KEY", "")
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "original_source_preflight_unavailable")?;
    let proof: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "original_source_preflight_failed")?;
    if !output.status.success()
        || proof["status"] != "development_source_verified"
        || proof["request_hash"] != row["request_hash"]
        || proof["new_paid_calls"] != 0
    {
        return Err("original_source_preflight_failed".into());
    }
    Ok(row)
}

fn response(raw: &[u8], body: &Value) -> Result<Value, String> {
    if raw.len() > jev::MAX_RESPONSE_BYTES {
        return Err("response_budget".into());
    }
    let value: Value = serde_json::from_slice(raw).map_err(|_| "invalid_json")?;
    if value["model"] != frozen::MODEL {
        return Err("actual_model_mismatch".into());
    }
    let expected = body["questions"].as_object().ok_or("questions")?;
    let answers = value["answers"].as_object().ok_or("answers")?;
    if answers.len() != expected.len() || answers.keys().any(|key| !expected.contains_key(key)) {
        return Err("exact_answers_required".into());
    }
    for answer in answers.values() {
        if answer["type"] != "noul"
            || !answer["noul"]
                .as_f64()
                .is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p))
        {
            return Err("invalid_noul".into());
        }
    }
    if ["input_tokens", "output_tokens"]
        .iter()
        .any(|key| value["usage"][key].as_u64().is_none())
    {
        return Err("invalid_usage".into());
    }
    Ok(value)
}

async fn attempt(
    row: &Value,
    prepared_sha: &str,
    dir: &Path,
    marker: &Path,
    transport: &dyn JevTransport,
    key: &str,
    cancel: Option<&Path>,
    timeout: Duration,
    offline: bool,
    reservation: Option<&Path>,
) -> Result<Value, String> {
    let body = &row["request"];
    let bytes = serde_json::to_vec(body).map_err(|_| "request_encoding")?;
    if sha(&bytes) != row["request_hash"] || bytes.len() > jev::MAX_INPUT_BYTES {
        return Err("request_byte_mismatch".into());
    }
    std::fs::create_dir(dir).map_err(|_| "new_attempt_directory_required")?;
    let directory = dir
        .canonicalize()
        .map_err(|_| "attempt_directory_identity")?;
    let exe = std::env::current_exe().map_err(|_| "exe_path")?;
    let mut event = json!({"status":"attempt_started", "id":row["id"], "group_id":row["group_id"],
        "template_version":TEMPLATE,"request_hash":row["request_hash"],"prepared_sha256":prepared_sha,
        "runner_exe_sha256":sha(&std::fs::read(exe).map_err(|_| "exe_read")?),
        "runner_source_sha256":sha(SOURCE),"provider_source_sha256":sha(PROVIDER),
        "frozen_support_sha256":sha(FROZEN_SUPPORT),"frozen_formal_sha256":sha(FROZEN_FORMAL),
        "intake_source_sha256":sha(INTAKE),"builder_source_sha256":sha(BUILDER),
        "development_cohort_source_sha256":sha(DEV_COHORT),
        "attempt_directory":directory,"raw_response_file":directory.join("raw-response.json"),
        "automatic_retries":0,"formal_certification":false,"offline":offline});
    if let Some(reservation) = reservation {
        event["reservation_file"] = json!(
            reservation
                .canonicalize()
                .map_err(|_| "reservation_identity")?
        );
        event["reservation_sha256"] = json!(sha(&read(reservation, 128_000)?));
    }
    write_new(marker, event.to_string().as_bytes())?;
    write_new(&dir.join("request.json"), &bytes)?;
    let mut events = new_file(&dir.join("events.jsonl"))?;
    durable(&mut events, &event)?;
    let mut submitted = false;
    let outcome = if cancel.is_some_and(Path::exists) {
        Err("cancelled".into())
    } else if !offline && key.trim().is_empty() {
        Err("missing_key".into())
    } else {
        event["status"] = json!("request_submitted");
        event["actual_request_hash"] = row["request_hash"].clone();
        durable(&mut events, &event)?;
        submitted = true;
        let call =
            tokio::time::timeout(timeout.min(jev::TIMEOUT), transport.post(key, body.clone()));
        tokio::pin!(call);
        loop {
            tokio::select! {
                result = &mut call => break result.map_err(|_| "timeout".to_string()).and_then(|r| r),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if cancel.is_some_and(Path::exists) { break Err("cancelled".into()); }
                }
            }
        }
    };
    event["status"] = json!("attempt_finished");
    event["submitted"] = json!(submitted);
    event["response_sha256"] = Value::Null;
    let status = match outcome {
        Ok((status, raw))
            if (200..300).contains(&status) && raw.len() <= jev::MAX_RESPONSE_BYTES =>
        {
            write_new(&dir.join("raw-response.json"), &raw)?;
            event["response_sha256"] = json!(sha(&raw));
            response(&raw, body)
                .map(|_| "ok".to_owned())
                .unwrap_or_else(|e| e)
        }
        Ok((status, _)) if !(200..300).contains(&status) => format!("http_{status}"),
        Ok(_) => "response_budget".into(),
        Err(error) => {
            if [
                "timeout",
                "cancelled",
                "missing_key",
                "response_budget",
                "client_error",
            ]
            .contains(&error.as_str())
            {
                error
            } else {
                "transport_error".into()
            }
        }
    };
    event["call_status"] = json!(status);
    durable(&mut events, &event)?;
    write_new(&dir.join("receipt.json"), event.to_string().as_bytes())?;
    Ok(event)
}

async fn live(path: &Path, reservation: &Path, dir: &Path, cancel: &Path) -> Result<Value, String> {
    let row = validate(path)?;
    let key = std::env::var("NCX_JEV_API_KEY").unwrap_or_default();
    let auth = json_file(Path::new(text(&row, "authorization_file")?), 128_000)?;
    let output = std::process::Command::new("python")
        .args([
            "-m",
            "evals.verification.artifact_v2_development",
            "validate-reservation",
            "--prepared",
        ])
        .arg(path)
        .arg("--reservation")
        .arg(reservation)
        .env("NCX_JEV_API_KEY", "")
        .env("DEEPSEEK_API_KEY", "")
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "reservation_preflight_unavailable")?;
    let proof: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "reservation_preflight_failed")?;
    if !output.status.success()
        || proof["status"] != "development_reservation_verified"
        || proof["request_hash"] != row["request_hash"]
        || proof["reservation_sha256"] != sha(&read(reservation, 128_000)?)
    {
        return Err("reservation_preflight_failed".into());
    }
    let marker = Path::new(text(&proof, "marker_file")?);
    formal::output_within_ledger(dir, &auth)?;
    attempt(
        &row,
        &sha(&read(path, 128_000)?),
        dir,
        marker,
        &jev::HttpTransport,
        &key,
        Some(cancel),
        jev::TIMEOUT,
        false,
        Some(reservation),
    )
    .await
}

struct Fake {
    status: u16,
    raw: Vec<u8>,
    delay: Duration,
    calls: Cell<u64>,
    cancel: Option<std::path::PathBuf>,
}
#[async_trait(?Send)]
impl JevTransport for Fake {
    async fn post(&self, _: &str, _: Value) -> Result<(u16, Vec<u8>), String> {
        self.calls.set(self.calls.get() + 1);
        if let Some(cancel) = &self.cancel {
            std::fs::write(cancel, b"cancel").map_err(|_| "fake_cancel_write")?;
        }
        tokio::time::sleep(self.delay).await;
        Ok((self.status, self.raw.clone()))
    }
}
async fn offline(dir: &Path, scenario: &str) -> Result<Value, String> {
    std::fs::create_dir(dir).map_err(|_| "new_offline_directory_required")?;
    let body = json!({"model":"jev-latest","state":{"fixture":"offline only"},
        "questions":{"setup_violation":{"type":"noul"},"assertion_0_violation":{"type":"noul"}}});
    let row = json!({"id":"offline-v2","group_id":"offline-fixture-never-formal","request_hash":digest(&body),"request":body});
    let mut raw = json!({"model":frozen::MODEL,"answers":{"setup_violation":{"type":"noul","noul":0.1},
        "assertion_0_violation":{"type":"noul","noul":0.2}},"usage":{"input_tokens":1,"output_tokens":2}});
    let cancel = dir.join("cancel");
    let mut delay = Duration::ZERO;
    let mut status = 200;
    let mut cancel_during = None;
    match scenario {
        "success" => {}
        "missing_answer" => {
            raw["answers"]
                .as_object_mut()
                .unwrap()
                .remove("assertion_0_violation");
        }
        "model_mismatch" => raw["model"] = json!("other"),
        "extra_answer" => raw["answers"]["foreign"] = json!({"type":"noul","noul":0.2}),
        "invalid_usage" => raw["usage"]["output_tokens"] = json!(-1),
        "boolean_probability" => raw["answers"]["setup_violation"]["noul"] = json!(true),
        "http403" => status = 403,
        "timeout" => delay = Duration::from_secs(1),
        "cancel_before" => write_new(&cancel, b"cancel")?,
        "cancel_during" => {
            delay = Duration::from_secs(1);
            cancel_during = Some(cancel.clone());
        }
        _ => return Err("unknown_offline_scenario".into()),
    }
    let fake = Fake {
        status,
        raw: raw.to_string().into_bytes(),
        delay,
        calls: Cell::new(0),
        cancel: cancel_during,
    };
    let marker = dir.join("fixture.started");
    let result = attempt(
        &row,
        "offline-only",
        &dir.join("attempt"),
        &marker,
        &fake,
        "",
        Some(&cancel),
        Duration::from_millis(100),
        true,
        None,
    )
    .await?;
    let before = fake.calls.get();
    if attempt(
        &row,
        "offline-only",
        &dir.join("duplicate"),
        &marker,
        &fake,
        "",
        None,
        Duration::from_millis(100),
        true,
        None,
    )
    .await
    .is_ok()
        || fake.calls.get() != before
    {
        return Err("duplicate_retry_not_blocked".into());
    }
    Ok(
        json!({"status":"offline_completed","scenario":scenario,"call_status":result["call_status"],
        "fake_transport_calls":before,"duplicate_reservation_blocked":true,"new_paid_calls":0,"network_calls":0,"formal_certification":false}),
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [a, path] if a == "validate" => validate(Path::new(path)).map(|r| json!({"status":"development_source_verified","id":r["id"],"new_paid_calls":0,"formal_certification":false})),
        [a, out, scenario] if a == "offline-transport-test" => offline(Path::new(out), scenario).await,
        [a, packet, reservation, dir, cancel, flag] if a == "live" && flag == "--live" => live(Path::new(packet), Path::new(reservation), Path::new(dir), Path::new(cancel)).await,
        _ => Err("usage: validate <packet> | offline-transport-test <newdir> <scenario> | live <packet> <reservation> <newdir> <cancel-file> --live".into()),
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}
