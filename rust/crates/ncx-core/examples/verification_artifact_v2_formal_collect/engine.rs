//! Explicit single-response collection, with a durable global request budget.
//! Default prepare and offline-receipt-test paths never construct an HTTP client.
#[path = "support.rs"]
mod support;
use async_trait::async_trait;
use futures_util::StreamExt;
use ncx_config::{load_config, Overrides};
use ncx_core::{context_store::ContextStore, context_trace::SourceKind, verification, Session};
use ncx_provider::{
    request::{build_body, to_request_json},
    response::parse_completion,
};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    fs::File,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
use support::*;

const RUNNER_SOURCE: &[u8] = include_bytes!("../verification_artifact_v2_formal_collect.rs");
const SUPPORT_SOURCE: &[u8] = include_bytes!("support.rs");
const REQUEST_SOURCE: &[u8] = include_bytes!("../../../ncx-provider/src/request.rs");
const RESPONSE_SOURCE: &[u8] = include_bytes!("../../../ncx-provider/src/response.rs");
const LEGACY_SUPPORT_SOURCE: &[u8] = include_bytes!("../verification_collect/support.rs");
const ENGINE_SOURCE: &[u8] = include_bytes!("engine.rs");
const FROZEN_SUPPORT_SOURCE: &[u8] = include_bytes!("../verification_artifact_collect/support.rs");
const PROFILE_SUPPORT_SOURCE: &[u8] =
    include_bytes!("../verification_artifact_profile_collect/support.rs");
const COHORT_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_cohort.py");
const INTAKE_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_intake.py");
const HOLDOUT_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/artifact_v2_formal_holdout.py");
const HUMAN_ADAPTER: &[u8] =
    include_bytes!("../../../../../evals/verification/humaneval_source.py");
const HUMAN_WORKER: &[u8] =
    include_bytes!("../../../../../evals/verification/humaneval_oracle_worker.py");
const MBPP_WORKER: &[u8] =
    include_bytes!("../../../../../evals/verification/benchmark_oracle_worker.py");
const FROZEN_DEV_SUPPORT: &[u8] = include_bytes!("../verification_artifact_v2_collect/support.rs");
const BUDGET_OWNER: &[u8] =
    include_bytes!("../../../../../evals/verification/budget_authorization_v2.py");
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

fn build_info() -> Result<Value, String> {
    let executable = std::env::current_exe().map_err(|_| "executable_path")?;
    Ok(
        json!({"runner_source_sha256":sha(RUNNER_SOURCE),"collection_engine_source_sha256":sha(ENGINE_SOURCE),"frozen_collector_support_sha256":sha(FROZEN_SUPPORT_SOURCE),"support_source_sha256":sha(SUPPORT_SOURCE),
        "request_builder_source_sha256":sha(REQUEST_SOURCE),"response_parser_source_sha256":sha(RESPONSE_SOURCE),"legacy_support_source_sha256":sha(LEGACY_SUPPORT_SOURCE),
        "executable_sha256":sha(&std::fs::read(executable).map_err(|_| "executable_read")?),
        "profile_collector_validator_source_sha256":sha(PROFILE_SUPPORT_SOURCE),"formal_cohort_owner_source_sha256":sha(COHORT_OWNER),"formal_intake_owner_source_sha256":sha(INTAKE_OWNER),"holdout_gate_owner_source_sha256":sha(HOLDOUT_OWNER),"human_adapter_source_sha256":sha(HUMAN_ADAPTER),"human_worker_source_sha256":sha(HUMAN_WORKER),"mbpp_worker_source_sha256":sha(MBPP_WORKER),"frozen_development_support_sha256":sha(FROZEN_DEV_SUPPORT),"budget_v2_owner_source_sha256":sha(BUDGET_OWNER),"policy_version":verification::types::POLICY_VERSION}),
    )
}

fn settings(manifest: &Value) -> Result<Value, String> {
    Ok(profile(manifest)?["main_collection_settings"].clone())
}

fn endpoint(base: &str) -> Result<String, String> {
    let url = url::Url::parse(base).map_err(|_| "endpoint_invalid")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !["https", "http"].contains(&url.scheme())
    {
        return Err("endpoint_unsafe".into());
    }
    Ok(format!("{}/chat/completions", base.trim_end_matches('/')))
}

