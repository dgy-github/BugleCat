//! Recover text-serialized tool calls (`<｜DSML｜calls>` or `<tool_call>`)
//! that a model wrote into `content` instead of native `tool_calls`.
//!
//! DeepSeek models occasionally serialize tool calls in DSML markup, while
//! local Qwen / Llama / Hermes models on llama.cpp, Ollama, or vLLM frequently
//! emit `<tool_call>{"name": ..., "arguments": ...}</tool_call>` blocks in
//! `content`. Well-formed blocks naming registered tools with all required
//! parameters are turned back into native calls so they pass through the exact
//! same sandbox, approval, middleware, and Jev review pipeline. Anything
//! malformed is left to the caller's bounded retry path.

use std::sync::OnceLock;

use ncx_provider::ToolCall;
use regex::Regex;
use serde_json::{Map, Value};

fn tag_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"<\s*(/)?[\s|｜]*DSML[\s|｜]*(function_calls|calls|invoke|parameter)\b([^>]*)>")
            .expect("valid DSML tag pattern")
    })
}

fn attribute(attributes: &str, name: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r#"\b{name}\s*=\s*"([^"]*)""#)).ok()?;
    pattern
        .captures(attributes)
        .map(|captures| captures[1].to_string())
}

/// True when `content` looks like an inline DSML or `<tool_call>` serialization
/// outside fenced Markdown code blocks.
pub(super) fn looks_like_text_tool_calls(content: &str) -> bool {
    let compact: String = content
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '|' && *ch != '｜')
        .take(1_024)
        .collect();
    if compact.contains("<DSMLcalls>") && compact.contains("<DSMLinvokename=") {
        return true;
    }
    let unfenced = mask_fenced_code_blocks(content);
    let lower = unfenced.to_ascii_lowercase();
    if let Some(pos) = lower.find("<tool_call>") {
        let tail = &lower[pos + "<tool_call>".len()..];
        return tail.contains("</tool_call>") || (tail.contains('{') && tail.contains("\"name\""));
    }
    false
}

/// Recover tool calls from either DSML or `<tool_call>...</tool_call>` blocks,
/// returning `(calls, preamble)` only when every call is well-formed and
/// accepted by `schemas`.
pub(super) fn recover_calls(
    content: &str,
    schemas: &[Value],
    id_prefix: &str,
) -> Option<(Vec<ToolCall>, String)> {
    recover_dsml_calls(content, schemas, id_prefix)
        .or_else(|| recover_xml_tool_calls(content, schemas, id_prefix))
}

fn recover_dsml_calls(
    content: &str,
    schemas: &[Value],
    id_prefix: &str,
) -> Option<(Vec<ToolCall>, String)> {
    let tags: Vec<_> = tag_pattern().captures_iter(content).collect();
    let first = tags.first()?;
    let opening = first.get(0)?;
    if first.get(1).is_some() || !matches!(&first[2], "calls" | "function_calls") {
        return None;
    }
    let preamble = content[..opening.start()].trim().to_string();

    let mut calls: Vec<ToolCall> = Vec::new();
    let mut current: Option<(String, Map<String, Value>)> = None;
    let mut parameter: Option<(String, bool, usize)> = None;
    let mut closed = false;
    for tag in tags.iter().skip(1) {
        let whole = tag.get(0)?;
        let closing = tag.get(1).is_some();
        let attributes = tag.get(3).map_or("", |m| m.as_str());
        match (&tag[2], closing) {
            ("invoke", false) => {
                if current.is_some() || parameter.is_some() {
                    return None;
                }
                let name = attribute(attributes, "name")?;
                current = Some((name.trim().to_string(), Map::new()));
            }
            ("parameter", false) => {
                if current.is_none() || parameter.is_some() {
                    return None;
                }
                let name = attribute(attributes, "name")?;
                let is_string = attribute(attributes, "string").as_deref() != Some("false");
                parameter = Some((name, is_string, whole.end()));
            }
            ("parameter", true) => {
                let (name, is_string, start) = parameter.take()?;
                let raw = &content[start..whole.start()];
                let value = if is_string {
                    Value::String(raw.strip_prefix('\n').unwrap_or(raw).to_string())
                } else {
                    serde_json::from_str(raw.trim()).ok()?
                };
                current.as_mut()?.1.insert(name, value);
            }
            ("invoke", true) => {
                if parameter.is_some() {
                    return None;
                }
                let (name, arguments) = current.take()?;
                calls.push(ToolCall {
                    id: format!("{id_prefix}-{}", calls.len()),
                    name,
                    arguments: Value::Object(arguments),
                });
            }
            ("calls" | "function_calls", true) => {
                if current.is_some() || parameter.is_some() {
                    return None;
                }
                closed = true;
                break;
            }
            _ => return None,
        }
    }
    if !closed || calls.is_empty() {
        return None;
    }
    calls
        .iter()
        .all(|call| accepts(schemas, call))
        .then_some((calls, preamble))
}

