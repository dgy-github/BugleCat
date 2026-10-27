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

use crate::agent_state::{AgentState, StateRecorder};
use crate::hooks::{run_matching_hooks, HookEvent};
pub use crate::model_provider::Provider;
use crate::model_provider::StreamDelta;
use crate::runtime_profile::AgentRuntimeProfile;
use crate::session::{ContextEditPolicy, ContextEditStats, Session};
use crate::tool_scheduler::{BoundedToolScheduler, ToolScheduler};
use crate::tools::ToolRegistry;
use crate::turn_context::{TurnContextProvider, TurnContextRegistry};

const DEFAULT_MAX_PARALLEL_TOOL_CALLS: usize = 8;
/// Token allowance for injected recall notes. Deliberately small: notes are
/// pointers into recorded history, and the model reads the real record through
/// `recall_sources` when it needs the detail.
const RETRIEVAL_BUDGET_TOKENS: usize = 1_500;

mod compaction;
mod deliverable;
mod dsml;
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
    /// A streamed chunk from the model's explicit reasoning channel. It is
    /// rendered separately from user-visible answers and tool execution logs.
    ReasoningDelta(String),
    /// Old tool-heavy history was compacted and persisted before a model call.
    ContextCompacted(ContextEditStats),
    /// Host verification status for the current draft. No probability is a
    /// calibrated correctness percentage.
    Verification {
        status: String,
        candidate_hash: String,
    },
    /// The assistant's final visible text for this step. The UI finalizes the
    /// streamed bubble with this authoritative text (or creates one if no deltas).
    AssistantText {
        text: String,
        model: String,
        confirmed_model: Option<String>,
    },
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
    /// Wall-clock execution budget; cancellation drains tool results before returning.
    pub turn_timeout: std::time::Duration,
    pub task_budget: TaskBudget,
    pub context_edit: ContextEditPolicy,
    pub context_window: i64,
    resolved_context_window: Option<i64>,
    context_usage: std::cell::Cell<Option<(usize, i64)>>,
    pub reasoning_effort: Option<String>,
    pub verification: crate::verification::Config,
    /// Test-only Jev key and transport for the pre-execution action review.
    #[cfg(test)]
    pub(crate) action_review_override: Option<(String, Rc<dyn ncx_provider::jev::JevTransport>)>,
    max_parallel_tool_calls: usize,
    tool_scheduler: Box<dyn ToolScheduler>,
    turn_context: TurnContextRegistry,
    next_turn_id: u64,
    use_vision_this_turn: bool,
    event_sink: Option<EventSink>,
    llm_capabilities: Option<Rc<crate::plugins::LlmServiceDescriptor>>,
    policy_service: Option<Rc<crate::plugins::PolicyService>>,
    interaction_service: Option<Rc<crate::plugins::InteractionService>>,
    context_service: Option<Rc<crate::plugins::ContextServiceDescriptor>>,
    media_service: Option<Rc<crate::plugins::MediaServiceDescriptor>>,
    cost_service: Option<Rc<crate::plugins::CostTelemetryService>>,
    pub(crate) state_machine: Rc<StateRecorder>,
    retrieval_provider: Option<Rc<crate::context_retrieval::RetrievalContextProvider>>,
    pub task_role: crate::outcome::TaskRole,
    pub parent_task_id: Option<String>,
    pub fast_model: Option<String>,
    pub feedback_memory_mode: String,
    pub feedback_routing_mode: String,
    pub(crate) last_recalled_note_ids: Vec<String>,
    pub(crate) last_memory_rank_changed: Option<bool>,
    pub(crate) last_verification_status: Option<String>,
    pub(crate) last_files_touched: Vec<String>,
    /// Failed tool calls of this turn, waiting for a same-target recovery.
    pub(crate) last_failed_tools: Vec<tool_dispatch::FailedToolCall>,
    /// `(text, source_ref)` experience candidates paired within this turn.
    pub(crate) last_recovery_candidates: Vec<(String, String)>,
    /// Where outcomes go when the session has no durable event store
    /// (orchestrator workers run on log-less sessions).
    outcome_store: Option<Rc<crate::context_store::ContextStore>>,
    /// Explicit task id (orchestrator workers share one session-id clock).
    task_id_override: Option<String>,
}

impl AgentLoop {
    /// Assemble an agent from the frontend-owned LLM factory service.
    pub fn from_runtime_services(tools: ToolRegistry, session: Session) -> Result<Self, String> {
        let factory = tools
            .service::<crate::plugins::LlmProviderFactoryHandle>("llm.factory")
            .ok_or_else(|| "Harness LLM factory service is not installed".to_string())?;
        let provider = factory.0.primary();
        let vision = factory.0.vision();
        Ok(Self::new(provider, tools, session).with_vision_provider(vision))
    }

