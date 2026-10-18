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
use crate::workspace_promotion::{promote_with_paths, snapshot, WorkspaceSnapshot};
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

#[derive(Default, Clone, Debug)]
struct SubrunTelemetry {
    tools_used: Vec<String>,
    files_touched: Vec<String>,
    recalled_note_ids: Vec<String>,
    memory_rank_changed: Option<bool>,
    recovery_candidates: Vec<(String, String)>,
}

impl SubrunTelemetry {
    fn merge(&mut self, other: SubrunTelemetry) {
        self.tools_used.extend(other.tools_used);
        for f in other.files_touched {
            if !self.files_touched.contains(&f) {
                self.files_touched.push(f);
            }
        }
        for id in other.recalled_note_ids {
            if !self.recalled_note_ids.contains(&id) {
                self.recalled_note_ids.push(id);
            }
        }
        if let Some(changed) = other.memory_rank_changed {
            self.memory_rank_changed = Some(self.memory_rank_changed.unwrap_or(false) || changed);
        }
        for cand in other.recovery_candidates {
            if !self.recovery_candidates.contains(&cand) {
                self.recovery_candidates.push(cand);
            }
        }
    }
}

#[derive(Default, Clone, Copy, Debug)]
struct HumanInterventions {
    approvals_requested: u32,
    approvals_denied: u32,
    user_questions: u32,
}

pub struct HarnessAgentRunner {
    cfg: Config,
    memory: Rc<MemoryStore>,
    bindings: RuntimeHostBindings,
    cancel: Option<CancelCheck>,
    observer: Option<LoopObserver>,
    harness_profile: Option<String>,
    parent_task_id: Option<String>,
    /// Numbers worker/node outcomes under the parent task id.
    child_seq: std::cell::Cell<usize>,
    followup_resolved: std::cell::Cell<bool>,
    scratch: RefCell<HashMap<usize, WorkerWorkspace>>,
    worker_telemetry: RefCell<HashMap<usize, SubrunTelemetry>>,
    promoted_telemetry: RefCell<SubrunTelemetry>,
    interventions: RefCell<HumanInterventions>,
}

struct WorkerWorkspace {
    root: PathBuf,
    baseline: WorkspaceSnapshot,
}

impl HarnessAgentRunner {
    pub fn new(cfg: Config) -> Self {
        let mut mem = MemoryStore::new(cfg.workspace.join(".ncx").join("memory"));
        mem.set_feedback_mode(&cfg.feedback_memory);
        let memory = Rc::new(mem);
        let _ = memory.consolidate(0.85);
        Self {
            cfg,
            memory,
            bindings: RuntimeHostBindings::default(),
            cancel: None,
            observer: None,
            harness_profile: None,
            parent_task_id: None,
            child_seq: std::cell::Cell::new(0),
            followup_resolved: std::cell::Cell::new(false),
            scratch: RefCell::new(HashMap::new()),
            worker_telemetry: RefCell::new(HashMap::new()),
            promoted_telemetry: RefCell::new(SubrunTelemetry::default()),
            interventions: RefCell::new(HumanInterventions::default()),
        }
    }

