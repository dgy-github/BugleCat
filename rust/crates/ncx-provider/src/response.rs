//! Response parsing — Rust port of `_extract_usage`, `_extract_reasoning`, and
//! the response-decoding portion of `chat()` / `chat_stream()` in
//! `nanocodex/provider/deepseek.py`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::types::{ModelResponse, ToolCall};

/// Read a DeepSeek/OpenAI-compatible reasoning field from a JSON object.
/// Accepts both `reasoning_content` and the `reasoning` alias.
pub fn extract_reasoning(obj: &Value) -> String {
    obj.get("reasoning_content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| obj.get("reasoning").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

/// Normalize a usage object into an int map.
///
/// DeepSeek returns `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` as
/// top-level usage fields. Missing fields become 0; cache fields are only
/// recorded when the backend actually reports them.
pub fn extract_usage(usage: Option<&Value>) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    let Some(u) = usage else { return out };
    if u.is_null() {
        return out;
    }

    let get = |name: &str| -> i64 {
        match u.get(name) {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(s)) => s.parse::<i64>().unwrap_or(0),
            _ => 0,
        }
    };

    out.insert("prompt_tokens".into(), get("prompt_tokens"));
    out.insert("completion_tokens".into(), get("completion_tokens"));
    let hit = get("prompt_cache_hit_tokens");
    let miss = get("prompt_cache_miss_tokens");
    if hit != 0 || miss != 0 {
        out.insert("prompt_cache_hit_tokens".into(), hit);
        out.insert("prompt_cache_miss_tokens".into(), miss);
    }
    out
}

/// Parse a non-streaming chat-completion response JSON into a [`ModelResponse`].
pub fn parse_completion(resp: &Value) -> ModelResponse {
    let choice = resp.get("choices").and_then(|c| c.get(0));
    let msg = choice.and_then(|c| c.get("message"));

    let mut content = msg
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut reasoning = msg.map(extract_reasoning).unwrap_or_default();
    if reasoning.is_empty() {
        if let Some((think, rest)) = split_leading_think_blocks(&content) {
            reasoning = think;
            content = rest;
        }
    }

    let mut tool_calls = Vec::new();
    if let Some(tcs) = msg
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tcs {
            let func = tc.get("function");
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let raw_args = func
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let parsed = parse_args(raw_args);
            let id = tc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            tool_calls.push(ToolCall {
                id,
                name,
                arguments: parsed,
            });
        }
    }

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("stop")
        .to_string();

    let usage = extract_usage(resp.get("usage"));

    ModelResponse {
        content,
        tool_calls,
        finish_reason,
        reasoning,
        usage,
    }
}

/// Extract closed leading `<think>...</think>` blocks emitted inline inside
/// `content` by local models (Qwen3, DeepSeek-R1-Distill via llama.cpp/Ollama)
/// when the server does not populate `reasoning_content`.
pub fn split_leading_think_blocks(content: &str) -> Option<(String, String)> {
    let mut rest = content;
    let mut thoughts: Vec<String> = Vec::new();
    loop {
        let trimmed = rest.trim_start();
        if trimmed.len() < "<think></think>".len() {
            break;
        }
        let open_prefix = &trimmed[..trimmed.floor_char_boundary(7)];
        if !open_prefix.eq_ignore_ascii_case("<think>") {
            break;
        }
        let after_open = &trimmed[7..];
        let Some(close_idx) = find_ascii_case_insensitive(after_open, "</think>") else {
            break;
        };
        let thought = after_open[..close_idx].trim();
        if !thought.is_empty() {
            thoughts.push(thought.to_string());
        }
        rest = after_open[close_idx + 8..].trim_start();
    }
    if rest.len() == content.len() {
        return None;
    }
    Some((thoughts.join("\n\n"), rest.to_string()))
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let n_bytes = needle.as_bytes();
    haystack
        .as_bytes()
        .windows(n_bytes.len())
        .position(|window| window.eq_ignore_ascii_case(n_bytes))
}

/// Parse tool-call argument JSON; tolerates markdown code fences, trailing
/// commas, and double-encoded JSON strings from small local models. Non-object
/// or irrecoverable input collapses to `{}`.
pub(crate) fn parse_args(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::Object(serde_json::Map::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v @ Value::Object(_)) => return v,
        Ok(Value::String(inner)) => {
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(inner.trim()) {
                return v;
            }
        }
        _ => {}
    }
    let unfenced = strip_code_fences(trimmed);
    let repaired = strip_json_trailing_commas(unfenced);
    match serde_json::from_str::<Value>(&repaired) {
        Ok(v @ Value::Object(_)) => v,
        _ => Value::Object(serde_json::Map::new()),
    }
}

