use super::*;
use std::cell::Cell;
struct FakeTransport {
    calls: Cell<u64>,
    response: Vec<u8>,
    fail: bool,
    pending: bool,
    status: u16,
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
            Err("transport_error".into())
        } else {
            Ok((self.status, self.response.clone()))
        }
    }
}

pub(super) async fn offline_receipt(directory: &Path, mode: &str) -> Result<Value, String> {
    let parent = directory
        .parent()
        .ok_or("fixture_parent")?
        .canonicalize()
        .map_err(|_| "fixture_parent")?;
    let temporary = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| "temporary_root")?;
    if !parent.starts_with(&temporary) {
        return Err("offline_fixture_requires_temporary_root".into());
    }
    if mode == "prepare" {
        let case = json!({"id":"offline", "group_id":"offline-only", "task_id":0,"split":"development","source_kind":"apps_stdin_restricted_script","task_sha256":"fixture-only","public_interface":{"question":"Echo one line","starter_code":""}});
        let settings = json!({"model":"offline-collection-fixture","reasoning_effort":"off","max_tokens":1024,"timeout_s":1});
        let manifest = json!({"schema":"fixture-only-not-genuine-stdin-manifest","fixture_only":true,"cases":[case],"settings":settings,"source_sha256":"offline","catalog_sha256":"offline","protocol_sha256":"offline","artifact_cohort_file":"fixture-no-authority"});
        let inputs = Inputs {
            source: json!([{"task_id":0,"source_kind":"apps_stdin_restricted_script","main_prompt":"Echo one line; fixture only"}]),
            catalog: json!({"tasks":[{"task_id":0,"task_sha256":"fixture-only"}]}),
            authorization: Value::Null,
            cohort: Value::Null,
        };
        return prepare_rows(
            &manifest,
            &serde_json::to_vec(&manifest).map_err(|_| "fixture_manifest")?,
            directory,
            &inputs,
            "fixture-not-endpoint",
            &settings,
        );
    }
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
    let mut row = seal(
        json!({"schema":"buglecat.verification.prepared-main.v1","id":"offline","group_id":"offline-only","task_id":0,"split":"development",
        "case_directory":directory,"workspace":workspace,"session_log":workspace.join(".nanocodex/sessions/offline.jsonl"),
        "source_sha256":"offline-fixture-not-independent","protocol_sha256":"offline","authorization_sha256":"offline",
        "main_resource_profile_file":"offline-profile", "main_resource_profile_sha256":"offline-only", "main_resource_profile_hash":"offline-only", "artifact_cohort_file":"offline","artifact_cohort_sha256":"offline-cohort","artifact_cohort_hash":"offline-seal",
        "prompt":"Implement the frozen benchmark acceptance criteria for a fixture","body":request,
        "request_hash":value_sha(&request),"endpoint_sha256":sha(b"offline-no-network"),"settings":model_settings,"build_info":build_info()?}),
    );
    if mode == "tamper" {
        row["prompt"] = json!("changed original request");
        row = seal({
            let mut plain = row.clone();
            plain.as_object_mut().unwrap().remove("artifact_sha256");
            plain
        });
    }
    write_new(
        &directory.join("offline-prepared.json"),
        &serde_json::to_vec(&row).map_err(|_| "offline_row_json")?,
    )?;
    let mut raw = json!({"model":"offline-collection-fixture",
        "choices":[{"message":{"content":"```python\nprint(input())\n```"},"finish_reason":"stop"}],
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
        status: if mode == "http400" { 400 } else { 200 },
        response: if mode == "http400" {
            b"{\"error\":\"fixture invalid public request\"}".to_vec()
        } else {
            raw.to_string().into_bytes()
        },
    };
    let mut ledger = Ledger::fixture(&directory.join("offline-ledger"))?;
    for n in 0..2 {
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
    if mode == "tamper" {
        if outcome.as_ref().err().map(String::as_str) != Some("actual_request_body_mismatch")
            || transport.calls.get() != 0
            || ledger.count()? != 2
        {
            return Err("tamper_before_attempt_invariant".into());
        }
        return Ok(
            json!({"status":"offline_tamper_rejected","paid_calls":0,"http_calls":0,"fake_transport_calls":0,
            "global_main_attempts":2,"remaining":1640,"build_info":build_info()?}),
        );
    }
    if mode == "cancel_before" {
        if outcome.as_ref().err().map(String::as_str) != Some("cancelled_before_attempt")
            || transport.calls.get() != 0
            || ledger.count()? != 2
        {
            return Err("cancel_before_invariant".into());
        }
        return Ok(
            json!({"status":"offline_cancel_before_passed","paid_calls":0,"http_calls":0,"fake_transport_calls":0,"global_main_attempts":2,"remaining":1640,"build_info":build_info()?}),
        );
    }
    let receipt = outcome?;
    if transport.calls.get() != 1 || receipt["actual_request_hash"] != receipt["request_hash"] {
        return Err("offline_receipt_invariant".into());
    }
    if mode == "duplicate"
        && (ledger
            .reserve("offline-only", &json!({"split":"development"}))
            .is_ok()
            || transport.calls.get() != 1
            || ledger.count()? != 3)
    {
        return Err("duplicate_group_invariant".into());
    }
    Ok(
        json!({"status":"offline_receipt_passed","http_calls":0,"paid_calls":0,"fake_transport_calls":1,
        "receipt":directory.join("receipt.json"),"global_main_attempts":ledger.count()?,"remaining":1642-ledger.count()?,"build_info":build_info()?}),
    )
}