fn body(model_settings: &Value, prompt: &str) -> Result<Value, String> {
    let session = Session::new(SYSTEM);
    let mut messages = session.for_model();
    messages.push(json!({"role":"user","content":prompt}));
    Ok(to_request_json(&build_body(
        text(model_settings, "model")?,
        &messages,
        None,
        None,
        model_settings["max_tokens"].as_i64(),
        Some(text(model_settings, "reasoning_effort")?),
    )))
}

fn prepare(manifest_path: &Path, directory: &Path) -> Result<Value, String> {
    let raw_manifest = std::fs::read(manifest_path).map_err(|_| "manifest_read")?;
    let mut manifest: Value = serde_json::from_slice(&raw_manifest).map_err(|_| "manifest_json")?;
    manifest["_actual_manifest_path"] = json!(manifest_path);
    let inputs = validate(&manifest)?;
    manifest
        .as_object_mut()
        .unwrap()
        .remove("_actual_manifest_path");
    let model_settings = settings(&manifest)?;
    let config = load_config(Overrides {
        reasoning_effort: Some("high".into()),
        ..Overrides::default()
    })
    .map_err(|_| "main_config_load")?;
    if config.provider_protocol != "openai"
        || config.model != text(&model_settings, "model")?
        || config.reasoning_effort != text(&model_settings, "reasoning_effort")?
    {
        return Err("configured_model_settings_mismatch".into());
    }
    let endpoint_hash = sha(endpoint(&config.base_url)?.as_bytes());
    std::fs::create_dir(directory).map_err(|_| "prepared_directory_exists_or_unwritable")?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "prepared_directory_identity")?;
    write_new(&directory.join("collection-manifest.json"), &raw_manifest)?;
    let mut output = new_file(&directory.join("prepared.jsonl"))?;
    let build = build_info()?;
    for case in manifest["cases"].as_array().ok_or("cases")? {
        let case_dir = directory.join(text(case, "id")?);
        std::fs::create_dir(&case_dir).map_err(|_| "case_directory")?;
        let workspace = case_dir.join("workspace");
        std::fs::create_dir(&workspace).map_err(|_| "workspace_create")?;
        let workspace = workspace.canonicalize().map_err(|_| "workspace_identity")?;
        let task = inputs
            .source
            .as_array()
            .ok_or("tasks")?
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("task")?;
        let catalog_entry = inputs.catalog["tasks"]
            .as_array()
            .ok_or("catalog")?
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("catalog_task")?;
        let prompt = user_prompt(task, case)?;
        let request = body(&model_settings, &prompt)?;
        let raw_request = request.to_string().into_bytes();
        if raw_request.len() > 128 * 1024 {
            return Err("request_size_budget".into());
        }
        write_new(&case_dir.join("request.json"), &raw_request)?;
        let mut row = json!({"schema":"buglecat.verification.prepared-main.v1","id":case["id"],
            "manifest_file":directory.join("collection-manifest.json"),
            "main_resource_profile_file":manifest["main_resource_profile_file"],"main_resource_profile_sha256":manifest["main_resource_profile_sha256"],"main_resource_profile_hash":manifest["main_resource_profile_hash"],
            "artifact_cohort_file":manifest["artifact_cohort_file"],"artifact_cohort_sha256":manifest["artifact_cohort_sha256"],"artifact_cohort_hash":manifest["artifact_cohort_hash"],
            "task_id":case["task_id"],"source_kind":case["source_kind"],"group_id":case["group_id"],"split":case["split"],
            "oracle_runtime_binding":manifest["oracle_runtime_binding"],
            "source_sha256":manifest["source_sha256"],"task_sha256":catalog_entry["task_sha256"],
            "catalog_sha256":manifest["catalog_sha256"],"protocol_sha256":manifest["protocol_sha256"],
            "authorization_sha256":manifest["authorization_sha256"],"manifest_sha256":sha(&raw_manifest),
            "public_interface":case["public_interface"],"prompt":prompt,"prompt_sha256":sha(prompt.as_bytes()),
            "system_prompt_sha256":sha(SYSTEM.as_bytes()),"body":request,"request_hash":sha(&raw_request),
            "settings":model_settings,"endpoint_sha256":endpoint_hash,"build_info":build,
            "case_directory":case_dir,"workspace":workspace,
            "session_log":workspace.join(".nanocodex/sessions").join(format!("{}.jsonl",text(case,"id")?)),
            "agent_loop_executed":false,"tools_available":false,"reference_and_assertions_sent":false,
            "public_interface_basis":"caller AST declaration; Rust validates signature subset, not Python AST"});
        if let Some(resource) = &inputs.resource {
            check_resource_identity(resource, &row)?;
            row["resource_cohort"] = json!({"cohort_id":resource["cohort_id"],
                "resource_adjustment_sha256":manifest["resource_adjustment_sha256"],
                "previous_settings_sha256":resource["previous_settings_sha256"]});
        }
        for key in [
            "holdout_consumption_file",
            "holdout_consumption_sha256",
            "holdout_consumption_hash",
            "calibration_hash",
        ] {
            if let Some(value) = manifest.get(key) {
                row[key] = value.clone();
            }
        }
        let row = seal(row);
        durable(&mut output, &row)?;
    }
    Ok(
        json!({"status":"prepared","cases":manifest["cases"].as_array().unwrap().len(),
        "directory":directory,"prepared_sha256":sha(&std::fs::read(directory.join("prepared.jsonl")).map_err(|_| "prepared_read")?),
        "paid_attempts":0,"paid_calls":0,"http_calls":0,"build_info":build}),
    )
}