/// Recover Qwen / Hermes / Llama `<tool_call>{...}</tool_call>` blocks emitted
/// outside Markdown code fences.
fn recover_xml_tool_calls(
    content: &str,
    schemas: &[Value],
    id_prefix: &str,
) -> Option<(Vec<ToolCall>, String)> {
    let masked = mask_fenced_code_blocks(content);
    let lower = masked.to_ascii_lowercase();
    let first_open = lower.find("<tool_call>")?;
    let preamble = content[..first_open].trim().to_string();

    let mut calls: Vec<ToolCall> = Vec::new();
    let mut cursor = first_open;
    while let Some(rel_open) = lower[cursor..].find("<tool_call>") {
        let open_pos = cursor + rel_open;
        let body_start = open_pos + "<tool_call>".len();
        // Reject unclosed `<tool_call>` blocks so partial/broken output goes to retry.
        let rel_close = lower[body_start..].find("</tool_call>")?;
        let close_pos = body_start + rel_close;
        // Reject nested `<tool_call>` inside the body before `</tool_call>`.
        if lower[body_start..close_pos].contains("<tool_call>") {
            return None;
        }
        let raw_body = content[body_start..close_pos].trim();
        let parsed = parse_xml_tool_json(raw_body)?;
        let obj = parsed.as_object()?;
        let name = obj.get("name")?.as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        let args_val = match obj.get("arguments").or_else(|| obj.get("parameters")) {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(Value::Object(map)) => Value::Object(map.clone()),
            Some(Value::String(raw_str)) => {
                let inner = parse_xml_tool_json(raw_str)?;
                if !inner.is_object() {
                    return None;
                }
                inner
            }
            Some(_) => return None,
        };
        calls.push(ToolCall {
            id: format!("{id_prefix}-{}", calls.len()),
            name,
            arguments: args_val,
        });
        cursor = close_pos + "</tool_call>".len();
    }

    // Any trailing text after the last `</tool_call>` must not contain stray tags.
    if lower[cursor..].contains("</tool_call>") {
        return None;
    }
    if calls.is_empty() {
        return None;
    }
    calls
        .iter()
        .all(|call| accepts(schemas, call))
        .then_some((calls, preamble))
}

fn parse_xml_tool_json(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    let unfenced = if let Some(rest) = trimmed.strip_prefix("```") {
        let body = rest.strip_suffix("```").unwrap_or(rest);
        match body.find('\n') {
            Some(nl) => body[nl + 1..].trim(),
            None => body.trim(),
        }
    } else {
        trimmed
    };
    serde_json::from_str::<Value>(&strip_trailing_commas(unfenced)).ok()
}

