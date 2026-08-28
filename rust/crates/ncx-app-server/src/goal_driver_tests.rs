use super::*;
use ncx_protocol::{GoalActivation, GoalPhase, GoalRef};

fn armed_goal(
    server: &AppServer<JsonThreadStore>,
    thread_id: &ThreadId,
    max_goal_rounds: u32,
) -> ncx_protocol::GoalView {
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
            objective: "finish the complete migration".into(),
            max_goal_rounds,
        })
        .unwrap();
    let ResponsePayload::Goal(Some(created)) = created.response.payload else {
        panic!("expected goal");
    };
    let resumed = server
        .dispatch(ClientRequest::GoalResume {
            thread_id: thread_id.clone(),
            goal: GoalRef {
                id: created.goal.id,
                revision: created.goal.revision,
            },
        })
        .unwrap();
    let ResponsePayload::Goal(Some(resumed)) = resumed.response.payload else {
        panic!("expected resumed goal");
    };
    resumed
}

fn read_goal(server: &AppServer<JsonThreadStore>, thread_id: &ThreadId) -> ncx_protocol::GoalView {
    let outcome = server
        .dispatch(ClientRequest::GoalRead {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    let ResponsePayload::Goal(Some(goal)) = outcome.response.payload else {
        panic!("expected goal");
    };
    goal
}

#[test]
fn checkpoint_failure_disarms_without_admitting_a_round() {
    let server = server();
    let thread_id = ThreadId::new("checkpoint-failure").unwrap();
    armed_goal(&server, &thread_id, 3);
    let result = GoalRoundDriver::new(&server).reserve_next(
        &thread_id,
        TurnId::new("round").unwrap(),
        || Err("disk unavailable".into()),
    );
    assert!(matches!(result, Err(AppServerError::Runtime(_))));
    assert_eq!(
        read_goal(&server, &thread_id).activation,
        GoalActivation::Disarmed
    );
    let thread = server
        .dispatch(ClientRequest::ThreadRead {
            thread_id: thread_id.clone(),
        })
        .unwrap();
    let ResponsePayload::Thread(thread) = thread.response.payload else {
        panic!("expected thread");
    };
    assert!(thread.turns.is_empty());
    assert_eq!(read_goal(&server, &thread_id).goal.rounds_started, 0);
}

#[test]
fn goal_change_during_checkpoint_prevents_stale_reservation() {
    let server = server();
    let thread_id = ThreadId::new("checkpoint-race").unwrap();
    let goal = armed_goal(&server, &thread_id, 3);
    let outcome = GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, TurnId::new("round").unwrap(), || {
            server
                .dispatch(ClientRequest::GoalPause {
                    thread_id: thread_id.clone(),
                    goal: GoalRef {
                        id: goal.goal.id.clone(),
                        revision: goal.goal.revision,
                    },
                })
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();
    assert_eq!(outcome, GoalRoundDriveOutcome::Idle);
    assert_eq!(read_goal(&server, &thread_id).goal.phase, GoalPhase::Paused);
}

#[test]
fn ordinary_turn_claimed_during_checkpoint_wins_over_goal_round() {
    let server = server();
    let thread_id = ThreadId::new("ordinary-wins").unwrap();
    armed_goal(&server, &thread_id, 3);
    let outcome = GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, TurnId::new("goal-round").unwrap(), || {
            server
                .dispatch(ClientRequest::TurnStart {
                    thread_id: thread_id.clone(),
                    turn_id: TurnId::new("human-turn").unwrap(),
                    execution_mode: ncx_protocol::ExecutionMode::Agent,
                })
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .unwrap();
    assert_eq!(outcome, GoalRoundDriveOutcome::CompetingTurn);
    assert_eq!(read_goal(&server, &thread_id).goal.rounds_started, 0);
}

#[test]
fn cancelled_reserved_round_pauses_and_disarms_goal() {
    let server = server();
    let thread_id = ThreadId::new("cancel-reserved").unwrap();
    let initial = armed_goal(&server, &thread_id, 3);
    let turn_id = TurnId::new("goal-round").unwrap();
    let reserved = GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, turn_id.clone(), || Ok(()))
        .unwrap();
    assert!(matches!(reserved, GoalRoundDriveOutcome::Reserved { .. }));
    GoalRoundDriver::new(&server)
        .cancel_reserved(
            &thread_id,
            turn_id,
            GoalRef {
                id: initial.goal.id,
                revision: initial.goal.revision,
            },
        )
        .unwrap();
    let goal = read_goal(&server, &thread_id);
    assert_eq!(goal.goal.phase, GoalPhase::Paused);
    assert_eq!(goal.activation, GoalActivation::Disarmed);
}

#[test]
fn exhausted_round_limit_blocks_without_reserving_another_turn() {
    let server = server();
    let thread_id = ThreadId::new("round-limit").unwrap();
    armed_goal(&server, &thread_id, 1);
    let first_turn = TurnId::new("round-1").unwrap();
    GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, first_turn.clone(), || Ok(()))
        .unwrap();
    server
        .dispatch(ClientRequest::TurnComplete {
            thread_id: thread_id.clone(),
            turn_id: first_turn,
            status: TurnStatus::Completed,
            error: None,
            usage: Default::default(),
        })
        .unwrap();
    let outcome = GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, TurnId::new("round-2").unwrap(), || Ok(()))
        .unwrap();
    let GoalRoundDriveOutcome::Blocked(goal) = outcome else {
        panic!("expected blocked round limit");
    };
    assert_eq!(goal.goal.phase, GoalPhase::Blocked);
    assert_eq!(goal.goal.blocked_reason.unwrap().code, "round-limit");
    assert_eq!(goal.activation, GoalActivation::Disarmed);
}

#[test]
fn queue_failure_finishes_turn_and_blocks_goal_without_raw_error() {
    let server = server();
    let thread_id = ThreadId::new("queue-failure").unwrap();
    let initial = armed_goal(&server, &thread_id, 3);
    let turn_id = TurnId::new("reserved").unwrap();
    GoalRoundDriver::new(&server)
        .reserve_next(&thread_id, turn_id.clone(), || Ok(()))
        .unwrap();
    GoalRoundDriver::new(&server)
        .fail_reserved(
            &thread_id,
            turn_id,
            GoalRef {
                id: initial.goal.id,
                revision: initial.goal.revision,
            },
            "queue-failed",
            "The local worker queue rejected the reserved round.",
        )
        .unwrap();
    let goal = read_goal(&server, &thread_id);
    assert_eq!(goal.goal.phase, GoalPhase::Blocked);
    assert_eq!(goal.goal.blocked_reason.unwrap().code, "queue-failed");
    let thread = server
        .dispatch(ClientRequest::ThreadRead { thread_id })
        .unwrap();
    let ResponsePayload::Thread(thread) = thread.response.payload else {
        panic!("expected thread");
    };
    assert_eq!(thread.turns[0].status, TurnStatus::Failed);
    assert_eq!(
        thread.turns[0].error.as_deref(),
        Some("automatic goal round could not start")
    );
}
