//! Runtime composition independent from tool execution.

use std::rc::Rc;

use super::{
    CoreToolsPlugin, HarnessPlugin, HarnessProfile, PluginInstallReport, PluginRegistry,
    ProcessToolsPlugin, SearchToolsPlugin, SessionToolsPlugin, WorkspaceToolsPlugin,
};
use crate::tools::{ToolContext, ToolRegistry};

pub struct HarnessRuntimeBuilder {
    plugins: PluginRegistry,
}

impl Default for HarnessRuntimeBuilder {
    fn default() -> Self {
        Self::for_profile(HarnessProfile::Full)
    }
}

impl HarnessRuntimeBuilder {
    pub fn empty() -> Self {
        Self {
            plugins: PluginRegistry::new(),
        }
    }

    pub fn for_profile(profile: HarnessProfile) -> Self {
        let mut builder = Self::empty();
        for plugin in default_plugins() {
            if profile.enables(plugin.manifest().capability) {
                builder.register(plugin).expect("unique built-in plugin");
            }
        }
        builder
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

fn default_plugins() -> Vec<Rc<dyn HarnessPlugin>> {
    vec![
        Rc::new(CoreToolsPlugin),
        Rc::new(SearchToolsPlugin),
        Rc::new(WorkspaceToolsPlugin),
        Rc::new(ProcessToolsPlugin),
        Rc::new(SessionToolsPlugin),
    ]
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

    #[test]
    fn profiles_select_components_without_changing_default_order() {
        let coding = HarnessRuntimeBuilder::for_profile(HarnessProfile::Coding);
        assert_eq!(
            coding.plugin_ids().collect::<Vec<_>>(),
            vec!["ncx.core", "ncx.search", "ncx.workspace", "ncx.session"]
        );
        let minimal = HarnessRuntimeBuilder::for_profile(HarnessProfile::Minimal);
        assert_eq!(
            minimal.plugin_ids().collect::<Vec<_>>(),
            vec!["ncx.core", "ncx.workspace"]
        );
    }
}
