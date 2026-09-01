use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ncx_config::Config;
use ncx_core::{
    AgentRuntimeProfile, ConfiguredHarnessRuntime, MemoryStore, ProviderMemorySummarizer,
    Summarizer,
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMergeStatus {
    pub generation: u64,
    pub status: String,
    pub requested_model: Option<String>,
    pub removed: Option<usize>,
    pub error: Option<String>,
}

impl MemoryMergeStatus {
    fn idle() -> Self {
        Self {
            generation: 0,
            status: "idle".into(),
            requested_model: None,
            removed: None,
            error: None,
        }
    }
}

struct ActiveMemoryMerge {
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

struct MemoryMergeState {
    status: MemoryMergeStatus,
    active: Option<ActiveMemoryMerge>,
    /// The canonical workspace that owns the current status projection. It is
    /// deliberately coordinator state rather than a UI hint: status and
    /// cancellation must not cross a process-wide workspace boundary.
    owner_workspace: Option<PathBuf>,
}

pub struct MemoryMergeCoordinator {
    state: Mutex<MemoryMergeState>,
}

impl Default for MemoryMergeCoordinator {
    fn default() -> Self {
        Self {
            state: Mutex::new(MemoryMergeState {
                status: MemoryMergeStatus::idle(),
                active: None,
                owner_workspace: None,
            }),
        }
    }
}

impl MemoryMergeCoordinator {
    pub fn start(
        self: &Arc<Self>,
        cfg: Config,
        workspace: PathBuf,
    ) -> Result<MemoryMergeStatus, String> {
        let workspace = canonical_workspace(&workspace)?;
        let model = selected_merge_model(&cfg);
        let (generation, cancelled) = {
            let mut state = self.lock()?;
            if state.active.is_some()
                || matches!(state.status.status.as_str(), "running" | "cancelling")
            {
                return Err("项目记忆正在由模型整理".into());
            }
            state.status.generation = state.status.generation.saturating_add(1);
            state.status.status = "running".into();
            state.status.requested_model = Some(model.clone());
            state.status.removed = None;
            state.status.error = None;
            state.owner_workspace = Some(workspace.clone());
            let cancelled = Arc::new(AtomicBool::new(false));
            let generation = state.status.generation;
            state.active = Some(ActiveMemoryMerge {
                generation,
                cancelled: cancelled.clone(),
            });
            (generation, cancelled)
        };
        let coordinator = self.clone();
        let thread_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name(format!("ncx-memory-merge-{generation}"))
            .spawn(move || {
                run_merge(
                    coordinator,
                    generation,
                    cfg,
                    workspace,
                    model,
                    thread_cancelled,
                )
            })
            .map_err(|error| {
                self.finish(
                    generation,
                    &cancelled,
                    "failed",
                    None,
                    Some("无法启动记忆整理任务".into()),
                );
                error.to_string()
            })?;
        self.status()
    }

    pub fn status(&self) -> Result<MemoryMergeStatus, String> {
        Ok(self.lock()?.status.clone())
    }

    /// Return only the status projection owned by `workspace`. A caller that
    /// switched projects, or a poller superseded by a newer generation, sees
    /// the neutral idle projection rather than another project's job details.
    pub fn status_for_workspace(
        &self,
        workspace: &Path,
        expected_generation: Option<u64>,
    ) -> Result<MemoryMergeStatus, String> {
        let state = self.lock()?;
        if !owns_workspace(&state, workspace)
            || expected_generation.is_some_and(|generation| generation != state.status.generation)
        {
            return Ok(MemoryMergeStatus::idle());
        }
        Ok(state.status.clone())
    }

    pub fn cancel(&self) -> Result<MemoryMergeStatus, String> {
        let mut state = self.lock()?;
        if state.status.status == "running" {
            if let Some(active) = &state.active {
                active.cancelled.store(true, Ordering::SeqCst);
                state.status.status = "cancelling".into();
            }
        }
        Ok(state.status.clone())
    }

    /// Cancel only the generation the caller actually observed, and only from
    /// its owning workspace. This makes delayed cancel requests fail closed
    /// after either a workspace transition or a same-workspace replacement.
    pub fn cancel_for_workspace(
        &self,
        workspace: &Path,
        expected_generation: u64,
    ) -> Result<MemoryMergeStatus, String> {
        let mut state = self.lock()?;
        if !owns_workspace(&state, workspace) {
            return Err("当前项目没有可取消的模型记忆整理任务".to_string());
        }
        if state.status.generation != expected_generation {
            return Err("模型记忆整理任务已被更新，拒绝取消旧任务".to_string());
        }
        if state.status.status == "running" {
            if let Some(active) = &state.active {
                active.cancelled.store(true, Ordering::SeqCst);
                state.status.status = "cancelling".into();
            }
        }
        Ok(state.status.clone())
    }

    /// A workspace change is also a cancellation boundary: the old workspace's
    /// prepared draft must not be promoted after the process CWD changes.
    pub fn cancel_for_workspace_switch(&self) -> Result<MemoryMergeStatus, String> {
        self.cancel()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemoryMergeState>, String> {
        self.state
            .lock()
            .map_err(|_| "项目记忆整理状态不可用".to_string())
    }

    fn finish(
        &self,
        generation: u64,
        cancelled: &Arc<AtomicBool>,
        status: &str,
        removed: Option<usize>,
        error: Option<String>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            if owns_active_job(&state, generation, cancelled) {
                state.status.status = status.into();
                state.status.removed = removed;
                state.status.error = error;
                state.active = None;
            }
        }
    }

    /// The state mutex is a commit fence. Cancellation and workspace changes
    /// acquire the same mutex, so once either one wins, a prepared old draft
    /// cannot be written later. A successful write becomes terminal while the
    /// fence is still held, so a later cancel reports completion rather than a
    /// misleading cancellation after files were already changed.
    fn commit_if_current(
        &self,
        generation: u64,
        cancelled: &Arc<AtomicBool>,
        commit: impl FnOnce() -> std::io::Result<usize>,
    ) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("项目记忆整理状态不可用"))?;
        if state.status.status != "running"
            || cancelled.load(Ordering::SeqCst)
            || !owns_active_job(&state, generation, cancelled)
        {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }

        let result = commit();
        if let Ok(removed) = result {
            state.status.status = "completed".into();
            state.status.removed = Some(removed);
            state.status.error = None;
            state.active = None;
        }
        result
    }
}

