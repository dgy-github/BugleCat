use super::*;
use std::cell::RefCell;

/// Records every (tier, stage) call and returns scripted outputs.
struct MockRunner {
    /// Returned by classify when `complexity_queue` is empty.
    default_complexity: &'static str,
    /// Per-call classify results, popped from the back; empty → default.
    complexity_queue: RefCell<Vec<&'static str>>,
    /// What the DECOMPOSE node returns (default: one subtask → atomic).
    decomposition: &'static str,
    // Verify verdicts, popped from the back in call order; empty → "PASS".
    verdicts: RefCell<Vec<&'static str>>,
    calls: RefCell<Vec<(Tier, &'static str)>>,
    promoted: RefCell<Vec<usize>>,
    /// Worker sets passed to `compose_workers`.
    composed: RefCell<Vec<Vec<usize>>>,
    /// Worker sets passed to `verify_composition`.
    composition_requests: RefCell<Vec<Vec<usize>>>,
    /// Scripted `verify_composition` result; `None` → the trait default
    /// (cannot stage).
    composition: Option<Result<CompositionCheck, String>>,
}

impl MockRunner {
    fn new(complexity: &'static str, verdicts: Vec<&'static str>) -> Self {
        MockRunner {
            default_complexity: complexity,
            complexity_queue: RefCell::new(vec![]),
            decomposition: "SUBTASK: do the whole thing",
            verdicts: RefCell::new(verdicts),
            calls: RefCell::new(vec![]),
            promoted: RefCell::new(vec![]),
            composed: RefCell::new(vec![]),
            composition_requests: RefCell::new(vec![]),
            composition: None,
        }
    }
    /// Script the staged-composition re-verification.
    fn with_composition(mut self, result: Result<CompositionCheck, String>) -> Self {
        self.composition = Some(result);
        self
    }
    /// Script per-call classify results (popped from the back).
    fn with_complexities(self, q: Vec<&'static str>) -> Self {
        *self.complexity_queue.borrow_mut() = q;
        self
    }
    /// Script the DECOMPOSE node's output.
    fn with_decomposition(mut self, d: &'static str) -> Self {
        self.decomposition = d;
        self
    }
    fn stage(system: &str) -> &'static str {
        if system == CLASSIFY_SYS {
            "classify"
        } else if system == PLAN_SYS {
            "plan"
        } else if system == DECOMPOSE_SYS {
            "decompose"
        } else if system == WORKER_SYS {
            "worker"
        } else if system == VERIFY_SYS {
            "verify"
        } else {
            "?"
        }
    }
}

#[async_trait(?Send)]
impl AgentRunner for MockRunner {
    async fn run(&self, tier: Tier, system: &str, _task: &str) -> String {
        let stage = MockRunner::stage(system);
        self.calls.borrow_mut().push((tier, stage));
        match stage {
            "classify" => self
                .complexity_queue
                .borrow_mut()
                .pop()
                .unwrap_or(self.default_complexity)
                .to_string(),
            "decompose" => self.decomposition.to_string(),
            "verify" => self
                .verdicts
                .borrow_mut()
                .pop()
                .unwrap_or("PASS")
                .to_string(),
            "plan" => "1. do a thing".to_string(),
            _ => "worker result".to_string(),
        }
    }
    async fn promote_worker(&self, idx: usize) -> Result<(), String> {
        self.promoted.borrow_mut().push(idx);
        Ok(())
    }
    async fn compose_workers(&self, indices: &[usize]) -> Result<(), String> {
        self.composed.borrow_mut().push(indices.to_vec());
        Ok(())
    }
    async fn verify_composition(
        &self,
        indices: &[usize],
        tier: Tier,
        system: &str,
        task: &str,
    ) -> Result<CompositionCheck, String> {
        assert_eq!(system, COMPOSE_VERIFY_SYS);
        assert!(task.contains("merged changes of workers"));
        self.calls.borrow_mut().push((tier, "compose_verify"));
        self.composition_requests
            .borrow_mut()
            .push(indices.to_vec());
        match &self.composition {
            Some(result) => result.clone(),
            None => Err("cannot stage".into()),
        }
    }
}

fn verified(text: &str) -> CompositionCheck {
    CompositionCheck {
        composed: vec![1, 0, 2],
        verification: Some(AgentCallResult {
            text: text.to_string(),
            ..Default::default()
        }),
    }
}

fn three_workers() -> OrchestratorConfig {
    OrchestratorConfig {
        workers: 3,
        high_workers: 3,
        max_verify_retries: 1,
        max_depth: 1,
        max_subtasks: 6,
    }
}

