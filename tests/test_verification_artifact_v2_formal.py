"""Fixed statistical gate fixtures, not actual model performance evidence."""
import copy

import pytest

from evals.verification import artifact_v2_formal as owner


@pytest.fixture
def spec():
    return {"schema": owner.SCHEMA, "policy_version": owner.POLICY, "template_version": owner.TEMPLATE,
            "model": "jev-1.13.0", "threshold_grid": owner.GRID, **owner.GATES,
            "categories": ["artifact_acceptance"], "score": "maximum_exact_question_noul",
            "source_kinds": ["mbpp", "humaneval"], "candidate_oracle_timeout_s": 5,
            "reference_admission_timeout_s": 10}


@pytest.fixture
def cases():
    return [{"id": str(i), "group_id": "group" + str(i),
             "label": "supported" if i < 150 else "unsupported"} for i in range(200)]


def observations(cases, negative=.92):
    return [{"id": case["id"], "call_status": "ok", "model": "jev-1.13.0",
             "transport_binding_status": "original_transport_bytes_verified",
             "request_hash": "a"*64, "actual_request_hash": "a"*64, "response_sha256": "b"*64,
             "acceptance_violation": .1 if case["label"] == "supported" else negative} for case in cases]


@pytest.mark.parametrize("field,value", [
    ("schema", "buglecat.verification.artifact-protocol.v1"), ("template_version", "jev-artifact-acceptance-v1"),
    ("min_supported", 149), ("min_unsupported", 49), ("confidence", .9),
    ("max_false_block_rate", .03), ("min_error_rejection_lower_bound", .49),
    ("threshold_grid", [.5]), ("candidate_oracle_timeout_s", 10), ("model", "jev-latest")])
def test_original_gates_cannot_be_lowered_or_old_protocol_reused(spec, field, value):
    spec[field] = value
    with pytest.raises(ValueError):
        owner.protocol(spec)


def test_new_grid_selection_does_not_certify_or_enable_production(spec, cases):
    result = owner.calibrate_numbers(cases, observations(cases), spec)
    assert result["qualified_for_holdout_threshold"] == .9
    assert result["certified_threshold"] is None
    assert result["production_authorized"] is result["formal_certification"] is False
    assert result["holdout_status"] == "not_run"


def test_failed_attempt_kept_in_negative_denominator(spec, cases):
    rows = observations(cases)
    rows[-1].update(call_status="timeout", acceptance_violation=None)
    result = owner.numeric(cases, rows, .5, spec)
    assert result["unsupported_groups"] == 50 and result["detected_unsupported"] == 49
    assert result["failed_attempts"] == {"199": "timeout"}
    assert result["verified_pass_signals"] == 0


@pytest.mark.parametrize("mutation", ["unknown", "duplicate_group", "missing", "duplicate", "foreign", "invalid_probability", "fake_transport"])
def test_incomplete_or_unverified_binary_data_rejected(spec, cases, mutation):
    rows = observations(cases)
    if mutation == "unknown":
        cases[0]["label"] = "unknown"
    elif mutation == "duplicate_group":
        cases[0]["group_id"] = cases[1]["group_id"]
    elif mutation == "missing":
        rows.pop()
    elif mutation == "duplicate":
        rows.append(copy.deepcopy(rows[0]))
    elif mutation == "foreign":
        rows[0]["id"] = "foreign"
    elif mutation == "invalid_probability":
        rows[0]["acceptance_violation"] = True
    else:
        rows[0]["transport_binding_status"] = "fixture"
    with pytest.raises(ValueError):
        owner.numeric(cases, rows, .5, spec)


def test_all_thresholds_fail_without_positive_detection(spec, cases):
    result = owner.calibrate_numbers(cases, observations(cases, negative=.1), spec)
    assert result["qualified_for_holdout_threshold"] is None
    assert len(result["grid_results"]) == 9


def test_insufficient_negative_quota_never_qualifies(spec, cases):
    cases.pop()
    assert owner.calibrate_numbers(cases, observations(cases), spec)["qualified_for_holdout_threshold"] is None


def test_registration_after_any_allocated_group_jev_reservation_is_forbidden(tmp_path, monkeypatch):
    from evals.verification import artifact_cohort, budget_authorization_v2
    dataset = tmp_path / "dataset.json"
    dataset.write_text('{"authorization_file":"fixture-auth"}')
    monkeypatch.setattr(owner, "registration_value", lambda *args: {"all_allocated_groups": ["allocated-unknown"]})
    monkeypatch.setattr(budget_authorization_v2, "resolve", lambda *args: {
        "authorization": {"jev": {"ledger_dir": "fixture-ledger"}}})
    monkeypatch.setattr(artifact_cohort, "reservations", lambda *args: ([{"group_id": "allocated-unknown"}], None))
    output = tmp_path / "not-created.json"
    with pytest.raises(ValueError, match="registration must precede"):
        owner.register(dataset, output)
    assert not output.exists()


def test_old_runtime_missing_is_rejected_before_packet_construction():
    with pytest.raises(ValueError, match="actual independently verified Oracle runtime"):
        owner.request_packets("fixture", {"split": "calibration"}, [{"call_eligible": True, "label": "supported"}])
