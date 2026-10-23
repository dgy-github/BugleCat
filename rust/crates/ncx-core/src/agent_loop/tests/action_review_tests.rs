//! Pre-execution action review through the real dispatch path. Only the model
//! and the Jev transport are scripted.
use super::*;
use crate::verification::{Config, Mode};

struct FakeJev {
    not_requested: f64,
    calls: Cell<usize>,
}

#[async_trait(?Send)]
impl ncx_provider::jev::JevTransport for FakeJev {
    async fn post(&self, _: &str, body: Value) -> Result<(u16, Vec<u8>), String> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(body["state"]["action"]["tool"], "apply_patch");
        let answers = json!({
            "not_requested": {"type":"noul","noul":self.not_requested},
            "exceeds_scope": {"type":"noul","noul":0.1},
            "irreversible": {"type":"noul","noul":0.2},
        });
        let response = json!({"model":"jev-1.13.0","answers":answers,
            "usage":{"input_tokens":5,"output_tokens":0}});
        Ok((200, response.to_string().into_bytes()))
    }
}

async fn run(mode: Mode, threshold: Option<f64>) -> (PathBuf, Rc<FakeJev>, Vec<Value>, Vec<Value>) {
    let ws = tmpdir("action_review");
    let provider = ScriptedProvider::new(vec![
        assistant_toolcall(vec![
            tc("look", "read_file", json!({"path":"missing.txt"})),
            tc(
                "plan",
                "update_plan",
                json!({"plan":[{"step":"write note","status":"in_progress"}]}),
            ),
            tc(
                "write",
                "apply_patch",
                json!({"patch":"*** Begin Patch\n*** Add File: note.txt\n+hello\n*** End Patch"}),
            ),
        ]),
        ModelResponse {
            content: "Done.".into(),
            ..Default::default()
        },
    ]);
    let session = Session::with_log("system", Some(ws.join("s.jsonl")));
    let mut agent = AgentLoop::new(
        Box::new(provider),
        ToolRegistry::new(ToolContext::new(
            ws.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, &ws),
        )),
        session,
    )
    .with_max_iterations(3)
    .with_verification(Config {
        mode,
        model_enabled: true,
        action_threshold: threshold,
        ..Default::default()
    });
    let fake = Rc::new(FakeJev {
        not_requested: 0.97,
        calls: Cell::new(0),
    });
    agent.action_review_override = Some(("test-key".into(), fake.clone()));
    agent
        .run_turn(json!("Explain what note.txt is for."), None)
        .await;
    let rows = std::fs::read_to_string(ws.join("s.action_review.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (ws, fake, rows, agent.session.messages.clone())
}

fn tool_result<'a>(messages: &'a [Value], id: &str) -> &'a str {
    messages
        .iter()
        .find(|m| m["role"] == "tool" && m["tool_call_id"] == id)
        .and_then(|m| m["content"].as_str())
        .unwrap()
}

#[tokio::test]
async fn shadow_reviews_only_the_write_and_still_runs_it() {
    let (ws, fake, rows, _) = run(Mode::Shadow, Some(0.9)).await;
    assert!(ws.join("note.txt").exists());
    assert_eq!(
        fake.calls.get(),
        1,
        "read_file and update_plan are never reviewed"
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["schema"], "buglecat.action_review.record.v1");
    assert_eq!(rows[0]["mode"], "shadow");
    assert_eq!(rows[0]["review"]["tool"], "apply_patch");
    assert_eq!(rows[0]["review"]["decision"], "allow");
    assert_eq!(rows[0]["review"]["reason_code"], "observe_only");
    assert_eq!(
        rows[0]["review"]["response"]["answers"]["not_requested"]["noul"],
        0.97
    );
}

#[tokio::test]
async fn enforce_with_calibrated_threshold_holds_the_write() {
    let (ws, _, rows, messages) = run(Mode::Enforce, Some(0.9)).await;
    assert!(!ws.join("note.txt").exists());
    assert_eq!(rows[0]["review"]["decision"], "hold");
    assert_eq!(rows[0]["review"]["reason_code"], "jev_not_requested");
    let held = tool_result(&messages, "write");
    assert!(
        held.starts_with("Error:") && held.contains("did not run"),
        "{held}"
    );
}

#[tokio::test]
async fn enforce_without_threshold_records_but_runs() {
    let (ws, _, rows, _) = run(Mode::Enforce, None).await;
    assert!(ws.join("note.txt").exists());
    assert_eq!(rows[0]["review"]["decision"], "allow");
}
