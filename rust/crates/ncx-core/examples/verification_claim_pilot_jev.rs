//! Prepare actual runtime Jev requests offline; send only after blind scope review.
#[allow(dead_code)]
#[path = "verification_collect/support.rs"]
mod support;
use async_trait::async_trait;
use ncx_core::verification::{self, types::VerificationInput};
use ncx_provider::jev::{self, JevTransport};
use serde_json::{json, Value};
use std::{cell::RefCell, path::Path};
use support::{read_json, sha, text, value_sha, Ledger};

fn save(path: &Path, value: &Value) -> Result<(), String> {
    use std::io::Write;
    let mut file = support::new_file(path)?;
    file.write_all(value.to_string().as_bytes())
        .map_err(|_| "write")?;
    file.sync_all().map_err(|_| "sync".into())
}
struct Capture(RefCell<Option<Value>>);
#[async_trait(?Send)]
impl JevTransport for Capture {
    async fn post(&self, _: &str, body: Value) -> Result<(u16, Vec<u8>), String> {
        if self.0.borrow().is_some() {
            return Err("duplicate_prepare".into());
        }
        *self.0.borrow_mut() = Some(body);
        Err("offline_preparation_no_transport".into())
    }
}

async fn run(mode: &str, root: &Path, index: usize) -> Result<(), String> {
    let auth = read_json(&root.join("authorization.json"))?;
    let manifest = read_json(&root.join("manifest.json"))?;
    let auth_hash = sha(&std::fs::read(root.join("authorization.json")).map_err(|_| "auth")?);
    if auth["approved"] != true
        || auth["jev_cap"] != 8
        || auth["automatic_retries"] != 0
        || manifest["authorization_sha256"] != auth_hash
        || index >= 8
        || Path::new(text(&auth, "ledger_root")?)
            .canonicalize()
            .map_err(|_| "root")?
            != root.canonicalize().map_err(|_| "root")?
    {
        return Err("authorization".into());
    }
    let group = text(&manifest["cases"][index], "id")?;
    if !group.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("group".into());
    }
    let directory = root.join(group);
    let receipt = read_json(&directory.join("receipt.json"))?;
    let receipt_hash = sha(&std::fs::read(directory.join("receipt.json")).map_err(|_| "receipt")?);
    if receipt["stop_reason"] != "completed" {
        return Err("no_natural_final_candidate".into());
    }
    if mode == "prepare" {
        let input: VerificationInput =
            serde_json::from_value(receipt["packet"].clone()).map_err(|_| "packet")?;
        let capture = Capture(RefCell::new(None));
        let mut budget = verification::Budget { calls: 0, limit: 1 };
        verification::verify_with(
            &input,
            &mut budget,
            None,
            Some("offline-placeholder"),
            &capture,
        )
        .await;
        let body = capture.0.into_inner().ok_or("no_sendable_semantic_claim")?;
        return save(
            &directory.join("jev-prepared.json"),
            &json!({"body":body,"request_hash":value_sha(&body),
            "receipt_sha256":receipt_hash,"paid_calls":0,"formal_eligible":false}),
        );
    }
    if mode != "send" {
        return Err("mode".into());
    }
    let prepared = read_json(&directory.join("jev-prepared.json"))?;
    let review = read_json(&directory.join("scope-review.json"))?;
    if prepared["receipt_sha256"] != receipt_hash
        || prepared["request_hash"] != value_sha(&prepared["body"])
        || review["request_hash"] != prepared["request_hash"]
        || review["receipt_sha256"] != receipt_hash
        || review["reviewer_kind"] != "model"
        || review["jev_scores_read"] != false
        || !["supported", "unsupported"].contains(&review["label"].as_str().unwrap_or(""))
        || review["scope_basis"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
    {
        return Err("independent_scope_review_required".into());
    }
    let assertions = std::fs::read(directory.join("assertions.json")).map_err(|_| "assertions")?;
    if review["assertions_sha256"] != sha(&assertions) {
        return Err("assertion_binding".into());
    }
    let key = std::env::var("NCX_JEV_API_KEY").map_err(|_| "missing_key")?;
    if key.trim().is_empty() {
        return Err("missing_key".into());
    }
    let mut ledger = Ledger::open(&root.join("jev"), &auth_hash, 8)?;
    ledger.bind("manifest", &value_sha(&manifest))?;
    ledger.bind(
        "sender_revision",
        &sha(include_bytes!("verification_claim_pilot_jev.rs")),
    )?;
    let ordinal=ledger.reserve(group,&json!({"group_id":group,"split":"development","request_hash":prepared["request_hash"],"automatic_retries":0}))?;
    let result = jev::HttpTransport
        .post(key.trim(), prepared["body"].clone())
        .await;
    let value = match result {
        Ok((status, body)) => {
            let response: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            json!({"http_status":status,"response":response,"response_sha256":sha(&body),
                "status":if (200..300).contains(&status)&&jev::validate(&response).is_ok(){"ok"}else{"invalid_or_http_error"}})
        }
        Err(error) => json!({"status":"transport_error","error_kind":error}),
    };
    save(&directory.join("jev-response.json"), &value)?;
    ledger.finish(ordinal, &value)?;
    println!(
        "task {index}: Jev status={} global_reserved={}",
        value["status"],
        ledger.count()?
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = if args.len() == 4 {
        match args[3].parse::<usize>() {
            Ok(i) => run(&args[1], Path::new(&args[2]), i).await,
            Err(_) => Err("index".into()),
        }
    } else {
        Err("usage: verification_claim_pilot_jev <prepare|send> <root> <index>".into())
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
