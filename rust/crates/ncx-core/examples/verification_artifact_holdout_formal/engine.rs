//! Separate formal source-registered artifact acceptance sidecar. Never changes native claims.
use crate::formal;
use crate::support::*;
use async_trait::async_trait;
use ncx_provider::jev::{self, JevTransport};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const RUNNER: &[u8] = crate::RUNNER_SOURCE;
const SUPPORT: &[u8] = crate::SUPPORT_SOURCE;
const FORMAL: &[u8] = crate::FORMAL_SOURCE;
const PROVIDER: &[u8] = include_bytes!("../../../ncx-provider/src/jev.rs");
const ENGINE: &[u8] = include_bytes!("engine.rs");
const COLLECTOR: &[u8] = include_bytes!("../verification_artifact_holdout_collect/support.rs");
const COLLECTOR_ENGINE: &[u8] =
    include_bytes!("../verification_artifact_profile_collect/engine.rs");
const COLLECTOR_MAIN: &[u8] = include_bytes!("../verification_artifact_profile_collect.rs");
const FROZEN_COLLECTOR: &[u8] = include_bytes!("../verification_artifact_collect/support.rs");
const LEGACY: &[u8] = include_bytes!("../verification_collect/support.rs");
fn base_build() -> Result<Value, String> {
    let path = std::env::current_exe().map_err(|_| "exe_path")?;
    Ok(
        json!({"runner_source_sha256":sha(RUNNER),"runner_support_source_sha256":sha(SUPPORT),
        "provider_transport_source_sha256":sha(PROVIDER),"formal_gate_source_sha256":sha(FORMAL),
        "artifact_transport_engine_source_sha256":sha(ENGINE),"collector_validator_source_sha256":sha(COLLECTOR),"profile_collection_engine_source_sha256":sha(COLLECTOR_ENGINE),"profile_collector_source_sha256":sha(COLLECTOR_MAIN),"frozen_collector_support_sha256":sha(FROZEN_COLLECTOR),"legacy_collector_support_sha256":sha(LEGACY),
        "runner_exe_sha256":sha(&std::fs::read(path).map_err(|_| "exe_read")?)}),
    )
}
fn validate(
    path: &Path,
    id: &str,
    registration_path: &Path,
    output: &Path,
) -> Result<Value, String> {
    let (row, input_sha) = select(path, id)?;
    validate_row(&row)?;
    formal::registration(registration_path, path, &row)?;
    let result = json!({"status":"validated_formal_artifact","id":id,"group_id":row["group_id"],
        "request_hash":row["request_hash"],"input_file_sha256":input_sha,"prepared_artifact_sha256":row["artifact_sha256"],
        "body_bytes":row["request"].to_string().len(),"build_info":build()?,"paid_calls":0,"network_calls":0,
        "formal_certification":false,"native_claim_certification":false});
    write_new(output, result.to_string().as_bytes())?;
    Ok(result)
}
async fn bounded(
    transport: &dyn JevTransport,
    key: &str,
    body: Value,
    limit: Duration,
    cancel: Option<&Path>,
) -> Result<(u16, Vec<u8>), String> {
    let call = tokio::time::timeout(limit.min(jev::TIMEOUT), transport.post(key, body));
    tokio::pin!(call);
    loop {
        tokio::select! {
            value=&mut call => return value.map_err(|_| "timeout".to_string())?.map_err(|e|
                if ["timeout","response_budget","client_error","transport_error"].contains(&e.as_str()) {e} else {"transport_error".into()}),
            _=tokio::time::sleep(Duration::from_millis(20)) => if cancel.is_some_and(Path::exists) {return Err("cancelled".into());}
        }
    }
}
struct Attempt<'a> {
    row: &'a Value,
    reservation: &'a Value,
    reservation_hash: &'a str,
    marker: &'a Path,
    directory: &'a Path,
    transport: &'a dyn JevTransport,
    key: &'a str,
    cancel: Option<&'a Path>,
    limit: Duration,
    offline: bool,
    registration: &'a Value,
    registration_hash: &'a str,
}
async fn attempt(input: Attempt<'_>) -> Result<Value, String> {
    let row = input.row;
    body_contract(row)?;
    let body = row["request"].clone();
    // HttpTransport::post uses reqwest.json, whose serde_json::to_vec encoding
    // is identical to these guarded bytes. Never call the native 3-question request().
    let bytes = serde_json::to_vec(&body).map_err(|_| "request_serialization")?;
    if sha(&bytes) != text(row, "request_hash")? {
        return Err("actual_request_hash_mismatch".into());
    }
    let provenance = build()?;
    std::fs::create_dir(input.directory).map_err(|_| "new_output_directory_required")?;
    let dir = input
        .directory
        .canonicalize()
        .map_err(|_| "output_identity")?;
    let response_file = dir.join("raw-response.json");
    let mut events = new_file(&dir.join("events.jsonl"))?;
    let mut started = json!({"schema":"buglecat.verification.artifact-attempt.v1","status":"attempt_started",
        "id":row["id"],"group_id":row["group_id"],"split":row["split"],"category":"artifact_acceptance",
        "main_resource_profile_file":row["main_resource_profile_file"],"main_resource_profile_sha256":row["main_resource_profile_sha256"],"main_resource_profile_hash":row["main_resource_profile_hash"],
        "artifact_cohort_file":row["artifact_cohort_file"],"artifact_cohort_sha256":row["artifact_cohort_sha256"],"artifact_cohort_hash":row["artifact_cohort_hash"],
        "formal_registration_hash":input.registration["artifact_hash"],"formal_registration_sha256":input.registration_hash,
        "policy_version":POLICY,"template_version":TEMPLATE,"native_policy_version":row["native_policy_version"],
        "request_hash":row["request_hash"],"request":body,"candidate_hash":row["candidate_sha256"],
        "code_sha256":row["code_sha256"],"input_hash":digest(&body["state"]),
        "prepared_artifact_sha256":row["artifact_sha256"],"source_sha256":row["source_sha256"],
        "main_receipt_sha256":row["main_receipt_sha256"],"oracle_sha256":row["oracle_sha256"],
        "protocol_sha256":row["protocol_sha256"],"authorization_sha256":row["authorization_sha256"],
        "reservation":input.reservation,"reservation_file_sha256":input.reservation_hash,
        "requested_model":"jev-latest","expected_actual_model":MODEL,"build_info":provenance,
        "runner_source_sha256":provenance["runner_source_sha256"],"runner_exe_sha256":provenance["runner_exe_sha256"],
        "offline":input.offline,"automatic_retries":0,"timeout_ms":input.limit.min(jev::TIMEOUT).as_millis(),
        "attempted_calls":1,"formal_certification":false,"native_claim_certification":false});
    // The non-.json marker does not change Python's reservation glob count.
    // Once claimed, even cancellation/crash consumes it permanently.
    write_new(input.marker, started.to_string().as_bytes())?;
    write_new(&dir.join("request.json"), &bytes)?;
    durable(&mut events, &started)?;
    let began = Instant::now();
    let mut submitted = false;
    let outcome = if input.cancel.is_some_and(Path::exists) {
        Err("cancelled".into())
    } else {
        started["status"] = json!("request_submitted");
        started["actual_request_hash"] = json!(sha(&bytes));
        started["planned_raw_response_file"] = json!(response_file);
        durable(&mut events, &started)?;
        submitted = true;
        bounded(input.transport, input.key, body, input.limit, input.cancel).await
    };
    let mut finished = started;
    finished["status"] = json!("attempt_finished");
    finished["submitted"] = json!(submitted);
    finished["elapsed_ms"] = json!(began.elapsed().as_millis());
    finished["response"] = Value::Null;
    finished["model"] = Value::Null;
    finished["usage"] = Value::Null;
    finished["response_sha256"] = Value::Null;
    finished["raw_response_file"] = Value::Null;
    match outcome {
        Ok((status, raw)) if (200..300).contains(&status) => {
            if raw.len() > jev::MAX_RESPONSE_BYTES {
                finished["call_status"] = json!("response_budget");
            } else {
                // Preserve successful transport bytes even when JSON/NOUL/model
                // validation fails; never replace a paid response with a retry.
                write_new(&response_file, &raw)?;
                finished["response_sha256"] = json!(sha(&raw));
                finished["raw_response_file"] = json!(response_file);
                match response(&raw, MODEL) {
                    Ok(value) => {
                        finished["call_status"] = json!("ok");
                        finished["model"] = value["model"].clone();
                        finished["usage"] = value["usage"].clone();
                        finished["response"] = value;
                    }
                    Err(error) => finished["call_status"] = json!(error),
                }
            }
        }
        Ok((status, _)) => finished["call_status"] = json!(format!("http_{status}")),
        Err(error) => finished["call_status"] = json!(error),
    }
    write_new(&dir.join("receipt.json"), finished.to_string().as_bytes())?;
    finished["receipt_file_sha256"] = json!(sha(&read(&dir.join("receipt.json"), 256_000)?));
    durable(&mut events, &finished)?;
    Ok(
        json!({"status":"artifact_attempt_finished","call_status":finished["call_status"],"attempted_calls":1,
        "directory":dir,"receipt":dir.join("receipt.json"),"offline":input.offline,
        "paid_calls":if input.offline {Some(0)}else{None},"paid_attempts":if input.offline {0}else{1},
        "network_calls":if input.offline||!submitted {0}else{1},"build_info":provenance}),
    )
}
async fn live(
    path: &Path,
    id: &str,
    registration_path: &Path,
    reservation_path: &Path,
    directory: &Path,
    cancel: Option<&Path>,
) -> Result<Value, String> {
    let (row, _) = select(path, id)?;
    let auth = validate_row(&row)?;
    let (registration, registration_hash) = formal::registration(registration_path, path, &row)?;
    let (reserved, marker, reservation_hash) = reservation(&row, &auth, reservation_path)?;
    formal::output_within_ledger(directory, &auth)?;
    let key = std::env::var("NCX_JEV_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let Some(key) = key else {
        std::fs::create_dir(directory).map_err(|_| "new_output_directory_required")?;
        let result = json!({"status":"not_run","reason":"missing_key","attempted_calls":0,"paid_calls":0,
            "network_calls":0,"reservation_consumed_by_python":true,"started_marker_created":false});
        write_new(
            &directory.join("not-run.json"),
            result.to_string().as_bytes(),
        )?;
        return Ok(result);
    };
    // Recheck frozen source files immediately before claiming a request.
    validate_row(&row)?;
    formal::registration(registration_path, path, &row)?;
    formal::output_within_ledger(directory, &auth)?;
    attempt(Attempt {
        row: &row,
        reservation: &reserved,
        reservation_hash: &reservation_hash,
        marker: &marker,
        directory,
        transport: &jev::HttpTransport,
        key: &key,
        cancel,
        limit: jev::TIMEOUT,
        offline: false,
        registration: &registration,
        registration_hash: &registration_hash,
    })
    .await
}

