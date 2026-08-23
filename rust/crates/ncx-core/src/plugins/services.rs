//! Service definitions shared by capability plugins and their consumers.

use crate::tools::{ApprovalHandler, ToolContext};
use crate::Provider;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyService {
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub plan_mode: bool,
    pub network_access: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextServiceDescriptor {
    pub workspace: String,
    pub has_memory: bool,
    pub skill_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryServiceDescriptor {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionServiceDescriptor {
    pub enabled: bool,
}

pub fn context_descriptor(ctx: &ToolContext) -> ContextServiceDescriptor {
    ContextServiceDescriptor {
        workspace: ctx.workspace.display().to_string(),
        has_memory: ctx.memory.is_some(),
        skill_count: ctx.skills.len(),
    }
}