fn owns_active_job(state: &MemoryMergeState, generation: u64, cancelled: &Arc<AtomicBool>) -> bool {
    state.status.generation == generation
        && state.active.as_ref().is_some_and(|active| {
            active.generation == generation && Arc::ptr_eq(&active.cancelled, cancelled)
        })
}

fn canonical_workspace(workspace: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(workspace).map_err(|_| "当前工作区不存在".to_string())
}

fn owns_workspace(state: &MemoryMergeState, workspace: &Path) -> bool {
    let Some(owner) = &state.owner_workspace else {
        return false;
    };
    let Ok(workspace) = std::fs::canonicalize(workspace) else {
        return false;
    };
    let Ok(owner) = std::fs::canonicalize(owner) else {
        return false;
    };
    #[cfg(windows)]
    {
        owner
            .to_string_lossy()
            .eq_ignore_ascii_case(&workspace.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        owner == workspace
    }
}

fn selected_merge_model(cfg: &Config) -> String {
    if cfg.fast_model.is_empty() {
        cfg.model.clone()
    } else {
        cfg.fast_model.clone()
    }
}

fn run_merge(
    coordinator: Arc<MemoryMergeCoordinator>,
    generation: u64,
    cfg: Config,
    workspace: PathBuf,
    model: String,
    cancelled: Arc<AtomicBool>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            coordinator.finish(
                generation,
                &cancelled,
                "failed",
                None,
                Some("无法启动异步运行时".into()),
            );
            return;
        }
    };
    let outcome = runtime.block_on(async {
        let harness = ConfiguredHarnessRuntime::new(
            cfg.clone(),
            model,
            AgentRuntimeProfile::from_legacy_permissions(&cfg),
        );
        let merger = ProviderMemorySummarizer::new(harness.primary_provider());
        let cancellable = CancelAwareSummarizer {
            inner: &merger,
            cancelled: cancelled.clone(),
        };
        let store = MemoryStore::new(workspace.join(".ncx").join("memory"));
        let draft = store
            .prepare_summarize_consolidate_cancellable(&cancellable, 0.85, || {
                cancelled.load(Ordering::SeqCst)
            })
            .await?;
        if merger.failure_count() > 0 {
            return Err(std::io::Error::other("provider merge failed"));
        }
        if cancelled.load(Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        coordinator.commit_if_current(generation, &cancelled, || {
            store.commit_summarize_consolidate(draft)
        })
    });
    finish_outcome(&coordinator, generation, &cancelled, outcome);
}

