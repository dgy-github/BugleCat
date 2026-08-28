//! Tiered flash+pro orchestration — a node graph that scales effort to task risk.
//!
//! The bottleneck for capability is the model, not the harness (see the project
//! notes), so this squeezes more reliability out of a cheap/strong model pair by
//! spending the cost savings on structure:
//!
//! ```text
//! classify (fast)
//!   ├─ Simple → single run (fast)
//!   ├─ Medium → plan (main) → workers×N (fast, parallel) → verify (fast)  ┐
//!   └─ High   → plan (main) → decompose (main)                            │
//!                  ├─ atomic        → workers×M (fast, parallel) → verify (main) ┘
//!                  └─ ≥2 subtasks   → for each: recurse(handle_at, depth+1)        (sequential,
//!                                     → verify (main)                               each promotes)
//!                                         ▲                         │
//!                                         └──── FAIL: retry ────────┘  (closed loop, ≤ max_verify_retries)
//! ```
//!
//! It cannot exceed the *main* model's reasoning ceiling (plan + verify run
//! there); the gains are completion-rate / reliability on simple+medium tasks
//! and divide-and-conquer reach on high ones. Model calls are abstracted behind
//! [`AgentRunner`] so this module is provider-agnostic and unit-testable.
//!
//! Recursion is live-safe because workers run in isolated workspace copies and
//! the verifier-chosen winner is promoted to the real workspace before the next
//! subtask starts — so sequential subtasks see each other's committed work
//! without parallel-write collisions (see `cli/runner.rs`).

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
mod control_tests;
use futures_util::future::{join_all, LocalBoxFuture};
use futures_util::FutureExt;
use std::cell::RefCell;

mod types;
pub use types::*;
mod support;
use support::*;

/// Drives the tiered node graph over an [`AgentRunner`].
pub struct Orchestrator<'a> {
    runner: &'a dyn AgentRunner,
    cfg: OrchestratorConfig,
    telemetry: RefCell<OrchestratorTelemetry>,
    control: Option<&'a dyn OrchestratorControl>,
}

impl<'a> Orchestrator<'a> {
    pub fn new(runner: &'a dyn AgentRunner, cfg: OrchestratorConfig) -> Self {
        Orchestrator {
            runner,
            cfg,
            telemetry: RefCell::new(OrchestratorTelemetry::default()),
            control: None,
        }
    }

    pub fn with_control(mut self, control: &'a dyn OrchestratorControl) -> Self {
        self.control = Some(control);
        self
    }

    /// Classify the task, then run the matching node graph (depth 0).
    pub async fn handle(&self, task: &str) -> OrchestratorOutcome {
        *self.telemetry.borrow_mut() = OrchestratorTelemetry::default();
        if self.is_cancelled() {
            return self.cancelled_outcome(Complexity::Medium, None, vec![]);
        }
        self.handle_at(task.to_string(), 0).await
    }

    fn emit_stage(&self, stage: OrchestratorStage, tier: Option<Tier>, detail: impl Into<String>) {
        if let Some(control) = self.control {
            control.emit(OrchestratorEvent {
                stage,
                tier,
                detail: detail.into(),
            });
        }
    }

    fn is_cancelled(&self) -> bool {
        self.telemetry.borrow().cancelled
            || self.control.is_some_and(OrchestratorControl::is_cancelled)
    }

    fn cancelled_outcome(
        &self,
        complexity: Complexity,
        plan: Option<String>,
        worker_results: Vec<String>,
    ) -> OrchestratorOutcome {
        self.telemetry.borrow_mut().cancelled = true;
        OrchestratorOutcome {
            complexity,
            final_text: "[orchestration cancelled]".into(),
            plan,
            worker_results,
            verify_passed: false,
            cancelled: true,
            promotion_error: None,
            verify_rounds: 0,
            best_worker: 0,
            telemetry: self.telemetry.borrow().clone(),
        }
    }

    fn record(&self, result: &AgentCallResult) {
        let mut telemetry = self.telemetry.borrow_mut();
        telemetry.calls += 1;
        telemetry.cancelled |= result.cancelled;
        for (key, value) in &result.usage {
            *telemetry.usage.entry(key.clone()).or_default() += value;
        }
        if let Some(model) = &result.requested_model {
            if !telemetry.requested_models.contains(model) {
                telemetry.requested_models.push(model.clone());
            }
        }
        if let Some(model) = &result.confirmed_model {
            if !telemetry.confirmed_models.contains(model) {
                telemetry.confirmed_models.push(model.clone());
            }
        }
    }

    async fn run_node(&self, tier: Tier, system: &str, task: &str) -> String {
        let result = self.runner.run_result(tier, system, task).await;
        self.record(&result);
        result.text
    }

    async fn reason_node(&self, tier: Tier, system: &str, task: &str) -> String {
        let result = self.runner.reason_result(tier, system, task).await;
        self.record(&result);
        result.text
    }

