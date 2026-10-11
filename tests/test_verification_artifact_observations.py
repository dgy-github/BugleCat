"""Original receipt verification must reject metadata and failed-byte tampering."""
import copy
import json

import pytest

from evals.verification import artifact_observations as module
from evals.verification.artifact_acceptance import POLICY, TEMPLATE


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


@pytest.fixture
def attempt(tmp_path, monkeypatch):
    # Source intake has separate real-source tests; this fixture isolates the
    # original marker/event/receipt/response join, not prediction generation.
    monkeypatch.setattr(module, "frozen_row", lambda row: None)
    ledger = tmp_path / "ledger"
    directory = ledger / "attempt"
    directory.mkdir(parents=True)
    runner = tmp_path / "crates/ncx-core/examples/verification_artifact.rs"
    executable = tmp_path / "runner.exe"
    paths = {
        "runner_source_sha256": runner,
        "runner_exe_sha256": executable,
        "runner_support_source_sha256": runner.parent / "verification_artifact/support.rs",
        "provider_transport_source_sha256": tmp_path / "crates/ncx-provider/src/jev.rs",
    }
    for path in paths.values():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"frozen compilation")
    auth = tmp_path / "authorization.json"
    write(auth, {"jev": {"ledger_dir": str(ledger)}})
    row = {"id": "case", "group_id": "group", "policy_version": POLICY, "template_version": TEMPLATE,
           "authorization_file": str(auth), "expected_actual_model": "jev-1.13.0", "request": {"state": "fixture"}}
    for field in ("request_hash", "candidate_sha256", "code_sha256", "artifact_sha256", "source_sha256",
                  "protocol_sha256", "oracle_sha256", "main_receipt_sha256", "authorization_sha256"):
        row[field] = field
    row["request_hash"] = module.digest(row["request"])
    reservation = ledger / "reservations/group.json"
    reserved = {"kind": "jev", "group_id": "group", "request_hash": row["request_hash"],
                "authorization_sha256": row["authorization_sha256"]}
    write(reservation, reserved)
    start = {"schema": "buglecat.verification.artifact-attempt.v1", "status": "attempt_started", "offline": False,
             "policy_version": POLICY, "template_version": TEMPLATE, "request": row["request"],
             "reservation": reserved, "reservation_file_sha256": module.raw_hash(reservation),
             "build_info": {key: module.raw_hash(path) for key, path in paths.items()}}
    for target, source in (("id", "id"), ("group_id", "group_id"), ("request_hash", "request_hash"),
                           ("candidate_hash", "candidate_sha256"), ("code_sha256", "code_sha256"),
                           ("prepared_artifact_sha256", "artifact_sha256"), ("source_sha256", "source_sha256"),
                           ("protocol_sha256", "protocol_sha256"), ("oracle_sha256", "oracle_sha256"),
                           ("main_receipt_sha256", "main_receipt_sha256"), ("authorization_sha256", "authorization_sha256")):
        start[target] = row[source]
    start.update({key: start["build_info"][key] for key in ("runner_source_sha256", "runner_exe_sha256")})
    write(reservation.with_suffix(".started"), start)
    response = {"model": "jev-1.13.0", "usage": {"input_tokens": 1, "output_tokens": 2},
                "answers": {"acceptance_violation": {"type": "noul", "noul": .2}}}
    raw = directory / "raw-response.json"
    write(raw, response)
    submit = {**start, "status": "request_submitted", "actual_request_hash": row["request_hash"],
              "planned_raw_response_file": str(raw)}
    finish = {**submit, "status": "attempt_finished", "call_status": "ok", "elapsed_ms": 1,
              "response": response, "model": response["model"], "usage": response["usage"],
              "raw_response_file": str(raw), "response_sha256": module.raw_hash(raw)}
    events_file = directory / "events.jsonl"

    def run(change=None, cancel=False):
        current = copy.deepcopy(finish)
        if cancel:
            current.update(call_status="cancelled", response=None, model=None, usage=None,
                           raw_response_file=None, response_sha256=None, submitted=False)
        if change:
            change(current)
        receipt = directory / "receipt.json"
        write(receipt, current)
        current["receipt_file_sha256"] = module.raw_hash(receipt)
        events = [start, current] if cancel else [start, submit, current]
        events_file.write_text("".join(json.dumps(event) + "\n" for event in events), encoding="utf-8")
        return module.observe(row, events, ledger, executable, runner, reservation, events_file)

    return run, raw


def test_original_success_and_cancel_before_submit(attempt):
    run, _ = attempt
    assert run()["acceptance_violation"] == .2
    cancelled = run(cancel=True)
    assert cancelled["acceptance_violation"] is None
    assert cancelled["transport_binding_status"] == "failed_original_attempt_verified"


def test_cannot_change_usage_even_with_resealed_final_receipt(attempt):
    run, _ = attempt
    with pytest.raises(ValueError, match="model/usage mismatch"):
        run(lambda event: event.update(usage={"input_tokens": 999, "output_tokens": 2}))


def test_failed_response_bytes_are_still_verified(attempt):
    run, raw = attempt
    raw.write_text("tampered", encoding="utf-8")
    with pytest.raises(ValueError, match="response bytes mismatch"):
        run(lambda event: event.update(call_status="invalid_json"))
