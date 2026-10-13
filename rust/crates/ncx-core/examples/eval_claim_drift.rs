//! Evaluator for the Claim-Level Contradiction and Agent Drift Benchmark.
//! Executes cases directly through ncx_core::verification::policy::evaluate.

use ncx_core::verification::{policy, types::*};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

#[derive(Debug, Deserialize)]
struct TestCase {
    id: String,
    group: String,
    language: String,
    drift_type: String,
    expected_verdict: String,
    expected_reason_code: String,
    rationale: String,
    input: VerificationInput,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    id: String,
    group: String,
    language: String,
    drift_type: String,
    expected_verdict: String,
    actual_claim_verdict: String,
    actual_turn_verdict: String,
    expected_reason_code: String,
    actual_reason_code: String,
    verdict_matches: bool,
    reason_matches: bool,
    rationale: String,
}

#[derive(Debug, Serialize)]
struct GroupMetric {
    total: usize,
    verdict_matches: usize,
    reason_matches: usize,
    match_rate: f64,
}

#[derive(Debug, Serialize)]
struct EvaluationSummary {
    schema: String,
    evaluation_scope: String,
    policy_version: String,
    total_cases: usize,
    drift_cases_total: usize,
    drift_detected: usize,
    drift_detection_rate: f64,
    compliant_cases_total: usize,
    compliant_passed: usize,
    false_block_rate: f64,
    exact_verdict_matches: usize,
    exact_verdict_accuracy: f64,
    exact_reason_matches: usize,
    exact_reason_accuracy: f64,
    elapsed_ms: u128,
    groups: BTreeMap<String, GroupMetric>,
    languages: BTreeMap<String, GroupMetric>,
    cases: Vec<CaseResult>,
}

fn verdict_to_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "pass",
        Verdict::NeedEvidence => "need_evidence",
        Verdict::Contradiction => "contradiction",
        Verdict::Unknown => "unknown",
    }
}

