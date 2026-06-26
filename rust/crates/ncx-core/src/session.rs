//! Conversation history — Rust port of the parts of `nanocodex/agent/session.py`
//! the turn loop relies on.
//!
//! Messages are stored as `serde_json::Value` objects in OpenAI chat shape so
//! they go straight onto the wire. The system prompt is held separately and
//! prepended by [`Session::for_model`].

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Session {
    pub system: String,
    pub messages: Vec<Value>,
}

impl Session {
    pub fn new(system: impl Into<String>) -> Self {
        Session { system: system.into(), messages: Vec::new() }
    }

    /// Append a user message. `content` may be a plain string or a multimodal
    /// content array (already a JSON value).
    pub fn add_user(&mut self, content: Value) {
        self.messages.push(json!({"role": "user", "content": content}));
    }

    pub fn add_user_text(&mut self, text: &str) {
        self.add_user(Value::String(text.to_string()));
    }

    /// Append an assistant message, optionally carrying tool_calls and reasoning.
    pub fn add_assistant(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<Value>>,
        reasoning: &str,
    ) {
        let mut msg = serde_json::Map::new();
        msg.insert("role".into(), json!("assistant"));
        msg.insert("content".into(), json!(content));
        if let Some(tcs) = tool_calls {
            if !tcs.is_empty() {
                msg.insert("tool_calls".into(), Value::Array(tcs));
            }
        }
        if !reasoning.trim().is_empty() {
            msg.insert("reasoning_content".into(), json!(reasoning));
        }
        self.messages.push(Value::Object(msg));
    }

    /// Append a tool result message answering a specific tool_call id.
    pub fn add_tool_result(&mut self, call_id: &str, name: &str, result: &str) {
        self.messages.push(json!({
            "role": "tool",
            "tool_call_id": call_id,
            "name": name,
            "content": result,
        }));
    }

    /// System message + history, ready for the provider.
    pub fn for_model(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.messages.len() + 1);
        out.push(json!({"role": "system", "content": self.system}));
        out.extend(self.messages.iter().cloned());
        out
    }

    /// Set of tool_call ids that already have a `tool` reply.
    fn answered_ids(&self) -> std::collections::HashSet<String> {
        self.messages
            .iter()
            .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
            .filter_map(|m| m.get("tool_call_id").and_then(|v| v.as_str()).map(String::from))
            .collect()
    }

    /// Backfill synthetic tool results for any assistant tool_call left
    /// unanswered, so the history stays valid (every tool_call has a tool reply).
    pub fn backfill_unanswered_tool_calls(&mut self, placeholder: &str) {
        let answered = self.answered_ids();
        let mut missing: Vec<(String, String)> = Vec::new();
        for m in &self.messages {
            if m.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }
            let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) else { continue };
            for tc in tcs {
                let Some(id) = tc.get("id").and_then(|v| v.as_str()) else { continue };
                if answered.contains(id) || missing.iter().any(|(mid, _)| mid == id) {
                    continue;
                }
                let name = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                missing.push((id.to_string(), name));
            }
        }
        for (id, name) in missing {
            self.add_tool_result(&id, &name, placeholder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_model_prepends_system() {
        let mut s = Session::new("sys");
        s.add_user_text("hi");
        let msgs = s.for_model();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "sys");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "hi");
    }

    #[test]
    fn assistant_records_reasoning_only_when_present() {
        let mut s = Session::new("sys");
        s.add_assistant("answer", None, "");
        assert!(s.messages[0].get("reasoning_content").is_none());
        s.add_assistant("answer2", None, "because");
        assert_eq!(s.messages[1]["reasoning_content"], "because");
    }

    #[test]
    fn backfill_answers_dangling_tool_calls() {
        let mut s = Session::new("sys");
        let tcs = vec![
            json!({"id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}),
            json!({"id": "c2", "type": "function", "function": {"name": "read_file", "arguments": "{}"}}),
        ];
        s.add_assistant("", Some(tcs), "");
        s.add_tool_result("c1", "read_file", "real result");
        s.backfill_unanswered_tool_calls("[interrupted]");
        let tool_ids: std::collections::HashSet<_> = s
            .messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(tool_ids, ["c1", "c2"].iter().map(|s| s.to_string()).collect());
    }
}