    /// The recursive core. `depth` bounds high-task decomposition. Boxed because
    /// it recurses into itself (an `async fn` calling itself is infinitely sized
    /// otherwise). `LocalBoxFuture` keeps the `?Send` (current-thread) contract.
    fn handle_at(&self, task: String, depth: usize) -> LocalBoxFuture<'_, OrchestratorOutcome> {
        async move {
            if self.is_cancelled() {
                return self.cancelled_outcome(Complexity::Medium, None, vec![]);
            }
            let complexity = self.classify(&task).await;
            if self.is_cancelled() {
                return self.cancelled_outcome(complexity, None, vec![]);
            }
            orch_trace(&format!("classified as {complexity:?} at depth {depth}"));
            match complexity {
                Complexity::Simple => {
                    self.emit_stage(
                        OrchestratorStage::Workers,
                        Some(Tier::Fast),
                        "执行单 Worker 任务",
                    );
                    let final_text = self.run_node(Tier::Fast, WORKER_SYS, &task).await;
                    OrchestratorOutcome {
                        complexity,
                        final_text,
                        plan: None,
                        worker_results: vec![],
                        verify_passed: true,
                        cancelled: false,
                        promotion_error: None,
                        verify_rounds: 0,
                        best_worker: 0,
                        telemetry: self.telemetry.borrow().clone(),
                    }
                }
                Complexity::Medium => {
                    self.pipeline(&task, complexity, Tier::Fast, self.cfg.workers)
                        .await
                }
                Complexity::High => {
                    if depth < self.cfg.max_depth {
                        self.decompose_and_recurse(&task, depth).await
                    } else {
                        // Depth budget spent → run as a single best-of-N attempt.
                        self.pipeline(&task, complexity, Tier::Main, self.cfg.high_workers)
                            .await
                    }
                }
            }
        }
        .boxed_local()
    }

    async fn classify(&self, task: &str) -> Complexity {
        self.emit_stage(
            OrchestratorStage::Classify,
            Some(Tier::Fast),
            "分析任务复杂度",
        );
        let out = self.reason_node(Tier::Fast, CLASSIFY_SYS, task).await;
        parse_complexity(&out)
    }

    /// plan(main) → run_attempts(workers, verify). The plan is computed once here
    /// so callers that already have a plan can drive [`Self::run_attempts`].
    async fn pipeline(
        &self,
        task: &str,
        complexity: Complexity,
        verify_tier: Tier,
        n_workers: usize,
    ) -> OrchestratorOutcome {
        self.emit_stage(OrchestratorStage::Plan, Some(Tier::Main), "生成执行计划");
        let plan = self.reason_node(Tier::Main, PLAN_SYS, task).await;
        if self.is_cancelled() {
            return self.cancelled_outcome(complexity, Some(plan), vec![]);
        }
        self.run_attempts(task, &plan, complexity, verify_tier, n_workers)
            .await
    }

    /// workers(fast, parallel best-of-N) → verify(`verify_tier`); on FAIL, feed
    /// the verdict back and retry up to `max_verify_retries` times.
    async fn run_attempts(
        &self,
        task: &str,
        plan: &str,
        complexity: Complexity,
        verify_tier: Tier,
        n_workers: usize,
    ) -> OrchestratorOutcome {
        let n = n_workers.max(1);
        let mut feedback = String::new();
        let mut worker_results: Vec<String>;
        let mut verify_passed;
        let mut rounds = 0;

        loop {
            rounds += 1;
            if self.is_cancelled() {
                return self.cancelled_outcome(complexity, Some(plan.to_string()), vec![]);
            }
            self.emit_stage(
                OrchestratorStage::Workers,
                Some(Tier::Fast),
                format!("第 {rounds} 轮，{n} 个隔离 Worker"),
            );
            if self.is_cancelled() {
                return self.cancelled_outcome(complexity, Some(plan.to_string()), vec![]);
            }
            // Workers run as independent attempts (best-of-N), so parallel
            // execution can't corrupt shared state. Each gets the plan (+ any
            // verifier feedback from the prior round).
            let worker_futs = (0..n).map(|i| {
                let wtask = build_worker_task(task, plan, &feedback, i, n);
                async move {
                    let result = self
                        .runner
                        .run_worker_result(i, n, WORKER_SYS, &wtask)
                        .await;
                    self.record(&result);
                    result.text
                }
            });
            worker_results = join_all(worker_futs).await;
            if self.is_cancelled() {
                return self.cancelled_outcome(complexity, Some(plan.to_string()), worker_results);
            }

            self.emit_stage(
                OrchestratorStage::Verify,
                Some(verify_tier),
                format!("验证第 {rounds} 轮结果"),
            );
            let verdict = self
                .reason_node(
                    verify_tier,
                    VERIFY_SYS,
                    &build_verify_task(task, plan, &worker_results),
                )
                .await;
            if self.is_cancelled() {
                return self.cancelled_outcome(complexity, Some(plan.to_string()), worker_results);
            }
            verify_passed = verdict_passed(&verdict);

            if verify_passed || rounds > self.cfg.max_verify_retries {
                // The verifier names the best attempt; promote that worker's
                // workspace to the real one and use its result as the answer.
                let best = parse_best_worker(&verdict, n);
                self.emit_stage(
                    OrchestratorStage::Promote,
                    None,
                    format!("采用 Worker {}", best + 1),
                );
                let promotion_error = self.runner.promote_worker(best).await.err();
                let promoted = promotion_error.is_none();
                let final_text = match &promotion_error {
                    Some(error) => {
                        format!("Worker 结果已验证，但无法安全提升到当前工作区：{error}")
                    }
                    None => synthesize(&worker_results, best, &verdict, verify_passed),
                };
                return OrchestratorOutcome {
                    complexity,
                    final_text,
                    plan: Some(plan.to_string()),
                    worker_results,
                    verify_passed: verify_passed && promoted,
                    cancelled: false,
                    promotion_error,
                    verify_rounds: rounds,
                    best_worker: best,
                    telemetry: self.telemetry.borrow().clone(),
                };
            }
            // Closed loop: carry the verifier's complaint into the next attempt.
            feedback = verdict;
        }
    }

    /// High-task path: plan, then split into subtasks. With ≥2 subtasks (and
    /// depth budget remaining) run each through a recursive [`Self::handle_at`]
    /// sequentially — each subtask promotes its own winner before the next, so
    /// they build on each other — then a single main-tier verify over the whole.
    /// An atomic decomposition (<2 subtasks) falls back to one best-of-N attempt.
    async fn decompose_and_recurse(&self, task: &str, depth: usize) -> OrchestratorOutcome {
        let (plan, mut subtasks) = match self.plan_and_decompose(task).await {
            Ok(result) => result,
            Err(cancelled) => return cancelled,
        };
        orch_trace(&format!(
            "high task at depth {depth}: decomposed into {} subtask(s)",
            subtasks.len()
        ));

        if subtasks.len() < 2 {
            orch_trace("atomic (<2 subtasks) -> best-of-N fallback on main");
            return self
                .run_attempts(
                    task,
                    &plan,
                    Complexity::High,
                    Tier::Main,
                    self.cfg.high_workers,
                )
                .await;
        }

        if subtasks.len() > self.cfg.max_subtasks {
            orch_trace(&format!(
                "capping {} subtasks to max_subtasks={}",
                subtasks.len(),
                self.cfg.max_subtasks
            ));
            subtasks.truncate(self.cfg.max_subtasks);
        }

        let mut sub_results = Vec::with_capacity(subtasks.len());
        let mut all_passed = true;
        for (index, subtask) in subtasks.iter().enumerate() {
            if self.is_cancelled() {
                return self.cancelled_outcome(Complexity::High, Some(plan), sub_results);
            }
            orch_trace(&format!(
                "recursing into subtask {}/{}: {subtask}",
                index + 1,
                subtasks.len()
            ));
            let outcome = self.handle_at(subtask.clone(), depth + 1).await;
            all_passed &= outcome.verify_passed;
            sub_results.push(format!("[subtask] {subtask}\n{}", outcome.final_text));
        }

        if self.is_cancelled() {
            return self.cancelled_outcome(Complexity::High, Some(plan), sub_results);
        }
        let verdict = self
            .reason_node(
                Tier::Main,
                VERIFY_SYS,
                &build_verify_task(task, &plan, &sub_results),
            )
            .await;
        if self.is_cancelled() {
            return self.cancelled_outcome(Complexity::High, Some(plan), sub_results);
        }
        let verify_passed = all_passed && verdict_passed(&verdict);
        OrchestratorOutcome {
            complexity: Complexity::High,
            final_text: synthesize_subtasks(&sub_results, &verdict, verify_passed),
            plan: Some(plan),
            worker_results: sub_results,
            verify_passed,
            cancelled: false,
            promotion_error: None,
            verify_rounds: 1,
            best_worker: 0,
            telemetry: self.telemetry.borrow().clone(),
        }
    }

    async fn plan_and_decompose(
        &self,
        task: &str,
    ) -> Result<(String, Vec<String>), OrchestratorOutcome> {
        self.emit_stage(
            OrchestratorStage::Plan,
            Some(Tier::Main),
            "为高风险任务生成计划",
        );
        let plan = self.reason_node(Tier::Main, PLAN_SYS, task).await;
        if self.is_cancelled() {
            return Err(self.cancelled_outcome(Complexity::High, Some(plan), vec![]));
        }
        self.emit_stage(
            OrchestratorStage::Decompose,
            Some(Tier::Main),
            "拆分独立子任务",
        );
        let raw = self
            .reason_node(
                Tier::Main,
                DECOMPOSE_SYS,
                &build_decompose_task(task, &plan),
            )
            .await;
        if self.is_cancelled() {
            return Err(self.cancelled_outcome(Complexity::High, Some(plan), vec![]));
        }
        Ok((plan, parse_subtasks(&raw)))
    }
}

#[cfg(test)]
mod tests;
