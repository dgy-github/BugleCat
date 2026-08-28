use super::*;
use ncx_protocol::{GoalActivation, GoalPhase, GoalSnapshot};
use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

struct RecordingGoalService {
    current: RefCell<Option<GoalView>>,
    calls: RefCell<Vec<String>>,
}

impl RecordingGoalService {
    fn new(rounds_started: u32) -> Self {
        Self {
            current: RefCell::new(Some(GoalView {
                goal: GoalSnapshot {
                    id: GoalId::new("goal-1").unwrap(),
                    revision: 4,
                    objective: "objective".into(),
                    phase: GoalPhase::Active,
                    blocked_reason: None,
                    max_goal_rounds: 8,
                    rounds_started,
                    created_at: 1,
                    updated_at: 2,
                },
                activation: GoalActivation::Armed,
            })),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, name: &str) -> Result<GoalView, String> {
        self.calls.borrow_mut().push(name.into());
        self.current
            .borrow()
            .clone()
            .ok_or_else(|| "missing goal".into())
    }
}

impl GoalToolService for RecordingGoalService {
    fn get(&self) -> Result<Option<GoalView>, String> {
        self.calls.borrow_mut().push("get".into());
        Ok(self.current.borrow().clone())
    }
    fn create(&self, _objective: String, _max: u32) -> Result<GoalView, String> {
        self.record("create")
    }
    fn edit(&self, _goal: GoalRef, _objective: String, _max: u32) -> Result<GoalView, String> {
        self.record("edit")
    }
    fn pause(&self, _goal: GoalRef) -> Result<GoalView, String> {
        self.record("pause")
    }
    fn resume(&self, _goal: GoalRef) -> Result<GoalView, String> {
        self.record("resume")
    }
    fn complete(&self, _goal: GoalRef) -> Result<GoalView, String> {
        self.record("complete")
    }
    fn block(&self, _goal: GoalRef, _reason: GoalBlockReason) -> Result<GoalView, String> {
        self.record("block")
    }
}

fn context(
    service: Rc<RecordingGoalService>,
    authority: Option<GoalAuthoritySource>,
) -> ToolContext {
    let workspace = PathBuf::from(".");
    let ctx = ToolContext::new(
        workspace.clone(),
        SandboxPolicy::new(WORKSPACE_WRITE, workspace),
    )
    .with_goal_service(service);
    ctx.active_turn_id.set(Some(7));
    ctx.goal_turn_authority
        .replace(authority.map(|source| GoalTurnAuthority { turn_id: 7, source }));
    ctx
}

fn take_tool(name: &str) -> Box<dyn Tool> {
    goal_tools()
        .into_iter()
        .find(|tool| tool.name() == name)
        .unwrap()
}

#[tokio::test]
async fn missing_or_stale_authority_is_rejected_before_service_access() {
    let service = Rc::new(RecordingGoalService::new(0));
    let ctx = context(service.clone(), None);
    let output = take_tool("get_goal").execute(&ctx, &json!({})).await;
    assert!(output.contains("host-attested"));
    assert!(service.calls.borrow().is_empty());

    ctx.goal_turn_authority.replace(Some(GoalTurnAuthority {
        turn_id: 6,
        source: GoalAuthoritySource::DirectHuman,
    }));
    let output = take_tool("create_goal")
        .execute(&ctx, &json!({"objective":"work"}))
        .await;
    assert!(output.contains("stale turn"));
    assert!(service.calls.borrow().is_empty());
}

#[tokio::test]
async fn goal_round_cannot_use_direct_human_mutations() {
    let service = Rc::new(RecordingGoalService::new(2));
    let ctx = context(
        service.clone(),
        Some(GoalAuthoritySource::GoalRound {
            goal_id: GoalId::new("goal-1").unwrap(),
            revision: 4,
            round: 2,
        }),
    );
    for (action, extras) in [
        ("edit", json!({"objective":"new","max_goal_rounds":8})),
        ("pause", json!({})),
        ("resume", json!({})),
    ] {
        let mut args = json!({"goal_id":"goal-1","revision":4,"action":action});
        args.as_object_mut()
            .unwrap()
            .extend(extras.as_object().unwrap().clone());
        let output = take_tool("update_goal").execute(&ctx, &args).await;
        assert!(output.contains("direct human"), "{output}");
    }
    assert!(service.calls.borrow().is_empty());
}

#[tokio::test]
async fn exact_goal_round_can_complete_but_stale_identity_cannot() {
    let service = Rc::new(RecordingGoalService::new(3));
    let ctx = context(
        service.clone(),
        Some(GoalAuthoritySource::GoalRound {
            goal_id: GoalId::new("goal-1").unwrap(),
            revision: 4,
            round: 3,
        }),
    );
    let output = take_tool("update_goal")
        .execute(
            &ctx,
            &json!({"goal_id":"goal-1","revision":4,"action":"complete"}),
        )
        .await;
    assert!(!output.starts_with("Error:"), "{output}");
    assert_eq!(&*service.calls.borrow(), &["get", "complete"]);

    service.calls.borrow_mut().clear();
    ctx.goal_turn_authority.replace(Some(GoalTurnAuthority {
        turn_id: 7,
        source: GoalAuthoritySource::GoalRound {
            goal_id: GoalId::new("goal-1").unwrap(),
            revision: 3,
            round: 3,
        },
    }));
    let output = take_tool("update_goal")
        .execute(
            &ctx,
            &json!({"goal_id":"goal-1","revision":4,"action":"complete"}),
        )
        .await;
    assert!(output.contains("does not match"));
    assert_eq!(&*service.calls.borrow(), &["get"]);
}

#[tokio::test]
async fn model_reported_block_requires_three_admitted_rounds() {
    let service = Rc::new(RecordingGoalService::new(2));
    let ctx = context(
        service.clone(),
        Some(GoalAuthoritySource::GoalRound {
            goal_id: GoalId::new("goal-1").unwrap(),
            revision: 4,
            round: 2,
        }),
    );
    let args = json!({
        "goal_id":"goal-1", "revision":4, "action":"blocked",
        "blocked_reason":"same external dependency is unavailable"
    });
    let output = take_tool("update_goal").execute(&ctx, &args).await;
    assert!(output.contains("at least 3 consecutive"));
    assert_eq!(&*service.calls.borrow(), &["get"]);
}
