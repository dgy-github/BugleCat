use crate::{AppServer, AppServerError};
use ncx_protocol::{
    ClientRequest, GoalBlockReason, GoalPhase, GoalRef, GoalView, ResponsePayload, ThreadId,
    TurnId, TurnStatus,
};
use ncx_thread_store::{ThreadStore, ThreadStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalRoundDriveOutcome {
    Idle,
    CompetingTurn,
    Reserved {
        turn_id: TurnId,
        goal: GoalView,
        prompt: String,
    },
    Blocked(GoalView),
}

/// Serialized, no-model-call coordinator for one automatic Goal round.
/// Frontends own the actual queue/task; this owner provides checkpoint and
/// race fences around the App Server's atomic admission transaction.
pub struct GoalRoundDriver<'a, S: ThreadStore> {
    server: &'a AppServer<S>,
}

impl<'a, S: ThreadStore> GoalRoundDriver<'a, S> {
    pub fn new(server: &'a AppServer<S>) -> Self {
        Self { server }
    }

    pub fn reserve_next(
        &self,
        thread_id: &ThreadId,
        turn_id: TurnId,
        checkpoint: impl FnOnce() -> Result<(), String>,
    ) -> Result<GoalRoundDriveOutcome, AppServerError> {
        let Some(before) = self.read(thread_id)? else {
            return Ok(GoalRoundDriveOutcome::Idle);
        };
        if before.goal.phase != GoalPhase::Active
            || before.activation != ncx_protocol::GoalActivation::Armed
        {
            return Ok(GoalRoundDriveOutcome::Idle);
        }
        if before.goal.rounds_started >= before.goal.max_goal_rounds {
            let blocked = self.server.dispatch(ClientRequest::GoalBlock {
                thread_id: thread_id.clone(),
                goal: goal_ref(&before),
                reason: GoalBlockReason {
                    code: "round-limit".into(),
                    message: format!(
                        "Goal reached its configured limit of {} rounds.",
                        before.goal.max_goal_rounds
                    ),
                },
            })?;
            return Ok(GoalRoundDriveOutcome::Blocked(expect_goal(blocked)?));
        }

        if let Err(error) = checkpoint() {
            // The checkpoint may yield to another Goal transition. Revoke
            // only the authority token we observed, so a newer explicit
            // resume cannot be cancelled by this stale worker.
            let _ = self
                .server
                .disarm_goal_if_matches(thread_id, &goal_ref(&before))?;
            return Err(AppServerError::Runtime(format!(
                "goal durability checkpoint failed: {error}"
            )));
        }

        let Some(after) = self.read(thread_id)? else {
            return Ok(GoalRoundDriveOutcome::Idle);
        };
        if after != before {
            return Ok(GoalRoundDriveOutcome::Idle);
        }
        let round = after.goal.rounds_started + 1;
        let prompt = render_prompt(&after, round);
        let started = self.server.dispatch(ClientRequest::GoalRoundStart {
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            goal: goal_ref(&after),
            round,
            prompt: prompt.clone(),
        });
        match started {
            Ok(outcome) => Ok(GoalRoundDriveOutcome::Reserved {
                turn_id,
                goal: expect_goal(outcome)?,
                prompt,
            }),
            Err(AppServerError::Store(ThreadStoreError::Busy { .. }))
            | Err(AppServerError::Store(ThreadStoreError::LeaseBusy(_))) => {
                Ok(GoalRoundDriveOutcome::CompetingTurn)
            }
            Err(AppServerError::Store(ThreadStoreError::StaleGoal { .. }))
            | Err(AppServerError::Store(ThreadStoreError::InvalidGoalRound(_))) => {
                Ok(GoalRoundDriveOutcome::Idle)
            }
            Err(error) => {
                let _ = self
                    .server
                    .disarm_goal_if_matches(thread_id, &goal_ref(&after))?;
                Err(error)
            }
        }
    }

    pub fn cancel_reserved(
        &self,
        thread_id: &ThreadId,
        turn_id: TurnId,
        goal: GoalRef,
    ) -> Result<(), AppServerError> {
        self.finish_turn(thread_id, turn_id, TurnStatus::Cancelled, None)?;
        let expected = goal.clone();
        match self.server.dispatch(ClientRequest::GoalPause {
            thread_id: thread_id.clone(),
            goal,
        }) {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = self.server.disarm_goal_if_matches(thread_id, &expected)?;
                Err(error)
            }
        }
    }

    pub fn fail_reserved(
        &self,
        thread_id: &ThreadId,
        turn_id: TurnId,
        goal: GoalRef,
        code: &str,
        message: &str,
    ) -> Result<(), AppServerError> {
        self.finish_turn(
            thread_id,
            turn_id,
            TurnStatus::Failed,
            Some("automatic goal round could not start".into()),
        )?;
        let expected = goal.clone();
        match self.server.dispatch(ClientRequest::GoalBlock {
            thread_id: thread_id.clone(),
            goal,
            reason: GoalBlockReason {
                code: code.trim().to_string(),
                message: message.trim().to_string(),
            },
        }) {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = self.server.disarm_goal_if_matches(thread_id, &expected)?;
                Err(error)
            }
        }
    }

    fn read(&self, thread_id: &ThreadId) -> Result<Option<GoalView>, AppServerError> {
        let outcome = self.server.dispatch(ClientRequest::GoalRead {
            thread_id: thread_id.clone(),
        })?;
        match outcome.response.payload {
            ResponsePayload::Goal(goal) => Ok(goal),
            _ => Err(AppServerError::Runtime(
                "goal read returned an unexpected response".into(),
            )),
        }
    }

    fn finish_turn(
        &self,
        thread_id: &ThreadId,
        turn_id: TurnId,
        status: TurnStatus,
        error: Option<String>,
    ) -> Result<(), AppServerError> {
        self.server.dispatch(ClientRequest::TurnComplete {
            thread_id: thread_id.clone(),
            turn_id,
            status,
            error,
            usage: Default::default(),
        })?;
        Ok(())
    }
}

fn goal_ref(view: &GoalView) -> GoalRef {
    GoalRef {
        id: view.goal.id.clone(),
        revision: view.goal.revision,
    }
}

fn expect_goal(outcome: crate::DispatchOutcome) -> Result<GoalView, AppServerError> {
    match outcome.response.payload {
        ResponsePayload::Goal(Some(goal)) => Ok(goal),
        _ => Err(AppServerError::Runtime(
            "goal mutation returned an unexpected response".into(),
        )),
    }
}

fn render_prompt(goal: &GoalView, round: u32) -> String {
    format!(
        "Continue the persisted objective below in the same session.\n\nObjective: {}\nGoal round: {round}/{}\n\nMake concrete progress, preserve existing work, and use get_goal before any update_goal call. Do not report completion unless the full objective is actually achieved.",
        goal.goal.objective, goal.goal.max_goal_rounds
    )
}