#[async_trait(?Send)]
trait Transport {
    async fn post(&self, key: &str, endpoint: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String>;
}
struct HttpTransport;
#[async_trait(?Send)]
impl Transport for HttpTransport {
    async fn post(&self, key: &str, endpoint: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "client_error")?;
        let response = client
            .post(endpoint)
            .bearer_auth(key)
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| "transport_error")?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Ok((status, Vec::new()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err("response_budget".into());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "transport_error")?;
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err("response_budget".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }
}

async fn bounded(
    transport: &dyn Transport,
    key: &str,
    endpoint: &str,
    bytes: &[u8],
    timeout: Duration,
    cancel: Option<&Path>,
) -> Result<(u16, Vec<u8>), String> {
    let request = tokio::time::timeout(timeout, transport.post(key, endpoint, bytes));
    tokio::pin!(request);
    loop {
        tokio::select! {
            result = &mut request => return result.map_err(|_| "timeout")?,
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                if cancel.is_some_and(Path::exists) { return Err("cancelled".into()); }
            }
        }
    }
}

fn record_response(row: &Value, bytes: &[u8], session: &mut Session) -> Result<Value, String> {
    let case_dir = Path::new(text(row, "case_directory")?);
    write_new(&case_dir.join("raw-response.json"), bytes)?;
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| "response_invalid_json")?;
    let choices = raw["choices"].as_array().ok_or("response_choices")?;
    if choices.len() != 1
        || choices[0]["message"]["content"]
            .as_str()
            .is_none_or(|c| c.is_empty())
    {
        return Err("response_content_invalid".into());
    }
    if raw["model"] != row["settings"]["model"]
        || raw["usage"]["prompt_tokens"].as_u64().is_none()
        || raw["usage"]["completion_tokens"].as_u64().is_none()
    {
        return Err("response_model_or_usage_invalid".into());
    }
    let response = parse_completion(&raw);
    let workspace = Path::new(text(row, "workspace")?);
    let log = PathBuf::from(text(row, "session_log")?);
    // Unrequested tool calls remain in the original raw response. They are not
    // invoked or turned into fabricated tool results/native successful evidence.
    session.add_assistant(
        &response.content,
        choices[0]["message"]["tool_calls"].as_array().cloned(),
        &response.reasoning,
    );
    let actual = Session::resume(SYSTEM, Some(log.clone()));
    if actual.messages.last().and_then(|m| m["content"].as_str()) != Some(&response.content) {
        return Err("session_persistence_failed".into());
    }
    write_new(&case_dir.join("candidate.txt"), response.content.as_bytes())?;
    let packet =
        verification::evidence::packet(session, workspace, &response.content, &Default::default());
    let host = verification::policy::evaluate(&packet);
    write_new(
        &case_dir.join("packet.json"),
        &serde_json::to_vec(&packet).map_err(|_| "packet_json")?,
    )?;
    write_new(
        &case_dir.join("host-B.json"),
        &serde_json::to_vec(&host).map_err(|_| "host_json")?,
    )?;
    Ok(
        json!({"candidate":response.content,"candidate_sha256":sha(response.content.as_bytes()),
        "response_sha256":sha(bytes),"confirmed_model":raw["model"],"usage":raw["usage"],
        "finish_reason":response.finish_reason,"unexpected_tool_calls":response.tool_calls.len(),
        "session_log":log,"session_sha256":sha(&std::fs::read(&log).map_err(|_| "session_read")?),
        "sources_sha256":sha(&std::fs::read(log.with_extension("sources.jsonl")).map_err(|_| "sources_read")?),
        "events_file":log.parent().unwrap().join("events.sqlite3"),"workspace":workspace,
        "packet_sha256":sha(&std::fs::read(case_dir.join("packet.json")).map_err(|_| "packet_read")?),
        "claims":packet.claims,"host_B":host,"assertions_executed":false,"agent_loop_executed":false,
        "tools_executed":0,"label":"unlabeled","scope":"external benchmark single-response; no execution or task completion proof"}),
    )
}

