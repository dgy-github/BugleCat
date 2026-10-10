use super::{evidence::*, types::*, *};
use crate::context_trace::SourceKind;
use serde_json::json;
use std::{cell::Cell, rc::Rc};

#[test]
fn explicit_success_denials_remain_negative_in_authentic_and_mixed_reports() {
    for phrase in ["not success.", "not a success."] {
        let extracted = claims(phrase);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].context, "narrative");
        assert_eq!(extracted[0].polarity, "negative");
    }
    let authentic = claims("python check.py exited with code 1; not a success.");
    assert_eq!(authentic.last().unwrap().polarity, "negative");
    let mixed = claims("not success, but the feature is completed and unit tests passed");
    assert_eq!(
        mixed
            .iter()
            .map(|c| c.polarity.as_str())
            .collect::<Vec<_>>(),
        ["negative", "positive", "positive"]
    );
    let positive = claims("The operation was a success; the notebook was successfully saved");
    assert!(positive.iter().all(|c| c.polarity == "positive"));
    for claim in mixed {
        assert_eq!(
            &"not success, but the feature is completed and unit tests passed"
                [claim.start..claim.end],
            claim.text
        );
    }
}

fn session(root: &Path) -> Session {
    let mut session = Session::with_log("system private", Some(root.join("session.jsonl")));
    session.set_workspace(root);
    session.set_turn_id(1);
    session.set_event_store(Rc::new(
        crate::context_store::ContextStore::open(root.join("events.sqlite3")).unwrap(),
    ));
    session.add_user_text("Run tests and report their precise scope");
    session
}
fn tool(session: &mut Session, id: &str, command: &str, result: &str) {
    session.add_assistant("", Some(vec![json!({"id":id,"function":{"name":"shell","arguments":json!({"command":command}).to_string()}})]), "");
    session.add_tool_result(id, "shell", result);
}
fn input(candidate: &str) -> VerificationInput {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    let snap = workspace_snapshot(&session, dir.path());
    tool(
        &mut session,
        "test",
        "cargo test --lib",
        "Exit code: 0\n3 passed",
    );
    let mut checks = CheckSnapshots::new();
    checks.insert("test".into(), (snap.clone(), snap));
    let (before, after) = checks.get("test").unwrap();
    session.record_verification_snapshot("test", before, after);
    packet(&session, dir.path(), candidate, &checks)
}

