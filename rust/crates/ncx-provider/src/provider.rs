//! `DeepSeekProvider` — async HTTP client over `reqwest`. Rust port of the
//! `DeepSeekProvider` class in `nanocodex/provider/deepseek.py`.
//!
//! The OpenAI Python SDK gives us retries, timeouts, and SSE decoding for free;
//! here we wire those by hand on `reqwest`:
//!
//! * transient-failure retry with exponential backoff (the SDK's `max_retries`),
//! * a bounded wait for streaming response *headers* (`NANOCODEX_STREAM_OPEN_TIMEOUT_S`),
//! * Server-Sent-Events decoding for `chat_stream`.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::time::timeout;

use crate::request::{build_body, to_request_json};
use crate::response::{extract_reasoning, extract_usage, parse_args, parse_completion};
use crate::types::{ModelResponse, ProviderError, ToolCall};

const DEFAULT_STREAM_OPEN_TIMEOUT_S: u64 = 45;
const STREAM_OPEN_TIMEOUT_MIN_S: u64 = 5;
const STREAM_OPEN_TIMEOUT_MAX_S: u64 = 300;

/// Bounded override for the streaming response-header wait (seconds).
pub fn stream_open_timeout_s() -> u64 {
    stream_open_timeout_from(
        std::env::var("NANOCODEX_STREAM_OPEN_TIMEOUT_S")
            .ok()
            .as_deref(),
    )
}

/// Exponential backoff between retries: 0.5s, 1s, 2s, … capped (matches `chat`).
async fn backoff_sleep(attempt: u32) {
    let ms = 500u64 << attempt.saturating_sub(1).min(5);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// Pure inner helper — injectable for tests (mirrors `_stream_open_timeout_s`).
fn stream_open_timeout_from(raw: Option<&str>) -> u64 {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return DEFAULT_STREAM_OPEN_TIMEOUT_S;
    };
    match raw.parse::<u64>() {
        Ok(secs) => secs.clamp(STREAM_OPEN_TIMEOUT_MIN_S, STREAM_OPEN_TIMEOUT_MAX_S),
        Err(_) => DEFAULT_STREAM_OPEN_TIMEOUT_S,
    }
}

/// Talk to DeepSeek (or any OpenAI-compatible endpoint) over HTTP.
#[derive(Debug, Clone)]
pub struct DeepSeekProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    pub model: String,
    pub max_retries: u32,
}

impl DeepSeekProvider {
    /// Streaming is supported.
    pub const SUPPORTS_STREAMING: bool = true;

    pub fn new(api_key: impl Into<String>, base_url: &str, model: impl Into<String>) -> Self {
        Self::with_opts(api_key, base_url, model, 120, 3)
    }

