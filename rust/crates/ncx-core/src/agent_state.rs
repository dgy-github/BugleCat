//! Explicit lifecycle for one agent task.
//!
//! The model/tool loop may still contain implementation-specific substeps, but
//! every externally observable task now has one validated state vocabulary.
//! Transitions are driven by the runtime — approval outcomes, tool results,
//! cancellation — never by model prose, and each one carries the evidence that
//! caused it so the history can be replayed and audited.

use serde::{Deserialize, Serialize};

/// Origin of the turn authority recorded in `TaskOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritySource {
    #[default]
    DirectHuman,
    SubagentTask,
    GoalRound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Understand,
    Planned,
    AwaitingConfirmation,
    Authorized,
    Executing,
    Observing,
    Replanning,
    NeedsHuman,
    /// A specific permission or precondition was refused. Unlike `NeedsHuman`
    /// the task may continue along another route, so the model is told and
    /// execution resumes; it is recorded separately from a plain failure.
    Blocked,
    Failed,
    Cancelled,
    /// The outcome could not be observed (crash, lost worker, missing result).
    /// Never collapsed into success or failure.
    Unknown,
    Completed,
}

impl AgentState {
    pub fn as_str(self) -> &'static str {
        use AgentState::*;
        match self {
            Understand => "understand",
            Planned => "planned",
            AwaitingConfirmation => "awaiting_confirmation",
            Authorized => "authorized",
            Executing => "executing",
            Observing => "observing",
            Replanning => "replanning",
            NeedsHuman => "needs_human",
            Blocked => "blocked",
            Failed => "failed",
            Cancelled => "cancelled",
            Unknown => "unknown",
            Completed => "completed",
        }
    }

    /// True once the task cannot progress without new human input.
    pub fn is_terminal(self) -> bool {
        use AgentState::*;
        matches!(self, Completed | Failed | Cancelled | NeedsHuman)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        use AgentState::*;
        // Cancellation and loss of observation can happen at any live moment.
        if matches!(next, Cancelled | Unknown) && !self.is_terminal() {
            return true;
        }
        // An unobserved task can only be re-entered deliberately.
        if self == Unknown {
            return matches!(next, Understand | NeedsHuman | Cancelled);
        }
        matches!(
            (self, next),
            (Understand, Planned)
                | (Understand, NeedsHuman)
                | (Planned, AwaitingConfirmation)
                | (Planned, Authorized)
                | (Planned, Replanning)
                | (Planned, NeedsHuman)
                | (AwaitingConfirmation, Authorized)
                | (AwaitingConfirmation, Blocked)
                | (AwaitingConfirmation, NeedsHuman)
                | (Authorized, Executing)
                | (Authorized, NeedsHuman)
                // Approval is requested mid-execution by tools, not only up front.
                | (Executing, AwaitingConfirmation)
                | (Executing, Observing)
                | (Executing, NeedsHuman)
                | (Executing, Failed)
                // A refusal is reported back to the model, which may route around it.
                | (Blocked, Executing)
                | (Blocked, Observing)
                | (Blocked, NeedsHuman)
                | (Blocked, Failed)
                | (Observing, Completed)
                | (Observing, Replanning)
                | (Observing, NeedsHuman)
                | (Observing, Failed)
                | (Replanning, Planned)
                | (Replanning, NeedsHuman)
        )
    }
}

/// Why a transition happened. Recorded alongside the states so an audit can
/// distinguish "the model asked" from "the user denied" from "the tool failed".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: AgentState,
    pub to: AgentState,
    pub trigger: String,
    pub turn_id: Option<u64>,
    pub tool_call_id: Option<String>,
    /// `approved` / `denied` when an approval prompt caused this transition.
    pub permission_decision: Option<String>,
    pub workspace_revision: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStateMachine {
    current: AgentState,
    history: Vec<StateTransition>,
}

impl Default for AgentStateMachine {
    fn default() -> Self {
        Self {
            current: AgentState::Understand,
            history: Vec::new(),
        }
    }
}

