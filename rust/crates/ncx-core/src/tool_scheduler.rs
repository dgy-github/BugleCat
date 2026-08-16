//! Pluggable execution policy for cancellable and bounded tool scheduling.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{FuturesUnordered, StreamExt};
use ncx_provider::ToolCall;

use crate::tools::ToolRegistry;

/// Scheduling boundary used by the agent runtime after it has separated
/// read-only batches from serial calls.
///
/// Implementations must dispatch through [`ToolRegistry::execute`] so tool
/// middleware, hooks, approvals, and sandbox enforcement remain in effect.
#[async_trait(?Send)]
pub trait ToolScheduler {
    /// Execute one serial tool call with cooperative cancellation.
    async fn execute_one(
        &self,
        tools: &ToolRegistry,
        call: &ToolCall,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> String;

    /// Execute a runtime-validated read-only batch and preserve model order.
    async fn execute_read_only_batch(
        &self,
        tools: &ToolRegistry,
        batch: &[&ToolCall],
        max_parallel: usize,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Vec<String>;
}

/// Default scheduler used by [`crate::AgentLoop`].
#[derive(Debug, Default)]
pub struct BoundedToolScheduler;

#[async_trait(?Send)]
impl ToolScheduler for BoundedToolScheduler {
    async fn execute_one(
        &self,
        tools: &ToolRegistry,
        call: &ToolCall,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> String {
        execute_cancellable(tools, call, cancel).await
    }

    async fn execute_read_only_batch(
        &self,
        tools: &ToolRegistry,
        batch: &[&ToolCall],
        max_parallel: usize,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Vec<String> {
        execute_bounded_read_only_batch(tools, batch, max_parallel, cancel).await
    }
}

async fn execute_cancellable(
    tools: &ToolRegistry,
    call: &ToolCall,
    cancel: Option<&dyn Fn() -> bool>,
) -> String {
    let future = tools.execute_with_recovery(&call.name, &call.arguments);
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            result = &mut future => return result,
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if cancel.is_some_and(|check| check()) {
                    return "[interrupted: stopped by user mid-command]".to_string();
                }
            }
        }
    }
}

async fn execute_bounded_read_only_batch(
    tools: &ToolRegistry,
    batch: &[&ToolCall],
    max_parallel: usize,
    cancel: Option<&dyn Fn() -> bool>,
) -> Vec<String> {
    let limit = max_parallel.min(batch.len()).max(1);
    let mut pending = FuturesUnordered::new();
    let mut results: Vec<Option<String>> = (0..batch.len()).map(|_| None).collect();
    let mut next = 0usize;

    while next < batch.len() || !pending.is_empty() {
        while next < batch.len() && pending.len() < limit {
            let index = next;
            let call = batch[index];
            pending.push(async move { (index, execute_cancellable(tools, call, cancel).await) });
            next += 1;
        }

        if let Some((index, result)) = pending.next().await {
            results[index] = Some(result);
        }
    }

    results
        .into_iter()
        .map(|result| result.expect("read-only tool pool result missing"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use async_trait::async_trait;
    use ncx_provider::ToolCall;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::{json, Value};

    use super::*;
    use crate::tools::{Tool, ToolContext};

    struct DelayedReadTool {
        active: Rc<Cell<usize>>,
        peak: Rc<Cell<usize>>,
    }

    #[async_trait(?Send)]
    impl Tool for DelayedReadTool {
        fn name(&self) -> &str {
            "delayed_read"
        }

        fn description(&self) -> &str {
            "Deterministic delayed read-only tool for scheduler tests."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn read_only(&self) -> bool {
            true
        }

        async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
            let active = self.active.get() + 1;
            self.active.set(active);
            self.peak.set(self.peak.get().max(active));
            let delay_ms = args["delay_ms"].as_u64().unwrap_or_default();
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            self.active.set(self.active.get().saturating_sub(1));
            args["result"].as_str().unwrap_or_default().to_string()
        }
    }

    #[tokio::test]
    async fn read_only_pool_is_bounded_and_preserves_model_order() {
        let workspace = std::env::temp_dir().join("ncx_bounded_read_pool");
        std::fs::create_dir_all(&workspace).unwrap();
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let active = Rc::new(Cell::new(0));
        let peak = Rc::new(Cell::new(0));
        let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
        tools.register(Box::new(DelayedReadTool {
            active: active.clone(),
            peak: peak.clone(),
        }));
        let calls: Vec<ToolCall> = (0..5)
            .map(|index| ToolCall {
                id: format!("c{index}"),
                name: "delayed_read".into(),
                arguments: json!({
                    "delay_ms": 25 - index * 5,
                    "result": format!("result-{index}"),
                }),
            })
            .collect();
        let batch: Vec<&ToolCall> = calls.iter().collect();

        let results = BoundedToolScheduler
            .execute_read_only_batch(&tools, &batch, 2, None)
            .await;

        assert_eq!(
            results,
            vec!["result-0", "result-1", "result-2", "result-3", "result-4"]
        );
        assert_eq!(active.get(), 0);
        assert!(peak.get() <= 2, "peak concurrency was {}", peak.get());
    }
}
