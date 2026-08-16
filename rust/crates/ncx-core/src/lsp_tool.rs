//! Pluggable Language Server Protocol tool boundary.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRequest {
    pub operation: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub column: Option<u64>,
    pub query: Option<String>,
}

#[async_trait(?Send)]
pub trait LspProvider {
    async fn request(&self, request: LspRequest) -> Result<Value, String>;
}

pub struct LspTool;

#[async_trait(?Send)]
impl Tool for LspTool {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Query an attached Language Server for symbols, definitions, references, hover information, or diagnostics."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["hover", "definition", "references", "document_symbols", "workspace_symbols", "diagnostics"]
                },
                "path": {"type": "string", "description": "Workspace-relative source file path."},
                "line": {"type": "integer", "minimum": 0, "description": "Zero-based line."},
                "column": {"type": "integer", "minimum": 0, "description": "Zero-based UTF-16 column."},
                "query": {"type": "string", "description": "Symbol query for workspace_symbols."}
            },
            "required": ["operation"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &ToolContext, args: &Value) -> String {
        let request = match parse_request(args) {
            Ok(request) => request,
            Err(error) => return error_response("LSP_INVALID_REQUEST", &error),
        };
        let Some(provider) = &ctx.lsp_provider else {
            return error_response(
                "LSP_UNAVAILABLE",
                "No LSP provider is attached to this runtime.",
            );
        };
        match provider.request(request).await {
            Ok(result) => json!({"status": "ok", "result": result}).to_string(),
            Err(error) => error_response("LSP_REQUEST_FAILED", &error),
        }
    }
}

fn parse_request(args: &Value) -> Result<LspRequest, String> {
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation must be a string".to_string())?;
    if !matches!(
        operation,
        "hover"
            | "definition"
            | "references"
            | "document_symbols"
            | "workspace_symbols"
            | "diagnostics"
    ) {
        return Err(format!("unsupported operation '{operation}'"));
    }
    let request = LspRequest {
        operation: operation.to_string(),
        path: string_arg(args, "path")?,
        line: integer_arg(args, "line")?,
        column: integer_arg(args, "column")?,
        query: string_arg(args, "query")?,
    };
    validate_required_fields(&request)?;
    Ok(request)
}

fn string_arg(args: &Value, name: &str) -> Result<Option<String>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(Value::String(_)) => Err(format!("{name} cannot be empty")),
        Some(_) => Err(format!("{name} must be a string")),
    }
}

fn integer_arg(args: &Value, name: &str) -> Result<Option<u64>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{name} must be a non-negative integer")),
    }
}

fn validate_required_fields(request: &LspRequest) -> Result<(), String> {
    match request.operation.as_str() {
        "hover" | "definition" | "references" => {
            if request.path.is_none() || request.line.is_none() || request.column.is_none() {
                return Err("path, line, and column are required for position queries".to_string());
            }
        }
        "document_symbols" | "diagnostics" if request.path.is_none() => {
            return Err(format!("path is required for {}", request.operation));
        }
        "workspace_symbols" if request.query.is_none() => {
            return Err("query is required for workspace_symbols".to_string());
        }
        _ => {}
    }
    Ok(())
}

fn error_response(code: &str, message: &str) -> String {
    json!({"status": "error", "code": code, "message": message}).to_string()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use ncx_sandbox::SandboxPolicy;

    use super::*;

    struct MockProvider {
        seen: Rc<RefCell<Vec<LspRequest>>>,
    }

    #[async_trait(?Send)]
    impl LspProvider for MockProvider {
        async fn request(&self, request: LspRequest) -> Result<Value, String> {
            self.seen.borrow_mut().push(request);
            Ok(json!({"contents": "u32"}))
        }
    }

    fn context() -> ToolContext {
        ToolContext::new(
            PathBuf::from("."),
            SandboxPolicy::new("workspace-write", "."),
        )
    }

    #[tokio::test]
    async fn reports_unavailable_without_a_provider() {
        let result = LspTool
            .execute(
                &context(),
                &json!({"operation": "diagnostics", "path": "src/lib.rs"}),
            )
            .await;
        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["code"],
            "LSP_UNAVAILABLE"
        );
    }

    #[tokio::test]
    async fn delegates_valid_requests_to_the_provider() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let provider = Rc::new(MockProvider { seen: seen.clone() });
        let result = LspTool
            .execute(
                &context().with_lsp_provider(provider),
                &json!({"operation": "hover", "path": "src/lib.rs", "line": 4, "column": 2}),
            )
            .await;

        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["status"],
            "ok"
        );
        assert_eq!(seen.borrow()[0].operation, "hover");
    }
}
