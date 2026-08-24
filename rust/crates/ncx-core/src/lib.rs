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
pub mod checkpoint;
pub mod custom_commands;
pub mod editor_tool;
pub mod genome;
pub mod hooks;
pub mod isolate;
pub mod lsp_tool;
pub mod mcp_tool;
pub mod memory;
pub mod mentions;
pub mod model_provider;
pub mod orchestrator;
pub mod plugins;
pub mod process_tools;
pub mod project_instructions;
pub mod prompt;
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
pub mod workspace_tools;

pub use agent_loop::{
    suggest_title_with_provider, AgentLoop, EventSink, LoopEvent, Provider, TaskBudget, TurnResult,
};
pub use checkpoint::{CheckpointMeta, CheckpointStore, RestoreReport};
pub use custom_commands::{
    custom_command_prompt, expand_custom_command_template, list_custom_commands,
    parse_custom_command_query, resolve_custom_command, CustomCommandQuery, CustomCommandSummary,
};
pub use ncx_context::{ContextFragment, TextContextFragment};
pub use editor_tool::StrReplaceEditorTool;
pub use genome::Genome;
pub use hooks::{HookEvent, HookOutcome};
pub use lsp_tool::{LspProvider, LspRequest, LspTool};
pub use mcp_tool::{prepare_mcp_server_tools, register_mcp_server};
pub use memory::{MemoryEntry, MemoryStore, Summarizer};
pub use mentions::{expand_file_mentions, find_mentions};
pub use orchestrator::{
    AgentRunner, Complexity, Orchestrator, OrchestratorConfig, OrchestratorOutcome, Tier,
};
pub use plugins::{
    discover_codex_apps, discover_codex_hooks, discover_codex_mcp_servers, discover_marketplaces,
    resolve_local_marketplace_plugin, AttachmentPlugin, AttachmentServiceDescriptor,
    BuiltinToolsPlugin, CodexAppResource, CodexPluginCatalog, CodexPluginManifest, CodexPluginRecord,
    CompactionPlugin, CompactionServiceDescriptor, ContextPlugin, ContextServiceDescriptor,
    CoreToolsPlugin, CostTelemetryPlugin, CostTelemetryService, CostTelemetryServiceDescriptor,
    CostTelemetrySnapshot, ExternalPluginCatalog, ExternalPluginManifest, ExternalPluginRecord,
    HarnessDiagnostics, HarnessPlugin, HarnessRuntimeBuilder, InteractionPlugin,
    InteractionService, LlmProviderFactory, LlmProviderFactoryHandle, LlmProviderPlugin,
    LlmServiceDescriptor, Marketplace, MarketplacePlugin, MarketplaceSource, McpPlugin,
    McpServiceDescriptor, MediaPlugin, MediaServiceDescriptor, MemoryPlugin,
    MemoryServiceDescriptor, PluginCapability, PluginHost, PluginInstallReport, PluginManifest,
    PluginRegistry, PolicyPlugin, PolicyService, ProcessToolsPlugin, SearchToolsPlugin,
    SessionToolsPlugin, WorkspaceToolsPlugin,
};
pub use project_instructions::{load_project_instructions, load_workspace_instructions};
pub use prompt::PromptAssembler;
pub use runtime_profile::{
    install_llm_provider_factory, model_provider_from_config, vision_provider_from_config,
    AgentRuntimeProfile, ConfiguredLlmProviderFactory, RuntimePermissionProfile,
};
pub use rust_analyzer::RustAnalyzerProvider;
pub use session::{
    estimate_tokens, ContextEditPolicy, ContextEditStats, Session, COMPACTED_HISTORY_PREFIX,
};
pub use session_index::{new_session_id, SessionIndex, SessionSummary};
pub use session_query_tools::session_query_tools;
pub use skills::{discover_skills, skills_index_block, Skill};
pub use slash::{parse_slash, split_loop_arg, SLASH_HELP};
pub use tool_middleware::{ToolMiddleware, ToolMiddlewareDecision};
pub use tool_recovery::{ToolCapability, ToolFailureClass};
pub use tool_scheduler::{BoundedToolScheduler, ToolScheduler};
pub use tools::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, SessionGrants, Tool, ToolContext,
    ToolRegistry,
};
pub use turn_context::{TurnContextProvider, TurnContextRegistry, TurnContextRequest};
pub use user_question::{UserQuestionHandler, UserQuestionRequest};
