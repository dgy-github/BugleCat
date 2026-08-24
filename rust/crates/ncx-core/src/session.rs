//! Conversation history — Rust port of the parts of `nanocodex/agent/session.py`
//! the turn loop relies on.
//!
//! Messages are stored as `serde_json::Value` objects in OpenAI chat shape so
//! they go straight onto the wire. The system prompt is held separately and
//! prepended by [`Session::for_model`].

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use ncx_context::{ContextEditPolicy, ContextEditStats};
use serde_json::{json, Value};

pub const COMPACTED_HISTORY_PREFIX: &str = "[压缩后保留的会话里程碑";

#[derive(Debug, Clone)]
pub struct ContextMessages {
    pub messages: Vec<Value>,
    pub stats: ContextEditStats,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub system: String,
    pub messages: Vec<Value>,
    pub log_path: Option<PathBuf>,
    pub restored_count: usize,
}

impl Session {
    pub fn new(system: impl Into<String>) -> Self {
        Self::with_log(system, None)
    }

    pub fn with_log(system: impl Into<String>, log_path: Option<PathBuf>) -> Self {
        if let Some(path) = &log_path {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
        }
        Session {
            system: system.into(),
            messages: Vec::new(),
            log_path,
            restored_count: 0,
        }
    }

    pub fn resume(system: impl Into<String>, log_path: Option<PathBuf>) -> Self {
        let restored = read_log(log_path.as_deref())
            .into_iter()
            .filter(|m| role(m) != Some("system"))
            .collect::<Vec<_>>();
        let body = sanitize_restored_messages(restored, "[interrupted: tool result not recorded]");
        let mut session = Self::with_log(system, log_path);
        session.restored_count = body.len();
        session.messages = body;
        session
    }

    pub fn fork(
        system: impl Into<String>,
        seed_messages: Vec<Value>,
        log_path: Option<PathBuf>,
    ) -> Self {
        let body = seed_messages
            .into_iter()
            .filter(|m| role(m) != Some("system"))
            .collect::<Vec<_>>();
        let body = sanitize_restored_messages(body, "[interrupted: tool result not recorded]");
        let mut session = Self::with_log(system, log_path);
        session.restored_count = body.len();
        session.messages = body;
        session
    }

