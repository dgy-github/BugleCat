//! Release-mode live soak for mixed parallel and serial tool dispatch.
//!
//! Requires `DEEPSEEK_API_KEY`. The default is 30 minutes with eight workers.
//! Use `--duration-secs 30` for the preflight run.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::future::{join, join_all};
use ncx_core::{
    AgentLoop, Provider, Session, TaskBudget, Tool, ToolContext, ToolRegistry, TurnResult,
};
use ncx_provider::{DeepSeekProvider, ModelResponse};
use ncx_sandbox::{SandboxPolicy, READ_ONLY};
use serde_json::{json, Value};

const CONCURRENCY: usize = 8;
const DEFAULT_DURATION_SECS: u64 = 30 * 60;
const PROGRESS_INTERVAL_SECS: u64 = 60;
const MAX_OUTPUT_TOKENS: i64 = 1_024;
const DEFAULT_TOKEN_BUDGET: i64 = 10_000_000;
const CONSECUTIVE_ERROR_LIMIT: usize = 16;
const CONTRACT_FAILURE_MIN_SAMPLES: usize = 20;
const CONTRACT_FAILURE_RATE_LIMIT: f64 = 0.20;
const MIN_WORKLOAD_SUCCESS_RATE: f64 = 0.80;

#[derive(Clone)]
struct CappedProvider(DeepSeekProvider);

#[async_trait(?Send)]
impl Provider for CappedProvider {
    fn model(&self) -> &str {
        &self.0.model
    }

    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse {
        let tools = (!tools.is_empty()).then_some(tools);
        self.0
            .chat(
                messages,
                tools,
                Some(0.0),
                Some(MAX_OUTPUT_TOKENS),
                reasoning_effort,
            )
            .await
            .unwrap_or_else(|error| ModelResponse {
                content: error.to_string(),
                finish_reason: "error".to_string(),
                ..Default::default()
            })
    }
}

#[derive(Default)]
struct ProbeState {
    active_reads: usize,
    peak_reads: usize,
    read_calls: usize,
    serial_calls: usize,
    barrier_violations: usize,
    completions: Vec<String>,
}

struct ReadProbe(Rc<RefCell<ProbeState>>);

#[async_trait(?Send)]
impl Tool for ReadProbe {
    fn name(&self) -> &str {
        "read_probe"
    }

    fn description(&self) -> &str {
        "Read-only soak probe. Call twice before serial_probe and twice after it."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"label": {"type": "string", "enum": ["a", "b", "c", "d"]}},
            "required": ["label"],
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        let label = args["label"].as_str().unwrap_or("invalid").to_string();
        {
            let mut state = self.0.borrow_mut();
            state.active_reads += 1;
            state.peak_reads = state.peak_reads.max(state.active_reads);
            state.read_calls += 1;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        let mut state = self.0.borrow_mut();
        state.active_reads = state.active_reads.saturating_sub(1);
        state.completions.push(label.clone());
        format!("read:{label}")
    }
}

struct SerialProbe(Rc<RefCell<ProbeState>>);

#[async_trait(?Send)]
impl Tool for SerialProbe {
    fn name(&self) -> &str {
        "serial_probe"
    }

    fn description(&self) -> &str {
        "Serial soak barrier. Call exactly once between the b and c read probes."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"label": {"type": "string", "const": "barrier"}},
            "required": ["label"],
        })
    }

    async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
        let mut state = self.0.borrow_mut();
        state.barrier_violations += usize::from(state.active_reads != 0);
        state.serial_calls += 1;
        state.completions.push("barrier".to_string());
        "serial:barrier".to_string()
    }
}

struct RequestOutcome {
    result: TurnResult,
    probe: ProbeState,
}

impl RequestOutcome {
    fn contract_ok(&self) -> bool {
        let barrier_at_two = self.probe.completions.get(2).map(String::as_str) == Some("barrier");
        self.probe.read_calls == 4
            && self.probe.serial_calls == 1
            && self.probe.barrier_violations == 0
            && self.probe.peak_reads == 2
            && barrier_at_two
    }
}

