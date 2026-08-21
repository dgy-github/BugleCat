//! Stable contracts between the runtime and capability plugins.

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use super::PluginManifest;
use crate::tool_middleware::ToolMiddleware;
use crate::tools::{Tool, ToolContext, ToolRegistry};

pub trait HarnessPlugin {
    fn id(&self) -> &str;
    fn manifest(&self) -> PluginManifest;
    /// Service names that must exist before this plugin can be activated.
    fn inject(&self) -> &[&str] {
        &[]
    }
    fn install(&self, host: &mut PluginHost<'_>) -> Result<(), String>;
}

type Disposer = Box<dyn FnOnce()>;

#[derive(Default)]
pub(crate) struct PluginRuntimeState {
    services: HashMap<String, Rc<dyn Any>>,
    effects: Vec<Disposer>,
}

impl Drop for PluginRuntimeState {
    fn drop(&mut self) {
        while let Some(dispose) = self.effects.pop() {
            dispose();
        }
    }
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

    /// Publish a shared service for dependent plugins in this runtime scope.
    pub fn provide<T: Any>(&mut self, name: &str, service: Rc<T>) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("harness service name must not be empty".to_string());
        }
        if self.registry.plugin_state.services.contains_key(name) {
            return Err(format!("harness service '{name}' is already provided"));
        }
        self.registry
            .plugin_state
            .services
            .insert(name.to_string(), service);
        Ok(())
    }

    pub fn service<T: Any>(&self, name: &str) -> Option<Rc<T>> {
        self.registry
            .plugin_state
            .services
            .get(name)
            .cloned()
            .and_then(|service| service.downcast::<T>().ok())
    }

    /// Register a reversible side effect. Disposers run in reverse order.
    pub fn effect(&mut self, dispose: impl FnOnce() + 'static) {
        self.registry.plugin_state.effects.push(Box::new(dispose));
    }

    pub(crate) fn has_service(&self, name: &str) -> bool {
        self.registry.plugin_state.services.contains_key(name)
    }
}
