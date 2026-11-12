use super::*;
use crate::session::estimate_tokens;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Stats and summed model usage of one compaction run.
pub type CompactionOutcome = Result<(ContextEditStats, BTreeMap<String, i64>), String>;

/// A replaceable context-compaction implementation.
///
/// Installed by a Harness plugin as the `compaction` service; the agent loop
/// owns only *when* compaction is considered (between model calls, wrapped by
/// PreCompact/PostCompact hooks) and delegates both the trigger decision and
/// the history rewrite to the installed strategy. Profiles or overlays swap
/// strategies by replacing the plugin, without touching the loop.
#[async_trait::async_trait(?Send)]
pub trait CompactionStrategy {
    /// Stable identifier, reported to hooks and diagnostics.
    fn id(&self) -> &str;

    /// Whether this strategy consumes LLM calls during [`CompactionStrategy::compact`].
    fn uses_model(&self) -> bool {
        true
    }

    /// Whether the next model request should be preceded by compaction.
    fn should_compact(&self, agent: &AgentLoop, notes: &[String], query: &str) -> bool {
        agent.exceeds_compaction_budget(notes, query)
    }

    /// Rewrite the live session history. On `Err` the history must be unchanged.
    async fn compact(
        &self,
        agent: &mut AgentLoop,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> CompactionOutcome;
}

/// Default: incremental model-written summary with safety snapshot (`ncx.compaction`).
pub struct SummaryCompaction;

#[async_trait::async_trait(?Send)]
impl CompactionStrategy for SummaryCompaction {
    fn id(&self) -> &str {
        "summary"
    }

    async fn compact(
        &self,
        agent: &mut AgentLoop,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> CompactionOutcome {
        agent.summarize_context(cancel).await
    }
}

/// Model-free alternative (`ncx.compaction.rule`): shortens old tool results
/// and drops the oldest turns per `ContextEditPolicy`. Costs no model call,
/// but keeps less of what was learned than a summary does.
pub struct RuleCompaction;

#[async_trait::async_trait(?Send)]
impl CompactionStrategy for RuleCompaction {
    fn id(&self) -> &str {
        "rule"
    }

    fn uses_model(&self) -> bool {
        false
    }

    fn should_compact(&self, agent: &AgentLoop, notes: &[String], query: &str) -> bool {
        if !agent.exceeds_compaction_budget(notes, query) {
            return false;
        }
        // A single user turn with no prior messages or tool outputs cannot be
        // reduced by rule-based dropping; skip rather than failing the turn.
        agent.session.messages.len() > 1
    }

    async fn compact(
        &self,
        agent: &mut AgentLoop,
        _cancel: Option<&dyn Fn() -> bool>,
    ) -> CompactionOutcome {
        let unresolved = agent.session.unresolved_tool_calls();
        if !unresolved.is_empty() {
            return Err(format!(
                "有 {} 个工具调用尚未记录结果（{}），未压缩；先确认这些调用的实际状态。",
                unresolved.len(),
                unresolved.join(", ")
            ));
        }
        let budget_tokens = agent.compaction_budget().max(1);
        let current_tokens = agent.estimated_request_tokens(&[], "").max(1);
        let base_chars = agent.session.estimated_body_chars();
        let scaled_max_chars = if current_tokens > budget_tokens {
            ((base_chars as u128 * budget_tokens as u128) / (current_tokens as u128))
                .clamp(1_024, agent.context_edit.max_chars as u128) as usize
        } else {
            agent.context_edit.max_chars
        };

        let mut policy = agent.context_edit.clone();
        policy.enabled = true;
        policy.max_chars = policy.max_chars.min(scaled_max_chars);

        let mut stats = agent.session.compact(&policy);
        if stats.compressed_tool_results == 0 && stats.dropped_messages == 0 {
            for keep_recent in [
                policy.keep_recent_messages.min(8),
                policy.keep_recent_messages.min(4),
                2,
                1,
            ] {
                if keep_recent >= policy.keep_recent_messages
                    && policy.max_tool_result_chars <= 1_200
                {
                    continue;
                }
                let mut tighter = policy.clone();
                tighter.keep_recent_messages = keep_recent;
                tighter.max_tool_result_chars = tighter.max_tool_result_chars.min(1_200);
                tighter.max_chars = tighter.max_chars.min(base_chars.saturating_sub(1).max(256));
                stats = agent.session.compact(&tighter);
                if stats.compressed_tool_results > 0 || stats.dropped_messages > 0 {
                    break;
                }
            }
        }
        if stats.compressed_tool_results == 0 && stats.dropped_messages == 0 {
            return Err("规则压缩没有可释放的内容，原始上下文已保留。".into());
        }
        agent.context_usage.set(None);
        Ok((stats, BTreeMap::new()))
    }
}

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

    /// The installed compaction strategy, if the profile includes one.
    pub fn compaction_strategy(&self) -> Option<Rc<dyn CompactionStrategy>> {
        self.tools
            .service::<crate::plugins::CompactionService>("compaction")
            .map(|service| service.strategy.clone())
    }

    /// Estimate tokens for the next model request without cloning the session transcript.
    pub(super) fn estimated_request_tokens(&self, notes: &[String], query: &str) -> i64 {
        let mut units = 0usize;
        if !self.session.system.is_empty() {
            units += self.session.system.chars().count() + 8;
        }
        for msg in &self.session.messages {
            units += crate::session::message_token_units(msg);
        }
        for note in notes {
            units += note.chars().count() + 8;
        }
        units += self.tools.schema_chars_for_query(query) + 8;
        (units / 2) as i64
    }

    /// Default trigger: estimated next request exceeds the model-window budget.
    pub fn exceeds_compaction_budget(&self, notes: &[String], query: &str) -> bool {
        if !self.context_edit.enabled {
            return false;
        }
        if let Some((count, tokens)) = self.context_usage.get() {
            if count <= self.session.messages.len() {
                return tokens + estimate_tokens(&self.session.messages[count..])
                    > self.compaction_budget();
            }
        }
        self.estimated_request_tokens(notes, query) > self.compaction_budget()
    }

    /// Compact with the installed strategy; without one, fall back to a summary
    /// so an explicit `/compact` still works in minimal runtimes.
    pub async fn compact_context(
        &mut self,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> CompactionOutcome {
        match self.compaction_strategy() {
            Some(strategy) => strategy.compact(self, cancel).await,
            None => self.summarize_context(cancel).await,
        }
    }

    /// Model-summary compaction used by [`SummaryCompaction`].
    pub async fn summarize_context(
        &mut self,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> CompactionOutcome {
        self.resolve_context_window().await;
        let overhead =
            ((self.session.system.chars().count() + 8 + self.tools.schema_chars_for_query("") + 8)
                / 2) as i64;
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
        let mut usage = BTreeMap::new();
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
                response = &mut call => {
                    self.last_compaction_calls.set(self.last_compaction_calls.get().saturating_add(1));
                    return Ok(response);
                }
                _ = &mut timeout => return Err("压缩摘要超时，原始上下文已保留。".into()),
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }
}