#[derive(Default)]
struct Metrics {
    started: usize,
    completed: usize,
    workload_ok: usize,
    provider_errors: usize,
    contract_failures: usize,
    semantic_mismatches: usize,
    rate_limits: usize,
    timeouts: usize,
    consecutive_errors: usize,
    in_flight: usize,
    max_in_flight: usize,
    read_calls: usize,
    serial_calls: usize,
    barrier_violations: usize,
    peak_parallel_reads: usize,
    prompt_tokens: i64,
    completion_tokens: i64,
    latencies_ms: Vec<u64>,
    contract_failure_samples: Vec<String>,
    stop_reason: Option<String>,
}

impl Metrics {
    fn record_start(&mut self) {
        self.started += 1;
        self.in_flight += 1;
        self.max_in_flight = self.max_in_flight.max(self.in_flight);
    }

    fn record_result(&mut self, outcome: &RequestOutcome, latency_ms: u64) {
        self.completed += 1;
        self.in_flight = self.in_flight.saturating_sub(1);
        self.latencies_ms.push(latency_ms);
        self.prompt_tokens += outcome
            .result
            .usage
            .get("prompt_tokens")
            .copied()
            .unwrap_or(0);
        self.completion_tokens += outcome
            .result
            .usage
            .get("completion_tokens")
            .copied()
            .unwrap_or(0);
        self.read_calls += outcome.probe.read_calls;
        self.serial_calls += outcome.probe.serial_calls;
        self.barrier_violations += outcome.probe.barrier_violations;
        self.peak_parallel_reads = self.peak_parallel_reads.max(outcome.probe.peak_reads);

        if outcome.result.stop_reason == "error" {
            self.record_provider_error(&outcome.result.final_text);
            return;
        }
        self.consecutive_errors = 0;
        if !outcome.contract_ok() {
            self.contract_failures += 1;
            if self.contract_failure_samples.len() < 3 {
                self.contract_failure_samples.push(format!(
                    "stop={} tools={} reads={} serial={} peak={} text={:?}",
                    outcome.result.stop_reason,
                    outcome.result.tools_used.len(),
                    outcome.probe.read_calls,
                    outcome.probe.serial_calls,
                    outcome.probe.peak_reads,
                    outcome
                        .result
                        .final_text
                        .chars()
                        .take(80)
                        .collect::<String>()
                ));
            }
        } else if outcome.result.final_text.trim() != "TOOL_OK" {
            self.semantic_mismatches += 1;
        } else {
            self.workload_ok += 1;
        }
    }

    fn record_provider_error(&mut self, message: &str) {
        self.provider_errors += 1;
        self.consecutive_errors += 1;
        let error = message.to_ascii_lowercase();
        self.rate_limits += usize::from(error.contains("429") || error.contains("rate limit"));
        self.timeouts += usize::from(error.contains("timeout") || error.contains("timed out"));
    }

    fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    fn workload_success_rate(&self) -> f64 {
        if self.completed == 0 {
            return 0.0;
        }
        self.workload_ok as f64 / self.completed as f64
    }

