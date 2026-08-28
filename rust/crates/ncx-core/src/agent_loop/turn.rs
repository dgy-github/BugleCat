//! One-turn state machine and tool-call orchestration.

use std::collections::BTreeMap;
use std::time::Duration;

use ncx_provider::ModelResponse;
use serde_json::{json, Value};

use super::deliverable::DeliverableRequirement;
use super::tool_dispatch::{self, DispatchStop};
use super::{dump_args, emit, trace, AgentLoop, EventSink, LoopEvent, TurnResult};
use crate::hooks::{run_matching_hooks, HookEvent};
use crate::turn_context::TurnContextRequest;

const MEMORY_RECALL_MAX_ENTRIES: usize = 8;
const MEMORY_RECALL_MAX_CHARS: usize = 4_000;
const MAX_CONSECUTIVE_EMPTY_RESPONSES: usize = 3;
const MAX_CONSECUTIVE_TRANSPORT_ERRORS: usize = 3;
const SOFT_CONVERGENCE_TOOL_CALLS: usize = 32;
const HARD_CONVERGENCE_TOOL_CALLS: usize = 40;
const HARD_CONVERGENCE_FAILURES: usize = 6;

#[derive(Default)]
struct TurnState {
    tools_used: Vec<String>,
    tool_failures: usize,
    deliverable_finish_rejections: usize,
    usage: BTreeMap<String, i64>,
    consecutive_empty_responses: usize,
    consecutive_transport_errors: usize,
}

impl TurnState {
    fn finish(self, text: String, iterations: usize, stop_reason: &str) -> TurnResult {
        TurnResult {
            final_text: text,
            iterations,
            stop_reason: stop_reason.into(),
            tools_used: self.tools_used,
            usage: self.usage,
        }
    }
}

struct PromptContext {
    tool_query: String,
    runtime_notes: Vec<String>,
    deliverable: Option<DeliverableRequirement>,
}

pub(super) async fn run(
    agent: &mut AgentLoop,
    user_input: Value,
    cancel: Option<&dyn Fn() -> bool>,
    sink: &mut Option<EventSink>,
) -> TurnResult {
    let mut prompt = match prepare_prompt(agent, &user_input).await {
        Ok(prompt) => prompt,
        Err(result) => return result,
    };
    agent.session.add_user(user_input);

    let mut state = TurnState::default();
    let max_model_calls = agent
        .max_iterations
        .min(agent.task_budget.max_model_calls.max(1));

    for iteration in 0..max_model_calls {
        if is_cancelled(cancel) {
            return cancelled_result(agent, iteration + 1, state);
        }

        if agent
            .tools
            .service::<crate::plugins::CompactionServiceDescriptor>("compaction")
            .is_some()
            && agent.session.needs_compaction(&agent.context_edit)
        {
            let hook_args = json!({
                "max_chars": agent.context_edit.max_chars,
                "keep_recent_messages": agent.context_edit.keep_recent_messages,
                "max_tool_result_chars": agent.context_edit.max_tool_result_chars,
            });
            let pre = run_matching_hooks(
                &agent.tools.ctx.hooks,
                HookEvent::PreCompact,
                "compaction",
                &hook_args,
                None,
                &agent.tools.ctx.workspace,
            )
            .await;
            if !pre.notes.trim().is_empty() {
                prompt.runtime_notes.push(pre.notes);
            }
            if !pre.blocked {
                if let Some(safe) = agent
                    .session
                    .compact_safely_if_needed(&agent.context_edit, &agent.tools.ctx.workspace)
                {
                    let stats = safe.stats;
                    let rendered = format!(
                        "chars {} -> {}; compressed_tool_results={}; dropped_messages={}",
                        stats.original_chars,
                        stats.edited_chars,
                        stats.compressed_tool_results,
                        stats.dropped_messages
                    );
                    let post = run_matching_hooks(
                        &agent.tools.ctx.hooks,
                        HookEvent::PostCompact,
                        "compaction",
                        &hook_args,
                        Some(&rendered),
                        &agent.tools.ctx.workspace,
                    )
                    .await;
                    if !post.notes.trim().is_empty() {
                        prompt.runtime_notes.push(post.notes);
                    }
                    if !safe.conflicts.is_empty() {
                        agent.tools.ctx.compaction_read_only_recovery.set(true);
                        prompt.runtime_notes.push(format!(
                            "压缩一致性校验失败，已进入只读恢复模式：{}。重新读取工作区、git diff、测试结果和最近有效决策；证据不足时暂停并询问用户。",
                            safe.conflicts.join("；")
                        ));
                    }
                    emit(sink, LoopEvent::ContextCompacted(stats));
                }
            }
        }

        let response =
            match request_model_cancellable(agent, &prompt, iteration, &mut state, sink, cancel)
                .await
            {
                Some(response) => response,
                None => return cancelled_result(agent, iteration + 1, state),
            };
        if let Some((text, reason)) = finish_response(agent, &prompt, &response, &mut state, sink) {
            return state.finish(text, iteration + 1, reason);
        }

        if !response.has_tool_calls() {
            continue;
        }
        persist_tool_calls(agent, &response);
        if let Some(stop) = tool_dispatch::execute(
            agent,
            &response.tool_calls,
            &mut state.tools_used,
            &mut state.tool_failures,
            cancel,
            sink,
        )
        .await
        {
            return stop_turn(agent, stop, iteration, state);
        }
    }

    model_budget_result(agent, max_model_calls, state)
}

