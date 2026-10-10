//! Rebuild the existing instruction ledger from original human events.
use super::types::*;
use crate::{
    context_store::{ContextEvent, EventType},
    context_trace::{content_hash, SourceKind, SourceRecord},
    instruction_state::{InstructionLedger, InstructionStatus, Polarity},
    session::Session,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn attachment_metadata(content: &Value) -> Vec<Value> {
    content.as_array().into_iter().flatten().filter(|p| !matches!(p["type"].as_str(),Some("text"|"input_text")))
        .map(|p| json!({"type":p["type"],"name":p.get("filename").or_else(||p.get("name")),"mime_type":p.get("mime_type")})).collect()
}

fn explicit(text: &str) -> Option<(Polarity, String)> {
    let low = text.trim().to_lowercase();
    let markers = [
        ("不要", Polarity::Forbid),
        ("禁止", Polarity::Forbid),
        ("不得", Polarity::Forbid),
        ("允许", Polarity::Allow),
        ("可以", Polarity::Allow),
        ("撤销", Polarity::Revoke),
        ("取消", Polarity::Revoke),
        ("do not ", Polarity::Forbid),
        ("don't ", Polarity::Forbid),
        ("you may ", Polarity::Allow),
        ("allow ", Polarity::Allow),
        ("revoke ", Polarity::Revoke),
    ];
    let cleaned = low
        .trim_start_matches("现在")
        .trim_start_matches("请")
        .trim_start_matches("now ");
    let (marker, polarity) = markers.iter().find(|(m, _)| cleaned.starts_with(m))?;
    let mut subject = cleaned[marker.len()..].trim().to_owned();
    for filler in ["之前", "原先", "previous ", "the ", "permission to "] {
        subject = subject.trim_start_matches(filler).to_owned();
    }
    for suffix in ["的要求", "的限制"] {
        subject = subject.trim_end_matches(suffix).to_owned();
    }
    subject = subject
        .trim_matches(|c: char| c.is_whitespace() || ".。!！".contains(c))
        .to_owned();
    (!subject.is_empty()).then_some((*polarity, subject))
}

pub fn collect(
    session: &Session,
    events: &[ContextEvent],
    traces: &[SourceRecord],
    workspace: &str,
) -> (Vec<String>, Vec<RequirementSource>, String, Vec<String>) {
    let mut ledger = InstructionLedger::new();
    let mut metadata = BTreeMap::new();
    let mut reasons = vec![];
    let mut human_count = 0;
    for event in events
        .iter()
        .filter(|e| e.event_type == EventType::UserMessage)
    {
        let Ok(payload) = serde_json::from_str::<Value>(&event.payload_json) else {
            reasons.push("requirement_source_invalid".into());
            continue;
        };
        let trace = traces.iter().find(|t| {
            t.kind == SourceKind::UserMessage
                && t.session_id == session.session_id
                && t.turn_id == event.turn_id
                && payload["text_hash"] == t.content_hash
        });
        let text = payload["text"].as_str().map(str::to_owned).or_else(|| {
            trace
                .filter(|t| content_hash(&t.text) == t.content_hash)
                .map(|t| t.text.clone())
        });
        let Some(text) = text else {
            reasons.push("requirement_history_incomplete".into());
            continue;
        };
        if event.workspace.as_deref() != Some(workspace) {
            reasons.push("requirement_foreign_workspace".into());
            continue;
        }
        if trace.is_none() || payload["text_truncated"] == true || payload["text_redacted"] == true
        {
            reasons.push("requirement_history_incomplete".into());
        }
        human_count += 1;
        if payload["attachment_metadata"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            reasons.push("attachment_content_unverified".into());
        }
        let mut fence = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("```") || line.starts_with("~~~") {
                fence = !fence;
                continue;
            }
            if fence
                || line.starts_with('>')
                || line.starts_with(crate::session::COMPACTED_HISTORY_PREFIX)
            {
                continue;
            }
            for clause in line.split(['。', ';', '；']).flat_map(|s| s.split(". ")) {
                let clause = clause.trim();
                if clause.is_empty() {
                    continue;
                }
                // Quotes are evidence/example content, not an instruction to us.
                if clause.starts_with(['"', '\'', '“', '「']) {
                    continue;
                }
                let parsed = explicit(clause);
                let low = clause.to_lowercase();
                let embedded = [
                    " and do not ",
                    " and don't ",
                    " but do not ",
                    " and you may ",
                    "不要",
                    "不得",
                    "禁止",
                ]
                .iter()
                .any(|marker| low.contains(marker) && !low.starts_with(marker));
                if embedded {
                    reasons.push("embedded_requirement_unparsed".into());
                }
                if parsed.is_none()
                    && !(clause.starts_with("Run ")
                        || clause.starts_with("run ")
                        || clause.starts_with("运行 ")
                        || clause.starts_with("执行 "))
                {
                    reasons.push("unparsed_requirement".into());
                }
                let (polarity, subject) =
                    parsed.unwrap_or((Polarity::Allow, format!("raw:{}", content_hash(clause))));
                let id = ledger.record_exact(
                    clause,
                    Some(workspace.into()),
                    polarity,
                    subject,
                    event.sequence as u64,
                );
                metadata.insert(
                    id,
                    (
                        event,
                        trace,
                        payload["attachment_metadata"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default(),
                    ),
                );
                if clause.contains("忽略之前") || clause.to_lowercase().contains("ignore previous")
                {
                    reasons.push("ambiguous_revocation".into());
                }
            }
        }
    }
    if human_count == 0 {
        reasons.push("requirement_history_incomplete".into());
        for text in session
            .messages
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| super::evidence::content_text(&m["content"]))
        {
            ledger.record_exact(
                &text,
                Some(workspace.into()),
                Polarity::Allow,
                format!("unproven:{}", content_hash(&text)),
                0,
            );
        }
    }
    if !ledger.conflicts().is_empty() {
        reasons.push("requirement_conflict".into());
    }
    let sources = ledger
        .all()
        .iter()
        .filter_map(|instruction| {
            let (e, t, attachments) = metadata.get(&instruction.id)?;
            Some(RequirementSource {
                source_ref: t
                    .map(|t| t.source_id.clone())
                    .unwrap_or_else(|| e.event_id.clone()),
                turn_id: e.turn_id,
                sequence: e.sequence,
                workspace: content_hash(workspace),
                content_hash: serde_json::from_str::<Value>(&e.payload_json).ok()?["text_hash"]
                    .as_str()?
                    .into(),
                instruction: instruction.clone(),
                attachment_metadata: attachments.clone(),
            })
        })
        .collect();
    let active = ledger
        .active()
        .iter()
        .filter(|i| {
            i.polarity != Polarity::Revoke
                && matches!(
                    i.status,
                    InstructionStatus::Active | InstructionStatus::Conflicted
                )
        })
        .map(|i| i.text.clone())
        .collect();
    let coverage = if reasons.is_empty() {
        "complete"
    } else if reasons
        .iter()
        .any(|r| r == "requirement_conflict" || r == "ambiguous_revocation")
    {
        "conflicted"
    } else {
        "unknown"
    };
    (active, sources, coverage.into(), reasons)
}
