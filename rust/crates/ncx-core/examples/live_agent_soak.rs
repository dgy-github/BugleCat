//! Low-token live soak test for the real AgentLoop and DeepSeek transport.
//!
//! Requires `DEEPSEEK_API_KEY`. The default run lasts 30 minutes with exactly
//! eight workers. Override only the duration for a short preflight:
//! `cargo run -p ncx-core --example live_agent_soak -- --duration-secs 30`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::future::{join, join_all};
use ncx_core::{AgentLoop, Provider, Session, TaskBudget, ToolContext, ToolRegistry};
use ncx_provider::{DeepSeekProvider, ModelResponse};
use ncx_sandbox::{SandboxPolicy, READ_ONLY};
use serde_json::{json, Value};

const CONCURRENCY: usize = 8;
const DEFAULT_DURATION_SECS: u64 = 30 * 60;
const PROGRESS_INTERVAL_SECS: u64 = 60;
const MAX_OUTPUT_TOKENS: i64 = 64;
const CONSECUTIVE_ERROR_LIMIT: usize = 16;
const ERROR_RATE_MIN_SAMPLES: usize = 40;
const ERROR_RATE_LIMIT: f64 = 0.25;
const DEFAULT_TOKEN_BUDGET: i64 = 5_000_000;

#[derive(Clone)]
struct CappedProvider {
    inner: DeepSeekProvider,
}

