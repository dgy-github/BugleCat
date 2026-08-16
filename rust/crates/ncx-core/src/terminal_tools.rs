//! Approved raw PTY terminal sessions with incremental stdin/stdout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use ncx_sandbox::{DANGER_FULL_ACCESS, READ_ONLY};
use ncx_tools::{PolicyExecutor, PtyProcess, PtySnapshot};
use serde_json::{json, Value};

use crate::tools::{authorize_shell, Tool, ToolContext};

const MAX_TERMINALS: usize = 8;

pub(crate) struct TerminalManager {
    next_id: u64,
    sessions: HashMap<u64, TerminalSession>,
}

struct TerminalSession {
    cwd: PathBuf,
    process: PtyProcess,
    cursor: u64,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            next_id: 1,
            sessions: HashMap::new(),
        }
    }
}

pub struct TerminalOpenTool;
pub struct TerminalExecTool;
pub struct TerminalWriteTool;
pub struct TerminalReadTool;
pub struct TerminalResizeTool;
pub struct TerminalCloseTool;
pub struct TerminalListTool;

#[async_trait(?Send)]
impl Tool for TerminalOpenTool {
    fn name(&self) -> &str {
        "terminal_open"
    }

    fn description(&self) -> &str {
        "Open an unrestricted raw PTY shell. This requires explicit approval outside danger-full-access because later stdin can run arbitrary commands."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workdir": {"type": "string"},
                "rows": {"type": "integer", "minimum": 4, "maximum": 200},
                "cols": {"type": "integer", "minimum": 20, "maximum": 400},
                "justification": {"type": "string"}
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        if ctx.plan_mode || ctx.policy.mode == READ_ONLY {
            return error(
                "PTY_NOT_ALLOWED",
                "raw PTY shells are disabled in plan/read-only mode",
            );
        }
        let cwd = match resolve_workdir(&ctx.workspace, args.get("workdir")) {
            Ok(cwd) => cwd,
            Err(message) => return error("INVALID_WORKDIR", &message),
        };
        let justification = args
            .get("justification")
            .and_then(Value::as_str)
            .unwrap_or("Open an unrestricted interactive shell for this task.");
        let needs_approval = ctx.policy.mode != DANGER_FULL_ACCESS;
        if let Err(message) =
            authorize_shell(ctx, "raw-pty-shell", &cwd, justification, needs_approval).await
        {
            return error("NOT_AUTHORIZED", &message);
        }
        let rows = bounded_u16(args, "rows", 24, 4, 200);
        let cols = bounded_u16(args, "cols", 100, 20, 400);
        let process = match PolicyExecutor::new().spawn_pty(&cwd, rows, cols) {
            Ok(process) => process,
            Err(message) => return error("PTY_SPAWN_FAILED", &message),
        };
        let mut manager = ctx.terminal_manager.lock().await;
        if manager.sessions.len() >= MAX_TERMINALS {
            return error("TERMINAL_LIMIT", "terminal session limit reached");
        }
        let id = manager.next_id;
        manager.next_id += 1;
        manager.sessions.insert(
            id,
            TerminalSession {
                cwd: cwd.clone(),
                process,
                cursor: 0,
            },
        );
        json!({"status": "opened", "terminal_id": id, "cwd": cwd}).to_string()
    }
}

#[async_trait(?Send)]
impl Tool for TerminalWriteTool {
    fn name(&self) -> &str {
        "terminal_write"
    }

    fn description(&self) -> &str {
        "Write exact raw stdin bytes to an approved PTY session, including control sequences or interactive answers."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {"type": "integer", "minimum": 1},
                "input": {"type": "string"},
                "append_newline": {
                    "type": "boolean",
                    "description": "Append the platform shell newline after input. Use this to submit a command without embedding control characters."
                }
            },
            "required": ["terminal_id", "input"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = terminal_id(args) else {
            return error("INVALID_ARGUMENT", "terminal_id must be a positive integer");
        };
        let Some(input) = args.get("input").and_then(Value::as_str) else {
            return error("INVALID_ARGUMENT", "input must be a string");
        };
        let mut manager = ctx.terminal_manager.lock().await;
        let Some(session) = manager.sessions.get_mut(&id) else {
            return error(
                "TERMINAL_NOT_FOUND",
                &format!("terminal session {id} does not exist"),
            );
        };
        let append_newline = args
            .get("append_newline")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let result = session.process.write(input).and_then(|()| {
            if append_newline {
                session.process.write(platform_newline())
            } else {
                Ok(())
            }
        });
        match result {
            Ok(()) => json!({
                "status": "written",
                "terminal_id": id,
                "bytes": input.len() + usize::from(append_newline) * platform_newline().len()
            })
            .to_string(),
            Err(message) => error("PTY_WRITE_FAILED", &message),
        }
    }
}