struct Fake {
    status: u16,
    raw: Vec<u8>,
    delay: Duration,
    calls: Cell<u64>,
    cancel: Option<PathBuf>,
}
#[async_trait(?Send)]
impl JevTransport for Fake {
    async fn post(&self, _: &str, _: Value) -> Result<(u16, Vec<u8>), String> {
        self.calls.set(self.calls.get() + 1);
        if let Some(path) = &self.cancel {
            std::fs::write(path, b"cancel").map_err(|_| "offline_cancel_write")?;
        }
        tokio::time::sleep(self.delay).await;
        Ok((self.status, self.raw.clone()))
    }
}
async fn offline(directory: &Path, scenario: &str) -> Result<Value, String> {
    std::fs::create_dir(directory).map_err(|_| "new_offline_root_required")?;
    let dir = directory.canonicalize().map_err(|_| "offline_identity")?;
    let body = json!({"model":"jev-latest","state":{"schema":"buglecat.jev.artifact-acceptance.v1",
        "statement_origin":"host_artifact_hypothesis","scope":"frozen_acceptance_assertions_only",
        "task_prompt":"Fixture 字节", "public_interface":["def f():"],"candidate_code":"def f():\n    return 1\n",
        "acceptance_assertions":["assert f() == 1"],"test_imports":[]},
        "questions":{"acceptance_violation":{"type":"noul","instructions":QUESTION}}});
    let row = json!({"id":"offline-artifact","group_id":"offline-fixture-never-formal","split":"development",
        "category":"artifact_acceptance","policy_version":POLICY,"template_version":TEMPLATE,
        "expected_actual_model":MODEL,"statement_origin":"host_artifact_hypothesis","request":body,"request_hash":digest(&body),
        "code_sha256":sha(body["state"]["candidate_code"].as_str().unwrap().as_bytes()),"candidate_sha256":"offline-fixture",
        "main_resource_profile_file":"offline-only", "main_resource_profile_sha256":"offline-only", "main_resource_profile_hash":"offline-only", "artifact_sha256":"offline-fixture","source_sha256":"offline-fixture","authorization_sha256":"offline-fixture"});
    let mut raw = json!({"model":MODEL,"answers":{"acceptance_violation":{"type":"noul","noul":0.2}},
        "usage":{"input_tokens":1,"output_tokens":2}});
    let cancel = dir.join("cancel");
    let mut fake = Fake {
        status: 200,
        raw: vec![],
        delay: Duration::ZERO,
        calls: Cell::new(0),
        cancel: None,
    };
    match scenario {
        "success" => {}
        "malformed" => raw = json!({}),
        "model_mismatch" => raw["model"] = json!("other"),
        "invalid_usage" => raw["usage"]["input_tokens"] = json!(-1),
        "extra_answers" => raw["answers"]["other"] = json!({"type":"noul","noul":0.1}),
        "http500" => fake.status = 500,
        "cancel_before" => write_new(&cancel, b"cancel")?,
        "cancel_during" => {
            fake.delay = Duration::from_secs(1);
            fake.cancel = Some(cancel.clone());
        }
        "timeout" => fake.delay = Duration::from_secs(1),
        _ => return Err("unknown_offline_scenario".into()),
    }
    fake.raw = if scenario == "malformed" {
        b"not JSON".to_vec()
    } else if scenario == "http500" {
        b"secret non-2xx fixture body".to_vec()
    } else {
        raw.to_string().into_bytes()
    };
    let marker = dir.join("fixture.started");
    let reserved = json!({"group_id":row["group_id"],"request_hash":row["request_hash"],"reservation_index":1,"offline":true});
    let offline_registration = json!({"artifact_hash":"offline-fixture-not-formal"});
    let call = || Attempt {
        row: &row,
        reservation: &reserved,
        reservation_hash: "offline-fixture",
        marker: &marker,
        directory: &dir,
        transport: &fake,
        key: "",
        cancel: Some(&cancel),
        limit: Duration::from_millis(100),
        offline: true,
        registration: &offline_registration,
        registration_hash: "offline-fixture",
    };
    let mut attempt_input = call();
    let run = dir.join("attempt");
    attempt_input.directory = &run;
    let mut result = attempt(attempt_input).await?;
    let retry = dir.join("duplicate-attempt");
    let mut retry_input = call();
    retry_input.directory = &retry;
    let calls = fake.calls.get();
    if attempt(retry_input).await.is_ok() || fake.calls.get() != calls {
        return Err("duplicate_reservation_was_reused".into());
    }
    result["fake_transport_calls"] = json!(calls);
    result["duplicate_reservation_blocked"] = json!(true);
    result["scenario"] = json!(scenario);
    write_new(&dir.join("summary.json"), result.to_string().as_bytes())?;
    Ok(result)
}
pub async fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result=match args.as_slice() {
        [action,path,id,registration] if action == "check-gate" => crate::guard::run(&["jev",path,id,registration]),
        [a,path,id] if a=="check-source"=>select(Path::new(path),id).and_then(|(row,_)|validate_row(&row).map(|_|json!({"status":"original_profile_source_verified","id":row["id"],"request_hash":row["request_hash"],"main_resource_profile_sha256":row["main_resource_profile_sha256"],"paid_calls":0,"network_calls":0,"formal_certification":false}))),
        [a,path,id,registration] if a=="check-registration"=>{
            match select(Path::new(path),id){
                Ok((row,_))=>formal::registration(Path::new(registration),Path::new(path),&row).map(|(r,_)|json!({"status":"formal_registration_verified","registration_hash":r["artifact_hash"],"paid_calls":0,"network_calls":0})),
                Err(error)=>Err(error),
            }
        },
        [a,path,id,registration,out] if a=="validate"=>validate(Path::new(path),id,Path::new(registration),Path::new(out)),
        [a,path,id,registration,res,out,flag] if a=="live"&&flag=="--live"=>live(Path::new(path),id,Path::new(registration),Path::new(res),Path::new(out),None).await,
        [a,path,id,registration,res,out,flag,cancel_flag,cancel] if a=="live"&&flag=="--live"&&cancel_flag=="--cancel-file"=>live(Path::new(path),id,Path::new(registration),Path::new(res),Path::new(out),Some(Path::new(cancel))).await,
        [a,out,scenario] if a=="offline-transport-test"=>offline(Path::new(out),scenario).await,
        _=>Err("usage: verification_artifact_holdout_formal validate <prepared-jsonl> <id> <registration.json> <new-output.json> | offline-transport-test <newdir> <scenario> | live <prepared-jsonl> <id> <registration.json> <reservation.json> <newdir> --live [--cancel-file path]".into()),
    };
    match result {
        Ok(v) => println!("{v}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn build() -> Result<Value, String> {
    let mut value = base_build()?;
    value
        .as_object_mut()
        .ok_or("build_object")?
        .extend(crate::guard::build_fields().as_object().unwrap().clone());
    value["holdout_collection_engine_source_sha256"] = json!(sha(include_bytes!(
        "../verification_artifact_holdout_collect/engine.rs"
    )));
    value["holdout_collector_source_sha256"] = json!(sha(include_bytes!(
        "../verification_artifact_holdout_collect.rs"
    )));
    Ok(value)
}
