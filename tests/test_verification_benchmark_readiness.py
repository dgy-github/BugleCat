import pytest

from evals.verification.benchmark_readiness import assess
from evals.verification.calibration import digest


def inputs():
    spec = {"model": "jev-test", "policy_version": "v3.2", "template_version": "v2", "categories": ["semantic"],
            "threshold_grid": [.95], "confidence": .95, "max_false_block_rate": .02,
            "min_supported": 150, "min_unsupported": 50, "min_paired_improvements": 5}
    budget = {"schema": "buglecat.verification.collection-budget-status.v1", "global_main_attempts": 3,
              "remaining_main_attempts": 397, "global_main_finished": 2,
              "receipt_statuses": [{"id": "a", "status": "ok", "attempt_ordinal": 1, "receipt_sha256": "sha-a"},
                                   {"id": "b", "status": "response_content_invalid", "attempt_ordinal": 2, "receipt_sha256": "sha-b"}]}
    queue = {"schema": "buglecat.verification.benchmark-review-queue.v1", "scores_hidden": True,
             "jev_probabilities_read": False, "cases": [
                 {"id": "a", "group_id": "source-a", "receipt_sha256": "sha-a",
                  "status": "original_transport_verified_and_assertions_audited",
                  "acceptance_result": {"status": "passed"}, "eligible_native_claims": [], "label": "unlabeled"},
                 {"id": "b", "status": "response_content_invalid", "label": "unknown"},
                 {"id": "c", "status": "not_finished", "label": "unlabeled"}]}
    return budget, {**queue, "artifact_hash": digest(queue)}, spec


def test_passed_code_and_unresolved_request_do_not_become_claim_labels():
    budget, queue, spec = inputs()
    result = assess(budget, [queue], spec)
    assert result["eligible_native_groups"] == 0
    assert result["acceptance_status_counts"] == {"passed": 1}
    assert result["unresolved_started_attempts"] == 1
    assert result["optimistic_successful_sample_ceiling"] == 399
    assert result["minimum_two_stage_labeled_groups"] == 400
    assert result["start_more_paid_calls"] is False
    assert result["holdout_status"] == "not_run" and result["formal_threshold"] is None


def test_duplicate_queue_cannot_inflate_independent_denominator():
    budget, queue, spec = inputs()
    with pytest.raises(ValueError, match="duplicate candidate"):
        assess(budget, [queue, queue], spec)


def test_changed_review_or_wrong_global_receipt_rejected():
    budget, queue, spec = inputs()
    queue["cases"][0]["receipt_sha256"] = "foreign"
    with pytest.raises(ValueError, match="seal mismatch"):
        assess(budget, [queue], spec)
    queue["artifact_hash"] = digest({k: v for k, v in queue.items() if k != "artifact_hash"})
    with pytest.raises(ValueError, match="review/global receipt mismatch"):
        assess(budget, [queue], spec)


def test_old_preparation_is_not_a_second_attempt_in_later_batch():
    budget, queue, spec = inputs()
    old = {**queue, "cases": [{"id": "a", "status": "not_finished", "label": "unlabeled"}]}
    old["artifact_hash"] = digest({k: v for k, v in old.items() if k != "artifact_hash"})
    assert assess(budget, [old, queue], spec)["verified_candidate_groups"] == 1
    assert assess(budget, [queue, old], spec)["verified_candidate_groups"] == 1
