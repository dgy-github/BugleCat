import copy

import pytest

from evals.verification.calibration import digest
from evals.verification.replay_inputs import observe, prepare, raw_hash


def fixture():
    candidate = "The final check passed"
    claim = {"id": "c1", "text": candidate, "start": 0, "end": len(candidate),
             "context": "narrative", "polarity": "positive", "kind": "semantic"}
    packet = {"candidate": candidate, "claims": [claim]}
    state = {"claim": candidate}
    request = {"state": state}
    row = {"id": "a", "group_id": "g", "schema": "buglecat.verification.prepared-replay.v1",
           "status": "prepared", "candidate_mode": "original_transcript", "eligible_semantic": True,
           "source_sha256": "source", "candidate_sha256": raw_hash(candidate.encode()),
           "request_hash": digest(request), "request": request, "state": state,
           "packet": packet, "packet_redacted_sha256": digest(packet), "selected_claim_id": "c1",
           "host_B": {"claim_verdict": "unknown"}, "input_hash": "packet-input",
           "policy_version": "policy", "template_version": "template"}
    row["artifact_sha256"] = digest(row)
    label = {"id": "a", "group_id": "g", "source_sha256": "source", "candidate_sha256": row["candidate_sha256"],
             "review_status": "assertion_reviewed", "label": "supported", "assertion_receipt_sha256": "receipt"}
    selection = {"id": "a", "reviewer_kind": "model", "scope_basis": "independent final check exit", "claim_sha256": raw_hash(candidate.encode()), "assertion_receipt_sha256": "receipt"}
    return row, label, selection


def bound_data():
    row, label, selection = fixture()
    return prepare([row], {"cases": [label]}, {"cases": [selection]})[0]


def test_prepared_input_is_real_bound_development_not_blind():
    data = bound_data()
    assert data["cases"][0]["author_exposed"] is True
    assert data["cases"][0]["human_reviewed"] is False
    assert data["cases"][0]["split"] == "development"


@pytest.mark.parametrize("mutation", ["seal", "review", "scope", "receipt", "decisive"])
def test_no_unbound_label_or_probability_promotion(mutation):
    row, label, selection = fixture()
    if mutation == "seal":
        row["state"]["claim"] = "other"
    elif mutation == "review":
        label["source_sha256"] = "other"
    elif mutation == "scope":
        selection["claim_sha256"] = "other"
    elif mutation == "receipt":
        selection["assertion_receipt_sha256"] = "other"
    else:
        row["host_B"]["claim_verdict"] = "pass"
        row["artifact_sha256"] = digest({k: v for k, v in row.items() if k != "artifact_sha256"})
    with pytest.raises(ValueError):
        prepare([row], {"cases": [label]}, {"cases": [selection]})


def test_same_original_task_cannot_spend_two_claims():
    row, label, selection = fixture()
    second = copy.deepcopy(row)
    second["id"] = "b"
    second["artifact_sha256"] = digest({k: v for k, v in second.items() if k != "artifact_sha256"})
    with pytest.raises(ValueError, match="one claim per original group"):
        prepare([row, second], {"cases": [label, {**label, "id": "b"}]},
                {"cases": [selection, {**selection, "id": "b"}]})


def events():
    data = bound_data()
    base = {**data["cases"][0], "status": "attempt_started"}
    response = {"model": "jev-version", "answers": {"contradiction": {"noul": .1}}, "usage": {"input_tokens": 3}}
    return data, [base, {**base, "status": "request_submitted", "actual_request_hash": base["request_hash"]},
                  {**base, "status": "attempt_finished", "call_status": "ok", "response": response,
                   "response_sha256": "raw-response", "model": response["model"], "usage": response["usage"]}]


def test_success_requires_durable_actual_transport_receipt():
    data, rows = events()
    assert observe(data, rows)[0]["questions"][0]["noul"] == .1
    with pytest.raises(ValueError, match="transport receipt"):
        observe(data, [rows[0], rows[2]])
    rows[1]["actual_request_hash"] = "other"
    with pytest.raises(ValueError, match="actual transport"):
        observe(data, rows)


def test_timeout_counts_but_no_probability_is_invented():
    data, rows = events()
    rows[2].update(call_status="timeout", response={}, model=None, usage=None, response_sha256=None)
    result = observe(data, rows)
    assert result[0]["questions"] == [] and result[0]["call_status"] == "timeout"
    with pytest.raises(ValueError, match="unfinished"):
        observe(data, rows[:2])


def test_duplicate_attempt_or_version_change_rejected():
    data, rows = events()
    with pytest.raises(ValueError, match="duplicate attempt"):
        observe(data, [rows[0], rows[0], *rows[1:]])
    rows[2]["policy_version"] = "changed"
    with pytest.raises(ValueError, match="policy_version"):
        observe(data, rows)


def test_unrecorded_hash_requires_explicit_development_exception():
    data, rows = events()
    del rows[1]["actual_request_hash"]
    with pytest.raises(ValueError, match="hash unrecorded"):
        observe(data, rows)
    result = observe(data, rows, allow_unrecorded_actual_hash=True)
    assert result[0]["actual_request_hash"] is None
    assert result[0]["transport_binding_status"] == "actual_transport_hash_unrecorded_development_only"
    data["cases"][0]["split"] = "holdout"
    with pytest.raises(ValueError, match="only allowed in development"):
        observe(data, rows, allow_unrecorded_actual_hash=True)