fn intervention(verdict: &str) -> bool {
    matches!(verdict, "need_evidence" | "contradiction")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dataset_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("evals/verification/claim_drift_dataset_v1.json")
    };
    let output_path = if args.len() > 2 {
        Some(PathBuf::from(&args[2]))
    } else {
        None
    };

    let raw = match std::fs::read_to_string(&dataset_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Failed to read dataset from {:?}: {}", dataset_path, e);
            std::process::exit(1);
        }
    };

    let cases: Vec<TestCase> = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse test cases: {}", e);
            std::process::exit(1);
        }
    };

    let started = Instant::now();
    let mut case_results = Vec::new();
    let mut groups: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut languages: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();

    let mut drift_total = 0;
    let mut drift_detected = 0;
    let mut compliant_total = 0;
    let mut compliant_blocked = 0;
    let mut total_verdict_matches = 0;
    let mut total_reason_matches = 0;

    for case in &cases {
        let result = policy::evaluate(&case.input);
        let actual_claim_verdict = result.claim_verdict.map(verdict_to_str).unwrap_or_else(|| {
            result
                .decisions
                .first()
                .map(|d| verdict_to_str(d.verdict))
                .unwrap_or(verdict_to_str(result.verdict))
        });
        let actual_turn_verdict = verdict_to_str(result.verdict);
        let deciding_decision = result
            .decisions
            .iter()
            .find(|d| Some(d.verdict) == result.claim_verdict)
            .or_else(|| result.decisions.first());
        let actual_reason = deciding_decision
            .map(|d| d.reason_code.as_str())
            .unwrap_or(result.reason_code.as_str());

        let verdict_matches = actual_claim_verdict == case.expected_verdict;
        let reason_matches = actual_reason == case.expected_reason_code;

        if verdict_matches {
            total_verdict_matches += 1;
        }
        if reason_matches {
            total_reason_matches += 1;
        }

        let is_drift = case.drift_type != "none";
        if is_drift {
            drift_total += 1;
            if intervention(actual_claim_verdict) {
                drift_detected += 1;
            }
        } else {
            compliant_total += 1;
            if intervention(actual_claim_verdict) {
                compliant_blocked += 1;
            }
        }

        let grp = groups.entry(case.group.clone()).or_insert((0, 0, 0));
        grp.0 += 1;
        if verdict_matches {
            grp.1 += 1;
        }
        if reason_matches {
            grp.2 += 1;
        }

        let lang = languages.entry(case.language.clone()).or_insert((0, 0, 0));
        lang.0 += 1;
        if verdict_matches {
            lang.1 += 1;
        }
        if reason_matches {
            lang.2 += 1;
        }

        case_results.push(CaseResult {
            id: case.id.clone(),
            group: case.group.clone(),
            language: case.language.clone(),
            drift_type: case.drift_type.clone(),
            expected_verdict: case.expected_verdict.clone(),
            actual_claim_verdict: actual_claim_verdict.into(),
            actual_turn_verdict: actual_turn_verdict.into(),
            expected_reason_code: case.expected_reason_code.clone(),
            actual_reason_code: actual_reason.into(),
            verdict_matches,
            reason_matches,
            rationale: case.rationale.clone(),
        });
    }

    let elapsed_ms = started.elapsed().as_millis();

    let mut group_metrics = BTreeMap::new();
    for (k, v) in groups {
        group_metrics.insert(
            k,
            GroupMetric {
                total: v.0,
                verdict_matches: v.1,
                reason_matches: v.2,
                match_rate: if v.0 > 0 {
                    v.1 as f64 / v.0 as f64
                } else {
                    0.0
                },
            },
        );
    }

    let mut lang_metrics = BTreeMap::new();
    for (k, v) in languages {
        lang_metrics.insert(
            k,
            GroupMetric {
                total: v.0,
                verdict_matches: v.1,
                reason_matches: v.2,
                match_rate: if v.0 > 0 {
                    v.1 as f64 / v.0 as f64
                } else {
                    0.0
                },
            },
        );
    }

    let summary = EvaluationSummary {
        schema: "buglecat.verification.claim-drift-evaluation.v2".into(),
        evaluation_scope:
            "synthetic_policy_only; excludes extraction, assembly and live correction".into(),
        policy_version: POLICY_VERSION.into(),
        total_cases: cases.len(),
        drift_cases_total: drift_total,
        drift_detected,
        drift_detection_rate: if drift_total > 0 {
            drift_detected as f64 / drift_total as f64
        } else {
            0.0
        },
        compliant_cases_total: compliant_total,
        compliant_passed: compliant_total.saturating_sub(compliant_blocked),
        false_block_rate: if compliant_total > 0 {
            compliant_blocked as f64 / compliant_total as f64
        } else {
            0.0
        },
        exact_verdict_matches: total_verdict_matches,
        exact_verdict_accuracy: if !cases.is_empty() {
            total_verdict_matches as f64 / cases.len() as f64
        } else {
            0.0
        },
        exact_reason_matches: total_reason_matches,
        exact_reason_accuracy: if !cases.is_empty() {
            total_reason_matches as f64 / cases.len() as f64
        } else {
            0.0
        },
        elapsed_ms,
        groups: group_metrics,
        languages: lang_metrics,
        cases: case_results,
    };

    let formatted = serde_json::to_string_pretty(&summary).unwrap();
    if let Some(out) = output_path {
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&out, &formatted) {
            eprintln!("Failed to write output to {:?}: {}", out, e);
            std::process::exit(2);
        } else {
            println!("Report written to {:?}", out);
        }
    }

    println!("=== Claim-Level Contradiction & Agent Drift Evaluation ===");
    println!("Policy version:         {}", summary.policy_version);
    println!("Total cases evaluated:  {}", summary.total_cases);
    println!(
        "Exact verdict accuracy: {}/{} ({:.1}%)",
        summary.exact_verdict_matches,
        summary.total_cases,
        summary.exact_verdict_accuracy * 100.0
    );
    println!(
        "Exact reason accuracy:  {}/{} ({:.1}%)",
        summary.exact_reason_matches,
        summary.total_cases,
        summary.exact_reason_accuracy * 100.0
    );
    println!(
        "Drift detection rate:   {}/{} ({:.1}%)",
        summary.drift_detected,
        summary.drift_cases_total,
        summary.drift_detection_rate * 100.0
    );
    println!(
        "False block rate:       {}/{} ({:.1}%)",
        summary
            .compliant_cases_total
            .saturating_sub(summary.compliant_passed),
        summary.compliant_cases_total,
        summary.false_block_rate * 100.0
    );
    println!("Evaluation elapsed:     {}ms", summary.elapsed_ms);
    if cases.is_empty()
        || drift_total == 0
        || compliant_total == 0
        || total_verdict_matches != cases.len()
        || total_reason_matches != cases.len()
    {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod metric_tests {
    #[test]
    fn unknown_is_not_detected_and_need_evidence_is_an_intervention() {
        assert!(!super::intervention("unknown"));
        assert!(!super::intervention("pass"));
        assert!(super::intervention("need_evidence"));
        assert!(super::intervention("contradiction"));
    }
}
