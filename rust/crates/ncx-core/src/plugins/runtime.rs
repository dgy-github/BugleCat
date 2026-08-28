//! Runtime composition independent from tool execution.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::external::{ExternalPluginRegistration, ExternalProcessTool};
use super::{
    AttachmentPlugin, BundleSpec, CompactionPlugin, ContextPlugin, CoreToolsPlugin,
    CostTelemetryPlugin, ExternalHostPlugin, ExternalPluginCatalog, ExternalPluginRecord,
    HarnessComposition, HarnessPlugin, InteractionPlugin, LlmProviderPlugin, McpPlugin,
    MediaPlugin, MemoryPlugin, PluginInstallReport, PluginRegistry, PolicyPlugin,
    ProcessToolsPlugin, ProfileSpec, ProviderCatalogPlugin, ProviderChatProbePlugin,
    ProviderDirectoryPlugin, SearchToolsPlugin, SessionToolsPlugin, WorkspaceToolsPlugin,
};
use crate::skills::skills_index_block;
use crate::tools::{ToolContext, ToolRegistry};
use ncx_context::{ContextEntry, ContextFragment, TextContextFragment};

pub struct HarnessRuntimeBuilder {
    plugins: PluginRegistry,
    media: Option<super::MediaServiceDescriptor>,
    external: Vec<(ExternalPluginRecord, ExternalPluginRegistration)>,
    external_enabled: bool,
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
            media: None,
            external: Vec::new(),
            external_enabled: false,
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
            if entry.plugin == "ncx.media" {
                builder.media = Some(super::MediaServiceDescriptor {
                    vision: media_flag(&entry.config, "vision"),
                    image_generation: media_flag(&entry.config, "image_generation"),
                    video_generation: media_flag(&entry.config, "video_generation"),
                });
            }
            if entry.plugin == "ncx.external" {
                builder.external_enabled = true;
            }
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
        Self::configured_for_profile(workspace, None)
    }

    /// Compose a runtime for one durable Thread. An explicit session profile
    /// wins the process environment so concurrent sessions never race through
    /// `NANOCODEX_HARNESS_PROFILE`.
    pub fn configured_for_profile(
        workspace: &Path,
        session_profile: Option<&str>,
    ) -> Result<Self, String> {
        let profile = session_profile
            .map(str::to_string)
            .or_else(|| std::env::var("NANOCODEX_HARNESS_PROFILE").ok())
            .unwrap_or_else(|| "full".to_string());
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
        let mut builder = Self::from_composition(&composition)?;
        let mut names = std::collections::HashSet::new();
        if builder.external_enabled {
            for plugin in ExternalPluginCatalog::new(workspace.join(".ncx").join("plugins"))
                .discover()?
                .into_iter()
                .filter(|plugin| plugin.enabled)
            {
                let registration = plugin.handshake()?;
                for tool in &registration.tools {
                    if !names.insert(tool.name.clone()) {
                        return Err(format!("外部插件工具 '{}' 重名", tool.name));
                    }
                }
                builder.external.push((plugin, registration));
            }
        }
        Ok(builder)
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

    pub fn build_with_report(
        self,
        mut context: ToolContext,
    ) -> (ToolRegistry, PluginInstallReport) {
        filter_skills_for_media(&mut context, self.media.as_ref());
        let mut tools = ToolRegistry::empty(context);
        let report = self
            .plugins
            .install_into(&mut tools)
            .expect("default Harness plugin composition must be valid");
        let external_tools = self
            .external
            .into_iter()
            .flat_map(|(plugin, registration)| {
                registration.tools.into_iter().map(move |descriptor| {
                    Box::new(ExternalProcessTool::new(plugin.clone(), descriptor))
                        as Box<dyn crate::Tool>
                })
            })
            .collect::<Vec<_>>();
        tools
            .replace_tools(&[], external_tools)
            .expect("validated external plugin tool names must be unique");
        (tools, report)
    }
}

fn media_flag(config: &toml::Value, name: &str) -> bool {
    config
        .get(name)
        .and_then(toml::Value::as_bool)
        .unwrap_or(true)
}