impl AgentStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current(&self) -> AgentState {
        self.current
    }

    pub fn history(&self) -> &[StateTransition] {
        &self.history
    }

    pub fn transition(&mut self, next: AgentState) -> Result<(), String> {
        self.transition_with(next, "unspecified", None, None, None, None)
    }

    /// Apply a transition and record what caused it. An invalid transition
    /// changes nothing and is not recorded, so history never contains a step the
    /// machine refused.
    #[allow(clippy::too_many_arguments)]
    pub fn transition_with(
        &mut self,
        next: AgentState,
        trigger: &str,
        turn_id: Option<u64>,
        tool_call_id: Option<String>,
        permission_decision: Option<String>,
        workspace_revision: Option<String>,
    ) -> Result<(), String> {
        if !self.current.can_transition_to(next) {
            return Err(format!(
                "invalid agent state transition: {:?} -> {:?}",
                self.current, next
            ));
        }
        self.history.push(StateTransition {
            from: self.current,
            to: next,
            trigger: trigger.to_string(),
            turn_id,
            tool_call_id,
            permission_decision,
            workspace_revision,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or_default(),
        });
        self.current = next;
        Ok(())
    }
}

/// Owns the state machine for one agent and persists every accepted transition
/// as a `ContextEvent`. Shared (`Rc`) with the tool layer so an approval prompt
/// raised deep inside a tool moves the same task state the loop is driving,
/// instead of each layer keeping its own private idea of what is happening.
pub struct StateRecorder {
    machine: std::cell::RefCell<AgentStateMachine>,
    store: Option<std::rc::Rc<crate::context_store::ContextStore>>,
    session_id: String,
    workspace: std::path::PathBuf,
    turn_id: std::cell::Cell<Option<u64>>,
    workspace_revision: std::cell::RefCell<Option<String>>,
}

impl StateRecorder {
    pub fn new(
        session_id: impl Into<String>,
        workspace: impl Into<std::path::PathBuf>,
        store: Option<std::rc::Rc<crate::context_store::ContextStore>>,
    ) -> Self {
        Self {
            machine: std::cell::RefCell::new(AgentStateMachine::new()),
            store,
            session_id: session_id.into(),
            workspace: workspace.into(),
            turn_id: std::cell::Cell::new(None),
            workspace_revision: std::cell::RefCell::new(None),
        }
    }

    pub fn current(&self) -> AgentState {
        self.machine.borrow().current()
    }

    /// Revision read at the start of this turn. Shared so tool events anchor to
    /// the same tree state the lifecycle does.
    pub fn workspace_revision(&self) -> Option<String> {
        self.workspace_revision.borrow().clone()
    }

    pub fn history(&self) -> Vec<StateTransition> {
        self.machine.borrow().history().to_vec()
    }

    /// Start a new task: reset the machine and re-read the workspace revision
    /// once, so every transition in this turn is anchored to the same revision
    /// without spawning git per step.
    pub fn begin_turn(&self, turn_id: u64) {
        self.turn_id.set(Some(turn_id));
        *self.machine.borrow_mut() = AgentStateMachine::new();
        *self.workspace_revision.borrow_mut() = read_workspace_revision(&self.workspace);
    }

    pub fn record(&self, next: AgentState, trigger: &str) -> Result<(), String> {
        self.record_detailed(next, trigger, None, None)
    }

    pub fn record_detailed(
        &self,
        next: AgentState,
        trigger: &str,
        tool_call_id: Option<String>,
        permission_decision: Option<String>,
    ) -> Result<(), String> {
        let revision = self.workspace_revision.borrow().clone();
        let turn_id = self.turn_id.get();
        self.machine.borrow_mut().transition_with(
            next,
            trigger,
            turn_id,
            tool_call_id.clone(),
            permission_decision.clone(),
            revision.clone(),
        )?;
        let Some(store) = &self.store else {
            return Ok(());
        };
        let step = self
            .machine
            .borrow()
            .history()
            .last()
            .cloned()
            .expect("an accepted transition is always recorded");
        let event = crate::context_store::ContextEvent::new(
            format!(
                "state-{}-{}-{}",
                self.session_id,
                step.timestamp,
                next.as_str()
            ),
            self.session_id.clone(),
            crate::context_store::EventType::StateTransition,
            crate::context_store::EventStatus::Completed,
        )
        .with_turn(turn_id)
        .with_tool_call(tool_call_id)
        .with_workspace(Some(self.workspace.to_string_lossy().to_string()))
        .with_payload(&serde_json::json!({
            "from": step.from,
            "to": step.to,
            "trigger": step.trigger,
            "permission_decision": permission_decision,
            "workspace_revision": revision,
        }));
        let _ = store.append(&event);
        Ok(())
    }

