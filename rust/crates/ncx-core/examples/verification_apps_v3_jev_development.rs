//! One-use development transport. Never qualifies a threshold or opens a holdout.
#[allow(dead_code)]
#[path = "verification_artifact_profile_formal/support.rs"]
mod frozen;
use frozen as support;
#[allow(dead_code)]
#[path = "verification_artifact_profile_formal/formal.rs"]
mod formal;
use async_trait::async_trait;
use frozen::{durable, json_file, new_file, read, sha, text, write_new};
use futures_util::StreamExt;
use ncx_provider::jev::{self, JevTransport};
use serde_json::{Value, json};
use std::{cell::Cell, path::Path, time::Duration};

const SOURCE: &[u8] = include_bytes!("verification_apps_v3_jev_development.rs");
const PROVIDER: &[u8] = include_bytes!("../../ncx-provider/src/jev.rs");
const INTAKE: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/development_jev.py");
const BUILDER: &[u8] = include_bytes!("../../../../evals/verification/apps_campaign_v3/packet.py");
const FROZEN_SUPPORT: &[u8] = include_bytes!("verification_artifact_profile_formal/support.rs");
const FROZEN_FORMAL: &[u8] = include_bytes!("verification_artifact_profile_formal/formal.rs");
const DEV_COHORT: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/development.py");
const DEV_INTAKE: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/development_intake.py");
const CANDIDATE_INTAKE: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/intake.py");
const RESOURCE_OWNER: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/resources.py");
const PROTOCOL_OWNER: &[u8] =
    include_bytes!("../../../../evals/verification/apps_campaign_v3/protocol.py");
const TEMPLATE: &str = "jev-apps-ordered-pairs-v3";
const MAX_PREPARED: usize = 512_000;

fn owner_hashes() -> Value {
    json!({"runner_source_sha256":sha(SOURCE),"provider_source_sha256":sha(PROVIDER),
        "frozen_support_sha256":sha(FROZEN_SUPPORT),"frozen_formal_sha256":sha(FROZEN_FORMAL),
        "intake_source_sha256":sha(INTAKE),"builder_source_sha256":sha(BUILDER),
        "development_cohort_source_sha256":sha(DEV_COHORT),"development_intake_source_sha256":sha(DEV_INTAKE),
        "candidate_intake_source_sha256":sha(CANDIDATE_INTAKE),"resource_owner_source_sha256":sha(RESOURCE_OWNER),
        "protocol_owner_source_sha256":sha(PROTOCOL_OWNER)})
}

fn command() -> std::process::Command {
    let mut command = std::process::Command::new("python");
    command.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."));
    for (key, _) in std::env::vars_os() {
        if ["KEY", "TOKEN", "SECRET"]
            .iter()
            .any(|s| key.to_string_lossy().to_ascii_uppercase().contains(s))
        {
            command.env_remove(key);
        }
    }
    command
}

fn validate(path: &Path) -> Result<Value, String> {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (path, compiled) in [
        (
            source_root.join("examples/verification_apps_v3_jev_development.rs"),
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
    let row = json_file(path, MAX_PREPARED)?;
    // Python authenticates its original canonical seal. Rust numeric spelling
    // is not identical for exponent IO, so never reserialize that request.
    if row["schema"] != "buglecat.verification.apps-development-jev-prepared.v3"
        || row["split"] != "development"
        || row["template_version"] != TEMPLATE
        || row["registered_for_formal_evaluation"] != false
        || row["production_authorized"] != false
        || row["holdout_opened"] != false
        || row["formal_certification"] != false
        || row["certified_threshold"] != Value::Null
        || sha(text(&row, "request_json")?.as_bytes()) != text(&row, "request_hash")?
        || text(&row, "request_json")?.len() > jev::MAX_INPUT_BYTES
        || serde_json::from_str::<Value>(text(&row, "request_json")?).map_err(|_| "request_json")?
            != row["request"]
    {
        return Err("development_packet_contract".into());
    }
    let bindings = row["source_bindings"].as_array().ok_or("source_bindings")?;
    for (suffix, compiled) in [
        ("development_jev.py", INTAKE),
        ("packet.py", BUILDER),
        ("development.py", DEV_COHORT),
        ("development_intake.py", DEV_INTAKE),
        ("intake.py", CANDIDATE_INTAKE),
        ("resources.py", RESOURCE_OWNER),
        ("protocol.py", PROTOCOL_OWNER),
    ] {
        let binding = bindings
            .iter()
            .find(|b| {
                b["file"]
                    .as_str()
                    .is_some_and(|s| Path::new(s).file_name().is_some_and(|name| name == suffix))
            })
            .ok_or("missing_compiled_python_owner")?;
        if binding["sha256"] != sha(compiled)
            || sha(&read(Path::new(text(binding, "file")?), 128_000)?) != sha(compiled)
            || sha(&read(
                &source_root
                    .join("../../../evals/verification/apps_campaign_v3")
                    .join(suffix),
                128_000,
            )?) != sha(compiled)
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
                    .is_some_and(|s| s.ends_with("development.py"))
            })
            .ok_or("missing_compiled_cohort_owner")?;
        if binding["sha256"] != sha(DEV_COHORT)
            || sha(&read(Path::new(text(binding, "file")?), 128_000)?) != sha(DEV_COHORT)
        {
            return Err("compiled_cohort_owner_changed".into());
        }
    }
    let output = command()
        .args([
            "-m",
            "evals.verification.apps_campaign_v3.development_jev",
            "validate",
            "--prepared",
        ])
        .arg(path)
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "original_source_preflight_unavailable")?;
    let proof: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "original_source_preflight_failed")?;
    if !output.status.success()
        || proof["status"] != "apps_development_source_verified"
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

