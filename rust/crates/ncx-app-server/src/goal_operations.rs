use crate::{AppServer, AppServerError, DispatchOutcome};
use ncx_protocol::{
    ClientRequest, Event, ExecutionMode, GoalBlockReason, GoalId, GoalPhase, GoalRef, GoalSnapshot,
    ItemId, ResponsePayload, ThreadId, ThreadItem, Turn, TurnStatus,
};
use ncx_thread_store::{GoalExpectation, ThreadStore};
use std::sync::atomic::Ordering;

pub(crate) fn dispatch<S: ThreadStore>(
    server: &AppServer<S>,
    request: ClientRequest,
) -> Result<DispatchOutcome, AppServerError> {
    match request {
        ClientRequest::GoalRead { thread_id } => {
            server.read_thread(&thread_id)?;
            let goal = server
                .store
                .read_goal(&thread_id)?
                .map(|goal| server.goal_view(&thread_id, goal))
                .transpose()?;
            Ok(server.outcome(ResponsePayload::Goal(goal), Vec::new()))
        }
        ClientRequest::GoalCreate {
            thread_id,
            objective,
            max_goal_rounds,
        } => create(server, thread_id, objective, max_goal_rounds),
        ClientRequest::GoalEdit {
            thread_id,
            goal,
            objective,
            max_goal_rounds,
        } => mutate(
            server,
            thread_id,
            goal,
            ActivationUpdate::Keep,
            |current, now| {
                require_not_complete(current)?;
                validate_definition(&objective, max_goal_rounds, current.rounds_started)?;
                current.objective = objective.trim().to_string();
                current.max_goal_rounds = max_goal_rounds;
                advance(current, now);
                Ok(())
            },
        ),
        ClientRequest::GoalPause { thread_id, goal } => mutate(
            server,
            thread_id,
            goal,
            ActivationUpdate::Disarm,
            |current, now| {
                require_phase(current, GoalPhase::Active, "pause")?;
                current.phase = GoalPhase::Paused;
                current.blocked_reason = None;
                advance(current, now);
                Ok(())
            },
        ),
        ClientRequest::GoalResume { thread_id, goal } => {
            mutate(
                server,
                thread_id,
                goal,
                ActivationUpdate::Arm,
                |current, now| {
                    if current.phase == GoalPhase::Active {
                        // Resuming an active durable goal after restart/fork only
                        // restores process-local authority; revision stays stable.
                        return Ok(());
                    }
                    if !matches!(current.phase, GoalPhase::Paused | GoalPhase::Blocked) {
                        return invalid("only active, paused, or blocked goals can be resumed");
                    }
                    current.phase = GoalPhase::Active;
                    current.blocked_reason = None;
                    advance(current, now);
                    Ok(())
                },
            )
        }
        ClientRequest::GoalBlock {
            thread_id,
            goal,
            reason,
        } => mutate(
            server,
            thread_id,
            goal,
            ActivationUpdate::Disarm,
            |current, now| {
                require_phase(current, GoalPhase::Active, "block")?;
                validate_block_reason(&reason)?;
                current.phase = GoalPhase::Blocked;
                current.blocked_reason = Some(GoalBlockReason {
                    code: reason.code.trim().to_string(),
                    message: reason.message.trim().to_string(),
                });
                advance(current, now);
                Ok(())
            },
        ),
        ClientRequest::GoalComplete { thread_id, goal } => mutate(
            server,
            thread_id,
            goal,
            ActivationUpdate::Disarm,
            |current, now| {
                require_not_complete(current)?;
                current.phase = GoalPhase::Complete;
                current.blocked_reason = None;
                advance(current, now);
                Ok(())
            },
        ),
        ClientRequest::GoalClear { thread_id, goal } => clear(server, thread_id, goal),
        ClientRequest::GoalRoundStart {
            thread_id,
            turn_id,
            goal,
            round,
            prompt,
        } => start_round(server, thread_id, turn_id, goal, round, prompt),
        _ => unreachable!("goal dispatcher received another request"),
    }
}

fn start_round<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    turn_id: ncx_protocol::TurnId,
    expected: GoalRef,
    round: u32,
    prompt: String,
) -> Result<DispatchOutcome, AppServerError> {
    if prompt.trim().is_empty() {
        return invalid("goal round prompt must not be empty");
    }
    let started_at = (server.clock)();
    let turn = Turn {
        id: turn_id.clone(),
        status: TurnStatus::Running,
        execution_mode: ExecutionMode::Agent,
        items: vec![ThreadItem::GoalMessage {
            id: ItemId::new(format!("goal-message-{turn_id}"))?,
            text: prompt,
            goal_id: expected.id.clone(),
            revision: expected.revision,
            round,
        }],
        started_at,
        completed_at: None,
        error: None,
        usage: Default::default(),
    };
    let activations = server
        .goal_activations
        .lock()
        .map_err(|_| AppServerError::Runtime("goal activation lock is poisoned".into()))?;
    if !activations.contains(thread_id.as_str()) {
        return invalid("goal continuation is disarmed");
    }
    let snapshot = server
        .store
        .claim_goal_round(&thread_id, expected, round, turn)?;
    drop(activations);
    let view = server.goal_view(&thread_id, snapshot)?;
    let events = vec![
        server.event(
            thread_id.clone(),
            Some(turn_id),
            Event::TurnStarted {
                status: TurnStatus::Running,
            },
        ),
        server.event(
            thread_id,
            None,
            Event::GoalChanged {
                goal: Some(view.clone()),
            },
        ),
    ];
    Ok(server.outcome(ResponsePayload::Goal(Some(view)), events))
}

