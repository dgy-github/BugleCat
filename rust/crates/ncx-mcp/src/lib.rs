//! ncx-mcp — a minimal Model Context Protocol (MCP) stdio client.
//!
//! Rust port of the client side of `nanocodex/tools/mcp.py`. Spawns an MCP
//! server process and talks JSON-RPC 2.0 over stdio (newline-delimited messages,
//! per the MCP stdio transport), does the `initialize` handshake, then exposes
//! `tools/list` and `tools/call`.
//!
//! Tool calls in the agent are sequential (one await at a time), so this uses a
//! simple synchronous request→read-until-matching-id loop rather than a
//! background reader + response map — much less machinery, same behavior.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

const PROTOCOL: &str = "2024-11-05";
const REQ_TIMEOUT: Duration = Duration::from_secs(30);

/// A tool advertised by an MCP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's arguments (the MCP `inputSchema`).
    pub input_schema: Value,
    /// Optional MCP tool annotations. Missing or incomplete hints are kept as
    /// `None`/partial values so the runtime can apply a fail-closed policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
}

/// Standard MCP tool annotations that influence approval decisions.
///
/// The protocol treats these as hints, not capabilities. In particular, the
/// runtime only considers a call read-only when both `readOnlyHint=true` and
/// `destructiveHint=false` are explicitly present. An absent or conflicting
/// pair therefore remains approval-gated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

impl McpToolAnnotations {
    /// Whether this annotation pair is an explicit, non-conflicting read-only
    /// declaration. Unknown/missing values intentionally return false.
    pub fn explicitly_read_only(&self) -> bool {
        self.read_only_hint == Some(true) && self.destructive_hint == Some(false)
    }
}

/// A connected MCP server (owns the child process; killed on drop).
pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    pub server: String,
}

