import copy
from pathlib import Path

import pytest

from evals.verification.artifact_acceptance import POLICY, TEMPLATE
from evals.verification.artifact_scoring import register, score
from evals.verification.calibration import digest, load


def seal(value, key="artifact_hash"):
    return {**value, key: digest(value)}


def fixture():
    spec = load(Path(__file__).resolve().parents[1] / "evals/verification/reports/artifact_protocol_20261011_v1.json")
    cases, rows, observations = [], [], []
    for identity, status in (("correct", "passed"), ("wrong", "failed")):
        oracle = seal({"result": {"status": status}})
        row = {"schema": "buglecat.verification.prepared-artifact.v1", "id": identity, "group_id": identity,
               "split": "development", "category": "artifact_acceptance", "policy_version": POLICY,
               "template_version": TEMPLATE, "candidate_sha256": digest(identity), "main_receipt_sha256": digest(identity),
               "oracle_sha256": oracle["artifact_hash"], "request": {"id": identity}, "request_hash": digest({"id": identity})}
        rows.append(seal(row, "artifact_sha256"))
        cases.append({"id": identity, "group_id": identity, "split": "development", "author_exposed": True,
                      "category": "artifact_acceptance", "call_eligible": True,
                      "label": "supported" if status == "passed" else "unsupported", "oracle": oracle,
                      "candidate_sha256": row["candidate_sha256"], "main_receipt_sha256": row["main_receipt_sha256"]})
        observations.append({"id": identity, "request_hash": row["request_hash"], "actual_request_hash": row["request_hash"],
                             "call_status": "ok", "transport_binding_status": "original_transport_bytes_verified",
                             "response_sha256": digest(identity), "model": spec["model"],
                             "acceptance_violation": .1 if status == "passed" else .99})
    data = seal({"schema": "buglecat.verification.artifact-dataset.v1", "cases": cases,
                 "scores_hidden": True, "jev_probabilities_read": False})
    return data, rows, spec, observations


def test_development_perfect_separation_does_not_certify_or_open_holdout():
    data, rows, spec, observations = fixture()
    registration = register(data, rows, spec)
    result = score(data, rows, registration, observations)
    assert result["grid_results"][0]["detected_unsupported"] == 1
    assert result["grid_results"][0]["false_rejected_supported"] == 0
    assert result["certified_threshold"] is None and result["holdout_status"] == "not_run"
    assert result["native_claim_certification"] is False
    assert result["baseline_framework_A"] == result["baseline_rules_B"] == "unavailable"


def test_failure_keeps_negative_denominator_and_missing_is_rejected():
    data, rows, spec, observations = fixture()
    registration = register(data, rows, spec)
    observations[1]["call_status"] = "timeout"
    result = score(data, rows, registration, observations)
    assert result["grid_results"][0]["unsupported_groups"] == 1
    assert result["grid_results"][0]["detected_unsupported"] == 0
    assert result["failed_attempts"] == {"wrong": "timeout"}
    with pytest.raises(ValueError, match="missing attempts"):
        score(data, rows, registration, observations[:1])


def test_label_cannot_be_reversed_even_with_new_dataset_seal():
    data, rows, spec, _ = fixture()
    data = copy.deepcopy(data)
    data["cases"][0]["label"] = "unsupported"
    data = seal({k: v for k, v in data.items() if k != "artifact_hash"})
    with pytest.raises(ValueError, match="label cannot override"):
        register(data, rows, spec)


def test_missing_transport_or_model_mismatch_blocks_successful_vote():
    data, rows, spec, observations = fixture()
    registration = register(data, rows, spec)
    observations[0]["model"] = "other-version"
    with pytest.raises(ValueError, match="model version mismatch"):
        score(data, rows, registration, observations)
    observations[0]["model"] = spec["model"]
    observations[0]["transport_binding_status"] = "just_an_attestation"
    with pytest.raises(ValueError, match="original transport bytes"):
        score(data, rows, registration, observations)


def test_viewed_candidates_cannot_be_rebranded_as_formal_calibration():
    data, rows, spec, _ = fixture()
    rows[0] = seal({**{k: v for k, v in rows[0].items() if k != "artifact_sha256"}, "split": "calibration"}, "artifact_sha256")
    with pytest.raises(ValueError, match="only development"):
        register(data, rows, spec)
