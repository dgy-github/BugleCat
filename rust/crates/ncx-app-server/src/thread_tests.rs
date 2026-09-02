use super::*;
use std::sync::{mpsc, Arc, Barrier};
use std::thread;

#[test]
fn create_and_start_turn_emit_owned_v3_events_and_mode() {
    let server = server();
    let created = server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: None,
            workspace: "workspace".into(),
            title: "title".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let ResponsePayload::Thread(thread) = created.response.payload else {
        panic!("expected created thread");
    };
    assert_eq!(created.events[0].thread_id, thread.metadata.id);
    assert_eq!(created.events[0].protocol_version, PROTOCOL_VERSION);

    let turn_id = TurnId::new("turn-1").unwrap();
    let started = server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread.metadata.id.clone(),
            turn_id: turn_id.clone(),
            execution_mode: ncx_protocol::ExecutionMode::Orchestrator,
        })
        .unwrap();
    assert_eq!(started.events[0].thread_id, thread.metadata.id);
    assert_eq!(started.events[0].turn_id, Some(turn_id));
    let stored = server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread.metadata.id,
        })
        .unwrap();
    let ResponsePayload::Thread(stored) = stored.response.payload else {
        panic!("expected thread");
    };
    assert_eq!(
        stored.turns[0].execution_mode,
        ncx_protocol::ExecutionMode::Orchestrator
    );
}

#[test]
fn second_concurrent_turn_is_rejected() {
    let server = server();
    let created = server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: None,
            workspace: "workspace".into(),
            title: "title".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let ResponsePayload::Thread(thread) = created.response.payload else {
        panic!("expected thread");
    };
    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread.metadata.id.clone(),
            turn_id: TurnId::new("one").unwrap(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .unwrap();
    assert!(server
        .dispatch(ClientRequest::TurnStart {
            thread_id: ThreadId::new(thread.metadata.id.as_str()).unwrap(),
            turn_id: TurnId::new("two").unwrap(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .is_err());
}

#[test]
fn rename_and_fork_are_owned_by_the_app_server() {
    let server = server();
    let thread_id = ThreadId::new("source").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "old".into(),
            harness_profile: "coding".into(),
        })
        .unwrap();
    let renamed = server
        .dispatch(ClientRequest::ThreadRename {
            thread_id: thread_id.clone(),
            title: "new title".into(),
        })
        .unwrap();
    assert!(matches!(
        renamed.events[0].event,
        Event::ThreadUpdated { .. }
    ));
    let forked = server
        .dispatch(ClientRequest::ThreadFork {
            thread_id,
            new_thread_id: ThreadId::new("target").unwrap(),
        })
        .unwrap();
    let ResponsePayload::Thread(forked_thread) = forked.response.payload else {
        panic!("expected forked thread");
    };
    assert_eq!(forked_thread.metadata.title, "new title");
    assert_eq!(forked_thread.metadata.harness_profile, "coding");
    assert_eq!(forked.events[0].thread_id.as_str(), "target");
}

#[test]
fn model_context_is_replaced_without_rewriting_visible_turns() {
    let server = server();
    let thread_id = ThreadId::new("thread").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "title".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let updated = server
        .dispatch(ClientRequest::ThreadModelContextReplace {
            thread_id: thread_id.clone(),
            messages: vec![serde_json::json!({"role":"user","content":"compact"})],
        })
        .unwrap();
    assert!(matches!(
        updated.events[0].event,
        Event::ModelContextUpdated { message_count: 1 }
    ));
    let read = server
        .dispatch(ClientRequest::ThreadModelContextRead { thread_id })
        .unwrap();
    let ResponsePayload::ModelContext(Some(context)) = read.response.payload else {
        panic!("expected stored model context");
    };
    assert_eq!(context.messages[0]["content"], "compact");
}

#[test]
fn visible_thread_never_returns_tool_logs_or_intermediate_assistant_text() {
    let server = server();
    let thread_id = ThreadId::new("visible").unwrap();
    let turn_id = TurnId::new("turn").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "title".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .unwrap();
    for item in [
        ThreadItem::UserMessage {
            id: ItemId::new("user").unwrap(),
            text: "请求".into(),
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("intermediate").unwrap(),
            text: "正在执行".into(),
            model: None,
            confirmed_model: None,
        },
        ThreadItem::ToolResult {
            id: ItemId::new("result").unwrap(),
            call_id: ItemId::new("call").unwrap(),
            output: "secret tool log".into(),
            success: true,
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("final").unwrap(),
            text: "最终结论".into(),
            model: None,
            confirmed_model: None,
        },
    ] {
        server
            .dispatch(ClientRequest::ItemAppend {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
                item,
            })
            .unwrap();
    }
    let visible = server
        .dispatch(ClientRequest::ThreadReadVisible { thread_id })
        .unwrap();
    let ResponsePayload::Thread(thread) = visible.response.payload else {
        panic!("expected visible thread");
    };
    assert_eq!(thread.turns[0].items.len(), 2);
    assert!(matches!(
        &thread.turns[0].items[1],
        ThreadItem::AssistantMessage { text, .. } if text == "最终结论"
    ));
}

#[test]
fn goal_lifecycle_is_revisioned_and_emits_durable_snapshots() {
    let server = server();
    let thread_id = ThreadId::new("goal-thread").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "  finish the harness migration  ".into(),
            max_goal_rounds: 8,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created_view)) = created.response.payload else {
        panic!("expected created goal");
    };
    assert_eq!(
        created_view.activation,
        ncx_protocol::GoalActivation::Disarmed
    );
    let created_goal = created_view.goal;
    assert_eq!(created_goal.objective, "finish the harness migration");
    assert_eq!(created_goal.revision, 1);
    assert!(matches!(
        created.events[0].event,
        Event::GoalChanged { goal: Some(_) }
    ));

    let paused = server
        .dispatch(ClientRequest::GoalPause {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: created_goal.id.clone(),
                revision: created_goal.revision,
            },
        })
        .unwrap();
    let ResponsePayload::Goal(Some(paused_view)) = paused.response.payload else {
        panic!("expected paused goal");
    };
    assert_eq!(
        paused_view.activation,
        ncx_protocol::GoalActivation::Disarmed
    );
    let paused_goal = paused_view.goal;
    assert_eq!(paused_goal.phase, ncx_protocol::GoalPhase::Paused);
    assert_eq!(paused_goal.revision, 2);

    let resumed = server
        .dispatch(ClientRequest::GoalResume {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: paused_goal.id.clone(),
                revision: paused_goal.revision,
            },
        })
        .unwrap();
    let ResponsePayload::Goal(Some(resumed_view)) = resumed.response.payload else {
        panic!("expected resumed goal");
    };
    assert_eq!(resumed_view.activation, ncx_protocol::GoalActivation::Armed);
    let resumed_goal = resumed_view.goal.clone();
    assert_eq!(resumed_goal.phase, ncx_protocol::GoalPhase::Active);
    assert_eq!(resumed_goal.revision, 3);

    let read = server
        .dispatch(ClientRequest::GoalRead { thread_id })
        .unwrap();
    assert_eq!(
        read.response.payload,
        ResponsePayload::Goal(Some(resumed_view))
    );
}