async fn collect_one(
    row: &Value,
    transport: &dyn Transport,
    key: &str,
    target: &str,
    ledger: &mut Ledger,
    events: &mut File,
    cancel: Option<&Path>,
) -> Result<Value, String> {
    check_seal(row)?;
    if row["id"] != "offline" {
        revalidate_row(row)?;
    }
    let case_dir = Path::new(text(row, "case_directory")?);
    let bytes = std::fs::read(case_dir.join("request.json")).map_err(|_| "request_read")?;
    if sha(&bytes) != text(row, "request_hash")?
        || bytes != row["body"].to_string().as_bytes()
        || row["body"]["messages"][1]["content"] != row["prompt"]
        || row["body"]["messages"][0]["content"] != SYSTEM
        || bytes.len() > 128 * 1024
    {
        return Err("actual_request_body_mismatch".into());
    }
    if cancel.is_some_and(Path::exists) {
        return Err("cancelled_before_attempt".into());
    }
    let log = PathBuf::from(text(row, "session_log")?);
    let mut session = Session::with_log(SYSTEM, Some(log.clone()));
    session.set_workspace(Path::new(text(row, "workspace")?));
    session.set_turn_id(1);
    let store = Rc::new(
        ContextStore::open(
            log.parent()
                .ok_or("session_directory")?
                .join("events.sqlite3"),
        )
        .map_err(|_| "events_open")?,
    );
    session.set_event_store(store);
    session.record_source(SourceKind::SystemPrompt, SYSTEM, None);
    session.add_user_text(text(row, "prompt")?);
    if std::fs::read_to_string(&log)
        .map_err(|_| "user_session_not_saved")?
        .lines()
        .count()
        != 1
    {
        return Err("user_session_already_exists_or_not_saved".into());
    }
    let started = Instant::now();
    let mut identity = json!({"id":row["id"],"group_id":row["group_id"],"task_id":row["task_id"],"source_kind":row["source_kind"],"split":row["split"],
        "source_sha256":row["source_sha256"],"protocol_sha256":row["protocol_sha256"],
        "main_resource_profile_file":row["main_resource_profile_file"],"main_resource_profile_sha256":row["main_resource_profile_sha256"],"main_resource_profile_hash":row["main_resource_profile_hash"],
        "artifact_cohort_file":row["artifact_cohort_file"],"artifact_cohort_sha256":row["artifact_cohort_sha256"],"artifact_cohort_hash":row["artifact_cohort_hash"],
        "authorization_sha256":row["authorization_sha256"],"request_hash":row["request_hash"],
        "manifest_sha256":row["manifest_sha256"],"case_directory":row["case_directory"],
        "settings":row["settings"],"build_info":row["build_info"],"resource_cohort":row["resource_cohort"],"oracle_runtime_binding":row["oracle_runtime_binding"]});
    for key in [
        "holdout_consumption_file",
        "holdout_consumption_sha256",
        "holdout_consumption_hash",
        "calibration_hash",
    ] {
        if let Some(value) = row.get(key) {
            identity[key] = value.clone();
        }
    }
    let mut event = identity.clone();
    event["event"] = json!("attempt_started");
    let ordinal = ledger.reserve(text(row, "group_id")?, &event)?;
    event["attempt_ordinal"] = json!(ordinal);
    durable(events, &event)?;
    event["event"] = json!("request_submitted");
    event["actual_request_hash"] = json!(sha(&bytes));
    durable(events, &event)?;
    let outcome = bounded(
        transport,
        key,
        target,
        &bytes,
        Duration::from_secs(row["settings"]["timeout_s"].as_u64().ok_or("timeout")?),
        cancel,
    )
    .await;
    let mut receipt = identity;
    receipt["schema"] = json!("buglecat.verification.collected-main.v1");
    receipt["actual_request_hash"] = json!(sha(&bytes));
    receipt["attempt_ordinal"] = json!(ordinal);
    receipt["elapsed_ms"] = json!(started.elapsed().as_millis());
    receipt["candidate_mode"] = json!("original_transcript");
    match outcome {
        Ok((status, bytes)) if (200..300).contains(&status) => {
            receipt["response_sha256"] = json!(sha(&bytes));
            match record_response(row, &bytes, &mut session) {
                Ok(result) => {
                    receipt["call_status"] = json!("ok");
                    receipt["result"] = result;
                }
                Err(reason) => {
                    receipt["call_status"] = json!(reason);
                }
            }
        }
        Ok((status, _)) => {
            receipt["call_status"] = json!(format!("http_{status}"));
        }
        Err(reason) => {
            receipt["call_status"] = json!(reason);
        }
    }
    let receipt = seal(receipt);
    write_new(
        &case_dir.join("receipt.json"),
        &serde_json::to_vec(&receipt).map_err(|_| "receipt_json")?,
    )?;
    event["event"] = json!("attempt_finished");
    event["call_status"] = receipt["call_status"].clone();
    event["response_sha256"] = receipt["response_sha256"].clone();
    event["receipt_sha256"] = json!(sha(
        &std::fs::read(case_dir.join("receipt.json")).map_err(|_| "receipt_read")?
    ));
    ledger.finish(ordinal, &event)?;
    durable(events, &event)?;
    Ok(receipt)
}