    pub fn full_messages(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.messages.len() + 1);
        out.push(json!({"role": "system", "content": self.system}));
        out.extend(self.messages.clone());
        out
    }

    /// Append a user message. `content` may be a plain string or a multimodal
    /// content array (already a JSON value).
    pub fn add_user(&mut self, content: Value) {
        self.append(json!({"role": "user", "content": content}));
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
        self.append(Value::Object(msg));
    }

    /// Append a tool result message answering a specific tool_call id.
    pub fn add_tool_result(&mut self, call_id: &str, name: &str, result: &str) {
        self.append(json!({
            "role": "tool",
            "tool_call_id": call_id,
            "name": name,
            "content": result,
        }));
    }

    /// System message + history, ready for the provider.
    pub fn for_model(&self) -> Vec<Value> {
        self.for_model_edited(
            &[],
            &ContextEditPolicy {
                enabled: false,
                ..Default::default()
            },
        )
        .messages
    }

    /// System message + optional runtime notes + an edited history, ready for
    /// the provider. Editing is a non-destructive send-time view: the complete
    /// session log remains in `self.messages`.
    pub fn for_model_edited(
        &self,
        system_notes: &[String],
        policy: &ContextEditPolicy,
    ) -> ContextMessages {
        let (body, mut stats) = self.edited_body(system_notes, policy);

        let mut out = Vec::with_capacity(self.messages.len() + 1);
        out.push(json!({"role": "system", "content": self.system}));
        for note in system_notes {
            if !note.trim().is_empty() {
                out.push(json!({"role": "system", "content": note}));
            }
        }
        out.extend(body);
        stats.edited_chars = out.iter().map(json_chars).sum();
        ContextMessages {
            messages: out,
            stats,
        }
    }

    /// Materialize the send-time context editing policy into the live session.
    /// This powers `/compact`: after it runs, future turns and `--resume` see
    /// the compacted history instead of only a temporary provider view.
    pub fn compact(&mut self, policy: &ContextEditPolicy) -> ContextEditStats {
        let mut policy = policy.clone();
        policy.enabled = true;
        let (body, stats) = self.edited_body(&[], &policy);
        if stats.compressed_tool_results > 0 || stats.dropped_messages > 0 {
            self.messages = body;
            self.rewrite_log();
        }
        stats
    }

    /// Materialize context editing only after the configured size threshold is
    /// crossed. Unlike the send-time view, this persists the compacted history
    /// to the session log so later turns and restarts do not repeatedly carry
    /// old tool noise.
    pub fn compact_if_needed(&mut self, policy: &ContextEditPolicy) -> Option<ContextEditStats> {
        if !self.needs_compaction(policy) {
            return None;
        }
        let stats = self.compact(policy);
        (stats.compressed_tool_results > 0 || stats.dropped_messages > 0).then_some(stats)
    }

    pub fn needs_compaction(&self, policy: &ContextEditPolicy) -> bool {
        policy.enabled && total_chars(&self.system, &[], &self.messages) > policy.max_chars
    }

    fn edited_body(
        &self,
        system_notes: &[String],
        policy: &ContextEditPolicy,
    ) -> (Vec<Value>, ContextEditStats) {
        let original_chars = total_chars(&self.system, system_notes, &self.messages);
        let mut body = self.messages.clone();
        let mut stats = ContextEditStats {
            original_chars,
            edited_chars: original_chars,
            ..Default::default()
        };

        if policy.enabled {
            let recent_cutoff = body.len().saturating_sub(policy.keep_recent_messages);
            for (i, msg) in body.iter_mut().enumerate() {
                if i < recent_cutoff
                    && role(msg) == Some("tool")
                    && compress_tool_result(msg, policy.max_tool_result_chars)
                {
                    stats.compressed_tool_results += 1;
                }
            }

            if total_chars(&self.system, system_notes, &body) > policy.max_chars
                && body.len() > policy.keep_recent_messages
            {
                let mut start = body.len().saturating_sub(policy.keep_recent_messages);
                if let Some(rel) = body[start..].iter().position(|m| role(m) == Some("user")) {
                    start += rel;
                }
                while start < body.len() && role(&body[start]) == Some("tool") {
                    start += 1;
                }
                if start > 0 && start < body.len() {
                    let retained_history = retained_conversation_history(
                        &body[..start],
                        policy.max_chars.saturating_div(3).clamp(256, 8_000),
                    );
                    stats.dropped_messages = start;
                    body = body[start..].to_vec();
                    if let Some(history) = retained_history {
                        body.insert(0, history);
                    }
                }
            }
        }

        stats.edited_chars = total_chars(&self.system, system_notes, &body);
        (body, stats)
    }

    /// Set of tool_call ids that already have a `tool` reply.
    fn answered_ids(&self) -> std::collections::HashSet<String> {
        self.messages
            .iter()
            .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("tool"))
            .filter_map(|m| {
                m.get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
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
            let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) else {
                continue;
            };
            for tc in tcs {
                let Some(id) = tc.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
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

    fn append(&mut self, msg: Value) {
        self.messages.push(msg.clone());
        self.append_log(&msg);
    }

    fn append_log(&self, msg: &Value) {
        let Some(path) = &self.log_path else {
            return;
        };
        let Some(mut record) = redact_image_data(msg, "[image omitted from log]")
            .as_object()
            .cloned()
        else {
            return;
        };
        record.insert("_ts".into(), json!(now_stamp()));
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        if let Ok(line) = serde_json::to_string(&Value::Object(record)) {
            let _ = writeln!(file, "{line}");
        }
    }

    fn rewrite_log(&self) {
        let Some(path) = &self.log_path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
        else {
            return;
        };
        for msg in &self.messages {
            let Some(mut record) = redact_image_data(msg, "[image omitted from log]")
                .as_object()
                .cloned()
            else {
                continue;
            };
            record.insert("_ts".into(), json!(now_stamp()));
            if let Ok(line) = serde_json::to_string(&Value::Object(record)) {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

fn role(msg: &Value) -> Option<&str> {
    msg.get("role").and_then(|v| v.as_str())
}

fn read_log(path: Option<&Path>) -> Vec<Value> {
    let Some(path) = path else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter_map(|mut value| {
            let obj = value.as_object_mut()?;
            obj.get("role").and_then(|v| v.as_str())?;
            obj.remove("_ts");
            Some(value)
        })
        .collect()
}

fn sanitize_restored_messages(messages: Vec<Value>, placeholder: &str) -> Vec<Value> {
    let mut fulfilled: HashSet<String> = messages
        .iter()
        .filter(|m| role(m) == Some("tool"))
        .filter_map(|m| m.get("tool_call_id").and_then(|v| v.as_str()))
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    for msg in messages {
        let calls = msg
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        out.push(msg);
        for tc in calls {
            let Some(id) = tc.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            if fulfilled.contains(id) {
                continue;
            }
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            out.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "name": name,
                "content": placeholder,
            }));
            fulfilled.insert(id.to_string());
        }
    }
    out
}

pub(crate) fn redact_image_data(msg: &Value, placeholder: &str) -> Value {
    let mut out = msg.clone();
    let Some(obj) = out.as_object_mut() else {
        return out;
    };
    let Some(content) = obj.get_mut("content").and_then(|v| v.as_array_mut()) else {
        return out;
    };
    for block in content {
        let is_data_image = block.get("type").and_then(|v| v.as_str()) == Some("image_url")
            && block
                .get("image_url")
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
                .is_some_and(|url| url.starts_with("data:"));
        if is_data_image {
            *block = json!({"type": "text", "text": placeholder});
        }
    }
    out
}

fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:013}", d.as_millis()))
        .unwrap_or_else(|_| "0000000000000".into())
}

