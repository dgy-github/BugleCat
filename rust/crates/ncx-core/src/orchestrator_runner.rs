//! Shared live runner for CLI and GUI orchestration hosts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use ncx_config::Config;
use regex::{NoExpand, RegexBuilder};
use serde_json::json;

use crate::isolate::copy_tree;
use crate::workspace_promotion::{promote, snapshot, WorkspaceSnapshot};
use crate::{
    discover_skills, load_workspace_instructions, tool_recovery::classify_tool_result,
    AgentCallResult, AgentLoop, AgentRunner, AgentRuntimeProfile, ConfiguredHarnessRuntime,
    ContextServiceDescriptor, LoopEvent, MemoryStore, RuntimeContextSources, RuntimeHostBindings,
    Session, Tier,
};

type CancelCheck = Rc<dyn Fn() -> bool>;
type LoopObserver = Rc<dyn Fn(HarnessRunnerEvent)>;
static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessRunnerEvent {
    WorkerToolStarted {
        worker: usize,
        tool: String,
    },
    WorkerToolFinished {
        worker: usize,
        tool: String,
        failure: Option<String>,
    },
}

pub struct HarnessAgentRunner {
    cfg: Config,
    memory: Rc<MemoryStore>,
    bindings: RuntimeHostBindings,
    cancel: Option<CancelCheck>,
    observer: Option<LoopObserver>,
    harness_profile: Option<String>,
    scratch: RefCell<HashMap<usize, WorkerWorkspace>>,
}

struct WorkerWorkspace {
    root: PathBuf,
    baseline: WorkspaceSnapshot,
}

impl HarnessAgentRunner {
    pub fn new(cfg: Config) -> Self {
        let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
        let _ = memory.consolidate(0.85);
        Self {
            cfg,
            memory,
            bindings: RuntimeHostBindings::default(),
            cancel: None,
            observer: None,
            harness_profile: None,
            scratch: RefCell::new(HashMap::new()),
        }
    }

    pub fn with_bindings(mut self, bindings: RuntimeHostBindings) -> Self {
        self.bindings = bindings;
        self
    }

    pub fn with_cancel(mut self, cancel: CancelCheck) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_observer(mut self, observer: LoopObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn with_harness_profile(mut self, profile: impl Into<String>) -> Self {
        self.harness_profile = Some(profile.into());
        self
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    fn model_for(&self, tier: Tier) -> String {
        match tier {
            Tier::Main => self.cfg.model.clone(),
            Tier::Fast if !self.cfg.fast_model.trim().is_empty() => self.cfg.fast_model.clone(),
            Tier::Fast => self.cfg.model.clone(),
        }
    }

    async fn run_in(
        &self,
        workspace: &Path,
        tier: Tier,
        system: &str,
        task: &str,
        with_tools: bool,
        worker: Option<usize>,
    ) -> AgentCallResult {
        let model = self.model_for(tier);
        let sources = RuntimeContextSources::new(
            load_workspace_instructions(workspace, 16_000),
            discover_skills(workspace),
            String::new(),
        )
        .with_memory(self.memory.clone())
        .with_hooks(self.cfg.hooks.clone());
        let mut runtime = ConfiguredHarnessRuntime::new(
            self.cfg.clone(),
            model.clone(),
            AgentRuntimeProfile::from_legacy_permissions(&self.cfg),
        );
        if let Some(profile) = &self.harness_profile {
            runtime = runtime.with_harness_profile(profile.clone());
        }
        let tools = if with_tools {
            match runtime.build_tools(workspace.to_path_buf(), sources, self.bindings.clone()) {
                Ok(tools) => tools,
                Err(error) => {
                    return AgentCallResult {
                        text: format!("Harness 配置错误：{error}"),
                        requested_model: Some(model),
                        ..Default::default()
                    }
                }
            }
        } else {
            runtime.build_toolless(workspace.to_path_buf(), sources, self.bindings.clone())
        };
        let assembled = tools
            .service::<ContextServiceDescriptor>("context")
            .expect("configured context service")
            .assemble(system);
        let mut agent = runtime.profile().clone().apply(
            AgentLoop::from_runtime_services(tools, Session::new(assembled))
                .expect("LLM factory service"),
        );
        let confirmed = Rc::new(RefCell::new(None));
        let confirmed_sink = confirmed.clone();
        let observer = self.observer.clone();
        agent.set_event_sink(Box::new(move |event| {
            if let LoopEvent::AssistantText {
                confirmed_model, ..
            } = &event
            {
                if confirmed_model.is_some() {
                    *confirmed_sink.borrow_mut() = confirmed_model.clone();
                }
            }
            if let (Some(observer), Some(worker)) = (&observer, worker) {
                if let Some(activity) = worker_activity(worker, &event) {
                    observer(activity);
                }
            }
        }));
        let cancelled = || self.cancel.as_ref().is_some_and(|check| check());
        let result = agent.run_turn(json!(task), Some(&cancelled)).await;
        let confirmed_model = confirmed.borrow().clone();
        AgentCallResult {
            text: result.final_text,
            usage: result.usage,
            requested_model: Some(model),
            confirmed_model,
            cancelled: matches!(result.stop_reason.as_str(), "cancelled" | "canceled"),
        }
    }

    fn prepare_worker_workspace(&self, index: usize) -> Result<PathBuf, String> {
        if let Some(old) = self.scratch.borrow_mut().remove(&index) {
            let _ = std::fs::remove_dir_all(old.root);
        }
        let next = SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let scratch =
            std::env::temp_dir().join(format!("ncx_worker_{}_{next}", std::process::id()));
        match copy_tree(&self.cfg.workspace, &scratch) {
            Ok(_) => {
                let baseline = snapshot(&scratch).map_err(|error| {
                    let _ = std::fs::remove_dir_all(&scratch);
                    format!("worker {} baseline failed: {error}", index + 1)
                })?;
                self.scratch.borrow_mut().insert(
                    index,
                    WorkerWorkspace {
                        root: scratch.clone(),
                        baseline,
                    },
                );
                Ok(scratch)
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&scratch);
                Err(format!("worker {} isolation failed: {error}", index + 1))
            }
        }
    }
}

#[async_trait(?Send)]
impl AgentRunner for HarnessAgentRunner {
    async fn run(&self, tier: Tier, system: &str, task: &str) -> String {
        self.run_result(tier, system, task).await.text
    }

