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

use async_trait::async_trait;
use futures_util::future::{join_all, LocalBoxFuture};
use futures_util::FutureExt;

/// Which model tier a node runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Cheap/fast model (flash).
    Fast,
    /// Strong/expensive model (pro).
    Main,
}

/// Task complexity, deciding the node graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Medium,
    High,
}

/// Runs one self-contained agent turn (fresh session) on a tier, returning the
/// final text. The CLI/GUI implements this by building an [`crate::AgentLoop`]
/// with the chosen provider; tests use a scripted mock.
#[async_trait(?Send)]
pub trait AgentRunner {
    async fn run(&self, tier: Tier, system: &str, task: &str) -> String;

    /// Run a tool-less *reasoning* turn — classify / plan / decompose / verify.
    /// These nodes judge or plan; they must NOT execute the task. The default
    /// delegates to [`Self::run`] (fine for mock / read-only runners); the live
    /// runner overrides it to attach NO tools, so a capable model can't start
    /// implementing the task during e.g. classification.
    async fn reason(&self, tier: Tier, system: &str, task: &str) -> String {
        self.run(tier, system, task).await
    }

    /// Run a parallel worker (always [`Tier::Fast`]). `idx`/`n` let the runner
    /// isolate workers so concurrent writes don't collide (worker 0 is the
    /// primary; others may run against a throwaway workspace copy). The default
    /// ignores isolation and just runs — fine for read/analysis workers.
    async fn run_worker(&self, _idx: usize, _n: usize, system: &str, task: &str) -> String {
        self.run(Tier::Fast, system, task).await
    }

    /// Promote worker `idx`'s (isolated) workspace to the real one — called once
    /// after the verifier picks the best attempt. Default: nothing (read-only
    /// runners / no isolation).
    async fn promote_worker(&self, _idx: usize) {}
}

// System prompts for each node. Kept terse; the model's own tools do the work.
const CLASSIFY_SYS: &str = "You are a task-complexity classifier. You have NO tools — do not \
    attempt to read files or run commands. Reply with exactly one word — simple, medium, or high \
    — rating how hard/risky the coding task is. simple = a one-step, low-risk change; medium = \
    multi-step but routine; high = risky, broad, or easy to get wrong.";
const PLAN_SYS: &str = "You are a senior engineer. You have NO tools and cannot read files — work \
    only from the task text. Produce a short, concrete step-by-step plan to accomplish the task. \
    Output the plan as plain text only — do not write code, do not call tools.";
const DECOMPOSE_SYS: &str = "You are a planning lead. You have NO tools and cannot read files — \
    work only from the task and plan text given. Break the task into the smallest set of \
    INDEPENDENT subtasks that can be carried out one after another. Output ONLY subtask lines, \
    each on its own line prefixed with 'SUBTASK: ' — no preamble, no prose, no tool calls. If the \
    task is atomic (cannot be usefully split), output a single 'SUBTASK: ' line restating it.";
const WORKER_SYS: &str =
    "You are an implementation worker. Carry out the task following the plan, \
    using your tools. Describe what you did and the outcome.";
const VERIFY_SYS: &str = "You are a strict reviewer. Given the task, plan, and the workers' \
    results, decide whether the task is correctly and completely done. Start your reply with PASS \
    or FAIL, then a one-line reason. If anything is wrong or missing, reply FAIL. On the LAST \
    line, output 'BEST:<n>' giving the 1-based number of the worker whose result is best.";

/// Tunables for the node graph.
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Parallel fast workers for medium tasks (best-of-N attempts).
    pub workers: usize,
    /// Parallel fast workers for high tasks that run as a single best-of-N
    /// attempt (atomic / depth-exhausted). High gets more attempts than medium.
    pub high_workers: usize,
    /// Extra verify→retry rounds after the first attempt (closed loop).
    pub max_verify_retries: usize,
    /// Max recursive decomposition depth for high tasks (0 = never decompose;
    /// high tasks then always run as a single best-of-N attempt).
    pub max_depth: usize,
    /// Cap on subtasks per decomposition — guards against a model that
    /// over-splits a task into many tiny pieces (each its own full pipeline).
    pub max_subtasks: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        OrchestratorConfig {
            workers: 2,
            high_workers: 3,
            max_verify_retries: 1,
            max_depth: 1,
            max_subtasks: 6,
        }
    }
}

/// What the orchestration produced, for the caller to present / log.
#[derive(Debug, Clone)]
pub struct OrchestratorOutcome {
    pub complexity: Complexity,
    pub final_text: String,
    pub plan: Option<String>,
    pub worker_results: Vec<String>,
    pub verify_passed: bool,
    /// How many plan→workers→verify rounds ran (1 = no retry; 0 = simple path).
    pub verify_rounds: usize,
    /// 0-based index of the worker the verifier picked as best (0 for simple /
    /// decomposed paths, where there is no single best-of-N attempt).
    pub best_worker: usize,
}

