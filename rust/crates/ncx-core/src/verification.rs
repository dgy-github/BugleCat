//! Opt-in pre-commit host verification. Jev probabilities stay shadow observations.
use crate::session::Session;
use serde_json::{json, Value};
use std::{io::Write, path::Path, time::Instant};

mod assembly;
pub mod evidence;
pub mod extraction;
pub mod policy;
pub mod provenance;
#[cfg(test)]
mod provenance_tests;
pub mod requirements;
pub mod snapshot;
#[cfg(test)]
mod tests;
pub mod types;

pub use types::{Config, Mode, VerificationResult};

pub struct Budget {
    pub calls: usize,
    pub limit: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            calls: 0,
            limit: MAX_CALLS_PER_TURN,
        }
    }
}
pub const MAX_CALLS_PER_TURN: usize = 3;
pub const MAX_CORRECTIONS: usize = 2;

/// Compatibility adapter for callers of the original shadow packet. No v1
/// input is promoted into a v2 verified result.
pub fn evidence_packet(session: &Session, candidate: &str) -> Value {
    serde_json::to_value(evidence::packet(
        session,
        Path::new("."),
        candidate,
        &Default::default(),
    ))
    .unwrap()
}

/// Each request asks about one claim and carries only its explicit sources.
/// Large/sensitive payloads are skipped visibly rather than silently truncated.
fn external_state(input: &types::VerificationInput, claim: &types::Claim) -> Option<Value> {
    let sources: Vec<_> = claim.evidence_ids.iter().filter_map(|id| input.evidence.iter().find(|e| &e.id == id)).map(|e| {
        json!({"source_id":e.id,"status":e.status,"exit_code":e.exit_code,
            "command":e.command,"scope":e.scope,"excerpt":e.excerpt,"truncated":e.truncated,
            "file_freshness":match (&e.before,&e.after) {
                (Some(before),Some(after)) => snapshot::fresh(before,after,&input.current_files), _ => false,
            }})
    }).collect();
    let raw = json!({"schema":"buglecat.jev.claim.v2", "claim":claim.text,
        "candidate":input.candidate,"requirements":input.requirements,"evidence":sources,
        "coverage":input.coverage,"limitations":input.limitations})
    .to_string();
    if evidence::unsafe_to_send(&raw) {
        return None;
    }
    let safe = evidence::redact(&raw);
    serde_json::from_str(&safe).ok()
}

pub async fn verify(
    input: &types::VerificationInput,
    budget: &mut Budget,
    cancel: Option<&dyn Fn() -> bool>,
) -> VerificationResult {
    let key = std::env::var("NCX_JEV_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    verify_with(
        input,
        budget,
        cancel,
        key.as_deref(),
        &ncx_provider::jev::HttpTransport,
    )
    .await
}

pub async fn verify_with(
    input: &types::VerificationInput,
    budget: &mut Budget,
    cancel: Option<&dyn Fn() -> bool>,
    key: Option<&str>,
    transport: &dyn ncx_provider::jev::JevTransport,
) -> VerificationResult {
    let started = Instant::now();
    let mut result = policy::evaluate(input);
    if cancel.is_some_and(|c| c()) {
        result.call_status = "cancelled".into();
    } else if key.is_none() {
        result.call_status = "missing_key".into();
    } else {
        let mut review: Vec<_> = input
            .claims
            .iter()
            .filter(|claim| {
                claim.kind == types::ClaimKind::Semantic
                    && claim.context == "narrative"
                    && claim.polarity == "positive"
            })
            .collect();
        // Whole-task completion claims get the scarce semantic budget first;
        // explicit host sources break ties, then retain narrative order.
        review.sort_by_key(|claim| {
            let low = claim.text.to_lowercase();
            let completion = [
                "completed",
                "finished",
                "resolved",
                "repaired",
                "已完成",
                "已修复",
            ]
            .iter()
            .any(|word| low.contains(word));
            (!completion, claim.evidence_ids.is_empty(), claim.start)
        });
        if review.is_empty() {
            result.call_status = "not_required".into();
        }
        for claim in review {
            if budget.calls >= budget.limit.min(MAX_CALLS_PER_TURN) {
                result.call_status = "call_budget".into();
                break;
            }
            let Some(state) = external_state(input, claim) else {
                result.call_status = "sensitive_input".into();
                break;
            };
            budget.calls += 1;
            result.attempted_calls += 1;
            let response = ncx_provider::jev::check_with(
                transport,
                key.unwrap().trim(),
                state,
                ncx_provider::jev::TIMEOUT,
                cancel,
            )
            .await;
            match response {
                Ok(value) => {
                    result.call_status = "ok".into();
                    result
                        .observations
                        .push(json!({"claim_id":claim.id,"response":value}));
                }
                Err(error) => {
                    result.call_status = error;
                    break;
                }
            }
        }
    }
    // A call failure never manufactures Pass. Deterministic contradictions
    // remain available and cannot be overridden by a model observation.
    result.elapsed_ms = started.elapsed().as_millis();
    result
}

