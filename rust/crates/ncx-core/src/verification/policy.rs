use super::{provenance, snapshot::fresh, types::*};

pub fn evaluate(input: &VerificationInput) -> VerificationResult {
    let decisions: Vec<_> = input
        .claims
        .iter()
        .map(|claim| {
            let (verdict, reason) = check(input, claim);
            ClaimDecision {
                claim_id: claim.id.clone(),
                verdict,
                reason_code: reason.into(),
                evidence_ids: claim.evidence_ids.clone(),
            }
        })
        .collect();
    let claim_verdict = aggregate(&decisions);
    let goal_complete = goal_complete(input, &decisions);
    let mut verdict = claim_verdict;
    if verdict == Verdict::Pass && !goal_complete {
        verdict = Verdict::Unknown;
    }
    if verdict != Verdict::Contradiction && !input.unresolved_call_ids.is_empty() {
        verdict = Verdict::NeedEvidence;
    }
    let mut coverage = input.coverage_detail.clone();
    coverage.requirements = if goal_complete {
        "complete"
    } else if coverage.requirements == "conflicted" {
        "conflicted"
    } else {
        "unknown"
    }
    .into();
    VerificationResult {
        schema_version: "buglecat.verification.result.v2".into(),
        verdict,
        reason_code: if !input.unresolved_call_ids.is_empty() {
            "unresolved_calls"
        } else if !goal_complete {
            "requirements_not_certified"
        } else {
            "claim_decisions"
        }
        .into(),
        decisions,
        call_status: "not_called".into(),
        candidate_hash: input.candidate_hash.clone(),
        input_hash: crate::context_trace::content_hash(&serde_json::to_string(input).unwrap()),
        policy_version: POLICY_VERSION.into(),
        template_version: TEMPLATE_VERSION.into(),
        elapsed_ms: 0,
        observations: vec![],
        persistence_status: "not_recorded".into(),
        attempted_calls: 0,
        claim_verdict: Some(claim_verdict),
        coverage,
    }
}

pub fn aggregate(decisions: &[ClaimDecision]) -> Verdict {
    for v in [
        Verdict::Contradiction,
        Verdict::NeedEvidence,
        Verdict::Unknown,
    ] {
        if decisions.iter().any(|d| d.verdict == v) {
            return v;
        }
    }
    if decisions.is_empty() {
        Verdict::Unknown
    } else {
        Verdict::Pass
    }
}

fn goal_complete(input: &VerificationInput, decisions: &[ClaimDecision]) -> bool {
    if input.coverage_detail.requirements != "complete"
        || input.coverage_detail.facts != "complete"
        || !input.limitations.is_empty()
        || input.requirements.is_empty()
    {
        return false;
    }
    input.requirements.iter().all(|r| {
        let text = r.trim().trim_matches('`');
        let command = text
            .strip_prefix("Run ")
            .or_else(|| text.strip_prefix("run "))
            .or_else(|| text.strip_prefix("运行 "))
            .or_else(|| text.strip_prefix("执行 "))
            .map(|s| s.trim().trim_matches('`').trim_end_matches('.'));
        command.is_some_and(|cmd| {
            !super::evidence::test_scope(cmd).is_empty()
                && input.evidence.iter().any(|e| {
                    e.command.as_deref() == Some(cmd)
                        && decisions
                            .iter()
                            .any(|d| d.verdict == Verdict::Pass && d.evidence_ids.contains(&e.id))
                })
        })
    })
}

fn check(input: &VerificationInput, claim: &Claim) -> (Verdict, &'static str) {
    if input.candidate.get(claim.start..claim.end) != Some(&claim.text) {
        return (Verdict::Unknown, "invalid_claim_span");
    }
    if claim.context == "code" || claim.context == "quoted" {
        return (Verdict::Unknown, "non_asserted_example");
    }
    if claim.kind == ClaimKind::Limitation {
        return (Verdict::Unknown, "honest_limitation_unverified");
    }
    if [
        "initially",
        "previously",
        "before the repair",
        "最初",
        "之前",
    ]
    .iter()
    .any(|s| claim.text.to_lowercase().contains(s))
    {
        return (Verdict::Unknown, "temporal_claim_unverified");
    }
    if claim.kind == ClaimKind::Semantic {
        return (Verdict::Unknown, "semantic_review_required");
    }
    if claim.evidence_ids.is_empty() {
        if claim.polarity == "negative"
            && ["not run", "未运行", "未执行"]
                .iter()
                .any(|s| claim.text.to_lowercase().contains(s))
        {
            return (Verdict::Unknown, "honest_unexecuted_limitation");
        }
        return (
            Verdict::NeedEvidence,
            if claim.binding_status == "ambiguous_target" {
                "ambiguous_evidence_target"
            } else {
                "missing_evidence"
            },
        );
    }
    let all: Vec<_> = claim
        .evidence_ids
        .iter()
        .filter_map(|id| input.evidence.iter().find(|e| &e.id == id))
        .collect();
    if all.len() != claim.evidence_ids.len() {
        return (Verdict::NeedEvidence, "invalid_reference");
    }
    if all.iter().any(|e| {
        e.session_id != input.session_id
            || e.turn_id != input.turn_id
            || e.workspace != input.workspace
    }) {
        return (Verdict::NeedEvidence, "foreign_evidence");
    }
    let evidence: Vec<_> = all
        .iter()
        .copied()
        .filter(|e| {
            e.superseded_by
                .as_ref()
                .is_none_or(|id| !claim.evidence_ids.contains(id))
        })
        .collect();
    let failed = |e: &&Evidence| {
        e.authoritative
            && (e.exit_code.is_some_and(|n| n != 0)
                || [
                    "failed",
                    "timeout",
                    "cancelled",
                    "command_error",
                    "permission_denied",
                ]
                .contains(&e.status.as_str()))
    };
    if claim.polarity == "negative" {
        if ["not run", "did not run", "未运行", "未执行"]
            .iter()
            .any(|s| claim.text.to_lowercase().contains(s))
        {
            return if evidence
                .iter()
                .any(|e| e.authoritative && (e.exit_code.is_some() || provenance::success(e)))
            {
                (Verdict::Contradiction, "denied_observed_execution")
            } else {
                (Verdict::Unknown, "honest_unexecuted_limitation")
            };
        }
        if evidence.iter().any(|e| provenance::success(e)) {
            return (Verdict::Contradiction, "denied_observed_execution");
        }
        if evidence.iter().any(failed) {
            return (Verdict::Pass, "supported_failure_report");
        }
        return (Verdict::Unknown, "negative_statement_unverified");
    }
    if evidence.iter().any(failed) {
        return (Verdict::Contradiction, "explicit_failure");
    }
    if evidence.iter().any(|e| !provenance::success(e)) {
        return (Verdict::NeedEvidence, "incomplete_evidence");
    }
    if claim.kind == ClaimKind::TestsPassed {
        if claim.requested_scope.as_deref() == Some("all")
            || evidence
                .iter()
                .any(|e| e.scope.is_empty() || e.tool.as_deref() != Some("shell"))
        {
            return (Verdict::NeedEvidence, "scope_mismatch");
        }
        if evidence.iter().any(|e| match (&e.before, &e.after) {
            (Some(b), Some(a)) => !fresh(b, a, &input.current_files),
            _ => true,
        }) {
            return (Verdict::NeedEvidence, "stale_evidence");
        }
    }
    (Verdict::Pass, "supported_host_claim")
}