#[test]
fn no_execution_and_partial_scope_cannot_pass() {
    let dir = tempfile::tempdir().unwrap();
    let session = session(dir.path());
    let value = packet(
        &session,
        dir.path(),
        "All tests passed",
        &Default::default(),
    );
    assert_eq!(policy::evaluate(&value).verdict, Verdict::NeedEvidence);
    let result = policy::evaluate(&input("All tests passed [test]"));
    assert_eq!(result.decisions[0].reason_code, "scope_mismatch");
}
#[test]
fn narrow_success_is_not_complete_task_verification() {
    let result = policy::evaluate(&input("cargo test --lib passed"));
    assert_eq!(result.decisions[0].verdict, Verdict::Pass);
    assert_eq!(result.verdict, Verdict::Unknown);
    assert_eq!(policy::evaluate(&input("")).verdict, Verdict::Unknown);
    assert_eq!(
        policy::evaluate(&input("Tests not run, result unverified")).verdict,
        Verdict::Contradiction
    );
}
#[test]
fn exact_spans_cover_multilingual_candidate_and_mixed_claims_stay_unknown() {
    let candidate = "测试通过。 未验证下一步\nTests passed but compile failed";
    let extracted = claims(candidate);
    assert_eq!(extracted.len(), 4);
    for claim in &extracted {
        assert_eq!(&candidate[claim.start..claim.end], claim.text);
    }
    assert_eq!(extracted[2].kind, ClaimKind::TestsPassed);
    assert_eq!(extracted[3].polarity, "negative");
    let mut value = input("cargo test --lib passed");
    value.claims[0].start = 1;
    assert_eq!(
        policy::evaluate(&value).decisions[0].reason_code,
        "invalid_claim_span"
    );
}
#[test]
fn foreign_and_fake_references_are_rejected() {
    let mut value = input("cargo test --lib passed");
    value.evidence[0].session_id = "other".into();
    assert_eq!(
        policy::evaluate(&value).decisions[0].reason_code,
        "foreign_evidence"
    );
    value.claims[0].evidence_ids = vec!["invented".into()];
    assert_eq!(
        policy::evaluate(&value).decisions[0].reason_code,
        "invalid_reference"
    );
}
#[test]
fn failure_timeout_unknown_truncation_and_conflicts_do_not_pass() {
    let mut value = input("cargo test --lib passed");
    value.evidence[0].exit_code = Some(1);
    assert_eq!(policy::evaluate(&value).verdict, Verdict::Contradiction);
    value.evidence[0].exit_code = Some(0);
    for status in ["failed", "timeout", "cancelled", "permission_denied"] {
        value.evidence[0].status = status.into();
        assert_eq!(policy::evaluate(&value).verdict, Verdict::Contradiction);
    }
    value.evidence[0].status = "unknown".into();
    assert_eq!(policy::evaluate(&value).verdict, Verdict::NeedEvidence);
    value.evidence[0].status = "ok".into();
    value.evidence[0].truncated = true;
    assert_eq!(policy::evaluate(&value).verdict, Verdict::NeedEvidence);
}
#[test]
fn file_changes_and_incomplete_snapshot_invalidate_results() {
    let mut value = input("cargo test --lib passed");
    value
        .current_files
        .hashes
        .insert("changed.py".into(), "new".into());
    assert_eq!(
        policy::evaluate(&value).decisions[0].reason_code,
        "stale_evidence"
    );
    value.current_files.hashes.clear();
    value.current_files.complete = false;
    assert_eq!(
        policy::evaluate(&value).decisions[0].reason_code,
        "stale_evidence"
    );
}
#[test]
fn logs_are_excluded_from_freshness_but_untracked_sources_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    let before = workspace_snapshot(&session, dir.path());
    tool(&mut session, "a", "cargo test --lib", "Exit code: 0");
    let after = workspace_snapshot(&session, dir.path());
    assert_eq!(before, after);
    std::fs::write(dir.path().join("untracked.rs"), "new source").unwrap();
    assert_ne!(after, workspace_snapshot(&session, dir.path()));
}
#[test]
fn commands_cannot_fake_test_scope_by_echoing_a_test_name() {
    for command in [
        "echo cargo test",
        "cargo test; echo success",
        "python -m pytest | exit 0",
        "pytest$command",
    ] {
        assert!(test_scope(command).is_empty(), "{command}");
    }
    assert_eq!(test_scope("cargo test --lib"), vec!["cargo test --lib"]);
}
#[test]
fn packet_preserves_requirements_and_runtime_evidence_and_excludes_old_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    tool(&mut session, "old", "cargo test", "old evidence");
    session.set_turn_id(2);
    session.add_user(json!([{"type":"text","text":"do not write"},{"type":"image_url","image_url":{"url":"private"}},{"type":"input_text","text":"run only lib tests"}]));
    tool(
        &mut session,
        "new",
        "cargo test --lib",
        "Exit code: 1\nfailed",
    );
    session.record_source(
        SourceKind::RuntimeVerification,
        "syntax failure",
        Some("failed"),
    );
    let value = packet(
        &session,
        dir.path(),
        "cargo test --lib passed",
        &Default::default(),
    );
    assert_eq!(value.requirements.len(), 3);
    assert!(value
        .requirements
        .iter()
        .any(|r| r.contains("do not write")));
    assert!(!value.requirements.iter().any(|r| r.contains("private")));
    assert!(!value
        .evidence
        .iter()
        .any(|e| e.excerpt.contains("old evidence")));
    assert!(value
        .evidence
        .iter()
        .any(|e| e.kind == "runtime_verification"));
}
#[test]
fn recent_failure_is_preserved_beyond_first_twelve_results() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    for i in 0..40 {
        tool(
            &mut session,
            &format!("call-{i}"),
            "cargo test --lib",
            "Exit code: 0\npassed",
        );
    }
    tool(
        &mut session,
        "latest",
        "cargo test --lib",
        "Exit code: 1\nfailed",
    );
    let value = packet(
        &session,
        dir.path(),
        "latest tests passed",
        &Default::default(),
    );
    assert_eq!(value.evidence.len(), 32);
    assert!(value
        .evidence
        .iter()
        .any(|e| e.tool_call_id.as_deref() == Some("latest")));
    assert!(value
        .limitations
        .contains(&"evidence_budget_truncated".into()));
}
#[test]
fn sensitive_fields_are_redacted_and_private_keys_are_never_sent() {
    let value = redact(
        r#"{"api_key":"arbitrary-secret", "password":"pass"} sk-abc user@example.com C:\Users\private\repo"#,
    );
    for secret in [
        "arbitrary-secret",
        "pass\"",
        "sk-abc",
        "user@example",
        "private",
    ] {
        assert!(!value.contains(secret), "{value}");
    }
    let value = input("-----BEGIN PRIVATE KEY-----");
    assert!(external_state(&value, &value.claims[0]).is_none());
}

