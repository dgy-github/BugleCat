//! Stable contracts between the runtime and capability plugins.

use std::rc::Rc;

use crate::tool_middleware::ToolMiddleware;
use crate::tools::{Tool, ToolContext, ToolRegistry};

pub trait HarnessPlugin {
    fn id(&self) -> &str;
    fn install(&self, host: &mut PluginHost<'_>);
}

pub struct PluginHost<'a> {
    registry: &'a mut ToolRegistry,
}

impl<'a> PluginHost<'a> {
    pub(crate) fn new(registry: &'a mut ToolRegistry) -> Self {
        Self { registry }
    }

    pub fn tool(&mut self, tool: Box<dyn Tool>) {
        self.registry.register(tool);
    }

    pub fn context(&self) -> &ToolContext {
        &self.registry.ctx
    }

    pub fn middleware(&mut self, middleware: Rc<dyn ToolMiddleware>) -> Result<(), String> {
        self.registry.register_middleware(middleware)
    }
}
