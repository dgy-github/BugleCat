"""Offline review of historical APPS Jev scores; never sends paid requests.

The previous HTTP runner bypassed reservations and retried failures. Paid work
must use the existing authorized collectors. Historical files cannot establish
actual attempt counts; this tool never rewrites their records or budget ledger.
"""
import argparse
import json
import math
from pathlib import Path

from scipy.stats import beta

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "reports/.nanocodex/reuse-strong-oracle-20261013"
GRID = [round(0.10 + i * 0.05, 2) for i in range(17)]


def valid_score(value):
    return type(value) in (int, float) and math.isfinite(value) and 0 <= value <= 1


def review(labels_path, calls_dir):
    """Keep failed/missing outcomes in cohort counts and refuse qualification."""
    labels, seen = [], set()
    for line in Path(labels_path).read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        cid = row["id"]
        if not isinstance(cid, str) or not cid or Path(cid).name != cid or "/" in cid or "\\" in cid:
            raise ValueError("invalid candidate id")
        if cid in seen:
            raise ValueError(f"duplicate candidate: {cid}")
        seen.add(cid)
        if row["new_label"] in ("supported", "unsupported"):
            labels.append(row)
    scores = {"supported": [], "unsupported": []}
    unresolved = []
    for row in labels:
        path = Path(calls_dir) / f"{row['id']}.json"
        try:
            saved = json.loads(path.read_text(encoding="utf-8"))
            score = saved.get("noul")
            if saved.get("ok") is not True or not valid_score(score):
                raise ValueError("failed_or_invalid_score")
            raw_score = saved["raw"]["answers"]["acceptance_violation"]["noul"]
            if not valid_score(raw_score) or raw_score != score:
                raise ValueError("raw_score_mismatch")
        except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
            unresolved.append({"id": row["id"], "label": row["new_label"],
                               "reason": type(error).__name__})
            continue
        scores[row["new_label"]].append(score)
    counts = {label: sum(r["new_label"] == label for r in labels) for label in scores}
    complete = bool(labels) and not unresolved
    quotas_met = counts["supported"] >= 150 and counts["unsupported"] >= 40
    grid = []
    # Unresolved responses never silently shrink denominators.
    if complete and quotas_met:
        for threshold in GRID:
            positive, negative = scores["supported"], scores["unsupported"]
            fp = sum(p >= threshold for p in positive)
            tp = sum(p >= threshold for p in negative)
            upper = 1.0 if fp == len(positive) else float(beta.ppf(.95, fp + 1, len(positive) - fp))
            lower = 0.0 if tp == 0 else float(beta.ppf(.05, tp, len(negative) - tp + 1))
            grid.append({"threshold": threshold, "false_blocks": fp, "detections": tp,
                         "n_supported": len(positive), "n_unsupported": len(negative),
                         "false_block_95_upper_bound": upper, "detection_95_lower_bound": lower,
                         "exploratory_bounds_met": upper <= .02 and lower >= .50})
    return {
        "schema": "buglecat.verification.pure-apps-offline-review.v2",
        "evaluation_scope": "historical_scores_exploratory_only",
        "status": "complete_exploratory" if complete and quotas_met else "incomplete",
        "cohort_counts": counts, "scored_counts": {k: len(v) for k, v in scores.items()},
        "unresolved": unresolved, "quotas_met": quotas_met,
        "new_network_attempts": 0, "automatic_retries": 0,
        "historical_network_attempts": None, "historical_accounting_reconciled": False,
        "request_binding_verified": False, "independent_holdout_verified": False,
        "certified_qualified_thresholds": [], "recommended_threshold": None,
        "grid_evaluation": grid,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--labels", type=Path, default=OUT_DIR / "results.jsonl")
    parser.add_argument("--calls-dir", type=Path, default=OUT_DIR / "jev_calls")
    parser.add_argument("--output", type=Path, help="New report path; existing files are never overwritten")
    parser.add_argument("--dry-run", action="store_true", help="Read-only review (also the default)")
    args = parser.parse_args()
    report = review(args.labels, args.calls_dir)
    encoded = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    if args.output and not args.dry_run:
        with args.output.open("x", encoding="utf-8") as stream:
            stream.write(encoded)
    print(encoded)
    return 0 if report["status"] == "complete_exploratory" else 2


if __name__ == "__main__":
    raise SystemExit(main())