fn json_chars(v: &Value) -> usize {
    serde_json::to_string(v)
        .map(|s| s.chars().count())
        .unwrap_or(0)
}

fn total_chars(system: &str, notes: &[String], messages: &[Value]) -> usize {
    system.chars().count()
        + notes.iter().map(|n| n.chars().count()).sum::<usize>()
        + messages.iter().map(json_chars).sum::<usize>()
}

/// Preserve compact conversation milestones when old tool-heavy history is
/// dropped. Keeping only user requests makes follow-ups such as "继续" lose the
/// completed result, chosen option, and current hand-off point.
fn retained_conversation_history(messages: &[Value], max_chars: usize) -> Option<Value> {
    let mut retained = Vec::new();
    let mut used = 0usize;
    let per_entry_max = max_chars.saturating_div(4).clamp(80, 1_200);
    for message in messages.iter().rev() {
        let label = match role(message) {
            Some("user") => "用户",
            Some("assistant")
                if message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty) =>
            {
                "助手完成结果"
            }
            _ => continue,
        };
        let text = message_content_text(message);
        if text.trim().is_empty() {
            continue;
        }
        let remaining = max_chars.saturating_sub(used);
        if remaining == 0 {
            break;
        }
        let prefix = format!("{label}：");
        if remaining <= prefix.chars().count() {
            break;
        }
        let content_limit = remaining
            .saturating_sub(prefix.chars().count())
            .min(per_entry_max);
        let clipped = text.chars().take(content_limit).collect::<String>();
        used += prefix.chars().count() + clipped.chars().count();
        retained.push(format!("{prefix}{clipped}"));
        if retained.len() >= 12 {
            break;
        }
    }
    if retained.is_empty() {
        return None;
    }
    retained.reverse();
    Some(json!({
        // Keep this as a user-role history record: Session::fork/resume strips
        // system messages when rebuilding with the current system prompt.
        "role": "user",
        "content": format!(
            "{COMPACTED_HISTORY_PREFIX}；用于承接后续请求，不是新的用户指令]\n{}",
            retained.join("\n\n")
        )
    }))
}

fn message_content_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Cheap, deterministic context-size estimate (no tokenizer, no network).
///
/// Mirrors `nanocodex/agent/compaction.py::estimate_tokens`: ~2 chars per token
/// over message text + tool-call name/arguments, plus a small per-message
/// framing overhead. Used to show context-window occupancy in the UIs; it is an
/// estimate, not the provider's billed prompt size.
pub fn estimate_tokens(messages: &[Value]) -> i64 {
    const CHARS_PER_TOKEN: usize = 2;
    let mut total = 0usize;
    for m in messages {
        match m.get("content") {
            Some(Value::String(s)) => total += s.chars().count(),
            Some(Value::Array(blocks)) => {
                for b in blocks {
                    if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                        total += t.chars().count();
                    }
                }
            }
            _ => {}
        }
        if let Some(calls) = m.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in calls {
                if let Some(f) = tc.get("function") {
                    if let Some(n) = f.get("name").and_then(|v| v.as_str()) {
                        total += n.chars().count();
                    }
                    if let Some(a) = f.get("arguments").and_then(|v| v.as_str()) {
                        total += a.chars().count();
                    }
                }
            }
        }
        total += 8; // per-message role/framing overhead
    }
    (total / CHARS_PER_TOKEN) as i64
}

