//! Real [`AgentRunner`] for the orchestrator: each node runs a one-shot
//! [`AgentLoop`] on the tier-appropriate model.
//!
//! `Main` → `cfg.model` (pro). `Fast` → `cfg.fast_model` (flash), falling back
//! to `cfg.model`. Each call builds a fresh session + tool registry.
//!
//! Parallel-write isolation: worker 0 (the synthesized "answer") runs in the
//! real workspace; workers 1..N run against a throwaway COPY so their writes
//! can't collide with worker 0's. See [`ncx_core::isolate`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use async_trait::async_trait;
use ncx_config::Config;
use ncx_core::isolate::copy_tree;
use ncx_core::{
    discover_skills, load_project_instructions, model_provider_from_config, skills_index_block,
    AgentLoop, AgentRunner, AgentRuntimeProfile, HarnessRuntimeBuilder, MemoryStore, Session,
    Summarizer, Tier, ToolContext, ToolRegistry,
};
use ncx_provider::DeepSeekProvider;
use ncx_sandbox::SandboxPolicy;
use serde_json::json;

pub struct LiveRunner {
    cfg: Config,
    memory: Rc<MemoryStore>,
    counter: Cell<u64>,
    /// Per-worker isolated scratch workspaces (kept until promote/cleanup).
    scratch: RefCell<HashMap<usize, PathBuf>>,
}

impl LiveRunner {
    pub fn new(cfg: Config) -> Self {
        let memory = Rc::new(MemoryStore::new(cfg.workspace.join(".ncx").join("memory")));
        let _ = memory.consolidate(0.85); // tidy near-dups at startup (idempotent)
        LiveRunner {
            cfg,
            memory,
            counter: Cell::new(0),
            scratch: RefCell::new(HashMap::new()),
        }
    }

    fn model_for(&self, tier: Tier) -> String {
        match tier {
            Tier::Main => self.cfg.model.clone(),
            Tier::Fast => {
                if self.cfg.fast_model.is_empty() {
                    self.cfg.model.clone()
                } else {
                    self.cfg.fast_model.clone()
                }
            }
        }
    }

    /// Run one node in a specific `workspace` (defaults to the real one in
    /// [`AgentRunner::run`]; an isolated copy for non-primary workers).
    ///
    /// `with_tools = false` builds a tool-less agent for reasoning nodes
    /// (classify/plan/decompose/verify) so the model can't start executing the
    /// task — it has no tools to call and must answer directly.
    async fn run_in(
        &self,
        workspace: &Path,
        tier: Tier,
        system: &str,
        task: &str,
        with_tools: bool,
    ) -> String {
        let provider = model_provider_from_config(&self.cfg, self.model_for(tier));
        let policy = SandboxPolicy::new(self.cfg.sandbox_mode.clone(), workspace)
            .with_network_access(self.cfg.network_access);
        let ctx = ToolContext::new(workspace.to_path_buf(), policy)
            .with_approval_policy(self.cfg.approval_policy.clone())
            .with_timeout(self.cfg.timeout_s as u64)
            .with_search(
                self.cfg.search_provider.clone(),
                self.cfg.search_api_key.clone(),
            )
            .with_memory(self.memory.clone()) // memory is project-level, not per-copy
            .with_hooks(self.cfg.hooks.clone())
            .with_skills(discover_skills(workspace));
        let skills_index = skills_index_block(&discover_skills(workspace));
        let tools = if with_tools {
            HarnessRuntimeBuilder::default().build(ctx)
        } else {
            ToolRegistry::empty(ctx)
        };
        let instructions = load_project_instructions(workspace, 16_000);
        let system = compose_system_prompt(system, &[instructions, skills_index]);
        let session = Session::new(system);
        let mut agent = AgentRuntimeProfile::from_legacy_permissions(&self.cfg)
            .apply(AgentLoop::new(Box::new(provider), tools, session));
        agent.run_turn(json!(task), None).await.final_text
    }

    /// A unique scratch dir for an isolated worker.
    fn scratch_dir(&self) -> PathBuf {
        let n = self.counter.get() + 1;
        self.counter.set(n);
        std::env::temp_dir().join(format!("ncx_worker_{}_{n}", std::process::id()))
    }
}

#[async_trait(?Send)]
impl AgentRunner for LiveRunner {
    async fn run(&self, tier: Tier, system: &str, task: &str) -> String {
        let ws = self.cfg.workspace.clone();
        self.run_in(&ws, tier, system, task, true).await
    }

    async fn reason(&self, tier: Tier, system: &str, task: &str) -> String {
        // Tool-less: classify/plan/decompose/verify reason over the task text,
        // they don't touch the workspace.
        let ws = self.cfg.workspace.clone();
        self.run_in(&ws, tier, system, task, false).await
    }

    async fn run_worker(&self, idx: usize, _n: usize, system: &str, task: &str) -> String {
        // Every worker runs against its OWN copy of the workspace, so parallel
        // writes never collide. The copy is kept until `promote_worker` syncs the
        // verifier-chosen winner back to the real workspace.
        let scratch = self.scratch_dir();
        let ws = {
            // Scope the borrow so it's released before the await below.
            let prev = self.scratch.borrow_mut().insert(idx, scratch.clone());
            if let Some(old) = prev {
                let _ = std::fs::remove_dir_all(old); // drop the prior round's copy
            }
            match copy_tree(&self.cfg.workspace, &scratch) {
                Ok(_) => scratch,
                Err(_) => {
                    self.scratch.borrow_mut().remove(&idx);
                    self.cfg.workspace.clone() // fallback: real ws (no isolation)
                }
            }
        };
        self.run_in(&ws, Tier::Fast, system, task, true).await
    }

    async fn promote_worker(&self, idx: usize) {
        // Sync the winning worker's workspace onto the real one, then clean up
        // every scratch copy from this round.
        if let Some(dir) = self.scratch.borrow().get(&idx).cloned() {
            let _ = copy_tree(&dir, &self.cfg.workspace);
        }
        for (_, dir) in self.scratch.borrow_mut().drain() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// LLM-backed [`Summarizer`] for `MemoryStore::summarize_consolidate` — folds a
/// cluster of related notes into one concise note using the FAST model.
fn compose_system_prompt(base: &str, blocks: &[String]) -> String {
    let mut out = base.to_string();
    for block in blocks {
        if !block.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(block.trim());
        }
    }
    out
}

pub struct LiveSummarizer {
    cfg: Config,
}

impl LiveSummarizer {
    pub fn new(cfg: Config) -> Self {
        LiveSummarizer { cfg }
    }
    fn fast_model(&self) -> String {
        if self.cfg.fast_model.is_empty() {
            self.cfg.model.clone()
        } else {
            self.cfg.fast_model.clone()
        }
    }
}

#[async_trait(?Send)]
impl Summarizer for LiveSummarizer {
    async fn merge(&self, facts: &[String]) -> Option<String> {
        let provider = DeepSeekProvider::with_opts(
            self.cfg.api_key.clone(),
            &self.cfg.base_url,
            self.fast_model(),
            self.cfg.timeout_s as u64,
            self.cfg.max_retries as u32,
        );
        let user = facts
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{}. {f}", i + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let messages = vec![
            json!({"role": "system", "content": "Merge these related project notes into ONE concise factual note (at most 2 sentences). Output ONLY the merged note — no preamble, no list, no quotes."}),
            json!({"role": "user", "content": user}),
        ];
        match provider.chat(&messages, None, None, None, None).await {
            Ok(r) if !r.content.trim().is_empty() => Some(r.content.trim().to_string()),
            _ => None,
        }
    }
}
