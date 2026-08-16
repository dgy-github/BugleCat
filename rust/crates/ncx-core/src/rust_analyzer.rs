//! Persistent rust-analyzer client implementing the pluggable LSP boundary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
use url::Url;

use crate::lsp_tool::{LspProvider, LspRequest};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// A lazily started, persistent rust-analyzer process scoped to one workspace.
pub struct RustAnalyzerProvider {
    workspace: PathBuf,
    state: Mutex<Option<LspSession>>,
}

impl RustAnalyzerProvider {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            state: Mutex::new(None),
        }
    }
}

#[async_trait(?Send)]
impl LspProvider for RustAnalyzerProvider {
    async fn request(&self, request: LspRequest) -> Result<Value, String> {
        let mut state = self.state.lock().await;
        if state.is_none() {
            *state = Some(LspSession::start(&self.workspace).await?);
        }
        let result = state
            .as_mut()
            .expect("LSP session initialized")
            .request(&self.workspace, request)
            .await;
        if result.is_err() {
            if let Some(session) = state.as_mut() {
                session.terminate();
            }
            *state = None;
        }
        result
    }
}

struct LspSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    open_documents: HashSet<String>,
}

impl LspSession {
    async fn start(workspace: &Path) -> Result<Self, String> {
        let mut child = Command::new("rust-analyzer")
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("failed to start rust-analyzer: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "rust-analyzer stdin was not piped".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "rust-analyzer stdout was not piped".to_string())?;
        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            open_documents: HashSet::new(),
        };
        let root_uri = Url::from_directory_path(workspace).map_err(|_| {
            format!(
                "cannot convert workspace to file URI: {}",
                workspace.display()
            )
        })?;
        session
            .call(
                "initialize",
                json!({
                    "processId": std::process::id(),
                    "rootUri": root_uri.as_str(),
                    "capabilities": {},
                    "clientInfo": {"name": "nanocodex", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .await?;
        session.notify("initialized", json!({})).await?;
        Ok(session)
    }

    async fn request(&mut self, workspace: &Path, request: LspRequest) -> Result<Value, String> {
        if let Some(path) = request.path.as_deref() {
            self.open_document(workspace, path).await?;
        }
        let (method, params) = request_parts(workspace, request)?;
        self.call(method, params).await
    }

    async fn open_document(&mut self, workspace: &Path, path: &str) -> Result<(), String> {
        let uri = file_uri(workspace, Some(path))?;
        let uri_text = uri.to_string();
        if self.open_documents.contains(&uri_text) {
            return Ok(());
        }
        let source_path = uri
            .to_file_path()
            .map_err(|_| format!("cannot convert source URI back to a path: {uri}"))?;
        let text = std::fs::read_to_string(&source_path).map_err(|error| {
            format!(
                "cannot read source file '{}': {error}",
                source_path.display()
            )
        })?;
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri_text,
                    "languageId": "rust",
                    "version": 1,
                    "text": text
                }
            }),
        )
        .await?;
        self.open_documents.insert(uri.to_string());
        Ok(())
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_message(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        timeout(REQUEST_TIMEOUT, self.read_response(id))
            .await
            .map_err(|_| format!("LSP request '{method}' timed out"))?
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_message(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    async fn write_message(&mut self, message: &Value) -> Result<(), String> {
        let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|error| format!("LSP write failed: {error}"))?;
        self.stdin
            .write_all(&body)
            .await
            .map_err(|error| format!("LSP write failed: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("LSP flush failed: {error}"))
    }

    async fn read_response(&mut self, expected_id: u64) -> Result<Value, String> {
        loop {
            let message = self.read_message().await?;
            if message.get("id").and_then(Value::as_u64) != Some(expected_id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("LSP server error: {error}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn read_message(&mut self) -> Result<Value, String> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|error| format!("LSP header read failed: {error}"))?;
            if read == 0 {
                return Err("rust-analyzer closed stdout".to_string());
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
        let length =
            content_length.ok_or_else(|| "LSP message omitted Content-Length".to_string())?;
        let mut body = vec![0; length];
        self.stdout
            .read_exact(&mut body)
            .await
            .map_err(|error| format!("LSP body read failed: {error}"))?;
        serde_json::from_slice(&body).map_err(|error| format!("LSP JSON decode failed: {error}"))
    }

    fn terminate(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn request_parts(workspace: &Path, request: LspRequest) -> Result<(&'static str, Value), String> {
    let position_params =
        |path: Option<String>, line: Option<u64>, column: Option<u64>| -> Result<Value, String> {
            Ok(json!({
                "textDocument": {"uri": file_uri(workspace, path.as_deref())?.to_string()},
                "position": {"line": line.unwrap_or(0), "character": column.unwrap_or(0)}
            }))
        };
    match request.operation.as_str() {
        "hover" => Ok((
            "textDocument/hover",
            position_params(request.path, request.line, request.column)?,
        )),
        "definition" => Ok((
            "textDocument/definition",
            position_params(request.path, request.line, request.column)?,
        )),
        "references" => {
            let mut params = position_params(request.path, request.line, request.column)?;
            params["context"] = json!({"includeDeclaration": true});
            Ok(("textDocument/references", params))
        }
        "document_symbols" => Ok((
            "textDocument/documentSymbol",
            json!({
                "textDocument": {"uri": file_uri(workspace, request.path.as_deref())?.to_string()}
            }),
        )),
        "diagnostics" => Ok((
            "textDocument/diagnostic",
            json!({
                "textDocument": {"uri": file_uri(workspace, request.path.as_deref())?.to_string()}
            }),
        )),
        "workspace_symbols" => Ok((
            "workspace/symbol",
            json!({"query": request.query.unwrap_or_default()}),
        )),
        operation => Err(format!("unsupported LSP operation '{operation}'")),
    }
}

fn file_uri(workspace: &Path, path: Option<&str>) -> Result<Url, String> {
    let path = path.ok_or_else(|| "source path is required".to_string())?;
    let workspace = workspace
        .canonicalize()
        .map_err(|error| format!("cannot resolve workspace: {error}"))?;
    let candidate = workspace.join(path);
    let candidate = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve source path '{path}': {error}"))?;
    if !candidate.starts_with(&workspace) {
        return Err(format!("source path is outside workspace: {path}"));
    }
    Url::from_file_path(&candidate).map_err(|_| {
        format!(
            "cannot convert source path to file URI: {}",
            candidate.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_source_paths_outside_workspace() {
        let workspace = std::env::current_dir().unwrap();
        let error = file_uri(&workspace, Some("../outside.rs")).unwrap_err();
        assert!(error.contains("cannot resolve") || error.contains("outside workspace"));
    }
}