async fn request_model_cancellable(
    agent: &AgentLoop,
    prompt: &PromptContext,
    iteration: usize,
    state: &mut TurnState,
    sink: &mut Option<EventSink>,
    cancel: Option<&dyn Fn() -> bool>,
) -> Option<ModelResponse> {
    let request = request_model(agent, prompt, iteration, state, sink);
    tokio::pin!(request);
    loop {
        tokio::select! {
            response = &mut request => return Some(response),
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if is_cancelled(cancel) {
                    return None;
                }
            }
        }
    }
}

fn cancelled_result(agent: &mut AgentLoop, iterations: usize, state: TurnState) -> TurnResult {
    retire_active_plan(agent);
    let text = "Stopped by user.".to_string();
    agent.session.add_assistant(&text, None, "");
    state.finish(text, iterations, "cancelled")
}

async fn prepare_prompt(
    agent: &mut AgentLoop,
    user_input: &Value,
) -> Result<PromptContext, TurnResult> {
    agent.use_vision_this_turn = agent.vision_provider.is_some() && has_image_block(user_input);
    let tool_query = user_query_text(user_input);
    let hook = run_matching_hooks(
        &agent.tools.ctx.hooks,
        HookEvent::UserPrompt,
        "user_prompt",
        &json!({"prompt": tool_query, "content": user_input.clone()}),
        None,
        &agent.tools.ctx.workspace,
    )
    .await;
    if hook.blocked {
        let text = format!("User prompt blocked by user_prompt hook.\n{}", hook.notes);
        agent.session.add_assistant(&text, None, "");
        return Err(TurnState::default().finish(text, 0, "blocked"));
    }

    let mut runtime_notes = Vec::new();
    if !hook.notes.trim().is_empty() {
        runtime_notes.push(format!("[user_prompt hook output]\n{}", hook.notes));
    }
    let context_request = TurnContextRequest {
        user_input: user_input.clone(),
        query: tool_query.clone(),
    };
    runtime_notes.extend(agent.turn_context.collect(&context_request).await);
    let memory = agent
        .tools
        .service::<crate::plugins::MemoryServiceDescriptor>("memory")
        .and_then(|service| service.store.clone());
    runtime_notes.extend(memory_recall_notes(
        &memory,
        &tool_query,
        MEMORY_RECALL_MAX_ENTRIES,
        MEMORY_RECALL_MAX_CHARS,
    ));
    Ok(PromptContext {
        deliverable: DeliverableRequirement::detect(&tool_query, &agent.tools.ctx.workspace),
        tool_query,
        runtime_notes,
    })
}

