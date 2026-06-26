//! Deterministic project hooks around tool execution.
//!
//! Hooks are intentionally outside the model's discretion: if configured, they
//! run before/after matching tools and can block a pre-tool action by returning
//! a non-zero exit code.

use std::collections::HashMap;
use std::path::Path;

use ncx_config::HookConfig;
use ncx_tools::{ExecResult, PolicyExecutor};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreTool,
    PostTool,
}

impl HookEvent {
    fn as_str(self) -> &'static str {
        match self {
            HookEvent::PreTool => "pre_tool",
            HookEvent::PostTool => "post_tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookOutcome {
    pub notes: String,
    pub blocked: bool,
}

pub async fn run_matching_hooks(
    hooks: &[HookConfig],
    event: HookEvent,
    tool_name: &str,
    args: &Value,
    result: Option<&str>,
    workspace: &Path,
) -> HookOutcome {
    let mut notes = Vec::new();
    let mut blocked = false;
    for hook in hooks
        .iter()
        .filter(|h| h.event == event.as_str() && matches_tool(&h.matcher, tool_name))
    {
        let outcome = run_one_hook(hook, event, tool_name, args, result, workspace).await;
        if !outcome.notes.is_empty() {
            notes.push(outcome.notes);
        }
        if outcome.blocked {
            blocked = true;
            break;
        }
    }
    HookOutcome {
        notes: notes.join("\n\n"),
        blocked,
    }
}

fn matches_tool(matcher: &str, tool_name: &str) -> bool {
    let matcher = matcher.trim();
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    matcher
        .split([',', '|'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| part == "*" || part == tool_name)
}

async fn run_one_hook(
    hook: &HookConfig,
    event: HookEvent,
    tool_name: &str,
    args: &Value,
    result: Option<&str>,
    workspace: &Path,
) -> HookOutcome {
    let mut env = HashMap::new();
    env.insert("NCX_HOOK_EVENT".into(), event.as_str().into());
    env.insert("NCX_HOOK_TOOL".into(), tool_name.to_string());
    env.insert("NCX_HOOK_ARGS".into(), args.to_string());
    env.insert("NCX_HOOK_RESULT".into(), result.unwrap_or("").to_string());
    env.insert("NCX_HOOK_WORKSPACE".into(), workspace.display().to_string());
    let timeout = u64::try_from(hook.timeout_s)
        .ok()
        .filter(|v| *v > 0)
        .unwrap_or(10);
    let exec = PolicyExecutor::new();
    let result = exec
        .run_with_env(&hook.command, workspace, timeout, &env)
        .await;
    render_hook_result(hook, event, &result)
}

fn render_hook_result(hook: &HookConfig, event: HookEvent, result: &ExecResult) -> HookOutcome {
    let rendered = result.render();
    if event == HookEvent::PreTool && !result.ok() {
        return HookOutcome {
            notes: format!(
                "pre_tool hook blocked execution: command={:?}\n{}",
                hook.command, rendered
            ),
            blocked: true,
        };
    }
    let visible = !result.stdout.trim().is_empty()
        || !result.stderr.trim().is_empty()
        || result.timed_out
        || result.sandbox_denied
        || result.exit_code != 0;
    HookOutcome {
        notes: if visible {
            format!(
                "{} hook output: command={:?}\n{}",
                event.as_str(),
                hook.command,
                rendered
            )
        } else {
            String::new()
        },
        blocked: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_supports_exact_wildcard_and_lists() {
        assert!(matches_tool("*", "shell"));
        assert!(matches_tool("shell|apply_patch", "apply_patch"));
        assert!(matches_tool("read_file, shell", "shell"));
        assert!(!matches_tool("read_file", "shell"));
    }
}
