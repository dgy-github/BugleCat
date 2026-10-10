"""Exercise the built main entry point with fake transport, never a paid API."""

import json
import os
import sqlite3
import subprocess
from hashlib import sha256
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("NCX_COLLECT_BINARY", ROOT / "rust/target/debug/examples/verification_collect.exe"))


@pytest.fixture
def executable():
    if not BINARY.is_file():
        pytest.skip("build verification_collect main executable first")
    return BINARY


def invoke(executable, *args):
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": ""}
    return subprocess.run([str(executable), *map(str, args)], cwd=ROOT, env=env,
                          capture_output=True, text=True, timeout=15, check=False)


def digest(path):
    return sha256(path.read_bytes()).hexdigest()


def test_main_binary_persists_original_response_request_session_and_build(executable, tmp_path):
    directory = tmp_path / "success"
    result = invoke(executable, "offline-receipt-test", directory)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["paid_calls"] == summary["http_calls"] == 0
    receipt = json.loads((directory / "receipt.json").read_text())
    events = [json.loads(line) for line in (directory / "attempt-events.jsonl").read_text().splitlines()]
    assert receipt["call_status"] == "ok"
    assert receipt["request_hash"] == receipt["actual_request_hash"] == digest(directory / "request.json")
    assert events[1]["actual_request_hash"] == receipt["actual_request_hash"]
    assert events[2]["response_sha256"] == digest(directory / "raw-response.json")
    raw = json.loads((directory / "raw-response.json").read_text())
    candidate = (directory / "candidate.txt").read_text()
    assert candidate == raw["choices"][0]["message"]["content"]
    assert receipt["result"]["candidate_sha256"] == digest(directory / "candidate.txt")
    session = [json.loads(line) for line in Path(receipt["result"]["session_log"]).read_text().splitlines()]
    assert [m["role"] for m in session] == ["user", "assistant"]
    assert session[-1]["content"] == candidate
    assert receipt["result"]["tools_executed"] == 0
    assert receipt["result"]["agent_loop_executed"] is False
    assert receipt["result"]["host_B"]["verdict"] == "unknown"
    build = summary["build_info"]
    assert build["runner_source_sha256"] == digest(ROOT / "rust/crates/ncx-core/examples/verification_collect.rs")
    assert build["support_source_sha256"] == digest(ROOT / "rust/crates/ncx-core/examples/verification_collect/support.rs")
    assert build["executable_sha256"] == digest(executable)
    before = digest(directory / "receipt.json")
    assert invoke(executable, "offline-receipt-test", directory).returncode != 0
    assert digest(directory / "receipt.json") == before


def test_failed_started_request_is_counted_with_completed_failure_receipt(executable, tmp_path):
    directory = tmp_path / "failure"
    result = invoke(executable, "offline-failure-test", directory)
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["http_calls"] == 0
    receipt = json.loads((directory / "receipt.json").read_text())
    assert receipt["call_status"] == "offline_transport_failure"
    assert not (directory / "raw-response.json").exists()
    with sqlite3.connect(directory / "offline-ledger/main-attempts.sqlite3") as connection:
        records = connection.execute("SELECT group_id,started_json,finished_json FROM attempts").fetchall()
    assert len(records) == 1
    assert records[0][0] == "offline-only"
    assert json.loads(records[0][1])["event"] == "attempt_started"
    assert json.loads(records[0][2])["call_status"] == "offline_transport_failure"


def test_live_without_explicit_flag_never_reads_config_or_attempts(executable, tmp_path):
    result = invoke(executable, "live", tmp_path / "nonexistent")
    assert result.returncode != 0
    assert "usage:" in result.stderr
    assert not list(tmp_path.iterdir())


def test_resource_cohort_preserves_old_binding_and_consumed_groups(executable, tmp_path):
    directory = tmp_path / "resource"
    result = invoke(executable, "offline-resource-test", directory)
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["paid_calls"] == summary["http_calls"] == 0
    assert summary["global_main_attempts"] == 7
    assert summary["preserved_original_attempts"] == 6
    assert summary["new_attempt_ordinal"] == 7
    assert summary["remaining"] == 393
    with sqlite3.connect(directory / "offline-ledger/main-attempts.sqlite3") as connection:
        metadata = dict(connection.execute("SELECT key,value FROM metadata"))
        groups = connection.execute("SELECT ordinal,group_id FROM attempts ORDER BY ordinal").fetchall()
    assert metadata["collection_settings_sha256"] == summary["old_binding_sha256"]
    assert len([key for key in metadata if key.startswith("resource_cohort:")]) == 1
    assert len(groups) == 7
    assert groups[-1] == (7, "new-0")
