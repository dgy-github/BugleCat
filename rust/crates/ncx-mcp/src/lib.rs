//! ncx-mcp — a minimal Model Context Protocol (MCP) stdio client.
//!
//! Rust port of the client side of `nanocodex/tools/mcp.py`. Spawns an MCP
//! server process and talks JSON-RPC 2.0 over stdio (newline-delimited messages,
//! per the MCP stdio transport), does the `initialize` handshake, then exposes
//! `tools/list` and `tools/call`.
//!
//! A background stdout reader routes responses by JSON-RPC request id. Public
//! request methods borrow the client immutably, so one server connection can
//! safely service several in-flight calls; the host still controls the server
//! concurrency budget at the tool layer.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;

const PROTOCOL: &str = "2024-11-05";
const REQ_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of simultaneous read-only calls sent to one MCP process.
/// A side-effecting call acquires all permits and is therefore exclusive.
pub const MCP_SERVER_MAX_READ_CONCURRENCY: usize = 4;

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

type RequestResult = Result<Value, String>;
type PendingRequests = Arc<StdMutex<HashMap<u64, oneshot::Sender<RequestResult>>>>;
type SharedStdin = Arc<tokio::sync::Mutex<ChildStdin>>;

/// Removes a request from the router when the waiter is cancelled or dropped.
/// Without this guard, a cancelled tool call would leave a sender retained
/// until the server eventually answered (or the connection closed).
struct PendingRequestGuard {
    pending: PendingRequests,
    id: u64,
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        remove_pending(&self.pending, self.id);
    }
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
    stdin: SharedStdin,
    pending: PendingRequests,
    next_id: AtomicU64,
    call_gate: Arc<Semaphore>,
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
        let stdin: SharedStdin = Arc::new(tokio::sync::Mutex::new(stdin));
        let pending: PendingRequests = Arc::new(StdMutex::new(HashMap::new()));
        let reader_pending = Arc::clone(&pending);
        let reader_stdin = Arc::clone(&stdin);
        let server_name = server.to_string();
        tokio::spawn(async move {
            let mut stdout = stdout;
            loop {
                let mut line = String::new();
                let result = stdout.read_line(&mut line).await;
                match result {
                    Ok(0) => {
                        fail_pending(
                            &reader_pending,
                            format!("server '{}' closed stdout", server_name),
                        );
                        break;
                    }
                    Ok(_) => {
                        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
                            // Stdio servers occasionally emit blank lines. A malformed
                            // line has no trustworthy id, so leave the request pending
                            // and let its bounded timeout produce the diagnostic.
                            continue;
                        };

                        if let Some(method) = value.get("method").and_then(Value::as_str) {
                            // MCP permits a server to send a JSON-RPC request back to
                            // the client (sampling/elicitation, for example). We do not
                            // expose those capabilities yet, but must answer explicitly
                            // instead of silently leaving the server blocked.
                            let Some(id) = value.get("id") else {
                                continue;
                            };
                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": {
                                    "code": -32601,
                                    "message": format!("method '{method}' is not supported by ncx-mcp")
                                }
                            });
                            if let Err(error) = write_json(&reader_stdin, &response).await {
                                fail_pending(&reader_pending, error);
                                break;
                            }
                            continue;
                        }

                        let Some(id) = value.get("id").and_then(Value::as_u64) else {
                            // Notifications do not carry a response id.
                            continue;
                        };
                        let result = if let Some(error) = value.get("error") {
                            Err(format!("rpc error: {error}"))
                        } else {
                            Ok(value.get("result").cloned().unwrap_or(Value::Null))
                        };
                        if let Some(sender) = take_pending(&reader_pending, id) {
                            let _ = sender.send(result);
                        }
                    }
                    Err(error) => {
                        fail_pending(&reader_pending, format!("read: {error}"));
                        break;
                    }
                }
            }
        });
        let client = McpClient {
            child,
            stdin,
            pending,
            next_id: AtomicU64::new(0),
            call_gate: Arc::new(Semaphore::new(MCP_SERVER_MAX_READ_CONCURRENCY)),
            server: server.to_string(),
        };
        client.initialize().await?;
        Ok(client)
    }

    async fn initialize(&self) -> Result<(), String> {
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

    async fn write_msg(&self, msg: &Value) -> Result<(), String> {
        write_json(&self.stdin, msg).await
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.write_msg(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    /// Send a request and wait for the response routed by its request id.
    /// Bounded by a timeout and removes abandoned requests from the pending map.
    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = next_request_id(&self.next_id);
        let (tx, rx) = oneshot::channel();
        insert_pending(&self.pending, id, tx)?;
        let _pending_guard = PendingRequestGuard {
            pending: Arc::clone(&self.pending),
            id,
        };
        if let Err(error) = self
            .write_msg(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }))
            .await
        {
            remove_pending(&self.pending, id);
            return Err(error);
        }
        match timeout(REQ_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(format!("server '{}' closed stdout", self.server)),
            Err(_) => Err(format!(
                "timeout waiting for '{method}' from '{}'",
                self.server
            )),
        }
    }

    /// List the server's tools.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, String> {
        let res = self.request("tools/list", json!({})).await?;
        let mut out = Vec::new();
        if let Some(tools) = res.get("tools").and_then(|t| t.as_array()) {
            out.extend(tools.iter().filter_map(parse_tool_def));
        }
        Ok(out)
    }

    /// Call a tool and return its content as a string.
    pub async fn call_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        let res = self
            .request("tools/call", json!({"name": name, "arguments": args}))
            .await?;
        Ok(format_content(&res))
    }

    /// Reserve capacity for one tool call on this server. Read-only calls use
    /// one permit and can run in a bounded batch; side-effecting calls use the
    /// full capacity and therefore cannot overlap with any other call.
    pub async fn acquire_call_permit(
        &self,
        read_only: bool,
    ) -> Result<OwnedSemaphorePermit, String> {
        acquire_call_permit(&self.call_gate, &self.server, read_only).await
    }
}