fn count(calls: &[(Tier, &str)], tier: Tier, stage: &str) -> usize {
    calls
        .iter()
        .filter(|(t, s)| *t == tier && *s == stage)
        .count()
}

struct TelemetryRunner;

#[async_trait(?Send)]
impl AgentRunner for TelemetryRunner {
    async fn run(&self, _tier: Tier, system: &str, _task: &str) -> String {
        if system == CLASSIFY_SYS {
            "simple".into()
        } else {
            "done".into()
        }
    }

    async fn run_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        AgentCallResult {
            text: self.run(tier, system, task).await,
            usage: [("prompt_tokens".into(), 2), ("completion_tokens".into(), 1)].into(),
            requested_model: Some("fast-model".into()),
            confirmed_model: Some("server-fast".into()),
            cancelled: false,
        }
    }

    async fn reason_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        self.run_result(tier, system, task).await
    }
}

#[tokio::test]
async fn aggregates_usage_and_model_evidence_across_nodes() {
    let out = Orchestrator::new(&TelemetryRunner, OrchestratorConfig::default())
        .handle("small task")
        .await;
    assert_eq!(out.telemetry.calls, 2);
    assert_eq!(out.telemetry.usage["prompt_tokens"], 4);
    assert_eq!(out.telemetry.usage["completion_tokens"], 2);
    assert_eq!(out.telemetry.requested_models, ["fast-model"]);
    assert_eq!(out.telemetry.confirmed_models, ["server-fast"]);
}

#[tokio::test]
async fn simple_runs_single_fast() {
    let m = MockRunner::new("simple", vec![]);
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let out = o.handle("rename a variable").await;
    assert_eq!(out.complexity, Complexity::Simple);
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Fast, "classify"), 1);
    assert_eq!(count(&calls, Tier::Fast, "worker"), 1);
    assert_eq!(count(&calls, Tier::Main, "plan"), 0);
    assert_eq!(count(&calls, Tier::Fast, "verify"), 0);
}

#[tokio::test]
async fn medium_runs_plan_2workers_then_flash_verify() {
    let m = MockRunner::new("medium", vec!["PASS ok"]);
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let out = o.handle("add a feature").await;
    assert_eq!(out.complexity, Complexity::Medium);
    assert!(out.verify_passed);
    assert_eq!(out.verify_rounds, 1);
    assert_eq!(out.worker_results.len(), 2);
    assert_eq!(
        out.telemetry.calls, 5,
        "classify + plan + 2 workers + verify"
    );
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Main, "plan"), 1);
    assert_eq!(count(&calls, Tier::Fast, "worker"), 2);
    assert_eq!(count(&calls, Tier::Fast, "verify"), 1);
    assert_eq!(count(&calls, Tier::Main, "verify"), 0);
}

#[tokio::test]
async fn high_atomic_falls_back_to_best_of_n_on_main() {
    // Default decomposition yields a single subtask → atomic → best-of-N
    // with high_workers (3), verified on main.
    let m = MockRunner::new("high", vec!["PASS"]);
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let out = o.handle("refactor the auth layer").await;
    assert_eq!(out.complexity, Complexity::High);
    assert!(out.verify_passed);
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Main, "plan"), 1);
    assert_eq!(count(&calls, Tier::Main, "decompose"), 1);
    assert_eq!(count(&calls, Tier::Fast, "worker"), 3, "high_workers");
    assert_eq!(count(&calls, Tier::Main, "verify"), 1);
    assert_eq!(count(&calls, Tier::Fast, "verify"), 0);
}

#[tokio::test]
async fn high_decomposes_into_recursive_subtasks() {
    // Top task = high → decompose into 2 subtasks; each subtask classifies
    // as simple (single fast run, no plan/verify). Then a main verify joins.
    let m = MockRunner::new("high", vec!["PASS whole"])
        .with_complexities(vec!["simple", "simple", "high"]) // popped: high(top), simple, simple
        .with_decomposition("SUBTASK: build module A\nSUBTASK: wire it into B");
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let out = o.handle("ship a big feature").await;

    assert_eq!(out.complexity, Complexity::High);
    assert!(out.verify_passed);
    assert_eq!(out.worker_results.len(), 2, "one entry per subtask");
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Main, "plan"), 1, "top plan only");
    assert_eq!(count(&calls, Tier::Main, "decompose"), 1);
    // Two simple subtasks → two fast worker runs, no per-subtask plan/verify.
    assert_eq!(count(&calls, Tier::Fast, "worker"), 2);
    assert_eq!(count(&calls, Tier::Main, "verify"), 1, "final join verify");
    assert_eq!(count(&calls, Tier::Fast, "classify"), 3, "top + 2 subtasks");
}