async fn request_model(
    agent: &AgentLoop,
    prompt: &PromptContext,
    iteration: usize,
    state: &mut TurnState,
    sink: &mut Option<EventSink>,
) -> ModelResponse {
    let deliverable_ready = prompt
        .deliverable
        .as_ref()
        .and_then(|requirement| requirement.completed_path(&agent.tools.ctx.workspace))
        .is_some();
    let force_convergence = state.tools_used.len() >= HARD_CONVERGENCE_TOOL_CALLS
        && state.tool_failures >= HARD_CONVERGENCE_FAILURES
        && (prompt.deliverable.is_none() || deliverable_ready)
        && !has_unfinished_plan(agent);
    let schemas = if force_convergence {
        Vec::new()
    } else {
        agent.tools.schemas_for_query(&prompt.tool_query)
    };
    let mut notes = vec![budget_note(agent, iteration + 1, state.tools_used.len())];
    #[cfg(windows)]
    notes.push(
        "Runtime platform: Windows. The shell tool runs through Windows cmd.exe. Never use heredoc (<<), tail, head, or wc. Put multiline Python in a temporary script via apply_patch, or use PowerShell-compatible commands."
            .to_string(),
    );
    notes.extend(prompt.runtime_notes.clone());
    if prompt.deliverable.is_some() && !deliverable_ready {
        notes.push(
            "The user explicitly requested a generated PDF deliverable. Do not finish with research or Markdown only. Create or update a valid PDF file in this workspace during this turn, verify it, and include its path in the final answer. An unchanged PDF from an earlier turn does not satisfy this request."
                .to_string(),
        );
    }
    if state.deliverable_finish_rejections > 0 {
        notes.push(format!(
            "Your previous attempt to finish was rejected because no new or updated valid PDF existed. Stop further research and produce the requested PDF now (rejected attempts: {}).",
            state.deliverable_finish_rejections
        ));
    }
    if state.tool_failures >= 4 {
        notes.push(format!(
            "Tool attempts have failed {} times in this turn. Do not repeat the same command syntax or search route; switch to a known-compatible method and use evidence already collected.",
            state.tool_failures
        ));
    }
    if state.tools_used.len() >= SOFT_CONVERGENCE_TOOL_CALLS {
        notes.push(
            "The turn has crossed the soft tool-call limit. Stop opening new research branches; close only essential gaps and prepare the final answer from existing evidence."
                .to_string(),
        );
    }
    if force_convergence {
        notes.push(
            "Too many tool calls and failures have accumulated. You have no tools for this call: converge now and provide the best final answer from existing evidence, clearly noting any remaining uncertainty."
                .to_string(),
        );
    }
    if has_unfinished_plan(agent) {
        notes.push(
            "The active plan still has pending or in-progress steps. Continue executing it and update the plan; do not end with only a progress report."
                .to_string(),
        );
    }
    if state.consecutive_empty_responses > 0 {
        notes.push(
            "Your previous response was empty. Continue the task with a tool call or a non-empty response."
                .to_string(),
        );
    }
    if state.consecutive_transport_errors > 0 {
        notes.push(format!(
            "The previous model request failed before a response arrived. Continue the same user request without asking them to repeat it (transport retries: {}).",
            state.consecutive_transport_errors
        ));
    }
    let (response, edit_stats) = agent.call_model(&schemas, &notes, sink).await;
    add_usage(&mut state.usage, &response.usage);
    trace::model_response(iteration, &response, &edit_stats);
    response
}

fn finish_response(
    agent: &mut AgentLoop,
    prompt: &PromptContext,
    response: &ModelResponse,
    state: &mut TurnState,
    sink: &mut Option<EventSink>,
) -> Option<(String, &'static str)> {
    if response.finish_reason == "error" && is_retryable_transport_error(&response.content) {
        state.consecutive_transport_errors += 1;
        if state.consecutive_transport_errors < MAX_CONSECUTIVE_TRANSPORT_ERRORS {
            return None;
        }
        let text = "连接模型服务失败，已自动重试多次。当前会话和你的问题都已保留，网络恢复后可以直接重试，无需重新描述任务。".to_string();
        agent.session.add_assistant(&text, None, "");
        emit(
            sink,
            LoopEvent::AssistantText {
                text: text.clone(),
                model: agent.provider.model().to_string(),
                confirmed_model: agent.provider.confirmed_model(),
            },
        );
        return Some((text, "error"));
    }
    if response.finish_reason == "error" {
        let text = if response.content.is_empty() {
            "模型服务调用失败，请稍后重试。".to_string()
        } else {
            response.content.clone()
        };
        agent.session.add_assistant(&text, None, "");
        return Some((text, "error"));
    }
    state.consecutive_transport_errors = 0;
    if response.has_tool_calls() {
        state.consecutive_empty_responses = 0;
        return None;
    }

    let text = response.content.clone();
    if text.trim().is_empty() {
        state.consecutive_empty_responses += 1;
        if state.consecutive_empty_responses < MAX_CONSECUTIVE_EMPTY_RESPONSES {
            return None;
        }

        let error = format!(
            "模型连续 {} 次返回空内容，任务未被标记为完成。请检查模型服务后重试。",
            MAX_CONSECUTIVE_EMPTY_RESPONSES
        );
        agent.session.add_assistant(&error, None, "");
        emit(
            sink,
            LoopEvent::AssistantText {
                text: error.clone(),
                model: agent.provider.model().to_string(),
                confirmed_model: agent.provider.confirmed_model(),
            },
        );
        return Some((error, "error"));
    }

    state.consecutive_empty_responses = 0;
    if prompt.deliverable.is_some()
        && prompt
            .deliverable
            .as_ref()
            .and_then(|requirement| requirement.completed_path(&agent.tools.ctx.workspace))
            .is_none()
    {
        state.deliverable_finish_rejections += 1;
        return None;
    }
    agent
        .session
        .add_assistant(&text, None, &response.reasoning);
    emit(
        sink,
        LoopEvent::AssistantText {
            text: text.clone(),
            model: agent.provider.model().to_string(),
            confirmed_model: agent.provider.confirmed_model(),
        },
    );
    if has_unfinished_plan(agent) {
        return None;
    }
    Some((text, "completed"))
}

