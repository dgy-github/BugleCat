//! The agent turn loop — Rust port of `nanocodex/agent/loop.py`.
//!
//! Drives one user turn: call model → run tools → feed results → repeat until
//! the model answers without tool calls, the step cap is hit, or the user stops.
//! A run of consecutive read-only tool calls runs concurrently; a write/unknown
//! tool stays serial and in order. Image-bearing turns route to the optional
//! vision provider.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::join_all;
use ncx_provider::{DeepSeekProvider, ModelResponse, ToolCall};
use serde_json::{json, Value};

use crate::session::Session;
use crate::tools::ToolRegistry;

/// Minimal async chat interface the loop drives. `?Send` so trait objects can
/// hold the single-threaded REPL's providers and mock closures.
#[async_trait(?Send)]
pub trait Provider {
    fn model(&self) -> &str;

    /// One completion. Implementations convert transport errors into a response
    /// with `finish_reason == "error"` so the loop can surface it uniformly.
    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse;
}

/// Adapt the real HTTP provider to the loop's trait, mapping errors to an
/// `"error"` response (mirrors how the Python loop sees `finish_reason=="error"`).
#[async_trait(?Send)]
impl Provider for DeepSeekProvider {
    fn model(&self) -> &str {
        &self.model
    }
    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse {
        let tools_opt = if tools.is_empty() { None } else { Some(tools) };
        match DeepSeekProvider::chat(self, messages, tools_opt, None, None, reasoning_effort).await {
            Ok(resp) => resp,
            Err(e) => ModelResponse {
                content: e.to_string(),
                finish_reason: "error".to_string(),
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub final_text: String,
    pub iterations: usize,
    pub stop_reason: String,
    pub tools_used: Vec<String>,
    pub usage: std::collections::BTreeMap<String, i64>,
}

/// Progress events emitted during a turn, for a UI to render live activity.
/// The GUI bridge forwards these to the frontend; the CLI ignores them.
#[derive(Debug, Clone)]
pub enum LoopEvent {
    /// The assistant produced visible text this step (non-streaming: whole message).
    AssistantText(String),
    /// A tool is about to run.
    ToolStart { name: String, args: String },
    /// A tool finished with this (possibly truncated by the UI) result.
    ToolResult { name: String, result: String },
}

/// Sink for [`LoopEvent`]s. Boxed `FnMut` so the GUI can push into a channel.
pub type EventSink = Box<dyn FnMut(LoopEvent)>;

fn emit(sink: &mut Option<EventSink>, ev: LoopEvent) {
    if let Some(s) = sink.as_mut() {
        s(ev);
    }
}

/// Drive one user turn to completion.
pub struct AgentLoop {
    provider: Box<dyn Provider>,
    pub vision_provider: Option<Box<dyn Provider>>,
    pub tools: ToolRegistry,
    pub session: Session,
    pub max_iterations: usize,
    pub reasoning_effort: Option<String>,
    use_vision_this_turn: bool,
    event_sink: Option<EventSink>,
}

impl AgentLoop {
    pub fn new(provider: Box<dyn Provider>, tools: ToolRegistry, session: Session) -> Self {
        AgentLoop {
            provider,
            vision_provider: None,
            tools,
            session,
            max_iterations: 60,
            reasoning_effort: None,
            use_vision_this_turn: false,
            event_sink: None,
        }
    }

    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n;
        self
    }

    /// Install a sink that receives [`LoopEvent`]s during every turn (the GUI
    /// bridge forwards them to the frontend). Replaces any previous sink.
    pub fn set_event_sink(&mut self, sink: EventSink) {
        self.event_sink = Some(sink);
    }

    fn active_provider(&self) -> &dyn Provider {
        if self.use_vision_this_turn {
            if let Some(v) = &self.vision_provider {
                return v.as_ref();
            }
        }
        self.provider.as_ref()
    }

    async fn call_model(&self, schemas: &[Value]) -> ModelResponse {
        let messages = self.session.for_model();
        let effort = self.reasoning_effort.as_deref();
        self.active_provider().chat(&messages, schemas, effort).await
    }

    /// Run one tool call but abandon it (drop = cancel) if `cancel` flips while
    /// it runs. Polls every 100 ms; a fast tool returns before the first poll.
    async fn execute_cancellable(
        &self,
        tc: &ToolCall,
        cancel: &Option<&dyn Fn() -> bool>,
    ) -> String {
        let fut = self.tools.execute(&tc.name, &tc.arguments);
        tokio::pin!(fut);
        loop {
            tokio::select! {
                biased;
                r = &mut fut => return r,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if let Some(c) = cancel {
                        if c() {
                            return "[interrupted: stopped by user mid-command]".to_string();
                        }
                    }
                }
            }
        }
    }

    pub async fn run_turn(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
    ) -> TurnResult {
        // Take the sink out so the inner loop can emit through a local without
        // borrow-conflicting with `&mut self`; restore it after (one return path).
        let mut sink = self.event_sink.take();
        let result = self.run_turn_inner(user_input, cancel_check, &mut sink).await;
        self.event_sink = sink;
        result
    }

    async fn run_turn_inner(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
        sink: &mut Option<EventSink>,
    ) -> TurnResult {
        self.use_vision_this_turn =
            self.vision_provider.is_some() && has_image_block(&user_input);
        self.session.add_user(user_input);

        let mut tools_used: Vec<String> = Vec::new();
        let schemas = self.tools.schemas();
        let mut turn_usage: std::collections::BTreeMap<String, i64> = Default::default();

        let cancelled = || cancel_check.map(|c| c()).unwrap_or(false);

        for iteration in 0..self.max_iterations {
            if cancelled() {
                let text = "Stopped by user.".to_string();
                self.session.add_assistant(&text, None, "");
                return TurnResult {
                    final_text: text,
                    iterations: iteration + 1,
                    stop_reason: "cancelled".into(),
                    tools_used,
                    usage: turn_usage,
                };
            }

            let response = self.call_model(&schemas).await;
            add_usage(&mut turn_usage, &response.usage);
            if trace_on() {
                eprintln!(
                    "[ncx-trace] iter={} finish={} n_tools={} content={:?}",
                    iteration,
                    response.finish_reason,
                    response.tool_calls.len(),
                    truncate(&response.content, 120)
                );
                for tc in &response.tool_calls {
                    eprintln!("[ncx-trace]   call {} args={}", tc.name, truncate(&tc.arguments.to_string(), 200));
                }
            }

            if response.finish_reason == "error" {
                let text = if response.content.is_empty() {
                    "Model call failed.".to_string()
                } else {
                    response.content.clone()
                };
                self.session.add_assistant(&text, None, "");
                return TurnResult {
                    final_text: text,
                    iterations: iteration + 1,
                    stop_reason: "error".into(),
                    tools_used,
                    usage: turn_usage,
                };
            }

            if !response.has_tool_calls() {
                let text = response.content.clone();
                self.session.add_assistant(&text, None, &response.reasoning);
                if !text.is_empty() {
                    emit(sink, LoopEvent::AssistantText(text.clone()));
                }
                return TurnResult {
                    final_text: text,
                    iterations: iteration + 1,
                    stop_reason: "completed".into(),
                    tools_used,
                    usage: turn_usage,
                };
            }

            // Persist the assistant message carrying the tool calls.
            let openai_tool_calls: Vec<Value> = response
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {"name": tc.name, "arguments": dump_args(&tc.arguments)},
                    })
                })
                .collect();
            self.session.add_assistant(
                &response.content,
                Some(openai_tool_calls),
                &response.reasoning,
            );

            let calls = &response.tool_calls;
            let n_calls = calls.len();
            let mut idx = 0usize;

            while idx < n_calls {
                // Stop check BEFORE starting the next tool / batch.
                if cancelled() {
                    return self.cancel_result(true, iteration, tools_used, turn_usage);
                }

                let parallel_run = self.tools.is_read_only(&calls[idx].name)
                    && idx + 1 < n_calls
                    && self.tools.is_read_only(&calls[idx + 1].name);

                if parallel_run {
                    // Gather the run of consecutive read-only calls.
                    let mut batch: Vec<&ToolCall> = Vec::new();
                    while idx < n_calls && self.tools.is_read_only(&calls[idx].name) {
                        batch.push(&calls[idx]);
                        idx += 1;
                    }
                    for tc in &batch {
                        tools_used.push(tc.name.clone());
                        emit(sink, LoopEvent::ToolStart {
                            name: tc.name.clone(),
                            args: dump_args(&tc.arguments),
                        });
                    }
                    let futures = batch
                        .iter()
                        .map(|tc| self.execute_cancellable(tc, &cancel_check));
                    let results = join_all(futures).await;
                    for (tc, result) in batch.iter().zip(results) {
                        emit(sink, LoopEvent::ToolResult {
                            name: tc.name.clone(),
                            result: result.clone(),
                        });
                        self.session.add_tool_result(&tc.id, &tc.name, &result);
                    }
                } else {
                    let tc = &calls[idx];
                    tools_used.push(tc.name.clone());
                    emit(sink, LoopEvent::ToolStart {
                        name: tc.name.clone(),
                        args: dump_args(&tc.arguments),
                    });
                    let result = self.execute_cancellable(tc, &cancel_check).await;
                    if trace_on() {
                        eprintln!("[ncx-trace]   result {} -> {:?}", tc.name, truncate(&result, 200));
                    }
                    emit(sink, LoopEvent::ToolResult {
                        name: tc.name.clone(),
                        result: result.clone(),
                    });
                    self.session.add_tool_result(&tc.id, &tc.name, &result);
                    idx += 1;
                }

                // A tool can hang; honor a Stop pressed while it ran.
                if cancelled() {
                    return self.cancel_result(false, iteration, tools_used, turn_usage);
                }
            }
        }

        let text = format!(
            "Reached the maximum of {} steps without finishing. The task may be incomplete.",
            self.max_iterations
        );
        self.session.add_assistant(&text, None, "");
        TurnResult {
            final_text: text,
            iterations: self.max_iterations,
            stop_reason: "max_iterations".into(),
            tools_used,
            usage: turn_usage,
        }
    }

    fn cancel_result(
        &mut self,
        before: bool,
        iteration: usize,
        tools_used: Vec<String>,
        turn_usage: std::collections::BTreeMap<String, i64>,
    ) -> TurnResult {
        let placeholder = if before {
            "[interrupted: stopped by user before this tool ran]"
        } else {
            "[interrupted: stopped by user]"
        };
        self.session.backfill_unanswered_tool_calls(placeholder);
        let text = "Stopped by user.".to_string();
        self.session.add_assistant(&text, None, "");
        TurnResult {
            final_text: text,
            iterations: iteration + 1,
            stop_reason: "cancelled".into(),
            tools_used,
            usage: turn_usage,
        }
    }
}

