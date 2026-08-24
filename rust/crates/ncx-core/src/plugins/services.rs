//! Service definitions shared by capability plugins and their consumers.

use crate::tools::{ApprovalHandler, ToolContext};
use crate::Provider;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmServiceDescriptor {
    pub model: String,
    pub supports_reasoning: bool,
    pub supports_vision: bool,
}

/// Runtime-owned provider factory exposed through the LLM capability boundary.
pub trait LlmProviderFactory {
    fn primary(&self) -> Box<dyn Provider>;
    fn vision(&self) -> Option<Box<dyn Provider>>;
}

#[derive(Clone)]
pub struct LlmProviderFactoryHandle(pub Rc<dyn LlmProviderFactory>);

#[derive(Clone)]
pub struct InteractionService {
    pub approver: Option<Rc<dyn ApprovalHandler>>,
}

pub use ncx_sandbox::PolicyService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextServiceDescriptor {
    pub workspace: String,
    pub has_memory: bool,
    pub skill_count: usize,
    pub service: ncx_context::ContextService,
}

impl ContextServiceDescriptor {
    pub fn assemble(&self, base: impl Into<String>) -> String {
        self.service.assemble(base)
    }
}

#[derive(Clone)]
pub struct MemoryServiceDescriptor {
    pub enabled: bool,
    pub store: Option<Rc<crate::memory::MemoryStore>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionServiceDescriptor {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpServiceDescriptor {
    pub enabled: bool,
    pub configured_servers: usize,
    pub active_tools: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AttachmentServiceDescriptor {
    pub max_bytes: u64,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MediaServiceDescriptor {
    pub vision: bool,
    pub image_generation: bool,
    pub video_generation: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CostTelemetryServiceDescriptor {
    pub currency: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub telemetry_enabled: bool,
}

impl CostTelemetryServiceDescriptor {
    pub fn estimate(&self, prompt_tokens: i64, completion_tokens: i64) -> f64 {
        (prompt_tokens.max(0) as f64 / 1_000_000.0) * self.input_per_million
            + (completion_tokens.max(0) as f64 / 1_000_000.0) * self.output_per_million
    }
}

pub struct CostTelemetryService {
    pub config: CostTelemetryServiceDescriptor,
    turns: Cell<u64>,
    prompt_tokens: Cell<i64>,
    completion_tokens: Cell<i64>,
}

impl CostTelemetryService {
    pub fn new(config: CostTelemetryServiceDescriptor) -> Self {
        Self {
            config,
            turns: Cell::new(0),
            prompt_tokens: Cell::new(0),
            completion_tokens: Cell::new(0),
        }
    }
    pub fn record(&self, usage: &std::collections::BTreeMap<String, i64>) {
        if !self.config.telemetry_enabled {
            return;
        }
        self.turns.set(self.turns.get() + 1);
        self.prompt_tokens.set(
            self.prompt_tokens.get() + usage.get("prompt_tokens").copied().unwrap_or(0).max(0),
        );
        self.completion_tokens.set(
            self.completion_tokens.get()
                + usage.get("completion_tokens").copied().unwrap_or(0).max(0),
        );
    }
    pub fn snapshot(&self) -> CostTelemetrySnapshot {
        CostTelemetrySnapshot {
            turns: self.turns.get(),
            prompt_tokens: self.prompt_tokens.get(),
            completion_tokens: self.completion_tokens.get(),
            estimated_cost: self
                .config
                .estimate(self.prompt_tokens.get(), self.completion_tokens.get()),
            currency: self.config.currency.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct CostTelemetrySnapshot {
    pub turns: u64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub estimated_cost: f64,
    pub currency: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct HarnessDiagnostics {
    pub llm: bool,
    pub interaction: bool,
    pub policy: bool,
    pub context: bool,
    pub memory: bool,
    pub compaction: bool,
    pub mcp: bool,
    pub attachment: bool,
    pub media: bool,
    pub cost_telemetry: bool,
}

pub fn context_descriptor(ctx: &ToolContext) -> ContextServiceDescriptor {
    ContextServiceDescriptor {
        workspace: ctx.workspace.display().to_string(),
        has_memory: ctx.memory.is_some(),
        skill_count: ctx.skills.len(),
        service: ncx_context::ContextService::new(ctx.context_entries.as_ref().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn telemetry_accumulates_usage_and_estimates_cost() {
        let service = CostTelemetryService::new(CostTelemetryServiceDescriptor {
            currency: "CNY".into(),
            input_per_million: 2.0,
            output_per_million: 4.0,
            telemetry_enabled: true,
        });
        let usage = [
            ("prompt_tokens".to_string(), 500_000),
            ("completion_tokens".to_string(), 250_000),
        ]
        .into_iter()
        .collect();
        service.record(&usage);
        let snapshot = service.snapshot();
        assert_eq!(snapshot.turns, 1);
        assert_eq!(snapshot.estimated_cost, 2.0);
    }
}