fn is_retryable_transport_error(message: &str) -> bool {
    let message = message.trim_start();
    message.starts_with("RequestError:")
        || message.starts_with("TimeoutError:")
        || message.starts_with("StreamError:")
}

fn has_unfinished_plan(agent: &AgentLoop) -> bool {
    if agent.tools.ctx.plan_turn_id.get() != agent.tools.ctx.active_turn_id.get() {
        return false;
    }
    agent.tools.ctx.plan.borrow().iter().any(|item| {
        matches!(
            item.get("status").and_then(Value::as_str),
            Some("pending" | "in_progress")
        )
    })
}

fn retire_active_plan(agent: &AgentLoop) {
    agent.tools.ctx.plan.borrow_mut().clear();
    agent.tools.ctx.plan_turn_id.set(None);
}

fn persist_tool_calls(agent: &mut AgentLoop, response: &ModelResponse) {
    let calls: Vec<Value> = response
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": dump_args(&call.arguments)},
            })
        })
        .collect();
    agent
        .session
        .add_assistant(&response.content, Some(calls), &response.reasoning);
}

fn stop_turn(
    agent: &mut AgentLoop,
    stop: DispatchStop,
    iteration: usize,
    state: TurnState,
) -> TurnResult {
    match stop {
        DispatchStop::Cancelled { before_next_tool } => {
            let placeholder = if before_next_tool {
                "[interrupted: stopped by user before this tool ran]"
            } else {
                "[interrupted: stopped by user]"
            };
            agent.session.backfill_unanswered_tool_calls(placeholder);
            retire_active_plan(agent);
            let text = "Stopped by user.".to_string();
            agent.session.add_assistant(&text, None, "");
            state.finish(text, iteration + 1, "cancelled")
        }
        DispatchStop::BudgetExhausted => tool_budget_result(agent, iteration, state),
    }
}

fn tool_budget_result(agent: &mut AgentLoop, iteration: usize, state: TurnState) -> TurnResult {
    agent.session.backfill_unanswered_tool_calls(
        "[interrupted: task budget exhausted before this tool ran]",
    );
    let text = format!(
        "Stopped because the task budget was exhausted (model calls: {}/{}, tool calls: {}/{}). The task may be incomplete.",
        iteration + 1,
        agent.task_budget.max_model_calls,
        state.tools_used.len(),
        agent.task_budget.max_tool_calls,
    );
    agent.session.add_assistant(&text, None, "");
    state.finish(text, iteration + 1, "task_budget")
}

fn model_budget_result(
    agent: &mut AgentLoop,
    max_model_calls: usize,
    state: TurnState,
) -> TurnResult {
    let text = format!(
        "Reached the task budget of {} model calls without finishing. The task may be incomplete.",
        max_model_calls
    );
    agent.session.add_assistant(&text, None, "");
    state.finish(text, max_model_calls, "task_budget")
}

fn budget_note(agent: &AgentLoop, model_call: usize, tool_calls_used: usize) -> String {
    format!(
        "Runtime task budget: model_call {}/{}; tool_calls {}/{}; context_limit_chars {}. Stay within this budget, prefer direct progress, and summarize before asking for more work.",
        model_call,
        agent.task_budget.max_model_calls,
        tool_calls_used,
        agent.task_budget.max_tool_calls,
        agent.context_edit.max_chars,
    )
}

fn has_image_block(user_input: &Value) -> bool {
    user_input.as_array().is_some_and(|blocks| {
        blocks
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("image_url"))
    })
}

fn user_query_text(user_input: &Value) -> String {
    if let Some(text) = user_input.as_str() {
        return text.to_string();
    }
    if let Some(blocks) = user_input.as_array() {
        return blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
    }
    user_input.to_string()
}

fn is_cancelled(cancel: Option<&dyn Fn() -> bool>) -> bool {
    cancel.is_some_and(|check| check())
}

fn add_usage(target: &mut BTreeMap<String, i64>, usage: &BTreeMap<String, i64>) {
    for (key, value) in usage {
        *target.entry(key.clone()).or_insert(0) += value;
    }
}

fn memory_recall_notes(
    memory: &Option<std::rc::Rc<crate::memory::MemoryStore>>,
    query: &str,
    max_entries: usize,
    max_chars: usize,
) -> Vec<String> {
    let Some(store) = memory else {
        return Vec::new();
    };
    let recalled = store.recall(query, max_entries, max_chars);
    if recalled.is_empty() {
        return Vec::new();
    }
    vec![format!("[memory recall for this prompt]\n{recalled}")]
}