    async fn run_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        self.run_in(&self.cfg.workspace, tier, system, task, true, None)
            .await
    }

    async fn reason(&self, tier: Tier, system: &str, task: &str) -> String {
        self.reason_result(tier, system, task).await.text
    }

    async fn reason_result(&self, tier: Tier, system: &str, task: &str) -> AgentCallResult {
        self.run_in(&self.cfg.workspace, tier, system, task, false, None)
            .await
    }

    async fn run_worker(&self, index: usize, count: usize, system: &str, task: &str) -> String {
        self.run_worker_result(index, count, system, task)
            .await
            .text
    }

    async fn run_worker_result(
        &self,
        index: usize,
        _count: usize,
        system: &str,
        task: &str,
    ) -> AgentCallResult {
        let workspace = match self.prepare_worker_workspace(index) {
            Ok(workspace) => workspace,
            Err(error) => {
                return AgentCallResult {
                    text: format!("[worker setup failed — no changes were made] {error}"),
                    ..Default::default()
                }
            }
        };
        let mut result = self
            .run_in(&workspace, Tier::Fast, system, task, true, Some(index))
            .await;
        result.text = remap_workspace_paths(&result.text, &workspace, &self.cfg.workspace);
        result
    }

    async fn promote_worker(&self, index: usize) -> Result<(), String> {
        let result = self
            .scratch
            .borrow()
            .get(&index)
            .ok_or_else(|| format!("Worker {} 没有可提升的隔离工作区", index + 1))
            .and_then(|worker| {
                promote(&worker.baseline, &worker.root, &self.cfg.workspace).map(|_| ())
            });
        for (_, worker) in self.scratch.borrow_mut().drain() {
            let _ = std::fs::remove_dir_all(worker.root);
        }
        result
    }
}

impl Drop for HarnessAgentRunner {
    fn drop(&mut self) {
        for (_, worker) in self.scratch.get_mut().drain() {
            let _ = std::fs::remove_dir_all(worker.root);
        }
    }
}

fn remap_workspace_paths(text: &str, isolated: &Path, live: &Path) -> String {
    let isolated_native = isolated.to_string_lossy();
    let live_native = live.to_string_lossy();
    let mut remapped = replace_path_variant(text, isolated_native.as_ref(), live_native.as_ref());
    let isolated_slash = isolated_native.replace('\\', "/");
    let live_slash = live_native.replace('\\', "/");
    if isolated_slash != isolated_native {
        remapped = replace_path_variant(&remapped, &isolated_slash, &live_slash);
    }
    remapped
}

