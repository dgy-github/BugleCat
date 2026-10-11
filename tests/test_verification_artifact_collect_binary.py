"""Actual new collector entry, fake transport only; no paid calls."""
import json
import os
import sqlite3
import subprocess
from hashlib import sha256
from pathlib import Path

import pytest

from evals.verification.calibration import digest
from evals.verification.collection_receipts import read_lines, verify

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact_collect.exe"
COHORT = ROOT / "reports/artifact-formal-cohort-expanded-20261011-v1.json"


def file_hash(path):
    return sha256(Path(path).read_bytes()).hexdigest()


def run(*args):
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": ""}
    return subprocess.run([str(BINARY), *map(str, args)], cwd=ROOT, env=env, capture_output=True,
                          text=True, encoding="utf-8", timeout=20, check=False)


@pytest.fixture(autouse=True)
def executable():
    if not BINARY.is_file():
        pytest.skip("build verification_artifact_collect main first")


@pytest.mark.parametrize("scenario,status", [("success", "ok"), ("failure", "offline_transport_failure"),
                                             ("malformed", "response_content_invalid"),
                                             ("model_mismatch", "response_model_or_usage_invalid"),
                                             ("invalid_usage", "response_model_or_usage_invalid"),
                                             ("timeout", "timeout"), ("cancel_during", "cancelled")])
def test_actual_binary_preserves_old12_plus_one_and_bound_cohort_receipt(tmp_path, scenario, status):
    directory = tmp_path / scenario
    result = run("offline-test", directory, scenario)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["http_calls"] == summary["paid_calls"] == 0
    assert summary["fake_transport_calls"] == 1
    assert summary["global_main_attempts"] == 13 and summary["remaining"] == 487
    receipt = json.loads((directory / "receipt.json").read_text(encoding="utf-8"))
    assert receipt["call_status"] == status and receipt["attempt_ordinal"] == 13
    events = read_lines(directory / "attempt-events.jsonl")
    assert [e["event"] for e in events] == ["attempt_started", "request_submitted", "attempt_finished"]
    for key in ("artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash"):
        assert all(e[key] == receipt[key] for e in events)
    assert receipt["request_hash"] == receipt["actual_request_hash"] == file_hash(directory / "request.json")
    assert events[-1]["receipt_sha256"] == file_hash(directory / "receipt.json")
    with sqlite3.connect(directory / "offline-ledger/main-attempts.sqlite3") as db:
        rows = db.execute("SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal").fetchall()
    assert len(rows) == 13 and [row[1] for row in rows[:12]] == [f"old-{i}" for i in range(12)]
    started = json.loads(rows[-1][2])
    assert started["artifact_cohort_sha256"] == receipt["artifact_cohort_sha256"]
    assert json.loads(rows[-1][3])["call_status"] == status
    assert (directory / "raw-response.json").exists() == (scenario in {"success", "malformed", "model_mismatch", "invalid_usage"})
    if scenario == "success":
        raw = json.loads((directory / "raw-response.json").read_text(encoding="utf-8"))
        assert receipt["result"]["candidate"] == raw["choices"][0]["message"]["content"]
        messages = read_lines(Path(receipt["result"]["session_log"]))
        assert [m["role"] for m in messages] == ["user", "assistant"]
        assert receipt["result"]["host_B"]["policy_version"] == "host-rules-v3.3"
        assert not receipt["result"]["assertions_executed"] and receipt["result"]["tools_executed"] == 0
        prepared = {k: receipt[k] for k in ("id", "group_id", "task_id", "split", "source_sha256",
                                           "protocol_sha256", "authorization_sha256", "request_hash", "settings", "build_info")}
        prepared.update(schema="buglecat.verification.prepared-main.v1",
                        body=json.loads((directory / "request.json").read_text(encoding="utf-8")))
        prepared["artifact_sha256"] = digest(prepared)
        assert verify(prepared, directory / "receipt.json", events, BINARY) == receipt
    before = file_hash(directory / "receipt.json")
    assert run("offline-test", directory, scenario).returncode != 0
    assert file_hash(directory / "receipt.json") == before
    build = summary["build_info"]
    assert build["executable_sha256"] == file_hash(BINARY)
    assert build["runner_source_sha256"] == file_hash(ROOT / "rust/crates/ncx-core/examples/verification_artifact_collect.rs")
    assert build["support_source_sha256"] == file_hash(ROOT / "rust/crates/ncx-core/examples/verification_artifact_collect/support.rs")


