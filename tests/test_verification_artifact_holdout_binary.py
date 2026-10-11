"""Actual new holdout entries with fake transports. No real holdout is opened."""
import json
import os
import subprocess
from pathlib import Path

import pytest

from evals.verification.artifact_formal_observations import holdout_owner_sources, runner_sources
from evals.verification.calibration import digest, load
from evals.verification.collection_manifest import file_hash
from evals.verification.collection_receipts import read_lines

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / "rust/crates/ncx-core/examples"
MAIN_SOURCE = EXAMPLES / "verification_artifact_holdout_collect.rs"
JEV_SOURCE = EXAMPLES / "verification_artifact_holdout_formal.rs"
MAIN = ROOT / "rust/target/debug/examples/verification_artifact_holdout_collect.exe"
JEV = ROOT / "rust/target/debug/examples/verification_artifact_holdout_formal.exe"


@pytest.fixture(autouse=True)
def built_binaries():
    if not MAIN.is_file() or not JEV.is_file():
        pytest.skip("build both verification_artifact_holdout examples first")


def run(binary, *args):
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": ""}
    return subprocess.run([str(binary), *map(str, args)], cwd=ROOT, env=env, capture_output=True,
                          encoding="utf-8", timeout=30, check=False)


def assert_owners(build):
    for field, path in holdout_owner_sources(EXAMPLES).items():
        assert build[field] == file_hash(path)
    assert build["profile_collector_validator_source_sha256"] == file_hash(
        EXAMPLES / "verification_artifact_profile_collect/support.rs")


@pytest.mark.parametrize("scenario,status", [("success", "ok"), ("failure", "offline_transport_failure"),
    ("malformed", "response_content_invalid"), ("model_mismatch", "response_model_or_usage_invalid"),
    ("invalid_usage", "response_model_or_usage_invalid"), ("timeout", "timeout"), ("cancel_during", "cancelled")])
def test_actual_main_keeps_bytes_budget_and_compiled_owners(tmp_path, scenario, status):
    out = tmp_path / "main"
    result = run(MAIN, "offline-test", out, scenario)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["paid_calls"] == summary["http_calls"] == 0 and summary["fake_transport_calls"] == 1
    assert summary["global_main_attempts"] == 36 and summary["remaining"] == 464
    receipt = load(out / "receipt.json")
    events = read_lines(out / "attempt-events.jsonl")
    assert receipt["split"] == "development" and receipt["call_status"] == status
    assert events[-1]["receipt_sha256"] == file_hash(out / "receipt.json")
    assert receipt["request_hash"] == receipt["actual_request_hash"] == file_hash(out / "request.json")
    build = summary["build_info"]
    assert build["executable_sha256"] == file_hash(MAIN)
    assert build["runner_source_sha256"] == file_hash(MAIN_SOURCE)
    assert build["support_source_sha256"] == file_hash(MAIN_SOURCE.parent / MAIN_SOURCE.stem / "support.rs")
    assert build["collection_engine_source_sha256"] == file_hash(MAIN_SOURCE.parent / MAIN_SOURCE.stem / "engine.rs")
    assert_owners(build)
    previous = file_hash(out / "receipt.json")
    assert run(MAIN, "offline-test", out, scenario).returncode != 0
    assert file_hash(out / "receipt.json") == previous


def test_actual_main_prestart_cancel_without_key(tmp_path):
    result = run(MAIN, "offline-test", tmp_path / "cancel", "cancel_before")
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["global_main_attempts"] == 35
    assert not (tmp_path / "cancel/receipt.json").exists()


def test_actual_main_high_body_without_key():
    profile = ROOT / "reports/artifact-main-resource-profile-high-20261011-v1.json"
    if not profile.is_file():
        pytest.skip("original local profile evidence is not distributed in Git")
    result = run(MAIN, "offline-body-test", profile)
    assert result.returncode == 0, result.stderr
    value = json.loads(result.stdout)
    assert value["body"]["reasoning_effort"] == "high" and value["body"]["max_tokens"] == 16384
    assert value["request_hash"] == digest(value["body"]) and "tools" not in value["body"]


@pytest.mark.parametrize("scenario,status,calls", [("success", "ok", 1), ("malformed", "invalid_json", 1),
    ("model_mismatch", "actual_model_mismatch", 1), ("invalid_usage", "invalid_usage", 1),
    ("extra_answers", "invalid_single_noul_answer", 1), ("http500", "http_500", 1),
    ("cancel_before", "cancelled", 0), ("cancel_during", "cancelled", 1), ("timeout", "timeout", 1)])
