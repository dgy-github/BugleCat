import pytest

from evals.verification.artifact_gate import calibrate, score
from evals.verification.calibration import load


def protocol():
    return load("evals/verification/reports/artifact_protocol_20261011_v1.json")


def data():
    cases = [{"id": str(i), "group_id": str(i), "label": "supported" if i < 150 else "unsupported"}
             for i in range(200)]
    rows = [{"id": str(i), "call_status": "ok", "model": "jev-1.13.0",
             "transport_binding_status": "original_transport_bytes_verified", "request_hash": "a" * 64,
             "actual_request_hash": "a" * 64, "response_sha256": "b" * 64,
             "acceptance_violation": .1 if i < 150 else .99} for i in range(200)]
    return cases, rows


def test_fixed_tie_break_qualifies_only_for_holdout_not_certification():
    cases, rows = data()
    result = calibrate(cases, rows, protocol())
    assert result["qualified_for_holdout_threshold"] == .99
    assert result["certified_threshold"] is None
    assert result["production_authorized"] is False
    assert all(trial["verified_pass_signals"] == 0 for trial in result["grid_results"])


def test_single_false_block_breaks_predeclared_confidence_gate():
    cases, rows = data()
    rows[0]["acceptance_violation"] = 1
    result = calibrate(cases, rows, protocol())
    assert not result["calibration_numeric_gate_met"]
    assert result["qualified_for_holdout_threshold"] is None


def test_failed_error_responses_stay_in_detection_denominator():
    cases, rows = data()
    for row in rows[170:]:
        row.update(call_status="timeout", acceptance_violation=None)
    result = score(cases, rows, .9, protocol())
    assert result["unsupported_groups"] == 50
    assert result["detected_unsupported"] == 20
    assert not result["gate_met"]
    assert len(result["failed_attempts"]) == 30


def test_missing_attempt_or_unknown_label_cannot_improve_denominator():
    cases, rows = data()
    with pytest.raises(ValueError, match="missing"):
        calibrate(cases, rows[:-1], protocol())
    cases[-1]["label"] = "unknown"
    with pytest.raises(ValueError, match="unknown"):
        calibrate(cases, rows, protocol())
