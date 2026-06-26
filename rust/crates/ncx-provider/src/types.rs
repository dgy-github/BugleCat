//! Shared response types — Rust port of `nanocodex/provider/base.py`.

use std::collections::BTreeMap;

use serde_json::Value;

/// A single tool invocation requested by the model.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Parsed arguments object. Non-object / invalid JSON collapses to `{}`.
    pub arguments: Value,
}

/// Normalized result of one model call.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub reasoning: String,
    /// Token accounting (prompt/completion + DeepSeek cache split). Missing = absent.
    pub usage: BTreeMap<String, i64>,
}

impl Default for ModelResponse {
    fn default() -> Self {
        ModelResponse {
            content: String::new(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning: String::new(),
            usage: BTreeMap::new(),
        }
    }
}

impl ModelResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// Raised when the backend call fails irrecoverably — port of `ProviderError`.
#[derive(Debug, Clone)]
pub struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProviderError {}