impl McpClient {
    /// Spawn `command args` as an MCP server and complete the initialize
    /// handshake. `env` is overlaid on the inherited environment.
    pub async fn connect(
        server: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<McpClient, String> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hide_child_console(&mut cmd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| format!("spawn {command}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut client = McpClient {
            child,
            stdin,
            stdout,
            next_id: 0,
            server: server.to_string(),
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "nanocodex", "version": "0.1"},
            }),
        )
        .await?;
        self.notify("notifications/initialized", json!({})).await
    }

    async fn write_msg(&mut self, msg: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write: {e}"))?;
        self.stdin.flush().await.map_err(|e| format!("flush: {e}"))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_msg(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    /// Send a request and read responses until the one with the matching id
    /// arrives (skipping notifications / other messages). Bounded by a timeout.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.write_msg(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;

        let read = async {
            loop {
                let mut line = String::new();
                let n = self
                    .stdout
                    .read_line(&mut line)
                    .await
                    .map_err(|e| format!("read: {e}"))?;
                if n == 0 {
                    return Err(format!("server '{}' closed stdout", self.server));
                }
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if v.get("id").and_then(|x| x.as_u64()) != Some(id) {
                    continue; // a notification or a different response
                }
                if let Some(err) = v.get("error") {
                    return Err(format!("rpc error: {err}"));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        };
        match timeout(REQ_TIMEOUT, read).await {
            Ok(r) => r,
            Err(_) => Err(format!(
                "timeout waiting for '{method}' from '{}'",
                self.server
            )),
        }
    }

    /// List the server's tools.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>, String> {
        let res = self.request("tools/list", json!({})).await?;
        let mut out = Vec::new();
        if let Some(tools) = res.get("tools").and_then(|t| t.as_array()) {
            out.extend(tools.iter().filter_map(parse_tool_def));
        }
        Ok(out)
    }

    /// Call a tool and return its content as a string.
    pub async fn call_tool(&mut self, name: &str, args: &Value) -> Result<String, String> {
        let res = self
            .request("tools/call", json!({"name": name, "arguments": args}))
            .await?;
        Ok(format_content(&res))
    }
}

/// Parse one entry from an MCP `tools/list` response. Invalid or missing
/// annotation values are retained as an incomplete annotation object rather
/// than causing discovery to fail; approval then fails closed in `ncx-core`.
fn parse_tool_def(value: &Value) -> Option<McpToolDef> {
    let name = value.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let annotations = value.get("annotations").map(|raw| {
        let object = raw.as_object();
        McpToolAnnotations {
            read_only_hint: object
                .and_then(|map| map.get("readOnlyHint"))
                .and_then(Value::as_bool),
            destructive_hint: object
                .and_then(|map| map.get("destructiveHint"))
                .and_then(Value::as_bool),
            open_world_hint: object
                .and_then(|map| map.get("openWorldHint"))
                .and_then(Value::as_bool),
        }
    });
    Some(McpToolDef {
        name: name.to_string(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        input_schema: value
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object"})),
        annotations,
    })
}

#[cfg(windows)]
fn hide_child_console(command: &mut Command) {
    // MCP servers are background stdio sidecars. In a GUI application Windows
    // must not allocate a visible console for Python/Node-based servers.
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_child_console(_: &mut Command) {}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Flatten an MCP `tools/call` result into text — text blocks joined, plus any
/// `structuredContent` (mirrors the Python `format_result`). Other block types
/// are noted but not rendered.
pub fn format_content(res: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(content) = res.get("content").and_then(|c| c.as_array()) {
        for block in content {
            match block.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        parts.push(t.to_string());
                    }
                }
                Some(other) => parts.push(format!("[{other} content]")),
                None => {}
            }
        }
    }
    if let Some(sc) = res.get("structuredContent") {
        if !sc.is_null() {
            parts.push(format!("structuredContent: {sc}"));
        }
    }
    if parts.is_empty() {
        if res
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "(tool error with no content)".to_string()
        } else {
            "(no content)".to_string()
        }
    } else {
        parts.join("\n")
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        for _ in 0..16 {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "{prefix}-{}-{timestamp}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&dir) {
                Ok(()) => return dir,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory {}: {error}", dir.display()),
            }
        }
        panic!("could not allocate unique test directory for {prefix}");
    }

    #[test]
    fn format_content_joins_text_blocks() {
        let res = json!({"content": [{"type": "text", "text": "hello"}, {"type": "text", "text": "world"}]});
        assert_eq!(format_content(&res), "hello\nworld");
    }

    #[test]
    fn format_content_includes_structured() {
        let res =
            json!({"content": [{"type": "text", "text": "ok"}], "structuredContent": {"x": 1}});
        let out = format_content(&res);
        assert!(out.contains("ok"));
        assert!(out.contains("structuredContent"));
        assert!(out.contains("\"x\":1") || out.contains("\"x\": 1"));
    }

    #[test]
    fn format_content_empty_error() {
        assert_eq!(
            format_content(&json!({"content": [], "isError": true})),
            "(tool error with no content)"
        );
    }

    #[test]
    fn mcp_annotations_round_trip_with_wire_camel_case() {
        let wire = json!({
            "readOnlyHint": true,
            "destructiveHint": false,
            "openWorldHint": false,
        });
        let annotations: McpToolAnnotations =
            serde_json::from_value(wire.clone()).expect("valid MCP annotations");
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.open_world_hint, Some(false));
        assert_eq!(serde_json::to_value(&annotations).unwrap(), wire);

        let tool = parse_tool_def(&json!({
            "name": "read_file",
            "description": "read",
            "inputSchema": {"type": "object"},
            "annotations": wire,
        }))
        .expect("tool definition");
        assert_eq!(tool.annotations, Some(annotations));
    }

    #[test]
    fn malformed_or_partial_mcp_annotations_are_retained_for_fail_closed_policy() {
        let partial = parse_tool_def(&json!({
            "name": "read_file",
            "annotations": {"readOnlyHint": true},
        }))
        .expect("tool definition");
        assert_eq!(partial.annotations.unwrap().read_only_hint, Some(true));
        assert_eq!(
            parse_tool_def(&json!({
                "name": "read_file",
                "annotations": "not-an-object",
            }))
            .unwrap()
            .annotations,
            Some(McpToolAnnotations::default())
        );
    }

    // ── live end-to-end against a Python mock MCP server ──────────────────────

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
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"tools":[{"name":"echo","description":"echo text","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}]}}), flush=True)
    elif method == "tools/call":
        args = msg.get("params",{}).get("arguments",{})
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"echo: "+str(args.get("text",""))}]}}), flush=True)
    else:
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
"#;
        let dir = unique_temp_dir("ncx_mcp_mock");
        let p = dir.join("mock_server.py");
        std::fs::write(&p, src).unwrap();
        p
    }

    fn python() -> &'static str {
        // Windows installs usually expose `python`; fall back is rarely needed here.
        "python"
    }

    #[tokio::test]
    async fn connects_lists_and_calls_against_mock_server() {
        let server = write_mock_server();
        let env = HashMap::new();
        let mut client = match McpClient::connect(
            "mock",
            python(),
            &[server.to_string_lossy().to_string()],
            &env,
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                // No python on PATH — skip rather than fail the suite.
                eprintln!("skipping MCP live test (no python?): {e}");
                return;
            }
        };
        let tools = client.list_tools().await.expect("list_tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert!(tools[0].description.contains("echo"));

        let out = client
            .call_tool("echo", &json!({"text": "hi there"}))
            .await
            .expect("call_tool");
        assert_eq!(out, "echo: hi there");
    }
}