    fn apply_circuit_breakers(&mut self, token_budget: i64) {
        if self.stop_reason.is_some() {
            return;
        }
        if self.consecutive_errors >= CONSECUTIVE_ERROR_LIMIT {
            self.stop_reason = Some("circuit_breaker: consecutive provider errors".to_string());
        } else if self.completed >= CONTRACT_FAILURE_MIN_SAMPLES
            && self.contract_failures as f64 / self.completed as f64 > CONTRACT_FAILURE_RATE_LIMIT
        {
            self.stop_reason = Some("circuit_breaker: tool contract failure rate".to_string());
        } else if self.total_tokens() >= token_budget {
            self.stop_reason = Some(format!("token_budget: reached {token_budget} tokens"));
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config = Config::from_process().unwrap_or_else(|error| {
        eprintln!("live_agent_tool_soak: {error}");
        std::process::exit(2);
    });
    let provider = CappedProvider(DeepSeekProvider::new(
        config.api_key.clone(),
        &config.base_url,
        config.model.clone(),
    ));
    let metrics = Rc::new(RefCell::new(Metrics::default()));
    let started_at = Instant::now();
    let deadline = started_at + config.duration;
    eprintln!(
        "tool_soak_start duration_s={} concurrency={} model={} max_output_tokens={} token_budget={}",
        config.duration.as_secs(), CONCURRENCY, config.model, MAX_OUTPUT_TOKENS, config.token_budget
    );

    let workers = (0..CONCURRENCY).map(|_| {
        run_worker(
            provider.clone(),
            config.workspace.clone(),
            deadline,
            config.token_budget,
            Rc::clone(&metrics),
        )
    });
    join(
        join_all(workers),
        report_progress(Rc::clone(&metrics), started_at, deadline),
    )
    .await;

    let elapsed = started_at.elapsed();
    print_summary(&metrics.borrow(), elapsed, &config);
    let failed = {
        let current = metrics.borrow();
        current.provider_errors > 0
            || current.barrier_violations > 0
            || current.stop_reason.is_some()
            || current.workload_success_rate() < MIN_WORKLOAD_SUCCESS_RATE
    };
    if failed {
        std::process::exit(1);
    }
}

async fn run_worker(
    provider: CappedProvider,
    workspace: PathBuf,
    deadline: Instant,
    token_budget: i64,
    metrics: Rc<RefCell<Metrics>>,
) {
    loop {
        if Instant::now() >= deadline || metrics.borrow().stop_reason.is_some() {
            return;
        }
        metrics.borrow_mut().record_start();
        let request_started = Instant::now();
        let outcome = run_request(provider.clone(), workspace.clone()).await;
        let mut current = metrics.borrow_mut();
        current.record_result(&outcome, request_started.elapsed().as_millis() as u64);
        current.apply_circuit_breakers(token_budget);
    }
}

async fn run_request(provider: CappedProvider, workspace: PathBuf) -> RequestOutcome {
    let probe = Rc::new(RefCell::new(ProbeState::default()));
    let policy = SandboxPolicy::new(READ_ONLY, &workspace);
    let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
    tools.register(Box::new(ReadProbe(Rc::clone(&probe))));
    tools.register(Box::new(SerialProbe(Rc::clone(&probe))));
    let session = Session::new(
        "Execute the exact requested tool sequence. After all tool results, reply exactly TOOL_OK.",
    );
    let mut agent = AgentLoop::new(Box::new(provider), tools, session)
        .with_max_parallel_tool_calls(8)
        .with_task_budget(TaskBudget {
            max_model_calls: 2,
            max_tool_calls: 5,
        });
    let result = agent
        .run_turn(
            json!("In one assistant response call: read_probe label a, read_probe label b, serial_probe label barrier, read_probe label c, read_probe label d. Preserve this exact order. Then use the results and reply TOOL_OK."),
            None,
        )
        .await;
    drop(agent);
    let probe = Rc::try_unwrap(probe)
        .ok()
        .expect("probe tools dropped with AgentLoop")
        .into_inner();
    RequestOutcome { result, probe }
}

async fn report_progress(metrics: Rc<RefCell<Metrics>>, started_at: Instant, deadline: Instant) {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || metrics.borrow().stop_reason.is_some() {
            return;
        }
        tokio::time::sleep(remaining.min(Duration::from_secs(PROGRESS_INTERVAL_SECS))).await;
        let current = metrics.borrow();
        eprintln!(
            "tool_soak_progress elapsed_s={} completed={} in_flight={} workload_ok={} contract_failures={} errors={} tokens={}",
            started_at.elapsed().as_secs(), current.completed, current.in_flight,
            current.workload_ok, current.contract_failures, current.provider_errors,
            current.total_tokens()
        );
    }
}

fn print_summary(metrics: &Metrics, elapsed: Duration, config: &Config) {
    let mut latencies = metrics.latencies_ms.clone();
    latencies.sort_unstable();
    println!(
        "{}",
        json!({
            "build_profile": "release",
            "duration_target_s": config.duration.as_secs(),
            "duration_actual_s": elapsed.as_secs_f64(),
            "concurrency": CONCURRENCY,
            "max_in_flight": metrics.max_in_flight,
            "started": metrics.started,
            "completed": metrics.completed,
            "workload_ok": metrics.workload_ok,
            "workload_success_rate": metrics.workload_success_rate(),
            "minimum_workload_success_rate": MIN_WORKLOAD_SUCCESS_RATE,
            "contract_failures": metrics.contract_failures,
            "semantic_mismatches": metrics.semantic_mismatches,
            "provider_errors": metrics.provider_errors,
            "rate_limits": metrics.rate_limits,
            "timeouts": metrics.timeouts,
            "requests_per_second": metrics.completed as f64 / elapsed.as_secs_f64(),
            "latency_ms": {
                "p50": percentile(&latencies, 50),
                "p95": percentile(&latencies, 95),
                "p99": percentile(&latencies, 99),
                "max": latencies.last().copied().unwrap_or(0),
            },
            "read_calls": metrics.read_calls,
            "serial_calls": metrics.serial_calls,
            "barrier_violations": metrics.barrier_violations,
            "peak_parallel_reads_per_request": metrics.peak_parallel_reads,
            "prompt_tokens": metrics.prompt_tokens,
            "completion_tokens": metrics.completion_tokens,
            "total_tokens": metrics.total_tokens(),
            "contract_failure_samples": metrics.contract_failure_samples,
            "stop_reason": metrics.stop_reason.as_deref().unwrap_or("duration_elapsed"),
        })
    );
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) * percentile).div_ceil(100)]
}

