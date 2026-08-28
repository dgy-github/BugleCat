use super::*;
use async_trait::async_trait;
use std::cell::{Cell, RefCell};

struct RecordingControl {
    events: RefCell<Vec<OrchestratorEvent>>,
    cancel_at: Option<OrchestratorStage>,
    cancelled: Cell<bool>,
}

impl OrchestratorControl for RecordingControl {
    fn emit(&self, event: OrchestratorEvent) {
        if self.cancel_at == Some(event.stage) {
            self.cancelled.set(true);
        }
        self.events.borrow_mut().push(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

struct MediumRunner {
    calls: Cell<usize>,
}

#[async_trait(?Send)]
impl AgentRunner for MediumRunner {
    async fn run(&self, _tier: Tier, system: &str, _task: &str) -> String {
        self.calls.set(self.calls.get() + 1);
        if system == CLASSIFY_SYS {
            "medium".into()
        } else if system == VERIFY_SYS {
            "PASS\nBEST:1".into()
        } else {
            "result".into()
        }
    }
}

#[tokio::test]
async fn emits_typed_progress_in_graph_order() {
    let runner = MediumRunner {
        calls: Cell::new(0),
    };
    let control = RecordingControl {
        events: RefCell::new(vec![]),
        cancel_at: None,
        cancelled: Cell::new(false),
    };
    let out = Orchestrator::new(&runner, OrchestratorConfig::default())
        .with_control(&control)
        .handle("task")
        .await;
    assert!(!out.cancelled);
    let stages = control
        .events
        .borrow()
        .iter()
        .map(|event| event.stage)
        .collect::<Vec<_>>();
    assert_eq!(
        stages,
        [
            OrchestratorStage::Classify,
            OrchestratorStage::Plan,
            OrchestratorStage::Workers,
            OrchestratorStage::Verify,
            OrchestratorStage::Promote
        ]
    );
}

#[tokio::test]
async fn cancellation_before_workers_stops_later_nodes_and_promotion() {
    let runner = MediumRunner {
        calls: Cell::new(0),
    };
    let control = RecordingControl {
        events: RefCell::new(vec![]),
        cancel_at: Some(OrchestratorStage::Workers),
        cancelled: Cell::new(false),
    };
    let out = Orchestrator::new(&runner, OrchestratorConfig::default())
        .with_control(&control)
        .handle("task")
        .await;
    assert!(out.cancelled);
    assert!(!out.verify_passed);
    assert_eq!(runner.calls.get(), 2, "only classify and plan may run");
    assert_eq!(out.telemetry.calls, 2);
    assert!(!control.events.borrow().iter().any(|event| matches!(
        event.stage,
        OrchestratorStage::Verify | OrchestratorStage::Promote
    )));
}
