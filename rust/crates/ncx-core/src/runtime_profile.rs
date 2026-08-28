//! Shared configuration-to-runtime assembly used by CLI and GUI frontends.

use std::path::Path;
use std::rc::Rc;

use ncx_config::{permission_mode_to_knobs, Config};
use ncx_provider::{AnthropicProvider, DashScopeMediaProvider, DeepSeekProvider};
use ncx_sandbox::SandboxPolicy;

use crate::plugins::{LlmProviderFactory, LlmProviderFactoryHandle};
use crate::{
    AgentLoop, ContextEditPolicy, GenerateImageTool, GenerateVideoTool, MediaGenerationService,
    MediaPrice, Provider, RustAnalyzerProvider, TaskBudget, ToolContext,
};

const DEFAULT_MAX_MODEL_CALLS: usize = 60;
const DEFAULT_MAX_TOOL_CALLS: usize = 120;
const DEFAULT_MAX_PARALLEL_TOOL_CALLS: usize = 8;
const DEFAULT_CONTEXT_MAX_CHARS: usize = 120_000;
const DEFAULT_CONTEXT_KEEP_RECENT: usize = 30;
const DEFAULT_CONTEXT_TOOL_RESULT_CHARS: usize = 4_000;

/// Normalized runtime controls shared by every frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeProfile {
    pub permissions: RuntimePermissionProfile,
    pub task_budget: TaskBudget,
    pub max_parallel_tool_calls: usize,
    pub context_edit: ContextEditPolicy,
}

/// Permission knobs derived from a single permission-mode selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePermissionProfile {
    pub sandbox_mode: String,
    pub approval_policy: String,
    pub require_edit_approval: bool,
    pub plan_mode: bool,
    pub network_access: bool,
}

impl AgentRuntimeProfile {
    /// Resolve the same runtime controls from the same persisted config.
    pub fn from_config(cfg: &Config) -> Self {
        Self::from_permission_mode(cfg, &cfg.permission_mode)
    }

    /// Resolve runtime controls with an explicit permission-mode override.
    pub fn from_permission_mode(cfg: &Config, permission_mode: &str) -> Self {
        let (sandbox_mode, approval_policy, require_edit_approval, plan_mode) =
            permission_mode_to_knobs(permission_mode);
        let permissions = RuntimePermissionProfile {
            sandbox_mode: sandbox_mode.to_string(),
            approval_policy: approval_policy.to_string(),
            require_edit_approval,
            plan_mode,
            network_access: sandbox_mode == "danger-full-access",
        };
        Self::with_permissions(cfg, permissions)
    }

    /// Preserve explicit legacy `--sandbox` / `--approval` CLI overrides.
    pub fn from_legacy_permissions(cfg: &Config) -> Self {
        Self::with_permissions(
            cfg,
            RuntimePermissionProfile {
                sandbox_mode: cfg.sandbox_mode.clone(),
                approval_policy: cfg.approval_policy.clone(),
                require_edit_approval: false,
                plan_mode: false,
                network_access: cfg.sandbox_mode == "danger-full-access",
            },
        )
    }

    fn with_permissions(cfg: &Config, permissions: RuntimePermissionProfile) -> Self {
        Self {
            permissions,
            task_budget: TaskBudget {
                max_model_calls: positive_usize(cfg.max_iterations, DEFAULT_MAX_MODEL_CALLS),
                max_tool_calls: nonnegative_usize(cfg.max_tool_calls, DEFAULT_MAX_TOOL_CALLS),
            },
            max_parallel_tool_calls: positive_usize(
                cfg.max_parallel_tool_calls,
                DEFAULT_MAX_PARALLEL_TOOL_CALLS,
            ),
            context_edit: ContextEditPolicy {
                enabled: cfg.context_edit_enabled,
                max_chars: positive_usize(cfg.context_edit_max_chars, DEFAULT_CONTEXT_MAX_CHARS),
                keep_recent_messages: positive_usize(
                    cfg.context_edit_keep_recent_messages,
                    DEFAULT_CONTEXT_KEEP_RECENT,
                ),
                max_tool_result_chars: positive_usize(
                    cfg.context_edit_max_tool_result_chars,
                    DEFAULT_CONTEXT_TOOL_RESULT_CHARS,
                ),
            },
        }
    }

