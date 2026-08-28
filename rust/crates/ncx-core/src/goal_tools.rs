use crate::tools::{Tool, ToolContext};
use async_trait::async_trait;
use ncx_protocol::{GoalBlockReason, GoalId, GoalRef, GoalView};
use serde_json::{json, Value};

const DEFAULT_MAX_GOAL_ROUNDS: u32 = 16;
const BLOCKED_AFTER_CONSECUTIVE_ROUNDS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalAuthoritySource {
    DirectHuman,
    GoalRound {
        goal_id: GoalId,
        revision: u64,
        round: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalTurnAuthority {
    pub turn_id: u64,
    pub source: GoalAuthoritySource,
}

/// Thread-bound host adapter. Implementations must route mutations through the
/// App Server Goal state machine instead of writing Thread Store directly.
pub trait GoalToolService {
    fn get(&self) -> Result<Option<GoalView>, String>;
    fn create(&self, objective: String, max_goal_rounds: u32) -> Result<GoalView, String>;
    fn edit(
        &self,
        goal: GoalRef,
        objective: String,
        max_goal_rounds: u32,
    ) -> Result<GoalView, String>;
    fn pause(&self, goal: GoalRef) -> Result<GoalView, String>;
    fn resume(&self, goal: GoalRef) -> Result<GoalView, String>;
    fn complete(&self, goal: GoalRef) -> Result<GoalView, String>;
    fn block(&self, goal: GoalRef, reason: GoalBlockReason) -> Result<GoalView, String>;
}

pub fn goal_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(GetGoalTool),
        Box::new(CreateGoalTool),
        Box::new(UpdateGoalTool),
    ]
}

struct GetGoalTool;
struct CreateGoalTool;
struct UpdateGoalTool;

fn service(ctx: &ToolContext) -> Result<&dyn GoalToolService, String> {
    ctx.goal_service
        .as_deref()
        .ok_or_else(|| "goal service is unavailable".to_string())
}

fn authority(ctx: &ToolContext) -> Result<GoalTurnAuthority, String> {
    let active = ctx
        .active_turn_id
        .get()
        .ok_or_else(|| "goal tools require an active model turn".to_string())?;
    let authority = ctx
        .goal_turn_authority
        .borrow()
        .clone()
        .ok_or_else(|| "goal tools require host-attested turn authority".to_string())?;
    if authority.turn_id != active {
        return Err("goal tool authority belongs to a stale turn".to_string());
    }
    Ok(authority)
}

fn require_direct_human(ctx: &ToolContext) -> Result<(), String> {
    match authority(ctx)?.source {
        GoalAuthoritySource::DirectHuman => Ok(()),
        GoalAuthoritySource::GoalRound { .. } => {
            Err("this goal operation requires a direct human turn".to_string())
        }
    }
}

fn exact_ref(args: &Value) -> Result<GoalRef, String> {
    let id = args
        .get("goal_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "goal_id is required".to_string())?;
    if id.trim() != id || id.is_empty() {
        return Err("goal_id must be non-empty and trimmed".to_string());
    }
    let revision = args
        .get("revision")
        .and_then(Value::as_u64)
        .filter(|revision| *revision > 0)
        .ok_or_else(|| "revision must be a positive integer".to_string())?;
    Ok(GoalRef {
        id: GoalId::new(id).map_err(|error| error.to_string())?,
        revision,
    })
}

fn render(result: Result<Option<GoalView>, String>) -> String {
    match result {
        Ok(Some(goal)) => serde_json::to_string(&goal)
            .unwrap_or_else(|error| format!("Error: could not serialize goal result: {error}")),
        Ok(None) => "{\"goal\":null}".to_string(),
        Err(error) => format!("Error: {error}"),
    }
}

#[async_trait(?Send)]
impl Tool for GetGoalTool {
    fn name(&self) -> &str {
        "get_goal"
    }
    fn description(&self) -> &str {
        "Read the current persisted same-session goal and its process-local activation."
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn read_only(&self) -> bool {
        true
    }
    async fn execute(&self, ctx: &ToolContext, _args: &Value) -> String {
        if let Err(error) = authority(ctx) {
            return render(Err(error));
        }
        render(service(ctx).and_then(GoalToolService::get))
    }
}

#[async_trait(?Send)]
impl Tool for CreateGoalTool {
    fn name(&self) -> &str {
        "create_goal"
    }
    fn description(&self) -> &str {
        "Create a persisted same-session goal for a genuinely long-running direct human request. Automatic continuation remains disarmed until explicitly resumed."
    }
    fn parameters(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "objective":{"type":"string"},
                "max_goal_rounds":{"type":"integer","minimum":1}
            },
            "required":["objective"]
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        if let Err(error) = require_direct_human(ctx) {
            return render(Err(error));
        }
        let objective = match args.get("objective").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.to_string(),
            _ => return render(Err("objective must not be empty".into())),
        };
        let max = match args.get("max_goal_rounds") {
            None => DEFAULT_MAX_GOAL_ROUNDS,
            Some(value) => match value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
            {
                Some(value) => value,
                None => return render(Err("max_goal_rounds must be a positive integer".into())),
            },
        };
        render(
            service(ctx)
                .and_then(|service| service.create(objective, max))
                .map(Some),
        )
    }
}

