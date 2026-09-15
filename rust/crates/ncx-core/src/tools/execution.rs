impl ToolRegistry {
    /// Run a tool by name. Unknown tool -> an error string for the model.
    pub async fn execute(&self, name: &str, args: &Value) -> String {
        self.execute_attempt(name, args).await
    }

    /// Execute with one conservative retry and a declared, argument-compatible
    /// substitute. Retry and substitution are both gated by the tool's
    /// [`ToolRuntimePolicy`] rather than by `read_only()` alone, and the whole
    /// chain is recorded so an apparent success can be traced back to the tool
    /// that actually produced it.
    pub async fn execute_with_recovery(&self, name: &str, args: &Value) -> String {
        // Fingerprint a mutating call's targets before it runs. A tool can fail
        // after writing part of its change, and that partial state is only
        // observable by comparing against this snapshot.
        let mutating = !self.call_is_read_only(name, args);
        let probe_paths = if mutating {
            crate::tool_recovery::implicated_paths(args)
        } else {
            Vec::new()
        };
        let before = mutating
            .then(|| crate::tool_recovery::fingerprint_paths(&self.ctx.workspace, &probe_paths));

        let first = self.execute_attempt(name, args).await;
        let Some(mut failure) = classify_tool_result(&first) else {
            return first;
        };
        let mut chain = RecoveryChain::new(name, failure);
        if !may_substitute(failure) {
            // Approval denial, sandbox refusal and user cancellation are
            // decisions, not transport faults. Routing around them would defeat
            // the boundary that produced them.
            self.record_recovery(chain.refused("failure class is not recoverable"));
            return first;
        }

        if name == "read_file" && failure == ToolFailureClass::NotFound {
            if let Some((resolved, recovered_args)) =
                resolve_unique_missing_read(&self.ctx.workspace, args)
            {
                let recovered = self.execute_attempt(name, &recovered_args).await;
                if classify_tool_result(&recovered).is_none() {
                    self.record_recovery(chain.resolved(&resolved));
                    return format!(
                        "[recovery: recursively resolved missing file to {resolved}]\n{recovered}"
                    );
                }
            }
        }

        let mut latest = first.clone();
        if self.retry_allowed(name, args, failure) {
            latest = self.execute_attempt(name, args).await;
            match classify_tool_result(&latest) {
                None => {
                    self.record_recovery(chain.retried_ok());
                    return format!("[recovery: retried {name} after {failure}]\n{latest}");
                }
                Some(retry_failure) => {
                    chain.retried_failed(retry_failure);
                    failure = retry_failure;
                    if !may_substitute(failure) {
                        self.record_recovery(chain.refused("retry failure is not recoverable"));
                        return latest;
                    }
                }
            }
        }

        let Some((fallback_name, fallback_args)) = self
            .get(name)
            .and_then(|tool| tool.fallback_route(args, failure))
        else {
            self.record_recovery(chain.refused("no declared substitute for this failure"));
            return latest;
        };
        if !self.substitute_allowed(&fallback_name, &fallback_args) {
            // A mutating substitute could double-apply a partially successful
            // write. Report the original failure instead of guessing.
            self.record_recovery(chain.refused_substitute(
                &fallback_name,
                "substitute is mutating and not idempotent",
            ));
            return latest;
        }
        // Before substituting for a failed write, check what that write already
        // did. Substituting after a partial effect would apply it twice, and an
        // unverifiable effect is treated the same way as a known partial one.
        if let Some(before) = &before {
            let after =
                crate::tool_recovery::fingerprint_paths(&self.ctx.workspace, &probe_paths);
            let effect = crate::tool_recovery::classify_write_effect(before, &after);
            self.record_write_probe(name, &effect);
            if effect != crate::tool_recovery::WriteEffect::None {
                let reason = format!("primary write left workspace state {effect}");
                self.record_recovery(chain.refused_substitute(&fallback_name, &reason));
                return format!(
                    "Error: {name} failed ({failure}) and its write is {effect}; no substitute was run. Re-read the affected paths before retrying.\n{latest}"
                );
            }
        }
        let fallback = self.execute_attempt(&fallback_name, &fallback_args).await;
        match classify_tool_result(&fallback) {
            None => {
                self.record_recovery(chain.substituted_ok(&fallback_name));
                format!("[recovery: {name} -> {fallback_name} after {failure}]\n{fallback}")
            }
            Some(class) => {
                self.record_recovery(chain.substituted_failed(&fallback_name, class));
                format!("Error: {name} failed ({failure}); fallback {fallback_name} also failed.\nprimary: {first}\nfallback: {fallback}")
            }
        }
    }

    /// One retry, only for transport-level faults, and only when replaying the
    /// call cannot double-apply an effect.
    fn retry_allowed(&self, name: &str, args: &Value, failure: ToolFailureClass) -> bool {
        if !failure.retryable() {
            return false;
        }
        let Some(policy) = self.runtime_policy(name) else {
            return false;
        };
        policy.retry_policy == crate::tools::ToolRetryPolicy::TransientOnce
            && (self.call_is_read_only(name, args) || policy.idempotent)
    }

    fn substitute_allowed(&self, name: &str, args: &Value) -> bool {
        self.call_is_read_only(name, args)
            || self
                .runtime_policy(name)
                .is_some_and(|policy| policy.idempotent)
    }

    fn record_write_probe(&self, tool: &str, effect: &crate::tool_recovery::WriteEffect) {
        if let Some(recorder) = &self.ctx.state_recorder {
            recorder.record_write_probe(tool, effect);
        }
    }

    fn record_recovery(&self, chain: &RecoveryChain) {
        let Some(store) = self.recovery_trace.borrow().clone() else {
            return;
        };
        let text = chain.render();
        let _ = store.append(&crate::context_trace::SourceRecord {
            source_id: format!("recovery-{}-{}", chain.tool, crate::context_trace::content_hash(&text)),
            kind: crate::context_trace::SourceKind::ToolRecovery,
            tool_name: Some(chain.tool.clone()),
            status: Some(chain.outcome.clone()),
            content_hash: crate::context_trace::content_hash(&text),
            text,
            ..crate::context_trace::SourceRecord::default()
        });
    }
}

