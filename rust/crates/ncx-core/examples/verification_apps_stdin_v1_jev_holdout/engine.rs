use super::{fixture, provenance, support::*};
use futures_util::StreamExt;
use ncx_provider::jev;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
pub const TEMPLATE: &str = "jev-stdin-script-ordered-pairs-v1-development";

pub fn parsed(raw: &[u8]) -> Result<Value, String> {
    let Strict(value) = serde_json::from_slice(raw).map_err(|_| "invalid_json")?;
    if value["model"] != "jev-1.13.0" {
        return Err("actual_model_mismatch".into());
    }
    let answers = value["answers"]
        .as_object()
        .ok_or("exact_answers_required")?;
    if answers.len() != 1 || !answers.contains_key("acceptance_violation") {
        return Err("exact_answers_required".into());
    }
    let answer = &answers["acceptance_violation"];
    if answer["type"] != "noul"
        || !answer["noul"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    {
        return Err("invalid_noul".into());
    }
    if ["input_tokens", "output_tokens"]
        .iter()
        .any(|key| value["usage"][key].as_u64().is_none())
    {
        return Err("invalid_usage".into());
    }
    Ok(value)
}

fn validate_local(row: &Value, genuine: bool) -> Result<(), String> {
    provenance::check()?;
    if row["schema"]
        != if genuine {
            "buglecat.verification.stdin-formal-holdout-jev-prepared.v1"
        } else {
            "buglecat.verification.stdin-formal-holdout-jev-fixture.v1"
        }
        || row["development_only"] != false
        || row["formal_registration"] != true
        || row["holdout_only"] != true
        || row["holdout_opened"] != true
        || row["production_authorized"] != false
        || row["formal_certification"] != false
        || !row["certified_threshold"].is_null()
        || row["template_version"] != TEMPLATE
        || row["sender_build_info"] != provenance::build()?
        || sha(text(row, "request_json")?.as_bytes()) != text(row, "request_hash")?
        || text(row, "request_json")?.len() > jev::MAX_INPUT_BYTES
        || serde_json::from_str::<Value>(text(row, "request_json")?).map_err(|_| "request_json")?
            != row["request"]
    {
        return Err("original_holdout_request_contract".into());
    }
    Ok(())
}

async fn post(key: &str, bytes: Vec<u8>) -> Result<(u16, Vec<u8>), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(jev::TIMEOUT)
        .build()
        .map_err(|_| "client_error")?;
    let response = client
        .post(ENDPOINT)
        .bearer_auth(key)
        .header("Content-Type", "application/json")
        .body(bytes)
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
    if response
        .content_length()
        .is_some_and(|n| n > jev::MAX_RESPONSE_BYTES as u64)
    {
        return Err("response_budget".into());
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "transport_error")?;
        if body.len() + chunk.len() > jev::MAX_RESPONSE_BYTES {
            return Err("response_budget".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}

pub fn identity(row: &Value, prepared: &Path, reservation: &Path) -> Result<Value, String> {
    let reserved = json(reservation)?;
    let mut result = json!({"prepared_file":prepared.canonicalize().map_err(|_| "prepared_path")?,"prepared_sha256":sha(&read(prepared,2_000_000)?),
        "reservation_file":reservation.canonicalize().map_err(|_| "reservation_path")?,"reservation_sha256":sha(&read(reservation,2_000_000)?),"reservation_index":reserved["reservation_index"],"build_info":provenance::build()?});
    for key in [
        "id",
        "group_id",
        "request_hash",
        "endpoint_sha256",
        "registration_file",
        "registration_sha256",
        "registration_hash",
        "cohort_hash",
        "candidate_code_sha256",
        "oracle_sha256",
        "holdout_opening_file",
        "holdout_opening_sha256",
        "holdout_opening_hash",
        "qualified_calibration_threshold",
    ] {
        result[key] = row[key].clone();
    }
    Ok(result)
}

pub async fn attempt(
    row: &Value,
    prepared: &Path,
    reservation: &Path,
    fake: Option<&str>,
    raw_override: Option<&Path>,
    cancel: bool,
) -> Result<Value, String> {
    let output = Path::new(text(row, "output_directory")?);
    let identity = identity(row, prepared, reservation)?;
    let events = output.join("events.jsonl");
    write_new(&events, b"")?;
    write_new(
        &output.join("request.json"),
        text(row, "request_json")?.as_bytes(),
    )?;
    let mut start = identity.clone();
    start["event"] = json!("attempt_started");
    event(&events, &start)?;
    let key = if fake.is_some() {
        Some("fixture-no-network".to_owned())
    } else {
        std::env::var("NCX_JEV_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
    };
    let mut http = None;
    let mut raw_file = Value::Null;
    let mut raw_sha = Value::Null;
    let mut response = Value::Null;
    let mut submitted_http = false;
    let status = if cancel {
        "cancelled".to_owned()
    } else if key.is_none() {
        "missing_key".to_owned()
    } else {
        let mut submitted = identity.clone();
        submitted["event"] = json!("request_submitted");
        submitted["actual_request_hash"] = json!(sha(text(row, "request_json")?.as_bytes()));
        event(&events, &submitted)?;
        submitted_http = true;
        let result = if let Some(kind) = fake {
            match kind {
                "timeout" => Err("timeout".to_owned()),
                "transport_error" => Err("transport_error".to_owned()),
                "response_budget" => Err("response_budget".to_owned()),
                "cancel_after_submit" => Err("cancelled".to_owned()),
                "http400" => Ok((
                    400,
                    b"{\"error\":\"fixture-private-provider-fault\"}".to_vec(),
                )),
                _ => Ok((
                    200,
                    if let Some(path) = raw_override {
                        read(path, jev::MAX_RESPONSE_BYTES)?
                    } else {
                        br#"{"model":"jev-1.13.0","answers":{"acceptance_violation":{"type":"noul","noul":0.1}},"usage":{"input_tokens":12,"output_tokens":3}}"#.to_vec()
                    },
                )),
            }
        } else {
            match tokio::time::timeout(
                jev::TIMEOUT,
                post(
                    key.as_deref().unwrap(),
                    text(row, "request_json")?.as_bytes().to_vec(),
                ),
            )
            .await
            {
                Ok(value) => value,
                Err(_) => Err("timeout".into()),
            }
        };
        match result {
            Err(kind) => kind,
            Ok((code, raw)) => {
                http = Some(code);
                let file = output.join("raw-response.json");
                write_new(&file, &raw)?;
                raw_file = json!(file);
                raw_sha = json!(sha(&raw));
                if !(200..300).contains(&code) {
                    format!("http_{code}")
                } else {
                    match parsed(&raw) {
                        Ok(value) => {
                            response = value;
                            "ok".into()
                        }
                        Err(kind) => kind,
                    }
                }
            }
        }
    };
    let mut finish = identity;
    finish["call_status"] = json!(status);
    finish["http_status"] = json!(http);
    finish["raw_response_file"] = raw_file;
    finish["response_sha256"] = raw_sha;
    finish["response"] = response;
    durable(&output.join("receipt.json"), &finish)?;
    finish["receipt_sha256"] = json!(sha(&read(&output.join("receipt.json"), 2_000_000)?));
    finish["event"] = json!("attempt_finished");
    event(&events, &finish)?;
    Ok(json!({"id":row["id"],"call_status":status,
        "new_paid_calls":if fake.is_none()&&submitted_http{1}else{0},
        "reserved_slots_consumed":1,"transport_submission_recorded":submitted_http,"verified_pass_signals":0}))
}

fn option(args: &[String], name: &str) -> Result<PathBuf, String> {
    args.iter()
        .position(|v| v == name)
        .and_then(|n| args.get(n + 1))
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing_{name}"))
}

pub async fn run(args: Vec<String>) -> Result<Value, String> {
    match args.first().map(String::as_str) {
        Some("build-info") => provenance::build(),
        Some("offline") => {
            let out = option(&args, "--output")?;
            let kind = args
                .iter()
                .position(|v| v == "--case")
                .and_then(|n| args.get(n + 1))
                .map_or("success", String::as_str);
            let (row, prepared, reserved) = fixture::prepare(&out)?;
            validate_local(&row, false)?;
            let override_file = option(&args, "--response-file").ok();
            attempt(
                &row,
                &prepared,
                &reserved,
                Some(kind),
                override_file.as_deref(),
                kind == "cancelled",
            )
            .await
        }
        Some("validate") => {
            let prepared = option(&args, "--prepared")?;
            let row = json(&prepared)?;
            validate_local(&row, true)?;
            let proof = command("validate", &prepared, None)?;
            if proof["status"] != "genuine_original_holdout_verified"
                || proof["request_hash"] != row["request_hash"]
            {
                return Err("original_proof_rejected".into());
            }
            Ok(json!({"status":"validated","new_paid_calls":0}))
        }
        Some("send") => {
            if !args.iter().any(|v| v == "--live") {
                return Err("explicit_live_required".into());
            }
            let prepared = option(&args, "--prepared")?;
            let reservation = option(&args, "--reservation")?;
            let row = json(&prepared)?;
            validate_local(&row, true)?;
            // claim is the sole fresh full proof in this action and preserves
            // an irreversible slot even when key/cancellation prevents HTTP.
            let claimed = command("claim", &prepared, Some(&reservation))?;
            if claimed["request_hash"] != row["request_hash"] {
                return Err("original_claim_rejected".into());
            }
            attempt(
                &row,
                &prepared,
                &reservation,
                None,
                None,
                args.iter().any(|v| v == "--cancel-before-submit"),
            )
            .await
        }
        _ => Err("usage_build-info_validate_offline_send".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_and_nonfinite_json_rejected() {
        for raw in [
            br#"{"x":1,"x":2}"#.as_slice(),
            br#"{"x":NaN}"#,
            br#"{"x":1e400}"#,
        ] {
            assert_eq!(parsed(raw).unwrap_err(), "invalid_json");
        }
    }
    #[test]
    fn exact_single_question_required() {
        let value =
            json!({"model":"jev-1.13.0","answers":{},"usage":{"input_tokens":1,"output_tokens":1}});
        assert_eq!(
            parsed(value.to_string().as_bytes()).unwrap_err(),
            "exact_answers_required"
        );
    }
    #[test]
    fn booleans_not_probabilities_and_unsigned_usage() {
        let mut value = json!({"model":"jev-1.13.0","answers":{"acceptance_violation":{"type":"noul","noul":true}},"usage":{"input_tokens":1,"output_tokens":1}});
        assert_eq!(
            parsed(value.to_string().as_bytes()).unwrap_err(),
            "invalid_noul"
        );
        value["answers"]["acceptance_violation"]["noul"] = json!(0.1);
        value["usage"]["input_tokens"] = json!(1.0);
        assert_eq!(
            parsed(value.to_string().as_bytes()).unwrap_err(),
            "invalid_usage"
        );
    }
    #[test]
    fn actual_model_is_pinned() {
        assert_eq!(
            parsed(br#"{"model":"jev-latest"}"#).unwrap_err(),
            "actual_model_mismatch"
        );
    }
    #[test]
    fn exclusive_bytes_never_overwrite() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("original");
        write_new(&file, b"original").unwrap();
        assert!(write_new(&file, b"changed").is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"original");
    }
}
