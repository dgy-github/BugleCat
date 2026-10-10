"""Summarize explicit synthetic replay; do not claim live task or blind accuracy."""
import argparse
import hashlib
import json
import math
import time
from pathlib import Path

from .baseline import evaluate
from .contract import VerificationInput


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def summarize(cases_path: Path, observations_path: Path) -> dict:
    raw = cases_path.read_bytes()
    cases = json.loads(raw)
    observations = [json.loads(line) for line in observations_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    by_id = {row["id"]: row for row in observations}
    rows = []
    for case in cases:
        start = time.perf_counter()
        decision = evaluate(VerificationInput.model_validate(case["input"]))
        elapsed = (time.perf_counter() - start) * 1000
        observation = by_id.get(case["id"], {})
        rows.append({"id": case["id"], "split": case["split"], "expected": case["expected"],
                     "rules_verdict": decision.verdict, "rules_plus_jev_verdict": decision.verdict,
                     "rules_elapsed_ms": elapsed, "call_status": observation.get("call_status", "not_run"),
                     "jev_elapsed_ms": observation.get("elapsed_ms"),
                     "probabilities": {name: answer["noul"] for name, answer in observation.get("response", {}).get("answers", {}).items()}})
    good = [r for r in rows if r["expected"] == "pass"]
    bad = [r for r in rows if r["expected"] in {"need_evidence", "contradiction"}]
    metrics = {
        "n": len(rows), "false_accept_n": sum(r["rules_verdict"] == "pass" for r in bad),
        "false_accept_denominator": len(bad), "false_block_n": sum(r["rules_verdict"] != "pass" for r in good),
        "false_block_denominator": len(good), "unknown_n": sum(r["rules_verdict"] == "unknown" for r in rows),
        "judgment_coverage_n": sum(r["rules_verdict"] != "unknown" for r in rows),
        "task_success_rate": None,
    }
    successful = [o for o in observations if o.get("call_status") == "ok"]
    usage = {name: sum(o["response"]["usage"][name] for o in successful)
             for name in ("input_tokens", "output_tokens")}
    return {
        "schema": "buglecat.verification.synthetic-live-report.v2",
        "dataset_sha256": hashlib.sha256(raw).hexdigest(),
        "observations_sha256": hashlib.sha256(observations_path.read_bytes()).hexdigest(),
        "method": "author-labeled synthetic replay; v1 fixture contract; not Rust host extraction or live agent task accuracy",
        "label_review": "author_labeled_unreviewed; previously viewed holdout; not an independent blind test",
        "runtime_contract": "Rust v2 is stricter; this report cannot certify its complete extraction",
        "policy": "v1 deterministic shadow; Jev observations never promote Unknown or override rules",
        "models": sorted({o["response"]["model"] for o in successful}),
        "original_framework": {"status": "not_run", "reason": "no same-model same-budget real task rerun"},
        "deterministic_rules": {"status": "fixture_replay", **metrics,
                                "latency_p50_ms": percentile([r["rules_elapsed_ms"] for r in rows], .5),
                                "latency_p95_ms": percentile([r["rules_elapsed_ms"] for r in rows], .95)},
        "rules_plus_jev": {"status": "fixture_shadow_replay", **metrics,
                           "call_attempts": len(observations), "successful_calls": len(successful),
                           "observation_coverage_n": len(successful), "observation_coverage_denominator": len(cases),
                           "latency_p50_ms": percentile([o["elapsed_ms"] for o in observations], .5),
                           "latency_p95_ms": percentile([o["elapsed_ms"] for o in observations], .95),
                           "incremental_usage": usage, "incremental_cost": None},
        "enablement": {"enforce": False, "reason": "no independent real-task effect or false-block evaluation; retain shadow", "threshold": "not calibrated"},
        "cases": rows,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--observations", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = summarize(args.cases, args.observations)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: report[k] for k in ("models", "rules_plus_jev", "enablement")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