#[tokio::test]
async fn subtask_count_is_capped() {
    // Model over-splits into 4 subtasks but max_subtasks=2 → only 2 recurse.
    let m = MockRunner::new("high", vec![])
        .with_complexities(vec!["simple", "simple", "simple", "simple", "high"])
        .with_decomposition("SUBTASK: a\nSUBTASK: b\nSUBTASK: c\nSUBTASK: d");
    let o = Orchestrator::new(
        &m,
        OrchestratorConfig {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 2,
        },
    );
    let out = o.handle("over-split me").await;
    assert_eq!(out.worker_results.len(), 2, "capped to max_subtasks");
    let calls = m.calls.borrow();
    // 2 capped simple subtasks → 2 fast worker runs (not 4).
    assert_eq!(count(&calls, Tier::Fast, "worker"), 2);
}

#[tokio::test]
async fn recursion_is_depth_capped() {
    // max_depth = 1: the top high task decomposes into 2 subtasks, but those
    // subtasks are ALSO classified high — at depth==max_depth they must NOT
    // decompose again; they run as best-of-N instead. So decompose is called
    // exactly once (top level).
    let m = MockRunner::new("high", vec![]) // all verdicts default PASS
        .with_complexities(vec!["high", "high", "high"]) // top + 2 subtasks all high
        .with_decomposition("SUBTASK: a\nSUBTASK: b");
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let _ = o.handle("deep task").await;
    let calls = m.calls.borrow();
    assert_eq!(
        count(&calls, Tier::Main, "decompose"),
        1,
        "only the top level decomposes; subtasks are depth-capped"
    );
}

#[tokio::test]
async fn decomposition_off_when_max_depth_zero() {
    // max_depth = 0 → high tasks never decompose; single best-of-N on main.
    let m = MockRunner::new("high", vec!["PASS"]);
    let o = Orchestrator::new(
        &m,
        OrchestratorConfig {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 0,
            max_subtasks: 6,
        },
    );
    let out = o.handle("big risky change").await;
    assert_eq!(out.complexity, Complexity::High);
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Main, "decompose"), 0, "no decompose");
    assert_eq!(count(&calls, Tier::Main, "plan"), 1);
    assert_eq!(count(&calls, Tier::Fast, "worker"), 3);
    assert_eq!(count(&calls, Tier::Main, "verify"), 1);
}

#[tokio::test]
async fn closed_loop_retries_on_fail_then_passes() {
    // Popped from the back: first verify → "FAIL needs work", second → "PASS good".
    let m = MockRunner::new("medium", vec!["PASS good", "FAIL needs work"]);
    let o = Orchestrator::new(
        &m,
        OrchestratorConfig {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 6,
        },
    );
    let out = o.handle("tricky change").await;
    assert!(out.verify_passed);
    assert_eq!(out.verify_rounds, 2);
    let calls = m.calls.borrow();
    assert_eq!(count(&calls, Tier::Fast, "worker"), 4); // 2 workers × 2 rounds
    assert_eq!(count(&calls, Tier::Fast, "verify"), 2);
}

#[tokio::test]
async fn verifier_selects_best_worker_and_promotes_it() {
    // Verifier names worker 2 (1-based) as best → 0-based index 1 promoted.
    let m = MockRunner::new("medium", vec!["PASS good\nBEST:2"]);
    let o = Orchestrator::new(
        &m,
        OrchestratorConfig {
            workers: 3,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 6,
        },
    );
    let out = o.handle("pick best").await;
    assert!(out.verify_passed);
    assert_eq!(out.best_worker, 1, "BEST:2 -> 0-based 1");
    assert_eq!(
        *m.promoted.borrow(),
        vec![1],
        "the chosen worker is promoted"
    );
}

struct FailingPromotionRunner(MockRunner);

#[async_trait(?Send)]
impl AgentRunner for FailingPromotionRunner {
    async fn run(&self, tier: Tier, system: &str, task: &str) -> String {
        self.0.run(tier, system, task).await
    }

    async fn promote_worker(&self, _idx: usize) -> Result<(), String> {
        Err("工作区冲突：src/app.rs".into())
    }
}

