//! End-to-end outcome recording through `run_turn`: one outcome per turn,
//! next-turn followup resolution, labels, and worker isolation.

use super::*;
use crate::outcome::{FollowupSignal, TaskRole, followups_for_task, load_outcomes_and_followups};

fn text(content: &str) -> ModelResponse {
    ModelResponse {
        content: content.into(),
        ..Default::default()
    }
}

fn build_logged(ws: &PathBuf, provider: Box<dyn Provider>) -> AgentLoop {
    let memory = Rc::new(crate::memory::MemoryStore::new(
        ws.join(".ncx").join("memory"),
    ));
    let ctx =
        ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, ws)).with_memory(memory);
    let session = Session::with_log(
        "system prompt",
        Some(
            ws.join(".nanocodex")
                .join("sessions")
                .join("s-outcome.jsonl"),
        ),
    );
    AgentLoop::new(provider, ToolRegistry::new(ctx), session).with_max_iterations(10)
}

fn store_of(agent: &AgentLoop) -> Rc<crate::context_store::ContextStore> {
    agent
        .session
        .event_store()
        .cloned()
        .expect("durable event store")
}

fn signals(agent: &AgentLoop, task_id: &str) -> Vec<FollowupSignal> {
    followups_for_task(&store_of(agent), task_id)
        .unwrap()
        .into_iter()
        .map(|f| f.signal)
        .collect()
}

#[tokio::test]
async fn next_human_turn_resolves_previous_task_once() {
    let ws = tmpdir("outcome_e2e_resolve");
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![tc(
            "edit",
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: notes.txt\n+hello\n*** End Patch"}),
        )]),
        text("created notes.txt"),
        text("second answer"),
        text("third answer"),
        text("fourth answer"),
    ]);
    let mut agent = build_logged(&ws, Box::new(p));
    let sid = agent.session.session_id.clone();

    agent.run_turn(json!("create notes.txt"), None).await;
    let first = format!("{sid}:turn-1");
    assert!(
        signals(&agent, &first).is_empty(),
        "no verdict until the human speaks"
    );

    // The human edits the file the agent just wrote, then speaks: I5.
    std::fs::write(ws.join("notes.txt"), "hello, edited by hand\n").unwrap();
    agent
        .run_turn(json!("now summarize the project"), None)
        .await;
    assert!(matches!(
        signals(&agent, &first).as_slice(),
        [FollowupSignal::ManualEdit { files }] if files == &vec!["notes.txt".to_string()]
    ));

    // A correction cue resolves turn 2 as I4 (inferred).
    agent.run_turn(json!("不对，还是不对"), None).await;
    let second = format!("{sid}:turn-2");
    assert!(matches!(
        signals(&agent, &second).as_slice(),
        [FollowupSignal::Correction { inferred: true, .. }]
    ));

    // An explicit label on turn 3 pre-empts automatic resolution of it.
    let labelled = agent.record_explicit_outcome_label(true, None).unwrap();
    let third = format!("{sid}:turn-3");
    assert_eq!(labelled, third);
    agent.run_turn(json!("thanks, next topic"), None).await;
    assert!(matches!(
        signals(&agent, &third).as_slice(),
        [FollowupSignal::Label { good: true, .. }]
    ));
    // Earlier tasks were not touched again.
    assert_eq!(signals(&agent, &first).len(), 1);
    assert_eq!(signals(&agent, &second).len(), 1);

    let (outcomes, _) = load_outcomes_and_followups(&store_of(&agent), None).unwrap();
    assert_eq!(outcomes.len(), 4);
    assert!(outcomes.iter().all(|o| o.role == TaskRole::Primary));
    assert_eq!(outcomes[0].files_touched, vec!["notes.txt".to_string()]);
    // No verifier ran in this test: completed turns are never recorded as "passed".
    assert!(
        outcomes
            .iter()
            .all(|o| o.verification.as_deref() != Some("passed"))
    );
}

#[tokio::test]
async fn worker_turns_are_recorded_under_parent_and_never_resolve_anything() {
    let ws = tmpdir("outcome_e2e_worker");
    let store =
        Rc::new(crate::context_store::ContextStore::open(ws.join("events.sqlite3")).unwrap());
    let p = ScriptedProvider::new(vec![text("worker one"), text("worker two")]);
    let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
    let mut agent = AgentLoop::new(Box::new(p), ToolRegistry::new(ctx), Session::new("sys"))
        .with_task_role(TaskRole::OrchestratorWorker, Some("s:orch-1".to_string()))
        .with_outcome_sink(store.clone(), "s:orch-1:worker-1-1".to_string());
    agent.run_turn(json!("fix the parser"), None).await;
    agent.run_turn(json!("不对"), None).await;

    let (outcomes, followups) = load_outcomes_and_followups(&store, None).unwrap();
    assert!(followups.is_empty(), "workers never produce followups");
    assert!(!outcomes.is_empty());
    assert!(outcomes.iter().all(|o| {
        o.role == TaskRole::OrchestratorWorker
            && o.parent_task_id.as_deref() == Some("s:orch-1")
            && o.task_id == "s:orch-1:worker-1-1"
    }));
}

