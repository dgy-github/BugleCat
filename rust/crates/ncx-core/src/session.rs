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
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub use ncx_context::{ContextEditPolicy, ContextEditStats};
use serde_json::{json, Value};

use crate::context_trace::{RetrievedSource, SourceKind, SourceRecord, SourceTraceStore};
use crate::tool_recovery::classify_tool_result;

mod semantic_compaction;
pub use semantic_compaction::CompactionPlan;

pub const COMPACTED_HISTORY_PREFIX: &str = "[压缩后保留的会话里程碑";

#[derive(Debug, Clone)]
pub struct ContextMessages {
    pub messages: Vec<Value>,
    pub stats: ContextEditStats,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompactionSafetySnapshot {
    pub original_requirements: Vec<String>,
    pub confirmed_decisions: Vec<String>,
    pub prohibitions: Vec<String>,
    pub completed: Vec<String>,
    pub pending: Vec<String>,
    pub files: Vec<String>,
    pub git_diff_summary: String,
    pub recent_tests: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SafeCompactionResult {
    pub stats: ContextEditStats,
    pub before: CompactionSafetySnapshot,
    pub after: CompactionSafetySnapshot,
    pub conflicts: Vec<String>,
}

/// Compaction boundary metadata embedded inside a compacted milestone entry's `_trace`.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CompactionTrace {
    /// Entry ID of the first kept message after the compacted prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_kept_entry_id: Option<String>,
    /// Number of historical messages folded into this compaction entry.
    pub dropped_messages: usize,
    /// Number of tool results compressed in-place.
    pub compressed_tool_results: usize,
    /// Context character count before compaction.
    pub chars_before: usize,
    /// Context character count after compaction.
    pub chars_after: usize,
}

/// Pi-style in-conversation trace metadata embedded under `"_trace"` on each
/// session message in `self.messages` and persisted in `session.jsonl`.
/// Automatically stripped by `for_model_edited` before sending to the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct MessageTrace {
    /// Stable unique entry ID within the session (e.g. `"e1"`, `"e2"`).
    pub id: String,
    /// Parent entry ID in the conversation tree (`None` for the root entry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Turn number assigned by the agent loop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<u64>,
    /// Millisecond epoch timestamp when the entry was recorded.
    pub timestamp: u64,
    /// Short SHA-256 digest of the untruncated message content / tool payload.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content_hash: String,
    /// Workspace git revision at the time of recording, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_revision: Option<String>,
    /// Model identifier that produced an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider finish reason for an assistant message (`"stop"`, `"tool_calls"`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Per-step token usage for an assistant message.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub usage: std::collections::BTreeMap<String, i64>,
    /// Normalized execution outcome for a tool result (`"completed"`, `"failed"`, `"cancelled"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Normalized failure class for a failed tool result (e.g. `"notfound"`, `"timeout"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    /// Process exit code parsed from a shell/tool result, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Execution wall-clock duration in milliseconds for a tool call, if measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Compaction metadata when this entry is a compaction milestone/summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionTrace>,
}

/// Longest excerpt stored per provenance record. The full text stays in the
/// JSONL log; `content_hash` covers the untruncated original.
const MAX_TRACE_EXCERPT_CHARS: usize = 4_000;