    pub fn with_opts(
        api_key: impl Into<String>,
        base_url: &str,
        model: impl Into<String>,
        timeout_s: u64,
        max_retries: u32,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_s))
            .build()
            .expect("reqwest client builds with default (rustls) config");
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        DeepSeekProvider {
            client,
            endpoint,
            api_key: api_key.into(),
            model: model.into(),
            max_retries,
        }
    }

    fn body(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        temperature: Option<f64>,
        max_tokens: Option<i64>,
        reasoning_effort: Option<&str>,
    ) -> serde_json::Map<String, Value> {
        build_body(
            &self.model,
            messages,
            tools,
            temperature,
            max_tokens,
            reasoning_effort,
        )
    }

    /// Non-streaming completion. Retries transient failures with backoff.
    pub async fn chat(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        temperature: Option<f64>,
        max_tokens: Option<i64>,
        reasoning_effort: Option<&str>,
    ) -> Result<ModelResponse, ProviderError> {
        let kwargs = self.body(messages, tools, temperature, max_tokens, reasoning_effort);
        let payload = to_request_json(&kwargs);

        let mut attempt = 0u32;
        loop {
            match self.post(&payload).await {
                Ok(json) => return Ok(parse_completion(&json)),
                Err(e) if e.transient && attempt < self.max_retries => {
                    attempt += 1;
                    // Exponential backoff: 0.5s, 1s, 2s, … (the SDK honors
                    // Retry-After; we approximate with a capped doubling).
                    let backoff = Duration::from_millis(500u64 << (attempt - 1).min(5));
                    tokio::time::sleep(backoff).await;
                }
                Err(e) => return Err(ProviderError(e.message)),
            }
        }
    }

    async fn post(&self, payload: &Value) -> Result<Value, HttpErr> {
        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(payload)
            .send()
            .await
            .map_err(HttpErr::from_reqwest)?;

        let status = resp.status();
        if status.is_success() {
            return resp.json::<Value>().await.map_err(|e| HttpErr {
                message: format!("decode error: {e}"),
                transient: false,
            });
        }
        // 408/409/429/5xx are transient; other 4xx are permanent.
        let code = status.as_u16();
        let transient = matches!(code, 408 | 409 | 429) || (500..600).contains(&code);
        let text = resp.text().await.unwrap_or_default();
        Err(HttpErr {
            message: format!("HTTP {code}: {text}"),
            transient,
        })
    }

    /// Streaming completion. Invokes the two callbacks with incremental text and
    /// returns the aggregate response (same shape as [`chat`]).
    pub async fn chat_stream<C, R>(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        temperature: Option<f64>,
        max_tokens: Option<i64>,
        reasoning_effort: Option<&str>,
        mut on_content: C,
        mut on_reasoning: R,
    ) -> Result<ModelResponse, ProviderError>
    where
        C: FnMut(&str),
        R: FnMut(&str),
    {
        let mut kwargs = self.body(messages, tools, temperature, max_tokens, reasoning_effort);
        kwargs.insert("stream".into(), json!(true));
        kwargs.insert("stream_options".into(), json!({"include_usage": true}));
        let payload = to_request_json(&kwargs);

        let open_to = Duration::from_secs(stream_open_timeout_s());
        // Retry transient failures with backoff, matching the non-streaming path:
        // send/connect errors, header timeout, 408/409/429/5xx, and a mid-stream
        // body error that happened BEFORE any text was shown (retrying after text
        // was emitted would duplicate the visible output, so we don't).
        let mut attempt = 0u32;
        loop {
            let send_fut = self
                .client
                .post(&self.endpoint)
                .bearer_auth(&self.api_key)
                .json(&payload)
                .send();

            let resp = match timeout(open_to, send_fut).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    if attempt < self.max_retries {
                        attempt += 1;
                        backoff_sleep(attempt).await;
                        continue;
                    }
                    return Err(ProviderError(format!("RequestError: {e}")));
                }
                Err(_) => {
                    if attempt < self.max_retries {
                        attempt += 1;
                        backoff_sleep(attempt).await;
                        continue;
                    }
                    return Err(ProviderError(format!(
                        "TimeoutError: no streaming response headers after {}s. On Windows or \
                         proxy networks, try a larger NANOCODEX_STREAM_OPEN_TIMEOUT_S or check \
                         connectivity.",
                        open_to.as_secs()
                    )));
                }
            };

            if !resp.status().is_success() {
                let code = resp.status().as_u16();
                let transient = matches!(code, 408 | 409 | 429) || (500..600).contains(&code);
                let text = resp.text().await.unwrap_or_default();
                if transient && attempt < self.max_retries {
                    attempt += 1;
                    backoff_sleep(attempt).await;
                    continue;
                }
                return Err(ProviderError(format!("HTTP {code}: {text}")));
            }

            let mut emitted = false;
            match self
                .consume_sse(resp, &mut on_content, &mut on_reasoning, &mut emitted)
                .await
            {
                Ok(r) => return Ok(r),
                Err(e) => {
                    if !emitted && attempt < self.max_retries {
                        attempt += 1;
                        backoff_sleep(attempt).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    async fn consume_sse<C, R>(
        &self,
        resp: reqwest::Response,
        on_content: &mut C,
        on_reasoning: &mut R,
        emitted: &mut bool,
    ) -> Result<ModelResponse, ProviderError>
    where
        C: FnMut(&str),
        R: FnMut(&str),
    {
        let mut agg = StreamAgg::default();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    // Tell the caller whether any text already reached the UI, so
                    // it only retries a stream that emitted nothing (otherwise a
                    // retry would duplicate the visible output).
                    *emitted = !agg.content.is_empty() || !agg.reasoning.is_empty();
                    return Err(ProviderError(format!("StreamError: {e}")));
                }
            };
            buf.push_str(&String::from_utf8_lossy(&bytes));
            // SSE events are separated by blank lines; data lines begin "data:".
            while let Some(nl) = buf.find('\n') {
                let line = buf[..nl].trim_end_matches('\r').to_string();
                buf.drain(..=nl);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    return Ok(agg.finish());
                }
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    agg.ingest(&json, on_content, on_reasoning);
                }
            }
        }
        Ok(agg.finish())
    }
}

/// Accumulates streamed chunks into a [`ModelResponse`] — mirrors the aggregation
/// loop in the Python `chat_stream`.
#[derive(Default)]
struct StreamAgg {
    content: String,
    reasoning: String,
    finish_reason: String,
    usage: std::collections::BTreeMap<String, i64>,
    // tool_calls arrive as indexed fragments; aggregate by index.
    tc: std::collections::BTreeMap<i64, ToolFrag>,
}

#[derive(Default)]
struct ToolFrag {
    id: String,
    name: String,
    arguments: String,
}

