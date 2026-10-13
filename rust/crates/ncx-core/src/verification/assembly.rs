use super::{
    evidence::{redact, test_scope, workspace_snapshot, CheckSnapshots},
    extraction, provenance, requirements,
    types::*,
};
use crate::{
    context_store::{ContextEvent, EventType},
    context_trace::{content_hash, SourceKind},
    session::Session,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn payload(e: &ContextEvent) -> Value {
    serde_json::from_str(&e.payload_json).unwrap_or(Value::Null)
}
fn evidence_base() -> Evidence {
    Evidence {
        id: String::new(),
        session_id: String::new(),
        turn_id: None,
        workspace: String::new(),
        source_ref: String::new(),
        kind: String::new(),
        tool_call_id: None,
        tool: None,
        command: None,
        status: "unknown".into(),
        exit_code: None,
        timestamp: 0,
        content_hash: String::new(),
        excerpt: String::new(),
        truncated: true,
        scope: vec![],
        before: None,
        after: None,
        sequence: 0,
        call_sequence: None,
        workdir: None,
        target_id: None,
        target_paths: vec![],
        authoritative: false,
        superseded_by: None,
        checker: None,
        checked_hashes: BTreeMap::new(),
    }
}

pub fn packet(
    session: &Session,
    root: &Path,
    candidate: &str,
    checks: &CheckSnapshots,
) -> VerificationInput {
    let root = root.canonicalize().unwrap_or_else(|_| root.into());
    let workspace = root.to_string_lossy().into_owned();
    let workspace_id = content_hash(&workspace);
    let current_files = workspace_snapshot(session, &root);
    let traces = session
        .trace_store()
        .map(|s| s.records())
        .unwrap_or_default();
    let events = session
        .event_store()
        .and_then(|s| s.session_events(&session.session_id).ok())
        .unwrap_or_default();
    let (requirements, requirement_sources, requirements_coverage, mut limitations) =
        requirements::collect(session, &events, &traces, &workspace);
    let mut evidence = vec![];
    for event in events
        .iter()
        .filter(|e| e.turn_id == session.turn_id() && e.event_type == EventType::ToolResult)
    {
        let data = payload(event);
        let id = event.tool_call_id.as_deref().unwrap_or_default();
        let calls: Vec<_> = events
            .iter()
            .filter(|e| {
                e.turn_id == event.turn_id
                    && e.event_type == EventType::ToolCall
                    && e.tool_call_id.as_deref() == Some(id)
                    && e.sequence < event.sequence
            })
            .collect();
        let call = calls.last().copied();
        let call_data = call.map(payload).unwrap_or(Value::Null);
        let invocation = &call_data["invocation"];
        let trace = traces.iter().rev().find(|t| {
            t.kind == SourceKind::ToolResult
                && t.turn_id == event.turn_id
                && t.tool_call_id.as_deref() == Some(id)
                && data["result_hash"] == t.content_hash
        });
        let call_trace = traces.iter().rev().find(|t| {
            t.kind == SourceKind::ToolCall
                && t.turn_id == event.turn_id
                && t.tool_call_id.as_deref() == Some(id)
                && call_data["call_hash"] == t.content_hash
        });
        let mut e = evidence_base();
        e.id = format!("tool:{id}:{}", event.sequence);
        e.session_id = event.session_id.clone();
        e.turn_id = event.turn_id;
        e.sequence = event.sequence;
        e.call_sequence = call.map(|e| e.sequence);
        e.workspace = event
            .workspace
            .as_deref()
            .map(content_hash)
            .unwrap_or_default();
        e.source_ref = trace
            .map(|t| t.source_id.clone())
            .unwrap_or_else(|| event.event_id.clone());
        e.kind = "tool_result".into();
        e.tool_call_id = Some(id.into());
        e.tool = data["tool"].as_str().map(str::to_owned);
        e.command = invocation["command"].as_str().map(str::to_owned);
        e.workdir = invocation["workdir"].as_str().map(str::to_owned);
        e.target_id = invocation["target_id"].as_str().map(str::to_owned);
        e.target_paths = invocation["target_paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        e.authoritative = invocation["complete"] == true
            && calls.len() == 1
            && event.workspace.as_deref() == Some(&workspace)
            && call.is_some_and(|c| c.workspace == event.workspace)
            && call_trace.is_some_and(|t| t.workspace == event.workspace)
            && trace.is_some_and(|t| t.workspace == event.workspace)
            && traces
                .iter()
                .filter(|t| {
                    t.kind == SourceKind::ToolCall
                        && t.turn_id == event.turn_id
                        && t.tool_call_id.as_deref() == Some(id)
                })
                .count()
                == 1;
        if let Some(t) = trace {
            e.status = if e.authoritative {
                t.status.clone().unwrap_or_else(|| "unknown".into())
            } else {
                "unknown".into()
            };
            e.content_hash = t.content_hash.clone();
            e.excerpt = redact(&t.text);
            e.truncated = content_hash(&t.text) != t.content_hash || e.excerpt != t.text;
            // The original invocation is not the actual substitute. Legacy
            // recovery records lack call/workspace identity, so do not certify
            // its scope as if the original tool completed successfully.
            if t.text.starts_with("[recovery:") {
                e.authoritative = false;
                e.status = "unknown".into();
            }
        }
        e.exit_code = data["exit_code"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok());
        e.timestamp = event.timestamp;
        if e.tool.as_deref() == Some("shell") {
            e.scope = e.command.as_deref().map(test_scope).unwrap_or_default();
            if !e.scope.is_empty() {
                let low = e.command.as_ref().unwrap().to_lowercase();
                if low.contains("--lib") || low.contains("tests/unit") {
                    e.scope.push("unit".into());
                }
                if low.contains("integration") {
                    e.scope.push("integration".into());
                }
            }
        }
        let durable = events
            .iter()
            .rev()
            .find(|s| {
                s.event_type == EventType::VerificationSnapshot
                    && s.turn_id == event.turn_id
                    && s.tool_call_id == event.tool_call_id
                    && s.workspace == event.workspace
                    && s.sequence > event.sequence
            })
            .map(payload);
        if let Some(s) = durable {
            e.before = serde_json::from_value(s["before"].clone()).ok();
            e.after = serde_json::from_value(s["after"].clone()).ok();
        }
        if e.before.is_none() {
            if let Some((b, a)) = checks.get(id) {
                e.before = Some(b.clone());
                e.after = Some(a.clone());
            }
        }
        evidence.push(e);
    }
    for trace in traces
        .iter()
        .filter(|t| t.kind == SourceKind::RuntimeVerification && t.turn_id == session.turn_id())
    {
        let event = events.iter().find(|e| {
            e.event_type == EventType::RuntimeVerification && e.event_id == trace.source_id
        });
        let mut e = evidence_base();
        e.id = format!("runtime:{}", trace.source_id);
        e.session_id = trace.session_id.clone();
        e.turn_id = trace.turn_id;
        e.workspace = trace
            .workspace
            .as_deref()
            .map(content_hash)
            .unwrap_or_default();
        e.kind = "runtime_verification".into();
        e.source_ref = trace.source_id.clone();
        e.timestamp = trace.timestamp;
        e.content_hash = trace.content_hash.clone();
        e.excerpt = redact(&trace.text);
        e.truncated = content_hash(&trace.text) != trace.content_hash;
        if let Some(r) = &trace.runtime {
            e.checker = Some(r.checker.clone());
            e.target_paths = r.target_paths.clone();
            e.checked_hashes = r.content_hashes.clone();
            e.scope = vec![r.scope.clone()];
            e.target_id = Some(content_hash(
                &serde_json::to_string(&(r.checker.clone(), r.target_paths.clone())).unwrap(),
            ));
            e.authoritative = event.is_some_and(|ev| {
                ev.workspace == trace.workspace
                    && ev.turn_id == trace.turn_id
                    && payload(ev)["record"] == serde_json::to_value(trace).unwrap_or(Value::Null)
            }) && trace.workspace.as_deref() == Some(&workspace)
                && r.workspace == workspace
                && r.checker == "python.compile.-I.-S"
                && r.scope == "python_syntax_only"
                && provenance::checked_files_current(&root, r);
            e.sequence = event.map(|e| e.sequence).unwrap_or_default();
            e.status = if e.authoritative {
                trace.status.clone().unwrap_or_else(|| "unknown".into())
            } else {
                "unknown".into()
            };
        }
        evidence.push(e);
    }
    // A fresh successful re-run supersedes only the same exact invocation target.
    for i in 0..evidence.len() {
        let later = evidence
            .iter()
            .filter(|s| {
                s.sequence > evidence[i].sequence
                    && s.target_id.is_some()
                    && s.target_id == evidence[i].target_id
                    && s.workdir == evidence[i].workdir
                    && s.scope == evidence[i].scope
                    && provenance::success(s)
                    && s.before
                        .as_ref()
                        .zip(s.after.as_ref())
                        .is_some_and(|(b, a)| super::snapshot::fresh(b, a, &current_files))
                    && evidence[i].authoritative
            })
            .max_by_key(|e| e.sequence)
            .map(|e| e.id.clone());
        evidence[i].superseded_by = later;
    }
    evidence.sort_by_key(|e| std::cmp::Reverse(e.sequence));
    if evidence.len() > 32 {
        evidence.sort_by_key(|e| {
            (
                e.superseded_by.is_some() || provenance::success(e),
                std::cmp::Reverse(e.sequence),
            )
        });
        evidence.truncate(32);
        limitations.push("evidence_budget_truncated".into());
    }
    let mut claims = extraction::claims(candidate);
    for claim in &mut claims {
        bind(claim, &evidence);
    }
    let unknown_fact_ids = claims
        .iter()
        .filter(|c| c.kind == ClaimKind::Semantic && c.context == "narrative")
        .map(|c| c.id.clone())
        .collect::<Vec<_>>();
    let coverage_detail = Coverage {
        text: "complete".into(),
        facts: if unknown_fact_ids.is_empty() {
            "complete"
        } else {
            "unknown"
        }
        .into(),
        requirement_sources: requirements_coverage.clone(),
        requirements: requirements_coverage,
        unknown_fact_ids,
        reasons: limitations.clone(),
    };
    VerificationInput {
        schema_version: "buglecat.verification.input.v2".into(),
        session_id: session.session_id.clone(),
        turn_id: session.turn_id(),
        workspace: workspace_id,
        requirements,
        candidate: candidate.into(),
        candidate_hash: content_hash(candidate),
        claims,
        evidence,
        current_files,
        unresolved_call_ids: session.unresolved_tool_calls(),
        coverage: coverage_detail.facts.clone(),
        limitations,
        coverage_detail,
        requirement_sources,
    }
}

fn bind(claim: &mut Claim, evidence: &[Evidence]) {
    if claim.context != "narrative" || claim.kind == ClaimKind::Limitation {
        return;
    }
    let refs: Vec<_> = regex::Regex::new(r"\[(?:call|evidence):([^\]]+)\]")
        .unwrap()
        .captures_iter(&claim.text)
        .map(|c| (c[0].starts_with("[call:"), c[1].to_owned()))
        .collect();
    if !refs.is_empty() {
        claim.binding_status = "explicit".into();
        claim.evidence_ids = refs
            .iter()
            .flat_map(|(call, id)| {
                let found: Vec<_> = evidence
                    .iter()
                    .filter(|e| {
                        if *call {
                            e.tool_call_id.as_ref() == Some(id)
                        } else {
                            &e.id == id
                        }
                    })
                    .map(|e| e.id.clone())
                    .collect();
                if found.is_empty() {
                    vec![format!("invalid:{id}")]
                } else {
                    found
                }
            })
            .collect();
        return;
    }
    let mut matching: Vec<_> = evidence
        .iter()
        .filter(|e| {
            e.tool_call_id.as_ref().is_some_and(|id| {
                claim.text.contains(&format!("[{id}]"))
                    || claim.text.contains(&format!("[call:{id}]"))
            }) || claim.text.contains(&format!("[evidence:{}]", e.id))
                || e.command
                    .as_ref()
                    .is_some_and(|cmd| claim.text.contains(cmd))
        })
        .collect();
    if !matching.is_empty() {
        claim.binding_status = "explicit".into();
    } else {
        matching = evidence
            .iter()
            .filter(|e| {
                (match claim.kind {
                    ClaimKind::TestsPassed => {
                        !e.scope.is_empty() && e.tool.as_deref() == Some("shell")
                    }
                    ClaimKind::ExecutionSuccess => match claim.tool_hint.as_deref() {
                        Some("read_file") => e.tool.as_deref() == Some("read_file"),
                        Some("file_write") => matches!(
                            e.tool.as_deref(),
                            Some("apply_patch" | "write_file" | "edit_file")
                        ),
                        _ => e.tool.as_deref() == Some("shell"),
                    },
                    ClaimKind::Semantic => true,
                    _ => false,
                }) && claim
                    .requested_scope
                    .as_ref()
                    .is_none_or(|s| s == "all" || e.scope.contains(s))
                    && claim.target.as_ref().is_none_or(|p| {
                        e.target_paths
                            .iter()
                            .any(|t| t.replace('\\', "/").ends_with(p))
                            || e.command.as_ref().is_some_and(|cmd| cmd.contains(p))
                    })
            })
            .collect();
        let groups: BTreeSet<_> = matching.iter().map(|e| e.target_id.clone()).collect();
        if groups.len() > 1 || groups.contains(&None) {
            claim.binding_status = "ambiguous_target".into();
            return;
        }
        claim.binding_status = "host_target".into();
    }
    claim.evidence_ids = matching.iter().map(|e| e.id.clone()).collect();
    if claim.evidence_ids.is_empty() {
        claim.binding_status = "missing_evidence".into();
    }
}
