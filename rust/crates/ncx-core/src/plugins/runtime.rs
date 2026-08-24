//! Runtime composition independent from tool execution.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{
    AttachmentPlugin, BundleSpec, CompactionPlugin, ContextPlugin, CoreToolsPlugin,
    CostTelemetryPlugin, HarnessComposition, HarnessPlugin, InteractionPlugin, LlmProviderPlugin,
    McpPlugin, MediaPlugin, MemoryPlugin, PluginInstallReport, PluginRegistry, PolicyPlugin,
    ProcessToolsPlugin, ProfileSpec, SearchToolsPlugin, SessionToolsPlugin, WorkspaceToolsPlugin,
};
use crate::tools::{ToolContext, ToolRegistry};

pub struct HarnessRuntimeBuilder {
    plugins: PluginRegistry,
}

impl Default for HarnessRuntimeBuilder {
    fn default() -> Self {
        Self::builtin("full").expect("embedded full Harness profile must be valid")
    }
}

impl HarnessRuntimeBuilder {
    pub fn empty() -> Self {
        Self {
            plugins: PluginRegistry::new(),
        }
    }

    pub fn from_composition(composition: &HarnessComposition) -> Result<Self, String> {
        let mut builder = Self::empty();
        for entry in composition.enabled_entries() {
            let plugin = builtin_plugin(&entry.plugin)
                .ok_or_else(|| format!("unknown Harness plugin '{}'", entry.plugin))?;
            builder
                .plugins
                .register_configured(plugin, entry.config.clone())?;
        }
        Ok(builder)
    }

    pub fn from_files(
        root: &std::path::Path,
        profile: &str,
        overlays: &[std::path::PathBuf],
    ) -> Result<Self, String> {
        let composition = HarnessComposition::load(root, profile, overlays)?;
        Self::from_composition(&composition)
    }

    pub fn builtin(profile: &str) -> Result<Self, String> {
        Self::from_composition(&builtin_composition(profile)?)
    }

    pub fn configured(workspace: &Path) -> Result<Self, String> {
        let profile =
            std::env::var("NANOCODEX_HARNESS_PROFILE").unwrap_or_else(|_| "full".to_string());
        let workspace_root = workspace.join(".ncx").join("harness");
        let external_root = std::env::var_os("NANOCODEX_HARNESS_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                workspace_root
                    .join("profiles")
                    .is_dir()
                    .then_some(workspace_root)
            });
        let mut overlays = Vec::new();
        let workspace_overlay = workspace.join(".ncx").join("harness.overlay.toml");
        if workspace_overlay.is_file() {
            overlays.push(workspace_overlay);
        }
        if let Some(value) = std::env::var_os("NANOCODEX_HARNESS_OVERLAYS") {
            overlays.extend(std::env::split_paths(&value));
        }
        let composition = if let Some(root) = external_root {
            HarnessComposition::load(&root, &profile, &overlays)?
        } else {
            builtin_composition(&profile)?.apply_overlay_files(&overlays)?
        };
        Self::from_composition(&composition)
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
        let report = self
            .plugins
            .install_into(&mut tools)
            .expect("default Harness plugin composition must be valid");
        (tools, report)
    }
}

fn builtin_composition(profile: &str) -> Result<HarnessComposition, String> {
    let profile_spec: ProfileSpec = parse_embedded(
        profile,
        match profile {
            "full" => include_str!("../../../../harness/profiles/full.toml"),
            "coding" => include_str!("../../../../harness/profiles/coding.toml"),
            "readonly" => include_str!("../../../../harness/profiles/readonly.toml"),
            "minimal" => include_str!("../../../../harness/profiles/minimal.toml"),
            "headless" => include_str!("../../../../harness/profiles/headless.toml"),
            _ => return Err(format!("unknown built-in Harness profile '{profile}'")),
        },
    )?;
    let bundles = profile_spec
        .bundles
        .iter()
        .map(|id| embedded_bundle(id))
        .collect::<Result<Vec<_>, _>>()?;
    let composition = HarnessComposition::compose(profile, profile_spec, bundles, Vec::new())?;
    Ok(composition)
}

fn builtin_plugin(id: &str) -> Option<Rc<dyn HarnessPlugin>> {
    match id {
        "ncx.core" => Some(Rc::new(CoreToolsPlugin)),
        "ncx.search" => Some(Rc::new(SearchToolsPlugin)),
        "ncx.workspace" => Some(Rc::new(WorkspaceToolsPlugin)),
        "ncx.process" => Some(Rc::new(ProcessToolsPlugin)),
        "ncx.session" => Some(Rc::new(SessionToolsPlugin)),
        "ncx.llm" => Some(Rc::new(LlmProviderPlugin)),
        "ncx.interaction" => Some(Rc::new(InteractionPlugin)),
        "ncx.policy" => Some(Rc::new(PolicyPlugin)),
        "ncx.context" => Some(Rc::new(ContextPlugin)),
        "ncx.memory" => Some(Rc::new(MemoryPlugin)),
        "ncx.compaction" => Some(Rc::new(CompactionPlugin)),
        "ncx.mcp" => Some(Rc::new(McpPlugin)),
        "ncx.attachment" => Some(Rc::new(AttachmentPlugin)),
        "ncx.media" => Some(Rc::new(MediaPlugin)),
        "ncx.cost-telemetry" => Some(Rc::new(CostTelemetryPlugin)),
        _ => None,
    }
}

