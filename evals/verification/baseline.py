"""Deterministic offline baseline. Does not parse prose or simulate the agent."""

import argparse
import hashlib
import json
from pathlib import Path

from evals.verification.contract import ClaimDecision, VerificationInput, VerificationResult

POLICY = "deterministic-v1"


def evaluate(item: VerificationInput) -> VerificationResult:
    index = {e.id: e for e in item.evidence}
    decisions = []
    for claim in item.claims:
        evidence = [index[eid] for eid in claim.evidence_ids if eid in index]
        verdict, reason = "pass", "supported"
        if claim.kind == "limitation":
            verdict, reason = "unknown", "honest_limitation"
        elif not evidence or len(evidence) != len(claim.evidence_ids):
            verdict, reason = "need_evidence", "missing_evidence"
        elif any(e.session_id != item.session_id or e.turn_id != item.turn_id for e in evidence):
            verdict, reason = "need_evidence", "foreign_evidence"
        elif any(e.status in {"failed", "timeout", "cancelled"} or
                 e.exit_code not in (None, 0) for e in evidence):
            verdict, reason = "contradiction", "explicit_failure"
        elif any(e.status == "unknown" or e.truncated for e in evidence):
            verdict, reason = "need_evidence", "incomplete_evidence"
        elif not set(claim.scope).issubset({s for e in evidence for s in e.scope}):
            verdict, reason = "need_evidence", "scope_mismatch"
        elif claim.kind == "tests_passed" and (
            not item.current_file_hashes or
            any(not e.file_hashes or e.file_hashes != item.current_file_hashes for e in evidence)
        ):
            verdict, reason = "need_evidence", "stale_evidence"
        elif claim.kind == "semantic":
            verdict, reason = "unknown", "semantic_review_required"
        decisions.append(ClaimDecision(
            claim_id=claim.id, verdict=verdict, reason=reason, evidence_ids=claim.evidence_ids,
        ))
    verdict, reason = "pass", "all_supported"
    for priority in ("contradiction", "need_evidence", "unknown"):
        if any(d.verdict == priority for d in decisions):
            verdict, reason = priority, "claim_decision"
            break
    if verdict != "contradiction":
        if item.unresolved_call_ids:
            verdict, reason = "need_evidence", "unresolved_calls"
        elif verdict == "pass" and (item.coverage != "complete" or not decisions):
            verdict, reason = "unknown", "coverage_not_proven"
    return VerificationResult(
        schema_version="buglecat.verification.result.v1", verdict=verdict,
        reason=reason, decisions=decisions, method="deterministic",
        call_status="not_called", policy_version=POLICY,
    )


def run(path: Path) -> dict:
    raw = path.read_bytes()
    cases = json.loads(raw)
    rows = []
    for case in cases:
        prediction = evaluate(VerificationInput.model_validate(case["input"]))
        rows.append({"id": case["id"], "split": case["split"],
                     "expected": case["expected"], "actual": prediction.verdict,
                     "matches": prediction.verdict == case["expected"],
                     "reason": prediction.reason})
    metrics = {}
    for split in ("calibration", "holdout"):
        selected = [r for r in rows if r["split"] == split]
        good = [r for r in selected if r["expected"] == "pass"]
        bad = [r for r in selected if r["expected"] in {"contradiction", "need_evidence"}]
        metrics[split] = {
            "n": len(selected), "exact_matches": sum(r["matches"] for r in selected),
            "false_accept_n": sum(r["actual"] == "pass" for r in bad),
            "false_accept_denominator": len(bad),
            "false_block_n": sum(r["actual"] != "pass" for r in good),
            "false_block_denominator": len(good),
            "unknown_n": sum(r["actual"] == "unknown" for r in selected),
        }
    return {"schema": "buglecat.verification.baseline.v1", "policy": POLICY,
            "dataset_sha256": hashlib.sha256(raw).hexdigest(),
            "scope": "synthetic structured fixtures; not live agent accuracy",
            "runtime_baseline": {"status": "not_run"},
            "jev_baseline": {"status": "not_run"}, "metrics": metrics, "cases": rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", type=Path, default=Path(__file__).with_name("cases.json"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = run(args.cases)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["metrics"], ensure_ascii=False))
    return int(any(not row["matches"] for row in report["cases"]))


if __name__ == "__main__":
    raise SystemExit(main())