async fn live(directory: &Path, cancel: Option<&Path>) -> Result<Value, String> {
    let directory = directory
        .canonicalize()
        .map_err(|_| "prepared_directory_identity")?;
    let manifest_path = directory.join("collection-manifest.json");
    let mut manifest = read_json(&manifest_path)?;
    manifest["_actual_manifest_path"] = json!(manifest_path);
    let inputs = validate(&manifest)?;
    manifest
        .as_object_mut()
        .unwrap()
        .remove("_actual_manifest_path");
    let model_settings = settings(&manifest)?;
    let config = load_config(Overrides {
        reasoning_effort: Some("high".into()),
        ..Overrides::default()
    })
    .map_err(|_| "main_config_load")?;
    if config.api_key.is_empty() {
        return Err("main_key_missing_not_run".into());
    }
    if config.provider_protocol != "openai"
        || config.model != text(&model_settings, "model")?
        || config.reasoning_effort != text(&model_settings, "reasoning_effort")?
    {
        return Err("configured_model_settings_mismatch".into());
    }
    let target = endpoint(&config.base_url)?;
    let build = build_info()?;
    let rows: Vec<Value> = std::fs::read_to_string(directory.join("prepared.jsonl"))
        .map_err(|_| "prepared_read")?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|_| "prepared_json".to_string()))
        .collect::<Result<_, _>>()?;
    if rows.len() != manifest["cases"].as_array().ok_or("cases")?.len() {
        return Err("prepared_case_count".into());
    }
    let manifest_hash =
        sha(&std::fs::read(directory.join("collection-manifest.json"))
            .map_err(|_| "manifest_read")?);
    for (row, case) in rows.iter().zip(manifest["cases"].as_array().unwrap()) {
        check_seal(row)?;
        for key in [
            "holdout_consumption_file",
            "holdout_consumption_sha256",
            "holdout_consumption_hash",
            "calibration_hash",
        ] {
            if row.get(key) != manifest.get(key) {
                return Err("holdout_opening_changed".into());
            }
        }
        let task = inputs
            .source
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("task")?;
        let prompt = user_prompt(task, case)?;
        if row["main_resource_profile_file"] != manifest["main_resource_profile_file"]
            || row["main_resource_profile_sha256"] != manifest["main_resource_profile_sha256"]
            || row["main_resource_profile_hash"] != manifest["main_resource_profile_hash"]
            || row["build_info"] != build
            || row["artifact_cohort_file"] != manifest["artifact_cohort_file"]
            || row["artifact_cohort_sha256"] != manifest["artifact_cohort_sha256"]
            || row["artifact_cohort_hash"] != manifest["artifact_cohort_hash"]
            || row["manifest_sha256"] != manifest_hash
            || row["id"] != case["id"]
            || row["group_id"] != case["group_id"]
            || row["task_id"] != case["task_id"]
            || row["source_kind"] != case["source_kind"]
            || row["oracle_runtime_binding"] != manifest["oracle_runtime_binding"]
            || row["split"] != case["split"]
            || row["source_sha256"] != manifest["source_sha256"]
            || row["catalog_sha256"] != manifest["catalog_sha256"]
            || row["protocol_sha256"] != manifest["protocol_sha256"]
            || row["authorization_sha256"] != manifest["authorization_sha256"]
            || row["settings"] != model_settings
            || row["prompt"] != prompt
            || row["prompt_sha256"] != sha(prompt.as_bytes())
            || row["public_interface"] != case["public_interface"]
            || row["endpoint_sha256"] != sha(target.as_bytes())
            || row["body"] != body(&model_settings, &prompt)?
            || row["system_prompt_sha256"] != sha(SYSTEM.as_bytes())
            || row["resource_cohort"] != inputs.resource.as_ref().map(|resource| json!({
                "cohort_id":resource["cohort_id"],"resource_adjustment_sha256":manifest["resource_adjustment_sha256"],
                "previous_settings_sha256":resource["previous_settings_sha256"]})).unwrap_or(Value::Null)
        {
            return Err("prepared_source_model_or_build_changed".into());
        }
        let case_dir = directory.join(text(case, "id")?);
        let workspace = case_dir
            .join("workspace")
            .canonicalize()
            .map_err(|_| "workspace_identity")?;
        if !workspace.starts_with(&directory)
            || row["case_directory"] != json!(case_dir)
            || row["workspace"] != json!(workspace)
            || row["session_log"]
                != json!(workspace
                    .join(".nanocodex/sessions")
                    .join(format!("{}.jsonl", text(case, "id")?)))
        {
            return Err("prepared_path_changed".into());
        }
    }
    let mut ledger = Ledger::open(
        Path::new(text(&inputs.authorization["main_model"], "ledger_dir")?),
        original_auth(),
        inputs.authorization["main_model"]["max_attempts"]
            .as_u64()
            .unwrap(),
    )?;
    ledger.validate_baseline(&ledger_baseline(&inputs.cohort))?;
    ledger.require_next_cases(&inputs.cohort, manifest["cases"].as_array().unwrap())?;
    let groups = rows
        .iter()
        .map(|r| text(r, "group_id"))
        .collect::<Result<Vec<_>, _>>()?;
    ledger.reject_existing(&groups)?;
    reject_reserved(&inputs, manifest["cases"].as_array().unwrap())?;
    let initial_count = ledger.count()?;
    if initial_count == 0
        && rows.len() as u64
            > inputs.authorization["first_batch_main_cap"]
                .as_u64()
                .ok_or("first_batch_cap")?
    {
        return Err("first_batch_cap_exceeded".into());
    }
    bind_collection(
        &ledger,
        &rows,
        inputs.resource.as_ref(),
        manifest["resource_adjustment_sha256"].as_str(),
    )?;
    let mut events = new_file(&directory.join("attempt-events.jsonl"))?;
    let mut succeeded = 0;

    let mut stop_reason = "batch_completed";
    for row in &rows {
        let receipt = collect_one(
            row,
            &HttpTransport,
            &config.api_key,
            &target,
            &mut ledger,
            &mut events,
            cancel,
        )
        .await?;
        if receipt["call_status"] == "ok" {
            succeeded += 1;
        }
        println!("{}: {}", text(row, "id")?, receipt["call_status"]);
        let status = receipt["call_status"].as_str().unwrap_or("invalid_status");
        if status != "ok" {
            stop_reason = "fault_remaining_groups_not_run";
            break;
        }
        if cancel.is_some_and(Path::exists) {
            stop_reason = "cancelled_remaining_groups_not_run";
            break;
        }
    }
    let terminal = json!({"schema":"buglecat.verification.main-batch-terminal.v2","terminal_main":true,
        "events_sha256":sha(&std::fs::read(directory.join("attempt-events.jsonl")).map_err(|_|"terminal_events")?),
        "manifest_sha256":manifest_hash,"prepared_sha256":sha(&std::fs::read(directory.join("prepared.jsonl")).map_err(|_|"terminal_prepared")?),
        "artifact_cohort_hash":manifest["artifact_cohort_hash"],"stop_reason":stop_reason,"attempts_started":ledger.count()? - initial_count});
    let mut terminal = terminal;
    terminal["artifact_hash"] = json!(value_sha(&terminal));
    write_new(
        &directory.join("batch-terminal.json"),
        &serde_json::to_vec(&terminal).map_err(|_| "terminal_json")?,
    )?;
    Ok(
        json!({"status":"collection_finished","successful_responses":succeeded,"prepared_cases":rows.len(),
        "attempts_started":ledger.count()? - initial_count,"global_main_attempts":ledger.count()?,
        "stop_reason":stop_reason,"resource_cohort":rows[0]["resource_cohort"],
        "agent_loop_executed":false,"paid_main_attempts_in_ledger":true,"jev_calls":0}),
    )
}

