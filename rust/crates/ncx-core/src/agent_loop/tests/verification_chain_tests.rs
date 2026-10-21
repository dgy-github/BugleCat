//! Real shell -> persisted evidence -> extraction -> policy -> correction.
//! Only the model is scripted. No external model/verification transport is used.
use super::*;
use crate::verification::{Config, Mode, types::ClaimKind};

fn answer(text: &str) -> ModelResponse {
    ModelResponse {
        content: text.into(),
        ..Default::default()
    }
}

async fn chain(
    exit_code: i32,
    mode: Mode,
    claim: &str,
    correction: &str,
) -> (TurnResult, Vec<Value>, Vec<Value>) {
    chain_with_edit(exit_code, mode, claim, correction, false).await
}

async fn chain_with_edit(
    exit_code: i32,
    mode: Mode,
    claim: &str,
    correction: &str,
    edit: bool,
) -> (TurnResult, Vec<Value>, Vec<Value>) {
    let ws = tmpdir("verification_real_chain");
    std::fs::write(
        ws.join("check.py"),
        format!("import sys\nprint('local fixture')\nsys.exit({exit_code})\n"),
    )
    .unwrap();
    let mut responses = vec![assistant_toolcall(vec![tc(
        "run-check",
        "shell",
        json!({"command":"python check.py"}),
    )])];
    if edit {
        responses.push(assistant_toolcall(vec![tc("edit-check", "apply_patch", json!({
            "patch":"*** Begin Patch\n*** Update File: check.py\n@@\n-print('local fixture')\n+print('changed after check')\n*** End Patch"
        }))]));
    }
    responses.extend([answer(claim), answer(correction)]);
    let provider = ScriptedProvider::new(responses);
    let mut session =
        Session::with_log("Report the actual check result.", Some(ws.join("s.jsonl")));
    session.set_workspace(&ws);
    session.set_event_store(Rc::new(
        crate::context_store::ContextStore::open(ws.join("events.sqlite3")).unwrap(),
    ));
    let mut agent = AgentLoop::new(
        Box::new(provider),
        ToolRegistry::new(ToolContext::new(
            ws.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, &ws),
        )),
        session,
    )
    .with_max_iterations(5)
    .with_verification(Config {
        mode,
        model_enabled: false,
        experimental_jev: None,
        enforce_categories: vec![ClaimKind::TestsPassed, ClaimKind::ExecutionSuccess],
    });
    let result = agent
        .run_turn(json!("Run python check.py and report its result."), None)
        .await;
    let rows: Vec<Value> = std::fs::read_to_string(ws.join("s.verification.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(rows.iter().all(|r| r["result"]["attempted_calls"] == 0));
    assert!(agent.session.messages.iter().any(|m| m["role"] == "tool"));
    (result, rows, agent.session.messages)
}

#[tokio::test]
async fn real_failed_shell_draft_is_rejected_and_corrected() {
    let claim = "python check.py passed [call:run-check]";
    let correction = "python check.py failed [call:run-check]";
    let (result, rows, messages) = chain(1, Mode::Enforce, claim, correction).await;
    assert_eq!(result.stop_reason, "completed");
    assert_eq!(result.final_text, correction);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason_code"] == "explicit_failure")
    );
    assert!(
        !messages
            .iter()
            .any(|m| m["role"] == "assistant" && m["content"] == claim)
    );
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn real_success_is_kept_without_spurious_correction() {
    let claim = "python check.py passed [call:run-check]";
    let (result, rows, _) = chain(0, Mode::Enforce, claim, "unexpected correction").await;
    assert_eq!(result.final_text, claim);
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["verdict"] == "pass")
    );
}

#[tokio::test]
async fn real_failure_shadow_records_contradiction_without_revising() {
    let claim = "python check.py passed [call:run-check]";
    let (result, rows, _) = chain(1, Mode::Shadow, claim, "unexpected correction").await;
    assert_eq!(result.final_text, claim);
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason_code"] == "explicit_failure")
    );
}

#[tokio::test]
async fn real_narrow_check_rejects_all_tests_claim() {
    let (result, rows, _) = chain(
        0,
        Mode::Enforce,
        "All tests passed [call:run-check]",
        "python check.py passed [call:run-check]",
    )
    .await;
    assert_eq!(result.stop_reason, "completed");
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason_code"] == "scope_mismatch")
    );
}

#[tokio::test]
async fn real_edit_after_check_rejects_stale_success() {
    let (result, rows, _) = chain_with_edit(
        0,
        Mode::Enforce,
        "python check.py passed [call:run-check]",
        "Current changes remain unverified",
        true,
    )
    .await;
    assert_eq!(result.stop_reason, "completed");
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason_code"] == "stale_evidence")
    );
}

#[tokio::test]
async fn real_failed_shell_chinese_claim_is_corrected() {
    let (result, rows, _) = chain(
        1,
        Mode::Enforce,
        "测试通过 [call:run-check]",
        "测试失败 [call:run-check]",
    )
    .await;
    assert_eq!(result.final_text, "测试失败 [call:run-check]");
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0]["result"]["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason_code"] == "explicit_failure")
    );
}