/// Failure classes that may never be routed around. A substitute that bypassed
/// an approval denial, a sandbox refusal or a user cancellation would turn a
/// deliberate boundary into a retry loop.
fn may_substitute(failure: ToolFailureClass) -> bool {
    !matches!(
        failure,
        ToolFailureClass::PermissionDenied | ToolFailureClass::Cancelled
    )
}

/// Structured record of one recovery attempt sequence.
struct RecoveryChain {
    tool: String,
    steps: Vec<String>,
    outcome: String,
}

impl RecoveryChain {
    fn new(tool: &str, failure: ToolFailureClass) -> Self {
        Self {
            tool: tool.to_string(),
            steps: vec![format!("{tool} failed: {failure}")],
            outcome: "unresolved".into(),
        }
    }

    fn finish(&mut self, step: String, outcome: &str) -> &Self {
        self.steps.push(step);
        self.outcome = outcome.to_string();
        self
    }

    fn refused(&mut self, reason: &str) -> &Self {
        self.finish(format!("refused: {reason}"), "refused")
    }

    fn refused_substitute(&mut self, candidate: &str, reason: &str) -> &Self {
        self.finish(format!("refused {candidate}: {reason}"), "refused")
    }

    fn resolved(&mut self, target: &str) -> &Self {
        self.finish(format!("resolved unique target {target}"), "recovered")
    }

    fn retried_ok(&mut self) -> &Self {
        self.finish("retry succeeded".into(), "recovered")
    }

    fn retried_failed(&mut self, failure: ToolFailureClass) -> &Self {
        self.steps.push(format!("retry failed: {failure}"));
        self
    }

