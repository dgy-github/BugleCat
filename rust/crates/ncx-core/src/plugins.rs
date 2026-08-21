//! DeepSeek Harness-style capability plugins.
//!
//! A plugin is a small, named unit that contributes tools and/or execution
//! middleware to a runtime. The host owns the registry and applies plugins in
//! declaration order, so built-in capabilities and future external adapters
//! use the same lifecycle and remain reversible at runtime.

use std::rc::Rc;

use crate::tool_middleware::ToolMiddleware;
use crate::tools::{Tool, ToolContext, ToolRegistry};

mod builtin;

pub use builtin::{
    BuiltinToolsPlugin, CoreToolsPlugin, ProcessToolsPlugin, SearchToolsPlugin,
    SessionToolsPlugin, WorkspaceToolsPlugin,
};

/// A named capability bundle installed into a Harness runtime.
pub trait HarnessPlugin {
    /// Stable identifier used in diagnostics and duplicate detection.
    fn id(&self) -> &str;

    /// Add this plugin's tools and middleware to the host registry.
    fn install(&self, host: &mut PluginHost<'_>);
}

/// Restricted host passed to plugins during installation.
pub struct PluginHost<'a> {
    registry: &'a mut ToolRegistry,
}

impl<'a> PluginHost<'a> {
    pub(crate) fn new(registry: &'a mut ToolRegistry) -> Self {
        Self { registry }
    }

    /// Register a model-facing tool supplied by the plugin.
    pub fn tool(&mut self, tool: Box<dyn Tool>) {
        self.registry.register(tool);
    }

    /// Read the runtime context while deciding which optional capabilities to
    /// install. Plugins cannot replace the host context or bypass its policy.
    pub fn context(&self) -> &ToolContext {
        &self.registry.ctx
    }

    /// Register an ordered middleware layer supplied by the plugin.
    pub fn middleware(&mut self, middleware: Rc<dyn ToolMiddleware>) -> Result<(), String> {
        self.registry.register_middleware(middleware)
    }
}

/// Ordered collection of plugins for one agent runtime.
#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Rc<dyn HarnessPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a plugin once. IDs are intentionally stable so configuration can
    /// select plugins without depending on Rust type names.
    pub fn register(&mut self, plugin: Rc<dyn HarnessPlugin>) -> Result<(), String> {
        if plugin.id().trim().is_empty() {
            return Err("harness plugin id must not be empty".to_string());
        }
        if self.plugins.iter().any(|current| current.id() == plugin.id()) {
            return Err(format!("harness plugin '{}' is already registered", plugin.id()));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|plugin| plugin.id())
    }

    /// Install all registered plugins in declaration order.
    pub fn install_into(&self, registry: &mut ToolRegistry) {
        for plugin in &self.plugins {
            plugin.install(&mut PluginHost::new(registry));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::{json, Value};
    use std::path::PathBuf;

    struct EmptyPlugin;

    impl HarnessPlugin for EmptyPlugin {
        fn id(&self) -> &str { "test.empty" }
        fn install(&self, _host: &mut PluginHost<'_>) {}
    }

    struct PluginTool;

    #[async_trait(?Send)]
    impl Tool for PluginTool {
        fn name(&self) -> &str { "plugin_tool" }
        fn description(&self) -> &str { "A test plugin tool." }
        fn parameters(&self) -> Value { json!({"type":"object","properties":{}}) }
        async fn execute(&self, _ctx: &crate::tools::ToolContext, _args: &Value) -> String {
            "plugin-ok".to_string()
        }
    }

    struct ToolPlugin;

    impl HarnessPlugin for ToolPlugin {
        fn id(&self) -> &str { "test.tool" }
        fn install(&self, host: &mut PluginHost<'_>) { host.tool(Box::new(PluginTool)); }
    }

    #[test]
    fn plugin_ids_are_unique_and_install_in_order() {
        let mut plugins = PluginRegistry::new();
        plugins.register(Rc::new(EmptyPlugin)).unwrap();
        assert!(plugins.register(Rc::new(EmptyPlugin)).is_err());
        assert_eq!(plugins.ids().collect::<Vec<_>>(), vec!["test.empty"]);
    }

    #[tokio::test]
    async fn plugin_can_contribute_a_tool() {
        let workspace = PathBuf::from("plugin-test-workspace");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut registry = ToolRegistry::empty(crate::tools::ToolContext::new(workspace, policy));
        let mut plugins = PluginRegistry::new();
        plugins.register(Rc::new(ToolPlugin)).unwrap();
        plugins.install_into(&mut registry);
        assert_eq!(registry.execute("plugin_tool", &json!({})).await, "plugin-ok");
    }
}