def test_cancel_before_submission_does_not_consume_extra_group(tmp_path):
    directory = tmp_path / "cancel"
    result = run("offline-test", directory, "cancel_before")
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["fake_transport_calls"] == 0 and summary["global_main_attempts"] == 12
    assert not (directory / "receipt.json").exists()
    assert read_lines(directory / "attempt-events.jsonl") == []


def manifest(cohort_file, count=8):
    cohort = json.loads(cohort_file.read_text(encoding="utf-8"))
    roles = {b["role"]: b for b in cohort["bindings"]}
    value = {"schema": "buglecat.verification.mbpp-collection.v1", **cohort["main_collection_settings"],
             "artifact_cohort_file": str(cohort_file), "artifact_cohort_sha256": file_hash(cohort_file),
             "artifact_cohort_hash": cohort["artifact_hash"], "cases": cohort["cases"][:count]}
    for name in ("source", "catalog", "authorization", "protocol"):
        value[f"{name}_file"], value[f"{name}_sha256"] = roles[name]["file"], roles[name]["sha256"]
    return value


def test_real_formal_cohort_prepare_does_not_send_and_binds_all_raw_fields(tmp_path):
    if not COHORT.is_file():
        pytest.skip("actual preregistered cohort unavailable")
    path = tmp_path / "manifest.json"
    frozen = manifest(COHORT)
    path.write_text(json.dumps(frozen), encoding="utf-8")
    ledger = ROOT / "evals/verification/reports/benchmark_400_budget_20261010/main/main-attempts.sqlite3"
    before = file_hash(ledger)
    directory = tmp_path / "prepare"
    result = run("prepare", path, directory)
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["paid_calls"] == 0
    rows = read_lines(directory / "prepared.jsonl")
    assert len(rows) == 8 and file_hash(ledger) == before
    for row, case in zip(rows, frozen["cases"]):
        assert row["id"] == case["id"] and row["split"] == "calibration"
        for field in ("artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash"):
            assert row[field] == frozen[field]
        request = Path(row["case_directory"]) / "request.json"
        assert file_hash(request) == row["request_hash"]
        assert row["reference_and_assertions_sent"] is False
        assert row["settings"]["max_retries"] == 0 and row["settings"]["max_tokens"] == 8192
    assert not (directory / "attempt-events.jsonl").exists()


@pytest.mark.parametrize("mutation", ["too_many", "holdout", "missing", "fixture", "body"])
def test_invalid_formal_cohort_is_refused_before_config_transport_or_output(tmp_path, mutation):
    if not COHORT.is_file():
        pytest.skip("private preregistered cohort unavailable")
    value = manifest(COHORT, 9 if mutation == "too_many" else 8)
    if mutation == "holdout":
        value["cases"][0]["split"] = "holdout"
    elif mutation == "missing":
        value.pop("artifact_cohort_file")
    elif mutation in {"fixture", "body"}:
        cohort = json.loads(COHORT.read_text(encoding="utf-8"))
        if mutation == "fixture":
            cohort["fixture_only"] = True
        else:
            cohort["main_collection_settings"]["max_tokens"] = 4096
        cohort["artifact_hash"] = digest({k: v for k, v in cohort.items() if k != "artifact_hash"})
        clone = tmp_path / "tampered.json"
        clone.write_text(json.dumps(cohort), encoding="utf-8")
        value.update(artifact_cohort_file=str(clone), artifact_cohort_sha256=file_hash(clone), artifact_cohort_hash=cohort["artifact_hash"])
    path, directory = tmp_path / "manifest.json", tmp_path / "output"
    path.write_text(json.dumps(value), encoding="utf-8")
    result = run("prepare", path, directory)
    assert result.returncode != 0 and not directory.exists()


def test_live_requires_explicit_flag_and_has_no_side_effect(tmp_path):
    assert run("live", tmp_path / "absent").returncode != 0
    assert not list(tmp_path.iterdir())