fn create<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    objective: String,
    max_goal_rounds: u32,
) -> Result<DispatchOutcome, AppServerError> {
    server.read_thread(&thread_id)?;
    validate_definition(&objective, max_goal_rounds, 0)?;
    let current = server.store.read_goal(&thread_id)?;
    let expected = match current {
        None => GoalExpectation::Absent,
        Some(ref goal) if goal.phase == GoalPhase::Complete => GoalExpectation::Exact(GoalRef {
            id: goal.id.clone(),
            revision: goal.revision,
        }),
        Some(_) => return invalid("finish or clear the current goal before creating another"),
    };
    let now = (server.clock)();
    let goal = GoalSnapshot {
        id: GoalId::new(format!(
            "goal-{now}-{}",
            server.sequence.fetch_add(1, Ordering::Relaxed)
        ))?,
        revision: 1,
        objective: objective.trim().to_string(),
        phase: GoalPhase::Active,
        blocked_reason: None,
        max_goal_rounds,
        rounds_started: 0,
        created_at: now,
        updated_at: now,
    };
    server
        .store
        .compare_and_set_goal(&thread_id, expected, Some(goal.clone()))?;
    changed(server, thread_id, Some(goal), ActivationUpdate::Disarm)
}

#[derive(Clone, Copy)]
enum ActivationUpdate {
    Keep,
    Arm,
    Disarm,
}

fn mutate<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    expected: GoalRef,
    activation: ActivationUpdate,
    update: impl FnOnce(&mut GoalSnapshot, i64) -> Result<(), AppServerError>,
) -> Result<DispatchOutcome, AppServerError> {
    let mut current = server
        .store
        .read_goal(&thread_id)?
        .ok_or_else(|| AppServerError::NotFound(expected.id.to_string()))?;
    if current.id != expected.id || current.revision != expected.revision {
        // Let the store produce the canonical stale-reference error under its
        // atomic lock instead of manufacturing a racy App Server result.
        server.store.compare_and_set_goal(
            &thread_id,
            GoalExpectation::Exact(expected),
            Some(current.clone()),
        )?;
        unreachable!("mismatched goal reference must fail compare-and-set")
    }
    update(&mut current, (server.clock)())?;
    server.store.compare_and_set_goal(
        &thread_id,
        GoalExpectation::Exact(expected),
        Some(current.clone()),
    )?;
    changed(server, thread_id, Some(current), activation)
}

fn clear<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    expected: GoalRef,
) -> Result<DispatchOutcome, AppServerError> {
    server
        .store
        .compare_and_set_goal(&thread_id, GoalExpectation::Exact(expected), None)?;
    changed(server, thread_id, None, ActivationUpdate::Disarm)
}

fn changed<S: ThreadStore>(
    server: &AppServer<S>,
    thread_id: ThreadId,
    goal: Option<GoalSnapshot>,
    activation: ActivationUpdate,
) -> Result<DispatchOutcome, AppServerError> {
    match activation {
        ActivationUpdate::Keep => {}
        ActivationUpdate::Arm => server.arm_goal(&thread_id)?,
        ActivationUpdate::Disarm => server.disarm_goal(&thread_id)?,
    }
    let view = goal
        .map(|goal| server.goal_view(&thread_id, goal))
        .transpose()?;
    let event = server.event(thread_id, None, Event::GoalChanged { goal: view.clone() });
    Ok(server.outcome(ResponsePayload::Goal(view), vec![event]))
}

fn advance(goal: &mut GoalSnapshot, now: i64) {
    goal.revision = goal.revision.saturating_add(1);
    goal.updated_at = now;
}

fn validate_definition(
    objective: &str,
    max_goal_rounds: u32,
    rounds_started: u32,
) -> Result<(), AppServerError> {
    if objective.trim().is_empty() {
        return invalid("goal objective must not be empty");
    }
    if max_goal_rounds == 0 {
        return invalid("maxGoalRounds must be positive");
    }
    if max_goal_rounds < rounds_started {
        return invalid("maxGoalRounds cannot be lower than roundsStarted");
    }
    Ok(())
}

fn validate_block_reason(reason: &GoalBlockReason) -> Result<(), AppServerError> {
    if reason.code.trim().is_empty() || reason.message.trim().is_empty() {
        return invalid("goal block reason code and message must not be empty");
    }
    Ok(())
}

fn require_not_complete(goal: &GoalSnapshot) -> Result<(), AppServerError> {
    if goal.phase == GoalPhase::Complete {
        return invalid("completed goals cannot be changed");
    }
    Ok(())
}

fn require_phase(
    goal: &GoalSnapshot,
    phase: GoalPhase,
    operation: &str,
) -> Result<(), AppServerError> {
    if goal.phase != phase {
        return invalid(format!("cannot {operation} a {:?} goal", goal.phase));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, AppServerError> {
    Err(AppServerError::InvalidRequest(message.into()))
}