    /// Record an approval outcome, including the refusal case. A denial does not
    /// end the task: it is reported to the model, which may take another route.
    pub fn record_approval(&self, tool_call_id: Option<String>, approved: bool) {
        let _ = self.record_detailed(
            AgentState::AwaitingConfirmation,
            "tool requested approval",
            tool_call_id.clone(),
            None,
        );
        let decision = if approved { "approved" } else { "denied" };
        let next = if approved {
            AgentState::Authorized
        } else {
            AgentState::Blocked
        };
        let _ = self.record_detailed(
            next,
            "approval resolved",
            tool_call_id,
            Some(decision.into()),
        );
        // Return to execution so the turn continues from a live state either way.
        let _ = self.record_detailed(AgentState::Executing, "resuming after approval", None, None);
    }

    /// Note an outcome the runtime could not observe. Kept distinct from both
    /// success and failure.
    pub fn record_unknown(&self, trigger: &str) {
        let _ = self.record_detailed(AgentState::Unknown, trigger, None, None);
    }

    /// Persist what a failed write actually left behind. This is evidence for a
    /// later decision, so it is recorded whether or not a substitute ran.
    pub fn record_write_probe(&self, tool: &str, effect: &crate::tool_recovery::WriteEffect) {
        let Some(store) = &self.store else {
            return;
        };
        let changed = match effect {
            crate::tool_recovery::WriteEffect::Partial(paths) => paths.clone(),
            _ => Vec::new(),
        };
        let payload = serde_json::json!({
            "tool": tool,
            "effect": effect.to_string(),
            "changed_paths": changed,
        });
        let event = crate::context_store::ContextEvent::new(
            format!(
                "probe-{}-{}-{}",
                self.session_id,
                tool,
                crate::context_trace::content_hash(&payload.to_string())
            ),
            self.session_id.clone(),
            crate::context_store::EventType::WriteProbe,
            // An unverifiable effect is recorded as `Unknown`, never as a
            // completed observation.
            match effect {
                crate::tool_recovery::WriteEffect::Unverifiable => {
                    crate::context_store::EventStatus::Unknown
                }
                _ => crate::context_store::EventStatus::Completed,
            },
        )
        .with_turn(self.turn_id.get())
        .with_workspace(Some(self.workspace.to_string_lossy().to_string()))
        .with_payload(&payload);
        let _ = store.append(&event);
    }
}

fn read_workspace_revision(workspace: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        return None;
    }
    Some(head)
}

/// Wraps the host's approval prompt so the shared task state follows the real
/// decision. It never changes the decision itself.
pub struct StateTrackingApprover {
    inner: std::rc::Rc<dyn crate::tools::ApprovalHandler>,
    recorder: std::rc::Rc<StateRecorder>,
}

impl StateTrackingApprover {
    pub fn new(
        inner: std::rc::Rc<dyn crate::tools::ApprovalHandler>,
        recorder: std::rc::Rc<StateRecorder>,
    ) -> Self {
        Self { inner, recorder }
    }
}

