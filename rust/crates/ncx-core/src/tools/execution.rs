impl ToolRegistry {
    /// Run a tool by name. Unknown tool -> an error string for the model.
    pub async fn execute(&self, name: &str, args: &Value) -> String {
        self.execute_attempt(name, args).await
    }

    /// Execute with one conservative retry and argument-compatible read-only fallbacks.
    pub async fn execute_with_recovery(&self, name: &str, args: &Value) -> String {
        let first = self.execute_attempt(name, args).await;
        let Some(mut failure) = classify_tool_result(&first) else {
            return first;
        };
        if !self.call_is_read_only(name, args) {
            return first;
        }
        if name == "read_file" && failure == crate::tool_recovery::ToolFailureClass::NotFound {
            if let Some((resolved, recovered_args)) =
                resolve_unique_missing_read(&self.ctx.workspace, args)
            {
                let recovered = self.execute_attempt(name, &recovered_args).await;
                if classify_tool_result(&recovered).is_none() {
                    return format!(
                        "[recovery: recursively resolved missing file to {resolved}]\n{recovered}"
                    );
                }
            }
        }
        let mut latest = first.clone();
        if failure.retryable() {
            latest = self.execute_attempt(name, args).await;
            let Some(retry_failure) = classify_tool_result(&latest) else {
                return format!("[recovery: retried {name} after {failure}]\n{latest}");
            };
            failure = retry_failure;
        }
        if let Some((fallback_name, fallback_args)) = fallback_call(name, args, failure) {
            if self.call_is_read_only(fallback_name, &fallback_args) {
                let fallback = self.execute_attempt(fallback_name, &fallback_args).await;
                if classify_tool_result(&fallback).is_none() {
                    return format!(
                        "[recovery: {name} -> {fallback_name} after {failure}]\n{fallback}"
                    );
                }
                return format!("Error: {name} failed ({failure}); fallback {fallback_name} also failed.\nprimary: {first}\nfallback: {fallback}");
            }
        }
        latest
    }

    async fn execute_attempt(&self, name: &str, args: &Value) -> String {
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