#[async_trait(?Send)]
impl Provider for CappedProvider {
    fn model(&self) -> &str {
        &self.inner.model
    }

    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse {
        let tools = (!tools.is_empty()).then_some(tools);
        self.inner
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
struct Metrics {
    started: usize,
    completed: usize,
    exact_ok: usize,
    semantic_mismatches: usize,
    provider_errors: usize,
    rate_limits: usize,
    timeouts: usize,
    consecutive_errors: usize,
    in_flight: usize,
    max_in_flight: usize,
    prompt_tokens: i64,
    completion_tokens: i64,
    latencies_ms: Vec<u64>,
    stop_reason: Option<String>,
}

impl Metrics {
    fn record_start(&mut self) {
        self.started += 1;
        self.in_flight += 1;
        self.max_in_flight = self.max_in_flight.max(self.in_flight);
    }

    fn record_result(&mut self, result: &ncx_core::TurnResult, latency_ms: u64) {
        self.completed += 1;
        self.in_flight = self.in_flight.saturating_sub(1);
        self.latencies_ms.push(latency_ms);
        self.prompt_tokens += result.usage.get("prompt_tokens").copied().unwrap_or(0);
        self.completion_tokens += result.usage.get("completion_tokens").copied().unwrap_or(0);

        if result.stop_reason == "error" {
            self.provider_errors += 1;
            self.consecutive_errors += 1;
            let error = result.final_text.to_ascii_lowercase();
            self.rate_limits += usize::from(error.contains("429") || error.contains("rate limit"));
            self.timeouts += usize::from(error.contains("timeout") || error.contains("timed out"));
        } else {
            self.consecutive_errors = 0;
            if result.final_text.trim() == "OK" {
                self.exact_ok += 1;
            } else {
                self.semantic_mismatches += 1;
            }
        }
    }

    fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    fn apply_circuit_breakers(&mut self, token_budget: i64) {
        if self.stop_reason.is_some() {
            return;
        }
        if self.consecutive_errors >= CONSECUTIVE_ERROR_LIMIT {
            self.stop_reason = Some(format!(
                "circuit_breaker: {} consecutive provider errors",
                self.consecutive_errors
            ));
            return;
        }
        if self.completed >= ERROR_RATE_MIN_SAMPLES
            && self.provider_errors as f64 / self.completed as f64 > ERROR_RATE_LIMIT
        {
            self.stop_reason = Some(format!(
                "circuit_breaker: provider error rate exceeded {:.0}%",
                ERROR_RATE_LIMIT * 100.0
            ));
            return;
        }
        if self.total_tokens() >= token_budget {
            self.stop_reason = Some(format!("token_budget: reached {token_budget} tokens"));
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let config = match Config::from_process() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("live_agent_soak: {error}");
            std::process::exit(2);
        }
    };
    let provider = CappedProvider {
        inner: DeepSeekProvider::new(
            config.api_key.clone(),
            &config.base_url,
            config.model.clone(),
        ),
    };
    let metrics = Rc::new(RefCell::new(Metrics::default()));
    let started_at = Instant::now();
    let deadline = started_at + config.duration;

    eprintln!(
        "soak_start duration_s={} concurrency={} model={} max_output_tokens={} token_budget={}",
        config.duration.as_secs(),
        CONCURRENCY,
        config.model,
        MAX_OUTPUT_TOKENS,
        config.token_budget
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
    let progress = report_progress(Rc::clone(&metrics), started_at, deadline);
    join(join_all(workers), progress).await;

    let elapsed = started_at.elapsed();
    print_summary(&metrics.borrow(), elapsed, &config);
    if metrics.borrow().provider_errors > 0 || metrics.borrow().stop_reason.is_some() {
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
        let result = run_request(provider.clone(), workspace.clone()).await;
        let latency_ms = request_started.elapsed().as_millis() as u64;
        let mut current = metrics.borrow_mut();
        current.record_result(&result, latency_ms);
        current.apply_circuit_breakers(token_budget);
    }
}

async fn run_request(provider: CappedProvider, workspace: PathBuf) -> ncx_core::TurnResult {
    let policy = SandboxPolicy::new(READ_ONLY, &workspace);
    let tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
    let session = Session::new("Reply with exactly OK and no other text.");
    let mut agent =
        AgentLoop::new(Box::new(provider), tools, session).with_task_budget(TaskBudget {
            max_model_calls: 1,
            max_tool_calls: 0,
        });
    agent.run_turn(json!("OK"), None).await
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
            "soak_progress elapsed_s={} started={} completed={} in_flight={} exact_ok={} errors={} tokens={}",
            started_at.elapsed().as_secs(),
            current.started,
            current.completed,
            current.in_flight,
            current.exact_ok,
            current.provider_errors,
            current.total_tokens()
        );
    }
}

fn print_summary(metrics: &Metrics, elapsed: Duration, config: &Config) {
    let mut latencies = metrics.latencies_ms.clone();
    latencies.sort_unstable();
    let throughput = if elapsed.is_zero() {
        0.0
    } else {
        metrics.completed as f64 / elapsed.as_secs_f64()
    };
    println!(
        "{}",
        json!({
            "duration_target_s": config.duration.as_secs(),
            "duration_actual_s": elapsed.as_secs_f64(),
            "concurrency": CONCURRENCY,
            "max_in_flight": metrics.max_in_flight,
            "started": metrics.started,
            "completed": metrics.completed,
            "exact_ok": metrics.exact_ok,
            "semantic_mismatches": metrics.semantic_mismatches,
            "provider_errors": metrics.provider_errors,
            "rate_limits": metrics.rate_limits,
            "timeouts": metrics.timeouts,
            "requests_per_second": throughput,
            "latency_ms": {
                "p50": percentile(&latencies, 50),
                "p95": percentile(&latencies, 95),
                "p99": percentile(&latencies, 99),
                "max": latencies.last().copied().unwrap_or(0),
            },
            "prompt_tokens": metrics.prompt_tokens,
            "completion_tokens": metrics.completion_tokens,
            "total_tokens": metrics.total_tokens(),
            "stop_reason": metrics.stop_reason.as_deref().unwrap_or("duration_elapsed"),
        })
    );
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
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
        let base_url = std::env::var("NCX_SOAK_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com/beta".to_string());
        let model =
            std::env::var("NCX_SOAK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".to_string());
        let token_budget = std::env::var("NCX_SOAK_MAX_TOTAL_TOKENS")
            .ok()
            .map(|value| value.parse::<i64>())
            .transpose()
            .map_err(|_| "NCX_SOAK_MAX_TOTAL_TOKENS must be an integer".to_string())?
            .unwrap_or(DEFAULT_TOKEN_BUDGET);
        if token_budget < 1 {
            return Err("NCX_SOAK_MAX_TOTAL_TOKENS must be positive".to_string());
        }
        let workspace = std::env::current_dir()
            .map_err(|error| format!("failed to resolve current directory: {error}"))?;
        Ok(Config {
            api_key,
            base_url,
            model,
            duration: Duration::from_secs(duration_secs),
            token_budget,
            workspace,
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
    fn percentile_uses_nearest_rank() {
        let values = [1, 2, 3, 4, 5];
        assert_eq!(percentile(&values, 50), 3);
        assert_eq!(percentile(&values, 95), 5);
    }

    #[test]
    fn duration_defaults_and_parses_override() {
        assert_eq!(duration_arg(std::iter::empty()).unwrap(), 1_800);
        assert_eq!(
            duration_arg(["--duration-secs".into(), "30".into()].into_iter()).unwrap(),
            30
        );
    }
}
