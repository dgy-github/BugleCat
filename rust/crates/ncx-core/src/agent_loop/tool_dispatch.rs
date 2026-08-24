//! Tool-call ordering policy, event emission, and result commitment.

use ncx_provider::ToolCall;

use super::{dump_args, emit, trace, AgentLoop, EventSink, LoopEvent};
use crate::tool_recovery::classify_tool_result;

pub(super) enum DispatchStop {
    Cancelled { before_next_tool: bool },
    BudgetExhausted,
}

struct DispatchOutput<'a> {
    tools_used: &'a mut Vec<String>,
    tool_failures: &'a mut usize,
    sink: &'a mut Option<EventSink>,
}

pub(super) async fn execute(
    agent: &mut AgentLoop,
    calls: &[ToolCall],
    tools_used: &mut Vec<String>,
    tool_failures: &mut usize,
    cancel: Option<&dyn Fn() -> bool>,
    sink: &mut Option<EventSink>,
) -> Option<DispatchStop> {
    let mut output = DispatchOutput {
        tools_used,
        tool_failures,
        sink,
    };
    let mut index = 0usize;
    while index < calls.len() {
        if is_cancelled(cancel) {
            return Some(DispatchStop::Cancelled {
                before_next_tool: true,
            });
        }
        let remaining = agent
            .task_budget
            .max_tool_calls
            .saturating_sub(output.tools_used.len());
        if remaining == 0 {
            return Some(DispatchStop::BudgetExhausted);
        }

        if starts_parallel_run(agent, calls, index) {
            index = execute_read_batch(agent, calls, index, remaining, cancel, &mut output).await;
        } else {
            execute_serial(agent, &calls[index], cancel, &mut output).await;
            index += 1;
        }

        if is_cancelled(cancel) {
            return Some(DispatchStop::Cancelled {
                before_next_tool: false,
            });
        }
    }
    None
}

fn starts_parallel_run(agent: &AgentLoop, calls: &[ToolCall], index: usize) -> bool {
    agent.tools.is_read_only(&calls[index].name)
        && index + 1 < calls.len()
        && agent.tools.is_read_only(&calls[index + 1].name)
}

async fn execute_read_batch(
    agent: &mut AgentLoop,
    calls: &[ToolCall],
    mut index: usize,
    remaining: usize,
    cancel: Option<&dyn Fn() -> bool>,
    output: &mut DispatchOutput<'_>,
) -> usize {
    let mut batch = Vec::new();
    while index < calls.len()
        && agent.tools.is_read_only(&calls[index].name)
        && batch.len() < remaining
    {
        batch.push(&calls[index]);
        index += 1;
    }
    for call in &batch {
        record_tool_start(output.tools_used, output.sink, call);
    }
    let results = agent
        .tool_scheduler
        .execute_read_only_batch(&agent.tools, &batch, agent.max_parallel_tool_calls, cancel)
        .await;
    for (position, call) in batch.into_iter().enumerate() {
        let result = results.get(position).cloned().unwrap_or_else(|| {
            "Error: tool scheduler returned no result for this call.".to_string()
        });
        if classify_tool_result(&result).is_some() {
            *output.tool_failures += 1;
        }
        record_tool_result(agent, output.sink, call, result);
    }
    index
}

async fn execute_serial(
    agent: &mut AgentLoop,
    call: &ToolCall,
    cancel: Option<&dyn Fn() -> bool>,
    output: &mut DispatchOutput<'_>,
) {
    record_tool_start(output.tools_used, output.sink, call);
    let result = agent
        .tool_scheduler
        .execute_one(&agent.tools, call, cancel)
        .await;
    trace::tool_result(&call.name, &result);
    if classify_tool_result(&result).is_some() {
        *output.tool_failures += 1;
    }
    record_tool_result(agent, output.sink, call, result);
}

fn record_tool_start(tools_used: &mut Vec<String>, sink: &mut Option<EventSink>, call: &ToolCall) {
    tools_used.push(call.name.clone());
    emit(
        sink,
        LoopEvent::ToolStart {
            name: call.name.clone(),
            args: dump_args(&call.arguments),
        },
    );
}

fn record_tool_result(
    agent: &mut AgentLoop,
    sink: &mut Option<EventSink>,
    call: &ToolCall,
    result: String,
) {
    emit(
        sink,
        LoopEvent::ToolResult {
            name: call.name.clone(),
            result: result.clone(),
        },
    );
    agent.session.add_tool_result(&call.id, &call.name, &result);
}

fn is_cancelled(cancel: Option<&dyn Fn() -> bool>) -> bool {
    cancel.is_some_and(|check| check())
}
