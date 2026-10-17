"""Actual profile main, fake bounded transport only; no real cohort registered."""
import copy
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification import artifact_profile_cohort as profile
from evals.verification.calibration import digest, load
from evals.verification.collection_manifest import file_hash
from evals.verification.collection_receipts import read_lines, verify

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact_profile_collect.exe"
SOURCE = ROOT / "rust/crates/ncx-core/examples/verification_artifact_profile_collect.rs"
PROFILE = ROOT / "reports/artifact-main-resource-profile-high-20261011-v1.json"


@pytest.fixture(autouse=True)
def built_binary():
    if not BINARY.is_file():
        pytest.skip("build the verification_artifact_profile_collect example first")


@pytest.fixture
def private_profile():
    if not PROFILE.is_file():
        pytest.fail("original local profile evidence is not distributed in Git")
    value = load(PROFILE)
    for field in ("previous_artifact_cohort_file", "diagnostic_file"):
        if not Path(value[field]).is_file():
            pytest.fail("original local profile evidence is not distributed in Git")
    return PROFILE


def run(*args):
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": ""}
    return subprocess.run([str(BINARY), *map(str, args)], cwd=ROOT, env=env, capture_output=True,
                          encoding="utf-8", timeout=20, check=False)


def assert_build(build):
    assert build["runner_source_sha256"] == file_hash(SOURCE)
    assert build["executable_sha256"] == file_hash(BINARY)
    for field, path in (("support_source_sha256", SOURCE.parent / SOURCE.stem / "support.rs"),
                        ("collection_engine_source_sha256", SOURCE.parent / SOURCE.stem / "engine.rs"),
                        ("frozen_collector_support_sha256", SOURCE.parent / "verification_artifact_collect/support.rs"),
                        ("request_builder_source_sha256", ROOT / "rust/crates/ncx-provider/src/request.rs")):
        assert build[field] == file_hash(path)


@pytest.mark.historical_artifact
def test_actual_profile_body_uses_high_without_key_or_config_mutation(private_profile):
    value, bindings = profile.validate_profile(PROFILE)
    assert len(bindings) == 10
    validated = run("validate-profile", PROFILE)
    assert validated.returncode == 0, validated.stderr
    assert json.loads(validated.stdout) == value
    result = run("offline-body-test", PROFILE)
    assert result.returncode == 0, result.stderr
    body = json.loads(result.stdout)
    assert body["paid_calls"] == body["http_calls"] == 0
    assert body["settings"] == profile.SETTINGS
    assert body["body"]["max_tokens"] == 16384 and body["body"]["reasoning_effort"] == "high"
    assert body["body"]["thinking"] == {"type": "enabled"}
    assert body["request_hash"] == digest(body["body"])
    assert "tools" not in body["body"]
    assert_build(body["build_info"])


@pytest.mark.parametrize("field", ["max_tokens", "reasoning_effort", "timeout_s", "max_retries",
                                    "previous_artifact_cohort_sha256", "diagnostic_sha256", "profile_id"])
@pytest.mark.historical_artifact
def test_resealed_profile_tamper_rejected_without_config_transport_or_output(tmp_path, field, private_profile):
    value = load(PROFILE)
    if field in profile.SETTINGS:
        value["main_collection_settings"][field] = "ultra" if field == "reasoning_effort" else 1
    else:
        value[field] = "tampered"
    value["artifact_hash"] = digest({k: v for k, v in value.items() if k != "artifact_hash"})
    path = tmp_path / "profile.json"
    path.write_text(json.dumps(value), encoding="utf-8")
    result = run("validate-profile", path)
    assert result.returncode != 0
    with pytest.raises(ValueError):
        profile.validate_profile(path)
    assert list(tmp_path.iterdir()) == [path]


@pytest.mark.parametrize("scenario,status", [("success", "ok"), ("failure", "offline_transport_failure"),
    ("malformed", "response_content_invalid"), ("model_mismatch", "response_model_or_usage_invalid"),
    ("invalid_usage", "response_model_or_usage_invalid"), ("timeout", "timeout"), ("cancel_during", "cancelled")])
