"""Receipt joins fail closed; fake facts here never certify real HTTP."""
import json
from pathlib import Path

import pytest

from evals.verification import artifact_cohort
from evals.verification import artifact_v2_development as owner
from evals.verification.artifact_request_v2 import request
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


def write(path, value, canonical=False):
    path.write_text(json.dumps(value, sort_keys=True, ensure_ascii=False,
                               separators=(",", ":") if canonical else None), encoding="utf-8")


@pytest.fixture
def attempt(tmp_path, monkeypatch):
    root = tmp_path / "ledger"
    directory = root / "attempts" / "one"
    directory.mkdir(parents=True)
    (root / "reservations").mkdir()
    exe = tmp_path / "actual.exe"
    exe.write_bytes(b"fixture")
    prepared = tmp_path / "prepared.json"
    prepared.write_text("{}")
    body = request({"prompt": "return x", "test_list": ["assert f(1) == 1"],
                    "test_imports": []}, ["def f(x):"], "def f(x):\n    return x\n")
    packet = {"id": "case", "group_id": "group", "request_hash": digest(body),
              "request": body, "authorization_file": "auth"}
    monkeypatch.setattr(owner, "validate", lambda path: packet)
    monkeypatch.setattr(owner, "resolve", lambda path: {"authorization": {"jev": {"ledger_dir": str(root)}}})
    monkeypatch.setattr(artifact_cohort, "reservations", lambda *args: ([{
        "group_id": "group", "request_hash": packet["request_hash"]}], None))
    reservation = root / "reservations" / (digest("group") + ".json")
    write(reservation, {})
    write(directory / "request.json", body, canonical=True)
    source = Path(owner.__file__).resolve().parents[2] / "rust/crates/ncx-core/examples/verification_artifact_v2_development.rs"
    paths = {"runner_source_sha256": source,
             "provider_source_sha256": source.parents[2] / "ncx-provider/src/jev.rs",
             "frozen_support_sha256": source.parent / "verification_artifact_profile_formal/support.rs",
             "frozen_formal_sha256": source.parent / "verification_artifact_profile_formal/formal.rs",
             "intake_source_sha256": Path(owner.__file__),
             "builder_source_sha256": Path(owner.__file__).with_name("artifact_request_v2.py"),
             "development_cohort_source_sha256": Path(owner.__file__).with_name("artifact_v2_development_cohort.py")}
    common = {"id": "case", "group_id": "group", "request_hash": digest(body),
              "prepared_sha256": file_hash(prepared), "template_version": owner.TEMPLATE,
              "automatic_retries": 0, "formal_certification": False, "offline": False,
              "attempt_directory": str(directory), "raw_response_file": str(directory / "raw-response.json"),
              "reservation_file": str(reservation), "reservation_sha256": file_hash(reservation),
              "runner_exe_sha256": file_hash(exe), **{k: file_hash(p) for k, p in paths.items()}}
    response = {"model": "jev-1.13.0", "answers": {
        name: {"type": "noul", "noul": .1} for name in body["questions"]},
        "usage": {"input_tokens": 10, "output_tokens": 2}}
    write(directory / "raw-response.json", response)
    events = [{**common, "status": "attempt_started"},
              {**common, "status": "request_submitted", "actual_request_hash": digest(body)},
              {**common, "status": "attempt_finished", "submitted": True, "call_status": "ok",
               "response_sha256": file_hash(directory / "raw-response.json")}]

    def save():
        (directory / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in events))
        write(directory / "receipt.json", events[-1])
        write(reservation.with_suffix(".started"), events[0])
    save()
    return prepared, directory, exe, events, save


def test_original_join_returns_development_signal_only(attempt):
    prepared, directory, exe, _, _ = attempt
    result = owner.observe(prepared, directory, exe)
    assert result["acceptance_violation"] == .1
    assert result["formal_certification"] is result["production_authorized"] is False
    assert result["certified_threshold"] is None


@pytest.mark.parametrize("field,value", [
    ("offline", True), ("runner_exe_sha256", "bad"), ("intake_source_sha256", "bad"),
    ("reservation_sha256", "bad"), ("actual_request_hash", "bad")])
def test_changed_event_binding_rejected(attempt, field, value):
    prepared, directory, exe, events, save = attempt
    events[1][field] = value
    save()
    with pytest.raises(ValueError):
        owner.observe(prepared, directory, exe)


@pytest.mark.parametrize("file", ["receipt.json", "request.json", "raw-response.json"])
def test_changed_original_bytes_rejected(attempt, file):
    prepared, directory, exe, _, _ = attempt
    write(directory / file, {"changed": True})
    with pytest.raises(ValueError):
        owner.observe(prepared, directory, exe)


@pytest.mark.parametrize("status,allowed", [("cancelled", True), ("missing_key", True),
                                          ("timeout", False), ("ok", False)])
def test_unsubmitted_attempt_cannot_claim_other_outcome(attempt, status, allowed):
    prepared, directory, exe, events, save = attempt
    events.pop(1)
    events[-1].update(submitted=False, call_status=status, response_sha256=None)
    (directory / "raw-response.json").unlink()
    save()
    if allowed:
        assert owner.observe(prepared, directory, exe)["acceptance_violation"] is None
    else:
        with pytest.raises(ValueError):
            owner.observe(prepared, directory, exe)


def test_failed_response_bytes_still_authenticated(attempt):
    prepared, directory, exe, events, save = attempt
    events[-1]["call_status"] = "invalid_usage"
    save()
    assert owner.observe(prepared, directory, exe)["acceptance_violation"] is None
    write(directory / "raw-response.json", {})
    with pytest.raises(ValueError, match="raw response bytes"):
        owner.observe(prepared, directory, exe)


@pytest.mark.parametrize("eligible", [False, True])
def test_collection_unknown_is_not_negative_and_private_outcomes_not_sent(tmp_path, monkeypatch, eligible):
    from evals.verification import artifact_v2_development_cohort as cohort
    directory = tmp_path / "collection"
    directory.mkdir()
    exe = tmp_path / "main.exe"
    exe.write_bytes(b"fixture-main")
    task = {"prompt": "return x", "code": "REFERENCE_SECRET", "test_list": ["assert f(1) == 1"],
            "test_imports": []}
    row = {"id": "case", "group_id": "group", "call_eligible": eligible, "task": task,
           "candidate_code": "def f(x):\n    return x\n", "public_interface": ["def f(x):"],
           "main_receipt_file": "receipt", "main_receipt_sha256": "receipt-sha",
           "artifact_cohort_hash": "cohort", "authorization_file": "auth",
           "authorization_sha256": "auth-sha", "label": "supported", "oracle": {"status": "passed"}}
    monkeypatch.setattr(cohort, "verify_collected", lambda *args: [row])
    if not eligible:
        with pytest.raises(ValueError, match="independently labeled eligible"):
            owner.prepare_collection(directory, "case", exe)
    else:
        packet = owner.prepare_collection(directory, "case", exe)
        raw = json.dumps(packet["request"])
        assert "REFERENCE_SECRET" not in raw
        assert "label" not in packet and "oracle" not in packet
        assert packet["request"]["state"]["candidate_code"] == row["candidate_code"]
        assert packet["registered_for_formal_evaluation"] is False