struct Config {
    api_key: String,
    base_url: String,
    model: String,
    duration: Duration,
    token_budget: i64,
    workspace: PathBuf,
}

impl Config {
    fn from_process() -> Result<Self, String> {
        let duration_secs = duration_arg(std::env::args().skip(1))?;
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .map_err(|_| "DEEPSEEK_API_KEY is required".to_string())?;
        let token_budget = std::env::var("NCX_SOAK_MAX_TOTAL_TOKENS")
            .ok()
            .map(|value| value.parse::<i64>())
            .transpose()
            .map_err(|_| "NCX_SOAK_MAX_TOTAL_TOKENS must be an integer".to_string())?
            .unwrap_or(DEFAULT_TOKEN_BUDGET);
        if token_budget < 1 {
            return Err("NCX_SOAK_MAX_TOTAL_TOKENS must be positive".to_string());
        }
        Ok(Self {
            api_key,
            base_url: std::env::var("NCX_SOAK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/beta".to_string()),
            model: std::env::var("NCX_SOAK_MODEL")
                .unwrap_or_else(|_| "deepseek-v4-pro".to_string()),
            duration: Duration::from_secs(duration_secs),
            token_budget,
            workspace: std::env::current_dir()
                .map_err(|error| format!("failed to resolve current directory: {error}"))?,
        })
    }
}

fn duration_arg(mut args: impl Iterator<Item = String>) -> Result<u64, String> {
    let Some(flag) = args.next() else {
        return Ok(DEFAULT_DURATION_SECS);
    };
    if flag != "--duration-secs" {
        return Err(format!("unknown argument: {flag}"));
    }
    let value = args
        .next()
        .ok_or_else(|| "--duration-secs requires a value".to_string())?;
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_string());
    }
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "--duration-secs must be a positive integer".to_string())?;
    if seconds == 0 {
        return Err("--duration-secs must be positive".to_string());
    }
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_requires_parallel_reads_and_middle_barrier() {
        let result = TurnResult {
            final_text: "TOOL_OK".to_string(),
            iterations: 2,
            stop_reason: "completed".to_string(),
            tools_used: Vec::new(),
            usage: Default::default(),
        };
        let outcome = RequestOutcome {
            result,
            probe: ProbeState {
                peak_reads: 2,
                read_calls: 4,
                serial_calls: 1,
                completions: vec![
                    "a".into(),
                    "b".into(),
                    "barrier".into(),
                    "c".into(),
                    "d".into(),
                ],
                ..Default::default()
            },
        };
        assert!(outcome.contract_ok());
    }

    #[test]
    fn duration_defaults_to_thirty_minutes() {
        assert_eq!(duration_arg(std::iter::empty()).unwrap(), 1_800);
    }
}