#[async_trait::async_trait(?Send)]
impl crate::tools::ApprovalHandler for StateTrackingApprover {
    async fn request(&self, req: crate::tools::ApprovalRequest) -> crate::tools::ApprovalDecision {
        // The prompt carries no tool_call_id, so the command identifies which
        // action the decision applies to.
        let subject = req.command.clone();
        let decision = self.inner.request(req).await;
        self.recorder.record_approval(
            Some(subject.chars().take(200).collect()),
            decision.approved(),
        );
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_execution_lifecycle() {
        let mut machine = AgentStateMachine::new();
        for state in [
            AgentState::Planned,
            AgentState::Authorized,
            AgentState::Executing,
            AgentState::Observing,
            AgentState::Completed,
        ] {
            machine.transition(state).unwrap();
        }
        assert_eq!(machine.current(), AgentState::Completed);
    }

    #[test]
    fn rejects_skipping_authorization_when_confirmation_is_required() {
        assert!(!AgentState::AwaitingConfirmation.can_transition_to(AgentState::Executing));
        assert!(!AgentState::Completed.can_transition_to(AgentState::Executing));
    }

    #[test]
    fn mid_execution_approval_round_trips_through_confirmation() {
        let mut machine = AgentStateMachine::new();
        for state in [
            AgentState::Planned,
            AgentState::Authorized,
            AgentState::Executing,
        ] {
            machine.transition(state).unwrap();
        }
        machine
            .transition_with(
                AgentState::AwaitingConfirmation,
                "tool approval requested",
                Some(3),
                Some("call-1".into()),
                None,
                None,
            )
            .unwrap();
        machine
            .transition_with(
                AgentState::Authorized,
                "approval granted",
                Some(3),
                Some("call-1".into()),
                Some("approved".into()),
                None,
            )
            .unwrap();
        machine.transition(AgentState::Executing).unwrap();

        let decisions: Vec<_> = machine
            .history()
            .iter()
            .filter_map(|step| step.permission_decision.as_deref())
            .collect();
        assert_eq!(decisions, vec!["approved"]);
        assert!(machine
            .history()
            .iter()
            .all(|step| step.tool_call_id.is_none()
                || step.tool_call_id.as_deref() == Some("call-1")));
    }

    #[test]
    fn denial_records_blocked_without_ending_the_task() {
        let mut machine = AgentStateMachine::new();
        for state in [
            AgentState::Planned,
            AgentState::Authorized,
            AgentState::Executing,
            AgentState::AwaitingConfirmation,
        ] {
            machine.transition(state).unwrap();
        }
        machine
            .transition_with(
                AgentState::Blocked,
                "approval denied",
                Some(1),
                Some("call-9".into()),
                Some("denied".into()),
                None,
            )
            .unwrap();
        assert!(!machine.current().is_terminal());
        // The model is told and may route around the refusal.
        machine.transition(AgentState::Executing).unwrap();
        assert_eq!(machine.history().last().unwrap().from, AgentState::Blocked);
    }

    #[test]
    fn cancellation_is_reachable_from_any_live_state_but_not_after_terminal() {
        let mut machine = AgentStateMachine::new();
        machine.transition(AgentState::Planned).unwrap();
        machine.transition(AgentState::Cancelled).unwrap();
        assert_eq!(machine.current(), AgentState::Cancelled);
        assert!(machine.transition(AgentState::Executing).is_err());

        let mut done = AgentStateMachine::new();
        for state in [
            AgentState::Planned,
            AgentState::Authorized,
            AgentState::Executing,
            AgentState::Observing,
            AgentState::Completed,
        ] {
            done.transition(state).unwrap();
        }
        assert!(done.transition(AgentState::Cancelled).is_err());
    }

    #[test]
    fn unknown_is_not_collapsed_into_success_or_failure() {
        let mut machine = AgentStateMachine::new();
        machine.transition(AgentState::Planned).unwrap();
        machine.transition(AgentState::Unknown).unwrap();
        assert!(machine.transition(AgentState::Completed).is_err());
        assert!(machine.transition(AgentState::Failed).is_err());
        machine.transition(AgentState::NeedsHuman).unwrap();
    }

    struct CannedApprover(bool);

    #[async_trait::async_trait(?Send)]
    impl crate::tools::ApprovalHandler for CannedApprover {
        async fn request(
            &self,
            _req: crate::tools::ApprovalRequest,
        ) -> crate::tools::ApprovalDecision {
            if self.0 {
                crate::tools::ApprovalDecision::Once
            } else {
                crate::tools::ApprovalDecision::Deny
            }
        }
    }

    /// Asks for approval from inside its own execution, like `shell` and
    /// `apply_patch` do.
    struct AskingTool;

    #[async_trait::async_trait(?Send)]
    impl crate::tools::Tool for AskingTool {
        fn name(&self) -> &str {
            "asking"
        }
        fn description(&self) -> &str {
            "Requests approval mid-execution."
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            ctx: &crate::tools::ToolContext,
            _args: &serde_json::Value,
        ) -> String {
            let Some(approver) = &ctx.approver else {
                return "Error: no approver".into();
            };
            let decision = approver
                .request(crate::tools::ApprovalRequest {
                    command: "rm -rf build".into(),
                    reason: "test".into(),
                    cwd: ".".into(),
                    escalated: true,
                    details: String::new(),
                })
                .await;
            if decision.approved() {
                "ran".into()
            } else {
                "Error: not approved by the user".into()
            }
        }
    }

    async fn run_approval_case(
        label: &str,
        approve: bool,
    ) -> (Vec<StateTransition>, std::path::PathBuf) {
        let workspace = crate::test_support::unique_temp_dir(label);
        std::fs::create_dir_all(&workspace).unwrap();
        let db = workspace.join("events.sqlite3");
        let recorder = std::rc::Rc::new(StateRecorder::new(
            "sess",
            workspace.clone(),
            Some(std::rc::Rc::new(
                crate::context_store::ContextStore::open(&db).unwrap(),
            )),
        ));
        recorder.begin_turn(1);
        recorder.record(AgentState::Planned, "t").unwrap();
        recorder.record(AgentState::Authorized, "t").unwrap();
        recorder.record(AgentState::Executing, "t").unwrap();

        let mut ctx = crate::tools::ToolContext::new(
            workspace.clone(),
            ncx_sandbox::SandboxPolicy::new(ncx_sandbox::WORKSPACE_WRITE, &workspace),
        );
        ctx.approver = Some(std::rc::Rc::new(CannedApprover(approve)));
        ctx.state_recorder = Some(recorder.clone());
        let mut registry = crate::tools::ToolRegistry::empty(ctx);
        registry.register(Box::new(AskingTool));

        registry.execute("asking", &serde_json::json!({})).await;
        (recorder.history(), db)
    }

    #[tokio::test]
    async fn a_denied_tool_approval_records_blocked_and_keeps_the_turn_live() {
        let (history, db) = run_approval_case("state_denied", false).await;

        let states: Vec<_> = history.iter().map(|step| step.to).collect();
        assert_eq!(
            &states[3..],
            &[
                AgentState::AwaitingConfirmation,
                AgentState::Blocked,
                AgentState::Executing
            ],
            "{states:?}"
        );
        let denial = history
            .iter()
            .find(|step| step.permission_decision.as_deref() == Some("denied"))
            .expect("the denial itself must be recorded");
        assert_eq!(denial.turn_id, Some(1));
        assert_eq!(denial.tool_call_id.as_deref(), Some("rm -rf build"));

        // The same transitions are durable, not just in memory.
        let store = crate::context_store::ContextStore::open(&db).unwrap();
        let persisted = store.session_events("sess").unwrap();
        assert!(persisted.iter().any(|event| event
            .payload_json
            .contains("\"permission_decision\":\"denied\"")));
        assert!(persisted
            .iter()
            .all(|event| event.event_type == crate::context_store::EventType::StateTransition));
    }

    #[tokio::test]
    async fn an_approved_tool_approval_records_authorized() {
        let (history, _) = run_approval_case("state_approved", true).await;
        let states: Vec<_> = history.iter().map(|step| step.to).collect();
        assert_eq!(
            &states[3..],
            &[
                AgentState::AwaitingConfirmation,
                AgentState::Authorized,
                AgentState::Executing
            ],
            "{states:?}"
        );
    }

    #[test]
    fn refused_transitions_are_not_recorded() {
        let mut machine = AgentStateMachine::new();
        assert!(machine.transition(AgentState::Completed).is_err());
        assert!(machine.history().is_empty());
        assert_eq!(machine.current(), AgentState::Understand);
    }
}