def test_actual_receipts_keep_profile_fields_and_old35_slots(tmp_path, scenario, status):
    directory = tmp_path / "attempt"
    response = run("offline-test", directory, scenario)
    assert response.returncode == 0, response.stderr
    summary = json.loads(response.stdout)
    assert summary["paid_calls"] == summary["http_calls"] == 0
    assert summary["fake_transport_calls"] == 1 and summary["global_main_attempts"] == 36
    assert summary["remaining"] == 464
    receipt = load(directory / "receipt.json")
    assert receipt["call_status"] == status
    events = read_lines(directory / "attempt-events.jsonl")
    for field in ("main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash"):
        assert all(event[field] == receipt[field] for event in events)
        assert receipt[field].startswith("offline")
    with sqlite3.connect(directory / "offline-ledger/main-attempts.sqlite3") as db:
        records = db.execute("SELECT group_id,started_json FROM attempts ORDER BY ordinal").fetchall()
    assert [row[0] for row in records[:35]] == [f"old-{n}" for n in range(35)]
    assert json.loads(records[-1][1])["main_resource_profile_sha256"] == receipt["main_resource_profile_sha256"]
    assert events[-1]["receipt_sha256"] == file_hash(directory / "receipt.json")
    assert receipt["request_hash"] == receipt["actual_request_hash"] == file_hash(directory / "request.json")
    if status == "ok":
        prepared = {field: receipt[field] for field in ("id", "group_id", "task_id", "split", "source_sha256",
                     "protocol_sha256", "authorization_sha256", "request_hash", "settings", "build_info")}
        prepared.update(schema="buglecat.verification.prepared-main.v1", body=load(directory / "request.json"))
        prepared["artifact_sha256"] = digest(prepared)
        assert verify(prepared, directory / "receipt.json", events, BINARY) == receipt
        assert receipt["result"]["host_B"]["policy_version"] == "host-rules-v3.4"
    before = file_hash(directory / "receipt.json")
    assert run("offline-test", directory, scenario).returncode != 0
    assert file_hash(directory / "receipt.json") == before
    assert_build(summary["build_info"])


def test_prestart_cancel_consumes_no_new_attempt(tmp_path):
    directory = tmp_path / "cancel"
    result = run("offline-test", directory, "cancel_before")
    assert result.returncode == 0, result.stderr
    value = json.loads(result.stdout)
    assert value["global_main_attempts"] == 35 and value["fake_transport_calls"] == 0
    assert value["remaining"] == 465 and not (directory / "receipt.json").exists()


def test_live_needs_explicit_flag_without_side_effects(tmp_path):
    assert run("live", tmp_path / "absent").returncode != 0
    assert not list(tmp_path.iterdir())


@pytest.mark.historical_artifact
def test_profile_wrapper_reuses_owner_and_does_not_register_real_cohort(tmp_path, monkeypatch, private_profile):
    # Explicitly mock upstream source registration; this is not a formal sample.
    previous = load(load(PROFILE)["previous_artifact_cohort_file"])
    original = copy.deepcopy(previous)
    original["ledger_snapshot"] = {"fixture": "no real ledger"}
    monkeypatch.setattr(profile.artifact_cohort, "preregister", lambda *_: copy.deepcopy(original))
    monkeypatch.setattr(profile.artifact_cohort, "ledger_snapshot", lambda _: ({"fixture": "no real ledger"}, []))
    value = profile.preregister("mock", "mock", "mock", "mock", PROFILE, "code-fixture")
    assert value["main_collection_settings"] == profile.SETTINGS
    assert value["previous_resource_candidates_included"] is False
    assert value["main_resource_profile_sha256"] == file_hash(PROFILE)
    assert value["main_resource_profile_hash"] == load(PROFILE)["artifact_hash"]
    assert previous["main_collection_settings"]["reasoning_effort"] == "ultra"
    assert not list(tmp_path.iterdir())