impl StreamAgg {
    fn ingest<C, R>(&mut self, chunk: &Value, on_content: &mut C, on_reasoning: &mut R)
    where
        C: FnMut(&str),
        R: FnMut(&str),
    {
        if let Some(u) = chunk.get("usage") {
            if !u.is_null() {
                self.usage = extract_usage(Some(u));
            }
        }
        let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) else {
            return;
        };
        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if !fr.is_empty() {
                self.finish_reason = fr.to_string();
            }
        }
        let Some(delta) = choice.get("delta") else {
            return;
        };

        let r = extract_reasoning(delta);
        if !r.is_empty() {
            self.reasoning.push_str(&r);
            on_reasoning(&r);
        }
        if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
            if !c.is_empty() {
                self.content.push_str(c);
                on_content(c);
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
                let slot = self.tc.entry(idx).or_default();
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        slot.id = id.to_string();
                    }
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            slot.name = name.to_string();
                        }
                    }
                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                        slot.arguments.push_str(args);
                    }
                }
            }
        }
    }

    fn finish(self) -> ModelResponse {
        let mut tool_calls = Vec::new();
        for (idx, frag) in &self.tc {
            if frag.name.is_empty() {
                continue;
            }
            let id = if frag.id.is_empty() {
                format!("call_{idx}")
            } else {
                frag.id.clone()
            };
            tool_calls.push(ToolCall {
                id,
                name: frag.name.clone(),
                arguments: parse_args(&frag.arguments),
            });
        }
        ModelResponse {
            content: self.content,
            tool_calls,
            finish_reason: if self.finish_reason.is_empty() {
                "stop".to_string()
            } else {
                self.finish_reason
            },
            reasoning: self.reasoning,
            usage: self.usage,
        }
    }
}

struct HttpErr {
    message: String,
    transient: bool,
}

impl HttpErr {
    fn from_reqwest(e: reqwest::Error) -> Self {
        // Connection / timeout errors are transient and worth retrying.
        let transient = e.is_timeout() || e.is_connect() || e.is_request();
        HttpErr {
            message: format!("RequestError: {e}"),
            transient,
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_open_timeout_defaults_when_unset() {
        assert_eq!(stream_open_timeout_from(None), 45);
        assert_eq!(stream_open_timeout_from(Some("")), 45);
    }

    #[test]
    fn stream_open_timeout_honors_env_within_bounds() {
        assert_eq!(stream_open_timeout_from(Some("90")), 90);
    }

    #[test]
    fn stream_open_timeout_clamps_and_tolerates_garbage() {
        assert_eq!(stream_open_timeout_from(Some("9999")), 300);
        assert_eq!(stream_open_timeout_from(Some("0")), 5);
        assert_eq!(stream_open_timeout_from(Some("notanint")), 45);
    }

    #[test]
    fn provider_default_max_retries_is_three() {
        let p = DeepSeekProvider::new("sk-test", "https://example.invalid", "deepseek-v4-pro");
        assert_eq!(p.max_retries, 3);
    }

    #[test]
    fn provider_sets_max_retries() {
        let p = DeepSeekProvider::with_opts(
            "sk-test",
            "https://example.invalid",
            "deepseek-chat",
            120,
            5,
        );
        assert_eq!(p.max_retries, 5);
    }

    #[test]
    fn endpoint_appends_chat_completions_without_double_slash() {
        let p = DeepSeekProvider::new("k", "https://api.deepseek.com/v1/", "m");
        assert_eq!(p.endpoint, "https://api.deepseek.com/v1/chat/completions");
    }

    #[test]
    fn stream_agg_aggregates_content_and_tool_calls() {
        let mut agg = StreamAgg::default();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut on_c = |s: &str| content.push_str(s);
        let mut on_r = |s: &str| reasoning.push_str(s);

        agg.ingest(
            &json!({"choices": [{"delta": {"content": "Hel"}}]}),
            &mut on_c,
            &mut on_r,
        );
        agg.ingest(
            &json!({"choices": [{"delta": {"content": "lo"}}]}),
            &mut on_c,
            &mut on_r,
        );
        agg.ingest(
            &json!({"choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "c1", "function": {"name": "f", "arguments": "{\"a\":"}
            }]}}]}),
            &mut on_c,
            &mut on_r,
        );
        agg.ingest(
            &json!({"choices": [{"delta": {"tool_calls": [{
                "index": 0, "function": {"arguments": "1}"}
            }]}, "finish_reason": "tool_calls"}]}),
            &mut on_c,
            &mut on_r,
        );
        let r = agg.finish();
        assert_eq!(r.content, "Hello");
        assert_eq!(r.finish_reason, "tool_calls");
        assert_eq!(r.tool_calls.len(), 1);
        assert_eq!(r.tool_calls[0].name, "f");
        assert_eq!(r.tool_calls[0].arguments, json!({"a": 1}));
    }

    #[test]
    fn stream_agg_synthesizes_id_when_missing() {
        let mut agg = StreamAgg::default();
        let mut on_c = |_: &str| {};
        let mut on_r = |_: &str| {};
        agg.ingest(
            &json!({"choices": [{"delta": {"tool_calls": [{
                "index": 2, "function": {"name": "g", "arguments": "{}"}
            }]}}]}),
            &mut on_c,
            &mut on_r,
        );
        let r = agg.finish();
        assert_eq!(r.tool_calls[0].id, "call_2");
    }
}
