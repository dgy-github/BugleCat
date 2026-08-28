//! Configuration-backed Harness assembly shared by CLI, GUI, and workers.
//!
//! Frontends provide only host interaction adapters and source text. This
//! module owns Provider, Policy, and ContextFragment construction so those
//! runtime contracts cannot drift between entry points.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use ncx_config::{Config, HookConfig};
use ncx_context::{ContextEntry, TextContextFragment};

use crate::goal_tools::GoalToolService;
use crate::{
    install_llm_provider_factory, AgentRuntimeProfile, ApprovalHandler, Genome,
    HarnessRuntimeBuilder, MemoryStore, SessionGrants, Skill, ToolContext, ToolRegistry,
    UserQuestionHandler,
};

const INSTRUCTIONS_ORDER: u16 = 10;
const SKILLS_ORDER: u16 = 20;
const PLAN_ORDER: u16 = 30;

/// Model-facing context owned by the Context provider boundary.
pub struct RuntimeContextSources {
    pub instructions: String,
    pub skills: Vec<Skill>,
    pub plan_note: String,
    pub memory: Option<Rc<MemoryStore>>,
    pub hooks: Vec<HookConfig>,
    pub genome: Genome,
}

impl RuntimeContextSources {
    pub fn new(instructions: String, skills: Vec<Skill>, plan_note: String) -> Self {
        Self {
            instructions,
            skills,
            plan_note,
            memory: None,
            hooks: Vec::new(),
            genome: Genome::default(),
        }
    }

    pub fn with_memory(mut self, memory: Rc<MemoryStore>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_hooks(mut self, hooks: Vec<HookConfig>) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_genome(mut self, genome: Genome) -> Self {
        self.genome = genome;
        self
    }
}

/// Frontend-specific interaction ports. No policy or model configuration is
/// accepted here; those belong to the configured Harness runtime.
#[derive(Clone, Default)]
pub struct RuntimeHostBindings {
    pub approver: Option<Rc<dyn ApprovalHandler>>,
    pub questioner: Option<Rc<dyn UserQuestionHandler>>,
    pub grants: Option<Rc<RefCell<SessionGrants>>>,
    pub goal_service: Option<Rc<dyn GoalToolService>>,
}

/// Single owner for Provider, Policy, and ContextFragment runtime assembly.
pub struct ConfiguredHarnessRuntime {
    cfg: Config,
    model: String,
    profile: AgentRuntimeProfile,
    harness_profile: Option<String>,
}

impl ConfiguredHarnessRuntime {
    pub fn new(cfg: Config, model: impl Into<String>, profile: AgentRuntimeProfile) -> Self {
        Self {
            cfg,
            model: model.into(),
            profile,
            harness_profile: None,
        }
    }

    pub fn from_config(cfg: Config) -> Self {
        let model = cfg.model.clone();
        let profile = AgentRuntimeProfile::from_config(&cfg);
        Self::new(cfg, model, profile)
    }

    pub fn profile(&self) -> &AgentRuntimeProfile {
        &self.profile
    }

    pub fn with_harness_profile(mut self, profile: impl Into<String>) -> Self {
        self.harness_profile = Some(profile.into());
        self
    }

    /// Resolve the configured primary Provider through the same runtime owner
    /// used by full agents and background helpers such as titles/summaries.
    pub fn primary_provider(&self) -> Box<dyn crate::Provider> {
        crate::model_provider_from_config(&self.cfg, self.model.clone())
    }

    /// Build the complete registry through the selected Profile/Bundle path.
    pub fn build_tools(
        &self,
        workspace: PathBuf,
        sources: RuntimeContextSources,
        bindings: RuntimeHostBindings,
    ) -> Result<ToolRegistry, String> {
        let context = self.build_context(workspace.clone(), sources, bindings);
        let mut tools = HarnessRuntimeBuilder::configured_for_profile(
            &workspace,
            self.harness_profile.as_deref(),
        )?
        .build(context);
        if tools.ctx.goal_service.is_some() {
            for tool in crate::goal_tools::goal_tools() {
                if tools.get(tool.name()).is_none() {
                    tools.register(tool);
                }
            }
        }
        install_llm_provider_factory(&mut tools, self.cfg.clone(), self.model.clone());
        Ok(tools)
    }

    /// Build a tool-less registry for reasoning-only workers while preserving
    /// the same Provider, Policy, and Context contracts.
    pub fn build_toolless(
        &self,
        workspace: PathBuf,
        sources: RuntimeContextSources,
        bindings: RuntimeHostBindings,
    ) -> ToolRegistry {
        let context = self.build_context(workspace, sources, bindings);
        let mut tools = ToolRegistry::empty(context);
        tools.replace_service(
            "policy",
            Rc::new(crate::plugins::PolicyService {
                sandbox: tools.ctx.policy.clone(),
                approval_policy: tools.ctx.approval_policy.clone(),
                plan_mode: tools.ctx.plan_mode,
            }),
        );
        tools.replace_service(
            "context",
            Rc::new(crate::plugins::context_descriptor(&tools.ctx)),
        );
        tools.replace_service(
            "interaction",
            Rc::new(crate::plugins::InteractionService {
                approver: tools.ctx.approver.clone(),
            }),
        );
        install_llm_provider_factory(&mut tools, self.cfg.clone(), self.model.clone());
        tools
    }