// Keep the original transport/reservation boundary explicit in this example;
// neither fixture mode nor caller cancellation can implicitly choose a ledger.
#[allow(clippy::too_many_arguments)]
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
    let bytes = text(row, "request_json")?.as_bytes().to_vec();
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
        "development_intake_source_sha256":sha(DEV_INTAKE),"candidate_intake_source_sha256":sha(CANDIDATE_INTAKE),
        "resource_owner_source_sha256":sha(RESOURCE_OWNER),"protocol_owner_source_sha256":sha(PROTOCOL_OWNER),
        "allocation_hash":row["allocation_hash"],"allocation_sha256":row["allocation_sha256"],
        "artifact_cohort_hash":row["artifact_cohort_hash"],"protocol_sha256":row["protocol_sha256"],"source_kind":row["source_kind"],
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
        event["actual_request_hash"] = json!(sha(&bytes));
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
    let prepared_sha = sha(&read(path, MAX_PREPARED)?);
    let row = validate(path)?;
    let key = std::env::var("NCX_JEV_API_KEY").unwrap_or_default();
    let output = command()
        .args([
            "-m",
            "evals.verification.apps_campaign_v3.development_jev",
            "validate-reservation",
            "--prepared",
        ])
        .arg(path)
        .arg("--reservation")
        .arg(reservation)
        .env("PYTHONWARNINGS", "ignore::SyntaxWarning")
        .output()
        .map_err(|_| "reservation_preflight_unavailable")?;
    let proof: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "reservation_preflight_failed")?;
    if !output.status.success()
        || proof["status"] != "apps_development_reservation_verified"
        || proof["request_hash"] != row["request_hash"]
        || proof["reservation_sha256"] != sha(&read(reservation, 128_000)?)
        || prepared_sha != sha(&read(path, MAX_PREPARED)?)
    {
        return Err("reservation_preflight_failed".into());
    }
    let marker = Path::new(text(&proof, "marker_file")?);
    let root = Path::new(text(&proof, "ledger_root")?)
        .join("attempts")
        .canonicalize()
        .map_err(|_| "attempt_root")?;
    let parent = dir
        .parent()
        .ok_or("output_parent")?
        .canonicalize()
        .map_err(|_| "output_parent")?;
    if !parent.starts_with(&root) || dir.exists() {
        return Err("output_outside_shared_attempts_or_exists".into());
    }
    let transport = ExactHttp {
        bytes: text(&row, "request_json")?.as_bytes().to_vec(),
    };
    attempt(
        &row,
        &prepared_sha,
        dir,
        marker,
        &transport,
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
// The original Python canonical bytes are the request. Value equality is an
// auxiliary check only: serde's exponent/float spellings are not Python's.
struct ExactHttp {
    bytes: Vec<u8>,
}
#[async_trait(?Send)]
impl JevTransport for ExactHttp {
    async fn post(&self, key: &str, body: Value) -> Result<(u16, Vec<u8>), String> {
        if self.bytes.len() > jev::MAX_INPUT_BYTES
            || serde_json::from_slice::<Value>(&self.bytes).map_err(|_| "request_json")? != body
        {
            return Err("request_byte_mismatch".into());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(jev::TIMEOUT)
            .build()
            .map_err(|_| "client_error")?;
        let response = client
            .post("https://api.typesafe.ai/v1/systemone")
            .bearer_auth(key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(self.bytes.clone())
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "timeout"
                } else {
                    "transport_error"
                }
            })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Ok((status, Vec::new()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > jev::MAX_RESPONSE_BYTES as u64)
        {
            return Err("response_budget".into());
        }
        let mut stream = response.bytes_stream();
        let mut raw = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "transport_error")?;
            if raw.len() + chunk.len() > jev::MAX_RESPONSE_BYTES {
                return Err("response_budget".into());
            }
            raw.extend_from_slice(&chunk);
        }
        Ok((status, raw))
    }
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
    // This is Python's sorted, ensure_ascii=False canonical representation.
    let original = r#"{"model":"jev-latest","questions":{"acceptance_violation":{"type":"noul"}},"state":{"escaped":"line\n\"quoted\"\\end","fixture":"offline only","numbers":[1e-07,1.0,1],"unicode":"汉字"}}"#;
    let body: Value = serde_json::from_str(original).map_err(|_| "fixture_json")?;
    let row = json!({"id":"offline-apps","group_id":"offline-fixture-never-formal","request_hash":sha(original.as_bytes()),"request":body,"request_json":original});
    let mut raw = json!({"model":frozen::MODEL,"answers":{"acceptance_violation":{"type":"noul","noul":0.1}},"usage":{"input_tokens":1,"output_tokens":2}});
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
                .remove("acceptance_violation");
        }
        "model_mismatch" => raw["model"] = json!("other"),
        "extra_answer" => raw["answers"]["foreign"] = json!({"type":"noul","noul":0.2}),
        "invalid_usage" => raw["usage"]["output_tokens"] = json!(-1),
        "boolean_probability" => raw["answers"]["acceptance_violation"]["noul"] = json!(true),
        "missing_usage" => {
            raw.as_object_mut().unwrap().remove("usage");
        }
        "malformed" => {}
        "nan_extra" | "infinity_extra" | "overflow_extra" | "deep_extra" => {}
        "oversize" => {}
        "missing_key" => {}
        "http401" => status = 401,
        "http403" => status = 403,
        "http429" => status = 429,
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
        raw: match scenario {
            "malformed" => b"not json".to_vec(),
            "nan_extra" | "infinity_extra" | "overflow_extra" | "deep_extra" => {
                let serialized = raw.to_string();
                let extra = match scenario {
                    "nan_extra" => "NaN".to_owned(),
                    "infinity_extra" => "Infinity".to_owned(),
                    "overflow_extra" => "1e400".to_owned(),
                    _ => format!("{}0{}", "[".repeat(130), "]".repeat(130)),
                };
                format!(
                    "{},\"extra\":{extra}}}",
                    &serialized[..serialized.len() - 1]
                )
                .into_bytes()
            }
            "oversize" => vec![b'x'; jev::MAX_RESPONSE_BYTES + 1],
            _ => raw.to_string().into_bytes(),
        },
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
        scenario != "missing_key",
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
        "fake_transport_calls":before,"duplicate_reservation_blocked":true,"request_hash":sha(original.as_bytes()),
        "exact_request_bytes_preserved":std::fs::read(dir.join("attempt/request.json")).map_err(|_| "fixture_request")? == original.as_bytes(),
        "new_paid_calls":0,"network_calls":0,"formal_certification":false}),
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [a] if a == "build-info" => std::env::current_exe().map_err(|_| "exe_path".to_owned()).and_then(|p| std::fs::read(p).map_err(|_| "exe_read".to_owned())).map(|bytes| { let mut info=owner_hashes(); info["runner_exe_sha256"]=json!(sha(&bytes)); info["new_paid_calls"]=json!(0); info["network_calls"]=json!(0); info["formal_certification"]=json!(false); info["template_version"]=json!(TEMPLATE); info }),
        [a, path] if a == "validate" => validate(Path::new(path)).map(|r| json!({"status":"apps_development_source_verified","id":r["id"],"new_paid_calls":0,"formal_certification":false})),
        [a, out, scenario] if a == "offline-transport-test" => offline(Path::new(out), scenario).await,
        [a, packet, reservation, dir, cancel, flag] if a == "live" && flag == "--live" => live(Path::new(packet), Path::new(reservation), Path::new(dir), Path::new(cancel)).await,
        _ => Err("usage: build-info | validate <packet> | offline-transport-test <newdir> <scenario> | live <packet> <reservation> <newdir> <cancel-file> --live".into()),
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}