    fn substituted_ok(&mut self, candidate: &str) -> &Self {
        self.finish(format!("substitute {candidate} succeeded"), "substituted")
    }

    fn substituted_failed(&mut self, candidate: &str, failure: ToolFailureClass) -> &Self {
        self.finish(format!("substitute {candidate} failed: {failure}"), "failed")
    }

    fn render(&self) -> String {
        format!("{} | {}", self.steps.join(" -> "), self.outcome)
    }
}

impl ToolRegistry {

    async fn execute_attempt(&self, name: &str, args: &Value) -> String {
        if let Some(blocked) = self.check_runtime_policy(name, args) {
            return blocked;
        }
        if self.ctx.compaction_read_only_recovery.get() && !self.call_is_read_only(name, args) {
            return format!("Error: {name} blocked: context compaction consistency check entered read-only recovery. Re-read the workspace, git diff, tests, and latest valid decision before any write.");
        }
        match self.get(name) {
            Some(tool) => {
                let context = self.effective_context();
                let (entered, blocked) = self.enter_middleware(&context, name, args).await;
                let result = match blocked {
                    Some(result) => result,
                    None => self.execute_with_hooks(&context, tool, name, args).await,
                };
                self.leave_middleware(&context, entered, name, args, result)
                    .await
            }
            None => format!("Error: unknown tool '{name}'."),
        }
    }

    /// Central ToolRuntime gate. It runs before middleware, hooks, approval,
    /// and the tool body, so every retry and fallback is accounted for.
    fn check_runtime_policy(&self, name: &str, args: &Value) -> Option<String> {
        let tool = self.get(name)?;
        let policy = tool.runtime_policy();
        let key = (name.to_string(), canonical_json(args));
        let mut state = self.runtime_state.borrow_mut();
        let turn_count = state.turn_calls.get(name).copied().unwrap_or_default();
        if policy
            .max_calls_per_turn
            .is_some_and(|limit| turn_count >= limit)
        {
            return Some(format!(
                "Error: {name} blocked by ToolRuntime: per-turn call limit reached."
            ));
        }
        let task_count = state.task_calls.get(name).copied().unwrap_or_default();
        if policy
            .max_calls_per_task
            .is_some_and(|limit| task_count >= limit)
        {
            return Some(format!(
                "Error: {name} blocked by ToolRuntime: per-task call limit reached."
            ));
        }
        if policy.duplicate_policy == crate::tools::ToolDuplicatePolicy::Reject
            && state.seen.contains(&key)
        {
            return Some(format!(
                "Error: {name} blocked by ToolRuntime: duplicate invocation rejected."
            ));
        }
        *state.turn_calls.entry(name.to_string()).or_default() += 1;
        *state.task_calls.entry(name.to_string()).or_default() += 1;
        state.seen.insert(key);
        None
    }

    fn effective_context(&self) -> ToolContext {
        let mut context = self.ctx.clone();
        if let Some(policy) = self.service::<crate::plugins::PolicyService>("policy") {
            context.policy = policy.sandbox.clone();
            context.approval_policy = policy.approval_policy.clone();
            context.plan_mode = policy.plan_mode;
        }
        if let Some(interaction) = self.service::<crate::plugins::InteractionService>("interaction")
        {
            context.approver = interaction.approver.clone();
        }
        // Wrap last, so whichever approver won above is the one whose decision
        // is recorded. The wrapper observes, it never changes the decision.
        if let (Some(approver), Some(recorder)) =
            (context.approver.clone(), context.state_recorder.clone())
        {
            context.approver = Some(Rc::new(crate::agent_state::StateTrackingApprover::new(
                approver, recorder,
            )));
        }
        context
    }

