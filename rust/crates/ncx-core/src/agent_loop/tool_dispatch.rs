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

        if starts_parallel_run(&agent.tools, calls, index) {
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

fn starts_parallel_run(
    tools: &crate::tools::ToolRegistry,
    calls: &[ToolCall],
    index: usize,
) -> bool {
    tools.call_is_read_only(&calls[index].name, &calls[index].arguments)
        && index + 1 < calls.len()
        && tools.call_is_read_only(&calls[index + 1].name, &calls[index + 1].arguments)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use async_trait::async_trait;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::{json, Value};

    use super::*;
    use crate::tools::{Tool, ToolContext, ToolRegistry};

    struct MultiplexedTool;

    #[async_trait(?Send)]
    impl Tool for MultiplexedTool {
        fn name(&self) -> &str {
            "multiplexed"
        }

        fn description(&self) -> &str {
            "Test-only tool with read and write actions."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }

        fn call_is_read_only(&self, args: &Value) -> bool {
            args["action"] == "read"
        }

        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            "ok".into()
        }
    }

    #[test]
    fn dynamic_read_only_calls_form_parallel_batches_but_writes_do_not() {
        let workspace = crate::test_support::unique_temp_dir("ncx_dynamic_read_dispatch");
        std::fs::create_dir_all(&workspace).unwrap();
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
        tools.register(Box::new(MultiplexedTool));
        let calls = vec![
            ToolCall {
                id: "read-1".into(),
                name: "multiplexed".into(),
                arguments: json!({"action": "read"}),
            },
            ToolCall {
                id: "read-2".into(),
                name: "multiplexed".into(),
                arguments: json!({"action": "read"}),
            },
            ToolCall {
                id: "write".into(),
                name: "multiplexed".into(),
                arguments: json!({"action": "write"}),
            },
        ];

        assert!(starts_parallel_run(&tools, &calls, 0));
        assert!(!starts_parallel_run(&tools, &calls, 1));
    }
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
        && agent
            .tools
            .call_is_read_only(&calls[index].name, &calls[index].arguments)
        && batch.len() < remaining
    {
        batch.push(&calls[index]);
        index += 1;
    }
    for call in &batch {
        record_tool_start(output.tools_used, output.sink, call);
    }
    let started = std::time::Instant::now();
    let results = agent
        .tool_scheduler
        .execute_read_only_batch(&agent.tools, &batch, agent.max_parallel_tool_calls, cancel)
        .await;
    let duration_ms = started.elapsed().as_millis() as u64;
    for (position, call) in batch.into_iter().enumerate() {
        let result = results.get(position).cloned().unwrap_or_else(|| {
            "Error: tool scheduler returned no result for this call.".to_string()
        });
        if classify_tool_result(&result).is_some() {
            *output.tool_failures += 1;
        }
        record_tool_result(agent, output.sink, call, result, Some(duration_ms));
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
    let started = std::time::Instant::now();
    let result = agent
        .tool_scheduler
        .execute_one(&agent.tools, call, cancel)
        .await;
    let duration_ms = started.elapsed().as_millis() as u64;
    trace::tool_result(&call.name, &result);
    if classify_tool_result(&result).is_some() {
        *output.tool_failures += 1;
    }
    record_tool_result(agent, output.sink, call, result, Some(duration_ms));
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

/// Emit `LoopEvent::ToolResult` and persist the tool result with wall-clock execution
/// duration (`duration_ms`) into the session's in-conversation `_trace`.
fn record_tool_result(
    agent: &mut AgentLoop,
    sink: &mut Option<EventSink>,
    call: &ToolCall,
    result: String,
    duration_ms: Option<u64>,
) {
    let failed = classify_tool_result(&result).is_some();
    let target = recovery_target(&call.name, &call.arguments);
    if !failed {
        for path in extract_touched_files(&call.name, &call.arguments) {
            if !agent.last_files_touched.contains(&path) {
                agent.last_files_touched.push(path);
            }
        }
        // Pair only a retry of the same tool on the same target; an unrelated
        // success after a failure says nothing about how the failure was fixed.
        if let Some(target) = target {
            if let Some(pos) = agent
                .last_failed_tools
                .iter()
                .rposition(|f| f.tool == call.name && f.target == target)
            {
                let failure = agent.last_failed_tools.remove(pos);
                let candidate_text = format!(
                    "`{}` on `{}` failed ({}); a later `{}` on the same target succeeded in the same task.",
                    failure.tool, failure.target, failure.excerpt, call.name
                );
                let source_ref = format!(
                    "{}#turn-{}:{}",
                    agent.session.session_id, agent.next_turn_id, call.id
                );
                if !agent
                    .last_recovery_candidates
                    .iter()
                    .any(|(text, _)| *text == candidate_text)
                {
                    agent
                        .last_recovery_candidates
                        .push((candidate_text, source_ref));
                }
            }
        }
    } else if let Some(target) = target {
        let excerpt: String = result
            .lines()
            .next()
            .unwrap_or("tool error")
            .chars()
            .take(100)
            .collect();
        if agent.last_failed_tools.len() >= MAX_PENDING_FAILURES {
            agent.last_failed_tools.remove(0);
        }
        agent.last_failed_tools.push(FailedToolCall {
            tool: call.name.clone(),
            target,
            excerpt,
        });
    }
    emit(
        sink,
        LoopEvent::ToolResult {
            name: call.name.clone(),
            result: result.clone(),
        },
    );
    agent
        .session
        .add_tool_result_timed(&call.id, &call.name, &result, duration_ms);
}

const MAX_PENDING_FAILURES: usize = 16;

/// A failed tool call of the current turn awaiting a same-target retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FailedToolCall {
    pub(crate) tool: String,
    pub(crate) target: String,
    pub(crate) excerpt: String,
}

/// What a tool call acted on, for failure->recovery pairing: the file for
/// file tools, the first changed file of a patch, the program (plus
/// subcommand) for shell. `None` means the call cannot be paired.
fn recovery_target(tool_name: &str, args: &serde_json::Value) -> Option<String> {
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        return Some(path.to_string());
    }
    if tool_name == "shell" {
        let command = args.get("command").and_then(|v| v.as_str())?;
        let head: Vec<&str> = command.split_whitespace().take(2).collect();
        return (!head.is_empty()).then(|| head.join(" "));
    }
    extract_touched_files(tool_name, args).into_iter().next()
}

fn extract_touched_files(tool_name: &str, args: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    match tool_name {
        "write_file" | "edit_file" | "multi_edit" | "str_replace_editor" => {
            if let Some(p) = args
                .get("path")
                .or_else(|| args.get("file_path"))
                .and_then(|v| v.as_str())
            {
                if !p.trim().is_empty() {
                    out.push(p.trim().to_string());
                }
            }
        }
        "apply_patch" => {
            if let Some(patch) = args
                .get("patch")
                .or_else(|| args.get("input"))
                .and_then(|v| v.as_str())
            {
                for line in patch.lines() {
                    for prefix in [
                        "*** Update File: ",
                        "*** Add File: ",
                        "*** Delete File: ",
                        "+++ b/",
                    ] {
                        if let Some(rest) = line.strip_prefix(prefix) {
                            let path = rest.trim();
                            if !path.is_empty() && path != "/dev/null" {
                                out.push(path.to_string());
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn is_cancelled(cancel: Option<&dyn Fn() -> bool>) -> bool {
    cancel.is_some_and(|check| check())
}
