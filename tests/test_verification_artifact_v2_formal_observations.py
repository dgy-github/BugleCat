"""Original formal transport joins; mocked registration is never certification."""
import json
import os
from pathlib import Path

import pytest

from evals.verification import artifact_v2_formal_observations as owner
from evals.verification.artifact_request_v2_formal import request
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


def write(path, value, canonical=False):
    path.write_text(json.dumps(value, sort_keys=True, ensure_ascii=False,
                               separators=(",", ":") if canonical else None), encoding="utf-8")


@pytest.fixture
def original(tmp_path, monkeypatch):
    root = tmp_path / "ledger"
    directory = root / "attempts/one"
    directory.mkdir(parents=True)
    (root / "reservations").mkdir()
    exe = tmp_path / "runner.exe"
    exe.write_bytes(b"fixture")
    body = request({"prompt": "return x", "test_list": ["assert f(1) == 1"],
                    "test_imports": []}, ["def f(x):"], "def f(x):\n    return x\n", "mbpp")
    value = {"id": "case", "group_id": "group", "split": "calibration", "request": body,
             "request_hash": digest(body), "authorization_file": "auth"}
    packet = {**value, "artifact_hash": digest(value)}
    registered = {"split": "calibration", "artifact_hash": "a" * 64}
    registration = tmp_path / "registered.json"
    write(registration, registered)
    prepared = tmp_path / "packet.json"
    write(prepared, packet)
    monkeypatch.setattr(owner, "revalidate", lambda path: (registered, {}, [], [packet]))
    monkeypatch.setattr(owner, "resolve", lambda path: {"authorization": {"jev": {"ledger_dir": str(root)}}})
    monkeypatch.setattr(owner, "reservations", lambda *args: ([{
        "group_id": "group", "request_hash": packet["request_hash"]}], None))
    reservation = root / "reservations" / (digest("group") + ".json")
    write(reservation, {})
    write(directory / "request.json", body, True)
    response = {"model": "jev-1.13.0", "answers": {
        name: {"type": "noul", "noul": .1} for name in body["questions"]},
        "usage": {"input_tokens": 10, "output_tokens": 2}}
    write(directory / "raw-response.json", response)
    sources = {"runner_source_sha256": owner.SOURCE,
               "provider_source_sha256": owner.ROOT / "rust/crates/ncx-provider/src/jev.rs",
               "frozen_support_sha256": owner.SOURCE.parent / "verification_artifact_profile_formal/support.rs",
               "frozen_formal_sha256": owner.SOURCE.parent / "verification_artifact_profile_formal/formal.rs"}
    common = {"id": "case", "group_id": "group", "split": "calibration",
              "request_hash": digest(body), "prepared_file": str(prepared),
              "prepared_sha256": file_hash(prepared), "template_version": owner.TEMPLATE,
              "automatic_retries": 0, "formal_certification": False, "offline": False,
              "formal_registration_file": str(registration), "formal_registration_sha256": file_hash(registration),
              "formal_registration_hash": registered["artifact_hash"], "attempt_directory": str(directory),
              "raw_response_file": str(directory / "raw-response.json"), "reservation_file": str(reservation),
              "reservation_sha256": file_hash(reservation), "runner_exe_sha256": file_hash(exe),
              "compiled_python_owners": [{"file": "evals/verification/" + name,
                                         "sha256": file_hash(Path(owner.__file__).with_name(name))}
                                        for name in owner.PYTHON_OWNERS],
              **{key: file_hash(path) for key, path in sources.items()}}
    events = [{**common, "status": "attempt_started"},
              {**common, "status": "request_submitted", "actual_request_hash": digest(body)},
              {**common, "status": "attempt_finished", "submitted": True, "call_status": "ok",
               "response_sha256": file_hash(directory / "raw-response.json")}]

    def save():
        (directory / "events.jsonl").write_text("".join(json.dumps(event) + "\n" for event in events))
        write(directory / "receipt.json", events[-1])
        write(reservation.with_suffix(".started"), events[0])
    save()
    return packet, registration, directory, exe, events, save


def test_actual_bytes_return_only_unverified_signal(original):
    packet, registration, directory, exe, _, _ = original
    result = owner.observe(packet, registration, directory, exe)
    assert result["call_status"] == "ok" and result["acceptance_violation"] == .1
    assert result["formal_certification"] is result["production_authorized"] is False


@pytest.mark.parametrize("field,value", [("split", "holdout"), ("formal_registration_hash", "bad"),
                                         ("compiled_python_owners", []), ("offline", True),
                                         ("actual_request_hash", "bad")])
def test_changed_binding_fails_closed(original, field, value):
    packet, registration, directory, exe, events, save = original
    events[1][field] = value
    save()
    with pytest.raises(ValueError):
        owner.observe(packet, registration, directory, exe)


@pytest.mark.parametrize("file", ["receipt.json", "request.json", "raw-response.json", "registered.json"])
def test_changed_original_file_fails_closed(original, file):
    packet, registration, directory, exe, _, _ = original
    write(registration if file == "registered.json" else directory / file, {"changed": True})
    with pytest.raises(ValueError):
        owner.observe(packet, registration, directory, exe)


@pytest.mark.skipif(os.name != "nt", reason="Windows filesystem identity")
def test_device_prefix_identity_preserves_original_events(original):
    packet, registration, directory, exe, events, save = original
    for event in events:
        for key in ("prepared_file", "formal_registration_file", "attempt_directory", "raw_response_file", "reservation_file"):
            event[key] = "\\\\?\\" + event[key]
    save()
    before = (directory / "events.jsonl").read_bytes()
    assert owner.observe(packet, registration, directory, exe)["call_status"] == "ok"
    assert (directory / "events.jsonl").read_bytes() == before


@pytest.mark.parametrize("status,allowed", [("cancelled", True), ("missing_key", True), ("timeout", False), ("ok", False)])
def test_unsubmitted_failure_never_claims_probability(original, status, allowed):
    packet, registration, directory, exe, events, save = original
    events.pop(1)
    events[-1].update(submitted=False, call_status=status, response_sha256=None)
    (directory / "raw-response.json").unlink()
    save()
    if allowed:
        assert owner.observe(packet, registration, directory, exe)["acceptance_violation"] is None
    else:
        with pytest.raises(ValueError):
            owner.observe(packet, registration, directory, exe)


def test_batch_proof_still_rejects_changed_raw_response(original, monkeypatch):
    from evals.verification import artifact_v2_formal as statistics
    packet, registration, directory, exe, _, _ = original
    observed = owner.observe(packet, registration, directory, exe)
    path = directory / 'observations.jsonl'
    path.write_text(json.dumps(observed) + '\n')
    registered = {'split': 'calibration', 'artifact_hash': 'a' * 64}
    monkeypatch.setattr(statistics, 'revalidate', lambda *args: (registered, {}, [], [packet]))
    assert statistics.verified_observations(registration, path)[-1] == [observed]
    write(directory / 'raw-response.json', {'changed': True})
    with pytest.raises(ValueError, match='original response bytes changed'):
        statistics.verified_observations(registration, path)