#[derive(Debug, Clone)]
pub struct Session {
    pub system: String,
    pub messages: Vec<Value>,
    pub log_path: Option<PathBuf>,
    pub restored_count: usize,
    /// Stable id for provenance records. Derived from the log path so a resumed
    /// session keeps appending to the same trace.
    pub session_id: String,
    trace: Option<SourceTraceStore>,
    /// Indexed fact events. Shared with the agent loop's state recorder so both
    /// write into one store and a query sees one consistent history.
    events: Option<std::rc::Rc<crate::context_store::ContextStore>>,
    turn_id: Option<u64>,
    workspace_revision: Option<String>,
    workspace: Option<String>,
    next_entry_seq: u64,
    last_entry_id: Option<String>,
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
        let session_id = log_path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(now_stamp);
        let trace = log_path
            .as_deref()
            .map(|path| SourceTraceStore::new(path.with_extension("sources.jsonl")));
        Session {
            system: system.into(),
            messages: Vec::new(),
            log_path,
            restored_count: 0,
            session_id,
            trace,
            events: None,
            turn_id: None,
            workspace_revision: None,
            workspace: None,
            next_entry_seq: 1,
            last_entry_id: None,
        }
    }

    /// Extract the embedded Pi-style `MessageTrace` from a session message value.
    pub fn message_trace(msg: &Value) -> Option<MessageTrace> {
        serde_json::from_value(msg.get("_trace")?.clone()).ok()
    }

    /// Return all in-conversation `MessageTrace` nodes currently in the session.
    pub fn trace_entries(&self) -> Vec<MessageTrace> {
        self.messages
            .iter()
            .filter_map(Self::message_trace)
            .collect()
    }

    /// Return the entry ID of the current conversation leaf node.
    pub fn leaf_entry_id(&self) -> Option<String> {
        self.last_entry_id.clone().or_else(|| {
            self.messages
                .last()
                .and_then(Self::message_trace)
                .map(|t| t.id)
        })
    }

    /// Attach the shared fact-event store. Without it the session still works;
    /// it simply has no indexed history to query.
    pub fn set_event_store(&mut self, store: std::rc::Rc<crate::context_store::ContextStore>) {
        self.events = Some(store);
    }

    pub fn event_store(&self) -> Option<&std::rc::Rc<crate::context_store::ContextStore>> {
        self.events.as_ref()
    }

    /// Tool calls this session started but never saw finish. These must not be
    /// compacted away and must not be silently re-issued.
    pub fn unresolved_tool_calls(&self) -> Vec<String> {
        self.events
            .as_ref()
            .and_then(|store| store.unresolved_tool_calls(&self.session_id).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|event| event.tool_call_id)
            .collect()
    }

    fn append_event(&self, msg: &Value, entry_id: &str) {
        let Some(store) = &self.events else {
            return;
        };
        let workspace = self.workspace.clone();
        let source_ref = self.source_ref_for(entry_id);
        match role(msg) {
            Some("user") => {
                let text = message_content_text(msg);
                if text.trim().is_empty()
                    && crate::verification::requirements::attachment_metadata(&msg["content"])
                        .is_empty()
                {
                    return;
                }
                let _ = store.append(
                    &crate::context_store::ContextEvent::new(
                        format!("{}-user-{}", self.session_id, record_nonce()),
                        self.session_id.clone(),
                        crate::context_store::EventType::UserMessage,
                        crate::context_store::EventStatus::Completed,
                    )
                    .with_turn(self.turn_id)
                    .with_workspace(workspace)
                    .with_source_ref(source_ref)
                    .with_payload(&json!({
                        "entry_id": entry_id,
                        "text_hash": crate::context_trace::content_hash(&text),
                        "text": crate::verification::evidence::redact(&text).chars().take(64_000).collect::<String>(),
                        "text_truncated": text.chars().count() > 64_000,
                        "text_redacted": crate::verification::evidence::redact(&text) != text,
                        "attachment_metadata": crate::verification::requirements::attachment_metadata(&msg["content"]),
                    })),
                );
            }
            Some("assistant") => {
                for call in msg
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let call_id = call.get("id").and_then(Value::as_str).unwrap_or_default();
                    let name = call
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    // Started, not completed: the result event closes it.
                    let _ = store.append(
                        &crate::context_store::ContextEvent::new(
                            format!("{}-{}-call-{call_id}-{}", self.session_id, self.turn_id.unwrap_or_default(),record_nonce()),
                            self.session_id.clone(),
                            crate::context_store::EventType::ToolCall,
                            crate::context_store::EventStatus::Started,
                        )
                        .with_turn(self.turn_id)
                        .with_tool_call(Some(call_id.to_string()))
                        .with_workspace(workspace.clone())
                        .with_source_ref(source_ref.clone())
                        .with_payload(&json!({
                            "entry_id": entry_id,
                            "tool": name,
                            "workspace_revision": self.workspace_revision,
                            "call_hash": crate::context_trace::content_hash(&format!("{name} {}", call["function"]["arguments"].as_str().unwrap_or_default())),
                            "invocation": crate::verification::provenance::invocation(workspace.as_deref().map(Path::new), name, &call["function"]["arguments"]),
                        })),
                    );
                }
            }
            Some("tool") => {
                let text = message_content_text(msg);
                let call_id = msg
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let status = match classify_tool_result(&text) {
                    None => crate::context_store::EventStatus::Completed,
                    Some(crate::tool_recovery::ToolFailureClass::Cancelled) => {
                        crate::context_store::EventStatus::Cancelled
                    }
                    Some(_) => crate::context_store::EventStatus::Failed,
                };
                let _ = store.append(
                    &crate::context_store::ContextEvent::new(
                        format!(
                            "{}-{}-result-{call_id}-{}",
                            self.session_id,
                            self.turn_id.unwrap_or_default(),
                            record_nonce()
                        ),
                        self.session_id.clone(),
                        crate::context_store::EventType::ToolResult,
                        status,
                    )
                    .with_turn(self.turn_id)
                    .with_tool_call(Some(call_id.to_string()))
                    .with_workspace(workspace.clone())
                    .with_source_ref(source_ref)
                    .with_payload(&json!({
                        "entry_id": entry_id,
                        "tool": msg.get("name").and_then(Value::as_str),
                        "result_hash": crate::context_trace::content_hash(&text),
                        "exit_code": parse_exit_code(&text),
                        "workspace_revision": self.workspace_revision,
                    })),
                );
            }
            _ => {}
        }
    }

    /// Points an event back at the immutable `_trace.id` of the JSONL entry it came from.
    fn source_ref(&self) -> Option<String> {
        let anchor = self
            .leaf_entry_id()
            .unwrap_or_else(|| format!("e{}", self.messages.len()));
        self.source_ref_for(&anchor)
    }

    fn source_ref_for(&self, entry_id: &str) -> Option<String> {
        self.log_path
            .as_ref()
            .map(|path| format!("{}#{entry_id}", path.display()))
    }

    /// Provenance store for this session, when it has a durable log.
    pub fn trace_store(&self) -> Option<&SourceTraceStore> {
        self.trace.as_ref()
    }

    /// Tag subsequent records with the active turn. Called by the agent loop at
    /// the start of each user task.
    pub fn set_turn_id(&mut self, turn_id: u64) {
        self.turn_id = Some(turn_id);
    }

    pub fn turn_id(&self) -> Option<u64> {
        self.turn_id
    }

    pub fn set_workspace(&mut self, workspace: &Path) {
        self.workspace = Some(
            workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.to_owned())
                .to_string_lossy()
                .into_owned(),
        );
    }

    /// Workspace revision every event in this turn is anchored to. Read once per
    /// turn by the caller rather than per event, so a tool result can be placed
    /// against the tree state it actually observed.
    pub fn set_workspace_revision(&mut self, revision: Option<String>) {
        self.workspace_revision = revision;
    }

    /// Search this session's recorded sources. Returns nothing when the session
    /// has no durable trace rather than falling back to the in-memory history,
    /// so a caller can never mistake live context for recorded provenance.
    pub fn retrieve_sources(&self, query: &str, max: usize) -> Vec<RetrievedSource> {
        self.trace
            .as_ref()
            .map(|store| store.retrieve(query, max))
            .unwrap_or_default()
    }

    /// Record a source that is not itself a session message, such as a
    /// compaction checkpoint summary.
    pub fn record_source(&self, kind: SourceKind, text: &str, status: Option<&str>) {
        let Some(store) = &self.trace else {
            return;
        };
        let mut record = self.new_record(kind, text);
        record.status = status.map(str::to_string);
        let _ = store.append(&record);
    }

    fn new_record(&self, kind: SourceKind, text: &str) -> SourceRecord {
        SourceRecord {
            source_id: format!("{}-{}", self.session_id, record_nonce()),
            session_id: self.session_id.clone(),
            task_id: None,
            turn_id: self.turn_id,
            timestamp: now_millis(),
            kind,
            text: text.chars().take(MAX_TRACE_EXCERPT_CHARS).collect(),
            content_hash: crate::context_trace::content_hash(text),
            workspace: self.workspace.clone(),
            ..SourceRecord::default()
        }
    }

    pub fn record_runtime_verification(
        &self,
        text: &str,
        provenance: crate::context_trace::RuntimeProvenance,
        status: &str,
    ) {
        use crate::context_store::{ContextEvent, EventStatus, EventType};
        let mut record = self.new_record(SourceKind::RuntimeVerification, text);
        record.workspace = Some(provenance.workspace.clone());
        record.status = Some(status.into());
        record.runtime = Some(provenance);
        if let Some(store) = &self.trace {
            let _ = store.append(&record);
        }
        if let Some(store) = &self.events {
            let _ = store.append(
                &ContextEvent::new(
                    record.source_id.clone(),
                    self.session_id.clone(),
                    EventType::RuntimeVerification,
                    if status == "passed" {
                        EventStatus::Completed
                    } else {
                        EventStatus::Failed
                    },
                )
                .with_turn(self.turn_id)
                .with_workspace(record.workspace.clone())
                .with_source_ref(Some(record.source_id.clone()))
                .with_payload(&json!({"record":record})),
            );
        }
    }

    pub fn record_verification_snapshot(
        &self,
        call_id: &str,
        before: &crate::verification::types::FileSnapshot,
        after: &crate::verification::types::FileSnapshot,
    ) {
        use crate::context_store::{ContextEvent, EventStatus, EventType};
        if let Some(store) = &self.events {
            let _ = store.append(
                &ContextEvent::new(
                    format!(
                        "{}-{}-snapshot-{call_id}",
                        self.session_id,
                        self.turn_id.unwrap_or_default()
                    ),
                    self.session_id.clone(),
                    EventType::VerificationSnapshot,
                    EventStatus::Completed,
                )
                .with_turn(self.turn_id)
                .with_workspace(self.workspace.clone())
                .with_tool_call(Some(call_id.into()))
                .with_source_ref(self.source_ref())
                .with_payload(&json!({"before":before,"after":after})),
            );
        }
    }

    /// Mirror one appended message into the provenance log. Only externally
    /// verifiable facts are recorded: user instructions, tool calls and tool
    /// results. Assistant prose is a proposal, not evidence, so it is skipped.
    fn append_trace(&self, msg: &Value) {
        let Some(store) = &self.trace else {
            return;
        };
        match role(msg) {
            Some("user") => {
                let text = message_content_text(msg);
                if !text.trim().is_empty() {
                    let _ = store.append(&self.new_record(SourceKind::UserMessage, &text));
                }
            }
            Some("assistant") => {
                let calls = msg.get("tool_calls").and_then(Value::as_array);
                for call in calls.into_iter().flatten() {
                    let function = call.get("function");
                    let name = function
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let mut record =
                        self.new_record(SourceKind::ToolCall, &format!("{name} {arguments}"));
                    record.tool_name = Some(name.to_string());
                    record.tool_call_id =
                        call.get("id").and_then(Value::as_str).map(str::to_string);
                    let _ = store.append(&record);
                }
            }
            Some("tool") => {
                let text = message_content_text(msg);
                let mut record = self.new_record(SourceKind::ToolResult, &text);
                record.tool_name = msg.get("name").and_then(Value::as_str).map(str::to_string);
                record.tool_call_id = msg
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                record.status = Some(match classify_tool_result(&text) {
                    Some(class) => class.to_string(),
                    None => "ok".to_string(),
                });
                let _ = store.append(&record);
            }
            _ => {}
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
        session.sync_trace_cursor();
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
        session.sync_trace_cursor();
        session
    }

    /// Synchronize the in-conversation trace sequence counter (`next_entry_seq`)
    /// and leaf cursor (`last_entry_id`) after restoring from JSONL, forking, or
    /// rewriting the message list during compaction.
    ///
    /// Guarantees that newly appended entries never collide with existing `eN`
    /// entry IDs and always link `parent_id` to the current leaf entry.
    pub(crate) fn sync_trace_cursor(&mut self) {
        let mut max_seq = 0u64;
        let mut last_id = None;
        for msg in &self.messages {
            if let Some(trace) = Self::message_trace(msg) {
                if let Some(seq) = trace
                    .id
                    .strip_prefix('e')
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    max_seq = max_seq.max(seq);
                }
                last_id = Some(trace.id);
            }
        }
        if max_seq >= self.next_entry_seq {
            self.next_entry_seq = max_seq + 1;
        }
        self.last_entry_id = last_id;
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
        self.add_assistant_with_meta(
            content,
            tool_calls,
            reasoning,
            None,
            None,
            &std::collections::BTreeMap::new(),
        );
    }

    /// Append an assistant message with per-step model, finish_reason, and token usage trace metadata.
    pub fn add_assistant_with_meta(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<Value>>,
        reasoning: &str,
        model: Option<&str>,
        finish_reason: Option<&str>,
        usage: &std::collections::BTreeMap<String, i64>,
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
        let mut seed_trace = MessageTrace {
            model: model
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            finish_reason: finish_reason
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            usage: usage.clone(),
            ..Default::default()
        };
        if seed_trace.model.is_some()
            || seed_trace.finish_reason.is_some()
            || !seed_trace.usage.is_empty()
        {
            seed_trace.id = String::new();
            if let Ok(trace_val) = serde_json::to_value(&seed_trace) {
                msg.insert("_trace".into(), trace_val);
            }
        }
        self.append(Value::Object(msg));
    }

    /// Append a tool result message answering a specific tool_call id.
    pub fn add_tool_result(&mut self, call_id: &str, name: &str, result: &str) {
        self.add_tool_result_timed(call_id, name, result, None);
    }

    /// Append a tool result message with measured execution duration in milliseconds.
    pub fn add_tool_result_timed(
        &mut self,
        call_id: &str,
        name: &str,
        result: &str,
        duration_ms: Option<u64>,
    ) {
        let mut msg = json!({
            "role": "tool",
            "tool_call_id": call_id,
            "name": name,
            "content": result,
        });
        if let Some(ms) = duration_ms {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("_trace".into(), json!({"id": "", "timestamp": 0, "duration_ms": ms}));
            }
        }
        self.append(msg);
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
        out.extend(body.into_iter().map(|mut message| {
            strip_internal_fields(&mut message);
            message
        }));
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
        let (mut body, stats) = self.edited_body(&[], &policy);
        if stats.compressed_tool_results > 0 || stats.dropped_messages > 0 {
            if stats.dropped_messages > 0 {
                let first_kept_entry_id = body
                    .iter()
                    .find_map(Self::message_trace)
                    .map(|t| t.id);
                if let Some(first) = body.first_mut() {
                    if Self::message_trace(first).is_none() {
                        self.attach_compaction_trace(
                            first,
                            CompactionTrace {
                                first_kept_entry_id,
                                dropped_messages: stats.dropped_messages,
                                compressed_tool_results: stats.compressed_tool_results,
                                chars_before: stats.original_chars,
                                chars_after: stats.edited_chars,
                            },
                        );
                    }
                }
            }
            self.messages = body;
            self.sync_trace_cursor();
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

    pub fn compact_safely_if_needed(
        &mut self,
        policy: &ContextEditPolicy,
        workspace: &Path,
    ) -> Option<SafeCompactionResult> {
        if !self.needs_compaction(policy) {
            return None;
        }
        let before = safety_snapshot(&self.messages, workspace);
        let marker = safety_marker(&before);
        let stats = self.compact(policy);
        if stats.dropped_messages == 0 && stats.compressed_tool_results == 0 {
            return None;
        }
        if !marker.is_empty() {
            let first_kept_entry_id = self
                .messages
                .iter()
                .find_map(Self::message_trace)
                .map(|t| t.id);
            let mut marker_msg = json!({"role": "user", "content": marker});
            self.attach_compaction_trace(
                &mut marker_msg,
                CompactionTrace {
                    first_kept_entry_id,
                    dropped_messages: stats.dropped_messages,
                    compressed_tool_results: stats.compressed_tool_results,
                    chars_before: stats.original_chars,
                    chars_after: stats.edited_chars,
                },
            );
            self.messages.insert(0, marker_msg);
            self.sync_trace_cursor();
            self.rewrite_log();
        }
        let after = safety_snapshot(&self.messages, workspace);
        let conflicts = compare_safety_snapshots(&before, &after);
        Some(SafeCompactionResult {
            stats,
            before,
            after,
            conflicts,
        })
    }

    /// Attach a Pi-style `CompactionTrace` (`first_kept_entry_id`, dropped/compressed
    /// counts, character budget delta) to a synthetic compaction checkpoint entry.
    pub(crate) fn attach_compaction_trace(&mut self, msg: &mut Value, compaction: CompactionTrace) {
        let entry_id = format!("e{}", self.next_entry_seq);
        self.next_entry_seq += 1;
        let text = message_content_text(msg);
        let trace = MessageTrace {
            id: entry_id,
            parent_id: self.last_entry_id.clone(),
            turn_id: self.turn_id,
            timestamp: now_millis(),
            content_hash: crate::context_trace::content_hash(&text),
            workspace_revision: self.workspace_revision.clone(),
            compaction: Some(compaction),
            ..Default::default()
        };
        if let (Some(obj), Ok(val)) = (msg.as_object_mut(), serde_json::to_value(trace)) {
            obj.insert("_trace".into(), val);
        }
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

    /// Populate or enrich the in-conversation `_trace` (`MessageTrace`) object on a
    /// message before appending it to `self.messages` and `session.jsonl`.
    ///
    /// Assigns the next monotonic `id` (`e1`, `e2`, ...), links `parent_id` to the
    /// previous leaf entry, stamps `turn_id`, `timestamp`, `workspace_revision`, and
    /// `content_hash`, and extracts normalized tool telemetry (`status`, `failure_class`,
    /// `exit_code`) for `"tool"` role entries.
    fn populate_message_trace(&mut self, msg: &mut Value) -> String {
        let entry_id = format!("e{}", self.next_entry_seq);
        self.next_entry_seq += 1;
        let parent_id = self.last_entry_id.clone();
        self.last_entry_id = Some(entry_id.clone());

        let mut trace = Self::message_trace(msg).unwrap_or_default();
        trace.id = entry_id.clone();
        trace.parent_id = parent_id;
        trace.turn_id = self.turn_id;
        trace.timestamp = now_millis();
        trace.workspace_revision = self.workspace_revision.clone();

        let text = message_content_text(msg);
        if !text.is_empty() {
            trace.content_hash = crate::context_trace::content_hash(&text);
        } else if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
            if let Ok(serialized) = serde_json::to_string(calls) {
                trace.content_hash = crate::context_trace::content_hash(&serialized);
            }
        }

        if role(msg) == Some("tool") {
            let failure = classify_tool_result(&text);
            trace.status = Some(match failure {
                None => "completed".to_string(),
                Some(crate::tool_recovery::ToolFailureClass::Cancelled) => "cancelled".to_string(),
                Some(_) => "failed".to_string(),
            });
            trace.failure_class = failure.map(|class| class.to_string());
            trace.exit_code = parse_exit_code(&text);
        }

        if let (Some(obj), Ok(val)) = (msg.as_object_mut(), serde_json::to_value(&trace)) {
            obj.insert("_trace".into(), val);
        }
        entry_id
    }

    fn append(&mut self, mut msg: Value) {
        let entry_id = self.populate_message_trace(&mut msg);
        self.append_trace(&msg);
        self.append_event(&msg, &entry_id);
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

fn safety_snapshot(messages: &[Value], workspace: &Path) -> CompactionSafetySnapshot {
    let users = messages
        .iter()
        .filter(|m| role(m) == Some("user"))
        .map(message_content_text)
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();
    let all = messages
        .iter()
        .map(message_content_text)
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();
    let pick = |needles: &[&str], limit: usize| {
        all.iter()
            .rev()
            .filter(|text| needles.iter().any(|needle| text.contains(needle)))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut files = Vec::new();
    for text in &all {
        for token in text.split_whitespace() {
            let clean = token.trim_matches(|c: char| "`'\"()[]{}，。,:：；".contains(c));
            if (clean.contains('/') || clean.contains('\\'))
                && clean
                    .rsplit('.')
                    .next()
                    .is_some_and(|ext| (1..=8).contains(&ext.len()))
                && !files.iter().any(|item| item == clean)
            {
                files.push(clean.to_string());
            }
        }
    }
    files.truncate(24);
    let confirmed_decisions = all
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, text)| {
            ["确认", "决定", "就按", "可以这么做", "方案"]
                .iter()
                .any(|needle| text.contains(needle))
        })
        .take(8)
        .map(|(index, text)| {
            format!(
                "D{} {}{}",
                index + 1,
                if ["替代", "改为", "后来", "不再"]
                    .iter()
                    .any(|needle| text.contains(needle))
                {
                    "[替代旧决策] "
                } else {
                    ""
                },
                text
            )
        })
        .collect();
    // The opening requests plus the latest one. Taking only the first few would
    // carry the task's origin into the checkpoint while dropping what the user
    // most recently asked for, which is the intent currently in force.
    let mut original_requirements: Vec<String> = users.iter().take(3).cloned().collect();
    if let Some(latest) = users.last() {
        if !original_requirements.contains(latest) {
            original_requirements.push(latest.clone());
        }
    }
    CompactionSafetySnapshot {
        original_requirements,
        confirmed_decisions,
        prohibitions: pick(
            &[
                "不要",
                "不能",
                "禁止",
                "先别",
                "不允许",
                "不要恢复",
                "不要删除",
                "不要提交",
            ],
            12,
        ),
        completed: pick(&["已完成", "已经修复", "通过", "完成"], 8),
        pending: pick(&["未完成", "待办", "下一步", "还没", "需要"], 8),
        files,
        git_diff_summary: git_diff_summary(workspace),
        recent_tests: pick(&["test", "测试", "passed", "通过"], 8),
    }
}

fn git_diff_summary(workspace: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["diff", "--stat", "--", "."])
        .output();
    output
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .chars()
                .take(2_000)
                .collect()
        })
        .unwrap_or_default()
}