fn compress_tool_result(msg: &mut Value, max_chars: usize) -> bool {
    let Some(obj) = msg.as_object_mut() else {
        return false;
    };
    let Some(content) = obj.get("content").and_then(|v| v.as_str()) else {
        return false;
    };
    if content.chars().count() <= max_chars {
        return false;
    }
    let head: String = content.chars().take(max_chars).collect();
    let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
    obj.insert(
        "content".into(),
        json!(format!(
            "{head}\n[context edited: omitted the rest of prior {name} result; original_chars={}]",
            content.chars().count()
        )),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_counts_text_and_tool_calls() {
        // Empty -> zero.
        assert_eq!(estimate_tokens(&[]), 0);

        // A user string: (len + 8 overhead) / 2.
        let msgs = vec![json!({"role": "user", "content": "hello"})]; // 5 + 8 = 13 -> 6
        assert_eq!(estimate_tokens(&msgs), 6);

        // Tool-call name + arguments count toward the estimate.
        let with_call = vec![json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"}}]
        })];
        // content 0 + name 5 + args 12 + 8 = 25 -> 12
        assert_eq!(estimate_tokens(&with_call), 12);

        // Multimodal: only the text blocks count; image blocks are ignored.
        let multimodal = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "what"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
            ]
        })]; // 4 + 8 = 12 -> 6
        assert_eq!(estimate_tokens(&multimodal), 6);
    }

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
        assert_eq!(
            tool_ids,
            ["c1", "c2"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn context_edit_compresses_old_tool_results_without_mutating_session() {
        let mut s = Session::new("sys");
        s.add_user_text("inspect logs");
        s.add_assistant("", Some(vec![json!({"id": "c1", "type": "function", "function": {"name": "shell", "arguments": "{}"}})]), "");
        s.add_tool_result("c1", "shell", &"x".repeat(200));
        s.add_user_text("continue");

        let out = s.for_model_edited(
            &["budget note".into()],
            &ContextEditPolicy {
                enabled: true,
                max_chars: 10_000,
                keep_recent_messages: 1,
                max_tool_result_chars: 20,
            },
        );
        assert_eq!(out.stats.compressed_tool_results, 1);
        assert!(out
            .messages
            .iter()
            .any(|m| m["role"] == "system" && m["content"] == "budget note"));
        assert!(out.messages.iter().any(|m| {
            m["role"] == "tool"
                && m["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("context edited")
        }));
        assert_eq!(s.messages[2]["content"].as_str().unwrap().len(), 200);
    }

    #[test]
    fn context_edit_drops_old_prefix_when_over_budget() {
        let mut s = Session::new("sys");
        for i in 0..8 {
            s.add_user_text(&format!("old user {i} {}", "x".repeat(40)));
            s.add_assistant(&format!("old answer {i} {}", "y".repeat(40)), None, "");
        }
        let out = s.for_model_edited(
            &[],
            &ContextEditPolicy {
                enabled: true,
                max_chars: 500,
                keep_recent_messages: 4,
                max_tool_result_chars: 20,
            },
        );
        assert!(out.stats.dropped_messages > 0);
        assert!(out.stats.edited_chars < out.stats.original_chars);
        assert_eq!(out.messages[0]["role"], "system");
    }

    #[test]
    fn context_edit_preserves_user_task_history_while_dropping_tool_noise() {
        let mut s = Session::new("sys");
        s.add_user_text("请调研六项技术，最终生成并验证 PDF 文件");
        for i in 0..12 {
            s.add_assistant(
                "",
                Some(vec![json!({
                    "id": format!("call-{i}"),
                    "type": "function",
                    "function": {"name": "web_fetch", "arguments": "{}"}
                })]),
                "",
            );
            s.add_tool_result(&format!("call-{i}"), "web_fetch", &"noise".repeat(100));
        }
        s.add_assistant(
            "PDF 已生成到 D:\\\\github_dgy\\\\DeepSeek-资料集.pdf",
            None,
            "",
        );
        s.add_user_text("把同一份资料改成 PPT");
        s.add_assistant("已生成科技版 PPT，下一步等待用户选择版式", None, "");
        s.add_user_text("继续");

        let out = s.for_model_edited(
            &[],
            &ContextEditPolicy {
                enabled: true,
                max_chars: 700,
                keep_recent_messages: 3,
                max_tool_result_chars: 20,
            },
        );
        let rendered = serde_json::to_string(&out.messages).unwrap();

        assert!(out.stats.dropped_messages > 0);
        assert!(rendered.contains("最终生成并验证 PDF 文件"), "{rendered}");
        assert!(
            rendered.contains("PDF 已生成到") && rendered.contains("等待用户选择版式"),
            "压缩后必须保留已完成结果和当前决策点：{rendered}"
        );
        assert!(!rendered.contains(&"noise".repeat(30)), "{rendered}");
    }

    #[test]
    fn compact_materializes_context_edit_and_rewrites_log() {
        let dir = std::env::temp_dir().join(format!("ncx_session_compact_{}", now_stamp()));
        let path = dir.join("session.jsonl");
        let mut s = Session::with_log("sys", Some(path.clone()));
        for i in 0..8 {
            s.add_user_text(&format!("old user {i} {}", "x".repeat(40)));
            s.add_assistant(&format!("old answer {i} {}", "y".repeat(40)), None, "");
        }
        let before = s.messages.len();

        let stats = s.compact(&ContextEditPolicy {
            enabled: true,
            max_chars: 500,
            keep_recent_messages: 4,
            max_tool_result_chars: 20,
        });

        assert!(stats.dropped_messages > 0);
        assert!(s.messages.len() < before);
        let resumed = Session::resume("fresh", Some(path));
        assert_eq!(resumed.messages.len(), s.messages.len());
        assert_eq!(resumed.messages[0]["role"], "user");
    }

    #[test]
    fn automatic_compaction_does_not_trigger_under_budget() {
        let mut s = Session::new("sys");
        s.add_user_text("hello");
        s.add_assistant("hi", None, "");

        let stats = s.compact_if_needed(&ContextEditPolicy {
            enabled: true,
            max_chars: 10_000,
            keep_recent_messages: 4,
            max_tool_result_chars: 20,
        });

        assert!(stats.is_none());
        assert_eq!(s.messages.len(), 2);
    }

    #[test]
    fn logs_messages_as_jsonl_and_resumes_body() {
        let dir = std::env::temp_dir().join(format!("ncx_session_log_{}", now_stamp()));
        let path = dir.join("session.jsonl");
        let mut s = Session::with_log("sys", Some(path.clone()));
        s.add_user_text("hello");
        s.add_assistant("hi", None, "");

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("\"role\":\"user\""));
        assert!(log.contains("\"_ts\""));

        let resumed = Session::resume("fresh sys", Some(path));
        assert_eq!(resumed.system, "fresh sys");
        assert_eq!(resumed.restored_count, 2);
        assert_eq!(resumed.messages[0]["role"], "user");
        assert_eq!(resumed.messages[1]["content"], "hi");
    }

    #[test]
    fn resume_backfills_dangling_tool_call() {
        let dir = std::env::temp_dir().join(format!("ncx_session_resume_{}", now_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"role":"system","content":"old sys"}"#,
                "\n",
                r#"{"role":"assistant","content":"","tool_calls":[{"id":"call_1","function":{"name":"shell"}}]}"#,
            ),
        )
        .unwrap();

        let resumed = Session::resume("sys", Some(path));
        assert_eq!(resumed.messages.len(), 2);
        assert_eq!(resumed.messages[1]["role"], "tool");
        assert_eq!(resumed.messages[1]["tool_call_id"], "call_1");
        assert!(resumed.messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("interrupted"));
    }

    #[test]
    fn log_redacts_inline_image_data() {
        let dir = std::env::temp_dir().join(format!("ncx_session_image_{}", now_stamp()));
        let path = dir.join("session.jsonl");
        let mut s = Session::with_log("sys", Some(path.clone()));
        s.add_user(json!([
            {"type": "text", "text": "describe"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
        ]));

        let log = std::fs::read_to_string(path).unwrap();
        assert!(log.contains("[image omitted from log]"));
        assert!(!log.contains("data:image"));
    }

    #[test]
    fn fork_uses_seed_without_touching_source_log() {
        let dir = std::env::temp_dir().join(format!("ncx_session_fork_{}", now_stamp()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.jsonl");
        std::fs::write(&source, "{\"role\":\"user\",\"content\":\"original\"}\n").unwrap();
        let before = std::fs::read_to_string(&source).unwrap();
        let fork_log = dir.join("fork.jsonl");

        let mut forked = Session::fork(
            "fresh",
            vec![
                json!({"role": "system", "content": "old"}),
                json!({"role": "user", "content": "original"}),
            ],
            Some(fork_log.clone()),
        );
        forked.add_user_text("new");

        assert_eq!(std::fs::read_to_string(source).unwrap(), before);
        assert!(std::fs::read_to_string(fork_log).unwrap().contains("new"));
        assert_eq!(forked.restored_count, 1);
    }
}