    pub fn new(provider: Box<dyn Provider>, mut tools: ToolRegistry, mut session: Session) -> Self {
        session.set_workspace(&tools.ctx.workspace);
        // Provenance recall is only offered when the session has a durable
        // trace; otherwise the tool could only ever answer "nothing recorded".
        if let Some(store) = session.trace_store() {
            tools.set_recovery_trace(store.clone());
            tools.register(Box::new(crate::context_trace::RecallSourcesTool::new(
                store.clone(),
            )));
        }
        // Fact events live beside the session log. Without a durable log the
        // recorder still drives state, it just has nowhere to persist it.
        let event_store = session
            .log_path
            .as_deref()
            .and_then(|path| path.parent())
            .and_then(|dir| {
                crate::context_store::ContextStore::open(dir.join("events.sqlite3")).ok()
            })
            .map(Rc::new);
        if let Some(store) = &event_store {
            session.set_event_store(store.clone());
        }
        let next_turn_id = event_store
            .as_ref()
            .and_then(|store| store.session_events(&session.session_id).ok())
            .into_iter()
            .flatten()
            .filter_map(|e| e.turn_id)
            .max()
            .unwrap_or(0)
            .max(
                session
                    .trace_store()
                    .map(|s| {
                        s.records()
                            .into_iter()
                            .filter_map(|r| r.turn_id)
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0),
            );
        let state_machine = Rc::new(StateRecorder::new(
            session.session_id.clone(),
            tools.ctx.workspace.clone(),
            event_store,
        ));
        tools.ctx.state_recorder = Some(state_machine.clone());
        let llm_capabilities =
            tools.service::<crate::plugins::LlmServiceDescriptor>("llm.provider");
        let policy_service = tools.service::<crate::plugins::PolicyService>("policy");
        let interaction_service =
            tools.service::<crate::plugins::InteractionService>("interaction");
        let context_service = tools.service::<crate::plugins::ContextServiceDescriptor>("context");
        let media_service = tools.service::<crate::plugins::MediaServiceDescriptor>("media");
        let cost_service = tools.service::<crate::plugins::CostTelemetryService>("cost.telemetry");
        let feedback_memory_mode = tools
            .ctx
            .memory
            .as_ref()
            .map(|m| m.feedback_mode().to_string())
            .unwrap_or_else(|| "shadow".to_string());
        let mut agent = AgentLoop {
            provider,
            vision_provider: None,
            tools,
            session,
            max_iterations: 60,
            turn_timeout: std::time::Duration::from_secs(
                std::env::var("NANOCODEX_TURN_TIMEOUT_S")
                    .ok()
                    .and_then(|raw| raw.parse::<u64>().ok())
                    .unwrap_or(600)
                    .clamp(1, 3600),
            ),
            task_budget: TaskBudget::default(),
            context_edit: ContextEditPolicy::default(),
            context_window: 1_048_576,
            resolved_context_window: None,
            context_usage: std::cell::Cell::new(None),
            reasoning_effort: None,
            verification: crate::verification::Config::from_env(),
            #[cfg(test)]
            action_review_override: None,
            max_parallel_tool_calls: DEFAULT_MAX_PARALLEL_TOOL_CALLS,
            tool_scheduler: Box::new(BoundedToolScheduler),
            turn_context: TurnContextRegistry::default(),
            next_turn_id,
            use_vision_this_turn: false,
            event_sink: None,
            llm_capabilities,
            policy_service,
            interaction_service,
            context_service,
            media_service,
            cost_service,
            state_machine,
            retrieval_provider: None,
            task_role: crate::outcome::TaskRole::Primary,
            parent_task_id: None,
            fast_model: None,
            feedback_memory_mode,
            feedback_routing_mode: "shadow".to_string(),
            last_recalled_note_ids: Vec::new(),
            last_memory_rank_changed: None,
            last_verification_status: None,
            last_files_touched: Vec::new(),
            last_failed_tools: Vec::new(),
            last_recovery_candidates: Vec::new(),
            outcome_store: None,
            task_id_override: None,
        };
        // Layered recall is registered as an ordinary turn context provider, so
        // it stays reversible and cannot mutate the session or tool registry.
        if agent.session.trace_store().is_some() || agent.session.event_store().is_some() {
            let mut provider = crate::context_retrieval::RetrievalContextProvider::new(
                agent.tools.ctx.workspace.clone(),
                agent.session.session_id.clone(),
                RETRIEVAL_BUDGET_TOKENS,
            );
            if let Some(trace) = agent.session.trace_store() {
                provider = provider.with_trace(trace.clone());
            }
            if let Some(events) = agent.session.event_store() {
                provider = provider.with_events(events.clone());
            }
            // The metadata index lives beside the Markdown memory store, so the
            // long-term layer is only available when a memory store is attached.
            if let Some(store) = &agent.tools.ctx.memory {
                if let Ok(index) = crate::memory_index::MemoryIndex::for_store(store) {
                    let _ = index
                        .rebuild_from_markdown(store, &agent.tools.ctx.workspace.to_string_lossy());
                    provider = provider.with_memory(index);
                }
            }
            let provider = Rc::new(provider);
            if agent.turn_context.register(provider.clone()).is_ok() {
                agent.retrieval_provider = Some(provider);
            }
        }
        agent
    }

    pub fn with_task_role(
        mut self,
        role: crate::outcome::TaskRole,
        parent_task_id: Option<String>,
    ) -> Self {
        self.task_role = role;
        self.parent_task_id = parent_task_id;
        self
    }

    /// Persist outcomes into `store` and record them under `task_id` instead of
    /// the session-derived id. Used for orchestrator workers, whose sessions
    /// have no durable log.
    pub fn with_outcome_sink(
        mut self,
        store: Rc<crate::context_store::ContextStore>,
        task_id: String,
    ) -> Self {
        self.outcome_store = Some(store);
        self.task_id_override = Some(task_id);
        self
    }

    /// Task id under which the most recent turn's outcome was recorded, so a
    /// host can bind a rating to exactly that task.
    pub fn last_task_id(&self) -> String {
        self.task_id_override
            .clone()
            .unwrap_or_else(|| format!("{}:turn-{}", self.session.session_id, self.next_turn_id))
    }

    fn outcome_event_store(&self) -> Option<Rc<crate::context_store::ContextStore>> {
        self.outcome_store
            .clone()
            .or_else(|| self.session.event_store().cloned())
    }

    /// The store recall reads from, so feedback scores the notes that were injected.
    fn recall_memory_store(&self) -> Option<Rc<crate::memory::MemoryStore>> {
        self.tools
            .service::<crate::plugins::MemoryServiceDescriptor>("memory")
            .and_then(|service| service.store.clone())
            .or_else(|| self.tools.ctx.memory.clone())
    }

    /// Record an explicit `/outcome good|bad [note]` against the latest primary
    /// task of this session (or, in a fresh session, of the workspace). A label
    /// overrides automatic signals in the report and revises the task's single
    /// memory score if it disagrees with the earlier automatic verdict.
    pub fn record_explicit_outcome_label(
        &mut self,
        good: bool,
        note: Option<String>,
    ) -> Result<String, String> {
        let store = self
            .outcome_event_store()
            .or_else(|| {
                let db = crate::outcome::workspace_events_db_path(
                    &self.tools.ctx.workspace,
                    self.session.log_path.as_deref(),
                );
                crate::context_store::ContextStore::open(db)
                    .ok()
                    .map(Rc::new)
            })
            .ok_or_else(|| "cannot open workspace outcome event store".to_string())?;
        let target = match crate::outcome::latest_primary_outcome(&store, &self.session.session_id)?
        {
            Some(outcome) => Some(outcome),
            None => crate::outcome::load_outcomes_and_followups(&store, None)?
                .0
                .into_iter()
                .rev()
                .find(|o| o.role == crate::outcome::TaskRole::Primary),
        };
        let Some(outcome) = target else {
            return Err("no previous primary task outcome found to label".to_string());
        };
        let memory = self.recall_memory_store();
        let ws_str = self.tools.ctx.workspace.to_string_lossy().to_string();
        crate::outcome::record_followup(
            &store,
            &ws_str,
            &outcome,
            crate::outcome::FollowupSignal::Label { good, note },
            memory
                .as_deref()
                .map(|store| crate::outcome::MemoryFeedback {
                    store,
                    mode: &self.feedback_memory_mode,
                }),
        )?;
        Ok(outcome.task_id)
    }

    /// Lifecycle state of the active task. This is intentionally separate from
    /// the model's textual reasoning and can be persisted by host adapters.
    pub fn state(&self) -> AgentState {
        self.state_machine.current()
    }

    /// Recorded lifecycle of the active task, including approval decisions.
    pub fn state_history(&self) -> Vec<crate::agent_state::StateTransition> {
        self.state_machine.history()
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
        self.context_usage.set(None);
        self.resolved_context_window = None;
        std::mem::replace(&mut self.provider, provider)
    }

    /// Return the active primary provider's model identifier.
    pub fn provider_model(&self) -> &str {
        self.provider.model()
    }

    /// Capabilities declared by the active Harness LLM plugin, if installed.
    pub fn llm_capabilities(&self) -> Option<&crate::plugins::LlmServiceDescriptor> {
        self.llm_capabilities.as_deref()
    }

    pub fn policy_service(&self) -> Option<&crate::plugins::PolicyService> {
        self.policy_service.as_deref()
    }

    pub fn has_interaction_service(&self) -> bool {
        self.interaction_service.is_some()
    }

    pub fn context_service(&self) -> Option<&crate::plugins::ContextServiceDescriptor> {
        self.context_service.as_deref()
    }

    pub fn estimated_cost(&self, result: &TurnResult) -> Option<(String, f64)> {
        let service = self.cost_service.as_ref()?;
        let prompt = result.usage.get("prompt_tokens").copied().unwrap_or(0);
        let completion = result.usage.get("completion_tokens").copied().unwrap_or(0);
        Some((
            service.config.currency.clone(),
            service.config.estimate(prompt, completion),
        ))
    }

    /// Snapshot the normalized controls applied by a frontend assembly path.
    pub fn runtime_profile(&self) -> AgentRuntimeProfile {
        let permissions = self.policy_service.as_ref().map(|policy| {
            crate::runtime_profile::RuntimePermissionProfile {
                sandbox_mode: policy.sandbox.mode.clone(),
                approval_policy: policy.approval_policy.clone(),
                require_edit_approval: self.tools.ctx.require_edit_approval,
                plan_mode: policy.plan_mode,
                network_access: policy.sandbox.network_access,
            }
        });
        AgentRuntimeProfile {
            permissions: permissions.unwrap_or(crate::runtime_profile::RuntimePermissionProfile {
                sandbox_mode: self.tools.ctx.policy.mode.clone(),
                approval_policy: self.tools.ctx.approval_policy.clone(),
                require_edit_approval: self.tools.ctx.require_edit_approval,
                plan_mode: self.tools.ctx.plan_mode,
                network_access: self.tools.ctx.policy.network_access,
            }),
            task_budget: self.task_budget.clone(),
            max_parallel_tool_calls: self.max_parallel_tool_calls,
            context_edit: self.context_edit.clone(),
            context_window: self.context_window,
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

    /// Configure pre-commit host verification mode and policies.
    pub fn with_verification(mut self, config: crate::verification::Config) -> Self {
        self.verification = config;
        self
    }

    /// Install a sink that receives [`LoopEvent`]s during every turn (the GUI
    /// bridge forwards them to the frontend). Replaces any previous sink.
    pub fn set_event_sink(&mut self, sink: EventSink) {
        self.event_sink = Some(sink);
    }

    fn active_provider(&self) -> &dyn Provider {
        if self.use_vision_this_turn {
            let supports_vision = self
                .media_service
                .as_ref()
                .map(|m| m.vision)
                .unwrap_or(true)
                && self
                    .llm_capabilities
                    .as_ref()
                    .map(|caps| caps.supports_vision)
                    .unwrap_or(true);
            if supports_vision {
                if let Some(v) = &self.vision_provider {
                    if trace::enabled() {
                        eprintln!("[ncx-trace] routing image turn -> vision provider");
                    }
                    return v.as_ref();
                }
            }
        }
        self.provider.as_ref()
    }

    /// Generate a compact, task-oriented label without sending conversation
    /// history or tools. Failure is deliberately non-fatal: callers retain the
    /// deterministic title produced by the session index.
    pub async fn suggest_title(&self, user_request: &str) -> Option<String> {
        suggest_title_with_provider(self.provider.as_ref(), user_request).await
    }
}

/// Generate a compact title using an isolated provider call. GUI clients can
/// run this outside their serial command queue so navigation stays responsive.
pub async fn suggest_title_with_provider(
    provider: &dyn Provider,
    user_request: &str,
) -> Option<String> {
    let request = bounded_title_source(user_request, 2_400);
    if request.trim().is_empty() {
        return None;
    }
    let messages = vec![
        json!({
            "role": "system",
            "content": "为工作任务生成简短中文标题。只输出标题，不要解释、引号、Markdown 或句号。标题应为 6 到 18 个汉字左右，使用动宾结构，描述用户最终要完成的结果而不是背景材料；保留关键文件名、分支名、PDF、PPT、Excel 等必要标识。例如：拉取 gui-merge-featgui 分支；修复历史会话切换；整理大模型架构资料 PDF。"
        }),
        json!({"role": "user", "content": request}),
    ];
    let response = provider.chat(&messages, &[], None).await;
    if response.finish_reason == "error" {
        return None;
    }
    sanitize_generated_title(&response.content)
}

impl AgentLoop {
    async fn call_model(
        &self,
        schemas: &[Value],
        system_notes: &[String],
        sink: &mut Option<EventSink>,
    ) -> (ModelResponse, ContextEditStats) {
        // Persistent semantic compaction owns history reduction. Do not silently
        // clip its recent tail again while constructing the provider request.
        let edited = self.session.for_model_edited(
            system_notes,
            &ContextEditPolicy {
                enabled: false,
                ..self.context_edit.clone()
            },
        );
        let effort = self
            .llm_capabilities
            .as_ref()
            .filter(|caps| caps.supports_reasoning)
            .and(self.reasoning_effort.as_deref());
        // Stream the assistant text live: each delta becomes an AssistantDelta the
        // UI appends. `sink` is a local (threaded from run_turn), not borrowed
        // from self, so this does not conflict with the &self provider borrow.
        let response = self
            .active_provider()
            .chat_streaming(&edited.messages, schemas, effort, &mut |delta| {
                let event = match delta {
                    StreamDelta::Content(text) => LoopEvent::AssistantDelta(text),
                    StreamDelta::Reasoning(text) => LoopEvent::ReasoningDelta(text),
                };
                emit(sink, event);
            })
            .await;
        if response.finish_reason != "error" {
            if let Some(tokens) = response.usage.get("prompt_tokens").filter(|n| **n > 0) {
                self.context_usage
                    .set(Some((self.session.messages.len(), *tokens)));
            }
        }
        (response, edited.stats)
    }

    pub async fn run_turn(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
    ) -> TurnResult {
        // A compaction conflict fails the rest of that model turn closed. A
        // fresh human message is a new authority boundary, so it may recover
        // in the existing conversation instead of forcing a new session.
        // Automatic Goal rounds deliberately do not clear this guard.
        self.tools.ctx.compaction_read_only_recovery.set(false);
        self.run_turn_with_authority(
            user_input,
            cancel_check,
            crate::goal_tools::GoalAuthoritySource::DirectHuman,
        )
        .await
    }

    /// Admit one host-reserved automatic goal round. Callers must have already
    /// fenced the exact durable goal revision and round in the App Server.
    pub async fn run_goal_round(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
        goal_id: ncx_protocol::GoalId,
        revision: u64,
        round: u32,
    ) -> TurnResult {
        self.run_turn_with_authority(
            user_input,
            cancel_check,
            crate::goal_tools::GoalAuthoritySource::GoalRound {
                goal_id,
                revision,
                round,
            },
        )
        .await
    }

    async fn run_turn_with_authority(
        &mut self,
        user_input: Value,
        cancel_check: Option<&dyn Fn() -> bool>,
        authority: crate::goal_tools::GoalAuthoritySource,
    ) -> TurnResult {
        let turn_started_ms = crate::outcome::now_millis();
        let query_text = crate::agent_loop::turn::user_query_text(&user_input);
        if matches!(
            &authority,
            crate::goal_tools::GoalAuthoritySource::DirectHuman
        ) && self.task_role == crate::outcome::TaskRole::Primary
        {
            self.resolve_previous_primary_followup(&query_text);
        }
        self.last_recalled_note_ids.clear();
        self.last_memory_rank_changed = None;
        self.last_verification_status = None;
        self.last_files_touched.clear();
        self.last_failed_tools.clear();
        self.last_recovery_candidates.clear();

        self.next_turn_id = self.next_turn_id.wrapping_add(1).max(1);
        if matches!(
            &authority,
            crate::goal_tools::GoalAuthoritySource::DirectHuman
        ) {
            self.tools.begin_task(self.next_turn_id);
        }
        self.session.set_turn_id(self.next_turn_id);
        if let Some(provider) = &self.retrieval_provider {
            provider.set_active_turn(Some(self.next_turn_id));
            // Only a direct human turn carries a new instruction. An automatic
            // Goal round must not be able to withdraw a stored memory.
            if matches!(
                &authority,
                crate::goal_tools::GoalAuthoritySource::DirectHuman
            ) {
                provider.record_user_instruction(&query_text, self.next_turn_id);
            }
        }
        self.state_machine.begin_turn(self.next_turn_id);
        self.session
            .set_workspace_revision(self.state_machine.workspace_revision());
        let _ = self
            .state_machine
            .record(AgentState::Planned, "model produced a plan for the task");
        let _ = self
            .state_machine
            .record(AgentState::Authorized, "turn authority attested by host");
        let _ = self
            .state_machine
            .record(AgentState::Executing, "turn loop started");
        self.tools.ctx.active_turn_id.set(Some(self.next_turn_id));
        self.tools
            .ctx
            .goal_turn_authority
            .replace(Some(crate::goal_tools::GoalTurnAuthority {
                turn_id: self.next_turn_id,
                source: authority.clone(),
            }));
        // Take the sink out so the inner loop can emit through a local without
        // borrow-conflicting with `&mut self`; restore it after (one return path).
        let mut sink = self.event_sink.take();
        let result = turn::run(self, user_input, cancel_check, &mut sink).await;
        let _ = self
            .state_machine
            .record(AgentState::Observing, "turn loop returned");
        let candidate_before_hook = result.final_text.clone();
        let result = self.apply_stop_hook(result, &mut sink).await;
        if self.verification.mode != crate::verification::Mode::Off
            && result.final_text != candidate_before_hook
        {
            emit(
                &mut sink,
                LoopEvent::Verification {
                    status: "invalidated".into(),
                    candidate_hash: crate::context_trace::content_hash(&result.final_text),
                },
            );
        }
        // The turn's own stop reason decides the terminal state. A turn that ran
        // out of budget or was cancelled is not a completed task.
        let (terminal, trigger) = match result.stop_reason.as_str() {
            "cancelled" => (AgentState::Cancelled, "user cancelled the turn"),
            "timeout" => (AgentState::NeedsHuman, "turn timed out"),
            "task_budget" => (AgentState::NeedsHuman, "task budget exhausted"),
            "compaction_failed" => (AgentState::NeedsHuman, "context compaction failed"),
            "blocked" => (AgentState::NeedsHuman, "turn blocked before execution"),
            "verification_failed" => (
                AgentState::NeedsHuman,
                "completion claim lacks verified evidence",
            ),
            "error" => (AgentState::Failed, "model service call failed"),
            other => {
                // Post-edit validation failures arrive as their own reason and
                // must not be reported as a completed task.
                if other.contains("compile") || other.contains("validation") {
                    (AgentState::Failed, "post-edit validation failed")
                } else {
                    (AgentState::Completed, "turn finished")
                }
            }
        };
        let _ = self.state_machine.record(terminal, trigger);
        self.tools.ctx.goal_turn_authority.replace(None);
        self.tools.ctx.active_turn_id.set(None);
        self.event_sink = sink;
        if let Some(service) = &self.cost_service {
            service.record(&result.usage);
        }
        self.record_turn_outcome(&query_text, &authority, terminal, &result, turn_started_ms);
        result
    }

    /// Resolve the previous primary task of this session when the human speaks
    /// again: manual edit (I5) > inferred correction (I4) > accepted. A task
    /// that already has a followup (label, rollback, earlier resolution) is
    /// left alone, so each task is auto-resolved at most once.
    fn resolve_previous_primary_followup(&mut self, next_query: &str) {
        let Some(store) = self.outcome_event_store() else {
            return;
        };
        let memory = self.recall_memory_store();
        crate::outcome::resolve_previous_primary_followup(
            &store,
            &self.tools.ctx.workspace,
            &self.session.session_id,
            next_query,
            memory
                .as_deref()
                .map(|store| crate::outcome::MemoryFeedback {
                    store,
                    mode: &self.feedback_memory_mode,
                }),
        );
    }

    fn record_turn_outcome(
        &mut self,
        query_text: &str,
        authority: &crate::goal_tools::GoalAuthoritySource,
        terminal: AgentState,
        result: &TurnResult,
        turn_started_ms: u64,
    ) {
        let now_ms = crate::outcome::now_millis();
        let duration_ms = now_ms.saturating_sub(turn_started_ms);
        let history = self.state_machine.history();
        let approvals_requested = history
            .iter()
            .filter(|t| {
                t.turn_id == Some(self.next_turn_id) && t.to == AgentState::AwaitingConfirmation
            })
            .count() as u32;
        let approvals_denied = history
            .iter()
            .filter(|t| {
                t.turn_id == Some(self.next_turn_id)
                    && (t.permission_decision.as_deref() == Some("denied")
                        || (t.from == AgentState::AwaitingConfirmation
                            && t.to == AgentState::Blocked))
            })
            .count() as u32;
        let user_questions = result
            .tools_used
            .iter()
            .filter(|t| t.as_str() == "ask_user_question")
            .count() as u32;

        let role = match authority {
            crate::goal_tools::GoalAuthoritySource::GoalRound { .. } => {
                crate::outcome::TaskRole::GoalRound
            }
            crate::goal_tools::GoalAuthoritySource::DirectHuman => self.task_role,
        };
        let auth_source = match authority {
            crate::goal_tools::GoalAuthoritySource::DirectHuman => {
                if role == crate::outcome::TaskRole::Primary {
                    crate::agent_state::AuthoritySource::DirectHuman
                } else {
                    crate::agent_state::AuthoritySource::SubagentTask
                }
            }
            crate::goal_tools::GoalAuthoritySource::GoalRound { .. } => {
                crate::agent_state::AuthoritySource::GoalRound
            }
        };

        let has_deliverable = !self.last_files_touched.is_empty() || !result.tools_used.is_empty();
        let category =
            crate::outcome::TaskCategory::classify(query_text, &result.tools_used, has_deliverable);
        let permission_mode = self
            .policy_service
            .as_ref()
            .map(|p| {
                if p.plan_mode {
                    "plan".to_string()
                } else if self.tools.ctx.require_edit_approval {
                    "default".to_string()
                } else {
                    ncx_config::derive_permission_mode(&p.sandbox.mode).to_string()
                }
            })
            .unwrap_or_else(|| "accept-edits".to_string());
        // Only a recorded verification verdict may say "passed"; a completed
        // turn without one is unverified, not verified.
        let verification = self.last_verification_status.clone().or_else(|| {
            if self.verification.mode == crate::verification::Mode::Off {
                Some("skipped".to_string())
            } else if result.stop_reason == "verification_failed" {
                Some("failed".to_string())
            } else {
                Some("unverified".to_string())
            }
        });
        let file_hashes = crate::outcome::snapshot_file_hashes(
            &self.tools.ctx.workspace,
            &self.last_files_touched,
        );
        let usage = crate::outcome::Usage {
            prompt_tokens: result
                .usage
                .get("prompt_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
            completion_tokens: result
                .usage
                .get("completion_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
            total_tokens: result
                .usage
                .get("total_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
        };
        let task_id = self
            .task_id_override
            .clone()
            .unwrap_or_else(|| format!("{}:turn-{}", self.session.session_id, self.next_turn_id));
        let outcome = crate::outcome::TaskOutcome {
            task_id,
            parent_task_id: self.parent_task_id.clone(),
            session_id: self.session.session_id.clone(),
            turn_id: self.next_turn_id,
            role,
            authority: auth_source,
            category,
            query_hash: crate::outcome::hash_text(query_text),
            model: self.provider.model().to_string(),
            fast_model: self.fast_model.clone(),
            permission_mode,
            route: None,
            terminal_state: format!("{terminal:?}"),
            stop_reason: result.stop_reason.clone(),
            verification,
            iterations: result.iterations,
            tool_calls: result.tools_used.len(),
            tools_used: result.tools_used.clone(),
            approvals_requested,
            approvals_denied,
            user_questions,
            recalled_note_ids: self.last_recalled_note_ids.clone(),
            files_touched: self.last_files_touched.clone(),
            file_hashes,
            workspace_revision: self.state_machine.workspace_revision(),
            usage,
            duration_ms,
            timestamp_ms: now_ms,
            memory_rank_changed: self.last_memory_rank_changed,
        };
        if let Some(store) = self.outcome_event_store() {
            let _ = crate::outcome::append_task_outcome(&store, &outcome);
        }
        // A failure->recovery pair only becomes a review candidate when the
        // whole task finished and nothing contradicted it. Orchestrator workers
        // defer candidate persistence to `HarnessAgentRunner`, which remaps
        // scratch workspace paths and only commits candidates from the winning
        // promoted worker.
        let verification_ok = matches!(
            outcome.verification.as_deref(),
            Some("passed") | Some("skipped")
        );
        if role == crate::outcome::TaskRole::Primary {
            if self.feedback_memory_mode == "on"
                && outcome.verification.as_deref() != Some("failed")
                && crate::tools::python_check::failure(&self.tools.ctx).is_none()
            {
                if let Some(mem) = self.recall_memory_store() {
                    if let Ok(idx) = crate::memory_index::MemoryIndex::for_store(&mem) {
                        let ws_key = mem.workspace_key();
                        let ts_sec = now_ms / 1_000;
                        let auto_promote = terminal == AgentState::Completed && verification_ok;
                        for (text, source_ref) in &self.last_recovery_candidates {
                            let tags = vec!["recovery".to_string()];
                            if let Ok(note_id) = idx.record_candidate(
                                &ws_key,
                                text,
                                &tags,
                                &crate::memory_index::Evidence::VerifiedToolResult {
                                    source_ref: source_ref.clone(),
                                },
                                ts_sec,
                            ) {
                                if auto_promote {
                                    let _ = mem.remember(text, &tags, ts_sec);
                                    let _ = idx.confirm_candidate(&note_id);
                                }
                            }
                        }
                        if let Some((note_text, tags)) = synthesize_task_experience_note(
                            &self.tools.ctx.workspace,
                            query_text,
                            &result.final_text,
                            &self.last_files_touched,
                            terminal == AgentState::Completed,
                        ) {
                            let source_ref = format!(
                                "{}#turn-{}:task",
                                self.session.session_id, self.next_turn_id
                            );
                            if let Ok(note_id) = idx.record_candidate(
                                &ws_key,
                                &note_text,
                                &tags,
                                &crate::memory_index::Evidence::VerifiedToolResult { source_ref },
                                ts_sec,
                            ) {
                                if auto_promote {
                                    let _ = mem.remember(&note_text, &tags, ts_sec);
                                    let _ = idx.confirm_candidate(&note_id);
                                }
                            }
                        }
                    }
                }
            } else if terminal == AgentState::Completed
                && verification_ok
                && !self.last_recovery_candidates.is_empty()
            {
                if let Some(mem) = self.recall_memory_store() {
                    if let Ok(idx) = crate::memory_index::MemoryIndex::for_store(&mem) {
                        let ws_key = mem.workspace_key();
                        let ts_sec = now_ms / 1_000;
                        for (text, source_ref) in &self.last_recovery_candidates {
                            let _ = idx.record_candidate(
                                &ws_key,
                                text,
                                &["recovery".to_string()],
                                &crate::memory_index::Evidence::VerifiedToolResult {
                                    source_ref: source_ref.clone(),
                                },
                                ts_sec,
                            );
                        }
                    }
                }
            }
        }
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
        emit(
            sink,
            LoopEvent::AssistantText {
                text: note.clone(),
                model: self.provider.model().to_string(),
                confirmed_model: self.provider.confirmed_model(),
            },
        );
        result.final_text.push_str("\n\n");
        result.final_text.push_str(&note);
        result
    }
}

fn bounded_title_source(text: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let head = max_chars / 2;
    let tail = max_chars - head;
    format!(
        "{}\n[中间背景已省略]\n{}",
        chars[..head].iter().collect::<String>(),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

fn sanitize_generated_title(text: &str) -> Option<String> {
    let first = text.lines().find(|line| !line.trim().is_empty())?.trim();
    let first = first
        .strip_prefix("标题：")
        .or_else(|| first.strip_prefix("标题:"))
        .or_else(|| first.strip_prefix("Title:"))
        .unwrap_or(first)
        .trim();
    let decorators = |c: char| {
        c.is_whitespace() || matches!(c, '*' | '#' | '`' | '"' | '\'' | '“' | '”' | '‘' | '’')
    };
    let punctuation = |c: char| matches!(c, '。' | '.' | '！' | '!' | '？' | '?' | '；' | ';');
    let title = first
        .trim_matches(decorators)
        .trim_end_matches(punctuation)
        .trim_matches(decorators)
        .trim();
    let len = title.chars().count();
    (2..=36).contains(&len).then(|| title.to_string())
}

fn is_scratch_file(rel_path: &str) -> bool {
    let p = std::path::Path::new(rel_path);
    for comp in p.components() {
        if let std::path::Component::Normal(os) = comp {
            if let Some(s) = os.to_str() {
                let lower = s.to_ascii_lowercase();
                if lower.starts_with("debug")
                    || lower.starts_with("repro")
                    || lower.starts_with("temp")
                    || lower.starts_with("tmp")
                    || lower.starts_with("_tmp")
                    || lower.starts_with("scratch")
                {
                    return true;
                }
            }
        }
    }
    false
}

fn extract_issue_summary(query_text: &str) -> String {
    let lines: Vec<&str> = query_text.lines().map(str::trim).collect();
    if let Some(line) = lines.iter().copied().find(|l| l.starts_with("Issue:")) {
        let rest = line.trim_start_matches("Issue:").trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    if let Some((idx, header)) = lines.iter().copied().enumerate().find(|(_, l)| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("github issue #") || lower.starts_with("issue #")
    }) {
        let issue_tag = header
            .split_whitespace()
            .find(|tok| tok.starts_with('#'))
            .map(|tok| tok.trim_end_matches(':'))
            .unwrap_or(header.trim_end_matches(':'));
        let title_line = lines[idx + 1..]
            .iter()
            .copied()
            .find(|l| !l.is_empty() && *l != "---" && !l.starts_with("```"))
            .unwrap_or("");
        return if title_line.is_empty() {
            issue_tag.to_string()
        } else {
            format!("{issue_tag}: {title_line}")
        };
    }
    if let Some(line) = lines
        .iter()
        .copied()
        .find(|l| l.starts_with('#') && l.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
    {
        return line.to_string();
    }
    lines
        .into_iter()
        .find(|l| {
            !l.is_empty()
                && !l.starts_with("You are working in a checkout")
                && !l.starts_with("Resolve the following")
        })
        .unwrap_or("task")
        .to_string()
}

fn synthesize_task_experience_note(
    workspace: &std::path::Path,
    query_text: &str,
    final_text: &str,
    files_touched: &[String],
    completed: bool,
) -> Option<(String, Vec<String>)> {
    let all_existing: Vec<String> = files_touched
        .iter()
        .map(|f| f.replace('\\', "/"))
        .filter(|f| {
            !f.starts_with(".nanocodex") && !f.starts_with(".ncx") && workspace.join(f).is_file()
        })
        .collect();
    if all_existing.is_empty() {
        return None;
    }
    let non_scratch: Vec<String> = all_existing
        .iter()
        .filter(|f| !is_scratch_file(f))
        .cloned()
        .collect();
    let existing_files = if non_scratch.is_empty() {
        all_existing
    } else {
        non_scratch
    };
    let non_test: Vec<String> = existing_files
        .iter()
        .filter(|f| !f.starts_with("tests/") && !f.contains("/tests/"))
        .cloned()
        .collect();
    let existing_files = if non_test.is_empty() {
        existing_files
    } else {
        non_test
    };

    let mut tags: Vec<String> = Vec::new();
    let mut push_tag = |raw: &str| {
        let t = raw.trim().to_ascii_lowercase();
        let t = t.strip_prefix("test_").unwrap_or(&t);
        if t.len() >= 3
            && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !matches!(
                t,
                "src"
                    | "lib"
                    | "mod"
                    | "init"
                    | "__init__"
                    | "tests"
                    | "test"
                    | "regressions"
                    | "regression"
            )
            && !tags.iter().any(|x| x == t)
        {
            tags.push(t.to_string());
        }
    };
    for file in &existing_files {
        let p = std::path::Path::new(file);
        let parent_comps: Vec<&str> = p
            .parent()
            .into_iter()
            .flat_map(|d| d.components())
            .filter_map(|c| match c {
                std::path::Component::Normal(os) => os.to_str(),
                _ => None,
            })
            .collect();
        // Skip top-level package folder (e.g. `sqlparse/`) so every note in the
        // same repository does not share a generic repo-wide tag.
        for s in parent_comps.iter().skip(1) {
            push_tag(s);
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            push_tag(stem);
        }
    }
    if tags.is_empty() {
        for file in &existing_files {
            if let Some(stem) = std::path::Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
            {
                let t = stem.trim().to_ascii_lowercase();
                if t.len() >= 3 && !tags.contains(&t) {
                    tags.push(t);
                }
            }
        }
    }
    tags.truncate(6);

    let issue_line = extract_issue_summary(query_text);
    let issue_summary: String = issue_line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(100)
        .collect();

    let files_joined = existing_files.join("`, `");
    let text_summary = if completed {
        let content_lines: Vec<String> = final_text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("```"))
            .map(|l| {
                l.trim_start_matches("- ")
                    .trim_start_matches("* ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|l| !l.is_empty())
            .collect();
        let substantive: Vec<&str> =
            if content_lines.len() > 1 && content_lines[0].split_whitespace().count() <= 4 {
                content_lines[1..]
                    .iter()
                    .take(2)
                    .map(String::as_str)
                    .collect()
            } else {
                content_lines.iter().take(2).map(String::as_str).collect()
            };
        (!substantive.is_empty())
            .then(|| substantive.join("; ").chars().take(240).collect::<String>())
    } else {
        None
    };
    let diff_summary = extract_git_diff_summary(workspace, &existing_files);
    let summary_line = match (text_summary, diff_summary) {
        (Some(t), Some(d)) if !t.contains(&d) => Some(format!("{t} ({d})")),
        (Some(t), _) => Some(t),
        (None, Some(d)) => Some(d),
        (None, None) => None,
    };

    let note_text = match summary_line {
        Some(summary) => format!("Task \"{issue_summary}\" modified `{files_joined}` — {summary}"),
        None => format!("Task \"{issue_summary}\" modified `{files_joined}`."),
    };
    Some((note_text, tags))
}

fn extract_git_diff_summary(workspace: &std::path::Path, files: &[String]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(workspace).args(["diff", "-U0", "--"]);
    for f in files.iter().take(3) {
        cmd.arg(f);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let diff = String::from_utf8_lossy(&out.stdout);
    let mut funcs: Vec<String> = Vec::new();
    let mut changes: Vec<String> = Vec::new();
    for line in diff.lines() {
        if let Some((_, ctx)) = line.split_once(" @@ ") {
            let ctx = ctx.trim();
            if !ctx.is_empty() {
                let short_ctx: String = ctx.chars().take(60).collect();
                if !funcs.contains(&short_ctx) && funcs.len() < 2 {
                    funcs.push(short_ctx);
                }
            }
        } else if (line.starts_with('+') && !line.starts_with("+++"))
            || (line.starts_with('-') && !line.starts_with("---"))
        {
            let prefix = &line[..1];
            let body = line[1..].trim();
            if !body.is_empty()
                && !body.starts_with('#')
                && !body.starts_with("//")
                && changes.len() < 2
            {
                let compact: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
                let clipped: String = compact.chars().take(70).collect();
                changes.push(format!("{prefix}{clipped}"));
            }
        }
    }
    match (funcs.is_empty(), changes.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("in `{}`", funcs.join("`, `"))),
        (true, false) => Some(changes.join(" / ")),
        (false, false) => Some(format!(
            "in `{}`: {}",
            funcs.join("`, `"),
            changes.join(" / ")
        )),
    }
}

fn dump_args(arguments: &Value) -> String {
    serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string())
}

// ── tests (mirror tests/test_loop.py) ─────────────────────────────────────────

#[cfg(test)]
mod tests;
