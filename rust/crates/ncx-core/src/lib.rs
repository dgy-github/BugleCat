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
pub mod hooks;
pub mod isolate;
pub mod mcp_tool;
pub mod memory;
pub mod mentions;
pub mod orchestrator;
pub mod project_instructions;
pub mod search;
pub mod session;
pub mod session_index;
pub mod skills;
pub mod slash;
pub mod tools;

pub use agent_loop::{AgentLoop, EventSink, LoopEvent, Provider, TaskBudget, TurnResult};
pub use checkpoint::{CheckpointMeta, CheckpointStore, RestoreReport};
pub use hooks::{HookEvent, HookOutcome};
pub use memory::{MemoryEntry, MemoryStore, Summarizer};
pub use mentions::{expand_file_mentions, find_mentions};
pub use orchestrator::{
    AgentRunner, Complexity, Orchestrator, OrchestratorConfig, OrchestratorOutcome, Tier,
};
pub use project_instructions::{load_project_instructions, load_workspace_instructions};
pub use session::{ContextEditPolicy, ContextEditStats, Session};
pub use session_index::{new_session_id, SessionIndex, SessionSummary};
pub use skills::{discover_skills, skills_index_block, Skill};
pub use slash::{parse_slash, split_loop_arg, SLASH_HELP};
pub use mcp_tool::register_mcp_server;
pub use tools::{
    ApprovalDecision, ApprovalHandler, ApprovalRequest, SessionGrants, Tool, ToolContext,
    ToolRegistry,
};
