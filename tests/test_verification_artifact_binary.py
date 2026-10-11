"""Actual artifact main regression: fake transport only, no paid HTTP."""
import json
import os
import subprocess
from hashlib import sha256
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact.exe"
PREPARED = ROOT / "reports/artifact-prepared-batch02-20261011-r2.jsonl"


def digest(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                             ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def file_hash(path):
    return sha256(path.read_bytes()).hexdigest()


@pytest.fixture
def executable():
    if not BINARY.is_file():
        pytest.skip("build actual verification_artifact main first")
    return BINARY


def invoke(executable, *args):
    return subprocess.run([str(executable), *map(str, args)], cwd=ROOT,
                          env={**os.environ, "NCX_JEV_API_KEY": ""}, capture_output=True,
                          text=True, timeout=15, check=False)


@pytest.mark.parametrize("scenario,status,calls", [
    ("success", "ok", 1), ("malformed", "invalid_json", 1),
    ("model_mismatch", "actual_model_mismatch", 1), ("invalid_usage", "invalid_usage", 1),
    ("extra_answers", "invalid_single_noul_answer", 1), ("http500", "http_500", 1),
    ("cancel_before", "cancelled", 0), ("cancel_during", "cancelled", 1), ("timeout", "timeout", 1),
])
def test_main_keeps_bytes_and_claimed_budget_without_retries(executable, tmp_path, scenario, status, calls):
    directory = tmp_path / scenario
    result = invoke(executable, "offline-transport-test", directory, scenario)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["paid_calls"] == summary["paid_attempts"] == summary["network_calls"] == 0
    assert summary["fake_transport_calls"] == calls
    assert summary["duplicate_reservation_blocked"] is True
    assert summary["call_status"] == status
    attempt = directory / "attempt"
    events = [json.loads(line) for line in (attempt / "events.jsonl").read_text(encoding="utf-8").splitlines()]
    start, finished = events[0], events[-1]
    assert start["status"] == "attempt_started"
    assert finished["status"] == "attempt_finished"
    assert start["request_hash"] == digest(start["request"]) == file_hash(attempt / "request.json")
    assert start["runner_source_sha256"] == file_hash(ROOT / "rust/crates/ncx-core/examples/verification_artifact.rs")
    assert start["runner_exe_sha256"] == file_hash(executable)
    for key, source in [("runner_support_source_sha256", "ncx-core/examples/verification_artifact/support.rs"),
                        ("provider_transport_source_sha256", "ncx-provider/src/jev.rs")]:
        assert start["build_info"][key] == file_hash(ROOT / "rust/crates" / source)
    assert list(start["request"]["questions"]) == ["acceptance_violation"]
    assert start["request"]["state"]["statement_origin"] == "host_artifact_hypothesis"
    assert "label" not in start["request"]["state"]
    if scenario != "cancel_before":
        assert events[1]["status"] == "request_submitted"
        assert events[1]["actual_request_hash"] == start["request_hash"]
    raw = attempt / "raw-response.json"
    assert raw.exists() == (scenario in {"success", "malformed", "model_mismatch", "invalid_usage", "extra_answers"})
    if raw.exists():
        assert finished["response_sha256"] == file_hash(raw)
        assert os.path.samefile(finished["raw_response_file"], raw)
    if scenario == "success":
        assert finished["response"]["answers"] == json.loads(raw.read_text(encoding="utf-8"))["answers"]
        assert finished["usage"] == {"input_tokens": 1, "output_tokens": 2}
    assert "secret non-2xx" not in (attempt / "events.jsonl").read_text(encoding="utf-8")
    assert "secret non-2xx" not in (attempt / "receipt.json").read_text(encoding="utf-8")
    marker = directory / "fixture.started"
    before = file_hash(marker)
    assert invoke(executable, "offline-transport-test", directory, scenario).returncode != 0
    assert file_hash(marker) == before


def test_real_frozen_development_validates_without_key_or_reservation(executable, tmp_path):
    if not PREPARED.is_file():
        pytest.skip("local frozen development source unavailable")
    result = invoke(executable, "validate", PREPARED, "mbpp-main-3", tmp_path / "validated.json")
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["network_calls"] == summary["paid_calls"] == 0
    assert summary["formal_certification"] is False
    row = next(json.loads(line) for line in PREPARED.read_text(encoding="utf-8").splitlines()
               if json.loads(line)["id"] == "mbpp-main-3")
    assert summary["request_hash"] == digest(row["request"])
    assert summary["prepared_artifact_sha256"] == digest({k: v for k, v in row.items() if k != "artifact_sha256"})


def test_formal_and_label_contaminated_bodies_reject_before_transport(executable, tmp_path):
    if not PREPARED.is_file():
        pytest.skip("local frozen development source unavailable")
    row = json.loads(PREPARED.read_text(encoding="utf-8").splitlines()[0])
    for name in ("formal", "label"):
        changed = json.loads(json.dumps(row))
        if name == "formal":
            changed["split"] = "calibration"
        else:
            changed["request"]["state"]["label"] = "supported"
            changed["request_hash"] = digest(changed["request"])
        changed.pop("artifact_sha256")
        changed["artifact_sha256"] = digest(changed)
        path = tmp_path / f"{name}.jsonl"
        path.write_text(json.dumps(changed, ensure_ascii=False) + "\n", encoding="utf-8")
        output = tmp_path / f"{name}.json"
        result = invoke(executable, "validate", path, row["id"], output)
        assert result.returncode != 0
        assert "artifact_body_contract" in result.stderr
        assert not output.exists()


def test_live_requires_explicit_flag(executable, tmp_path):
    result = invoke(executable, "live", "missing", "id", "missing", tmp_path / "out")
    assert result.returncode != 0
    assert "usage:" in result.stderr
    assert not list(tmp_path.iterdir())