/// Explicit development-only rejection signals. Never infer Pass from scores.
pub fn apply_experimental_policy(
    config: &Config,
    input: &types::VerificationInput,
    result: &mut VerificationResult,
) {
    let Some(policy) = &config.experimental_jev else {
        return;
    };
    if !policy.threshold.is_finite()
        || !(0.5..=1.0).contains(&policy.threshold)
        || policy.version.trim().is_empty()
    {
        return;
    }
    for decision in &mut result.decisions {
        if decision.verdict != types::Verdict::Unknown {
            continue;
        }
        let Some(claim) = input.claims.iter().find(|c| {
            c.id == decision.claim_id
                && policy.categories.contains(&c.kind)
                && c.context == "narrative"
                && c.polarity == "positive"
        }) else {
            continue;
        };
        let Some(observation) = result
            .observations
            .iter()
            .find(|o| o["claim_id"] == claim.id)
        else {
            continue;
        };
        let answers = &observation["response"]["answers"];
        for (question, verdict, reason) in [
            (
                "contradiction",
                types::Verdict::Contradiction,
                "experimental_jev_contradiction",
            ),
            (
                "unsupported_success",
                types::Verdict::NeedEvidence,
                "experimental_jev_unsupported",
            ),
        ] {
            if answers[question]["noul"]
                .as_f64()
                .is_some_and(|p| p >= policy.threshold)
            {
                decision.verdict = verdict;
                decision.reason_code = reason.into();
                break;
            }
        }
    }
    result.claim_verdict = Some(policy::aggregate(&result.decisions));
    if matches!(
        result.claim_verdict,
        Some(types::Verdict::Contradiction | types::Verdict::NeedEvidence)
    ) {
        result.verdict = result.claim_verdict.unwrap();
    }
    result.policy_version = format!("{}+experiment:{}", types::POLICY_VERSION, policy.version);
}

/// Independent log is not a SourceRecord and is never fed into fact retrieval.
/// Return errors to the event sink; no silent success on an unwritable log.
pub fn persist(
    session: &Session,
    mode: Mode,
    result: &VerificationResult,
) -> Result<(), &'static str> {
    let path = session
        .log_path
        .as_ref()
        .ok_or("no_durable_log")?
        .with_extension("verification.jsonl");
    let record = json!({"schema":"buglecat.verification.record.v2", "session_id":session.session_id,
        "turn_id":session.turn_id(), "mode":mode,"result":result});
    let mut line = record.to_string();
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "persistence_failed")?;
    file.write_all(line.as_bytes())
        .map_err(|_| "persistence_failed")
}

/// Load only this exact candidate in this exact session/turn. Malformed/old
/// schemas are diagnostic history, never a verified label.
pub fn latest(
    session: &Session,
    candidate: &str,
    turn_id: Option<u64>,
) -> Option<VerificationResult> {
    let path = session
        .log_path
        .as_ref()?
        .with_extension("verification.jsonl");
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|value| {
            if value["schema"] != "buglecat.verification.record.v2"
                || value["session_id"] != session.session_id
                || value["turn_id"] != json!(turn_id)
            {
                return None;
            }
            let result: VerificationResult =
                serde_json::from_value(value["result"].clone()).ok()?;
            (result.candidate_hash == crate::context_trace::content_hash(candidate))
                .then_some(result)
        })
}

/// Kept for the explicit live smoke; runtime uses the pre-commit turn gate.
pub async fn shadow(session: &Session, candidate: &str, cancel: Option<&dyn Fn() -> bool>) {
    if Config::from_env().mode != Mode::Shadow {
        return;
    }
    let input = evidence::packet(session, Path::new("."), candidate, &Default::default());
    let result = verify(&input, &mut Budget::default(), cancel).await;
    if let Err(reason) = persist(session, Mode::Shadow, &result) {
        eprintln!("Jev verification: {reason}");
    }
}
