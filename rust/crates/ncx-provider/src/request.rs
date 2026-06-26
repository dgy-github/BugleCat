//! Request-body shaping — Rust port of the pure request helpers in
//! `nanocodex/provider/deepseek.py` (`_build_kwargs`, `_apply_reasoning_effort`,
//! `_sanitize_reasoning_replay`).
//!
//! The body is built as a `serde_json::Map` that mirrors the Python `kwargs`
//! dict exactly — including the DeepSeek `extra_body` nesting — so the unit
//! tests match the Python ones field-for-field. [`to_request_json`] then
//! flattens `extra_body` into the top-level POST body for the actual HTTP call.

use serde_json::{json, Map, Value};

const REASONING_PLACEHOLDER: &str = "(reasoning omitted)";
const DISABLED_REASONING_EFFORTS: &[&str] = &["off", "disabled", "none", "false"];

/// True for DeepSeek's own models (their thinking-mode API shape applies).
pub fn is_deepseek_model(model: &str) -> bool {
    model.trim().to_lowercase().starts_with("deepseek")
}

/// Build the request body (mirrors `_build_kwargs`).
///
/// `messages` / `tools` are JSON arrays of message / tool-spec objects.
pub fn build_body(
    model: &str,
    messages: &[Value],
    tools: Option<&[Value]>,
    temperature: Option<f64>,
    max_tokens: Option<i64>,
    reasoning_effort: Option<&str>,
) -> Map<String, Value> {
    let mut kwargs = Map::new();
    kwargs.insert("model".into(), json!(model));
    kwargs.insert(
        "messages".into(),
        Value::Array(sanitize_reasoning_replay(messages, model, reasoning_effort)),
    );
    if let Some(t) = tools {
        if !t.is_empty() {
            kwargs.insert("tools".into(), Value::Array(t.to_vec()));
            kwargs.insert("tool_choice".into(), json!("auto"));
        }
    }
    if let Some(temp) = temperature {
        kwargs.insert("temperature".into(), json!(temp));
    }
    if let Some(mt) = max_tokens {
        kwargs.insert("max_tokens".into(), json!(mt));
    }
    apply_reasoning_effort(&mut kwargs, model, reasoning_effort);
    kwargs
}

