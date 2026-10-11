"""Run the actual new example against fake transport only; zero paid calls."""
import json
import os
import subprocess
from pathlib import Path

import pytest

from evals.verification.collection_manifest import file_hash

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact_v2_formal.exe"
SOURCE = ROOT / "rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs"


@pytest.mark.parametrize("scenario,status,calls", [
    ("success", "ok", 1), ("missing_answer", "exact_answers_required", 1),
    ("model_mismatch", "actual_model_mismatch", 1), ("timeout", "timeout", 1),
    ("cancel_before", "cancelled", 0), ("cancel_during", "cancelled", 1),
    ("extra_answer", "exact_answers_required", 1), ("invalid_usage", "invalid_usage", 1),
    ("boolean_probability", "invalid_noul", 1), ("http403", "http_403", 1),
])
def test_actual_compiled_attempt_preserves_failure_and_claim_and_no_retry(tmp_path, scenario, status, calls):
    if not BINARY.is_file():
        pytest.skip("build verification_artifact_v2_formal first")
    directory = tmp_path / scenario
    result = subprocess.run([str(BINARY), "offline-transport-test", str(directory), scenario],
                            cwd=ROOT, env={**os.environ, "NCX_JEV_API_KEY": ""},
                            capture_output=True, text=True, timeout=15, check=False)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["call_status"] == status
    assert summary["fake_transport_calls"] == calls
    assert summary["new_paid_calls"] == summary["network_calls"] == 0
    assert summary["duplicate_reservation_blocked"] is True
    events = [json.loads(line) for line in (directory / "attempt/events.jsonl").read_text().splitlines()]
    assert events[0]["status"] == "attempt_started"
    assert events[-1]["status"] == "attempt_finished"
    assert events[0]["runner_source_sha256"] == file_hash(SOURCE)
    assert events[0]["runner_exe_sha256"] == file_hash(BINARY)
    assert events[0]["request_hash"] == file_hash(directory / "attempt/request.json")
    assert events[-1]["submitted"] is (calls == 1)
    assert all(event["formal_certification"] is False and event["offline"] is True for event in events)
    if calls:
        assert events[1]["status"] == "request_submitted"
        assert events[1]["actual_request_hash"] == events[0]["request_hash"]
    if status not in {"timeout", "cancelled", "http_403"}:
        assert events[-1]["response_sha256"] == file_hash(directory / "attempt/raw-response.json")


def test_no_implicit_live_mode_or_key_exposure(tmp_path):
    if not BINARY.is_file():
        pytest.skip("build verification_artifact_v2_formal first")
    result = subprocess.run([str(BINARY), "live", "x", "x", "x", str(tmp_path / "never"), "x"],
                            cwd=ROOT, env={**os.environ, "NCX_JEV_API_KEY": "SECRET_TEST_KEY"},
                            capture_output=True, text=True, timeout=15, check=False)
    assert result.returncode != 0
    assert "SECRET_TEST_KEY" not in result.stdout + result.stderr
    assert not (tmp_path / "never").exists()
