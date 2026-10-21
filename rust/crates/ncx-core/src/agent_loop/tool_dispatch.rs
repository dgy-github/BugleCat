//! Tool-call ordering policy, event emission, and result commitment.

use ncx_provider::ToolCall;

use super::{AgentLoop, EventSink, LoopEvent, dump_args, emit, trace};
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
    use serde_json::{Value, json};

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
        // Pair only a retry of the same tool on the same target; an unrelated
        // success after a failure says nothing about how the failure was fixed.
        if let Some(target) = target {
            let matched_pos = agent
                .last_failed_tools
                .iter()
                .rposition(|f| f.tool == call.name && f.target == target);
            if let Some(pos) = matched_pos {
                let failure = agent.last_failed_tools.remove(pos);
                let current_detail = tool_call_detail(&call.name, &call.arguments);
                let candidate_opt = if call.name == "shell" {
                    reusable_shell_environment_note(&failure, current_detail.as_deref())
                } else {
                    let intervening_files: Vec<String> = agent
                        .last_files_touched
                        .iter()
                        .skip(failure.files_touched_len)
                        .cloned()
                        .collect();
                    let resolution_clause = match (
                        !intervening_files.is_empty(),
                        current_detail
                            .as_deref()
                            .filter(|d| Some(*d) != failure.detail.as_deref()),
                    ) {
                        (true, Some(cmd)) => format!(
                            "a later `{}` on the same target succeeded after editing `{}` (`{cmd}`).",
                            call.name,
                            intervening_files.join("`, `")
                        ),
                        (true, None) => format!(
                            "a later `{}` on the same target succeeded after editing `{}`.",
                            call.name,
                            intervening_files.join("`, `")
                        ),
                        (false, Some(cmd)) => format!(
                            "a later `{}` on the same target succeeded with `{cmd}`.",
                            call.name
                        ),
                        (false, None) => format!(
                            "a later `{}` on the same target succeeded in the same task.",
                            call.name
                        ),
                    };
                    let failed_context = match &failure.detail {
                        Some(detail) if detail != &failure.target => {
                            format!("`{detail}` -> {}", failure.excerpt)
                        }
                        _ => failure.excerpt.clone(),
                    };
                    Some(format!(
                        "`{}` on `{}` failed ({}); {}",
                        failure.tool, failure.target, failed_context, resolution_clause
                    ))
                };
                if let Some(candidate_text) = candidate_opt {
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
        }
        for path in extract_touched_files(&call.name, &call.arguments) {
            if !agent.last_files_touched.contains(&path) {
                agent.last_files_touched.push(path);
            }
        }
    } else if let Some(target) = target {
        let excerpt = extract_error_excerpt(&result);
        let detail = tool_call_detail(&call.name, &call.arguments);
        if agent.last_failed_tools.len() >= MAX_PENDING_FAILURES {
            agent.last_failed_tools.remove(0);
        }
        agent.last_failed_tools.push(FailedToolCall {
            tool: call.name.clone(),
            target,
            excerpt,
            detail,
            files_touched_len: agent.last_files_touched.len(),
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
    pub(crate) detail: Option<String>,
    pub(crate) files_touched_len: usize,
}

fn extract_error_excerpt(result: &str) -> String {
    let lines: Vec<&str> = result
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return "tool error".to_string();
    }
    let first = lines[0];
    let diagnostic = lines.iter().skip(1).copied().find(|l| {
        let lower = l.to_ascii_lowercase();
        lower.contains("error")
            || lower.contains("failed")
            || lower.contains("panic")
            || lower.contains("not found")
            || lower.contains("denied")
            || lower.contains("invalid")
    });
    let combined = match diagnostic {
        Some(diag) if !first.to_ascii_lowercase().contains("error") => format!("{first}: {diag}"),
        _ => first.to_string(),
    };
    combined.chars().take(120).collect()
}

fn tool_call_detail(tool_name: &str, args: &serde_json::Value) -> Option<String> {
    if tool_name == "shell" {
        let cmd = args.get("command").and_then(|v| v.as_str())?.trim();
        if !cmd.is_empty() {
            let single_line = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
            return Some(single_line.chars().take(80).collect());
        }
    }
    None
}

/// Filter `shell` failure->recovery pairs down to cross-task environment
/// constraints, discarding one-off `python -c` / `pytest` / `git` syntax or
/// test assertion trial-and-error logs.
fn reusable_shell_environment_note(
    failure: &FailedToolCall,
    current_detail: Option<&str>,
) -> Option<String> {
    let target_lower = failure.target.to_ascii_lowercase();
    if target_lower.starts_with("python -c")
        || target_lower.starts_with("python3 -c")
        || target_lower.starts_with("py -c")
        || target_lower.starts_with("pytest")
        || target_lower.starts_with("git ")
    {
        return None;
    }
    for detail in [failure.detail.as_deref(), current_detail]
        .into_iter()
        .flatten()
    {
        let lower = detail.to_ascii_lowercase();
        if lower.contains("pip install")
            || lower.contains("pip download")
            || lower.contains("pip wheel")
            || lower.contains("pytest")
        {
            return None;
        }
    }
    let exc_lower = failure.excerpt.to_ascii_lowercase();
    if exc_lower.contains("denied by approval policy") {
        return None;
    }
    if exc_lower.contains("windows shell is cmd.exe")
        || exc_lower.contains("does not support heredoc")
        || exc_lower.contains("unix tail/head/wc")
    {
        return Some(
            "Environment constraint: Windows shell is cmd.exe and does not support heredoc (<<) or Unix tail/head/wc commands; run commands without tail/head/wc pipes and write multiline scripts to files."
                .to_string(),
        );
    }
    if exc_lower.contains("is not recognized as an internal or external command")
        || exc_lower.contains("command not found")
    {
        return Some(format!(
            "Environment constraint: `{}` failed ({}).",
            failure.target, failure.excerpt
        ));
    }
    None
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
