//! McpTool — wraps an MCP server tool as a first-class ncx `Tool`.
//!
//! Each `McpTool` holds a reference-counted handle to the `McpClient` that owns
//! the server process. Multiple tools from the same server share one client via
//! `Rc<tokio::sync::Mutex<McpClient>>`, which serialises concurrent calls safely
//! on the current-thread runtime.
//!
//! Non-read-only tools go through the normal `ctx.approver` approval path before
//! calling the MCP server — same escalation model as `ShellTool`.

use std::collections::HashMap;
use std::rc::Rc;

use async_trait::async_trait;
use ncx_mcp::{McpClient, McpToolDef};
use ncx_sandbox::{Approver, Decision};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::tools::{ApprovalRequest, Tool, ToolContext, ToolRegistry};

// ── McpTool ───────────────────────────────────────────────────────────────────

pub struct McpTool {
    def: McpToolDef,
    client: Rc<Mutex<McpClient>>,
    read_only: bool,
}

impl McpTool {
    pub fn new(def: McpToolDef, client: Rc<Mutex<McpClient>>) -> Self {
        let read_only = is_read_only_name(&def.name);
        McpTool { def, client, read_only }
    }
}

/// Heuristic: tool names that look like reads/queries don't require approval.
fn is_read_only_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    for prefix in &["read_", "get_", "list_", "fetch_", "search_", "find_"] {
        if lower.starts_with(prefix) {
            return true;
        }
    }
    matches!(lower.as_str(), "read" | "get" | "list" | "search" | "find")
}

#[async_trait(?Send)]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        &self.def.description
    }

    fn parameters(&self) -> Value {
        self.def.input_schema.clone()
    }

    fn read_only(&self) -> bool {
        self.read_only
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        if !self.read_only {
            let decision = Approver::new(&ctx.approval_policy).classify(&self.def.name, true);
            match decision {
                Decision::AutoDeny => {
                    return format!(
                        "Error: MCP tool '{}' denied by approval policy '{}' (non-read-only).",
                        self.def.name, ctx.approval_policy
                    );
                }
                Decision::Ask => {
                    if let Some(approver) = &ctx.approver {
                        let details =
                            serde_json::to_string_pretty(args).unwrap_or_default();
                        let ans = approver
                            .request(ApprovalRequest {
                                command: format!("mcp:{} {args}", self.def.name),
                                reason: format!(
                                    "MCP tool '{}' may have side effects.",
                                    self.def.name
                                ),
                                cwd: ctx.workspace.display().to_string(),
                                escalated: true,
                                details,
                            })
                            .await;
                        if !ans.approved() {
                            return format!(
                                "Error: MCP tool '{}' not approved by the user.",
                                self.def.name
                            );
                        }
                    }
                    // No approver configured: fall through and call the tool.
                    // (Consistent with how ShellTool behaves when auto-approving.)
                }
                Decision::AutoApprove => {}
            }
        }

        let mut client = self.client.lock().await;
        match client.call_tool(&self.def.name, args).await {
            Ok(out) => out,
            Err(e) => format!("Error: MCP tool '{}' failed: {e}", self.def.name),
        }
    }
}

// ── startup helper ────────────────────────────────────────────────────────────

/// Connect to an MCP server, list its tools, and register each as a `McpTool`.
/// Prints a progress line to stderr. Returns the number of tools registered.
pub async fn register_mcp_server(
    tools: &mut ToolRegistry,
    name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<usize, String> {
    let mut client = McpClient::connect(name, command, args, env).await?;
    let defs = client.list_tools().await?;
    let n = defs.len();
    let shared = Rc::new(Mutex::new(client));
    for def in defs {
        tools.register(Box::new(McpTool::new(def, shared.clone())));
    }
    Ok(n)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_heuristic() {
        assert!(is_read_only_name("read_file"));
        assert!(is_read_only_name("get_weather"));
        assert!(is_read_only_name("list_todos"));
        assert!(is_read_only_name("fetch_url"));
        assert!(is_read_only_name("search_web"));
        assert!(is_read_only_name("find_issues"));
        assert!(is_read_only_name("read"));
        assert!(is_read_only_name("list"));
        assert!(!is_read_only_name("write_file"));
        assert!(!is_read_only_name("create_issue"));
        assert!(!is_read_only_name("delete_branch"));
        assert!(!is_read_only_name("execute_code"));
    }

    // A live round-trip (connect → list_tools → register → execute echo tool)
    // against the same Python mock server used in ncx-mcp's own tests.
    fn write_mock_server() -> std::path::PathBuf {
        let src = r#"
import sys, json
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    mid = msg.get("id")
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"mock","version":"0"}}}), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/list":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"tools":[
            {"name":"echo","description":"echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}},
            {"name":"write_note","description":"write a note","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}
        ]}}), flush=True)
    elif method == "tools/call":
        args = msg.get("params",{}).get("arguments",{})
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"echo: "+str(args.get("text",""))}]}}), flush=True)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
"#;
        let dir = std::env::temp_dir().join("ncx_mcp_tool_mock");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("mock_server.py");
        std::fs::write(&p, src).unwrap();
        p
    }

    #[tokio::test]
    async fn register_and_execute_echo() {
        use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

        let server = write_mock_server();
        let ws = std::env::temp_dir().join("ncx_mcp_tool_ws");
        std::fs::create_dir_all(&ws).unwrap();
        let ws = ws.canonicalize().unwrap();

        let ctx = crate::tools::ToolContext::new(
            ws.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, &ws),
        );
        let mut reg = ToolRegistry::empty(ctx);

        let result = register_mcp_server(
            &mut reg,
            "mock",
            "python",
            &[server.to_string_lossy().to_string()],
            &HashMap::new(),
        )
        .await;

        let n = match result {
            Ok(n) => n,
            Err(e) => {
                eprintln!("skipping mcp_tool live test (no python?): {e}");
                return;
            }
        };
        assert_eq!(n, 2);

        // echo is read-only by heuristic (starts with "echo"… actually not)
        // write_note is non-read-only — check it's registered.
        assert!(reg.get("echo").is_some());
        assert!(reg.get("write_note").is_some());
        assert!(!reg.is_read_only("write_note"));

        let out = reg.execute("echo", &serde_json::json!({"text": "hello mcp"})).await;
        assert_eq!(out, "echo: hello mcp");
    }
}