    async fn enter_middleware(
        &self,
        context: &ToolContext,
        name: &str,
        args: &Value,
    ) -> (usize, Option<String>) {
        for (index, middleware) in self.middleware.iter().enumerate() {
            match middleware.before_execute(context, name, args).await {
                ToolMiddlewareDecision::Continue => {}
                ToolMiddlewareDecision::Block { reason } => {
                    return (
                        index + 1,
                        Some(format!(
                            "Error: {name} blocked by tool middleware '{}': {reason}",
                            middleware.name()
                        )),
                    )
                }
            }
        }
        (self.middleware.len(), None)
    }

    async fn leave_middleware(
        &self,
        context: &ToolContext,
        entered: usize,
        name: &str,
        args: &Value,
        mut result: String,
    ) -> String {
        for middleware in self.middleware[..entered].iter().rev() {
            if let Some(replacement) = middleware.after_execute(context, name, args, &result).await
            {
                result = replacement;
            }
        }
        result
    }

    async fn execute_with_hooks(
        &self,
        context: &ToolContext,
        tool: &dyn Tool,
        name: &str,
        args: &Value,
    ) -> String {
        let pre = run_matching_hooks(
            &context.hooks,
            HookEvent::PreTool,
            name,
            args,
            None,
            &context.workspace,
        )
        .await;
        if pre.blocked {
            return format!("Error: {name} blocked by pre_tool hook.\n{}", pre.notes);
        }
        let mut result = tool.execute(context, args).await;
        let post = run_matching_hooks(
            &context.hooks,
            HookEvent::PostTool,
            name,
            args,
            Some(&result),
            &context.workspace,
        )
        .await;
        let hook_notes = [pre.notes, post.notes]
            .into_iter()
            .filter(|note| !note.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !hook_notes.is_empty() {
            result.push_str("\n\n[hook output]\n");
            result.push_str(&hook_notes);
        }
        result
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let body = entries
                .into_iter()
                .map(|(key, value)| format!("{}:{}", serde_json::to_string(key).unwrap(), canonical_json(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical_json).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod runtime_tests {
    use async_trait::async_trait;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::{json, Value};
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::tools::{Tool, ToolContext, ToolDuplicatePolicy, ToolRegistry, ToolRuntimePolicy};

    struct CountingTool {
        calls: Rc<Cell<usize>>,
    }

    #[async_trait(?Send)]
    impl Tool for CountingTool {
        fn name(&self) -> &str { "counting" }
        fn description(&self) -> &str { "runtime policy test tool" }
        fn parameters(&self) -> Value { json!({"type":"object"}) }
        fn read_only(&self) -> bool { true }
        fn runtime_policy(&self) -> ToolRuntimePolicy {
            ToolRuntimePolicy {
                max_calls_per_turn: Some(2),
                max_calls_per_task: Some(3),
                duplicate_policy: ToolDuplicatePolicy::Reject,
                ..ToolRuntimePolicy::default()
            }
        }
        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            self.calls.set(self.calls.get() + 1);
            "ok".into()
        }
    }

    #[tokio::test]
    async fn runtime_gate_counts_limits_and_normalizes_object_order() {
        let workspace = crate::test_support::unique_temp_dir("tool_runtime_gate");
        std::fs::create_dir_all(&workspace).unwrap();
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let calls = Rc::new(Cell::new(0));
        let mut registry = ToolRegistry::empty(ToolContext::new(workspace, policy));
        registry.register(Box::new(CountingTool { calls: calls.clone() }));
        registry.begin_task(1);

        assert_eq!(registry.execute("counting", &json!({"a":1,"b":2})).await, "ok");
        let duplicate = registry.execute("counting", &json!({"b":2,"a":1})).await;
        assert!(duplicate.contains("duplicate invocation rejected"));
        assert_eq!(registry.execute("counting", &json!({"a":3})).await, "ok");
        let limited = registry.execute("counting", &json!({"a":4})).await;
        assert!(limited.contains("per-turn call limit reached"));
        assert_eq!(calls.get(), 2);
    }
}
