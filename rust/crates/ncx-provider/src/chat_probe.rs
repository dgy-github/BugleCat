//! Explicit, low-cost provider chat capability probe.
//!
//! This is intentionally separate from `/models`: a relay may expose its
//! catalog while rejecting actual inference. Callers must only invoke it after
//! an explicit user action because even a one-token request can be billable.

use std::io::Read;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_PROBE_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderChatProbeRequest {
    pub base_url: String,
    pub protocol: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
}

impl ProviderChatProbeRequest {
    pub fn new(
        base_url: impl Into<String>,
        protocol: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            protocol: protocol.into(),
            api_key: api_key.into(),
            model: model.into(),
            timeout: Duration::from_secs(20),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderChatProbeResult {
    pub requested_model: String,
    pub confirmed_model: Option<String>,
    pub protocol: String,
}

pub trait ProviderChatProbeClient: Send + Sync {
    fn probe(&self, request: &ProviderChatProbeRequest) -> Result<ProviderChatProbeResult, String>;
}

#[derive(Clone, Debug, Default)]
pub struct HttpProviderChatProbeClient;

impl ProviderChatProbeClient for HttpProviderChatProbeClient {
    fn probe(&self, request: &ProviderChatProbeRequest) -> Result<ProviderChatProbeResult, String> {
        if request.api_key.trim().is_empty() {
            return Err("请先配置该模型商 Token".into());
        }
        let protocol = request.protocol.trim().to_ascii_lowercase();
        if !matches!(protocol.as_str(), "openai" | "anthropic") {
            return Err("不支持的模型协议".into());
        }
        let endpoint = chat_probe_endpoint(&request.base_url, &protocol)?;
        let client = Client::builder()
            .timeout(
                request
                    .timeout
                    .clamp(Duration::from_secs(1), Duration::from_secs(60)),
            )
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("对话探测客户端初始化失败：{error}"))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let body = if protocol == "anthropic" {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(request.api_key.trim())
                    .map_err(|_| "Token 包含无效字符".to_string())?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            json!({
                "model": request.model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "Reply OK"}]
            })
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", request.api_key.trim()))
                    .map_err(|_| "Token 包含无效字符".to_string())?,
            );
            json!({
                "model": request.model,
                "max_tokens": 1,
                "stream": false,
                "messages": [{"role": "user", "content": "Reply OK"}]
            })
        };
        let response = client
            .post(endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .map_err(|error| format!("对话接口连接失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!("对话接口返回 HTTP {}", response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PROBE_RESPONSE_BYTES as u64)
        {
            return Err("对话探测响应超过 256 KiB 限制".into());
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_PROBE_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("读取对话探测响应失败：{error}"))?;
        if bytes.len() > MAX_PROBE_RESPONSE_BYTES {
            return Err("对话探测响应超过 256 KiB 限制".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "对话接口未返回有效 JSON".to_string())?;
        let valid_shape = if protocol == "anthropic" {
            value.get("content").and_then(Value::as_array).is_some()
        } else {
            value.get("choices").and_then(Value::as_array).is_some()
        };
        if !valid_shape {
            return Err("对话接口响应格式与所选协议不匹配".into());
        }
        Ok(ProviderChatProbeResult {
            requested_model: request.model.clone(),
            confirmed_model: value
                .get("model")
                .and_then(Value::as_str)
                .filter(|model| !model.trim().is_empty())
                .map(str::to_string),
            protocol,
        })
    }
}

pub fn chat_probe_endpoint(base_url: &str, protocol: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base_url.trim()).map_err(|_| "Base URL 无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Base URL 只支持 HTTP/HTTPS".into());
    }
    url.set_query(None);
    url.set_fragment(None);
    let suffix = if protocol == "anthropic" {
        "messages"
    } else {
        "chat/completions"
    };
    let current = url.path().trim_end_matches('/');
    if !current.ends_with(suffix) {
        url.set_path(&format!("{current}/{suffix}"));
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn server(response: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = vec![0; 16 * 1024];
            let read = stream.read(&mut bytes).unwrap();
            let _ = tx.send(String::from_utf8_lossy(&bytes[..read]).to_string());
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}/v1"), rx)
    }

    fn response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn openai_probe_uses_chat_completions_and_reports_confirmed_model() {
        let reply = Box::leak(
            response(r#"{"model":"server-model","choices":[{"message":{"content":"OK"}}]}"#)
                .into_boxed_str(),
        );
        let (base_url, request) = server(reply);
        let result = HttpProviderChatProbeClient
            .probe(&ProviderChatProbeRequest::new(
                base_url,
                "openai",
                "probe-secret",
                "requested-model",
            ))
            .unwrap();
        let request = request.recv().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer probe-secret"));
        assert!(request.contains("\"max_tokens\":1"));
        assert_eq!(result.confirmed_model.as_deref(), Some("server-model"));
    }

    #[test]
    fn anthropic_probe_uses_messages_headers_and_shape() {
        let reply = Box::leak(
            response(r#"{"model":"claude-confirmed","content":[{"type":"text","text":"O"}]}"#)
                .into_boxed_str(),
        );
        let (base_url, request) = server(reply);
        let result = HttpProviderChatProbeClient
            .probe(&ProviderChatProbeRequest::new(
                base_url,
                "anthropic",
                "anthropic-secret",
                "claude-requested",
            ))
            .unwrap();
        let request = request.recv().unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /v1/messages http/1.1"));
        assert!(request.contains("x-api-key: anthropic-secret"));
        assert!(request.contains("anthropic-version: 2023-06-01"));
        assert_eq!(result.confirmed_model.as_deref(), Some("claude-confirmed"));
    }

    #[test]
    fn failed_probe_exposes_only_status_not_provider_body_or_token() {
        let (base_url, _) = server(
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 31\r\nConnection: close\r\n\r\nsecret-body probe-secret token",
        );
        let error = HttpProviderChatProbeClient
            .probe(&ProviderChatProbeRequest::new(
                base_url,
                "openai",
                "probe-secret",
                "requested-model",
            ))
            .unwrap_err();
        assert_eq!(error, "对话接口返回 HTTP 403");
        assert!(!error.contains("secret-body"));
        assert!(!error.contains("probe-secret"));
    }
}
