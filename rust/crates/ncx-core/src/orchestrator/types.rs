use async_trait::async_trait;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorStage {
    Classify,
    Plan,
    Decompose,
    Workers,
    Verify,
    Promote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestratorEvent {
    pub stage: OrchestratorStage,
    pub tier: Option<Tier>,
    pub detail: String,
}

/// Host-owned progress and cancellation boundary. GUI implementations can
/// project events and share the same atomic cancel flag as ordinary turns.
pub trait OrchestratorControl {
    fn emit(&self, _event: OrchestratorEvent) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Structured evidence from one classify/plan/worker/verify model call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCallResult {
    pub text: String,
    pub usage: BTreeMap<String, i64>,
    pub requested_model: Option<String>,
    pub confirmed_model: Option<String>,
    pub cancelled: bool,
}

/// Aggregated evidence for the whole orchestration graph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrchestratorTelemetry {
    pub usage: BTreeMap<String, i64>,
    pub calls: usize,
    pub requested_models: Vec<String>,
    pub confirmed_models: Vec<String>,
    pub cancelled: bool,
}

#[async_trait(?Send)]
pub trait AgentRunner {
    async fn run(&self, tier: Tier, system: &str, task: &str) -> String;

    async fn run_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        AgentCallResult {
            text: self.run(tier, system, task).await,
            ..Default::default()
        }
    }

    async fn reason(&self, tier: Tier, system: &str, task: &str) -> String {
        self.run(tier, system, task).await
    }

    async fn reason_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        AgentCallResult {
            text: self.reason(tier, system, task).await,
            ..Default::default()
        }
    }

    async fn run_worker(&self, _idx: usize, _n: usize, system: &str, task: &str) -> String {
        self.run(Tier::Fast, system, task).await
    }

    async fn run_worker_result(
        &self,
        idx: usize,
        n: usize,
        system: &str,
        task: &str,
    ) -> AgentCallResult {
        AgentCallResult {
            text: self.run_worker(idx, n, system, task).await,
            ..Default::default()
        }
    }

    async fn promote_worker(&self, _idx: usize) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub workers: usize,
    pub high_workers: usize,
    pub max_verify_retries: usize,
    pub max_depth: usize,
    pub max_subtasks: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 6,
        }
    }
}

impl OrchestratorConfig {
    pub fn from_runtime_config(config: &ncx_config::Config) -> Self {
        Self {
            workers: config.orchestrator_workers.clamp(1, 4) as usize,
            high_workers: config.orchestrator_high_workers.clamp(1, 6) as usize,
            max_verify_retries: config.orchestrator_verify_retries.clamp(0, 3) as usize,
            max_depth: config.orchestrator_max_depth.clamp(0, 2) as usize,
            max_subtasks: config.orchestrator_max_subtasks.clamp(1, 12) as usize,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrchestratorOutcome {
    pub complexity: Complexity,
    pub final_text: String,
    pub plan: Option<String>,
    pub worker_results: Vec<String>,
    pub verify_passed: bool,
    pub cancelled: bool,
    pub promotion_error: Option<String>,
    pub verify_rounds: usize,
    pub best_worker: usize,
    pub telemetry: OrchestratorTelemetry,
}