async fn acquire_call_permit(
    gate: &Arc<Semaphore>,
    server: &str,
    read_only: bool,
) -> Result<OwnedSemaphorePermit, String> {
    let count = if read_only {
        1
    } else {
        MCP_SERVER_MAX_READ_CONCURRENCY
    };
    gate.clone()
        .acquire_many_owned(count as u32)
        .await
        .map_err(|_| format!("MCP server '{server}' call gate is closed"))
}

async fn write_json(stdin: &SharedStdin, msg: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut writer = stdin.lock().await;
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;
    writer.flush().await.map_err(|e| format!("flush: {e}"))
}

fn next_request_id(counter: &AtomicU64) -> u64 {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.wrapping_add(1).max(1))
        })
        .unwrap_or(1)
}

fn insert_pending(
    pending: &PendingRequests,
    id: u64,
    sender: oneshot::Sender<RequestResult>,
) -> Result<(), String> {
    let mut entries = pending
        .lock()
        .map_err(|_| "MCP response router lock poisoned".to_string())?;
    entries.insert(id, sender);
    Ok(())
}

fn remove_pending(pending: &PendingRequests, id: u64) {
    if let Ok(mut entries) = pending.lock() {
        entries.remove(&id);
    }
}

fn take_pending(pending: &PendingRequests, id: u64) -> Option<oneshot::Sender<RequestResult>> {
    pending.lock().ok()?.remove(&id)
}