#[tokio::test]
async fn unrelated_success_after_a_failure_is_not_a_recovery_candidate() {
    let ws = tmpdir("outcome_e2e_candidates");
    let p = ScriptedProvider::new(vec![
        // Fails: file does not exist yet.
        assistant_toolcall(vec![tc("r1", "read_file", json!({"path": "cfg.toml"}))]),
        // Unrelated success: must not pair with the failure above.
        assistant_toolcall(vec![tc("r2", "read_file", json!({"path": "other.toml"}))]),
        assistant_toolcall(vec![tc(
            "w",
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: cfg.toml\n+a = 1\n*** End Patch"}),
        )]),
        // Same tool, same target, now succeeds: one candidate.
        assistant_toolcall(vec![tc("r3", "read_file", json!({"path": "cfg.toml"}))]),
        text("done"),
    ]);
    std::fs::write(ws.join("other.toml"), "b = 2\n").unwrap();
    let mut agent = build_logged(&ws, Box::new(p));
    agent.run_turn(json!("read cfg.toml"), None).await;

    let memory = crate::memory::MemoryStore::new(ws.join(".ncx").join("memory"));
    let items = memory.review_items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].text.contains("`read_file` on `cfg.toml` failed"));
    // Nothing reaches LEARNINGS.md without a human confirming it.
    assert!(memory.entries().is_empty());
}

#[tokio::test]
async fn on_mode_promotes_verified_task_experience_and_withdraws_rejected_turn_notes() {
    let ws = tmpdir("outcome_e2e_on_mode");
    let mut mem_store = crate::memory::MemoryStore::new(ws.join(".ncx").join("memory"));
    mem_store.set_feedback_mode("on");
    let memory = Rc::new(mem_store);
    let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
        .with_memory(memory.clone());
    let session = Session::with_log(
        "system prompt",
        Some(ws.join(".nanocodex").join("sessions").join("s-on.jsonl")),
    );
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![tc(
            "w1",
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: sqlparse/grouping.py\n+x = 1\n*** End Patch"}),
        )]),
        text("Fixed Operation.get_type grouping in sqlparse/grouping.py."),
        assistant_toolcall(vec![tc(
            "w2",
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: sqlparse/broken.py\n+y = 2\n*** End Patch"}),
        )]),
        text("Attempted fix in sqlparse/broken.py."),
    ]);
    let mut agent =
        AgentLoop::new(Box::new(p), ToolRegistry::new(ctx), session).with_max_iterations(10);

    // Turn 1: completes and promotes a synthesized task experience note.
    agent
        .run_turn(
            json!("Issue: TypeError in Operation.get_type grouping"),
            None,
        )
        .await;
    agent.record_explicit_outcome_label(true, None).unwrap();
    assert_eq!(memory.entries().len(), 1);
    assert!(memory.entries()[0].text.contains("sqlparse/grouping.py"));

    // Turn 2: recalls Turn 1's note, makes an edit, then gets labeled bad.
    agent
        .run_turn(json!("Issue: Another grouping issue in sqlparse"), None)
        .await;
    assert_eq!(memory.entries().len(), 2);
    agent.record_explicit_outcome_label(false, None).unwrap();
    // Turn 2's own note is withdrawn from LEARNINGS.md, while Turn 1's note remains!
    assert_eq!(memory.entries().len(), 1);
    assert!(memory.entries()[0].text.contains("sqlparse/grouping.py"));
}

#[tokio::test]
async fn github_issue_title_and_shell_trial_filtering_avoid_prompt_prefix_pollution() {
    let ws = tmpdir("outcome_e2e_issue_title");
    let mut mem_store = crate::memory::MemoryStore::new(ws.join(".ncx").join("memory"));
    mem_store.set_feedback_mode("on");
    let memory = Rc::new(mem_store);
    let ctx = ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws))
        .with_memory(memory.clone());
    let session = Session::with_log(
        "system prompt",
        Some(ws.join(".nanocodex").join("sessions").join("s-title.jsonl")),
    );
    let p = ScriptedProvider::new(vec![
        // One-off `python -c` failure followed by `python -c` success must NOT become a recovery note.
        assistant_toolcall(vec![tc(
            "s1",
            "shell",
            json!({"command": "python -c \"raise ValueError('bad')\""}),
        )]),
        assistant_toolcall(vec![tc(
            "s2",
            "shell",
            json!({"command": "python -c \"print(1)\""}),
        )]),
        assistant_toolcall(vec![tc(
            "w1",
            "apply_patch",
            json!({"patch":"*** Begin Patch\n*** Add File: debug_324.py\n+x = 1\n*** Add File: sqlparse/sql.py\n+M_CLOSE = ('INTO',)\n*** Add File: tests/test_regressions.py\n+def test_324(): pass\n*** End Patch"}),
        )]),
        text("Fix applied.\n- Added INTO to sql.Where.M_CLOSE in sqlparse/sql.py."),
    ]);
    let mut agent =
        AgentLoop::new(Box::new(p), ToolRegistry::new(ctx), session).with_max_iterations(10);

    let replay_prompt = "You are working in a checkout of the `andialbrecht/sqlparse` Python project.\nResolve the following GitHub issue.\n\nGitHub issue #324:\n---\nkeyword INTO is not parsed properly\n\nDetails here.\n---";
    agent.run_turn(json!(replay_prompt), None).await;

    let entries = memory.entries();
    assert_eq!(
        entries.len(),
        1,
        "one-off python -c retry must not create a recovery note: {entries:?}"
    );
    assert!(
        entries[0]
            .text
            .contains("Task \"#324: keyword INTO is not parsed properly\""),
        "got: {}",
        entries[0].text
    );
    assert!(
        !entries[0].text.contains("You are working in a checkout"),
        "prompt prefix must not leak into note: {}",
        entries[0].text
    );
    assert!(
        !entries[0].text.contains("debug_324.py"),
        "scratch file must be excluded when real files exist: {}",
        entries[0].text
    );
    assert_eq!(entries[0].tags, vec!["sql".to_string()]);
}
