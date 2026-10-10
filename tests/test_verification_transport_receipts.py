import hashlib
import json

import pytest

from evals.verification.calibration import digest
from evals.verification.transport_receipts import verify


def fixture(tmp_path):
    executable, source = tmp_path / "runner.exe", tmp_path / "runner.rs"
    executable.write_bytes(b"test-executable")
    source.write_bytes(b"test-source")
    request = {"state": "test", "model": "jev-latest"}
    identity = {"request_hash": digest(request), "runner_exe_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                "runner_source_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
    raw = {"model": "jev-test", "answers": {"contradiction": {"type": "noul", "noul": .1}},
           "usage": {"input_tokens": 2, "output_tokens": 3}}
    path = tmp_path / "response.json"
    path.write_text(json.dumps(raw))
    start = {**identity, "status": "attempt_started"}
    submit = {**identity, "status": "request_submitted", "actual_request_hash": digest(request), "planned_raw_response_file": str(path)}
    end = {"status": "attempt_finished", "call_status": "ok", "raw_response_file": str(path), "response": raw,
           "response_sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    return start, submit, end, request, tmp_path, executable, source


def test_original_bytes_join_and_candidate_only_json_is_not_enough(tmp_path):
    args = fixture(tmp_path)
    assert verify(*args)["raw_response_verified"]
    args[2]["response"] = {**args[2]["response"], "answers": {"contradiction": {"type": "noul", "noul": .9}}}
    with pytest.raises(ValueError, match="parsed answers"):
        verify(*args)


def test_response_bytes_and_executable_mutation_rejected(tmp_path):
    args = fixture(tmp_path)
    args[-2].write_bytes(b"other-executable")
    with pytest.raises(ValueError, match="compiled"):
        verify(*args)