#[test]
fn goal_transition_lock_serializes_pause_against_an_in_flight_request() {
    let server = Arc::new(server());
    let thread_id = ThreadId::new("goal-transition-lock").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "serialize goal authority".into(),
            max_goal_rounds: 2,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(view)) = created.response.payload else {
        panic!("expected goal");
    };
    let goal = view.goal;
    server
        .dispatch(ClientRequest::GoalResume {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: goal.id.clone(),
                revision: goal.revision,
            },
        })
        .unwrap();

    // Hold the same lock used by Goal transitions, then start a competing
    // pause. A request that bypasses the lock (the pre-fix behavior) would
    // complete while this guard is held; the fixed path must remain blocked.
    let transition = server.lock_goal_transition().unwrap();
    let entered = Arc::new(Barrier::new(2));
    let (result_tx, result_rx) = mpsc::channel();
    let worker_server = Arc::clone(&server);
    let worker_entered = Arc::clone(&entered);
    let worker = thread::spawn(move || {
        worker_entered.wait();
        let result = worker_server.dispatch(ClientRequest::GoalPause {
            thread_id,
            goal: ncx_protocol::GoalRef {
                id: goal.id,
                revision: goal.revision,
            },
        });
        result_tx.send(result).unwrap();
    });
    entered.wait();
    assert!(result_rx.try_recv().is_err());
    drop(transition);

    result_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("pause must proceed after the transition lock is released")
        .unwrap();
    worker.join().unwrap();

    let read = server
        .dispatch(ClientRequest::GoalRead {
            thread_id: ThreadId::new("goal-transition-lock").unwrap(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(view)) = read.response.payload else {
        panic!("expected goal");
    };
    assert_eq!(view.goal.phase, ncx_protocol::GoalPhase::Paused);
    assert_eq!(view.activation, ncx_protocol::GoalActivation::Disarmed);
}

#[test]
fn stale_or_invalid_goal_transition_performs_no_mutation() {
    let server = server();
    let thread_id = ThreadId::new("goal-stale").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "objective".into(),
            max_goal_rounds: 4,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(goal_view)) = created.response.payload else {
        panic!("expected goal");
    };
    let goal = goal_view.goal;
    assert!(server
        .dispatch(ClientRequest::GoalBlock {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: goal.id.clone(),
                revision: goal.revision,
            },
            reason: ncx_protocol::GoalBlockReason {
                code: "".into(),
                message: "missing code".into(),
            },
        })
        .is_err());
    assert!(server
        .dispatch(ClientRequest::GoalPause {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: goal.id.clone(),
                revision: 99,
            },
        })
        .is_err());
    let current = server
        .dispatch(ClientRequest::GoalRead { thread_id })
        .unwrap();
    let ResponsePayload::Goal(Some(current)) = current.response.payload else {
        panic!("expected current goal");
    };
    assert_eq!(current.goal, goal);
    assert_eq!(current.activation, ncx_protocol::GoalActivation::Disarmed);
}

