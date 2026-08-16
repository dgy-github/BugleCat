//! One-turn state machine and tool-call orchestration.

use std::collections::BTreeMap;
use std::time::Duration;

use ncx_provider::ModelResponse;
use serde_json::{json, Value};

use super::tool_dispatch::{self, DispatchStop};
use super::{dump_args, emit, trace, AgentLoop, EventSink, LoopEvent, TurnResult};
use crate::hooks::{run_matching_hooks, HookEvent};
use crate::turn_context::TurnContextRequest;

const MEMORY_RECALL_MAX_ENTRIES: usize = 8;
const MEMORY_RECALL_MAX_CHARS: usize = 4_000;

#[derive(Default)]
struct TurnState {
    tools_used: Vec<String>,
    usage: BTreeMap<String, i64>,
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
}

pub(super) async fn run(
    agent: &mut AgentLoop,
    user_input: Value,
    cancel: Option<&dyn Fn() -> bool>,
    sink: &mut Option<EventSink>,
) -> TurnResult {
    let prompt = match prepare_prompt(agent, &user_input).await {
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

        let response =
            match request_model_cancellable(agent, &prompt, iteration, &mut state, sink, cancel)
                .await
            {
                Some(response) => response,
                None => return cancelled_result(agent, iteration + 1, state),
            };
        if let Some((text, reason)) = finish_response(agent, &response, sink) {
            return state.finish(text, iteration + 1, reason);
        }

        persist_tool_calls(agent, &response);
        if let Some(stop) = tool_dispatch::execute(
            agent,
            &response.tool_calls,
            &mut state.tools_used,
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
    runtime_notes.extend(memory_recall_notes(
        &agent.tools.ctx.memory,
        &tool_query,
        MEMORY_RECALL_MAX_ENTRIES,
        MEMORY_RECALL_MAX_CHARS,
    ));
    Ok(PromptContext {
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
    let schemas = agent.tools.schemas_for_query(&prompt.tool_query);
    let mut notes = vec![budget_note(agent, iteration + 1, state.tools_used.len())];
    notes.extend(prompt.runtime_notes.clone());
    let (response, edit_stats) = agent.call_model(&schemas, &notes, sink).await;
    add_usage(&mut state.usage, &response.usage);
    trace::model_response(iteration, &response, &edit_stats);
    response
}

fn finish_response(
    agent: &mut AgentLoop,
    response: &ModelResponse,
    sink: &mut Option<EventSink>,
) -> Option<(String, &'static str)> {
    if response.finish_reason == "error" {
        let text = if response.content.is_empty() {
            "Model call failed.".to_string()
        } else {
            response.content.clone()
        };
        agent.session.add_assistant(&text, None, "");
        return Some((text, "error"));
    }
    if response.has_tool_calls() {
        return None;
    }

    let text = response.content.clone();
    agent
        .session
        .add_assistant(&text, None, &response.reasoning);
    if !text.is_empty() {
        emit(sink, LoopEvent::AssistantText(text.clone()));
    }
    Some((text, "completed"))
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