/// Translate a reasoning-effort tier into the right request fields per backend.
///
/// DeepSeek's thinking-mode API only understands enabled/disabled plus
/// `reasoning_effort` high|max, so low/medium collapse to high there. Any other
/// OpenAI-compatible model gets the STANDARD top-level `reasoning_effort` field
/// with the real tier.
pub fn apply_reasoning_effort(kwargs: &mut Map<String, Value>, model: &str, effort: Option<&str>) {
    let Some(raw) = effort else { return };
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() || normalized == "auto" {
        return;
    }

    if is_deepseek_model(model) {
        let mut extra = match kwargs.get("extra_body") {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        if DISABLED_REASONING_EFFORTS.contains(&normalized.as_str()) {
            extra.insert("thinking".into(), json!({"type": "disabled"}));
        } else if matches!(normalized.as_str(), "xhigh" | "max" | "highest") {
            extra.insert("reasoning_effort".into(), json!("max"));
            extra.insert("thinking".into(), json!({"type": "enabled"}));
        } else if matches!(normalized.as_str(), "low" | "minimal" | "medium" | "mid" | "high") {
            // DeepSeek maps low/medium to high in its current thinking-mode API.
            extra.insert("reasoning_effort".into(), json!("high"));
            extra.insert("thinking".into(), json!({"type": "enabled"}));
        } else {
            return;
        }
        kwargs.insert("extra_body".into(), Value::Object(extra));
        return;
    }

    // Generic OpenAI-compatible endpoint.
    if DISABLED_REASONING_EFFORTS.contains(&normalized.as_str()) {
        return;
    }
    let tier = match normalized.as_str() {
        "xhigh" | "max" | "highest" => "high",
        "mid" => "medium",
        other => other,
    };
    if matches!(tier, "minimal" | "low" | "medium" | "high") {
        kwargs.insert("reasoning_effort".into(), json!(tier));
    }
}

/// Ensure DeepSeek thinking-mode tool-call history replays `reasoning_content`.
///
/// DeepSeek V4/reasoner rejects a request when an assistant history item carries
/// `tool_calls` but lacks non-empty `reasoning_content`. Returns a cloned,
/// possibly-patched copy; the input slice is never mutated.
pub fn sanitize_reasoning_replay(
    messages: &[Value],
    model: &str,
    reasoning_effort: Option<&str>,
) -> Vec<Value> {
    if !should_replay_reasoning_content(model, reasoning_effort) {
        return messages.to_vec();
    }
    let mut out = messages.to_vec();
    for msg in &mut out {
        let Some(obj) = msg.as_object_mut() else { continue };
        if obj.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let has_tool_calls = obj
            .get("tool_calls")
            .map(|v| !v.is_null() && v.as_array().map(|a| !a.is_empty()).unwrap_or(true))
            .unwrap_or(false);
        if !has_tool_calls {
            continue;
        }
        let current = obj.get("reasoning_content").and_then(|v| v.as_str()).unwrap_or("");
        if current.trim().is_empty() {
            obj.insert("reasoning_content".into(), json!(REASONING_PLACEHOLDER));
        }
    }
    out
}

fn should_replay_reasoning_content(model: &str, reasoning_effort: Option<&str>) -> bool {
    if let Some(e) = reasoning_effort {
        if DISABLED_REASONING_EFFORTS.contains(&e.trim().to_lowercase().as_str()) {
            return false;
        }
    }
    requires_reasoning_content(model)
}

fn requires_reasoning_content(model: &str) -> bool {
    let n = model.trim().to_lowercase();
    n.starts_with("deepseek-chat")
        || n.starts_with("deepseek-reasoner")
        || n.starts_with("deepseek-v4")
}

/// Flatten the mirror-shaped body into the actual HTTP POST JSON: any keys under
/// `extra_body` are merged into the top level (that is what the OpenAI SDK does
/// with `extra_body`), and `stream` / `stream_options` may be added by caller.
pub fn to_request_json(kwargs: &Map<String, Value>) -> Value {
    let mut body = kwargs.clone();
    if let Some(Value::Object(extra)) = body.remove("extra_body") {
        for (k, v) in extra {
            body.insert(k, v);
        }
    }
    Value::Object(body)
}

// ── tests (mirror tests/test_deepseek_provider.py request-shaping cases) ───────

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(text: &str) -> Value {
        json!({"role": "user", "content": text})
    }

    fn assistant_toolcall() -> Value {
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "read_file", "arguments": "{}"},
            }],
        })
    }

    #[test]
    fn replays_reasoning_placeholder_for_deepseek_tool_history() {
        let messages = vec![user_msg("read a file"), assistant_toolcall()];
        let kwargs = build_body("deepseek-v4-pro", &messages, None, None, None, None);
        let out_messages = kwargs["messages"].as_array().unwrap();
        assert_eq!(
            out_messages[1]["reasoning_content"].as_str().unwrap(),
            "(reasoning omitted)"
        );
        // original history is not mutated
        assert!(messages[1].as_object().unwrap().get("reasoning_content").is_none());
    }

    #[test]
    fn preserves_existing_reasoning_content() {
        let messages = vec![json!({
            "role": "assistant",
            "content": "",
            "reasoning_content": "real reasoning",
            "tool_calls": [{
                "id": "c1", "type": "function",
                "function": {"name": "read_file", "arguments": "{}"},
            }],
        })];
        let kwargs = build_body("deepseek-chat", &messages, None, None, None, None);
        assert_eq!(
            kwargs["messages"][0]["reasoning_content"].as_str().unwrap(),
            "real reasoning"
        );
    }

    #[test]
    fn does_not_replay_reasoning_when_effort_disabled() {
        let messages = vec![assistant_toolcall()];
        let kwargs = build_body("deepseek-v4-pro", &messages, None, None, None, Some("off"));
        assert!(kwargs["messages"][0]
            .as_object()
            .unwrap()
            .get("reasoning_content")
            .is_none());
        assert_eq!(kwargs["extra_body"]["thinking"], json!({"type": "disabled"}));
    }

    #[test]
    fn maps_reasoning_effort_to_deepseek_beta_body() {
        let kwargs = build_body(
            "deepseek-v4-pro",
            &[user_msg("think")],
            None,
            None,
            None,
            Some("max"),
        );
        assert_eq!(
            kwargs["extra_body"],
            json!({"reasoning_effort": "max", "thinking": {"type": "enabled"}})
        );
    }

    #[test]
    fn deepseek_collapses_low_medium_to_high() {
        for tier in ["low", "medium", "minimal"] {
            let kwargs = build_body(
                "deepseek-chat",
                &[user_msg("x")],
                None,
                None,
                None,
                Some(tier),
            );
            assert_eq!(kwargs["extra_body"]["reasoning_effort"], json!("high"));
            assert_eq!(kwargs["extra_body"]["thinking"], json!({"type": "enabled"}));
            assert!(!kwargs.contains_key("reasoning_effort"));
        }
    }

    #[test]
    fn generic_model_passes_real_tier_through_top_level() {
        let cases = [
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
            ("minimal", "minimal"),
            ("max", "high"),
            ("mid", "medium"),
        ];
        for (tier, expected) in cases {
            let kwargs = build_body(
                "Qwen3.6-27B",
                &[user_msg("x")],
                None,
                None,
                None,
                Some(tier),
            );
            assert_eq!(kwargs.get("reasoning_effort"), Some(&json!(expected)), "{tier}");
            assert!(!kwargs.contains_key("extra_body"));
        }
    }

    #[test]
    fn generic_model_off_omits_reasoning_field() {
        let kwargs = build_body("Qwen3.6-27B", &[user_msg("x")], None, None, None, Some("off"));
        assert!(!kwargs.contains_key("reasoning_effort"));
        assert!(!kwargs.contains_key("extra_body"));
    }

    #[test]
    fn auto_and_none_set_no_reasoning_fields() {
        for effort in [None, Some("auto")] {
            for model in ["deepseek-chat", "Qwen3.6-27B"] {
                let kwargs = build_body(model, &[user_msg("x")], None, None, None, effort);
                assert!(!kwargs.contains_key("reasoning_effort"));
                assert!(!kwargs.contains_key("extra_body"));
            }
        }
    }

    #[test]
    fn tools_add_tool_choice_auto() {
        let tools = vec![json!({"type": "function", "function": {"name": "x"}})];
        let kwargs = build_body("deepseek-chat", &[user_msg("x")], Some(&tools), None, None, None);
        assert_eq!(kwargs["tool_choice"], json!("auto"));
        assert_eq!(kwargs["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn to_request_json_flattens_extra_body() {
        let kwargs = build_body(
            "deepseek-v4-pro",
            &[user_msg("x")],
            None,
            None,
            None,
            Some("max"),
        );
        let body = to_request_json(&kwargs);
        // extra_body keys are now top-level; extra_body itself is gone.
        assert!(body.get("extra_body").is_none());
        assert_eq!(body["reasoning_effort"], json!("max"));
        assert_eq!(body["thinking"], json!({"type": "enabled"}));
    }
}