    /// Apply the normalized profile to a newly assembled agent.
    pub fn apply(self, agent: AgentLoop) -> AgentLoop {
        agent
            .with_task_budget(self.task_budget)
            .with_max_parallel_tool_calls(self.max_parallel_tool_calls)
            .with_context_edit(self.context_edit)
    }

    /// Build the sandbox policy represented by this profile.
    pub fn sandbox_policy(&self, workspace: impl AsRef<Path>) -> SandboxPolicy {
        SandboxPolicy::new(&self.permissions.sandbox_mode, workspace)
            .with_network_access(self.permissions.network_access)
    }

    /// Apply shared permission knobs to a frontend-specific tool context.
    pub fn apply_tool_context(&self, context: ToolContext) -> ToolContext {
        let lsp = Rc::new(RustAnalyzerProvider::new(context.workspace.clone()));
        context
            .with_approval_policy(self.permissions.approval_policy.clone())
            .with_require_edit_approval(self.permissions.require_edit_approval)
            .with_plan_mode(self.permissions.plan_mode)
            .with_lsp_provider(lsp)
    }
}

/// Build the primary or tier-specific OpenAI-compatible provider with the
/// shared timeout, retry, endpoint, and API-key mapping.
pub fn model_provider_from_config(cfg: &Config, model: impl Into<String>) -> Box<dyn Provider> {
    let model = model.into();
    if cfg.provider_protocol == "anthropic" {
        return Box::new(AnthropicProvider::new(
            cfg.api_key.clone(),
            &cfg.base_url,
            model,
            cfg.timeout_s as u64,
        ));
    }
    Box::new(DeepSeekProvider::with_opts(
        cfg.api_key.clone(),
        &cfg.base_url,
        model,
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    ))
}

/// Build the optional vision provider using the main endpoint/key as fallback.
pub fn vision_provider_from_config(cfg: &Config) -> Option<Box<dyn Provider>> {
    if !cfg.alibaba_attachment_parser_enabled || cfg.vl_model.trim().is_empty() {
        return None;
    }
    let base_url = if cfg.vl_base_url.trim().is_empty() {
        &cfg.base_url
    } else {
        &cfg.vl_base_url
    };
    let api_key = if cfg.vl_api_key.trim().is_empty() {
        cfg.api_key.clone()
    } else {
        cfg.vl_api_key.clone()
    };
    Some(Box::new(DeepSeekProvider::with_opts(
        api_key,
        base_url,
        cfg.vl_model.clone(),
        cfg.timeout_s as u64,
        cfg.max_retries as u32,
    )))
}

/// Conservative built-in capability catalog used before sending image blocks.
/// Unknown model IDs fail closed and can opt into the local parser plugin.
pub fn model_supports_native_vision(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.contains("vision")
        || model.contains("-vl")
        || model.starts_with("gpt-5.6-")
        || model.starts_with("gemini-")
}

/// Frontend configuration adapter installed as the Harness LLM service.
pub struct ConfiguredLlmProviderFactory {
    cfg: Config,
    model: String,
}

impl ConfiguredLlmProviderFactory {
    pub fn new(cfg: Config, model: impl Into<String>) -> Self {
        Self {
            cfg,
            model: model.into(),
        }
    }
}

impl LlmProviderFactory for ConfiguredLlmProviderFactory {
    fn primary(&self) -> Box<dyn Provider> {
        model_provider_from_config(&self.cfg, self.model.clone())
    }

    fn vision(&self) -> Option<Box<dyn Provider>> {
        vision_provider_from_config(&self.cfg)
    }
}

pub fn install_llm_provider_factory(
    tools: &mut crate::ToolRegistry,
    cfg: Config,
    model: impl Into<String>,
) {
    install_media_provider(tools, &cfg);
    let pricing = crate::plugins::CostTelemetryServiceDescriptor {
        currency: cfg.price_currency.clone(),
        input_per_million: cfg.price_in,
        output_per_million: cfg.price_out,
        telemetry_enabled: true,
    };
    tools.replace_service(
        "llm.factory",
        Rc::new(LlmProviderFactoryHandle(Rc::new(
            ConfiguredLlmProviderFactory::new(cfg, model),
        ))),
    );
    if tools
        .service::<crate::plugins::CostTelemetryService>("cost.telemetry")
        .is_some()
    {
        tools.replace_service(
            "cost.telemetry",
            Rc::new(crate::plugins::CostTelemetryService::new(pricing)),
        );
    }
}

