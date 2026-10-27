//! Recover DeepSeek DSML tool calls that a model wrote into its message text.
//!
//! DeepSeek models occasionally serialize tool calls in their native DSML
//! markup inside `content` instead of the `tool_calls` field:
//!
//! ```text
//! <｜DSML｜calls>
//! <｜DSML｜invoke name="shell">
//! <｜DSML｜parameter name="command" string="true">dir</｜DSML｜parameter>
//! </｜DSML｜invoke>
//! </｜DSML｜calls>
//! ```
//!
//! The markers vary (`|` or `｜`, doubled, spaced; `calls` or
//! `function_calls`). A well-formed block naming registered tools with their
//! required parameters is turned back into native calls, which then go
//! through the same sandbox, approval, and review path as any other call.
//! Anything malformed is left to the caller's retry path.

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

/// The calls in the first DSML block of `content`, plus the text before it.
/// `None` unless the block is well-formed and every call names a tool in
/// `schemas` with all of that tool's required parameters.
pub(super) fn recover_calls(
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
}
