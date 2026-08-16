//! Background command lifecycle tools backed by PolicyExecutor containment.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ncx_tools::{ManagedProcess, PolicyExecutor, ProcessSnapshot};
use serde_json::{json, Value};

use crate::tools::{authorize_shell, resolve_shell_workdir, ShellTool, Tool, ToolContext};

const MAX_BACKGROUND_TASKS: usize = 8;
const MAX_BACKGROUND_TIMEOUT_S: u64 = 86_400;

pub(crate) struct ProcessManager {
    next_id: u64,
    tasks: HashMap<u64, BackgroundTask>,
}

struct BackgroundTask {
    command: String,
    process: ManagedProcess,
    deadline: Instant,
    timed_out: bool,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self {
            next_id: 1,
            tasks: HashMap::new(),
        }
    }
}

pub struct BackgroundStartTool;
pub struct BackgroundPollTool;
pub struct BackgroundStopTool;
pub struct BackgroundListTool;

#[async_trait(?Send)]
impl Tool for BackgroundStartTool {
    fn name(&self) -> &str {
        "background_start"
    }

    fn description(&self) -> &str {
        "Start an approved command as a contained background task and return a task id."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "workdir": {"type": "string"},
                "timeout": {"type": "integer", "minimum": 1, "maximum": MAX_BACKGROUND_TIMEOUT_S},
                "justification": {"type": "string"}
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return error("INVALID_ARGUMENT", "command must be a string");
        };
        if command.trim().is_empty() {
            return error("INVALID_ARGUMENT", "command cannot be empty");
        }
        let workdir = resolve_shell_workdir(ctx, args);
        if !workdir.exists() {
            return error(
                "INVALID_WORKDIR",
                &format!("working directory does not exist: {}", workdir.display()),
            );
        }
        let justification = args
            .get("justification")
            .and_then(Value::as_str)
            .unwrap_or("");
        let escalation = ShellTool::needs_escalation(ctx, command, &workdir);
        if let Err(message) =
            authorize_shell(ctx, command, &workdir, justification, escalation).await
        {
            return error("NOT_AUTHORIZED", &message);
        }
        let timeout_s = args
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(ctx.timeout_s)
            .clamp(1, MAX_BACKGROUND_TIMEOUT_S);
        let process = match PolicyExecutor::new().spawn_managed(command, &workdir) {
            Ok(process) => process,
            Err(message) => return error("SPAWN_FAILED", &message),
        };
        let mut manager = ctx.process_manager.lock().await;
        if manager.tasks.len() >= MAX_BACKGROUND_TASKS {
            return error("TASK_LIMIT", "background task limit reached");
        }
        let id = manager.next_id;
        manager.next_id += 1;
        manager.tasks.insert(
            id,
            BackgroundTask {
                command: command.to_string(),
                process,
                deadline: Instant::now() + Duration::from_secs(timeout_s),
                timed_out: false,
            },
        );
        json!({"status": "started", "task_id": id}).to_string()
    }
}

#[async_trait(?Send)]
impl Tool for BackgroundPollTool {
    fn name(&self) -> &str {
        "background_poll"
    }

    fn description(&self) -> &str {
        "Poll a background task for incremental stdout/stderr and completion status."
    }

    fn parameters(&self) -> Value {
        task_id_schema(true)
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = task_id(args) else {
            return error("INVALID_ARGUMENT", "task_id must be a non-negative integer");
        };
        let cursor = args.get("cursor").and_then(Value::as_u64).unwrap_or(0);
        let mut manager = ctx.process_manager.lock().await;
        let Some(task) = manager.tasks.get_mut(&id) else {
            return error(
                "TASK_NOT_FOUND",
                &format!("background task {id} does not exist"),
            );
        };
        if !task.timed_out && Instant::now() >= task.deadline {
            task.timed_out = true;
            task.process.terminate();
        }
        match task.process.poll(cursor).await {
            Ok(snapshot) => render_snapshot(id, task.timed_out, snapshot),
            Err(message) => error("POLL_FAILED", &message),
        }
    }
}

#[async_trait(?Send)]
impl Tool for BackgroundStopTool {
    fn name(&self) -> &str {
        "background_stop"
    }

    fn description(&self) -> &str {
        "Terminate and remove a background task owned by this runtime."
    }

    fn parameters(&self) -> Value {
        task_id_schema(false)
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = task_id(args) else {
            return error("INVALID_ARGUMENT", "task_id must be a non-negative integer");
        };
        let mut manager = ctx.process_manager.lock().await;
        let Some(mut task) = manager.tasks.remove(&id) else {
            return error(
                "TASK_NOT_FOUND",
                &format!("background task {id} does not exist"),
            );
        };
        task.process.terminate();
        json!({"status": "stopped", "task_id": id}).to_string()
    }
}

#[async_trait(?Send)]
impl Tool for BackgroundListTool {
    fn name(&self) -> &str {
        "background_list"
    }

    fn description(&self) -> &str {
        "List background tasks owned by this runtime."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, _args: &Value) -> String {
        let manager = ctx.process_manager.lock().await;
        let tasks = manager
            .tasks
            .iter()
            .map(|(id, task)| json!({"task_id": id, "command": task.command, "timed_out": task.timed_out}))
            .collect::<Vec<_>>();
        json!({"status": "ok", "tasks": tasks}).to_string()
    }
}

fn task_id_schema(with_cursor: bool) -> Value {
    let mut properties = json!({"task_id": {"type": "integer", "minimum": 1}});
    if with_cursor {
        properties["cursor"] = json!({"type": "integer", "minimum": 0});
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": ["task_id"],
        "additionalProperties": false
    })
}

fn task_id(args: &Value) -> Option<u64> {
    args.get("task_id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
}

fn render_snapshot(id: u64, timed_out: bool, snapshot: ProcessSnapshot) -> String {
    let chunks = snapshot
        .chunks
        .into_iter()
        .map(|chunk| json!({"seq": chunk.seq, "stream": chunk.stream, "text": chunk.text}))
        .collect::<Vec<_>>();
    json!({
        "status": if snapshot.running { "running" } else { "completed" },
        "task_id": id,
        "exit_code": snapshot.exit_code,
        "timed_out": timed_out,
        "cursor": snapshot.next_cursor,
        "output": chunks
    })
    .to_string()
}

fn error(code: &str, message: &str) -> String {
    json!({"status": "error", "code": code, "message": message}).to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ncx_sandbox::{SandboxPolicy, DANGER_FULL_ACCESS};

    use super::*;
    use crate::tools::ToolRegistry;

    #[tokio::test]
    async fn background_command_can_be_started_and_polled() {
        let root = std::env::temp_dir();
        let context = ToolContext::new(
            PathBuf::from(&root),
            SandboxPolicy::new(DANGER_FULL_ACCESS, &root),
        );
        let registry = ToolRegistry::new(context);
        let started = registry
            .execute(
                "background_start",
                &json!({"command": "echo background_ok"}),
            )
            .await;
        let id = serde_json::from_str::<Value>(&started).unwrap()["task_id"]
            .as_u64()
            .unwrap();
        let mut polled = Value::Null;
        for _ in 0..50 {
            let output = registry
                .execute("background_poll", &json!({"task_id": id}))
                .await;
            polled = serde_json::from_str(&output).unwrap();
            if polled["status"] == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(polled["exit_code"], 0);
        assert!(polled["output"].to_string().contains("background_ok"));
    }
}