#[async_trait(?Send)]
impl Tool for TerminalReadTool {
    fn name(&self) -> &str {
        "terminal_read"
    }

    fn description(&self) -> &str {
        "Read incremental raw output from a PTY session using a cursor."
    }

    fn parameters(&self) -> Value {
        terminal_cursor_schema()
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = terminal_id(args) else {
            return error("INVALID_ARGUMENT", "terminal_id must be a positive integer");
        };
        let mut manager = ctx.terminal_manager.lock().await;
        let Some(session) = manager.sessions.get_mut(&id) else {
            return error(
                "TERMINAL_NOT_FOUND",
                &format!("terminal session {id} does not exist"),
            );
        };
        let cursor = args
            .get("cursor")
            .and_then(Value::as_u64)
            .unwrap_or(session.cursor);
        match session.process.poll(cursor) {
            Ok(snapshot) => {
                session.cursor = snapshot.next_cursor;
                render_snapshot(id, snapshot)
            }
            Err(message) => error("PTY_READ_FAILED", &message),
        }
    }
}

#[async_trait(?Send)]
impl Tool for TerminalExecTool {
    fn name(&self) -> &str {
        "terminal_exec"
    }

    fn description(&self) -> &str {
        "Write a command line to an approved PTY shell, wait briefly, and return incremental output. Use terminal_read for long-running commands."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {"type": "integer", "minimum": 1},
                "command": {"type": "string"},
                "wait_ms": {"type": "integer", "minimum": 50, "maximum": 5000}
            },
            "required": ["terminal_id", "command"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = terminal_id(args) else {
            return error("INVALID_ARGUMENT", "terminal_id must be a positive integer");
        };
        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return error("INVALID_ARGUMENT", "command must be a string");
        };
        if command.trim().is_empty() {
            return error("INVALID_ARGUMENT", "command cannot be empty");
        }
        let wait_ms = args
            .get("wait_ms")
            .and_then(Value::as_u64)
            .unwrap_or(300)
            .clamp(50, 5000);
        let cursor = {
            let mut manager = ctx.terminal_manager.lock().await;
            let Some(session) = manager.sessions.get_mut(&id) else {
                return error(
                    "TERMINAL_NOT_FOUND",
                    &format!("terminal session {id} does not exist"),
                );
            };
            let cursor = session.cursor;
            if let Err(message) = session.process.write(&format!("{command}\r\n")) {
                return error("PTY_WRITE_FAILED", &message);
            }
            cursor
        };
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        let mut manager = ctx.terminal_manager.lock().await;
        let session = manager
            .sessions
            .get_mut(&id)
            .expect("terminal exists after write");
        match session.process.poll(cursor) {
            Ok(snapshot) => {
                session.cursor = snapshot.next_cursor;
                render_snapshot(id, snapshot)
            }
            Err(message) => error("PTY_READ_FAILED", &message),
        }
    }
}

#[async_trait(?Send)]
impl Tool for TerminalResizeTool {
    fn name(&self) -> &str {
        "terminal_resize"
    }

    fn description(&self) -> &str {
        "Resize an approved PTY session."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {"type": "integer", "minimum": 1},
                "rows": {"type": "integer", "minimum": 4, "maximum": 200},
                "cols": {"type": "integer", "minimum": 20, "maximum": 400}
            },
            "required": ["terminal_id", "rows", "cols"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = terminal_id(args) else {
            return error("INVALID_ARGUMENT", "terminal_id must be a positive integer");
        };
        let rows = bounded_u16(args, "rows", 24, 4, 200);
        let cols = bounded_u16(args, "cols", 100, 20, 400);
        let manager = ctx.terminal_manager.lock().await;
        let Some(session) = manager.sessions.get(&id) else {
            return error(
                "TERMINAL_NOT_FOUND",
                &format!("terminal session {id} does not exist"),
            );
        };
        match session.process.resize(rows, cols) {
            Ok(()) => json!({"status": "resized", "terminal_id": id, "rows": rows, "cols": cols})
                .to_string(),
            Err(message) => error("PTY_RESIZE_FAILED", &message),
        }
    }
}

#[async_trait(?Send)]
impl Tool for TerminalCloseTool {
    fn name(&self) -> &str {
        "terminal_close"
    }

    fn description(&self) -> &str {
        "Terminate and close an approved PTY session."
    }

    fn parameters(&self) -> Value {
        terminal_id_schema()
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let Some(id) = terminal_id(args) else {
            return error("INVALID_ARGUMENT", "terminal_id must be a positive integer");
        };
        let mut manager = ctx.terminal_manager.lock().await;
        let Some(mut session) = manager.sessions.remove(&id) else {
            return error(
                "TERMINAL_NOT_FOUND",
                &format!("terminal session {id} does not exist"),
            );
        };
        session.process.terminate();
        json!({"status": "closed", "terminal_id": id}).to_string()
    }
}

