//! ncx-core — the agent turn loop and the pieces around it.
//!
//! Rust port of `nanocodex/agent/`:
//!
//! * [`session`] — conversation history (OpenAI message shape, tool-call backfill).
//! * [`tools`] — the `Tool` trait, `ToolRegistry`, and the core tool set.
//! * [`agent_loop`] — [`agent_loop::AgentLoop`], the call-model→run-tools loop with
//!   concurrent read-only batching, cancellation, and vision routing.
//! * [`mentions`] — `@path` file-mention expansion.
//! * [`slash`] — REPL slash-command parsing.

pub mod agent_loop;
pub mod agent_state;
pub mod checkpoint;
pub mod context_retrieval;
pub mod context_store;
pub mod context_trace;
pub mod custom_commands;
pub mod editor_tool;
pub mod genome;
pub mod goal_tools;
pub mod hooks;
pub mod instruction_state;
pub mod isolate;
pub mod lsp_tool;
pub mod mcp_tool;
pub mod media_tools;
pub mod memory;
pub mod memory_index;
mod memory_merge;
mod memory_summarizer;
pub mod mentions;
pub mod model_provider;
pub mod orchestrator;
pub mod orchestrator_runner;
pub mod outcome;
pub mod plugins;
pub mod process_tools;
pub mod project_instructions;
pub mod prompt;
pub mod runtime_assembly;
pub mod runtime_profile;
pub mod rust_analyzer;
pub mod search;
pub mod session;
pub mod session_index;
pub mod session_query_tools;
pub mod skills;
pub mod slash;
pub mod terminal_tools;
pub mod tool_middleware;
pub mod tool_recovery;
pub mod tool_scheduler;
pub mod tools;
pub mod turn_context;
pub mod user_question;
pub mod verification;
mod workspace_promotion;
pub mod workspace_tools;

#[cfg(test)]
pub(crate) mod test_support;