fn finish_outcome(
    coordinator: &MemoryMergeCoordinator,
    generation: u64,
    cancelled: &Arc<AtomicBool>,
    outcome: std::io::Result<usize>,
) {
    match outcome {
        Ok(removed) => coordinator.finish(generation, cancelled, "completed", Some(removed), None),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            coordinator.finish(generation, cancelled, "cancelled", None, None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => coordinator.finish(
            generation,
            cancelled,
            "conflict",
            None,
            Some("整理期间项目记忆已变化，结果未写入".into()),
        ),
        Err(_) => coordinator.finish(
            generation,
            cancelled,
            "failed",
            None,
            Some("模型未能完成记忆整理，原文件未修改".into()),
        ),
    }
}

struct CancelAwareSummarizer<'a> {
    inner: &'a ProviderMemorySummarizer,
    cancelled: Arc<AtomicBool>,
}

#[async_trait(?Send)]
impl Summarizer for CancelAwareSummarizer<'_> {
    async fn merge(&self, facts: &[String]) -> Option<String> {
        tokio::select! {
            result = self.inner.merge(facts) => result,
            _ = wait_for_cancel(self.cancelled.clone()) => None,
        }
    }
}

async fn wait_for_cancel(cancelled: Arc<AtomicBool>) {
    while !cancelled.load(Ordering::SeqCst) {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ncx-memory-merge-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn running_job(
        generation: u64,
        workspace: PathBuf,
    ) -> (MemoryMergeCoordinator, Arc<AtomicBool>) {
        let coordinator = MemoryMergeCoordinator::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut state = coordinator.state.lock().unwrap();
        state.status.generation = generation;
        state.status.status = "running".into();
        state.owner_workspace = Some(workspace);
        state.active = Some(ActiveMemoryMerge {
            generation,
            cancelled: cancelled.clone(),
        });
        drop(state);
        (coordinator, cancelled)
    }

    #[test]
    fn cancellation_moves_a_running_job_to_cancelling() {
        let workspace = test_workspace("cancel");
        let (coordinator, cancelled) = running_job(7, workspace.clone());
        let status = coordinator.cancel().unwrap();
        assert_eq!(status.generation, 7);
        assert_eq!(status.status, "cancelling");
        assert!(cancelled.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn workspace_switch_cancellation_fences_a_prepared_merge_before_commit() {
        let workspace = test_workspace("switch");
        let (coordinator, cancelled) = running_job(11, workspace.clone());
        coordinator.cancel_for_workspace_switch().unwrap();

        let committed = AtomicBool::new(false);
        let error = coordinator
            .commit_if_current(11, &cancelled, || {
                committed.store(true, Ordering::SeqCst);
                Ok(1)
            })
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        assert!(!committed.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn conflict_status_is_safe_and_keeps_the_generation() {
        let workspace = test_workspace("conflict");
        let (coordinator, cancelled) = running_job(3, workspace.clone());
        finish_outcome(
            &coordinator,
            3,
            &cancelled,
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "secret third-party response",
            )),
        );
        let status = coordinator.status().unwrap();
        assert_eq!(status.status, "conflict");
        assert_eq!(status.generation, 3);
        assert!(!status.error.unwrap().contains("secret"));
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn workspace_and_generation_fence_status_and_cancel() {
        let owner = test_workspace("owner");
        let other = test_workspace("other");
        let (coordinator, cancelled) = running_job(11, owner.clone());

        let hidden = coordinator.status_for_workspace(&other, None).unwrap();
        assert_eq!(hidden.status, "idle");
        assert_eq!(hidden.generation, 0);
        assert!(coordinator.cancel_for_workspace(&other, 11).is_err());
        assert!(!cancelled.load(Ordering::SeqCst));

        let stale = coordinator.status_for_workspace(&owner, Some(10)).unwrap();
        assert_eq!(stale.status, "idle");
        assert!(coordinator.cancel_for_workspace(&owner, 10).is_err());
        assert!(!cancelled.load(Ordering::SeqCst));

        let status = coordinator.cancel_for_workspace(&owner, 11).unwrap();
        assert_eq!(status.status, "cancelling");
        assert!(cancelled.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(owner);
        let _ = std::fs::remove_dir_all(other);
    }
}
