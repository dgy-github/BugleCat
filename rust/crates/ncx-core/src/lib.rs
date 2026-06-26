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
pub mod isolate;
pub mod memory;
pub mod mentions;
pub mod orchestrator;
pub mod search;
pub mod session;
pub mod slash;
pub mod tools;

pub use agent_loop::{AgentLoop, EventSink, LoopEvent, Provider, TaskBudget, TurnResult};
pub use memory::{MemoryEntry, MemoryStore, Summarizer};
pub use mentions::{expand_file_mentions, find_mentions};
pub use orchestrator::{
    AgentRunner, Complexity, Orchestrator, OrchestratorConfig, OrchestratorOutcome, Tier,
};
pub use session::{ContextEditPolicy, ContextEditStats, Session};
pub use slash::{parse_slash, split_loop_arg, SLASH_HELP};
pub use tools::{ApprovalHandler, ApprovalRequest, Tool, ToolContext, ToolRegistry};