fn replace_path_variant(text: &str, source: &str, destination: &str) -> String {
    RegexBuilder::new(&regex::escape(source))
        .case_insensitive(cfg!(windows))
        .build()
        .map(|pattern| {
            pattern
                .replace_all(text, NoExpand(destination))
                .into_owned()
        })
        .unwrap_or_else(|_| text.to_string())
}

fn worker_activity(worker: usize, event: &LoopEvent) -> Option<HarnessRunnerEvent> {
    match event {
        LoopEvent::ToolStart { name, .. } => Some(HarnessRunnerEvent::WorkerToolStarted {
            worker,
            tool: name.clone(),
        }),
        LoopEvent::ToolResult { name, result } => Some(HarnessRunnerEvent::WorkerToolFinished {
            worker,
            tool: name.clone(),
            failure: classify_tool_result(result).map(|kind| kind.to_string()),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_failure_is_fail_closed() {
        let mut config = Config::default();
        config.workspace =
            std::env::temp_dir().join(format!("ncx_missing_runner_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&config.workspace);
        let runner = HarnessAgentRunner::new(config.clone());
        let error = runner.prepare_worker_workspace(0).unwrap_err();
        assert!(error.contains("isolation failed"), "{error}");
        assert!(runner.scratch.borrow().is_empty());
        assert!(!config.workspace.exists());
    }

    #[tokio::test]
    async fn runner_promotion_applies_deletions_and_cleans_all_scratch() {
        let workspace =
            std::env::temp_dir().join(format!("ncx_runner_promotion_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("remove.txt"), "old").unwrap();
        let mut config = Config::default();
        config.workspace = workspace.clone();
        let runner = HarnessAgentRunner::new(config);
        let worker = runner.prepare_worker_workspace(0).unwrap();
        std::fs::remove_file(worker.join("remove.txt")).unwrap();
        std::fs::write(worker.join("added.txt"), "new").unwrap();

        runner.promote_worker(0).await.unwrap();

        assert!(!workspace.join("remove.txt").exists());
        assert_eq!(
            std::fs::read_to_string(workspace.join("added.txt")).unwrap(),
            "new"
        );
        assert!(runner.scratch.borrow().is_empty());
        assert!(!worker.exists());
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn worker_paths_are_remapped_without_touching_unrelated_text() {
        let isolated = Path::new(r"C:\Temp\ncx_worker_42_7");
        let live = Path::new(r"D:\projects\buglecat");
        let text = r"Updated C:\Temp\ncx_worker_42_7\src\app.rs and C:/Temp/ncx_worker_42_7/docs/readme.md. Keep ncx_worker_42_7 as plain text.";
        let remapped = remap_workspace_paths(text, isolated, live);
        assert!(remapped.contains(r"D:\projects\buglecat\src\app.rs"));
        assert!(remapped.contains("D:/projects/buglecat/docs/readme.md"));
        assert!(remapped.contains("Keep ncx_worker_42_7 as plain text"));
        assert!(!remapped.contains(r"C:\Temp\ncx_worker_42_7"));
    }

    #[cfg(windows)]
    #[test]
    fn worker_path_remapping_is_case_insensitive_on_windows() {
        let remapped = remap_workspace_paths(
            r"c:\temp\NCX_WORKER_42_7\src\app.rs",
            Path::new(r"C:\Temp\ncx_worker_42_7"),
            Path::new(r"D:\projects\buglecat"),
        );
        assert_eq!(remapped, r"D:\projects\buglecat\src\app.rs");
    }

    #[test]
    fn worker_activity_never_contains_tool_arguments_or_results() {
        let started = worker_activity(
            2,
            &LoopEvent::ToolStart {
                name: "shell".into(),
                args: r#"{"api_key":"secret-value"}"#.into(),
            },
        )
        .unwrap();
        assert_eq!(
            started,
            HarnessRunnerEvent::WorkerToolStarted {
                worker: 2,
                tool: "shell".into()
            }
        );
        assert!(!format!("{started:?}").contains("secret-value"));

        let finished = worker_activity(
            2,
            &LoopEvent::ToolResult {
                name: "shell".into(),
                result: "Error: token=secret-value".into(),
            },
        )
        .unwrap();
        assert_eq!(
            finished,
            HarnessRunnerEvent::WorkerToolFinished {
                worker: 2,
                tool: "shell".into(),
                failure: Some("execution".into())
            }
        );
        assert!(!format!("{finished:?}").contains("secret-value"));
    }
}