pub use agent_loop::{
    suggest_title_with_provider, AgentLoop, EventSink, LoopEvent, Provider, TaskBudget, TurnResult,
};
pub use agent_state::{AgentState, AgentStateMachine};
pub use checkpoint::{CheckpointMeta, CheckpointStore, RestoreReport};
pub use context_retrieval::{
    ContextRetriever, Layer as ContextLayer, RetrievalContextProvider, RetrievalResult,
    RetrievedNote,
};
pub use context_store::{ContextEvent, ContextStore, EventStatus, EventType, ToolCallDetail};
pub use context_trace::{
    retrieve as retrieve_sources, RetrievedSource, SourceKind, SourceRecord, SourceTraceStore,
};
pub use custom_commands::{
    custom_command_prompt, expand_custom_command_template, list_custom_commands,
    parse_custom_command_query, resolve_custom_command, CustomCommandQuery, CustomCommandSummary,
};
pub use editor_tool::StrReplaceEditorTool;
pub use genome::Genome;
pub use goal_tools::{GoalAuthoritySource, GoalToolService, GoalTurnAuthority};
pub use hooks::{HookEvent, HookOutcome};
pub use instruction_state::{
    Instruction, InstructionLedger, InstructionStatus, Origin as InstructionOrigin, Polarity,
};
pub use lsp_tool::{LspProvider, LspRequest, LspTool};
pub use mcp_tool::{prepare_mcp_server_tools, register_mcp_server};
pub use media_tools::{GenerateImageTool, GenerateVideoTool, MediaGenerationService, MediaPrice};
pub use memory::{MemoryEntry, MemoryRecall, MemoryStore, Summarizer};
pub use memory_index::{Evidence, IndexedNote, MemoryIndex, NoteStatus, PromotionRefused};
pub use memory_merge::MemoryMergeDraft;
pub use memory_merge::{MemoryReviewItem, MemoryReviewKind};
pub use memory_summarizer::ProviderMemorySummarizer;
pub use mentions::{expand_file_mentions, find_mentions};
pub use ncx_context::{
    ContextAssembler, ContextEntry, ContextFragment, ContextService, TextContextFragment,
};
pub use ncx_provider::model_catalog::resolve_context_capacity;
pub use ncx_provider::preset_catalog;
pub use ncx_provider::{parse_catalog_models, DiscoveredProviderModel};
pub use orchestrator::{
    AgentCallResult, AgentRunner, Complexity, Orchestrator, OrchestratorConfig,
    OrchestratorControl, OrchestratorEvent, OrchestratorOutcome, OrchestratorStage,
    OrchestratorTelemetry, Tier,
};
pub use orchestrator_runner::{HarnessAgentRunner, HarnessRunnerEvent};
pub use outcome::{
    append_checkpoint_restore, append_outcome_followup, append_task_outcome, build_outcome_report,
    followup_polarity, followups_for_task, format_outcome_report, latest_primary_outcome,
    load_outcomes_and_followups, now_millis, parse_since_duration_ms, primary_outcomes_for_session,
    record_checkpoint_rollback, record_followup, wilson_ci, workspace_events_db_path,
    BaselineReadiness, CategorySingularity, FollowupSignal, MemoryFeedback, OutcomeFollowup,
    OutcomeReport, OutcomeSliceReport, RouteKey, RoutingStats, ShadowSummary, SingularityStatus,
    TaskCategory, TaskOutcome, TaskRole, WilsonCi,
};
pub use plugins::{
    discover_codex_apps, discover_codex_hooks, discover_codex_mcp_servers, discover_marketplaces,
    resolve_local_marketplace_plugin, AttachmentPlugin, AttachmentServiceDescriptor,
    BuiltinToolsPlugin, CodexAppResource, CodexPluginCatalog, CodexPluginManifest,
    CodexPluginRecord, CompactionPlugin, CompactionServiceDescriptor, ContextPlugin,
    ContextServiceDescriptor, CoreToolsPlugin, CostTelemetryPlugin, CostTelemetryService,
    CostTelemetryServiceDescriptor, CostTelemetrySnapshot, ExternalHostPlugin,
    ExternalPluginCatalog, ExternalPluginHandshake, ExternalPluginManifest, ExternalPluginRecord,
    ExternalPluginRegistration, ExternalProtocolRequest, ExternalProtocolResponse,
    ExternalToolDescriptor, HarnessDiagnostics, HarnessPlugin, HarnessRuntimeBuilder,
    InteractionPlugin, InteractionService, LlmProviderFactory, LlmProviderFactoryHandle,
    LlmProviderPlugin, LlmServiceDescriptor, Marketplace, MarketplacePlugin, MarketplaceSource,
    McpPlugin, McpServiceDescriptor, MediaPlugin, MediaServiceDescriptor, MemoryPlugin,
    MemoryServiceDescriptor, PluginCapability, PluginHost, PluginInstallReport, PluginManifest,
    PluginRegistry, PolicyPlugin, PolicyService, ProcessToolsPlugin, ProviderCatalogPlugin,
    ProviderCatalogService, ProviderChatProbePlugin, ProviderChatProbeService,
    ProviderDirectoryDiagnostics, ProviderDirectoryPlugin, ProviderDirectoryService,
    SearchToolsPlugin, SessionToolsPlugin, WorkspaceToolsPlugin,
};
pub use project_instructions::{load_project_instructions, load_workspace_instructions};
pub use prompt::PromptAssembler;
pub use runtime_assembly::{ConfiguredHarnessRuntime, RuntimeContextSources, RuntimeHostBindings};
pub use runtime_profile::{
    install_llm_provider_factory, model_provider_from_config, model_supports_native_vision,
    vision_provider_from_config, AgentRuntimeProfile, ConfiguredLlmProviderFactory,
    RuntimePermissionProfile,
};
pub use rust_analyzer::RustAnalyzerProvider;
pub use session::{
    estimate_tokens, CompactionTrace, ContextEditPolicy, ContextEditStats, MessageTrace, Session,
    COMPACTED_HISTORY_PREFIX,
};
pub use session_index::{new_session_id, SessionIndex, SessionSummary};
pub use session_query_tools::session_query_tools;
pub use skills::{discover_skills, skills_index_block, Skill, SkillCapability};
pub use slash::{parse_slash, split_loop_arg, SLASH_HELP};
pub use tool_middleware::{ToolMiddleware, ToolMiddlewareDecision};
pub use tool_recovery::{ToolCapability, ToolFailureClass};
pub use tool_scheduler::{BoundedToolScheduler, ToolScheduler};
pub use tools::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, SessionGrants, Tool, ToolContext,
    ToolDuplicatePolicy, ToolRegistry, ToolRetryPolicy, ToolRuntimePolicy,
};
pub use turn_context::{TurnContextProvider, TurnContextRegistry, TurnContextRequest};
pub use user_question::{UserQuestionHandler, UserQuestionRequest};