struct Fake {
    calls: Cell<usize>,
}
#[async_trait::async_trait(?Send)]
impl ncx_provider::jev::JevTransport for Fake {
    async fn post(&self, _: &str, body: Value) -> Result<(u16, Vec<u8>), String> {
        assert_eq!(body["state"]["schema"], "buglecat.jev.claim.v2");
        self.calls.set(self.calls.get() + 1);
        Ok((200,json!({"model":"mock", "answers":{
            "unsupported_success":{"type":"noul","noul":0.0},"contradiction":{"type":"noul","noul":0.0},"incomplete":{"type":"noul","noul":0.0}},
            "usage":{"input_tokens":2,"output_tokens":1}}).to_string().into_bytes()))
    }
}
#[tokio::test]
async fn probabilities_cannot_overrule_failure_and_call_budget_is_global() {
    let fake = Fake {
        calls: Cell::new(0),
    };
    let mut value = input("cargo test --lib passed");
    value.evidence[0].exit_code = Some(1);
    let mut budget = Budget::default();
    for _ in 0..5 {
        let result = verify_with(&value, &mut budget, None, Some("fake"), &fake).await;
        assert_eq!(result.verdict, Verdict::Contradiction);
    }
    assert_eq!(fake.calls.get(), 0);
    let value = input("The requested feature is completed");
    for _ in 0..5 {
        let result = verify_with(&value, &mut budget, None, Some("fake"), &fake).await;
        assert_eq!(result.verdict, Verdict::Unknown);
    }
    assert_eq!(fake.calls.get(), MAX_CALLS_PER_TURN);
}
#[tokio::test]
async fn missing_key_and_cancellation_are_observable_unverified_states() {
    let fake = Fake {
        calls: Cell::new(0),
    };
    let value = input("cargo test --lib passed");
    let result = verify_with(&value, &mut Budget::default(), None, None, &fake).await;
    assert_eq!(result.call_status, "missing_key");
    assert_eq!(result.verdict, Verdict::Unknown);
    let result = verify_with(
        &value,
        &mut Budget::default(),
        Some(&|| true),
        Some("fake"),
        &fake,
    )
    .await;
    assert_eq!(result.call_status, "cancelled");
    assert_eq!(fake.calls.get(), 0);
}
#[test]
fn independent_log_recovers_exact_candidate_and_reports_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let session = session(dir.path());
    let value = input("cargo test --lib passed");
    let result = policy::evaluate(&value);
    persist(&session, Mode::Shadow, &result).unwrap();
    assert!(latest(&session, &value.candidate, Some(1)).is_some());
    assert!(latest(&session, "changed answer", Some(1)).is_none());
    assert!(latest(&session, &value.candidate, Some(2)).is_none());
    assert!(!session
        .trace_store()
        .unwrap()
        .records()
        .iter()
        .any(|r| r.text.contains("verification.result")));
    let mut session = session;
    session.log_path = Some(dir.path().join("missing-parent").join("session.jsonl"));
    assert_eq!(
        persist(&session, Mode::Shadow, &result),
        Err("persistence_failed")
    );
}

#[tokio::test]
#[ignore = "Requires key; sends one explicit semantic smoke request"]
async fn jev_shadow_live_records_without_changing_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path());
    tool(
        &mut session,
        "test",
        "cargo test --lib",
        "Exit code: 1\n2 failed",
    );
    let before = session.messages.clone();
    // Deterministic claims intentionally do not consume Jev calls. Include
    // exactly one affirmative semantic claim and use the fixture workspace.
    let candidate = "cargo test --lib passed; The requested feature is completed";
    let value = packet(&session, dir.path(), candidate, &Default::default());
    let checked = verify(&value, &mut Budget::default(), None).await;
    persist(&session, Mode::Shadow, &checked).unwrap();
    assert_eq!(session.messages, before);
    let result = latest(&session, candidate, Some(1)).unwrap();
    assert_eq!(result.call_status, "ok");
    assert_eq!(result.verdict, Verdict::Contradiction);
    assert_eq!(result.observations.len(), 1);
    assert_eq!(result.attempted_calls, 1);
}
