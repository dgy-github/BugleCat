//! Built-in capability plugins composed by the default nanocodex runtime.

use crate::plugins::{
    context_descriptor, AttachmentServiceDescriptor, CompactionServiceDescriptor,
    CostTelemetryService, CostTelemetryServiceDescriptor, HarnessPlugin, InteractionService,
    LlmServiceDescriptor, McpServiceDescriptor, MediaServiceDescriptor, MemoryServiceDescriptor,
    PluginCapability, PluginHost, PluginManifest, PolicyService,
};
use crate::tools::{
    ApplyPatchTool, RememberTool, ShellTool, SkillTool, ToolSearchTool, UpdatePlanTool,
};
use crate::StrReplaceEditorTool;
use std::rc::Rc;

/// File editing, shell execution, planning and dynamic tool discovery.
pub struct CoreToolsPlugin;

impl HarnessPlugin for CoreToolsPlugin {
    fn id(&self) -> &str {
        "ncx.core"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.core", "Core Tools", PluginCapability::Core)
    }
    fn inject(&self) -> &[&str] {
        &[]
    }

    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        let _policy = host.service::<PolicyService>("policy");
        let _context = host.service::<crate::plugins::ContextServiceDescriptor>("context");
        host.tool(Box::new(crate::tools::ReadFileTool));
        host.tool(Box::new(ApplyPatchTool));
        host.tool(Box::new(StrReplaceEditorTool));
        host.tool(Box::new(UpdatePlanTool));
        host.tool(Box::new(ShellTool));
        host.tool(Box::new(ToolSearchTool));
        Ok(())
    }
}

/// Local and web search capabilities.
pub struct SearchToolsPlugin;

impl HarnessPlugin for SearchToolsPlugin {
    fn id(&self) -> &str {
        "ncx.search"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.search", "Search Tools", PluginCapability::Search)
    }

    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.tool(Box::new(crate::search::GrepTool));
        host.tool(Box::new(crate::search::GrepLiteralTool));
        host.tool(Box::new(crate::search::GlobTool));
        host.tool(Box::new(crate::search::FindFilesTool));
        host.tool(Box::new(crate::search::WebSearchTool));
        host.tool(Box::new(crate::search::WebFetchTool));
        Ok(())
    }
}

/// Workspace inspection and source-control capabilities.
pub struct WorkspaceToolsPlugin;

impl HarnessPlugin for WorkspaceToolsPlugin {
    fn id(&self) -> &str {
        "ncx.workspace"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.workspace",
            "Workspace Tools",
            PluginCapability::Workspace,
        )
    }

    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.tool(Box::new(crate::workspace_tools::ListDirectoryTool));
        host.tool(Box::new(crate::workspace_tools::PathInfoTool));
        host.tool(Box::new(crate::workspace_tools::GitStatusTool));
        host.tool(Box::new(crate::workspace_tools::GitDiffTool));
        Ok(())
    }
}

/// LSP, managed background processes and interactive terminal capabilities.
pub struct ProcessToolsPlugin;

impl HarnessPlugin for ProcessToolsPlugin {
    fn id(&self) -> &str {
        "ncx.process"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.process", "Process Tools", PluginCapability::Process)
    }

    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
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
        Ok(())
    }
}

/// Session lookup plus optional runtime-bound capabilities.
pub struct SessionToolsPlugin;

impl HarnessPlugin for SessionToolsPlugin {
    fn id(&self) -> &str {
        "ncx.session"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.session", "Session Tools", PluginCapability::Session)
    }

    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
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
        Ok(())
    }
}

pub struct MemoryPlugin;
impl HarnessPlugin for MemoryPlugin {
    fn id(&self) -> &str {
        "ncx.memory"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.memory", "Memory", PluginCapability::Memory)
    }
    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.provide(
            "memory",
            Rc::new(MemoryServiceDescriptor {
                enabled: host.context().memory.is_some(),
                store: host.context().memory.clone(),
            }),
        )
    }
}

pub struct CompactionPlugin;
impl HarnessPlugin for CompactionPlugin {
    fn id(&self) -> &str {
        "ncx.compaction"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.compaction", "Compaction", PluginCapability::Compaction)
    }
    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.provide(
            "compaction",
            Rc::new(CompactionServiceDescriptor { enabled: true }),
        )
    }
}

pub struct McpPlugin;
impl HarnessPlugin for McpPlugin {
    fn id(&self) -> &str {
        "ncx.mcp"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.mcp", "MCP", PluginCapability::Mcp)
    }
    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        host.provide(
            "mcp",
            Rc::new(McpServiceDescriptor {
                enabled: config
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                configured_servers: config
                    .get("configured_servers")
                    .and_then(toml::Value::as_integer)
                    .unwrap_or(0)
                    .max(0) as usize,
                active_tools: 0,
            }),
        )
    }
}

pub struct AttachmentPlugin;
impl HarnessPlugin for AttachmentPlugin {
    fn id(&self) -> &str {
        "ncx.attachment"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.attachment",
            "Attachments",
            PluginCapability::Attachment,
        )
    }
    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        let max_bytes = config
            .get("max_bytes")
            .and_then(toml::Value::as_integer)
            .unwrap_or(20 * 1024 * 1024)
            .max(1) as u64;
        host.provide(
            "attachment",
            Rc::new(AttachmentServiceDescriptor {
                max_bytes,
                extensions: [
                    "png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf", "txt", "md", "docx", "xlsx",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            }),
        )
    }
}

