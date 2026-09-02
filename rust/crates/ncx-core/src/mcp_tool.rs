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
use ncx_mcp::{McpClient, McpToolAnnotations, McpToolDef};
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
        // MCP names are model-facing labels, not a trustworthy authority
        // boundary. Only an explicit, non-conflicting protocol declaration can
        // make an arbitrary server tool eligible for the read-only path.
        let read_only = annotation_declares_read_only(def.annotations.as_ref());
        McpTool {
            def,
            client,
            read_only,
        }
    }
}

fn annotation_declares_read_only(annotations: Option<&ncx_mcp::McpToolAnnotations>) -> bool {
    annotations.is_some_and(ncx_mcp::McpToolAnnotations::explicitly_read_only)
}

fn mcp_call_is_read_only(
    name: &str,
    annotations: Option<&McpToolAnnotations>,
    tool_is_read_only: bool,
    args: &Value,
) -> bool {
    // LLM Wiki deliberately multiplexes reads and mutations behind one tool
    // name. Even a server-level read-only annotation must not bless a write
    // action; the repository-owned action allowlist remains the narrowest
    // authority for this special tool.
    if name == "llmwiki" {
        if annotations.is_some() && !tool_is_read_only {
            return false;
        }
        return matches!(
            args.get("action").and_then(Value::as_str),
            Some("recall_user" | "recall_project" | "project_status" | "status" | "corpus")
        );
    }
    if tool_is_read_only {
        return true;
    }
    // An explicit annotation object that is missing either required hint (or
    // contains a conflicting pair) is never upgraded by a local heuristic.
    if annotations.is_some() {
        return false;
    }
    // No name-based fallback follows. Missing, malformed, and conflicting
    // annotations all left `tool_is_read_only` false at construction time.
    false
}

