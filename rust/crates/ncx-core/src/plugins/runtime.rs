//! Runtime composition independent from tool execution.

use std::rc::Rc;

use super::{
    CoreToolsPlugin, HarnessPlugin, PluginInstallReport, PluginRegistry, ProcessToolsPlugin,
    SearchToolsPlugin, SessionToolsPlugin, WorkspaceToolsPlugin,
};
use crate::tools::{ToolContext, ToolRegistry};

pub struct HarnessRuntimeBuilder {
    plugins: PluginRegistry,
}

impl Default for HarnessRuntimeBuilder {
    fn default() -> Self {
        let mut builder = Self::empty();
        builder
            .register(Rc::new(CoreToolsPlugin))
            .expect("unique built-in plugin");
        builder
            .register(Rc::new(SearchToolsPlugin))
            .expect("unique built-in plugin");
        builder
            .register(Rc::new(WorkspaceToolsPlugin))
            .expect("unique built-in plugin");
        builder
            .register(Rc::new(ProcessToolsPlugin))
            .expect("unique built-in plugin");
        builder
            .register(Rc::new(SessionToolsPlugin))
            .expect("unique built-in plugin");
        builder
    }
}

impl HarnessRuntimeBuilder {
    pub fn empty() -> Self {
        Self {
            plugins: PluginRegistry::new(),
        }
    }

    pub fn register(&mut self, plugin: Rc<dyn HarnessPlugin>) -> Result<&mut Self, String> {
        self.plugins.register(plugin)?;
        Ok(self)
    }

    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.ids()
    }

    pub fn build(self, context: ToolContext) -> ToolRegistry {
        self.build_with_report(context).0
    }

    pub fn build_with_report(self, context: ToolContext) -> (ToolRegistry, PluginInstallReport) {
        let mut tools = ToolRegistry::empty(context);
        let report = self.plugins.install_into(&mut tools);
        (tools, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use std::path::PathBuf;

    fn context() -> ToolContext {
        let workspace = PathBuf::from("runtime-plugin-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        ToolContext::new(workspace, policy)
    }

    #[test]
    fn default_runtime_reports_architectural_plugin_order() {
        let builder = HarnessRuntimeBuilder::default();
        assert_eq!(
            builder.plugin_ids().collect::<Vec<_>>(),
            vec![
                "ncx.core",
                "ncx.search",
                "ncx.workspace",
                "ncx.process",
                "ncx.session"
            ]
        );
        let (_, report) = builder.build_with_report(context());
        assert_eq!(report.installed.len(), 5);
    }

    #[test]
    fn empty_runtime_has_no_model_facing_tools() {
        let tools = HarnessRuntimeBuilder::empty().build(context());
        assert!(tools.schemas().is_empty());
    }
}