pub struct MediaPlugin;
impl HarnessPlugin for MediaPlugin {
    fn id(&self) -> &str {
        "ncx.media"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.media", "Media", PluginCapability::Media)
    }
    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        host.provide(
            "media",
            Rc::new(MediaServiceDescriptor {
                vision: config
                    .get("vision")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
                image_generation: config
                    .get("image_generation")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
                video_generation: config
                    .get("video_generation")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
            }),
        )
    }
}

/// Enables discovery of process-isolated protocol plugins for this profile.
pub struct ExternalHostPlugin;
impl HarnessPlugin for ExternalHostPlugin {
    fn id(&self) -> &str {
        "ncx.external"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.external",
            "External Plugin Host",
            PluginCapability::External,
        )
    }
    fn install(&self, _host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        Ok(())
    }
}

pub struct CostTelemetryPlugin;
impl HarnessPlugin for CostTelemetryPlugin {
    fn id(&self) -> &str {
        "ncx.cost-telemetry"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.cost-telemetry",
            "Cost & Telemetry",
            PluginCapability::CostTelemetry,
        )
    }
    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        host.provide(
            "cost.telemetry",
            Rc::new(CostTelemetryService::new(CostTelemetryServiceDescriptor {
                currency: config
                    .get("currency")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("CNY")
                    .to_string(),
                input_per_million: config
                    .get("input_per_million")
                    .and_then(toml::Value::as_float)
                    .unwrap_or(0.0),
                output_per_million: config
                    .get("output_per_million")
                    .and_then(toml::Value::as_float)
                    .unwrap_or(0.0),
                telemetry_enabled: config
                    .get("telemetry_enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
            })),
        )
    }
}

pub struct LlmProviderPlugin;
impl HarnessPlugin for LlmProviderPlugin {
    fn id(&self) -> &str {
        "ncx.llm"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.llm", "LLM Provider", PluginCapability::Llm)
    }
    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        let model = config
            .get("model")
            .and_then(toml::Value::as_str)
            .unwrap_or("configured");
        host.provide(
            "llm.provider",
            Rc::new(LlmServiceDescriptor {
                model: model.to_string(),
                supports_reasoning: true,
                supports_vision: true,
            }),
        )
    }
}
pub struct InteractionPlugin;
impl HarnessPlugin for InteractionPlugin {
    fn id(&self) -> &str {
        "ncx.interaction"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.interaction",
            "Interaction",
            PluginCapability::Interaction,
        )
    }
    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.provide(
            "interaction",
            Rc::new(InteractionService {
                approver: host.context().approver.clone(),
            }),
        )
    }
}
pub struct PolicyPlugin;
impl HarnessPlugin for PolicyPlugin {
    fn id(&self) -> &str {
        "ncx.policy"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.policy", "Policy", PluginCapability::Policy)
    }
    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        let ctx = host.context();
        host.provide(
            "policy",
            Rc::new(PolicyService {
                sandbox: ctx.policy.clone(),
                approval_policy: ctx.approval_policy.clone(),
                plan_mode: ctx.plan_mode,
            }),
        )
    }
}
pub struct ContextPlugin;
impl HarnessPlugin for ContextPlugin {
    fn id(&self) -> &str {
        "ncx.context"
    }
    fn manifest(&self) -> PluginManifest {
        PluginManifest::new("ncx.context", "Context", PluginCapability::Context)
    }
    fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
        host.provide("context", Rc::new(context_descriptor(host.context())))
    }
}
/// Compatibility bundle used by the default runtime.
pub struct BuiltinToolsPlugin;

impl HarnessPlugin for BuiltinToolsPlugin {
    fn id(&self) -> &str {
        "ncx.builtin-tools"
    }

    fn manifest(&self) -> PluginManifest {
        PluginManifest::new(
            "ncx.builtin-tools",
            "Built-in Tools",
            PluginCapability::Core,
        )
    }

    fn install(&self, host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
        CoreToolsPlugin.install(host, config)?;
        SearchToolsPlugin.install(host, config)?;
        WorkspaceToolsPlugin.install(host, config)?;
        ProcessToolsPlugin.install(host, config)?;
        SessionToolsPlugin.install(host, config)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolContext, ToolRegistry};
    use async_trait::async_trait;
    use ncx_sandbox::{PolicyService, SandboxPolicy, READ_ONLY, WORKSPACE_WRITE};
    use serde_json::{json, Value};
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

    struct PolicyProbe;

    #[async_trait(?Send)]
    impl Tool for PolicyProbe {
        fn name(&self) -> &str {
            "policy_probe"
        }
        fn description(&self) -> &str {
            "returns the effective policy"
        }
        fn parameters(&self) -> Value {
            json!({"type":"object"})
        }
        async fn execute(&self, ctx: &ToolContext, _args: &Value) -> String {
            ctx.policy.mode.clone()
        }
    }

    #[tokio::test]
    async fn tool_execution_consumes_replaceable_policy_service() {
        let mut registry = empty_registry();
        registry.register(Box::new(PolicyProbe));
        registry.replace_service(
            "policy",
            Rc::new(PolicyService {
                sandbox: SandboxPolicy::new(READ_ONLY, "builtin-plugin-test"),
                approval_policy: "never".into(),
                plan_mode: true,
            }),
        );
        assert_eq!(
            registry.execute("policy_probe", &json!({})).await,
            READ_ONLY
        );
    }
}