fn approval_denied_message(name: &str, approval_policy: &str) -> String {
    format!(
        "Error: MCP tool '{name}' denied by approval policy '{approval_policy}' (non-read-only)."
    )
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

    fn call_is_read_only(&self, args: &Value) -> bool {
        mcp_call_is_read_only(
            &self.def.name,
            self.def.annotations.as_ref(),
            self.read_only,
            args,
        )
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        if !self.call_is_read_only(args) {
            let decision = Approver::new(&ctx.approval_policy).classify(&self.def.name, true);
            match decision {
                Decision::AutoDeny => {
                    return approval_denied_message(&self.def.name, &ctx.approval_policy);
                }
                Decision::Ask => {
                    let Some(approver) = &ctx.approver else {
                        return format!(
                            "Error: MCP tool '{}' requires approval but no approver is configured.",
                            self.def.name
                        );
                    };
                    let details = serde_json::to_string_pretty(args).unwrap_or_default();
                    let ans = approver
                        .request(ApprovalRequest {
                            command: format!("mcp:{} {args}", self.def.name),
                            reason: format!("MCP tool '{}' may have side effects.", self.def.name),
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

/// Connect to one MCP server and prepare its tools without mutating a registry.
///
/// The returned tools keep the server process alive. Callers can prepare every
/// configured server first and only commit them after all connections and name
/// validation succeed, which prevents a failed reload from dropping live tools.
pub async fn prepare_mcp_server_tools(
    name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<Vec<Box<dyn Tool>>, String> {
    let mut client = McpClient::connect(name, command, args, env).await?;
    let defs = client.list_tools().await?;
    let shared = Rc::new(Mutex::new(client));
    Ok(defs
        .into_iter()
        .map(|def| Box::new(McpTool::new(def, shared.clone())) as Box<dyn Tool>)
        .collect())
}

/// Connect to an MCP server, list its tools, and register each as a `McpTool`.
/// Returns the number of tools registered.
pub async fn register_mcp_server(
    tools: &mut ToolRegistry,
    name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<usize, String> {
    let prepared = prepare_mcp_server_tools(name, command, args, env).await?;
    let count = prepared.len();
    for tool in prepared {
        tools.register(tool);
    }
    Ok(count)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_only_admit_explicit_non_destructive_reads() {
        let safe = ncx_mcp::McpToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            open_world_hint: None,
        };
        assert!(annotation_declares_read_only(Some(&safe)));

        for annotations in [
            None,
            Some(ncx_mcp::McpToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: None,
                open_world_hint: None,
            }),
            Some(ncx_mcp::McpToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: Some(true),
                open_world_hint: None,
            }),
            Some(ncx_mcp::McpToolAnnotations {
                read_only_hint: Some(false),
                destructive_hint: Some(false),
                open_world_hint: None,
            }),
        ] {
            assert!(!annotation_declares_read_only(annotations.as_ref()));
        }
    }

    #[test]
    fn llmwiki_read_actions_bypass_approval_but_mutations_do_not() {
        for action in [
            "recall_user",
            "recall_project",
            "project_status",
            "status",
            "corpus",
        ] {
            assert!(mcp_call_is_read_only(
                "llmwiki",
                None,
                false,
                &serde_json::json!({"action": action})
            ));
        }
        for action in ["initialize_project", "record_project", "propose", "approve"] {
            assert!(!mcp_call_is_read_only(
                "llmwiki",
                None,
                false,
                &serde_json::json!({"action": action})
            ));
        }
        assert!(!mcp_call_is_read_only(
            "llmwiki",
            None,
            false,
            &serde_json::json!({})
        ));
        let incomplete = ncx_mcp::McpToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: None,
            open_world_hint: None,
        };
        assert!(!mcp_call_is_read_only(
            "llmwiki",
            Some(&incomplete),
            false,
            &serde_json::json!({"action": "recall_user"})
        ));
        let safe = ncx_mcp::McpToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            open_world_hint: None,
        };
        assert!(mcp_call_is_read_only(
            "llmwiki",
            Some(&safe),
            true,
            &serde_json::json!({"action": "recall_user"})
        ));
        assert!(!mcp_call_is_read_only(
            "llmwiki",
            Some(&safe),
            true,
            &serde_json::json!({"action": "record_project"})
        ));
    }

    #[test]
    fn never_policy_denies_read_named_tools_without_safe_annotations() {
        let destructive = ncx_mcp::McpToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(true),
            open_world_hint: None,
        };
        for annotations in [Some(&destructive), None] {
            assert!(!mcp_call_is_read_only(
                "read_file",
                annotations,
                annotation_declares_read_only(annotations),
                &serde_json::json!({}),
            ));
            assert!(matches!(
                Approver::new("never").classify("read_file", true),
                Decision::AutoDeny
            ));
        }

        let safe = ncx_mcp::McpToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            open_world_hint: None,
        };
        assert!(mcp_call_is_read_only(
            "read_file",
            Some(&safe),
            annotation_declares_read_only(Some(&safe)),
            &serde_json::json!({}),
        ));
    }

    #[test]
    fn never_policy_denies_side_effecting_llmwiki_actions_without_a_live_server() {
        assert!(!mcp_call_is_read_only(
            "llmwiki",
            None,
            false,
            &serde_json::json!({"action": "record_project"}),
        ));
        assert!(matches!(
            Approver::new("never").classify("llmwiki", true),
            Decision::AutoDeny
        ));
        assert!(mcp_call_is_read_only(
            "llmwiki",
            None,
            false,
            &serde_json::json!({"action": "recall_user"}),
        ));
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
            {"name":"echo","description":"echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}},"annotations":{"readOnlyHint":True,"destructiveHint":False}},
            {"name":"read_safe","description":"annotated read","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":True,"destructiveHint":False}},
            {"name":"read_destructive","description":"misleading destructive read","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":True,"destructiveHint":True}},
            {"name":"read_unannotated","description":"unannotated read","inputSchema":{"type":"object","properties":{}}},
            {"name":"write_note","description":"write a note","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}},
            {"name":"llmwiki","description":"memory actions","inputSchema":{"type":"object","properties":{"action":{"type":"string"}}},"annotations":{"readOnlyHint":True,"destructiveHint":False}}
        ]}}), flush=True)
    elif method == "tools/call":
        args = msg.get("params",{}).get("arguments",{})
        name = msg.get("params",{}).get("name","")
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"called: "+name+": "+str(args.get("text",""))}]}}), flush=True)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
"#;
        let dir = crate::test_support::unique_temp_dir("ncx_mcp_tool_mock");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("mock_server.py");
        std::fs::write(&p, src).unwrap();
        p
    }

    #[tokio::test]
    async fn register_and_execute_echo() {
        use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};

        let server = write_mock_server();
        let ws = crate::test_support::unique_temp_dir("ncx_mcp_tool_ws");
        std::fs::create_dir_all(&ws).unwrap();
        let ws = ws.canonicalize().unwrap();

        let ctx =
            crate::tools::ToolContext::new(ws.clone(), SandboxPolicy::new(WORKSPACE_WRITE, &ws));
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
            Err(e) if e.starts_with("spawn python:") => {
                eprintln!("skipping mcp_tool live test (no python?): {e}");
                return;
            }
            Err(e) => panic!("MCP live server failed unexpectedly: {e}"),
        };
        assert_eq!(n, 6);

        // Only a complete MCP annotation opts a tool into the read-only path.
        assert!(reg.get("echo").is_some());
        assert!(reg.is_read_only("read_safe"));
        assert!(!reg.is_read_only("read_destructive"));
        assert!(!reg.is_read_only("read_unannotated"));
        assert!(reg.get("write_note").is_some());
        assert!(!reg.is_read_only("write_note"));
        assert!(reg.call_is_read_only("llmwiki", &serde_json::json!({"action": "recall_user"})));
        assert!(!reg.call_is_read_only("llmwiki", &serde_json::json!({"action": "record_project"})));

        let out = reg
            .execute("echo", &serde_json::json!({"text": "hello mcp"}))
            .await;
        assert_eq!(out, "called: echo: hello mcp");

        reg.ctx.approval_policy = "never".into();
        reg.ctx.compaction_read_only_recovery.set(false);
        let safe = reg.execute("read_safe", &serde_json::json!({})).await;
        assert_eq!(safe, "called: read_safe: ");
        for name in ["read_destructive", "read_unannotated"] {
            let denied = reg.execute(name, &serde_json::json!({})).await;
            assert!(
                denied.contains("denied by approval policy 'never'"),
                "{name}: {denied}"
            );
        }
        let recalled = reg
            .execute("llmwiki", &serde_json::json!({"action": "recall_user"}))
            .await;
        assert!(!recalled.contains("context compaction consistency check"));
        assert!(!recalled.contains("denied by approval policy"));
        let mutation = reg
            .execute("llmwiki", &serde_json::json!({"action": "record_project"}))
            .await;
        assert!(
            mutation.contains("denied by approval policy 'never'"),
            "{mutation}"
        );
    }
}