#[tokio::test]
async fn promotion_failure_is_reported_and_never_claimed_as_verified() {
    let runner = FailingPromotionRunner(MockRunner::new("medium", vec!["PASS\nBEST:1"]));
    let out = Orchestrator::new(&runner, OrchestratorConfig::default())
        .handle("conflicting change")
        .await;
    assert!(!out.verify_passed);
    assert_eq!(
        out.promotion_error.as_deref(),
        Some("工作区冲突：src/app.rs")
    );
    assert!(
        out.final_text.contains("无法安全提升"),
        "{}",
        out.final_text
    );
}

#[test]
fn runtime_config_controls_orchestrator_resource_budget() {
    let config = ncx_config::Config {
        orchestrator_workers: 4,
        orchestrator_high_workers: 5,
        orchestrator_verify_retries: 2,
        orchestrator_max_depth: 2,
        orchestrator_max_subtasks: 10,
        ..Default::default()
    };
    let budget = OrchestratorConfig::from_runtime_config(&config);
    assert_eq!(budget.workers, 4);
    assert_eq!(budget.high_workers, 5);
    assert_eq!(budget.max_verify_retries, 2);
    assert_eq!(budget.max_depth, 2);
    assert_eq!(budget.max_subtasks, 10);
}

#[tokio::test]
async fn missing_best_defaults_to_worker_zero() {
    let m = MockRunner::new("medium", vec!["PASS looks fine"]); // no BEST line
    let o = Orchestrator::new(&m, OrchestratorConfig::default());
    let out = o.handle("no best line").await;
    assert_eq!(out.best_worker, 0);
    assert_eq!(*m.promoted.borrow(), vec![0]);
}

#[tokio::test]
async fn retries_are_capped() {
    let m = MockRunner::new("medium", vec!["FAIL", "FAIL", "FAIL"]);
    let o = Orchestrator::new(
        &m,
        OrchestratorConfig {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 6,
        },
    );
    let out = o.handle("impossible").await;
    assert!(!out.verify_passed);
    assert_eq!(out.verify_rounds, 2); // initial + 1 retry, then give up
    assert!(out.final_text.contains("unverified after retries"));
}

#[tokio::test]
async fn parse_subtasks_extracts_prefixed_lines() {
    let raw = "SUBTASK: first thing\nnoise line\n  subtask: second\nSUBTASK:   \nSUBTASK: third";
    let got = parse_subtasks(raw);
    assert_eq!(got, vec!["first thing", "second", "third"]);
}

#[tokio::test]
async fn parse_subtasks_falls_back_to_lists() {
    // No SUBTASK: prefix → numbered/bulleted lines are used instead.
    assert_eq!(
        parse_subtasks("1. alpha\n2) beta\n- gamma\n* delta"),
        vec!["alpha", "beta", "gamma", "delta"]
    );
    // Explicit SUBTASK: prefixes take priority (no list fallback then).
    assert_eq!(parse_subtasks("SUBTASK: x\n1. y"), vec!["x"]);
}

#[tokio::test]
async fn verifier_composes_multiple_workers_and_builds_compose_feedback() {
    let m = MockRunner::new("medium", vec!["PASS composed\nBEST:2\nCOMPOSE:2,1,3"])
        .with_composition(Ok(verified("PASS merged build and tests ok")));
    let o = Orchestrator::new(&m, three_workers());
    let out = o.handle("compose disjoint fixes").await;
    assert!(out.verify_passed);
    assert_eq!(out.best_worker, 1);
    assert_eq!(*m.composition_requests.borrow(), vec![vec![1, 0, 2]]);
    assert_eq!(*m.composed.borrow(), vec![vec![1, 0, 2]]);
    assert!(m.promoted.borrow().is_empty());
    assert!(out.final_text.contains("[Composed from Worker 1]"));
    assert!(out.final_text.contains("[Composed from Worker 3]"));

    let fb = build_compose_feedback(
        &["fixed parser".to_string(), "added edge test".to_string()],
        "FAIL missing import",
    );
    assert!(fb.contains("[Propose-Probe-Compose Peer Digest"));
    assert!(fb.contains("- Worker 1: fixed parser"));
    assert!(fb.contains("- Worker 2: added edge test"));
}

#[tokio::test]
async fn failed_composition_reverification_falls_back_to_best_worker() {
    let m = MockRunner::new("medium", vec!["PASS ok\nBEST:2\nCOMPOSE:2,1,3"]).with_composition(Ok(
        verified("FAIL duplicate definition of parse()\nmore detail"),
    ));
    let out = Orchestrator::new(&m, three_workers())
        .handle("compose clashing fixes")
        .await;
    assert!(out.verify_passed, "the best worker alone was verified");
    assert!(m.composed.borrow().is_empty(), "nothing composed");
    assert_eq!(*m.promoted.borrow(), vec![1]);
    assert!(!out.final_text.contains("[Composed from Worker"));
    assert!(out
        .final_text
        .contains("组合结果复验未通过（FAIL duplicate definition of parse()）"));
    assert!(out.final_text.contains("已回退为只采用 Worker 2"));
}

