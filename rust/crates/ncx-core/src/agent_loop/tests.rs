use super::*;
use crate::tool_scheduler::ToolScheduler;
use crate::tools::{Tool, ToolContext};
use crate::turn_context::{TurnContextProvider, TurnContextRequest};
use async_trait::async_trait;
use ncx_config::HookConfig;
use ncx_provider::ToolCall;
use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Returns a pre-scripted sequence of responses, one per chat() call.
struct ScriptedProvider {
    responses: RefCell<Vec<ModelResponse>>,
    calls: RefCell<usize>,
}
use std::cell::RefCell;
impl ScriptedProvider {
    fn new(responses: Vec<ModelResponse>) -> Self {
        ScriptedProvider {
            responses: RefCell::new(responses),
            calls: RefCell::new(0),
        }
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
            ModelResponse {
                content: "(no more scripted responses)".into(),
                ..Default::default()
            }
        } else {
            r.remove(0)
        }
    }
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ncx_loop_{name}_{}", std::process::id()));
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

fn build_with_hooks(
    ws: &PathBuf,
    provider: Box<dyn Provider>,
    hooks: Vec<HookConfig>,
) -> AgentLoop {
    let policy = SandboxPolicy::new(WORKSPACE_WRITE, ws);
    let ctx = ToolContext::new(ws.clone(), policy).with_hooks(hooks);
    let tools = ToolRegistry::new(ctx);
    let session = Session::new("system prompt");
    AgentLoop::new(provider, tools, session).with_max_iterations(10)
}

fn tc(id: &str, name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }
}

fn assistant_toolcall(calls: Vec<ToolCall>) -> ModelResponse {
    ModelResponse {
        content: String::new(),
        tool_calls: calls,
        finish_reason: "tool_calls".into(),
        ..Default::default()
    }
}

mod basic_tests;
mod runtime_tests;

struct CapturingProvider {
    seen: Rc<RefCell<Vec<Value>>>,
}
#[async_trait(?Send)]
impl Provider for CapturingProvider {
    fn model(&self) -> &str {
        "capturing"
    }
    async fn chat(&self, messages: &[Value], _t: &[Value], _r: Option<&str>) -> ModelResponse {
        *self.seen.borrow_mut() = messages.to_vec();
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        }
    }
}

struct CountingProvider {
    calls: Rc<Cell<usize>>,
}

struct LongFailureProvider {
    calls: Cell<usize>,
    converged_without_tools: Rc<Cell<bool>>,
    saw_convergence_note: Rc<Cell<bool>>,
}

struct LongPdfDeliveryProvider {
    calls: Cell<usize>,
    tools_remained_available: Rc<Cell<bool>>,
}

#[async_trait(?Send)]
impl Provider for LongPdfDeliveryProvider {
    fn model(&self) -> &str {
        "long-pdf-delivery"
    }

    async fn chat(&self, _messages: &[Value], tools: &[Value], _r: Option<&str>) -> ModelResponse {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call < 40 {
            return assistant_toolcall(vec![tc(
                &format!("missing-pdf-{call}"),
                "read_file",
                json!({"path": format!("missing-pdf-{call}.txt")}),
            )]);
        }
        if call == 40 {
            self.tools_remained_available.set(!tools.is_empty());
            return assistant_toolcall(vec![tc(
                "create-pdf",
                "apply_patch",
                json!({
                    "patch": "*** Begin Patch\n*** Add File: final.pdf\n+%PDF-1.4\n+/Type /Page\n+%%EOF\n*** End Patch"
                }),
            )]);
        }
        ModelResponse {
            content: "PDF 已生成：final.pdf".into(),
            ..Default::default()
        }
    }
}

#[async_trait(?Send)]
impl Provider for LongFailureProvider {
    fn model(&self) -> &str {
        "long-failure"
    }

    async fn chat(&self, messages: &[Value], tools: &[Value], _r: Option<&str>) -> ModelResponse {
        let call = self.calls.get();
        self.calls.set(call + 1);
        if call < 40 {
            return assistant_toolcall(vec![tc(
                &format!("missing-{call}"),
                "read_file",
                json!({"path": format!("missing-{call}.txt")}),
            )]);
        }

        self.converged_without_tools.set(tools.is_empty());
        self.saw_convergence_note
            .set(messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("system")
                    && message
                        .get("content")
                        .and_then(Value::as_str)
                        .is_some_and(|content| content.contains("converge now"))
            }));
        ModelResponse {
            content: "final answer from existing evidence".into(),
            ..Default::default()
        }
    }
}

struct StaticContextProvider;

#[async_trait(?Send)]
impl TurnContextProvider for StaticContextProvider {
    fn name(&self) -> &str {
        "static-test-context"
    }

    async fn provide(&self, request: &TurnContextRequest) -> Vec<String> {
        vec![format!("[plugin context]\nquery={}", request.query)]
    }
}
#[async_trait(?Send)]
impl Provider for CountingProvider {
    fn model(&self) -> &str {
        "counting"
    }
    async fn chat(&self, _m: &[Value], _t: &[Value], _r: Option<&str>) -> ModelResponse {
        self.calls.set(self.calls.get() + 1);
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        }
    }
}

#[cfg(windows)]
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

struct RecordingScheduler {
    serial_calls: Rc<Cell<usize>>,
    read_batches: Rc<Cell<usize>>,
}

#[async_trait(?Send)]
impl ToolScheduler for RecordingScheduler {
    async fn execute_one(
        &self,
        tools: &ToolRegistry,
        call: &ToolCall,
        _cancel: Option<&dyn Fn() -> bool>,
    ) -> String {
        self.serial_calls.set(self.serial_calls.get() + 1);
        tools.execute(&call.name, &call.arguments).await
    }

    async fn execute_read_only_batch(
        &self,
        tools: &ToolRegistry,
        batch: &[&ToolCall],
        _max_parallel: usize,
        _cancel: Option<&dyn Fn() -> bool>,
    ) -> Vec<String> {
        self.read_batches.set(self.read_batches.get() + 1);
        let mut results = Vec::with_capacity(batch.len());
        for call in batch {
            results.push(tools.execute(&call.name, &call.arguments).await);
        }
        results
    }
}
