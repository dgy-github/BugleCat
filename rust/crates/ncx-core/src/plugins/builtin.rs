//! Built-in capability plugins composed by the default nanocodex runtime.

use crate::plugins::{HarnessPlugin, PluginHost};
use crate::tools::{
    ApplyPatchTool, RememberTool, ShellTool, SkillTool, ToolSearchTool, UpdatePlanTool,
};
use crate::StrReplaceEditorTool;

/// File editing, shell execution, planning and dynamic tool discovery.
pub struct CoreToolsPlugin;

impl HarnessPlugin for CoreToolsPlugin {
    fn id(&self) -> &str {
        "ncx.core"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        host.tool(Box::new(crate::tools::ReadFileTool));
        host.tool(Box::new(ApplyPatchTool));
        host.tool(Box::new(StrReplaceEditorTool));
        host.tool(Box::new(UpdatePlanTool));
        host.tool(Box::new(ShellTool));
        host.tool(Box::new(ToolSearchTool));
    }
}

/// Local and web search capabilities.
pub struct SearchToolsPlugin;

impl HarnessPlugin for SearchToolsPlugin {
    fn id(&self) -> &str {
        "ncx.search"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        host.tool(Box::new(crate::search::GrepTool));
        host.tool(Box::new(crate::search::GrepLiteralTool));
        host.tool(Box::new(crate::search::GlobTool));
        host.tool(Box::new(crate::search::FindFilesTool));
        host.tool(Box::new(crate::search::WebSearchTool));
        host.tool(Box::new(crate::search::WebFetchTool));
    }
}

/// Workspace inspection and source-control capabilities.
pub struct WorkspaceToolsPlugin;

impl HarnessPlugin for WorkspaceToolsPlugin {
    fn id(&self) -> &str {
        "ncx.workspace"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        host.tool(Box::new(crate::workspace_tools::ListDirectoryTool));
        host.tool(Box::new(crate::workspace_tools::PathInfoTool));
        host.tool(Box::new(crate::workspace_tools::GitStatusTool));
        host.tool(Box::new(crate::workspace_tools::GitDiffTool));
    }
}

/// LSP, managed background processes and interactive terminal capabilities.
pub struct ProcessToolsPlugin;

impl HarnessPlugin for ProcessToolsPlugin {
    fn id(&self) -> &str {
        "ncx.process"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        host.tool(Box::new(crate::lsp_tool::LspTool));
        host.tool(Box::new(crate::process_tools::BackgroundStartTool));
        host.tool(Box::new(crate::process_tools::BackgroundPollTool));
        host.tool(Box::new(crate::process_tools::BackgroundStopTool));
        host.tool(Box::new(crate::process_tools::BackgroundListTool));
        host.tool(Box::new(crate::terminal_tools::TerminalOpenTool));
        host.tool(Box::new(crate::terminal_tools::TerminalWriteTool));
        host.tool(Box::new(crate::terminal_tools::TerminalReadTool));
        host.tool(Box::new(crate::terminal_tools::TerminalExecTool));
        host.tool(Box::new(crate::terminal_tools::TerminalResizeTool));
        host.tool(Box::new(crate::terminal_tools::TerminalCloseTool));
        host.tool(Box::new(crate::terminal_tools::TerminalListTool));
    }
}

/// Session lookup plus optional runtime-bound capabilities.
pub struct SessionToolsPlugin;

impl HarnessPlugin for SessionToolsPlugin {
    fn id(&self) -> &str {
        "ncx.session"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        for tool in crate::session_query_tools::session_query_tools() {
            host.tool(tool);
        }
        let question_handler = host.context().user_question_handler.clone();
        let has_memory = host.context().memory.is_some();
        let has_skills = !host.context().skills.is_empty();
        if let Some(handler) = question_handler {
            host.tool(Box::new(crate::user_question::AskUserQuestionTool::new(
                handler,
            )));
        }
        if has_memory {
            host.tool(Box::new(RememberTool));
        }
        if has_skills {
            host.tool(Box::new(SkillTool));
        }
    }
}

/// Compatibility bundle used by the default runtime.
pub struct BuiltinToolsPlugin;

impl HarnessPlugin for BuiltinToolsPlugin {
    fn id(&self) -> &str {
        "ncx.builtin-tools"
    }

    fn install(&self, host: &mut PluginHost<'_>) {
        CoreToolsPlugin.install(host);
        SearchToolsPlugin.install(host);
        WorkspaceToolsPlugin.install(host);
        ProcessToolsPlugin.install(host);
        SessionToolsPlugin.install(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolContext, ToolRegistry};
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use std::path::PathBuf;

    fn empty_registry() -> ToolRegistry {
        let workspace = PathBuf::from("builtin-plugin-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        ToolRegistry::empty(ToolContext::new(workspace, policy))
    }

    fn names(registry: &ToolRegistry) -> Vec<String> {
        registry
            .schemas()
            .into_iter()
            .filter_map(|schema| schema["function"]["name"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn capability_plugins_install_independent_tool_sets() {
        let mut core = empty_registry();
        core.install_plugin(&CoreToolsPlugin);
        let core_names = names(&core);
        assert!(core_names.contains(&"apply_patch".to_string()));
        assert!(core_names.contains(&"tool_search".to_string()));
        assert!(!core_names.contains(&"grep".to_string()));

        let mut search = empty_registry();
        search.install_plugin(&SearchToolsPlugin);
        let search_names = names(&search);
        assert!(search_names.contains(&"grep".to_string()));
        assert!(search_names.contains(&"web_search".to_string()));
        assert!(!search_names.contains(&"apply_patch".to_string()));
    }

    #[test]
    fn compatibility_bundle_matches_default_registry() {
        let explicit_context = empty_registry().ctx;
        let default_context = explicit_context.clone();
        let mut explicit = ToolRegistry::empty(explicit_context);
        explicit.install_plugin(&BuiltinToolsPlugin);
        let default = ToolRegistry::new(default_context);
        assert_eq!(names(&explicit), names(&default));
    }
}