#[async_trait(?Send)]
impl Tool for TerminalListTool {
    fn name(&self) -> &str {
        "terminal_list"
    }

    fn description(&self) -> &str {
        "List approved raw PTY sessions."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, _args: &Value) -> String {
        let manager = ctx.terminal_manager.lock().await;
        let sessions = manager
            .sessions
            .iter()
            .map(|(id, session)| json!({"terminal_id": id, "cwd": session.cwd}))
            .collect::<Vec<_>>();
        json!({"status": "ok", "terminals": sessions}).to_string()
    }
}

fn resolve_workdir(base: &Path, requested: Option<&Value>) -> Result<PathBuf, String> {
    let path = match requested {
        None => base.to_path_buf(),
        Some(Value::String(path)) if path.trim().is_empty() => base.to_path_buf(),
        Some(Value::String(path)) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                base.join(path)
            }
        }
        Some(_) => return Err("workdir must be a string".to_string()),
    };
    let path = path.canonicalize().map_err(|error| {
        format!(
            "cannot resolve working directory '{}': {error}",
            path.display()
        )
    })?;
    let path = shell_compatible_path(path);
    if !path.is_dir() {
        return Err(format!(
            "working directory is not a directory: {}",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(windows)]
fn shell_compatible_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    text.strip_prefix(r"\\?\")
        .map_or(path.clone(), PathBuf::from)
}

#[cfg(not(windows))]
fn shell_compatible_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn platform_newline() -> &'static str {
    "\r\n"
}

#[cfg(not(windows))]
fn platform_newline() -> &'static str {
    "\n"
}

fn render_snapshot(id: u64, snapshot: PtySnapshot) -> String {
    let output = snapshot
        .chunks
        .into_iter()
        .map(|chunk| json!({"seq": chunk.seq, "text": chunk.text}))
        .collect::<Vec<_>>();
    json!({
        "status": if snapshot.running { "running" } else { "completed" },
        "terminal_id": id,
        "exit_code": snapshot.exit_code,
        "cursor": snapshot.next_cursor,
        "output": output
    })
    .to_string()
}

fn terminal_cursor_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "terminal_id": {"type": "integer", "minimum": 1},
            "cursor": {"type": "integer", "minimum": 0}
        },
        "required": ["terminal_id"],
        "additionalProperties": false
    })
}

fn terminal_id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"terminal_id": {"type": "integer", "minimum": 1}},
        "required": ["terminal_id"],
        "additionalProperties": false
    })
}

fn terminal_id(args: &Value) -> Option<u64> {
    args.get("terminal_id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
}

fn bounded_u16(args: &Value, name: &str, default: u16, min: u16, max: u16) -> u16 {
    args.get(name)
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(default))
        .clamp(u64::from(min), u64::from(max)) as u16
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
    async fn raw_terminal_accepts_stdin_and_returns_incremental_output() {
        let root = std::env::temp_dir();
        let context = ToolContext::new(
            PathBuf::from(&root),
            SandboxPolicy::new(DANGER_FULL_ACCESS, &root),
        );
        let registry = ToolRegistry::new(context);
        let opened = registry.execute("terminal_open", &json!({})).await;
        let id = serde_json::from_str::<Value>(&opened).unwrap()["terminal_id"]
            .as_u64()
            .unwrap();
        registry
            .execute(
                "terminal_write",
                &json!({
                    "terminal_id": id,
                    "input": "echo raw_terminal_ok",
                    "append_newline": true
                }),
            )
            .await;
        let mut output = String::new();
        for _ in 0..100 {
            output = registry
                .execute("terminal_read", &json!({"terminal_id": id, "cursor": 0}))
                .await;
            if output.contains("raw_terminal_ok") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(output.contains("raw_terminal_ok"), "{output}");
        let closed = registry
            .execute("terminal_close", &json!({"terminal_id": id}))
            .await;
        assert!(closed.contains("closed"), "{closed}");
    }

    #[cfg(windows)]
    #[test]
    fn strips_windows_extended_path_prefix_for_cmd() {
        assert_eq!(
            shell_compatible_path(PathBuf::from(r"\\?\D:\agent_prac\nanocodex")),
            PathBuf::from(r"D:\agent_prac\nanocodex")
        );
    }

    #[tokio::test]
    async fn raw_terminal_is_denied_in_read_only_mode() {
        let root = std::env::temp_dir();
        let context = ToolContext::new(PathBuf::from(&root), SandboxPolicy::new(READ_ONLY, &root));
        let result = TerminalOpenTool.execute(&context, &json!({})).await;
        assert!(result.contains("PTY_NOT_ALLOWED"), "{result}");
    }
}