fn safety_marker(snapshot: &CompactionSafetySnapshot) -> String {
    let section = |name: &str, values: &[String]| {
        if values.is_empty() {
            String::new()
        } else {
            format!(
                "\n{name}:\n{}",
                values
                    .iter()
                    .map(|v| format!("- {}", v.chars().take(500).collect::<String>()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    };
    format!("[压缩安全快照；仅用于恢复方向，代码事实须重新核对工作区/diff/测试]{}{}{}{}{}{}\ngit diff 摘要:\n{}",
        section("原始要求", &snapshot.original_requirements), section("已确认决策", &snapshot.confirmed_decisions),
        section("不可丢失禁止事项", &snapshot.prohibitions), section("已完成", &snapshot.completed),
        section("未完成", &snapshot.pending), section("涉及文件", &snapshot.files), snapshot.git_diff_summary)
}

fn compare_safety_snapshots(
    before: &CompactionSafetySnapshot,
    after: &CompactionSafetySnapshot,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    for (name, expected, actual) in [
        (
            "任务目标",
            &before.original_requirements,
            &after.original_requirements,
        ),
        (
            "已确认决策",
            &before.confirmed_decisions,
            &after.confirmed_decisions,
        ),
        ("禁止事项", &before.prohibitions, &after.prohibitions),
        ("涉及文件", &before.files, &after.files),
    ] {
        if expected.iter().any(|item| {
            !actual.iter().any(|candidate| {
                let retained: String = item.chars().take(500).collect();
                candidate.contains(&retained) || retained.contains(candidate)
            })
        }) {
            conflicts.push(format!("{name}在压缩后不一致"));
        }
    }
    if before.git_diff_summary != after.git_diff_summary {
        conflicts.push("压缩期间 git diff 状态发生变化".into());
    }
    conflicts
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

/// Exit code reported by a tool result, when it declares one.
///
/// This reads the documented `Exit code: N` line of the shell result contract —
/// the same string contract `classify_tool_result` already depends on. Tools
/// that do not report an exit code simply have none; a missing value is never
/// inferred to be success.
fn parse_exit_code(text: &str) -> Option<i32> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("Exit code:"))
        .filter_map(|code| code.trim().parse::<i32>().ok())
        .next_back()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn record_nonce() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}",
        now_millis(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

fn now_stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{:013}", d.as_millis()))
        .unwrap_or_else(|_| "0000000000000".into())
}

pub(crate) fn strip_internal_fields(v: &mut Value) {
    if let Some(object) = v.as_object_mut() {
        object.remove("_compaction_safety");
        object.remove("_trace");
        object.remove("_ts");
    }
}

fn json_chars(v: &Value) -> usize {
    let mut wire = v.clone();
    strip_internal_fields(&mut wire);
    serde_json::to_string(&wire)
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
    fn records_facts_but_not_assistant_prose_in_the_provenance_trace() {
        let dir = crate::test_support::unique_temp_dir("session_trace_facts");
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::with_log("sys", Some(dir.join("s.jsonl")));
        session.set_turn_id(7);
        session.add_user_text("不要提交任何改动");
        session.add_assistant(
            "我打算先读文件",
            Some(vec![json!({"id":"call-1","type":"function",
                "function":{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"}})]),
            "internal thinking",
        );
        session.add_tool_result("call-1", "read_file", "Error: not found");

        let records = session.trace_store().unwrap().records();
        let kinds: Vec<_> = records.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SourceKind::UserMessage,
                SourceKind::ToolCall,
                SourceKind::ToolResult
            ],
            "assistant prose and reasoning must not become evidence"
        );
        assert!(records.iter().all(|r| r.turn_id == Some(7)));
        assert_eq!(records[1].tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(records[2].status.as_deref(), Some("notfound"));
        assert_eq!(
            records[0].content_hash,
            crate::context_trace::content_hash("不要提交任何改动")
        );
    }

    #[test]
    fn compaction_rewrites_the_log_but_the_original_instruction_stays_recallable() {
        let dir = crate::test_support::unique_temp_dir("session_trace_compaction");
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::with_log("sys", Some(dir.join("s.jsonl")));
        session.add_user_text("调查 releases 目录");
        for i in 0..40 {
            session.add_assistant(
                "",
                Some(vec![json!({"id":format!("c{i}"),"type":"function",
                    "function":{"name":"read_file","arguments":"{}"}})]),
                "",
            );
            let body = if i == 0 {
                format!("releases 校验和 {}", "内容".repeat(400))
            } else {
                "内容".repeat(400)
            };
            session.add_tool_result(&format!("c{i}"), "read_file", &body);
        }
        let plan = session.prepare_compaction(20_000, &dir).unwrap();
        session
            .commit_compaction(
                plan,
                &ncx_provider::ModelResponse {
                    content: "Goal: 调查。Next steps: 继续。".into(),
                    finish_reason: "stop".into(),
                    ..Default::default()
                },
                &dir,
            )
            .unwrap();

        // The rewritten JSONL no longer holds the summarized tool evidence...
        let log = std::fs::read_to_string(dir.join("s.jsonl")).unwrap();
        assert!(!log.contains("releases 校验和"));
        // ...but the append-only trace still does, and recall finds it.
        let found = session.retrieve_sources("releases 校验和", 5);
        assert!(
            found
                .iter()
                .any(|item| item.record.text.contains("releases 校验和")),
            "{found:?}"
        );
        assert!(session
            .trace_store()
            .unwrap()
            .records()
            .iter()
            .any(|r| r.kind == SourceKind::SessionSummary));
    }

    fn session_with_events(label: &str) -> (Session, std::path::PathBuf) {
        let dir = crate::test_support::unique_temp_dir(label);
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::with_log("sys", Some(dir.join("s.jsonl")));
        session.set_event_store(std::rc::Rc::new(
            crate::context_store::ContextStore::open(dir.join("events.sqlite3")).unwrap(),
        ));
        (session, dir)
    }

    #[test]
    fn tool_calls_and_results_are_paired_by_call_id_with_terminal_status() {
        let (mut session, _dir) = session_with_events("session_events_pairing");
        session.set_turn_id(2);
        session.add_user_text("跑一下测试");
        session.add_assistant(
            "",
            Some(vec![
                json!({"id":"ok-1","type":"function","function":{"name":"shell","arguments":"{}"}}),
                json!({"id":"bad-1","type":"function","function":{"name":"shell","arguments":"{}"}}),
            ]),
            "",
        );
        session.add_tool_result("ok-1", "shell", "all tests passed");
        session.add_tool_result("bad-1", "shell", "Error: command not found");

        let store = session.event_store().unwrap();
        assert_eq!(
            store.tool_call_status("ok-1").unwrap(),
            Some(crate::context_store::EventStatus::Completed)
        );
        assert_eq!(
            store.tool_call_status("bad-1").unwrap(),
            Some(crate::context_store::EventStatus::Failed)
        );
        // A recorded failure is still a resolved call.
        assert!(session.unresolved_tool_calls().is_empty());
        let events = store.session_events(&session.session_id).unwrap();
        assert!(events.iter().all(|event| event.turn_id == Some(2)));
        assert!(events.iter().all(|event| event
            .source_ref
            .as_deref()
            .is_some_and(|r| r.contains("s.jsonl"))));
    }

    #[test]
    fn a_tool_call_query_reports_exit_code_revision_and_source_together() {
        let (mut session, _dir) = session_with_events("session_events_detail");
        session.set_turn_id(5);
        session.set_workspace_revision(Some("abc1234".into()));
        session.add_assistant(
            "",
            Some(vec![json!({"id":"sh-1","type":"function",
                "function":{"name":"shell","arguments":"{}"}})]),
            "",
        );
        session.add_tool_result("sh-1", "shell", "STDERR: boom\n\nExit code: 2");

        let detail = session
            .event_store()
            .unwrap()
            .tool_call_detail("sh-1")
            .unwrap()
            .expect("a recorded call must be queryable");
        assert_eq!(detail.tool.as_deref(), Some("shell"));
        assert_eq!(detail.status, crate::context_store::EventStatus::Failed);
        assert_eq!(detail.exit_code, Some(2));
        assert_eq!(detail.workspace_revision.as_deref(), Some("abc1234"));
        assert_eq!(detail.turn_id, Some(5));
        assert!(detail.source_ref.is_some());
        assert!(detail.result_hash.is_some());
    }

    #[test]
    fn a_call_with_no_recorded_outcome_reports_unknown_not_success() {
        let (mut session, _dir) = session_with_events("session_events_unknown_detail");
        session.add_assistant(
            "",
            Some(vec![json!({"id":"open-1","type":"function",
                "function":{"name":"apply_patch","arguments":"{}"}})]),
            "",
        );

        let detail = session
            .event_store()
            .unwrap()
            .tool_call_detail("open-1")
            .unwrap()
            .unwrap();
        assert_eq!(detail.status, crate::context_store::EventStatus::Unknown);
        // A tool that never reported an exit code does not get one invented.
        assert_eq!(detail.exit_code, None);
    }

    #[test]
    fn a_tool_that_reports_no_exit_code_is_not_given_one() {
        let (mut session, _dir) = session_with_events("session_events_no_exit_code");
        session.add_assistant(
            "",
            Some(vec![json!({"id":"r-1","type":"function",
                "function":{"name":"read_file","arguments":"{}"}})]),
            "",
        );
        session.add_tool_result("r-1", "read_file", "file contents");

        let detail = session
            .event_store()
            .unwrap()
            .tool_call_detail("r-1")
            .unwrap()
            .unwrap();
        assert_eq!(detail.status, crate::context_store::EventStatus::Completed);
        assert_eq!(detail.exit_code, None);
    }

    #[test]
    fn an_unanswered_tool_call_blocks_compaction_instead_of_being_summarized_away() {
        let (mut session, dir) = session_with_events("session_events_unresolved");
        session.add_user_text("改一下配置");
        for i in 0..40 {
            session.add_assistant(
                "",
                Some(vec![json!({"id":format!("c{i}"),"type":"function",
                    "function":{"name":"read_file","arguments":"{}"}})]),
                "",
            );
            session.add_tool_result(&format!("c{i}"), "read_file", &"内容".repeat(400));
        }
        // A write whose result never arrived: its effect on the workspace is
        // unknown, so the history containing it is not finished history.
        session.add_assistant(
            "",
            Some(vec![json!({"id":"dangling","type":"function",
                "function":{"name":"apply_patch","arguments":"{}"}})]),
            "",
        );

        assert_eq!(
            session.unresolved_tool_calls(),
            vec!["dangling".to_string()]
        );
        let error = match session.prepare_compaction(20_000, &dir) {
            Err(error) => error,
            Ok(_) => panic!("compaction must refuse while a call is unresolved"),
        };
        assert!(error.contains("dangling"), "{error}");

        // Once the result is recorded, compaction may proceed.
        session.add_tool_result("dangling", "apply_patch", "applied");
        assert!(session.unresolved_tool_calls().is_empty());
        assert!(session.prepare_compaction(20_000, &dir).is_ok());
    }

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
        let dir = crate::test_support::unique_temp_dir("ncx_session_compact");
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
    fn safe_compaction_preserves_prohibitions_and_workspace_evidence() {
        let root = crate::test_support::unique_temp_dir("ncx_safe_compact");
        std::fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("system");
        session.add_user_text("原始要求：修改 src/app.rs，但不要删除旧实现，不要提交代码");
        for index in 0..20 {
            session.add_assistant(&format!("普通讨论 {index} {}", "x".repeat(120)), None, "");
            session.add_user_text(&format!("继续讨论 {index}"));
        }
        let result = session
            .compact_safely_if_needed(
                &ContextEditPolicy {
                    enabled: true,
                    max_chars: 900,
                    keep_recent_messages: 4,
                    max_tool_result_chars: 40,
                },
                &root,
            )
            .expect("compaction");
        let rendered = serde_json::to_string(&session.messages).unwrap();
        assert!(rendered.contains("不要删除旧实现"));
        assert!(rendered.contains("不要提交代码"));
        assert!(rendered.contains("代码事实须重新核对"));
        assert!(result.conflicts.is_empty(), "{:?}", result.conflicts);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_compaction_validates_long_requirements_at_the_marker_boundary() {
        let root = crate::test_support::unique_temp_dir("ncx_safe_compact_long");
        std::fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("system");
        session.add_user_text(&format!(
            "原始要求：制作一份长篇交付物，但不要删除现有文件。{}",
            "需要保留的详细背景。".repeat(100)
        ));
        for index in 0..20 {
            session.add_assistant(&format!("普通讨论 {index} {}", "x".repeat(120)), None, "");
            session.add_user_text(&format!("继续讨论 {index}"));
        }

        let result = session
            .compact_safely_if_needed(
                &ContextEditPolicy {
                    enabled: true,
                    max_chars: 900,
                    keep_recent_messages: 4,
                    max_tool_result_chars: 40,
                },
                &root,
            )
            .expect("compaction");

        assert!(result.conflicts.is_empty(), "{:?}", result.conflicts);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn logs_messages_as_jsonl_and_resumes_body() {
        let dir = crate::test_support::unique_temp_dir("ncx_session_log");
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
        let dir = crate::test_support::unique_temp_dir("ncx_session_resume");
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
        let dir = crate::test_support::unique_temp_dir("ncx_session_image");
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
        let dir = crate::test_support::unique_temp_dir("ncx_session_fork");
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

    #[test]
    fn pi_style_in_conversation_trace_links_parent_ids_and_strips_on_for_model() {
        let dir = crate::test_support::unique_temp_dir("ncx_pi_trace_tree");
        let path = dir.join("session.jsonl");
        let mut s = Session::with_log("sys", Some(path.clone()));
        s.set_turn_id(7);
        s.set_workspace_revision(Some("abc1234".to_string()));

        s.add_user_text("inspect Cargo.toml");
        let mut usage = std::collections::BTreeMap::new();
        usage.insert("prompt_tokens".to_string(), 120i64);
        usage.insert("completion_tokens".to_string(), 35i64);
        s.add_assistant_with_meta(
            "",
            Some(vec![json!({
                "id": "call_read_1",
                "type": "function",
                "function": {"name": "read_file", "arguments": "{\"path\":\"Cargo.toml\"}"}
            })]),
            "need to read Cargo.toml",
            Some("deepseek-reasoner"),
            Some("tool_calls"),
            &usage,
        );
        s.add_tool_result_timed(
            "call_read_1",
            "read_file",
            "[package]\nname = \"ncx\"\nExit code: 0",
            Some(18),
        );

        let traces = s.trace_entries();
        assert_eq!(traces.len(), 3);
        assert_eq!(traces[0].id, "e1");
        assert_eq!(traces[0].parent_id, None);
        assert_eq!(traces[0].turn_id, Some(7));
        assert_eq!(traces[0].workspace_revision.as_deref(), Some("abc1234"));

        assert_eq!(traces[1].id, "e2");
        assert_eq!(traces[1].parent_id.as_deref(), Some("e1"));
        assert_eq!(traces[1].model.as_deref(), Some("deepseek-reasoner"));
        assert_eq!(traces[1].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(traces[1].usage.get("prompt_tokens"), Some(&120));

        assert_eq!(traces[2].id, "e3");
        assert_eq!(traces[2].parent_id.as_deref(), Some("e2"));
        assert_eq!(traces[2].status.as_deref(), Some("completed"));
        assert_eq!(traces[2].exit_code, Some(0));
        assert_eq!(traces[2].duration_ms, Some(18));
        assert_eq!(s.leaf_entry_id().as_deref(), Some("e3"));

        // Wire messages sent to the LLM must strip internal `_trace` metadata.
        for msg in s.for_model() {
            assert!(
                msg.get("_trace").is_none(),
                "wire format leaked _trace: {msg}"
            );
        }

        // Resuming from JSONL preserves the trace tree and continues sequence numbering.
        let mut resumed = Session::resume("sys", Some(path));
        let resumed_traces = resumed.trace_entries();
        assert_eq!(resumed_traces, traces);
        resumed.add_user_text("follow-up");
        let follow = Session::message_trace(&resumed.messages[3]).expect("follow-up trace");
        assert_eq!(follow.id, "e4");
        assert_eq!(follow.parent_id.as_deref(), Some("e3"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compaction_checkpoint_embeds_first_kept_entry_id_and_links_tail() {
        let dir = crate::test_support::unique_temp_dir("ncx_pi_compaction_trace");
        let path = dir.join("session.jsonl");
        let mut s = Session::with_log("sys", Some(path.clone()));

        s.add_user_text("initial request");
        for i in 0..8 {
            let call_id = format!("call_{i}");
            s.add_assistant(
                "",
                Some(vec![json!({
                    "id": call_id,
                    "type": "function",
                    "function": {"name": "read_file", "arguments": "{}"}
                })]),
                "",
            );
            s.add_tool_result(&call_id, "read_file", &"x".repeat(600));
        }
        s.add_user_text("recent question that stays in tail");
        s.add_assistant("recent answer", None, "");

        let stats = s.compact(&ContextEditPolicy {
            enabled: true,
            max_chars: 1_200,
            keep_recent_messages: 2,
            max_tool_result_chars: 100,
        });
        assert!(stats.dropped_messages > 0);

        let checkpoint_trace = Session::message_trace(&s.messages[0]).expect("checkpoint trace");
        let compaction = checkpoint_trace
            .compaction
            .as_ref()
            .expect("compaction trace on checkpoint message");
        assert_eq!(compaction.dropped_messages, stats.dropped_messages);
        assert!(compaction.first_kept_entry_id.is_some());

        let first_tail_trace = Session::message_trace(&s.messages[1]).expect("first tail trace");
        assert_eq!(
            compaction.first_kept_entry_id.as_deref(),
            Some(first_tail_trace.id.as_str())
        );

        // Next turn after compaction links cleanly to the leaf of the kept tail.
        let expected_parent = s.leaf_entry_id().unwrap();
        s.add_user_text("post-compaction message");
        let new_leaf = Session::message_trace(s.messages.last().unwrap()).unwrap();
        assert_eq!(new_leaf.parent_id.as_deref(), Some(expected_parent.as_str()));

        let _ = std::fs::remove_dir_all(dir);
    }
}