#[async_trait(?Send)]
impl Tool for UpdateGoalTool {
    fn name(&self) -> &str {
        "update_goal"
    }
    fn description(&self) -> &str {
        "Update the exact current goal revision. edit/pause/resume require a direct human turn; complete/blocked also accept the exact admitted goal round."
    }
    fn parameters(&self) -> Value {
        json!({
            "type":"object",
            "properties":{
                "goal_id":{"type":"string"},
                "revision":{"type":"integer","minimum":1},
                "action":{"type":"string","enum":["edit","pause","resume","complete","blocked"]},
                "objective":{"type":"string"},
                "max_goal_rounds":{"type":"integer","minimum":1},
                "blocked_reason":{"type":"string"}
            },
            "required":["goal_id","revision","action"]
        })
    }
    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        render(update(ctx, args).map(Some))
    }
}

fn update(ctx: &ToolContext, args: &Value) -> Result<GoalView, String> {
    let goal_ref = exact_ref(args)?;
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "action is required".to_string())?;
    let goal_service = service(ctx)?;
    if matches!(action, "edit" | "pause" | "resume") {
        require_direct_human(ctx)?;
    } else if matches!(action, "complete" | "blocked") {
        match authority(ctx)?.source {
            GoalAuthoritySource::DirectHuman => {}
            GoalAuthoritySource::GoalRound {
                goal_id,
                revision,
                round,
            } => {
                let current = goal_service
                    .get()?
                    .ok_or_else(|| "current goal was not found".to_string())?;
                if goal_id != current.goal.id
                    || revision != current.goal.revision
                    || round != current.goal.rounds_started
                    || goal_ref.id != goal_id
                    || goal_ref.revision != revision
                {
                    return Err(
                        "goal round authority does not match the exact live goal revision".into(),
                    );
                }
                if action == "blocked" && round < BLOCKED_AFTER_CONSECUTIVE_ROUNDS {
                    return Err(format!("blocked requires at least {BLOCKED_AFTER_CONSECUTIVE_ROUNDS} consecutive goal rounds; current round is {round}"));
                }
            }
        }
    } else {
        return Err("action must be edit, pause, resume, complete, or blocked".into());
    }

    match action {
        "edit" => {
            let objective = args
                .get("objective")
                .and_then(Value::as_str)
                .ok_or_else(|| "objective is required with action edit".to_string())?;
            let max = args
                .get("max_goal_rounds")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| "max_goal_rounds is required with action edit".to_string())?;
            goal_service.edit(goal_ref, objective.to_string(), max)
        }
        "pause" => goal_service.pause(goal_ref),
        "resume" => goal_service.resume(goal_ref),
        "complete" => goal_service.complete(goal_ref),
        "blocked" => {
            let message = args
                .get("blocked_reason")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "blocked_reason is required with action blocked".to_string())?;
            goal_service.block(
                goal_ref,
                GoalBlockReason {
                    code: "model-reported".into(),
                    message: message.trim().into(),
                },
            )
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
#[path = "goal_tools_tests.rs"]
mod tests;
