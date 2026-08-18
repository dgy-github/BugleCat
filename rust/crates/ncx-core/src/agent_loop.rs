//! The agent turn loop — Rust port of `nanocodex/agent/loop.py`.
//!
//! Drives one user turn: call model → run tools → feed results → repeat until
//! the model answers without tool calls, the step cap is hit, or the user stops.
//! A run of consecutive read-only tool calls runs concurrently; a write/unknown
//! tool stays serial and in order. Image-bearing turns route to the optional
//! vision provider.

use ncx_provider::ModelResponse;
use serde_json::{json, Value};
use std::rc::Rc;

use crate::hooks::{run_matching_hooks, HookEvent};
pub use crate::model_provider::Provider;
use crate::runtime_profile::AgentRuntimeProfile;
use crate::session::{ContextEditPolicy, ContextEditStats, Session};
use crate::tool_scheduler::{BoundedToolScheduler, ToolScheduler};
use crate::tools::ToolRegistry;
use crate::turn_context::{TurnContextProvider, TurnContextRegistry};

const DEFAULT_MAX_PARALLEL_TOOL_CALLS: usize = 8;

mod tool_dispatch;
mod trace;
mod turn;

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub final_text: String,
    pub iterations: usize,
    pub stop_reason: String,
    pub tools_used: Vec<String>,
    pub usage: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBudget {
    /// Maximum model calls for a single user task.
    pub max_model_calls: usize,
    /// Maximum tool calls for a single user task.
    pub max_tool_calls: usize,
}

impl Default for TaskBudget {
    fn default() -> Self {
        TaskBudget {
            max_model_calls: 60,
            max_tool_calls: 120,
        }
    }
}

