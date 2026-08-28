use std::path::PathBuf;
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

pub struct MemoryMergeCoordinator {
    state: Mutex<MemoryMergeStatus>,
    cancelled: Arc<AtomicBool>,
}

impl Default for MemoryMergeCoordinator {
    fn default() -> Self {
        Self {
            state: Mutex::new(MemoryMergeStatus {
                generation: 0,
                status: "idle".into(),
                requested_model: None,
                removed: None,
                error: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl MemoryMergeCoordinator {
    pub fn start(
        self: &Arc<Self>,
        cfg: Config,
        workspace: PathBuf,
    ) -> Result<MemoryMergeStatus, String> {
        let model = selected_merge_model(&cfg);
        let generation = {
            let mut state = self.lock()?;
            if matches!(state.status.as_str(), "running" | "cancelling") {
                return Err("项目记忆正在由模型整理".into());
            }
            state.generation = state.generation.saturating_add(1);
            state.status = "running".into();
            state.requested_model = Some(model.clone());
            state.removed = None;
            state.error = None;
            self.cancelled.store(false, Ordering::SeqCst);
            state.generation
        };
        let coordinator = self.clone();
        std::thread::Builder::new()
            .name(format!("ncx-memory-merge-{generation}"))
            .spawn(move || run_merge(coordinator, generation, cfg, workspace, model))
            .map_err(|error| {
                self.finish(
                    generation,
                    "failed",
                    None,
                    Some("无法启动记忆整理任务".into()),
                );
                error.to_string()
            })?;
        self.status()
    }

    pub fn status(&self) -> Result<MemoryMergeStatus, String> {
        Ok(self.lock()?.clone())
    }

    pub fn cancel(&self) -> Result<MemoryMergeStatus, String> {
        let mut state = self.lock()?;
        if state.status == "running" {
            self.cancelled.store(true, Ordering::SeqCst);
            state.status = "cancelling".into();
        }
        Ok(state.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemoryMergeStatus>, String> {
        self.state
            .lock()
            .map_err(|_| "项目记忆整理状态不可用".to_string())
    }

    fn finish(&self, generation: u64, status: &str, removed: Option<usize>, error: Option<String>) {
        if let Ok(mut state) = self.state.lock() {
            if state.generation == generation {
                state.status = status.into();
                state.removed = removed;
                state.error = error;
            }
        }
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
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            coordinator.finish(
                generation,
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
            cancelled: coordinator.cancelled.clone(),
        };
        let store = MemoryStore::new(workspace.join(".ncx").join("memory"));
        let draft = store
            .prepare_summarize_consolidate_cancellable(&cancellable, 0.85, || {
                coordinator.cancelled.load(Ordering::SeqCst)
            })
            .await?;
        if merger.failure_count() > 0 {
            return Err(std::io::Error::other("provider merge failed"));
        }
        if coordinator.cancelled.load(Ordering::SeqCst) {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        store.commit_summarize_consolidate(draft)
    });
    finish_outcome(&coordinator, generation, outcome);
}

fn finish_outcome(
    coordinator: &MemoryMergeCoordinator,
    generation: u64,
    outcome: std::io::Result<usize>,
) {
    match outcome {
        Ok(removed) => coordinator.finish(generation, "completed", Some(removed), None),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
            coordinator.finish(generation, "cancelled", None, None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => coordinator.finish(
            generation,
            "conflict",
            None,
            Some("整理期间项目记忆已变化，结果未写入".into()),
        ),
        Err(_) => coordinator.finish(
            generation,
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

    #[test]
    fn cancellation_moves_a_running_job_to_cancelling() {
        let coordinator = MemoryMergeCoordinator::default();
        {
            let mut state = coordinator.state.lock().unwrap();
            state.generation = 7;
            state.status = "running".into();
        }
        let status = coordinator.cancel().unwrap();
        assert_eq!(status.generation, 7);
        assert_eq!(status.status, "cancelling");
        assert!(coordinator.cancelled.load(Ordering::SeqCst));
    }

    #[test]
    fn conflict_status_is_safe_and_keeps_the_generation() {
        let coordinator = MemoryMergeCoordinator::default();
        coordinator.state.lock().unwrap().generation = 3;
        finish_outcome(
            &coordinator,
            3,
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "secret third-party response",
            )),
        );
        let status = coordinator.status().unwrap();
        assert_eq!(status.status, "conflict");
        assert_eq!(status.generation, 3);
        assert!(!status.error.unwrap().contains("secret"));
    }
}