struct FakeTransport {
    calls: Cell<u64>,
    response: Vec<u8>,
    fail: bool,
    pending: bool,
}
#[async_trait(?Send)]
impl Transport for FakeTransport {
    async fn post(
        &self,
        _key: &str,
        _endpoint: &str,
        _body: &[u8],
    ) -> Result<(u16, Vec<u8>), String> {
        self.calls.set(self.calls.get() + 1);
        if self.pending {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if self.fail {
            Err("offline_transport_failure".into())
        } else {
            Ok((200, self.response.clone()))
        }
    }
}

async fn offline_receipt(directory: &Path, mode: &str) -> Result<Value, String> {
    std::fs::create_dir(directory).map_err(|_| "offline_directory_exists")?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "offline_directory_identity")?;
    let workspace = directory.join("workspace");
    std::fs::create_dir(&workspace).map_err(|_| "offline_workspace")?;
    let model_settings = json!({"model":"offline-collection-fixture","reasoning_effort":"off","max_tokens":1024,"timeout_s":1});
    let request = body(
        &model_settings,
        "Implement the frozen benchmark acceptance criteria for a fixture",
    )?;
    write_new(
        &directory.join("request.json"),
        request.to_string().as_bytes(),
    )?;
    let row = seal(
        json!({"id":"offline","group_id":"offline-only","task_id":0,"source_kind":"mbpp","split":"development",
        "oracle_runtime_binding":{"fixture_only":true,"formal_runtime_attested":false},
        "case_directory":directory,"workspace":workspace,"session_log":workspace.join(".nanocodex/sessions/offline.jsonl"),
        "source_sha256":"offline-fixture-not-independent","protocol_sha256":"offline","authorization_sha256":"offline",
        "main_resource_profile_file":"offline-profile", "main_resource_profile_sha256":"offline-only", "main_resource_profile_hash":"offline-only", "artifact_cohort_file":"offline","artifact_cohort_sha256":"offline-cohort","artifact_cohort_hash":"offline-seal",
        "prompt":"Implement the frozen benchmark acceptance criteria for a fixture","body":request,
        "request_hash":value_sha(&request),"settings":model_settings,"build_info":build_info()?}),
    );
    let mut raw = json!({"model":"offline-collection-fixture",
        "choices":[{"message":{"content":"```python\ndef fixture():\n    return 1\n```\nI successfully implemented the frozen benchmark acceptance criteria."},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":0,"completion_tokens":0}});
    if mode == "malformed" {
        raw["choices"][0]["message"]["content"] = json!("");
    }
    if mode == "model_mismatch" {
        raw["model"] = json!("wrong-model");
    }
    if mode == "invalid_usage" {
        raw["usage"]["prompt_tokens"] = json!(-1);
    }
    let transport = FakeTransport {
        calls: Cell::new(0),
        fail: mode == "failure",
        pending: ["timeout", "cancel_during"].contains(&mode),
        response: raw.to_string().into_bytes(),
    };
    let mut ledger = Ledger::open(&directory.join("offline-ledger"), "offline", 750)?;
    for n in 0..267 {
        ledger.reserve(&format!("old-{n}"), &json!({"split":"development"}))?;
    }
    let mut events = new_file(&directory.join("attempt-events.jsonl"))?;
    let cancel = directory.join("cancel.flag");
    if mode == "cancel_before" {
        write_new(&cancel, b"cancel")?;
    }
    if mode == "cancel_during" {
        let path = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            std::fs::write(path, b"cancel").unwrap();
        });
    }
    let outcome = collect_one(
        &row,
        &transport,
        "",
        "offline-no-network",
        &mut ledger,
        &mut events,
        Some(&cancel),
    )
    .await;
    if mode == "cancel_before" {
        if outcome.as_ref().err().map(String::as_str) != Some("cancelled_before_attempt")
            || transport.calls.get() != 0
            || ledger.count()? != 267
        {
            return Err("cancel_before_invariant".into());
        }
        return Ok(
            json!({"status":"offline_cancel_before_passed","paid_calls":0,"http_calls":0,"fake_transport_calls":0,"global_main_attempts":267,"remaining":483,"build_info":build_info()?}),
        );
    }
    let receipt = outcome?;
    if transport.calls.get() != 1 || receipt["actual_request_hash"] != receipt["request_hash"] {
        return Err("offline_receipt_invariant".into());
    }
    Ok(
        json!({"status":"offline_receipt_passed","http_calls":0,"paid_calls":0,"fake_transport_calls":1,
        "receipt":directory.join("receipt.json"),"global_main_attempts":ledger.count()?,"remaining":750-ledger.count()?,"build_info":build_info()?}),
    )
}

pub async fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [action,path] if action == "validate-profile" => profile_path(Path::new(path)),
        [action,path] if action == "offline-body-test" => profile_path(Path::new(path)).and_then(|profile| {
            let settings=profile["main_collection_settings"].clone();
            let request=body(&settings,"Implement the frozen benchmark acceptance criteria for a fixture")?;
            Ok(json!({"settings":settings,"body":request,"request_hash":value_sha(&request),"http_calls":0,"paid_calls":0,"build_info":build_info()?}))
        }),
        [action,manifest,directory] if action == "prepare" => prepare(Path::new(manifest),Path::new(directory)),
        [action,directory,flag] if action == "live" && flag == "--live" => live(Path::new(directory),None).await,
        [action,directory,flag,cancel_flag,cancel] if action == "live" && flag == "--live" && cancel_flag == "--cancel-file" => live(Path::new(directory),Some(Path::new(cancel))).await,
        [action,directory] if action == "offline-receipt-test" => offline_receipt(Path::new(directory),"success").await,
        [action,directory] if action == "offline-failure-test" => offline_receipt(Path::new(directory),"failure").await,
        [action,directory,scenario] if action == "offline-test" && ["success","failure","malformed","model_mismatch","invalid_usage","timeout","cancel_before","cancel_during"].contains(&scenario.as_str()) => offline_receipt(Path::new(directory),scenario).await,

        _ => Err("usage: verification_artifact_v2_formal_collect prepare <manifest> <newdir> | live <prepared-dir> --live [--cancel-file path] | offline-receipt-test <newdir>".into()),
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}
