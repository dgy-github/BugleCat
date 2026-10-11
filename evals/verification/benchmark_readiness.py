"""Account for actual sample eligibility before opening further paid batches.

No labels, claims, model probabilities or successful calls are manufactured.
This report cannot authorize a budget change or open a holdout.
"""

import argparse
import json
from collections import Counter
from pathlib import Path

from .calibration import create, digest, load, protocol, require


def assess(budget, queues, spec):
    spec = protocol(spec)
    require(budget.get("schema") == "buglecat.verification.collection-budget-status.v1", "budget schema")
    attempted, remaining = budget["global_main_attempts"], budget["remaining_main_attempts"]
    require(type(attempted) is int and type(remaining) is int and attempted >= 0 and remaining >= 0,
            "invalid global attempt counts")
    receipts = budget["receipt_statuses"]
    require(len({r["attempt_ordinal"] for r in receipts}) == len(receipts), "duplicate global receipt ordinal")
    require(all(type(r["attempt_ordinal"]) is int and 1 <= r["attempt_ordinal"] <= attempted for r in receipts),
            "receipt outside global ledger")
    by_id = {r["id"]: r for r in receipts}
    require(len(by_id) == len(receipts), "duplicate global receipt ID")
    rows, groups = {}, set()
    for queue in queues:
        require(queue.get("schema") == "buglecat.verification.benchmark-review-queue.v1", "review queue schema")
        require(queue.get("jev_probabilities_read") is False and queue.get("scores_hidden") is True,
                "labels must precede Jev scores")
        require(queue.get("artifact_hash") == digest({k: v for k, v in queue.items() if k != "artifact_hash"}),
                "review queue seal mismatch")
        for row in queue["cases"]:
            # Preparation can precede a later batch's one actual attempt.
            # The old not_finished row is not a second candidate/denominator.
            previous = rows.get(row["id"])
            if previous:
                if row["status"] == "not_finished":
                    continue
                require(previous["status"] == "not_finished", "duplicate candidate across batches")
            rows[row["id"]] = row
            if "group_id" in row:
                require(row["group_id"] not in groups, "repeated source group")
                groups.add(row["group_id"])
            receipt = by_id.get(row["id"])
            if row["status"] == "original_transport_verified_and_assertions_audited":
                require(receipt is not None and receipt["status"] == "ok"
                        and row["receipt_sha256"] == receipt["receipt_sha256"], "review/global receipt mismatch")
            elif receipt is not None and row["status"] != "not_finished":
                require(row["status"] == receipt["status"], "review status contradicts global receipt")
    verified = [r for r in rows.values() if r["status"] == "original_transport_verified_and_assertions_audited"]
    eligible = [r for r in verified if r.get("eligible_native_claims")]
    failed = sum(r["status"] != "ok" for r in receipts)
    # Most generous ceiling: every unresolved and every future attempt succeeds
    # and supplies a uniquely labelable claim. Even this is not a prediction.
    theoretical_ceiling = attempted + remaining - failed
    required = 2 * (spec["min_supported"] + spec["min_unsupported"])
    reasons = []
    if not eligible:
        reasons.append("no_eligible_native_positive_semantic_claims")
    if theoretical_ceiling < required:
        reasons.append("successful_sample_ceiling_below_two_stage_minimum")
    reasons.append("independent_scoped_labels_not_frozen")
    return {"schema": "buglecat.verification.benchmark-readiness.v1",
            "protocol_sha256": digest(spec), "budget_status_sha256": digest(budget),
            "review_queue_sha256s": [digest(q) for q in queues],
            "global_main_attempts": attempted, "remaining_main_attempts": remaining,
            "completed_responses": budget["global_main_finished"],
            "confirmed_failed_candidates": failed,
            "unresolved_started_attempts": attempted - budget["global_main_finished"],
            "verified_candidate_groups": len(verified), "eligible_native_groups": len(eligible),
            "acceptance_status_counts": dict(Counter(r["acceptance_result"]["status"] for r in verified)),
            "acceptance_facts_are_claim_labels": False,
            "optimistic_successful_sample_ceiling": theoretical_ceiling,
            "minimum_two_stage_labeled_groups": required,
            "current_budget_can_cover_minimum_even_if_all_future_attempts_succeed": theoretical_ceiling >= required,
            "start_more_paid_calls": False, "reasons": reasons,
            "formal_threshold": None, "holdout_status": "not_run", "production_authorized": False,
            "new_paid_calls": 0,
            "scope": "Local source-bound single-response intake readiness; no semantic certification or authorization."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("budget", "protocol", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--review", type=Path, action="append", required=True)
    args = parser.parse_args()
    value = assess(load(args.budget), [load(p) for p in args.review], load(args.protocol))
    create(args.output, value)
    print(json.dumps({k: value[k] for k in ("verified_candidate_groups", "eligible_native_groups",
                                          "optimistic_successful_sample_ceiling", "start_more_paid_calls", "new_paid_calls")}))


if __name__ == "__main__":
    main()
