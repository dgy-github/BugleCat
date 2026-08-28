use crate::{ModelResponse, ProviderError, ToolCall};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    pub model: String,
    confirmed_model: RefCell<Option<String>>,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: &str, model: String, timeout_s: u64) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_s))
                .build()
                .expect("reqwest client"),
            endpoint: format!("{}/messages", base_url.trim_end_matches('/')),
            api_key,
            model,
            confirmed_model: RefCell::new(None),
        }
    }
    pub fn confirmed_model(&self) -> Option<String> {
        self.confirmed_model.borrow().clone()
    }
    pub async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
    ) -> Result<ModelResponse, ProviderError> {
        *self.confirmed_model.borrow_mut() = None;
        let (system, messages) = convert_messages(messages);
        let mut body = json!({"model": self.model, "max_tokens": 8192, "messages": messages});
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        let converted_tools = tools.iter().filter_map(convert_tool).collect::<Vec<_>>();
        if !converted_tools.is_empty() {
            body["tools"] = json!(converted_tools);
        }
        let response = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError(format!("RequestError: {e}")))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|e| ProviderError(format!("decode error: {e}")))?;
        if !status.is_success() {
            return Err(ProviderError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                value
            )));
        }
        *self.confirmed_model.borrow_mut() = value
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .map(str::to_string);
        Ok(parse_response(&value))
    }
}

fn convert_tool(tool: &Value) -> Option<Value> {
    let f = tool.get("function")?;
    Some(
        json!({"name": f.get("name")?, "description": f.get("description").cloned().unwrap_or(json!("")), "input_schema": f.get("parameters").cloned().unwrap_or(json!({"type":"object"}))}),
    )
}
fn convert_messages(messages: &[Value]) -> (String, Vec<Value>) {
    let mut system = Vec::new();
    let mut out = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "system" {
            if let Some(text) = message.get("content").and_then(Value::as_str) {
                system.push(text.to_string());
            }
            continue;
        }
        if role == "tool" {
            out.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":message.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),"content":message.get("content").cloned().unwrap_or(json!(""))}]}));
            continue;
        }
        let mut content = match message.get("content") {
            Some(Value::Array(items)) => items.clone(),
            Some(Value::String(text)) if !text.is_empty() => {
                vec![json!({"type":"text","text":text})]
            }
            _ => Vec::new(),
        };
        if role == "assistant" {
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    let f = &call["function"];
                    let input = f
                        .get("arguments")
                        .and_then(Value::as_str)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(json!({}));
                    content.push(json!({"type":"tool_use","id":call.get("id").and_then(Value::as_str).unwrap_or(""),"name":f.get("name").and_then(Value::as_str).unwrap_or(""),"input":input}));
                }
            }
        }
        if content.is_empty() {
            content.push(json!({"type":"text","text":""}));
        }
        out.push(json!({"role":role,"content":content}));
    }
    (system.join("\n\n"), out)
}
fn parse_response(value: &Value) -> ModelResponse {
    let mut response = ModelResponse::default();
    for block in value
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => response
                .content
                .push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("thinking") => response
                .reasoning
                .push_str(block.get("thinking").and_then(Value::as_str).unwrap_or("")),
            Some("tool_use") => response.tool_calls.push(ToolCall {
                id: block.get("id").and_then(Value::as_str).unwrap_or("").into(),
                name: block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                arguments: block.get("input").cloned().unwrap_or(json!({})),
            }),
            _ => {}
        }
    }
    response.finish_reason = if response.tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    }
    .into();
    let mut usage = BTreeMap::new();
    if let Some(v) = value.pointer("/usage/input_tokens").and_then(Value::as_i64) {
        usage.insert("prompt_tokens".into(), v);
    }
    if let Some(v) = value
        .pointer("/usage/output_tokens")
        .and_then(Value::as_i64)
    {
        usage.insert("completion_tokens".into(), v);
    }
    response.usage = usage;
    response
}
