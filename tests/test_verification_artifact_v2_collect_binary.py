"""Actual new development executable, fake transport and temporary ledger only."""
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.collection_manifest import file_hash

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "rust/target/debug/examples/verification_artifact_v2_collect.exe"
SOURCE = ROOT / "rust/crates/ncx-core/examples/verification_artifact_v2_collect.rs"


@pytest.mark.parametrize("scenario,status,started", [
    ("success", "ok", 268), ("failure", "offline_transport_failure", 268),
    ("malformed", "response_content_invalid", 268), ("model_mismatch", "response_model_or_usage_invalid", 268),
    ("invalid_usage", "response_model_or_usage_invalid", 268), ("timeout", "timeout", 268),
    ("cancel_before", None, 267), ("cancel_during", "cancelled", 268),
])
def test_actual_binary_shared_counter_and_original_receipt(tmp_path, scenario, status, started):
    if not BINARY.exists():
        pytest.skip("build new development collector first")
    directory = tmp_path / scenario
    env = {k: os.environ[k] for k in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATH") if k in os.environ}
    env.update(DEEPSEEK_API_KEY="", NCX_JEV_API_KEY="")
    child = subprocess.run([str(BINARY), "offline-test", str(directory), scenario], cwd=ROOT,
                           env=env, capture_output=True, text=True, timeout=15, check=False)
    assert child.returncode == 0, child.stderr
    summary = json.loads(child.stdout)
    assert summary["paid_calls"] == summary["http_calls"] == 0
    assert summary["global_main_attempts"] == started and summary["remaining"] == 750 - started
    assert summary["build_info"]["runner_source_sha256"] == file_hash(SOURCE)
    assert summary["build_info"]["executable_sha256"] == file_hash(BINARY)
    with sqlite3.connect(directory / "offline-ledger/main-attempts.sqlite3") as db:
        assert db.execute("SELECT count(*) FROM attempts").fetchone()[0] == started
        assert db.execute("SELECT value FROM metadata WHERE key='authorization_sha256'").fetchone()[0] == "offline"
        if status:
            saved = json.loads(db.execute("SELECT finished_json FROM attempts WHERE ordinal=268").fetchone()[0])
            assert saved["call_status"] == status
    if status:
        receipt = json.loads((directory / "receipt.json").read_text())
        assert receipt["call_status"] == status and receipt["attempt_ordinal"] == 268
        events = [json.loads(line) for line in (directory / "attempt-events.jsonl").read_text().splitlines()]
        assert [e["event"] for e in events] == ["attempt_started", "request_submitted", "attempt_finished"]
        assert events[1]["actual_request_hash"] == file_hash(directory / "request.json")
        assert events[-1]["receipt_sha256"] == file_hash(directory / "receipt.json")


def test_no_implicit_live_and_invalid_source_stops_before_counter(tmp_path):
    if not BINARY.exists():
        pytest.skip("build new development collector first")
    child = subprocess.run([str(BINARY), "live", str(tmp_path)], cwd=ROOT, capture_output=True, text=True, timeout=15)
    assert child.returncode != 0
    assert not (tmp_path / "main-attempts.sqlite3").exists()