    pub fn with_parent_task_id(mut self, parent_task_id: impl Into<String>) -> Self {
        self.parent_task_id = Some(parent_task_id.into());
        self
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

    /// Resolve the previous `Primary` turn's followup (`I5` manual edit > `I4`
    /// correction > `Accepted`) before an orchestrated turn mutates the live
    /// workspace. Idempotent per runner instance.
    pub fn resolve_previous_primary_followup(&self, session_id: &str, next_query: &str) {
        if self.followup_resolved.replace(true) {
            return;
        }
        let db = crate::outcome::workspace_events_db_path(&self.cfg.workspace, None);
        let Ok(store) = crate::context_store::ContextStore::open(db) else {
            return;
        };
        crate::outcome::resolve_previous_primary_followup(
            &store,
            &self.cfg.workspace,
            session_id,
            next_query,
            Some(crate::outcome::MemoryFeedback {
                store: &self.memory,
                mode: &self.cfg.feedback_memory,
            }),
        );
    }

    /// Load `RoutingStats` for the last 30 days from the workspace `events.sqlite3` store.
    pub fn load_routing_stats(&self) -> crate::outcome::RoutingStats {
        const ROUTING_WINDOW_MS: u64 = 30 * 86_400 * 1_000;
        let db = crate::outcome::workspace_events_db_path(&self.cfg.workspace, None);
        let Ok(store) = crate::context_store::ContextStore::open(db) else {
            return crate::outcome::RoutingStats::default();
        };
        let since = crate::outcome::now_millis().saturating_sub(ROUTING_WINDOW_MS);
        let Ok((outcomes, followups)) =
            crate::outcome::load_outcomes_and_followups(&store, Some(since))
        else {
            return crate::outcome::RoutingStats::default();
        };
        crate::outcome::RoutingStats::from_outcomes(&outcomes, &followups)
    }

    /// Record the parent `Primary` `TaskOutcome` for an orchestrated turn.
    /// `initial_complexity` is the classifier's choice before any routing
    /// feedback upgrade; statistics bucket on it.
    #[allow(clippy::too_many_arguments)]
    pub fn record_orchestrated_outcome(
        &self,
        session_id: &str,
        turn_id: u64,
        task: &str,
        outcome: &crate::OrchestratorOutcome,
        initial_complexity: Option<crate::Complexity>,
        shadow_recommended_route: Option<String>,
        duration_ms: u64,
    ) {
        self.resolve_previous_primary_followup(session_id, task);
        let db = crate::outcome::workspace_events_db_path(&self.cfg.workspace, None);
        let Ok(store) = crate::context_store::ContextStore::open(db) else {
            return;
        };
        let next_seq = crate::outcome::latest_primary_outcome(&store, session_id)
            .ok()
            .flatten()
            .map(|prev| prev.turn_id + 1)
            .unwrap_or(1);
        let effective_turn_id = turn_id.max(next_seq);
        let complexity_key = |c: crate::Complexity| match c {
            crate::Complexity::Simple => "Simple",
            crate::Complexity::Medium => "Medium",
            crate::Complexity::High => "Complex",
        };
        let complexity_str = complexity_key(outcome.complexity);
        let tier_config = match outcome.complexity {
            crate::Complexity::Simple => "single",
            crate::Complexity::Medium => "flash+pro",
            crate::Complexity::High => "dag",
        };
        let task_id = self
            .parent_task_id
            .clone()
            .unwrap_or_else(|| format!("{session_id}:orch-{effective_turn_id}"));
        let usage = crate::outcome::Usage {
            prompt_tokens: outcome
                .telemetry
                .usage
                .get("prompt_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
            completion_tokens: outcome
                .telemetry
                .usage
                .get("completion_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
            total_tokens: outcome
                .telemetry
                .usage
                .get("total_tokens")
                .copied()
                .unwrap_or(0)
                .max(0) as u64,
        };
        let stop_reason = if outcome.cancelled {
            "cancelled".to_string()
        } else if outcome.promotion_error.is_some() {
            "error".to_string()
        } else {
            "completed".to_string()
        };
        let terminal_state = if outcome.cancelled {
            "Cancelled".to_string()
        } else if outcome.promotion_error.is_some() {
            "Failed".to_string()
        } else {
            "Completed".to_string()
        };
        // The Simple route runs no verifier (0 rounds, verify_passed=true):
        // that is "skipped", not a verified pass.
        let verification = Some(
            match (outcome.verify_rounds, outcome.verify_passed) {
                (0, true) => "skipped",
                (_, true) => "passed",
                (_, false) => "failed",
            }
            .to_string(),
        );
        let promoted = self.promoted_telemetry.borrow().clone();
        let inter = *self.interventions.borrow();
        let file_hashes =
            crate::outcome::snapshot_file_hashes(&self.cfg.workspace, &promoted.files_touched);
        let has_deliverable = !promoted.files_touched.is_empty() || !promoted.tools_used.is_empty();
        let task_outcome = crate::outcome::TaskOutcome {
            task_id,
            parent_task_id: None,
            session_id: session_id.to_string(),
            turn_id: effective_turn_id,
            role: crate::outcome::TaskRole::Primary,
            authority: crate::agent_state::AuthoritySource::DirectHuman,
            category: crate::outcome::TaskCategory::classify(
                task,
                &promoted.tools_used,
                has_deliverable,
            ),
            query_hash: crate::outcome::hash_text(task),
            model: self.cfg.model.clone(),
            fast_model: (!self.cfg.fast_model.trim().is_empty())
                .then(|| self.cfg.fast_model.clone()),
            permission_mode: self.cfg.permission_mode.clone(),
            route: Some(crate::outcome::RouteKey {
                complexity: complexity_str.to_string(),
                tier_config: tier_config.to_string(),
                verify_passed: outcome.verify_passed,
                verify_rounds: outcome.verify_rounds as u32,
                shadow_recommended_route,
                initial_complexity: initial_complexity.map(|c| complexity_key(c).to_string()),
            }),
            terminal_state,
            stop_reason,
            verification,
            iterations: outcome.telemetry.calls,
            tool_calls: promoted.tools_used.len(),
            tools_used: promoted.tools_used,
            approvals_requested: inter.approvals_requested,
            approvals_denied: inter.approvals_denied,
            user_questions: inter.user_questions,
            recalled_note_ids: promoted.recalled_note_ids,
            files_touched: promoted.files_touched,
            file_hashes,
            workspace_revision: crate::agent_state::read_workspace_revision(&self.cfg.workspace),
            usage,
            duration_ms,
            timestamp_ms: crate::outcome::now_millis(),
            memory_rank_changed: promoted.memory_rank_changed,
        };
        let timestamp_ms = task_outcome.timestamp_ms;
        let _ = crate::outcome::append_task_outcome(&store, &task_outcome);
        if !outcome.cancelled
            && outcome.promotion_error.is_none()
            && outcome.verify_passed
            && !promoted.recovery_candidates.is_empty()
        {
            if let Ok(idx) = crate::memory_index::MemoryIndex::for_store(&self.memory) {
                let ws_key = self.memory.workspace_key();
                let ts_sec = timestamp_ms / 1_000;
                for (text, source_ref) in &promoted.recovery_candidates {
                    let _ = idx.record_candidate(
                        &ws_key,
                        text,
                        &["recovery".to_string()],
                        &crate::memory_index::Evidence::VerifiedToolResult {
                            source_ref: source_ref.clone(),
                        },
                        ts_sec,
                    );
                }
            }
        }
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
        let role = if with_tools {
            crate::outcome::TaskRole::OrchestratorWorker
        } else {
            crate::outcome::TaskRole::OrchestratorNode
        };
        let mut agent = runtime
            .profile()
            .clone()
            .apply(
                AgentLoop::from_runtime_services(tools, Session::new(assembled))
                    .expect("LLM factory service"),
            )
            .with_task_role(role, self.parent_task_id.clone());
        // Worker sessions have no durable log; give their outcomes the
        // workspace store and an id under the parent task so they are kept
        // out of the ACR denominator but stay linked for routing analysis.
        if let Some(parent) = &self.parent_task_id {
            let db = crate::outcome::workspace_events_db_path(&self.cfg.workspace, None);
            if let Ok(store) = crate::context_store::ContextStore::open(db) {
                let seq = self.child_seq.get() + 1;
                self.child_seq.set(seq);
                let label = match worker {
                    Some(index) => format!("worker-{}", index + 1),
                    None => "node".to_string(),
                };
                agent = agent.with_outcome_sink(Rc::new(store), format!("{parent}:{label}-{seq}"));
            }
        }
        agent.feedback_memory_mode = self.cfg.feedback_memory.clone();
        agent.feedback_routing_mode = self.cfg.feedback_routing.clone();
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
        let history = agent.state_machine.history();
        let approvals_requested = history
            .iter()
            .filter(|t| t.to == crate::agent_state::AgentState::AwaitingConfirmation)
            .count() as u32;
        let approvals_denied = history
            .iter()
            .filter(|t| {
                t.permission_decision.as_deref() == Some("denied")
                    || (t.from == crate::agent_state::AgentState::AwaitingConfirmation
                        && t.to == crate::agent_state::AgentState::Blocked)
            })
            .count() as u32;
        let user_questions = result
            .tools_used
            .iter()
            .filter(|t| t.as_str() == "ask_user_question")
            .count() as u32;
        {
            let mut inter = self.interventions.borrow_mut();
            inter.approvals_requested += approvals_requested;
            inter.approvals_denied += approvals_denied;
            inter.user_questions += user_questions;
        }
        let normalize_touched = |p: String| -> String {
            let path = Path::new(&p);
            if let Ok(rel) = path.strip_prefix(workspace) {
                return rel.to_string_lossy().replace('\\', "/");
            }
            p.replace('\\', "/")
        };
        let sub_telem = SubrunTelemetry {
            tools_used: result.tools_used.clone(),
            files_touched: agent
                .last_files_touched
                .into_iter()
                .map(normalize_touched)
                .collect(),
            recalled_note_ids: agent.last_recalled_note_ids,
            memory_rank_changed: agent.last_memory_rank_changed,
            recovery_candidates: agent
                .last_recovery_candidates
                .into_iter()
                .map(|(text, source_ref)| {
                    (
                        remap_workspace_paths(&text, workspace, &self.cfg.workspace),
                        source_ref,
                    )
                })
                .collect(),
        };
        if let Some(w_idx) = worker {
            self.worker_telemetry.borrow_mut().insert(w_idx, sub_telem);
        } else if with_tools {
            self.promoted_telemetry.borrow_mut().merge(sub_telem);
        }
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
        self.worker_telemetry.borrow_mut().remove(&index);
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
        let promoted_paths = self
            .scratch
            .borrow()
            .get(&index)
            .ok_or_else(|| format!("Worker {} 没有可提升的隔离工作区", index + 1))
            .and_then(|worker| {
                promote_with_paths(&worker.baseline, &worker.root, &self.cfg.workspace)
            });
        for (_, worker) in self.scratch.borrow_mut().drain() {
            let _ = std::fs::remove_dir_all(worker.root);
        }
        let promoted_paths = promoted_paths?;
        let mut promoted = self.promoted_telemetry.borrow_mut();
        if let Some(w_telem) = self.worker_telemetry.borrow_mut().remove(&index) {
            promoted.merge(w_telem);
        }
        for path in promoted_paths {
            if !promoted.files_touched.contains(&path) {
                promoted.files_touched.push(path);
            }
        }
        self.worker_telemetry.borrow_mut().clear();
        Ok(())
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
        let config = Config {
            workspace: crate::test_support::unique_temp_dir("ncx_missing_runner"),
            ..Default::default()
        };
        let _ = std::fs::remove_dir_all(&config.workspace);
        let runner = HarnessAgentRunner::new(config.clone());
        let error = runner.prepare_worker_workspace(0).unwrap_err();
        assert!(error.contains("isolation failed"), "{error}");
        assert!(runner.scratch.borrow().is_empty());
        assert!(!config.workspace.exists());
    }

    #[tokio::test]
    async fn runner_promotion_applies_deletions_and_cleans_all_scratch() {
        let workspace = crate::test_support::unique_temp_dir("ncx_runner_promotion");
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("remove.txt"), "old").unwrap();
        let config = Config {
            workspace: workspace.clone(),
            ..Default::default()
        };
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

    #[tokio::test]
    async fn orchestrated_outcome_aggregates_worker_promotion_and_resolves_followup() {
        use crate::context_store::ContextStore;
        use crate::outcome::{
            followups_for_task, load_outcomes_and_followups, FollowupSignal, TaskCategory,
        };
        use crate::{Complexity, OrchestratorOutcome, OrchestratorTelemetry};

        let workspace = crate::test_support::unique_temp_dir("ncx_orch_outcome");
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("lib.rs"), "pub fn v() -> i32 { 1 }\n").unwrap();

        let config = Config {
            workspace: workspace.clone(),
            ..Default::default()
        };
        let runner = HarnessAgentRunner::new(config);

        // Turn 1: Worker 0 edits lib.rs in isolated workspace and gets promoted;
        // Worker 1 is a losing worker whose recovery_candidates must NOT leak.
        let worker_dir = runner.prepare_worker_workspace(0).unwrap();
        let _worker_dir_1 = runner.prepare_worker_workspace(1).unwrap();
        std::fs::write(worker_dir.join("lib.rs"), "pub fn v() -> i32 { 2 }\n").unwrap();
        runner.worker_telemetry.borrow_mut().insert(
            0,
            SubrunTelemetry {
                tools_used: vec!["edit_file".to_string()],
                files_touched: vec!["lib.rs".to_string()],
                recalled_note_ids: vec!["note-abc".to_string()],
                memory_rank_changed: Some(true),
                recovery_candidates: vec![(
                    "winning worker recovery pattern".to_string(),
                    "turn1:w0".to_string(),
                )],
            },
        );
        runner.worker_telemetry.borrow_mut().insert(
            1,
            SubrunTelemetry {
                tools_used: vec!["shell".to_string()],
                files_touched: Vec::new(),
                recalled_note_ids: Vec::new(),
                memory_rank_changed: None,
                recovery_candidates: vec![(
                    "losing worker noise candidate".to_string(),
                    "turn1:w1".to_string(),
                )],
            },
        );
        runner.promote_worker(0).await.unwrap();
        runner.interventions.borrow_mut().approvals_requested = 1;

        let orch_outcome = OrchestratorOutcome {
            final_text: "Updated v()".to_string(),
            plan: None,
            worker_results: Vec::new(),
            best_worker: 0,
            complexity: Complexity::Medium,
            verify_rounds: 1,
            verify_passed: true,
            cancelled: false,
            promotion_error: None,
            telemetry: OrchestratorTelemetry {
                calls: 2,
                ..Default::default()
            },
        };
        runner.record_orchestrated_outcome(
            "sess-orch",
            1,
            "新增 v() 返回值并在代码里加上注释说明",
            &orch_outcome,
            Some(Complexity::Simple),
            Some("Medium (shadow)".to_string()),
            400,
        );

        let idx = crate::memory_index::MemoryIndex::for_store(&runner.memory).unwrap();
        let cands = idx.candidate_notes(&runner.memory.workspace_key());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].text, "winning worker recovery pattern");

        // User manually edits lib.rs before Turn 2 -> should resolve Turn 1 as ManualEdit (I5).
        std::fs::write(workspace.join("lib.rs"), "pub fn v() -> i32 { 99 }\n").unwrap();
        let runner2 = HarnessAgentRunner::new(Config {
            workspace: workspace.clone(),
            ..Default::default()
        });
        runner2.resolve_previous_primary_followup("sess-orch", "继续优化");
        runner2.record_orchestrated_outcome(
            "sess-orch",
            1,
            "继续优化",
            &orch_outcome,
            Some(Complexity::Simple),
            None,
            300,
        );

        let db_path = crate::outcome::workspace_events_db_path(&workspace, None);
        let store = ContextStore::open(db_path).unwrap();
        let (outcomes, _) = load_outcomes_and_followups(&store, None).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].turn_id, 1);
        assert_eq!(outcomes[1].turn_id, 2);
        assert_eq!(outcomes[0].category, TaskCategory::Feature);
        assert_eq!(outcomes[0].files_touched, vec!["lib.rs".to_string()]);
        assert!(outcomes[0].file_hashes.contains_key("lib.rs"));
        assert_eq!(outcomes[0].tools_used, vec!["edit_file".to_string()]);
        assert_eq!(outcomes[0].tool_calls, 1);
        assert_eq!(outcomes[0].recalled_note_ids, vec!["note-abc".to_string()]);
        assert_eq!(outcomes[0].memory_rank_changed, Some(true));
        assert_eq!(outcomes[0].approvals_requested, 1);

        let f1 = followups_for_task(&store, &outcomes[0].task_id).unwrap();
        assert_eq!(f1.len(), 1);
        assert!(matches!(
            &f1[0].signal,
            FollowupSignal::ManualEdit { files } if files == &vec!["lib.rs".to_string()]
        ));

        let _ = std::fs::remove_dir_all(workspace);
    }
}