fn install_media_provider(tools: &mut crate::ToolRegistry, cfg: &Config) {
    let Some(capabilities) = tools.service::<crate::plugins::MediaServiceDescriptor>("media")
    else {
        return;
    };
    let media_key = if cfg.dashscope_workspace_key.trim().is_empty() {
        cfg.vl_api_key.trim()
    } else {
        cfg.dashscope_workspace_key.trim()
    };
    if media_key.is_empty() {
        return;
    }
    let telemetry = tools.service::<crate::plugins::CostTelemetryService>("cost.telemetry");
    let service = Rc::new(MediaGenerationService {
        provider: Rc::new(DashScopeMediaProvider::new(
            media_key.to_string(),
            cfg.timeout_s as u64,
        )),
        image: media_price("NANOCODEX_IMAGE_PRICE_CNY", 0.14, "张"),
        video: media_price("NANOCODEX_VIDEO_PRICE_CNY_PER_SECOND", 0.24, "秒"),
        telemetry,
    });
    let mut additions: Vec<Box<dyn crate::Tool>> = Vec::new();
    if capabilities.image_generation {
        additions.push(Box::new(GenerateImageTool::new(service.clone())));
    }
    if capabilities.video_generation {
        additions.push(Box::new(GenerateVideoTool::new(service.clone())));
    }
    if !additions.is_empty() {
        tools
            .replace_tools(&[], additions)
            .expect("media tool names must not conflict with builtins");
        tools.replace_service("media.generation", service);
    }
}

fn media_price(env_name: &str, fallback: f64, unit: &str) -> MediaPrice {
    let amount = std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value: &f64| value.is_finite() && *value >= 0.0)
        .unwrap_or(fallback);
    MediaPrice {
        amount,
        unit: unit.into(),
        currency: "CNY".into(),
        source: "https://help.aliyun.com/zh/model-studio/model-pricing".into(),
        audited_at: "2026-08-25（当前网络未能重新打开官方页，可用环境变量覆盖）".into(),
    }
}

fn positive_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .unwrap_or(fallback)
}

