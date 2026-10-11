"""Actual artifact main regression: fake transport only, no paid HTTP."""
import json
import os
import subprocess
from hashlib import sha256
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact_profile_formal.exe"
PREPARED = ROOT / "reports/artifact-prepared-batch02-20261011-r2.jsonl"


def digest(value):
    return sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                             ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def file_hash(path):
    return sha256(path.read_bytes()).hexdigest()


@pytest.fixture
def executable():
    if not BINARY.is_file():
        pytest.skip("build actual verification_artifact_profile_formal main first")
    return BINARY


def invoke(executable, *args):
    return subprocess.run([str(executable), *map(str, args)], cwd=ROOT,
                          env={**os.environ, "NCX_JEV_API_KEY": ""}, capture_output=True,
                          text=True, encoding="utf-8", timeout=15, check=False)


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
    assert start["runner_source_sha256"] == file_hash(ROOT / "rust/crates/ncx-core/examples/verification_artifact_profile_formal.rs")
    assert start["runner_exe_sha256"] == file_hash(executable)
    for key, source in [("runner_support_source_sha256", "ncx-core/examples/verification_artifact_profile_formal/support.rs"),
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
    assert start["formal_registration_hash"] == "offline-fixture-not-formal"
    assert start["formal_registration_sha256"] == "offline-fixture"
    assert start["offline"] is True and start["formal_certification"] is False
    for field in ("formal_registration_hash", "formal_registration_sha256", "artifact_cohort_hash", "artifact_cohort_sha256", "main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash"):
        assert all(event[field] == start[field] for event in events)
    assert start["build_info"]["formal_gate_source_sha256"] == file_hash(ROOT / "rust/crates/ncx-core/examples/verification_artifact_profile_formal/formal.rs")


def test_actual_binary_refuses_unqualified_registration_before_dataset_or_key(executable, tmp_path):
    source = tmp_path / "quota-probe.jsonl"
    original = {"id": "quota-probe", "group_id": "quota-probe-group"}
    original["artifact_sha256"] = digest(original)
    source.write_text(json.dumps(original) + "\n", encoding="utf-8")
    for supported, unsupported in ((149, 50), (150, 49), (8, 0)):
        registration = {"schema": "buglecat.verification.artifact-formal-registration.v1",
                        "registered_before_jev": True, "holdout_opened": False, "production_authorized": False,
                        "protocol": {"min_supported": 150, "min_unsupported": 50},
                        "label_counts": {"supported": supported, "unsupported": unsupported}}
        registration["artifact_hash"] = digest(registration)
        path = tmp_path / f"registration-{supported}-{unsupported}.json"
        path.write_text(json.dumps(registration), encoding="utf-8")
        result = invoke(executable, "check-registration", source, original["id"], path)
        assert result.returncode != 0
        assert "formal_independent_label_quotas_required" in result.stderr
    assert not any(path.name.endswith(".started") for path in tmp_path.iterdir())


def test_old_development_cannot_be_promoted_by_reseal_and_claimed_counts(executable, tmp_path):
    source = ROOT / "reports/artifact-prepared-smoke104-20261011.jsonl"
    if not source.is_file():
        pytest.skip("private original development receipt unavailable")
    row = json.loads(source.read_text(encoding="utf-8").splitlines()[0])
    row.update(split="calibration", source_kind="source_preregistered_original_model_code", author_exposed=False)
    row.pop("artifact_sha256")
    row["artifact_sha256"] = digest(row)
    prepared = tmp_path / "fake-formal.jsonl"
    prepared.write_text(json.dumps(row) + "\n", encoding="utf-8")
    registration = tmp_path / "not-created.json"
    output = tmp_path / "not-created-validation.json"
    result = invoke(executable, "validate", prepared, row["id"], registration, output)
    assert result.returncode != 0 and result.stderr.strip() in {"source_protocol_authorization_binding", "prepared_schema"}
    assert not output.exists()


def test_formal_live_requires_registration_and_explicit_flag(executable, tmp_path):
    result = invoke(executable, "live", "missing", "id", "registration", "reservation", tmp_path / "out")
    assert result.returncode != 0 and "usage:" in result.stderr
    assert not list(tmp_path.iterdir())


def test_real_high_original_source_bytes_profile_and_sql_join_without_registration_or_key(executable):
    path = ROOT / "reports/artifact-profile-high-main-batch01-20261011-artifacts.jsonl"
    if not path.exists():
        pytest.skip("original private high-profile artifacts unavailable")
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    assert rows
    for row in rows:
        result = invoke(executable, "check-source", path, row["id"])
        assert result.returncode == 0, result.stderr
        proof = json.loads(result.stdout)
        assert proof["request_hash"] == row["request_hash"]
        assert proof["main_resource_profile_sha256"] == row["main_resource_profile_sha256"]
        assert proof["paid_calls"] == proof["network_calls"] == 0
        assert proof["formal_certification"] is False


@pytest.mark.parametrize("field", ["main_resource_profile_file", "main_resource_profile_sha256",
                                    "main_resource_profile_hash", "native_policy_version"])
def test_resealed_high_row_cannot_change_profile_or_original_native_policy(executable, tmp_path, field):
    original = ROOT / "reports/artifact-profile-high-main-batch01-20261011-artifacts.jsonl"
    if not original.exists():
        pytest.skip("original private high-profile artifacts unavailable")
    row = json.loads(original.read_text(encoding="utf-8").splitlines()[0])
    row[field] = "host-rules-v3.2" if field == "native_policy_version" else "wrong-profile"
    row["artifact_sha256"] = digest({key: value for key, value in row.items() if key != "artifact_sha256"})
    path = tmp_path / "resealed-row.jsonl"
    path.write_text(json.dumps(row) + "\n", encoding="utf-8")
    result = invoke(executable, "check-source", path, row["id"])
    assert result.returncode != 0
    assert not any(path.name.endswith(".started") for path in tmp_path.iterdir())


def test_profile_engine_and_collector_build_fields_bind_exact_frozen_sources(executable, tmp_path):
    result = invoke(executable, "offline-transport-test", tmp_path / "offline", "success")
    assert result.returncode == 0, result.stderr
    proof = json.loads(result.stdout)["build_info"]
    for field, file in (
        ("artifact_transport_engine_source_sha256", "ncx-core/examples/verification_artifact_profile_formal/engine.rs"),
        ("collector_validator_source_sha256", "ncx-core/examples/verification_artifact_profile_collect/support.rs"),
        ("profile_collection_engine_source_sha256", "ncx-core/examples/verification_artifact_profile_collect/engine.rs"),
        ("profile_collector_source_sha256", "ncx-core/examples/verification_artifact_profile_collect.rs"),
        ("frozen_collector_support_sha256", "ncx-core/examples/verification_artifact_collect/support.rs"),
        ("legacy_collector_support_sha256", "ncx-core/examples/verification_collect/support.rs"),
    ):
        assert proof[field] == file_hash(ROOT / "rust/crates" / file)