    fn build_context(
        &self,
        workspace: PathBuf,
        sources: RuntimeContextSources,
        bindings: RuntimeHostBindings,
    ) -> ToolContext {
        let entries = context_entries(&sources);
        let policy = self.profile.sandbox_policy(&workspace);
        let mut context = self
            .profile
            .apply_tool_context(ToolContext::new(workspace, policy))
            .with_timeout(self.cfg.timeout_s as u64)
            .with_search(
                self.cfg.search_provider.clone(),
                self.cfg.search_api_key.clone(),
            )
            .with_hooks(sources.hooks)
            .with_skills(sources.skills)
            .with_genome(sources.genome)
            .with_context_entries(entries);
        if let Some(memory) = sources.memory {
            context = context.with_memory(memory);
        }
        if let Some(goal_service) = bindings.goal_service {
            context = context.with_goal_service(goal_service);
        }
        if let Some(grants) = bindings.grants {
            context = context.with_session_grants(grants);
        }
        if let Some(approver) = bindings.approver {
            context = context.with_approver(approver);
        }
        if let Some(questioner) = bindings.questioner {
            context = context.with_user_question_handler(questioner);
        }
        context
    }
}

fn context_entries(sources: &RuntimeContextSources) -> Vec<ContextEntry> {
    vec![
        ContextEntry {
            order: INSTRUCTIONS_ORDER,
            fragment: TextContextFragment::new(
                "project_instructions",
                sources.instructions.clone(),
                16_000,
            ),
        },
        ContextEntry {
            order: SKILLS_ORDER,
            fragment: TextContextFragment::new(
                "skills",
                crate::skills_index_block(&sources.skills),
                32_000,
            ),
        },
        ContextEntry {
            order: PLAN_ORDER,
            fragment: TextContextFragment::new("plan_mode", sources.plan_note.clone(), 4_000),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_config::Config;
    use ncx_protocol::{GoalBlockReason, GoalRef, GoalView};

    struct TestGoalService;
    impl GoalToolService for TestGoalService {
        fn get(&self) -> Result<Option<GoalView>, String> {
            Ok(None)
        }
        fn create(&self, _: String, _: u32) -> Result<GoalView, String> {
            Err("unused".into())
        }
        fn edit(&self, _: GoalRef, _: String, _: u32) -> Result<GoalView, String> {
            Err("unused".into())
        }
        fn pause(&self, _: GoalRef) -> Result<GoalView, String> {
            Err("unused".into())
        }
        fn resume(&self, _: GoalRef) -> Result<GoalView, String> {
            Err("unused".into())
        }
        fn complete(&self, _: GoalRef) -> Result<GoalView, String> {
            Err("unused".into())
        }
        fn block(&self, _: GoalRef, _: GoalBlockReason) -> Result<GoalView, String> {
            Err("unused".into())
        }
    }

    #[test]
    fn configured_runtime_owns_policy_provider_and_context_fragments() {
        let cfg = Config {
            workspace: PathBuf::from("runtime-assembly-test"),
            permission_mode: "plan".into(),
            api_key: "test-key".into(),
            ..Default::default()
        };
        let runtime = ConfiguredHarnessRuntime::from_config(cfg.clone());
        let tools = runtime
            .build_tools(
                cfg.workspace.clone(),
                RuntimeContextSources::new("project rules".into(), Vec::new(), "plan only".into()),
                RuntimeHostBindings::default(),
            )
            .unwrap();

        let policy = tools
            .service::<crate::plugins::PolicyService>("policy")
            .expect("policy provider");
        assert!(policy.plan_mode);
        assert_eq!(policy.sandbox.mode, ncx_sandbox::READ_ONLY);
        assert!(tools
            .service::<crate::plugins::LlmProviderFactoryHandle>("llm.factory")
            .is_some());
        let context = tools
            .service::<crate::plugins::ContextServiceDescriptor>("context")
            .expect("context provider")
            .assemble("base");
        assert!(context.contains("project rules"));
        assert!(context.contains("plan only"));
    }

    #[test]
    fn tool_and_toolless_paths_share_the_same_runtime_contracts() {
        let cfg = Config {
            workspace: PathBuf::from("runtime-tool-less-test"),
            api_key: "test-key".into(),
            ..Default::default()
        };
        let runtime = ConfiguredHarnessRuntime::new(
            cfg.clone(),
            "worker-model",
            AgentRuntimeProfile::from_legacy_permissions(&cfg),
        );
        let tools = runtime.build_toolless(
            cfg.workspace,
            RuntimeContextSources::new("instructions".into(), Vec::new(), String::new()),
            RuntimeHostBindings::default(),
        );
        assert!(tools.schemas().is_empty());
        let factory = tools
            .service::<crate::plugins::LlmProviderFactoryHandle>("llm.factory")
            .expect("llm factory");
        assert_eq!(factory.0.primary().model(), "worker-model");
        assert_eq!(
            tools.ctx.policy.mode,
            runtime.profile.permissions.sandbox_mode
        );
    }

    #[test]
    fn goal_tools_remain_executable_in_every_session_profile() {
        let cfg = Config {
            workspace: PathBuf::from("runtime-minimal-goal-test"),
            api_key: "test-key".into(),
            ..Default::default()
        };
        let tools = ConfiguredHarnessRuntime::from_config(cfg.clone())
            .with_harness_profile("minimal")
            .build_tools(
                cfg.workspace,
                RuntimeContextSources::new(String::new(), Vec::new(), String::new()),
                RuntimeHostBindings {
                    goal_service: Some(Rc::new(TestGoalService)),
                    ..Default::default()
                },
            )
            .unwrap();

        for name in ["get_goal", "create_goal", "update_goal"] {
            assert!(tools.get(name).is_some(), "missing executable {name}");
            assert!(tools.schemas().iter().any(|schema| schema
                .pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                == Some(name)));
        }
    }
}