/// True when the user content carries at least one `image_url` block.
fn has_image_block(user_input: &Value) -> bool {
    user_input
        .as_array()
        .map(|blocks| {
            blocks.iter().any(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url"))
        })
        .unwrap_or(false)
}

fn dump_args(arguments: &Value) -> String {
    serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
}

fn trace_on() -> bool {
    std::env::var("NCX_TRACE").map(|v| !v.is_empty()).unwrap_or(false)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Sum token usage across model calls (mirrors `pricing.add_usage`).
fn add_usage(acc: &mut std::collections::BTreeMap<String, i64>, usage: &std::collections::BTreeMap<String, i64>) {
    for (k, v) in usage {
        *acc.entry(k.clone()).or_insert(0) += v;
    }
}

// ── tests (mirror tests/test_loop.py) ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolContext};
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use std::cell::Cell;
    use std::path::PathBuf;

    /// Returns a pre-scripted sequence of responses, one per chat() call.
    struct ScriptedProvider {
        responses: RefCell<Vec<ModelResponse>>,
        calls: RefCell<usize>,
    }
    use std::cell::RefCell;
    impl ScriptedProvider {
        fn new(responses: Vec<ModelResponse>) -> Self {
            ScriptedProvider { responses: RefCell::new(responses), calls: RefCell::new(0) }
        }
    }
    #[async_trait(?Send)]
    impl Provider for ScriptedProvider {
        fn model(&self) -> &str {
            "scripted"
        }
        async fn chat(&self, _m: &[Value], _t: &[Value], _r: Option<&str>) -> ModelResponse {
            *self.calls.borrow_mut() += 1;
            let mut r = self.responses.borrow_mut();
            if r.is_empty() {
                ModelResponse { content: "(no more scripted responses)".into(), ..Default::default() }
            } else {
                r.remove(0)
            }
        }
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ncx_loop_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn build(ws: &PathBuf, provider: Box<dyn Provider>) -> AgentLoop {
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, ws);
        let ctx = ToolContext::new(ws.clone(), policy);
        let tools = ToolRegistry::new(ctx);
        let session = Session::new("system prompt");
        AgentLoop::new(provider, tools, session).with_max_iterations(10)
    }

    fn tc(id: &str, name: &str, args: Value) -> ToolCall {
        ToolCall { id: id.into(), name: name.into(), arguments: args }
    }

    fn assistant_toolcall(calls: Vec<ToolCall>) -> ModelResponse {
        ModelResponse {
            content: String::new(),
            tool_calls: calls,
            finish_reason: "tool_calls".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn returns_final_text_without_tools() {
        let p = ScriptedProvider::new(vec![ModelResponse { content: "All done.".into(), ..Default::default() }]);
        let ws = tmpdir("notools");
        let mut loop_ = build(&ws, Box::new(p));
        let r = loop_.run_turn(json!("say hi"), None).await;
        assert_eq!(r.stop_reason, "completed");
        assert_eq!(r.final_text, "All done.");
        assert_eq!(r.iterations, 1);
    }

    #[tokio::test]
    async fn executes_apply_patch_then_finishes() {
        let patch = "*** Begin Patch\n*** Add File: out.txt\n+hello\n*** End Patch";
        let p = ScriptedProvider::new(vec![
            assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]),
            ModelResponse { content: "Created out.txt.".into(), ..Default::default() },
        ]);
        let ws = tmpdir("applypatch");
        let mut loop_ = build(&ws, Box::new(p));
        let r = loop_.run_turn(json!("create out.txt"), None).await;
        assert_eq!(std::fs::read_to_string(ws.join("out.txt")).unwrap(), "hello\n");
        assert_eq!(r.stop_reason, "completed");
        assert!(r.tools_used.contains(&"apply_patch".to_string()));
        // Second call saw a tool message in history.
        assert!(loop_.session.messages.iter().any(|m| m["role"] == "tool"));
    }

    #[tokio::test]
    async fn emits_events_for_tool_turn() {
        let patch = "*** Begin Patch\n*** Add File: ev.txt\n+hi\n*** End Patch";
        let p = ScriptedProvider::new(vec![
            assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]),
            ModelResponse { content: "done".into(), ..Default::default() },
        ]);
        let ws = tmpdir("events");
        let mut loop_ = build(&ws, Box::new(p));
        let events = std::rc::Rc::new(RefCell::new(Vec::<LoopEvent>::new()));
        let sink = events.clone();
        loop_.set_event_sink(Box::new(move |e| sink.borrow_mut().push(e)));
        loop_.run_turn(json!("create ev.txt"), None).await;
        let evs = events.borrow();
        assert!(evs.iter().any(|e| matches!(e, LoopEvent::ToolStart { name, .. } if name == "apply_patch")));
        assert!(evs.iter().any(|e| matches!(e, LoopEvent::ToolResult { name, .. } if name == "apply_patch")));
        assert!(evs.iter().any(|e| matches!(e, LoopEvent::AssistantText(t) if t == "done")));
    }

    #[tokio::test]
    async fn persists_reasoning_on_tool_call_turn() {
        let patch = "*** Begin Patch\n*** Add File: reasoned.txt\n+ok\n*** End Patch";
        let mut first = assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]);
        first.reasoning = "I need to create a file before answering.".into();
        let p = ScriptedProvider::new(vec![
            first,
            ModelResponse { content: "Created reasoned.txt.".into(), ..Default::default() },
        ]);
        let ws = tmpdir("reasoning");
        let mut loop_ = build(&ws, Box::new(p));
        loop_.run_turn(json!("create reasoned.txt"), None).await;
        let m = loop_
            .session
            .messages
            .iter()
            .find(|m| m["role"] == "assistant" && m.get("tool_calls").is_some())
            .unwrap();
        assert_eq!(m["reasoning_content"], "I need to create a file before answering.");
    }

    #[tokio::test]
    async fn runs_update_plan_and_records_state() {
        let p = ScriptedProvider::new(vec![
            {
                let mut r = assistant_toolcall(vec![tc(
                    "p1",
                    "update_plan",
                    json!({"plan": [
                        {"step": "write file", "status": "in_progress"},
                        {"step": "verify", "status": "pending"},
                    ]}),
                )]);
                r.content = "planning".into();
                r
            },
            ModelResponse { content: "done".into(), ..Default::default() },
        ]);
        let ws = tmpdir("plan");
        let mut loop_ = build(&ws, Box::new(p));
        let r = loop_.run_turn(json!("two step task"), None).await;
        assert_eq!(r.stop_reason, "completed");
        let plan = loop_.tools.ctx.plan.borrow();
        assert_eq!(plan[0]["step"], "write file");
        assert_eq!(plan[0]["status"], "in_progress");
    }

    #[tokio::test]
    async fn stops_at_max_iterations() {
        let looping: Vec<ModelResponse> = (0..20)
            .map(|i| assistant_toolcall(vec![tc(&format!("c{i}"), "read_file", json!({"path": "nope.txt"}))]))
            .collect();
        let p = ScriptedProvider::new(looping);
        let ws = tmpdir("maxiter");
        let mut loop_ = build(&ws, Box::new(p));
        let r = loop_.run_turn(json!("loop forever"), None).await;
        assert_eq!(r.stop_reason, "max_iterations");
        assert_eq!(r.iterations, 10);
    }

    fn answered(messages: &[Value]) -> bool {
        let ans: std::collections::HashSet<&str> = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .filter_map(|m| m["tool_call_id"].as_str())
            .collect();
        for m in messages {
            if m["role"] == "assistant" {
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        if !ans.contains(tc["id"].as_str().unwrap()) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    #[tokio::test]
    async fn cancel_mid_tool_loop_backfills_tool_results() {
        let p = ScriptedProvider::new(vec![assistant_toolcall(vec![
            tc("c1", "read_file", json!({"path": "a.txt"})),
            tc("c2", "read_file", json!({"path": "b.txt"})),
        ])]);
        let ws = tmpdir("cancelmid");
        let mut loop_ = build(&ws, Box::new(p));
        let n = Cell::new(0u32);
        let check = move || {
            let v = n.get();
            n.set(v + 1);
            v >= 1
        };
        let r = loop_.run_turn(json!("read two files"), Some(&check)).await;
        assert_eq!(r.stop_reason, "cancelled");
        assert!(answered(&loop_.session.messages));
        let ids: std::collections::HashSet<&str> = loop_
            .session
            .messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["c1", "c2"].into_iter().collect());
    }

    #[tokio::test]
    async fn image_turn_routes_to_vision_provider() {
        let main = ScriptedProvider::new(vec![ModelResponse { content: "text reply".into(), ..Default::default() }]);
        let vision = ScriptedProvider::new(vec![ModelResponse {
            content: "vision reply: I see a cat".into(),
            ..Default::default()
        }]);
        let ws = tmpdir("vision");
        let mut loop_ = build(&ws, Box::new(main));
        loop_.vision_provider = Some(Box::new(vision));
        let content = json!([
            {"type": "text", "text": "what's in this image?"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
        ]);
        let r = loop_.run_turn(content, None).await;
        assert_eq!(r.stop_reason, "completed");
        assert_eq!(r.final_text, "vision reply: I see a cat");
    }

    #[tokio::test]
    async fn read_only_calls_run_concurrently() {
        struct SlowReadTool;
        #[async_trait(?Send)]
        impl Tool for SlowReadTool {
            fn name(&self) -> &str {
                "slow_read"
            }
            fn description(&self) -> &str {
                "sleeps (test)"
            }
            fn parameters(&self) -> Value {
                json!({"type": "object", "properties": {"i": {"type": "integer"}}})
            }
            fn read_only(&self) -> bool {
                true
            }
            async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
                tokio::time::sleep(Duration::from_millis(300)).await;
                format!("read {}", args.get("i").and_then(|v| v.as_i64()).unwrap_or(-1))
            }
        }

        let p = ScriptedProvider::new(vec![
            assistant_toolcall(
                (0..4).map(|i| tc(&format!("c{i}"), "slow_read", json!({"i": i}))).collect(),
            ),
            ModelResponse { content: "done".into(), ..Default::default() },
        ]);
        let ws = tmpdir("concurrent");
        let mut loop_ = build(&ws, Box::new(p));
        loop_.tools.register(Box::new(SlowReadTool));

        let t0 = std::time::Instant::now();
        let r = loop_.run_turn(json!("read four things"), None).await;
        let elapsed = t0.elapsed();
        assert_eq!(r.stop_reason, "completed");
        assert!(elapsed < Duration::from_millis(800), "elapsed {elapsed:?}");
        let ids: Vec<&str> = loop_
            .session
            .messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["c0", "c1", "c2", "c3"]);
    }

    #[tokio::test]
    async fn write_between_reads_stays_serial_and_ordered() {
        let patch = "*** Begin Patch\n*** Add File: mid.txt\n+x\n*** End Patch";
        let p = ScriptedProvider::new(vec![
            assistant_toolcall(vec![
                tc("r1", "read_file", json!({"path": "none1.txt"})),
                tc("w1", "apply_patch", json!({"patch": patch})),
                tc("r2", "read_file", json!({"path": "none2.txt"})),
            ]),
            ModelResponse { content: "done".into(), ..Default::default() },
        ]);
        let ws = tmpdir("serial");
        let mut loop_ = build(&ws, Box::new(p));
        let r = loop_.run_turn(json!("read write read"), None).await;
        assert_eq!(r.stop_reason, "completed");
        assert_eq!(std::fs::read_to_string(ws.join("mid.txt")).unwrap(), "x\n");
        let ids: Vec<&str> = loop_
            .session
            .messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["r1", "w1", "r2"]);
        assert_eq!(r.tools_used, vec!["read_file", "apply_patch", "read_file"]);
    }

    #[tokio::test]
    async fn stop_interrupts_a_hanging_tool() {
        struct HangingTool;
        #[async_trait(?Send)]
        impl Tool for HangingTool {
            fn name(&self) -> &str {
                "hang"
            }
            fn description(&self) -> &str {
                "blocks forever (test)"
            }
            fn parameters(&self) -> Value {
                json!({"type": "object", "properties": {}})
            }
            async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
                std::future::pending::<()>().await;
                "unreachable".into()
            }
        }

        let p = ScriptedProvider::new(vec![assistant_toolcall(vec![tc("h1", "hang", json!({}))])]);
        let ws = tmpdir("hang");
        let mut loop_ = build(&ws, Box::new(p));
        loop_.tools.register(Box::new(HangingTool));

        let n = Cell::new(0u32);
        let check = move || {
            let v = n.get();
            n.set(v + 1);
            v >= 2
        };
        let r = tokio::time::timeout(
            Duration::from_secs(5),
            loop_.run_turn(json!("do the hang"), Some(&check)),
        )
        .await
        .expect("must finish under 5s");
        assert_eq!(r.stop_reason, "cancelled");
        assert!(loop_.session.messages.iter().any(|m| m["role"] == "tool" && m["tool_call_id"] == "h1"));
    }
}
