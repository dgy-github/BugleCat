//! Explicit single-response collection, with a durable global request budget.
//! Default prepare and offline-receipt-test paths never construct an HTTP client.
#[path = "support.rs"]
mod support;
use async_trait::async_trait;
use ncx_config::{load_config, Overrides};
use ncx_core::{context_store::ContextStore, context_trace::SourceKind, verification, Session};
use ncx_provider::{
    request::{build_body, to_request_json},
    response::parse_completion,
};
use serde_json::{json, Value};
use std::{
    fs::File,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
use support::*;

#[path = "fixture.rs"]
mod fixture;
#[path = "provenance.rs"]
mod provenance;
#[path = "../verification_apps_stdin_v1_collect/transport.rs"]
mod transport;
use provenance::build_info;
use transport::*;
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

fn response_candidate_too_large(value: &Value) -> Result<bool, String> {
    value.as_str().ok_or("candidate_string")?;
    // Strict UTF8 JSON literal, matching the frozen Python candidate bound.
    Ok(serde_json::to_vec(value)
        .map_err(|_| "candidate_json")?
        .len()
        > 16000)
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
    prepare_rows(
        &manifest,
        &raw_manifest,
        directory,
        &inputs,
        &endpoint_hash,
        &model_settings,
    )
}

fn prepare_rows(
    manifest: &Value,
    raw_manifest: &[u8],
    directory: &Path,
    inputs: &Inputs,
    endpoint_hash: &str,
    model_settings: &Value,
) -> Result<Value, String> {
    std::fs::create_dir(directory).map_err(|_| "prepared_directory_exists_or_unwritable")?;
    let directory = directory
        .canonicalize()
        .map_err(|_| "prepared_directory_identity")?;
    write_new(&directory.join("collection-manifest.json"), raw_manifest)?;
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
        let request = body(model_settings, &prompt)?;
        let raw_request = request.to_string().into_bytes();
        if raw_request.len() > 128 * 1024 {
            return Err("request_size_budget".into());
        }
        write_new(&case_dir.join("request.json"), &raw_request)?;
        let mut row = json!({"schema":"buglecat.verification.prepared-main.v1","id":case["id"],
            "manifest_file":directory.join("collection-manifest.json"),
            "main_resource_profile_file":manifest["main_resource_profile_file"],"main_resource_profile_sha256":manifest["main_resource_profile_sha256"],"main_resource_profile_hash":manifest["main_resource_profile_hash"],
            "artifact_cohort_file":manifest["artifact_cohort_file"],"artifact_cohort_sha256":manifest["artifact_cohort_sha256"],"artifact_cohort_hash":manifest["artifact_cohort_hash"],
            "task_id":case["task_id"],"group_id":case["group_id"],"split":case["split"],
            "source_sha256":manifest["source_sha256"],"task_sha256":catalog_entry["task_sha256"],
            "catalog_sha256":manifest["catalog_sha256"],"protocol_sha256":manifest["protocol_sha256"],
            "authorization_file":manifest["authorization_file"],"authorization_sha256":manifest["authorization_sha256"],"manifest_sha256":sha(raw_manifest),
            "public_interface":case["public_interface"],"prompt":prompt,"prompt_sha256":sha(prompt.as_bytes()),
            "system_prompt_sha256":sha(SYSTEM.as_bytes()),"body":request,"request_hash":sha(&raw_request),
            "settings":model_settings,"endpoint_sha256":endpoint_hash,"build_info":build,
            "case_directory":case_dir,"workspace":workspace,
            "session_log":workspace.join(".nanocodex/sessions").join(format!("{}.jsonl",text(case,"id")?)),
            "agent_loop_executed":false,"tools_available":false,"reference_and_assertions_sent":false,
            "public_interface_basis":"original public STDIN question/starter and frozen policy only; no reference or hidden IO",
            "candidate_literal_limit":16000,"source_kind":case["source_kind"],"formal_registration":false,"holdout_opened":false,"production_authorized":false});
        for field in APPS_BINDINGS {
            row[*field] = manifest[*field].clone();
        }
        let row = seal(row);
        write_new(
            &case_dir.join("prepared-row.json"),
            &serde_json::to_vec(&row).map_err(|_| "row_json")?,
        )?;
        durable(&mut output, &row)?;
    }
    Ok(
        json!({"status":"prepared","cases":manifest["cases"].as_array().unwrap().len(),
        "directory":directory,"prepared_sha256":sha(&std::fs::read(directory.join("prepared.jsonl")).map_err(|_| "prepared_read")?),
        "paid_attempts":0,"paid_calls":0,"http_calls":0,"build_info":build}),
    )
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
    if response_candidate_too_large(&choices[0]["message"]["content"])? {
        return Err("candidate_literal_resource_budget".into());
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
    let mut identity = json!({"id":row["id"],"group_id":row["group_id"],"task_id":row["task_id"],"split":row["split"],
        "source_sha256":row["source_sha256"],"protocol_sha256":row["protocol_sha256"],
        "main_resource_profile_file":row["main_resource_profile_file"],"main_resource_profile_sha256":row["main_resource_profile_sha256"],"main_resource_profile_hash":row["main_resource_profile_hash"],
        "artifact_cohort_file":row["artifact_cohort_file"],"artifact_cohort_sha256":row["artifact_cohort_sha256"],"artifact_cohort_hash":row["artifact_cohort_hash"],
        "authorization_sha256":row["authorization_sha256"],"request_hash":row["request_hash"],"endpoint_sha256":row["endpoint_sha256"],
        "settings":row["settings"],"build_info":row["build_info"],"resource_cohort":row["resource_cohort"],
        "source_kind":row["source_kind"],"task_sha256":row["task_sha256"],"manifest_file":row["manifest_file"],"manifest_sha256":row["manifest_sha256"],
        "case_directory":row["case_directory"],"workspace":row["workspace"],"session_log":row["session_log"],"catalog_sha256":row["catalog_sha256"],
        "formal_registration":false,"holdout_opened":false,"production_authorized":false,"tools_executed":0,"agent_loop_executed":false});
    for field in APPS_BINDINGS {
        identity[*field] = row[*field].clone();
    }
    let mut event = identity.clone();
    event["event"] = json!("attempt_started");
    let ordinal = ledger.reserve(text(row, "group_id")?, &event)?;
    event["attempt_ordinal"] = json!(ordinal);
    durable(events, &event)?;
    event["event"] = json!("request_submitted");
    event["actual_request_hash"] = json!(sha(&bytes));
    if row["id"] != "offline" {
        static_row(row)?;
    }
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
    receipt["http_status"] = Value::Null;
    receipt["response_sha256"] = Value::Null;
    match outcome {
        Ok((status, bytes)) if (200..300).contains(&status) => {
            receipt["http_status"] = json!(status);
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
        Ok((status, bytes)) => {
            receipt["http_status"] = json!(status);
            write_new(&case_dir.join("raw-response.json"), &bytes)?;
            receipt["response_sha256"] = json!(sha(&bytes));
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
    event["http_status"] = receipt["http_status"].clone();
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
        let task = inputs
            .source
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["task_id"] == case["task_id"])
            .ok_or("task")?;
        let prompt = user_prompt(task, case)?;
        if APPS_BINDINGS
            .iter()
            .any(|field| row[*field] != manifest[*field])
            || row["source_kind"] != case["source_kind"]
            || row["authorization_file"] != manifest["authorization_file"]
            || row["main_resource_profile_file"] != manifest["main_resource_profile_file"]
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
        text(
            &inputs.cohort["ledger_snapshot"]["authorization_chain"],
            "ledger_authorization_sha256",
        )?,
        inputs.authorization["main_model"]["max_attempts"]
            .as_u64()
            .unwrap(),
        &inputs.cohort,
    )?;
    let groups = rows
        .iter()
        .map(|r| text(r, "group_id"))
        .collect::<Result<Vec<_>, _>>()?;
    ledger.reject_existing(&groups)?;
    let initial_count = ledger.count()?;
    bind_collection(&rows)?;
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
    close_batch(&rows[0])?;
    Ok(
        json!({"status":"collection_finished","successful_responses":succeeded,"prepared_cases":rows.len(),
        "attempts_started":ledger.count()? - initial_count,"global_main_attempts":ledger.count()?,
        "stop_reason":stop_reason,"resource_cohort":rows[0]["resource_cohort"],
        "agent_loop_executed":false,"paid_main_attempts_in_ledger":true,"jev_calls":0}),
    )
}

pub async fn run() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result=match args.as_slice() {
        [action] if action=="build-info" => build_info().map(|build| json!({"build_info":build,"paid_calls":0,"http_calls":0})),
        [action,manifest,directory] if action=="prepare" => prepare(Path::new(manifest),Path::new(directory)),
        [action,directory,flag] if action=="live" && flag=="--live" => live(Path::new(directory),None).await,
        [action,directory,flag,cancel_flag,cancel] if action=="live" && flag=="--live" && cancel_flag=="--cancel-file" => live(Path::new(directory),Some(Path::new(cancel))).await,
        [action,directory,scenario] if action=="offline-test" && ["prepare","success","http400","failure","malformed","model_mismatch","invalid_usage","timeout","cancel_before","cancel_during","tamper","duplicate"].contains(&scenario.as_str()) => fixture::offline_receipt(Path::new(directory),scenario).await,
        _=>Err("usage: verification_apps_stdin_v1_collect build-info | prepare <manifest> <newdir> | live <prepared-dir> --live [--cancel-file path] | offline-test <new-tempdir> <scenario>".into())
    };
    match result {
        Ok(value) => println!("{value}"),
        Err(reason) => {
            eprintln!("{reason}");
            std::process::exit(1);
        }
    }
}