#[test]
fn completed_goal_can_be_replaced_but_active_goal_cannot() {
    let server = server();
    let thread_id = ThreadId::new("goal-replace").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let first = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "first".into(),
            max_goal_rounds: 3,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(first)) = first.response.payload else {
        panic!("expected first goal");
    };
    let first = first.goal;
    assert!(server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "too soon".into(),
            max_goal_rounds: 3,
        })
        .is_err());
    server
        .dispatch(ClientRequest::GoalComplete {
            thread_id: thread_id.clone(),
            goal: ncx_protocol::GoalRef {
                id: first.id.clone(),
                revision: first.revision,
            },
        })
        .unwrap();
    let replacement = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id,
            objective: "second".into(),
            max_goal_rounds: 2,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(replacement)) = replacement.response.payload else {
        panic!("expected replacement goal");
    };
    let replacement = replacement.goal;
    assert_ne!(replacement.id, first.id);
    assert_eq!(replacement.revision, 1);
}

#[test]
fn goal_activation_is_process_local_and_fork_never_inherits_it() {
    let path = std::env::temp_dir().join(format!(
        "ncx-goal-activation-{}-{}.json",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let source = ThreadId::new("activation-source").unwrap();
    let target = ThreadId::new("activation-target").unwrap();
    let server = AppServer::new(Arc::new(JsonThreadStore::open(&path).unwrap()), || 100);
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(source.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: source.clone(),
            objective: "persist state, not authority".into(),
            max_goal_rounds: 4,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected goal");
    };
    let revision = created.goal.revision;
    let armed = server
        .dispatch(ClientRequest::GoalResume {
            thread_id: source.clone(),
            goal: ncx_protocol::GoalRef {
                id: created.goal.id,
                revision,
            },
        })
        .unwrap();
    let ResponsePayload::Goal(Some(armed)) = armed.response.payload else {
        panic!("expected armed goal");
    };
    assert_eq!(armed.activation, ncx_protocol::GoalActivation::Armed);
    assert_eq!(armed.goal.revision, revision);

    server
        .dispatch(ClientRequest::ThreadFork {
            thread_id: source.clone(),
            new_thread_id: target.clone(),
        })
        .unwrap();
    let forked = server
        .dispatch(ClientRequest::GoalRead { thread_id: target })
        .unwrap();
    let ResponsePayload::Goal(Some(forked)) = forked.response.payload else {
        panic!("expected forked goal");
    };
    assert_eq!(forked.goal, armed.goal);
    assert_eq!(forked.activation, ncx_protocol::GoalActivation::Disarmed);

    drop(server);
    let reopened = AppServer::new(Arc::new(JsonThreadStore::open(path).unwrap()), || 200);
    let restored = reopened
        .dispatch(ClientRequest::GoalRead { thread_id: source })
        .unwrap();
    let ResponsePayload::Goal(Some(restored)) = restored.response.payload else {
        panic!("expected restored goal");
    };
    assert_eq!(restored.goal, armed.goal);
    assert_eq!(restored.activation, ncx_protocol::GoalActivation::Disarmed);
}

#[test]
fn goal_activation_token_cannot_authorize_a_replacement_from_another_server() {
    let path = std::env::temp_dir().join(format!(
        "ncx-goal-cross-server-{}-{}.json",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let server_a = AppServer::new(Arc::new(JsonThreadStore::open(&path).unwrap()), || 100);
    let server_b = AppServer::new(Arc::new(JsonThreadStore::open(&path).unwrap()), || 200);
    let thread_id = ThreadId::new("cross-server-goal").unwrap();

    server_a
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server_a
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "first durable objective".into(),
            max_goal_rounds: 2,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected first goal");
    };
    let first_ref = ncx_protocol::GoalRef {
        id: created.goal.id.clone(),
        revision: created.goal.revision,
    };
    let resumed = server_a
        .dispatch(ClientRequest::GoalResume {
            thread_id: thread_id.clone(),
            goal: first_ref.clone(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(resumed)) = resumed.response.payload else {
        panic!("expected resumed first goal");
    };
    assert_eq!(resumed.activation, ncx_protocol::GoalActivation::Armed);

    // The second App Server has no process-local token, but it can still
    // perform the durable lifecycle transition through the shared store.
    let completed = server_b
        .dispatch(ClientRequest::GoalComplete {
            thread_id: thread_id.clone(),
            goal: first_ref.clone(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(completed)) = completed.response.payload else {
        panic!("expected completed first goal");
    };
    assert_eq!(completed.goal.phase, ncx_protocol::GoalPhase::Complete);
    assert_eq!(completed.activation, ncx_protocol::GoalActivation::Disarmed);

    let replacement = server_b
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "replacement durable objective".into(),
            max_goal_rounds: 2,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(replacement)) = replacement.response.payload else {
        panic!("expected replacement goal");
    };
    assert_ne!(replacement.goal.id, first_ref.id);
    assert_eq!(replacement.goal.phase, ncx_protocol::GoalPhase::Active);
    assert_eq!(
        replacement.activation,
        ncx_protocol::GoalActivation::Disarmed
    );
    let replacement_ref = ncx_protocol::GoalRef {
        id: replacement.goal.id.clone(),
        revision: replacement.goal.revision,
    };

    // Server A still holds the old in-memory token. It must not authorize the
    // newly-created durable Goal, even before its next read refreshes state.
    let stale_round = server_a.dispatch(ClientRequest::GoalRoundStart {
        thread_id: thread_id.clone(),
        turn_id: TurnId::new("stale-replacement-round").unwrap(),
        goal: replacement_ref.clone(),
        round: 1,
        prompt: "must not run".into(),
    });
    assert!(matches!(
        stale_round,
        Err(AppServerError::InvalidRequest(message))
            if message.contains("goal continuation authorization is stale")
    ));

    let read = server_a
        .dispatch(ClientRequest::GoalRead {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(read)) = read.response.payload else {
        panic!("expected replacement goal from server A");
    };
    assert_eq!(read.goal, replacement.goal);
    assert_eq!(read.activation, ncx_protocol::GoalActivation::Disarmed);

    // The stale attempt above must not leave any authority behind, so a
    // subsequent admission is rejected as disarmed as well.
    let disarmed_round = server_a.dispatch(ClientRequest::GoalRoundStart {
        thread_id,
        turn_id: TurnId::new("disarmed-replacement-round").unwrap(),
        goal: replacement_ref,
        round: 1,
        prompt: "must still not run".into(),
    });
    assert!(matches!(
        disarmed_round,
        Err(AppServerError::InvalidRequest(message))
            if message.contains("goal continuation is disarmed")
    ));
}

#[test]
fn goal_round_requires_armed_exact_identity_and_claims_the_turn_atomically() {
    let server = server();
    let thread_id = ThreadId::new("round-thread").unwrap();
    server
        .dispatch(ClientRequest::ThreadCreate {
            thread_id: Some(thread_id.clone()),
            workspace: "workspace".into(),
            title: "goal round".into(),
            harness_profile: "full".into(),
        })
        .unwrap();
    let created = server
        .dispatch(ClientRequest::GoalCreate {
            thread_id: thread_id.clone(),
            objective: "continue safely".into(),
            max_goal_rounds: 2,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected goal");
    };
    let goal_ref = ncx_protocol::GoalRef {
        id: created.goal.id.clone(),
        revision: created.goal.revision,
    };
    assert!(server
        .dispatch(ClientRequest::GoalRoundStart {
            thread_id: thread_id.clone(),
            turn_id: TurnId::new("disarmed").unwrap(),
            goal: goal_ref.clone(),
            round: 1,
            prompt: "continue".into(),
        })
        .is_err());
    let before = server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    let ResponsePayload::Thread(before) = before.response.payload else {
        panic!("expected thread");
    };
    assert!(before.turns.is_empty());

    server
        .dispatch(ClientRequest::GoalResume {
            thread_id: thread_id.clone(),
            goal: goal_ref.clone(),
        })
        .unwrap();
    let started = server
        .dispatch(ClientRequest::GoalRoundStart {
            thread_id: thread_id.clone(),
            turn_id: TurnId::new("goal-turn").unwrap(),
            goal: goal_ref,
            round: 1,
            prompt: "Continue the exact durable objective.".into(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(started_goal)) = started.response.payload else {
        panic!("expected started goal");
    };
    assert_eq!(started_goal.goal.rounds_started, 1);
    assert_eq!(started.events.len(), 2);
    assert!(matches!(started.events[0].event, Event::TurnStarted { .. }));
    assert!(matches!(started.events[1].event, Event::GoalChanged { .. }));
    assert!(server
        .dispatch(ClientRequest::TurnStart {
            thread_id: thread_id.clone(),
            turn_id: TurnId::new("competing-user").unwrap(),
            execution_mode: ncx_protocol::ExecutionMode::Agent,
        })
        .is_err());
}