fn strip_code_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some(before_close) = after_open.strip_suffix("```") else {
        return trimmed;
    };
    let body = match before_close.find('\n') {
        Some(nl) => {
            let lang = before_close[..nl].trim();
            if lang.is_empty() || lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                &before_close[nl + 1..]
            } else {
                before_close
            }
        }
        None => before_close,
    };
    body.trim()
}

fn strip_json_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == ',' {
            let mut look = i + 1;
            while look < chars.len() && chars[look].is_whitespace() {
                look += 1;
            }
            if look < chars.len() && matches!(chars[look], '}' | ']') {
                i += 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_reasoning_accepts_reasoning_alias() {
        assert_eq!(
            extract_reasoning(&json!({"reasoning": "proxy reasoning"})),
            "proxy reasoning"
        );
    }

    #[test]
    fn extract_reasoning_prefers_reasoning_content() {
        let obj = json!({"reasoning_content": "primary", "reasoning": "alias"});
        assert_eq!(extract_reasoning(&obj), "primary");
    }

    #[test]
    fn extract_usage_captures_basic_tokens() {
        let usage = json!({"prompt_tokens": 100, "completion_tokens": 42});
        let u = extract_usage(Some(&usage));
        assert_eq!(u["prompt_tokens"], 100);
        assert_eq!(u["completion_tokens"], 42);
        assert!(!u.contains_key("prompt_cache_hit_tokens"));
    }

    #[test]
    fn extract_usage_records_cache_split_when_present() {
        let usage = json!({
            "prompt_tokens": 100, "completion_tokens": 42,
            "prompt_cache_hit_tokens": 80, "prompt_cache_miss_tokens": 20,
        });
        let u = extract_usage(Some(&usage));
        assert_eq!(u["prompt_cache_hit_tokens"], 80);
        assert_eq!(u["prompt_cache_miss_tokens"], 20);
    }

    #[test]
    fn extract_usage_none_is_empty() {
        assert!(extract_usage(None).is_empty());
        assert!(extract_usage(Some(&Value::Null)).is_empty());
    }

    #[test]
    fn parse_completion_basic_content() {
        let resp = json!({
            "choices": [{"message": {"content": "hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 1},
        });
        let r = parse_completion(&resp);
        assert_eq!(r.content, "hello");
        assert_eq!(r.finish_reason, "stop");
        assert!(!r.has_tool_calls());
        assert_eq!(r.usage["prompt_tokens"], 5);
    }

    #[test]
    fn parse_completion_tool_calls() {
        let resp = json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "read_file", "arguments": "{\"path\": \"a.txt\"}"},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
        });
        let r = parse_completion(&resp);
        assert!(r.has_tool_calls());
        assert_eq!(r.tool_calls[0].id, "call_1");
        assert_eq!(r.tool_calls[0].name, "read_file");
        assert_eq!(r.tool_calls[0].arguments["path"], json!("a.txt"));
        assert_eq!(r.finish_reason, "tool_calls");
    }

    #[test]
    fn parse_completion_bad_args_collapse_to_empty_object() {
        let resp = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "c", "function": {"name": "f", "arguments": "not json"},
                    }],
                },
            }],
        });
        let r = parse_completion(&resp);
        assert_eq!(r.tool_calls[0].arguments, json!({}));
    }

    #[test]
    fn parse_completion_defaults_finish_reason_to_stop() {
        let resp = json!({"choices": [{"message": {"content": "x"}}]});
        let r = parse_completion(&resp);
        assert_eq!(r.finish_reason, "stop");
    }

    #[test]
    fn parse_completion_extracts_inline_leading_think_blocks_when_reasoning_absent() {
        let resp = json!({
            "choices": [{
                "message": {
                    "content": "<think>\ncheck the order rule\n</think>\n\nApproved.",
                },
                "finish_reason": "stop",
            }],
        });
        let r = parse_completion(&resp);
        assert_eq!(r.reasoning, "check the order rule");
        assert_eq!(r.content, "Approved.");

        // Explicit reasoning_content is preserved untouched.
        let explicit = json!({
            "choices": [{
                "message": {
                    "reasoning_content": "native reasoning",
                    "content": "<think>literal example</think> kept",
                },
            }],
        });
        let r2 = parse_completion(&explicit);
        assert_eq!(r2.reasoning, "native reasoning");
        assert_eq!(r2.content, "<think>literal example</think> kept");
    }

    #[test]
    fn parse_args_repairs_fences_trailing_commas_and_double_encoded_json() {
        assert_eq!(
            parse_args("```json\n{\"path\": \"a,b.txt\", \"limit\": 10,}\n```"),
            json!({"path": "a,b.txt", "limit": 10})
        );
        assert_eq!(
            parse_args("\"{\\\"path\\\": \\\"nested.txt\\\"}\""),
            json!({"path": "nested.txt"})
        );
    }
}