fn fail_pending(pending: &PendingRequests, error: String) {
    let senders = pending
        .lock()
        .map(|mut entries| {
            entries
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for sender in senders {
        let _ = sender.send(Err(error.clone()));
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
        fail_pending(
            &self.pending,
            format!("MCP server '{}' connection dropped", self.server),
        );
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
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Barrier;

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

    #[test]
    fn dropping_a_cancelled_request_removes_its_pending_sender() {
        let pending: PendingRequests = Arc::new(StdMutex::new(HashMap::new()));
        let (tx, _rx) = oneshot::channel();
        insert_pending(&pending, 7, tx).expect("insert pending request");
        {
            let _guard = PendingRequestGuard {
                pending: Arc::clone(&pending),
                id: 7,
            };
            assert_eq!(pending.lock().unwrap().len(), 1);
        }
        assert!(pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn call_gate_bounds_reads_and_excludes_writes() {
        let gate = Arc::new(Semaphore::new(MCP_SERVER_MAX_READ_CONCURRENCY));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(Barrier::new(MCP_SERVER_MAX_READ_CONCURRENCY + 1));
        let release = Arc::new(Barrier::new(MCP_SERVER_MAX_READ_CONCURRENCY + 1));
        let writer_started = Arc::new(AtomicBool::new(false));

        async fn held_read(
            gate: Arc<Semaphore>,
            active: Arc<AtomicUsize>,
            peak: Arc<AtomicUsize>,
            ready: Arc<Barrier>,
            release: Arc<Barrier>,
        ) {
            let _permit = acquire_call_permit(&gate, "test", true)
                .await
                .expect("read permit");
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            ready.wait().await;
            release.wait().await;
            active.fetch_sub(1, Ordering::SeqCst);
        }

        let writer = {
            let gate = gate.clone();
            let writer_started = writer_started.clone();
            async move {
                let _permit = acquire_call_permit(&gate, "test", false)
                    .await
                    .expect("write permit");
                writer_started.store(true, Ordering::SeqCst);
            }
        };
        let coordinator = {
            let ready = ready.clone();
            let release = release.clone();
            let writer_started = writer_started.clone();
            async move {
                ready.wait().await;
                assert!(!writer_started.load(Ordering::SeqCst));
                release.wait().await;
            }
        };

        let reads = std::array::from_fn(|_| {
            held_read(
                gate.clone(),
                active.clone(),
                peak.clone(),
                ready.clone(),
                release.clone(),
            )
        });
        let [read_0, read_1, read_2, read_3] = reads;
        tokio::join!(read_0, read_1, read_2, read_3, writer, coordinator);

        assert_eq!(peak.load(Ordering::SeqCst), MCP_SERVER_MAX_READ_CONCURRENCY);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(writer_started.load(Ordering::SeqCst));
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

    fn write_script_server(prefix: &str, source: &str) -> std::path::PathBuf {
        let dir = unique_temp_dir(prefix);
        let path = dir.join("server.py");
        fs::write(&path, source).unwrap();
        path
    }

    fn python() -> &'static str {
        // Windows installs usually expose `python`; fall back is rarely needed here.
        "python"
    }

    #[tokio::test]
    async fn connects_lists_and_calls_against_mock_server() {
        let server = write_mock_server();
        let env = HashMap::new();
        let client = match McpClient::connect(
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

    #[tokio::test]
    async fn routes_concurrent_out_of_order_responses_to_the_matching_call() {
        let server = write_script_server(
            "ncx_mcp_out_of_order",
            r#"
import json, sys
calls = []
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/call":
        calls.append(msg)
        if len(calls) == 2:
            for call in reversed(calls):
                name = call["params"]["name"]
                print(json.dumps({"jsonrpc":"2.0","id":call["id"],"result":{"content":[{"type":"text","text":name}]}}), flush=True)
    elif mid is not None:
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
            "#,
        );
        let client = McpClient::connect(
            "out-of-order",
            python(),
            &[server.to_string_lossy().to_string()],
            &HashMap::new(),
        )
        .await
        .expect("mock server must start");

        let empty_args = json!({});
        let (first, second) = tokio::join!(
            client.call_tool("first", &empty_args),
            client.call_tool("second", &empty_args),
        );
        assert_eq!(first.expect("first response"), "first");
        assert_eq!(second.expect("second response"), "second");
    }

    #[tokio::test]
    async fn fails_pending_calls_when_server_closes_stdout() {
        let server = write_script_server(
            "ncx_mcp_eof",
            r#"
import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":msg.get("id"),"result":{}}), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/call":
        break
            "#,
        );
        let client = McpClient::connect(
            "eof",
            python(),
            &[server.to_string_lossy().to_string()],
            &HashMap::new(),
        )
        .await
        .expect("mock server must start");
        let error = client
            .call_tool("closes", &json!({}))
            .await
            .expect_err("closed stdout must fail the pending call");
        assert!(error.contains("closed stdout"), "{error}");
    }

    #[tokio::test]
    async fn answers_unsupported_server_requests_without_blocking_the_call() {
        let server = write_script_server(
            "ncx_mcp_server_request",
            r#"
import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{}}), flush=True)
    elif method == "notifications/initialized":
        pass
    elif method == "tools/call":
        print(json.dumps({"jsonrpc":"2.0","id":99,"method":"sampling/createMessage","params":{}}), flush=True)
        reply = json.loads(next(sys.stdin))
        if reply.get("error", {}).get("code") != -32601:
            raise SystemExit(2)
        print(json.dumps({"jsonrpc":"2.0","id":mid,"result":{"content":[{"type":"text","text":"ok"}]}}), flush=True)
            "#,
        );
        let client = McpClient::connect(
            "server-request",
            python(),
            &[server.to_string_lossy().to_string()],
            &HashMap::new(),
        )
        .await
        .expect("mock server must start");
        let output = client
            .call_tool("requesting", &json!({}))
            .await
            .expect("normal response after unsupported request");
        assert_eq!(output, "ok");
    }
}