fn embedded_bundle(id: &str) -> Result<BundleSpec, String> {
    let text = match id {
        "base" => include_str!("../../../../harness/bundles/base.toml"),
        "search" => include_str!("../../../../harness/bundles/search.toml"),
        "workspace" => include_str!("../../../../harness/bundles/workspace.toml"),
        "process" => include_str!("../../../../harness/bundles/process.toml"),
        "session" => include_str!("../../../../harness/bundles/session.toml"),
        "media" => include_str!("../../../../harness/bundles/media.toml"),
        _ => return Err(format!("unknown built-in Harness bundle '{id}'")),
    };
    parse_embedded(id, text)
}

fn parse_embedded<T: for<'de> serde::Deserialize<'de>>(id: &str, text: &str) -> Result<T, String> {
    toml::from_str(text).map_err(|error| format!("invalid embedded Harness config '{id}': {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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
                "ncx.llm",
                "ncx.interaction",
                "ncx.policy",
                "ncx.context",
                "ncx.memory",
                "ncx.compaction",
                "ncx.search",
                "ncx.workspace",
                "ncx.process",
                "ncx.session",
                "ncx.mcp",
                "ncx.attachment",
                "ncx.media",
                "ncx.cost-telemetry"
            ]
        );
        let (_, report) = builder.build_with_report(context());
        assert_eq!(report.installed.len(), 15);
    }

    #[test]
    fn empty_runtime_has_no_model_facing_tools() {
        let tools = HarnessRuntimeBuilder::empty().build(context());
        assert!(tools.schemas().is_empty());
    }

    #[test]
    fn file_driven_profiles_select_components_without_changing_default_order() {
        let coding = HarnessRuntimeBuilder::builtin("coding").unwrap();
        assert_eq!(
            coding.plugin_ids().collect::<Vec<_>>(),
            vec![
                "ncx.core",
                "ncx.llm",
                "ncx.interaction",
                "ncx.policy",
                "ncx.context",
                "ncx.memory",
                "ncx.compaction",
                "ncx.search",
                "ncx.workspace",
                "ncx.session",
            ]
        );
        let minimal = HarnessRuntimeBuilder::builtin("minimal").unwrap();
        assert_eq!(
            minimal.plugin_ids().collect::<Vec<_>>(),
            vec![
                "ncx.core",
                "ncx.llm",
                "ncx.interaction",
                "ncx.policy",
                "ncx.context",
                "ncx.memory",
                "ncx.compaction",
                "ncx.workspace",
            ]
        );
        let headless = HarnessRuntimeBuilder::builtin("headless").unwrap();
        assert!(!headless.plugin_ids().any(|id| id == "ncx.media"));
        assert!(headless.plugin_ids().any(|id| id == "ncx.process"));
    }

    #[test]
    fn full_minimal_and_headless_are_real_isolated_compositions() {
        let full = HarnessRuntimeBuilder::builtin("full")
            .unwrap()
            .build(context());
        let minimal = HarnessRuntimeBuilder::builtin("minimal")
            .unwrap()
            .build(context());
        let headless = HarnessRuntimeBuilder::builtin("headless")
            .unwrap()
            .build(context());
        let full_diag = full.harness_diagnostics();
        let minimal_diag = minimal.harness_diagnostics();
        let headless_diag = headless.harness_diagnostics();
        assert!(
            full_diag.media && full_diag.attachment && full_diag.cost_telemetry && full_diag.mcp
        );
        assert!(!minimal_diag.media && !minimal_diag.mcp && !minimal_diag.attachment);
        assert!(!headless_diag.media && !headless_diag.mcp && !headless_diag.attachment);
        assert!(full.schemas().len() > minimal.schemas().len());
        assert!(headless.schemas().len() > minimal.schemas().len());
    }

    #[test]
    fn external_profile_bundle_and_overlay_drive_runtime_selection() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ncx-runtime-profile-{id}"));
        std::fs::create_dir_all(root.join("profiles")).unwrap();
        std::fs::create_dir_all(root.join("bundles")).unwrap();
        std::fs::write(
            root.join("profiles/custom.toml"),
            "name = \"custom\"\nbundles = [\"tools\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("bundles/tools.toml"),
            concat!(
                "id = \"tools\"\n",
                "[[plugins]]\nid = \"core\"\nplugin = \"ncx.core\"\n",
                "[[plugins]]\nid = \"process\"\nplugin = \"ncx.process\"\n"
            ),
        )
        .unwrap();
        let overlay = root.join("disable-process.toml");
        std::fs::write(&overlay, "[[plugins]]\nid = \"process\"\nenabled = false\n").unwrap();
        let builder = HarnessRuntimeBuilder::from_files(&root, "custom", &[overlay]).unwrap();
        assert_eq!(builder.plugin_ids().collect::<Vec<_>>(), vec!["ncx.core"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