/// Drives the tiered node graph over an [`AgentRunner`].
pub struct Orchestrator<'a> {
    runner: &'a dyn AgentRunner,
    cfg: OrchestratorConfig,
}

impl<'a> Orchestrator<'a> {
    pub fn new(runner: &'a dyn AgentRunner, cfg: OrchestratorConfig) -> Self {
        Orchestrator { runner, cfg }
    }

    /// Classify the task, then run the matching node graph (depth 0).
    pub async fn handle(&self, task: &str) -> OrchestratorOutcome {
        self.handle_at(task.to_string(), 0).await
    }

    /// The recursive core. `depth` bounds high-task decomposition. Boxed because
    /// it recurses into itself (an `async fn` calling itself is infinitely sized
    /// otherwise). `LocalBoxFuture` keeps the `?Send` (current-thread) contract.
    fn handle_at(&self, task: String, depth: usize) -> LocalBoxFuture<'_, OrchestratorOutcome> {
        async move {
            let complexity = self.classify(&task).await;
            orch_trace(&format!("classified as {complexity:?} at depth {depth}"));
            match complexity {
                Complexity::Simple => {
                    let final_text = self.runner.run(Tier::Fast, WORKER_SYS, &task).await;
                    OrchestratorOutcome {
                        complexity,
                        final_text,
                        plan: None,
                        worker_results: vec![],
                        verify_passed: true,
                        verify_rounds: 0,
                        best_worker: 0,
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
        let out = self.runner.reason(Tier::Fast, CLASSIFY_SYS, task).await;
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
        let plan = self.runner.reason(Tier::Main, PLAN_SYS, task).await;
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
            // Workers run as independent attempts (best-of-N), so parallel
            // execution can't corrupt shared state. Each gets the plan (+ any
            // verifier feedback from the prior round).
            let worker_futs = (0..n).map(|i| {
                let wtask = build_worker_task(task, plan, &feedback, i, n);
                async move { self.runner.run_worker(i, n, WORKER_SYS, &wtask).await }
            });
            worker_results = join_all(worker_futs).await;

            let verdict = self
                .runner
                .reason(
                    verify_tier,
                    VERIFY_SYS,
                    &build_verify_task(task, plan, &worker_results),
                )
                .await;
            verify_passed = verdict_passed(&verdict);

            if verify_passed || rounds > self.cfg.max_verify_retries {
                // The verifier names the best attempt; promote that worker's
                // workspace to the real one and use its result as the answer.
                let best = parse_best_worker(&verdict, n);
                self.runner.promote_worker(best).await;
                let final_text = synthesize(&worker_results, best, &verdict, verify_passed);
                return OrchestratorOutcome {
                    complexity,
                    final_text,
                    plan: Some(plan.to_string()),
                    worker_results,
                    verify_passed,
                    verify_rounds: rounds,
                    best_worker: best,
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
        let plan = self.runner.reason(Tier::Main, PLAN_SYS, task).await;
        let raw = self
            .runner
            .reason(
                Tier::Main,
                DECOMPOSE_SYS,
                &build_decompose_task(task, &plan),
            )
            .await;
        let mut subtasks = parse_subtasks(&raw);
        orch_trace(&format!(
            "high task at depth {depth}: decomposed into {} subtask(s)",
            subtasks.len()
        ));

        if subtasks.len() < 2 {
            orch_trace("atomic (<2 subtasks) -> best-of-N fallback on main");
            // Atomic → reuse the plan we already have for a single best-of-N run.
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

        // Cap over-decomposition — keep the first N, log what was dropped.
        if subtasks.len() > self.cfg.max_subtasks {
            orch_trace(&format!(
                "capping {} subtasks to max_subtasks={} (dropping {} tail)",
                subtasks.len(),
                self.cfg.max_subtasks,
                subtasks.len() - self.cfg.max_subtasks
            ));
            subtasks.truncate(self.cfg.max_subtasks);
        }

        let mut sub_results = Vec::with_capacity(subtasks.len());
        let mut all_passed = true;
        for (i, st) in subtasks.iter().enumerate() {
            orch_trace(&format!(
                "recursing into subtask {}/{}: {st}",
                i + 1,
                subtasks.len()
            ));
            let outcome = self.handle_at(st.clone(), depth + 1).await;
            all_passed &= outcome.verify_passed;
            sub_results.push(format!("[subtask] {st}\n{}", outcome.final_text));
        }

        let verdict = self
            .runner
            .reason(
                Tier::Main,
                VERIFY_SYS,
                &build_verify_task(task, &plan, &sub_results),
            )
            .await;
        let verify_passed = all_passed && verdict_passed(&verdict);
        let final_text = synthesize_subtasks(&sub_results, &verdict, verify_passed);

        OrchestratorOutcome {
            complexity: Complexity::High,
            final_text,
            plan: Some(plan),
            worker_results: sub_results,
            verify_passed,
            verify_rounds: 1,
            best_worker: 0,
        }
    }
}

/// Emit an orchestration progress line when `NCX_TRACE` is set (mirrors the
/// agent loop's trace gating). No-op otherwise, so the node graph stays quiet
/// in normal runs.
fn orch_trace(msg: &str) {
    if std::env::var_os("NCX_TRACE").is_some_and(|v| !v.is_empty()) {
        eprintln!("[ncx-trace][orch] {msg}");
    }
}

fn parse_complexity(s: &str) -> Complexity {
    let lc = s.to_lowercase();
    if lc.contains("high") {
        Complexity::High
    } else if lc.contains("simple") {
        Complexity::Simple
    } else if lc.contains("medium") {
        Complexity::Medium
    } else {
        // Unclear → treat as medium (the safe middle: gets a plan + verify).
        Complexity::Medium
    }
}

/// A verdict passes unless it explicitly says FAIL (fail-loud, not fail-silent).
fn verdict_passed(verdict: &str) -> bool {
    !verdict.to_uppercase().contains("FAIL")
}

/// Parse subtasks from a decomposition reply. Prefers explicit `SUBTASK:`
/// prefixes (case-insensitive, may follow leading bullets/whitespace); if the
/// model ignored the format and emitted a plain numbered or bulleted list, fall
/// back to that so a usable decomposition isn't lost. Empty items are skipped.
fn parse_subtasks(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in s.lines() {
        let t = line.trim();
        let up = t.to_uppercase();
        if let Some(pos) = up.find("SUBTASK:") {
            let rest = t[pos + "SUBTASK:".len()..].trim();
            if !rest.is_empty() {
                out.push(rest.to_string());
            }
        }
    }
    if out.is_empty() {
        // Fallback: numbered (`1.`/`1)`) or bulleted (`-`/`*`/`•`) list lines.
        for line in s.lines() {
            if let Some(item) = strip_list_marker(line.trim()) {
                if !item.is_empty() {
                    out.push(item.to_string());
                }
            }
        }
    }
    out
}

/// Strip a leading list marker (`1.`, `1)`, `-`, `*`, `•`) from a line, returning
/// the item text. `None` if the line isn't a list item.
fn strip_list_marker(line: &str) -> Option<&str> {
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("• "))
    {
        return Some(rest.trim());
    }
    // Numbered: leading digits then '.' or ')'.
    let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &line[digits.len()..];
        if let Some(rest) = after.strip_prefix('.').or_else(|| after.strip_prefix(')')) {
            return Some(rest.trim());
        }
    }
    None
}

fn build_worker_task(task: &str, plan: &str, feedback: &str, i: usize, n: usize) -> String {
    let mut s = format!(
        "Task:\n{task}\n\nPlan:\n{plan}\n\n(You are worker {} of {}.)",
        i + 1,
        n
    );
    if !feedback.is_empty() {
        s.push_str(&format!(
            "\n\nThe previous attempt was rejected. Address this feedback:\n{feedback}"
        ));
    }
    s
}

fn build_decompose_task(task: &str, plan: &str) -> String {
    format!("Task:\n{task}\n\nPlan:\n{plan}")
}

fn build_verify_task(task: &str, plan: &str, results: &[String]) -> String {
    let joined = results
        .iter()
        .enumerate()
        .map(|(i, r)| format!("--- worker {} ---\n{r}", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Task:\n{task}\n\nPlan:\n{plan}\n\nWorker results:\n{joined}")
}

fn synthesize(results: &[String], best_idx: usize, verdict: &str, passed: bool) -> String {
    let best = results
        .get(best_idx)
        .or_else(|| results.first())
        .cloned()
        .unwrap_or_default();
    if passed {
        best
    } else {
        format!("{best}\n\n[unverified after retries — reviewer said: {verdict}]")
    }
}

/// Join the per-subtask outcomes into the decomposed task's answer.
fn synthesize_subtasks(results: &[String], verdict: &str, passed: bool) -> String {
    let body = results.join("\n\n");
    if passed {
        body
    } else {
        format!("{body}\n\n[unverified after decomposition — reviewer said: {verdict}]")
    }
}

/// Parse `BEST:<n>` (1-based) from a verdict into a 0-based worker index,
/// clamped to `0..n`. Defaults to 0 when absent/unparseable.
fn parse_best_worker(verdict: &str, n: usize) -> usize {
    let idx = verdict
        .to_uppercase()
        .find("BEST:")
        .and_then(|p| {
            verdict[p + 5..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<usize>()
                .ok()
        })
        .map(|one_based| one_based.saturating_sub(1))
        .unwrap_or(0);
    idx.min(n.saturating_sub(1))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
            }
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
        async fn promote_worker(&self, idx: usize) {
            self.promoted.borrow_mut().push(idx);
        }
    }

    fn count(calls: &[(Tier, &str)], tier: Tier, stage: &str) -> usize {
        calls
            .iter()
            .filter(|(t, s)| *t == tier && *s == stage)
            .count()
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
        let raw =
            "SUBTASK: first thing\nnoise line\n  subtask: second\nSUBTASK:   \nSUBTASK: third";
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
}