fn nonnegative_usize(value: i64, fallback: usize) -> usize {
    usize::try_from(value).ok().unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{Session, ToolContext, ToolRegistry};

    fn assemble_frontend(cfg: &Config, frontend: &str) -> AgentLoop {
        let workspace = PathBuf::from(format!("runtime-profile-{frontend}"));
        let profile = AgentRuntimeProfile::from_config(cfg);
        let policy = profile.sandbox_policy(&workspace);
        let context = profile.apply_tool_context(ToolContext::new(workspace, policy));
        let tools = ToolRegistry::empty(context);
        let provider = model_provider_from_config(cfg, cfg.model.clone());
        profile
            .apply(AgentLoop::new(provider, tools, Session::new(frontend)))
            .with_vision_provider(vision_provider_from_config(cfg))
    }

    #[test]
    fn cli_and_gui_runtime_assembly_is_equivalent_for_same_config() {
        let cfg = Config {
            model: "shared-main-model".into(),
            vl_model: "shared-vision-model".into(),
            permission_mode: "default".into(),
            max_iterations: 7,
            max_tool_calls: 19,
            max_parallel_tool_calls: 3,
            context_edit_enabled: false,
            context_edit_max_chars: 9_000,
            context_edit_keep_recent_messages: 11,
            context_edit_max_tool_result_chars: 777,
            ..Default::default()
        };

        let cli = assemble_frontend(&cfg, "cli");
        let gui = assemble_frontend(&cfg, "gui");
        let cli_profile = cli.runtime_profile();
        let gui_profile = gui.runtime_profile();

        assert_eq!(cli_profile, gui_profile);
        assert_eq!(cli.provider_model(), gui.provider_model());
        assert_eq!(
            cli.vision_provider
                .as_ref()
                .map(|provider| provider.model()),
            gui.vision_provider
                .as_ref()
                .map(|provider| provider.model())
        );
        assert_eq!(
            cli_profile,
            AgentRuntimeProfile {
                permissions: RuntimePermissionProfile {
                    sandbox_mode: "workspace-write".into(),
                    approval_policy: "untrusted".into(),
                    require_edit_approval: true,
                    plan_mode: false,
                    network_access: false,
                },
                task_budget: TaskBudget {
                    max_model_calls: 7,
                    max_tool_calls: 19,
                },
                max_parallel_tool_calls: 3,
                context_edit: ContextEditPolicy {
                    enabled: false,
                    max_chars: 9_000,
                    keep_recent_messages: 11,
                    max_tool_result_chars: 777,
                },
            }
        );
    }

    #[test]
    fn invalid_numeric_values_use_runtime_defaults() {
        let cfg = Config {
            max_iterations: 0,
            max_tool_calls: -1,
            max_parallel_tool_calls: 0,
            context_edit_max_chars: 0,
            context_edit_keep_recent_messages: -1,
            context_edit_max_tool_result_chars: 0,
            ..Default::default()
        };
        let profile = AgentRuntimeProfile::from_config(&cfg);

        assert_eq!(profile.task_budget.max_model_calls, DEFAULT_MAX_MODEL_CALLS);
        assert_eq!(profile.task_budget.max_tool_calls, DEFAULT_MAX_TOOL_CALLS);
        assert_eq!(
            profile.max_parallel_tool_calls,
            DEFAULT_MAX_PARALLEL_TOOL_CALLS
        );
        assert_eq!(profile.context_edit.max_chars, DEFAULT_CONTEXT_MAX_CHARS);
        assert_eq!(
            profile.context_edit.keep_recent_messages,
            DEFAULT_CONTEXT_KEEP_RECENT
        );
        assert_eq!(
            profile.context_edit.max_tool_result_chars,
            DEFAULT_CONTEXT_TOOL_RESULT_CHARS
        );
    }

    #[test]
    fn alibaba_attachment_parser_is_opt_in_and_native_vision_is_catalogued() {
        let disabled = Config {
            vl_model: "qwen3.7-plus".into(),
            vl_api_key: "secret".into(),
            ..Default::default()
        };
        assert!(vision_provider_from_config(&disabled).is_none());

        let enabled = Config {
            alibaba_attachment_parser_enabled: true,
            ..disabled
        };
        assert!(vision_provider_from_config(&enabled).is_some());
        assert!(model_supports_native_vision("gpt-5.6-sol"));
        assert!(!model_supports_native_vision("deepseek-v4-pro"));
    }

    #[test]
    fn explicit_legacy_permissions_remain_available_for_cli_flags() {
        let cfg = Config {
            sandbox_mode: "read-only".into(),
            approval_policy: "never".into(),
            permission_mode: "accept-edits".into(),
            ..Default::default()
        };

        let profile = AgentRuntimeProfile::from_legacy_permissions(&cfg);

        assert_eq!(profile.permissions.sandbox_mode, "read-only");
        assert_eq!(profile.permissions.approval_policy, "never");
        assert!(!profile.permissions.require_edit_approval);
        assert!(!profile.permissions.plan_mode);
    }

    #[test]
    fn media_tools_require_both_full_profile_capability_and_dashscope_key() {
        let workspace = PathBuf::from("media-install-test");
        let context = || {
            ToolContext::new(
                workspace.clone(),
                ncx_sandbox::SandboxPolicy::new(ncx_sandbox::WORKSPACE_WRITE, &workspace),
            )
        };
        let cfg = Config {
            dashscope_workspace_key: "dashscope-workspace-secret".into(),
            ..Default::default()
        };
        let mut full = crate::HarnessRuntimeBuilder::builtin("full")
            .unwrap()
            .build(context());
        install_llm_provider_factory(&mut full, cfg.clone(), cfg.model.clone());
        assert!(full.get("generate_image").is_some());
        assert!(full.get("generate_video").is_some());
        assert!(full
            .service::<MediaGenerationService>("media.generation")
            .is_some());

        let mut minimal = crate::HarnessRuntimeBuilder::builtin("minimal")
            .unwrap()
            .build(context());
        install_llm_provider_factory(&mut minimal, cfg.clone(), cfg.model.clone());
        assert!(minimal.get("generate_image").is_none());
        assert!(minimal.get("generate_video").is_none());

        let mut no_key = crate::HarnessRuntimeBuilder::builtin("full")
            .unwrap()
            .build(context());
        install_llm_provider_factory(&mut no_key, Config::default(), "model");
        assert!(no_key.get("generate_image").is_none());

        let mut legacy = crate::HarnessRuntimeBuilder::builtin("full")
            .unwrap()
            .build(context());
        let legacy_cfg = Config {
            vl_api_key: "legacy-dashscope-secret".into(),
            ..Default::default()
        };
        install_llm_provider_factory(&mut legacy, legacy_cfg.clone(), legacy_cfg.model.clone());
        assert!(legacy.get("generate_image").is_some());
    }
}
