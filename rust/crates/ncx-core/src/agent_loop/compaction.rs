use super::*;
use crate::session::estimate_tokens;

impl AgentLoop {
    /// Reserve 1/8 of the active model's window for output and tool growth.
    /// Missing metadata uses a conservative fallback, never the old character cap.
    pub(super) fn compaction_budget(&self) -> i64 {
        let window = self
            .resolved_context_window
            .unwrap_or(self.context_window.clamp(1, 32_768));
        (window - window / 8).max(1)
    }

    pub(super) async fn resolve_context_window(&mut self) {
        let resolved = self
            .active_provider()
            .context_window()
            .await
            .filter(|n| *n > 0);
        if self.resolved_context_window != resolved {
            self.context_usage.set(None);
        }
        self.resolved_context_window = resolved;
    }

    pub(super) fn should_compact(&self, notes: &[String], query: &str) -> bool {
        if !self.context_edit.enabled {
            return false;
        }
        if let Some((count, tokens)) = self.context_usage.get() {
            if count <= self.session.messages.len() {
                return tokens + estimate_tokens(&self.session.messages[count..])
                    > self.compaction_budget();
            }
        }
        let mut messages = self.session.for_model();
        messages.extend(
            notes
                .iter()
                .map(|note| json!({"role":"system", "content":note})),
        );
        let schemas = self.tools.schemas_for_query(query);
        messages.push(json!({"content": serde_json::to_string(&schemas).unwrap_or_default()}));
        estimate_tokens(&messages) > self.compaction_budget()
    }

    pub async fn compact_context(
        &mut self,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<(ContextEditStats, std::collections::BTreeMap<String, i64>), String> {
        self.resolve_context_window().await;
        let overhead = estimate_tokens(&[
            json!({"content":self.session.system}),
            json!({"content":serde_json::to_string(&self.tools.schemas_for_query("")).unwrap_or_default()}),
        ]);
        let available = self.compaction_budget().saturating_sub(overhead);
        if available <= 0 {
            return Err("系统提示与工具定义已占满上下文预算，请增大预算或减少工具。".into());
        }
        let plan = self
            .session
            .prepare_compaction(available, &self.tools.ctx.workspace)?;
        // A large history is summarized incrementally: each chunk is its own
        // bounded request, and the running checkpoint is carried forward. Only
        // the last response is committed, so a failure part-way through leaves
        // the live history untouched.
        let mut running = String::new();
        let mut usage = std::collections::BTreeMap::new();
        let mut last: Option<ncx_provider::ModelResponse> = None;
        for index in 0..plan.chunk_count() {
            let request = plan.chunk_request(index, &running);
            let response = self.summarize_chunk(&request, cancel).await?;
            for (key, value) in &response.usage {
                *usage.entry(key.clone()).or_insert(0) += value;
            }
            if response.finish_reason != "stop" || response.content.trim().is_empty() {
                return Err(format!(
                    "第 {}/{} 段摘要不完整（finish_reason={}），原始上下文已保留。",
                    index + 1,
                    plan.chunk_count(),
                    response.finish_reason
                ));
            }
            running = response.content.trim().to_string();
            last = Some(response);
        }
        let Some(response) = last else {
            return Err("没有可摘要的历史，原始上下文已保留。".into());
        };
        let stats = self
            .session
            .commit_compaction(plan, &response, &self.tools.ctx.workspace)?;
        self.context_usage.set(None);
        Ok((stats, usage))
    }

    async fn summarize_chunk(
        &self,
        request: &[Value],
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<ncx_provider::ModelResponse, String> {
        let call = self.active_provider().chat(request, &[], None);
        tokio::pin!(call);
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(90));
        tokio::pin!(timeout);
        loop {
            if cancel.is_some_and(|cancel| cancel()) {
                return Err("压缩已取消，原始上下文已保留。".into());
            }
            tokio::select! {
                response = &mut call => return Ok(response),
                _ = &mut timeout => return Err("压缩摘要超时，原始上下文已保留。".into()),
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }
}