def test_actual_jev_single_question_raw_bytes_and_once_reservation(tmp_path, scenario, status, calls):
    out = tmp_path / "jev"
    result = run(JEV, "offline-transport-test", out, scenario)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["paid_calls"] == summary["paid_attempts"] == summary["network_calls"] == 0
    assert summary["fake_transport_calls"] == calls and summary["duplicate_reservation_blocked"] is True
    assert summary["call_status"] == status
    attempt = out / "attempt"
    events = read_lines(attempt / "events.jsonl")
    start, finish = events[0], events[-1]
    assert start["offline"] is True and start["formal_certification"] is False
    assert start["formal_registration_hash"] == "offline-fixture-not-formal"
    assert start["request_hash"] == file_hash(attempt / "request.json") == digest(start["request"])
    assert list(start["request"]["questions"]) == ["acceptance_violation"]
    assert "label" not in start["request"]["state"]
    if calls:
        assert events[1]["actual_request_hash"] == start["request_hash"]
    for field, source in runner_sources(JEV_SOURCE).items():
        assert start["build_info"][field] == file_hash(source)
    assert start["runner_exe_sha256"] == file_hash(JEV)
    assert_owners(start["build_info"])
    raw = attempt / "raw-response.json"
    assert raw.exists() == (scenario in {"success", "malformed", "model_mismatch", "invalid_usage", "extra_answers"})
    if raw.exists():
        assert finish["response_sha256"] == file_hash(raw)
    if status == "ok":
        assert finish["usage"] == {"input_tokens": 1, "output_tokens": 2}
        assert finish["response"]["answers"] == load(raw)["answers"]
    assert "secret non-2xx" not in (attempt / "events.jsonl").read_text(encoding="utf-8")
    marker_hash = file_hash(out / "fixture.started")
    assert run(JEV, "offline-transport-test", out, scenario).returncode != 0
    assert file_hash(out / "fixture.started") == marker_hash


def test_actual_qualified_owner_subprocess_rejects_old_calibration_and_resealed_holdout(tmp_path):
    cohort_file = ROOT / "reports/artifact-profile-cohort-high-20261011-v1.json"
    if not cohort_file.is_file():
        pytest.skip("original local cohort evidence is not distributed in Git")
    body = load(cohort_file)
    probe = tmp_path / "manifest.json"
    probe.write_text(json.dumps({"artifact_cohort_file": str(cohort_file)}), encoding="utf-8")
    result = run(MAIN, "check-gate", probe)
    assert result.returncode != 0 and "qualified_original_holdout_gate_required" in result.stderr
    body.update(split="holdout", holdout_opened=True, collection_authorized=False,
                holdout_lock={"threshold": 0.95, "production_authorized": False, "threshold_retuning_allowed": False})
    body["artifact_hash"] = digest({key: value for key, value in body.items() if key != "artifact_hash"})
    fake = tmp_path / "fake-holdout.json"
    fake.write_text(json.dumps(body), encoding="utf-8")
    probe.write_text(json.dumps({"artifact_cohort_file": str(fake)}), encoding="utf-8")
    assert run(MAIN, "check-gate", probe).returncode != 0
    assert not any(path.name.endswith(".started") for path in tmp_path.iterdir())
    assert run(JEV, "check-gate", tmp_path / "missing", "id", tmp_path / "missing-reg").returncode != 0


@pytest.mark.parametrize("positive,negative", [(149, 50), (150, 49), (8, 0)])
def test_actual_holdout_registration_refuses_missing_quotas_before_dataset(tmp_path, positive, negative):
    row = {"id": "quota-probe", "group_id": "quota-probe-group"}
    row["artifact_sha256"] = digest(row)
    prepared = tmp_path / "probe.jsonl"
    prepared.write_text(json.dumps(row) + "\n", encoding="utf-8")
    registration = {"schema": "buglecat.verification.artifact-holdout-registration.v1",
                    "registered_before_jev": True, "threshold_retuning_allowed": False, "production_authorized": False,
                    "protocol": {"min_supported": 150, "min_unsupported": 50},
                    "label_counts": {"supported": positive, "unsupported": negative}}
    registration["artifact_hash"] = digest(registration)
    path = tmp_path / "registration.json"
    path.write_text(json.dumps(registration), encoding="utf-8")
    result = run(JEV, "check-registration", prepared, row["id"], path)
    assert result.returncode != 0 and "formal_independent_label_quotas_required" in result.stderr
    assert not any(path.name.endswith(".started") for path in tmp_path.iterdir())


def test_actual_entries_need_explicit_live_without_side_effects(tmp_path):
    assert run(MAIN, "live", tmp_path / "absent").returncode != 0
    assert run(JEV, "live", "absent", "id", "reg", "res", tmp_path / "out").returncode != 0
    assert not list(tmp_path.iterdir())