#[tokio::test]
async fn composition_without_surviving_secondaries_promotes_best_only() {
    let m = MockRunner::new("medium", vec!["PASS ok\nBEST:1\nCOMPOSE:1,2"]).with_composition(Ok(
        CompositionCheck {
            composed: vec![0],
            verification: None,
        },
    ));
    let out = Orchestrator::new(&m, three_workers()).handle("t").await;
    assert!(out.verify_passed);
    assert!(m.composed.borrow().is_empty());
    assert_eq!(*m.promoted.borrow(), vec![0]);
    assert!(out.final_text.contains("未组合，已回退为只采用 Worker 1"));
}

#[tokio::test]
async fn runner_that_cannot_stage_promotes_best_only() {
    let m = MockRunner::new("medium", vec!["PASS ok\nBEST:3\nCOMPOSE:3,1"]);
    let out = Orchestrator::new(&m, three_workers()).handle("t").await;
    assert!(out.verify_passed);
    assert_eq!(*m.composition_requests.borrow(), vec![vec![2, 0]]);
    assert!(m.composed.borrow().is_empty());
    assert_eq!(*m.promoted.borrow(), vec![2]);
    assert!(out
        .final_text
        .contains("无法复验组合结果（cannot stage），已回退为只采用 Worker 3"));
}

#[tokio::test]
async fn exhausted_retries_never_compose_unverified_workers() {
    // Both rounds fail (verdicts pop from the back); the last still asks to compose.
    let m = MockRunner::new(
        "medium",
        vec![
            "FAIL still broken\nBEST:2\nCOMPOSE:2,1",
            "FAIL broken\nBEST:1",
        ],
    )
    .with_composition(Ok(verified("PASS")));
    let out = Orchestrator::new(&m, three_workers()).handle("t").await;
    assert!(!out.verify_passed);
    assert_eq!(out.verify_rounds, 2);
    assert!(
        m.composition_requests.borrow().is_empty(),
        "no re-verification"
    );
    assert!(m.composed.borrow().is_empty());
    assert_eq!(*m.promoted.borrow(), vec![1]);
    assert!(out.final_text.contains("[unverified after retries"));
}

#[test]
fn verify_prompt_documents_compose_line() {
    assert!(VERIFY_SYS.contains("BEST:<n>"));
    assert!(VERIFY_SYS.contains("COMPOSE:<best>,<n>"));
    assert!(COMPOSE_VERIFY_SYS.contains("Do not modify any files"));
}

#[test]
fn parse_compose_workers_ignores_out_of_range_workers() {
    assert_eq!(
        parse_compose_workers("BEST:2\nCOMPOSE:2,7,0,3", 3, 1),
        vec![1, 2]
    );
    assert_eq!(parse_compose_workers("BEST:1", 3, 0), vec![0]);
    assert_eq!(parse_compose_workers("COMPOSE:1,2", 1, 0), vec![0]);
}

#[test]
fn compose_verify_task_lists_only_merged_workers() {
    let results = vec!["r1".to_string(), "r2".to_string(), "r3".to_string()];
    let task = build_compose_verify_task("fix bug", "1. fix", &results, &[2, 0]);
    assert!(task.contains("merged changes of workers 3, 1"));
    assert!(task.contains("--- worker 3 ---\nr3"));
    assert!(task.contains("--- worker 1 ---\nr1"));
    assert!(!task.contains("r2"));
}

#[test]
fn verdict_uses_the_leading_pass_or_fail() {
    assert!(verdict_passed(
        "PASS — the previously failing test now passes\nBEST:1"
    ));
    assert!(verdict_passed("**PASS**: fixes the FAILURE path\nBEST:2"));
    assert!(verdict_passed("\n  pass. no failures remain"));
    assert!(!verdict_passed("FAIL missing import\nBEST:1"));
    assert!(!verdict_passed("## FAIL\nPASS would need tests"));
    // No leading verdict: any FAIL still counts as a failure.
    assert!(!verdict_passed("Looks mostly fine but tests FAIL\nBEST:1"));
    assert!(verdict_passed("Looks fine\nBEST:1"));
}