/// Progress events emitted during a turn, for a UI to render live activity.
/// The GUI bridge forwards these to the frontend; the CLI ignores them.
#[derive(Debug, Clone)]
pub enum LoopEvent {
    /// A streamed chunk of assistant text (token delta). The UI appends it to the
    /// in-progress assistant bubble.
    AssistantDelta(String),
    /// The assistant's final visible text for this step. The UI finalizes the
    /// streamed bubble with this authoritative text (or creates one if no deltas).
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
    pub task_budget: TaskBudget,
    pub context_edit: ContextEditPolicy,
    pub reasoning_effort: Option<String>,
    max_parallel_tool_calls: usize,
    tool_scheduler: Box<dyn ToolScheduler>,
    turn_context: TurnContextRegistry,
    next_turn_id: u64,
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
            task_budget: TaskBudget::default(),
            context_edit: ContextEditPolicy::default(),
            reasoning_effort: None,
            max_parallel_tool_calls: DEFAULT_MAX_PARALLEL_TOOL_CALLS,
            tool_scheduler: Box::new(BoundedToolScheduler),
            turn_context: TurnContextRegistry::default(),
            next_turn_id: 0,
            use_vision_this_turn: false,
            event_sink: None,
        }
    }

    pub fn with_max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = n;
        self.task_budget.max_model_calls = n;
        self
    }

    pub fn with_task_budget(mut self, budget: TaskBudget) -> Self {
        let budget = TaskBudget {
            max_model_calls: budget.max_model_calls.max(1),
            max_tool_calls: budget.max_tool_calls,
        };
        self.max_iterations = budget.max_model_calls;
        self.task_budget = budget;
        self
    }

    /// Bound concurrent read-only tool calls while preserving model order.
    ///
    /// A bounded pool avoids oversubscribing the process when a model emits a
    /// large batch of independent searches. Runtime entry points should pass
    /// the resolved config value; direct library users receive a default of 8.
    pub fn with_max_parallel_tool_calls(mut self, n: usize) -> Self {
        self.max_parallel_tool_calls = n.max(1);
        self
    }

    /// Replace the tool scheduler while retaining runtime-owned ordering and
    /// read-only concurrency checks.
    pub fn with_tool_scheduler(mut self, scheduler: Box<dyn ToolScheduler>) -> Self {
        self.tool_scheduler = scheduler;
        self
    }

    /// Replace the primary model provider without rebuilding tools or session.
    pub fn replace_provider(&mut self, provider: Box<dyn Provider>) -> Box<dyn Provider> {
        std::mem::replace(&mut self.provider, provider)
    }

    /// Return the active primary provider's model identifier.
    pub fn provider_model(&self) -> &str {
        self.provider.model()
    }

    /// Snapshot the normalized controls applied by a frontend assembly path.
    pub fn runtime_profile(&self) -> AgentRuntimeProfile {
        AgentRuntimeProfile {
            permissions: crate::runtime_profile::RuntimePermissionProfile {
                sandbox_mode: self.tools.ctx.policy.mode.clone(),
                approval_policy: self.tools.ctx.approval_policy.clone(),
                require_edit_approval: self.tools.ctx.require_edit_approval,
                plan_mode: self.tools.ctx.plan_mode,
                network_access: self.tools.ctx.policy.network_access,
            },
            task_budget: self.task_budget.clone(),
            max_parallel_tool_calls: self.max_parallel_tool_calls,
            context_edit: self.context_edit.clone(),
        }
    }

    /// Register a named source of query-scoped context notes.
    pub fn register_context_provider(
        &mut self,
        provider: Rc<dyn TurnContextProvider>,
    ) -> Result<(), String> {
        self.turn_context.register(provider)
    }

    /// Remove a turn context provider by its stable registration name.
    pub fn unregister_context_provider(&mut self, name: &str) -> bool {
        self.turn_context.unregister(name)
    }

    pub fn with_context_edit(mut self, policy: ContextEditPolicy) -> Self {
        self.context_edit = policy;
        self
    }

    /// Route turns that carry an image block to a dedicated vision provider.
    /// When `None`, image turns stay on the main provider (no special routing).
    pub fn with_vision_provider(mut self, provider: Option<Box<dyn Provider>>) -> Self {
        self.vision_provider = provider;
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
                if trace::enabled() {
                    eprintln!("[ncx-trace] routing image turn -> vision provider");
                }
                return v.as_ref();
            }
        }
        self.provider.as_ref()
    }

    async fn call_model(
        &self,
        schemas: &[Value],
        system_notes: &[String],
        sink: &mut Option<EventSink>,
    ) -> (ModelResponse, ContextEditStats) {
        let edited = self
            .session
            .for_model_edited(system_notes, &self.context_edit);
        let effort = self.reasoning_effort.as_deref();
        // Stream the assistant text live: each delta becomes an AssistantDelta the
        // UI appends. `sink` is a local (threaded from run_turn), not borrowed
        // from self, so this does not conflict with the &self provider borrow.
        let response = self
            .active_provider()
            .chat_streaming(&edited.messages, schemas, effort, &mut |delta: String| {
                emit(sink, LoopEvent::AssistantDelta(delta));
            })
            .await;
        (response, edited.stats)
    }

    pub async fn run_turn(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
    ) -> TurnResult {
        self.next_turn_id = self.next_turn_id.wrapping_add(1).max(1);
        self.tools.ctx.active_turn_id.set(Some(self.next_turn_id));
        // Take the sink out so the inner loop can emit through a local without
        // borrow-conflicting with `&mut self`; restore it after (one return path).
        let mut sink = self.event_sink.take();
        let result = turn::run(self, user_input, cancel_check, &mut sink).await;
        let result = self.apply_stop_hook(result, &mut sink).await;
        self.tools.ctx.active_turn_id.set(None);
        self.event_sink = sink;
        result
    }

    async fn apply_stop_hook(
        &mut self,
        mut result: TurnResult,
        sink: &mut Option<EventSink>,
    ) -> TurnResult {
        let args = json!({
            "stop_reason": result.stop_reason.clone(),
            "iterations": result.iterations,
            "tools_used": result.tools_used.clone(),
        });
        let hook = run_matching_hooks(
            &self.tools.ctx.hooks,
            HookEvent::Stop,
            "stop",
            &args,
            Some(&result.final_text),
            &self.tools.ctx.workspace,
        )
        .await;
        if hook.notes.trim().is_empty() {
            return result;
        }
        let note = format!("[stop hook output]\n{}", hook.notes);
        self.session.add_assistant(&note, None, "");
        emit(sink, LoopEvent::AssistantText(note.clone()));
        result.final_text.push_str("\n\n");
        result.final_text.push_str(&note);
        result
    }
}

fn dump_args(arguments: &Value) -> String {
    serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
}

// ── tests (mirror tests/test_loop.py) ─────────────────────────────────────────

#[cfg(test)]
mod tests;