fn strip_trailing_commas(text: &str) -> String {
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

/// Replace characters inside triple-backtick fenced blocks with spaces of equal
/// byte length so byte offsets in the unmasked string remain aligned with `content`.
fn mask_fenced_code_blocks(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    let mut in_fence = false;
    while i < bytes.len() {
        if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"```" {
            out[i] = b' ';
            out[i + 1] = b' ';
            out[i + 2] = b' ';
            in_fence = !in_fence;
            i += 3;
            continue;
        }
        if in_fence {
            out[i] = b' ';
        }
        i += 1;
    }
    // Replacing arbitrary UTF-8 bytes inside fences with ASCII spaces preserves valid UTF-8
    // as long as we only blank complete regions between ` ``` ` ASCII delimiters.
    String::from_utf8(out).unwrap_or_else(|_| content.to_string())
}

/// Whether `call` names one of the offered tools and carries every
/// parameter that tool requires.
fn accepts(schemas: &[Value], call: &ToolCall) -> bool {
    let Some(function) = schemas.iter().find_map(|schema| {
        let function = schema.get("function").unwrap_or(schema);
        (function.get("name").and_then(Value::as_str) == Some(call.name.as_str()))
            .then_some(function)
    }) else {
        return false;
    };
    let required = function
        .pointer("/parameters/required")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    required.iter().filter_map(Value::as_str).all(|name| {
        call.arguments
            .get(name)
            .is_some_and(|value| !value.is_null())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schemas() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {"name": "shell", "parameters": {
                "type": "object", "required": ["command"],
                "properties": {"command": {"type": "string"}, "timeout": {"type": "integer"}}}}}),
            json!({"type": "function", "function": {"name": "read_file", "parameters": {
                "type": "object", "required": ["path"], "properties": {"path": {"type": "string"}}}}}),
        ]
    }

    #[test]
    fn recovers_calls_with_any_marker_style() {
        for content in [
            "Checking.\n<｜DSML｜calls>\n<｜DSML｜invoke name=\"shell\">\n<｜DSML｜parameter name=\"command\" string=\"true\">dir /b</｜DSML｜parameter>\n<｜DSML｜parameter name=\"timeout\" string=\"false\">30</｜DSML｜parameter>\n</｜DSML｜invoke>\n</｜DSML｜calls>",
            "Checking.< | | DSML | | calls>< | | DSML | | invoke name=\"shell\">< | | DSML | | parameter name=\"command\">dir /b</ | | DSML | | parameter>< | | DSML | | parameter name=\"timeout\" string=\"false\">30</ | | DSML | | parameter></ | | DSML | | invoke></ | | DSML | | calls>",
            "Checking.<|DSML|function_calls><|DSML|invoke name=\"shell\"><|DSML|parameter name=\"command\">dir /b</|DSML|parameter><|DSML|parameter name=\"timeout\" string=\"false\">30</|DSML|parameter></|DSML|invoke></|DSML|function_calls>",
        ] {
            let (calls, preamble) = recover_calls(content, &schemas(), "dsml-1").expect(content);
            assert_eq!(preamble, "Checking.");
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].id, "dsml-1-0");
            assert_eq!(calls[0].name, "shell");
            assert_eq!(calls[0].arguments, json!({"command": "dir /b", "timeout": 30}));
        }
    }

    #[test]
    fn recovers_several_calls_and_keeps_multiline_values() {
        let content = "<|DSML|calls><|DSML|invoke name=\"read_file\"><|DSML|parameter name=\"path\">a.txt</|DSML|parameter></|DSML|invoke><|DSML|invoke name=\"shell\"><|DSML|parameter name=\"command\">\necho one\necho two</|DSML|parameter></|DSML|invoke></|DSML|calls>";
        let (calls, preamble) = recover_calls(content, &schemas(), "x").unwrap();
        assert!(preamble.is_empty());
        assert_eq!(calls[0].arguments, json!({"path": "a.txt"}));
        assert_eq!(calls[1].arguments, json!({"command": "echo one\necho two"}));
    }

    #[test]
    fn rejects_unknown_tools_missing_parameters_and_broken_blocks() {
        for content in [
            // Missing the required `command`.
            "<|DSML|calls><|DSML|invoke name=\"shell\"></|DSML|invoke></|DSML|calls>",
            // Not a registered tool.
            "<|DSML|calls><|DSML|invoke name=\"grep\"><|DSML|parameter name=\"pattern\">x</|DSML|parameter></|DSML|invoke></|DSML|calls>",
            // Never closed.
            "<|DSML|calls><|DSML|invoke name=\"shell\"><|DSML|parameter name=\"command\">dir</|DSML|parameter></|DSML|invoke>",
            // Parameter left open.
            "<|DSML|calls><|DSML|invoke name=\"shell\"><|DSML|parameter name=\"command\">dir</|DSML|invoke></|DSML|calls>",
            // Invalid JSON for a non-string value.
            "<|DSML|calls><|DSML|invoke name=\"shell\"><|DSML|parameter name=\"command\">dir</|DSML|parameter><|DSML|parameter name=\"timeout\" string=\"false\">soon</|DSML|parameter></|DSML|invoke></|DSML|calls>",
            // No block at all.
            "plain answer",
        ] {
            assert!(recover_calls(content, &schemas(), "x").is_none(), "{content}");
        }
    }

    #[test]
    fn recovers_qwen_and_hermes_xml_tool_call_blocks() {
        let content = "Let me read the file first.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"notes.txt\",}}\n</tool_call>\n<tool_call>\n{\"name\": \"shell\", \"parameters\": {\"command\": \"dir /b\", \"timeout\": 15}}\n</tool_call>";
        assert!(looks_like_text_tool_calls(content));
        let (calls, preamble) = recover_calls(content, &schemas(), "xml").unwrap();
        assert_eq!(preamble, "Let me read the file first.");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "xml-0");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, json!({"path": "notes.txt"}));
        assert_eq!(calls[1].id, "xml-1");
        assert_eq!(calls[1].name, "shell");
        assert_eq!(
            calls[1].arguments,
            json!({"command": "dir /b", "timeout": 15})
        );
    }

    #[test]
    fn ignores_xml_tool_call_examples_inside_markdown_code_fences() {
        let explanation = "Qwen formats calls like this:\n```xml\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"notes.txt\"}}\n</tool_call>\n```";
        assert!(!looks_like_text_tool_calls(explanation));
        assert!(recover_calls(explanation, &schemas(), "xml").is_none());
    }
}