fn filter_skills_for_media(
    context: &mut ToolContext,
    media: Option<&super::MediaServiceDescriptor>,
) {
    let (vision, image_generation, video_generation) = media
        .map(|service| {
            (
                service.vision,
                service.image_generation,
                service.video_generation,
            )
        })
        .unwrap_or_default();
    let skills = context
        .skills
        .iter()
        .filter(|skill| {
            skill
                .capability
                .is_available(vision, image_generation, video_generation)
        })
        .cloned()
        .collect::<Vec<_>>();
    context.skills = Rc::new(skills);

    let skills_fragment = context
        .context_entries
        .iter()
        .find(|entry| entry.fragment.source() == "skills")
        .map(|entry| (entry.order, entry.fragment.max_chars()));
    if let Some((order, max_chars)) = skills_fragment {
        let mut entries = context
            .context_entries
            .iter()
            .filter(|entry| entry.fragment.source() != "skills")
            .cloned()
            .collect::<Vec<_>>();
        entries.push(ContextEntry {
            order,
            fragment: TextContextFragment::new(
                "skills",
                skills_index_block(context.skills.as_ref()),
                max_chars,
            ),
        });
        context.context_entries = Rc::new(entries);
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
        "ncx.provider-directory" => Some(Rc::new(ProviderDirectoryPlugin)),
        "ncx.provider-catalog" => Some(Rc::new(ProviderCatalogPlugin)),
        "ncx.provider-chat-probe" => Some(Rc::new(ProviderChatProbePlugin)),
        "ncx.interaction" => Some(Rc::new(InteractionPlugin)),
        "ncx.policy" => Some(Rc::new(PolicyPlugin)),
        "ncx.context" => Some(Rc::new(ContextPlugin)),
        "ncx.memory" => Some(Rc::new(MemoryPlugin)),
        "ncx.compaction" => Some(Rc::new(CompactionPlugin)),
        "ncx.mcp" => Some(Rc::new(McpPlugin)),
        "ncx.attachment" => Some(Rc::new(AttachmentPlugin)),
        "ncx.media" => Some(Rc::new(MediaPlugin)),
        "ncx.cost-telemetry" => Some(Rc::new(CostTelemetryPlugin)),
        "ncx.external" => Some(Rc::new(ExternalHostPlugin)),
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
        "external" => include_str!("../../../../harness/bundles/external.toml"),
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
    use crate::plugins::{
        AttachmentServiceDescriptor, CostTelemetryService, McpServiceDescriptor,
        MediaServiceDescriptor,
    };
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn context() -> ToolContext {
        let workspace = PathBuf::from("runtime-plugin-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        ToolContext::new(workspace, policy)
    }

    fn skill(name: &str, capability: crate::SkillCapability) -> crate::Skill {
        crate::Skill {
            name: name.into(),
            description: format!("{name} description"),
            capability,
            always_apply: false,
            path: PathBuf::from("<test>"),
            dir: PathBuf::from("<test>"),
            embedded: Some("test body".into()),
        }
    }

    #[test]
    fn default_runtime_reports_architectural_plugin_order() {
        let builder = HarnessRuntimeBuilder::default();
        assert_eq!(
            builder.plugin_ids().collect::<Vec<_>>(),
            vec![
                "ncx.core",
                "ncx.llm",
                "ncx.provider-directory",
                "ncx.provider-catalog",
                "ncx.provider-chat-probe",
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
                "ncx.cost-telemetry",
                "ncx.external"
            ]
        );
        let (_, report) = builder.build_with_report(context());
        assert_eq!(report.installed.len(), 19);
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
                "ncx.provider-directory",
                "ncx.provider-catalog",
                "ncx.provider-chat-probe",
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
                "ncx.provider-directory",
                "ncx.provider-catalog",
                "ncx.provider-chat-probe",
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
        assert!(!headless.plugin_ids().any(|id| id == "ncx.external"));
        assert!(!minimal.plugin_ids().any(|id| id == "ncx.external"));
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
            full_diag.media
                && full_diag.attachment
                && full_diag.cost_telemetry
                && full_diag.mcp
                && full_diag.provider_directory
                && full_diag.provider_catalog
                && full_diag.provider_chat_probe
        );
        assert!(!minimal_diag.media && !minimal_diag.mcp && !minimal_diag.attachment);
        assert!(!headless_diag.media && !headless_diag.mcp && !headless_diag.attachment);
        assert!(full.schemas().len() > minimal.schemas().len());
        assert!(headless.schemas().len() > minimal.schemas().len());

        let attachment = full
            .service::<AttachmentServiceDescriptor>("attachment")
            .expect("full profile must publish the real attachment policy");
        assert!(attachment.max_bytes > 0 && attachment.extensions.iter().any(|ext| ext == "pdf"));
        let media = full
            .service::<MediaServiceDescriptor>("media")
            .expect("full profile must publish media routing");
        assert!(media.vision && media.image_generation && media.video_generation);
        let mcp = full
            .service::<McpServiceDescriptor>("mcp")
            .expect("full profile must publish MCP lifecycle state");
        assert!(!mcp.enabled && mcp.configured_servers == 0 && mcp.active_tools == 0);
        let telemetry = full
            .service::<CostTelemetryService>("cost.telemetry")
            .expect("full profile must publish the cost accumulator");
        telemetry.record(&std::collections::BTreeMap::from([
            ("prompt_tokens".to_string(), 10),
            ("completion_tokens".to_string(), 5),
        ]));
        assert_eq!(telemetry.snapshot().turns, 1);

        for registry in [&minimal, &headless] {
            assert!(registry
                .service::<AttachmentServiceDescriptor>("attachment")
                .is_none());
            assert!(registry
                .service::<MediaServiceDescriptor>("media")
                .is_none());
            assert!(registry.service::<McpServiceDescriptor>("mcp").is_none());
            assert!(registry
                .service::<CostTelemetryService>("cost.telemetry")
                .is_none());
        }
    }

    #[test]
    fn profiles_filter_media_skills_before_tool_and_context_installation() {
        use crate::SkillCapability::{General, ImageGeneration, VideoGeneration, Vision};

        let skills = vec![
            skill("general", General),
            skill("vision", Vision),
            skill("image", ImageGeneration),
            skill("video", VideoGeneration),
        ];
        let build = |profile: &str| {
            let index = skills_index_block(&skills);
            let context = context()
                .with_skills(skills.clone())
                .with_context_entries(vec![ContextEntry {
                    order: 20,
                    fragment: TextContextFragment::new("skills", index, 32_000),
                }]);
            HarnessRuntimeBuilder::builtin(profile)
                .unwrap()
                .build(context)
        };

        let full = build("full");
        assert_eq!(full.ctx.skills.len(), 4);
        let minimal = build("minimal");
        assert_eq!(
            minimal
                .ctx
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["general"]
        );
        let prompt = minimal
            .ctx
            .context_entries
            .iter()
            .find(|entry| entry.fragment.source() == "skills")
            .unwrap()
            .fragment
            .render();
        assert!(prompt.contains("general: general description"));
        assert!(!prompt.contains("image: image description"));
        assert!(!prompt.contains("video: video description"));
        assert!(!prompt.contains("vision: vision description"));
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
