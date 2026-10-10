"""Build the real example main first; these tests never use an API key/network."""

import hashlib
import json
import os
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "rust/crates/ncx-core/examples/verification_replay.rs"
DEFAULT_BINARY = ROOT / "rust/target/debug/examples/verification_replay.exe"


def sha(data):
    return hashlib.sha256(data).hexdigest()


@pytest.fixture
def binary():
    path = Path(os.environ.get("NCX_REPLAY_BINARY", DEFAULT_BINARY))
    if not path.is_file():
        pytest.skip("Build cargo build -p ncx-core --example verification_replay first")
    return path


def run(binary, *args):
    env = dict(os.environ)
    env["NCX_JEV_API_KEY"] = ""
    return subprocess.run([str(binary), *map(str, args)], env=env, capture_output=True,
                          text=True, timeout=20)


def test_main_binary_emits_actual_body_hash_and_compiled_source_receipt(binary, tmp_path):
    output = tmp_path / "offline.jsonl"
    result = run(binary, "offline-receipt-test", output)
    assert result.returncode == 0, result.stderr
    rows = [json.loads(line) for line in output.read_text().splitlines()]
    assert [row["status"] for row in rows] == [
        "attempt_started", "request_submitted", "attempt_finished"]
    started, submitted, finished = rows
    actual = sha(json.dumps(started["request"], sort_keys=True, ensure_ascii=False,
                           separators=(",", ":")).encode())
    assert submitted["actual_request_hash"] == started["request_hash"] == actual
    assert started["runner_source_sha256"] == sha(SOURCE.read_bytes())
    assert started["runner_exe_sha256"] == sha(binary.read_bytes())
    assert all(row["network_calls"] == row["paid_attempts"] == 0 for row in rows)
    assert finished["response"]["model"] == "offline-receipt-fixture"
    assert finished["response"]["usage"] == {"input_tokens": 0, "output_tokens": 0}
    assert len(finished["response_sha256"]) == 64
    raw_path = Path(finished["raw_response_file"])
    assert submitted["planned_raw_response_file"] == str(raw_path)
    raw_bytes = raw_path.read_bytes()
    assert sha(raw_bytes) == finished["response_sha256"]
    raw = json.loads(raw_bytes)
    assert raw["model"] == finished["response"]["model"]
    assert raw["answers"] == finished["response"]["answers"]
    assert raw["usage"] == finished["response"]["usage"]
    assert run(binary, "offline-receipt-test", output).returncode != 0
    assert raw_path.read_bytes() == raw_bytes


def test_main_live_without_key_creates_no_attempt_or_ledger(binary, tmp_path):
    packet = {"schema": "buglecat.verification.prepared-replay.v1", "id": "offline",
              "group_id": "offline", "eligible_semantic": False, "status": "prepared"}
    packet["artifact_sha256"] = sha(json.dumps(packet, sort_keys=True, ensure_ascii=False,
                                              separators=(",", ":")).encode())
    packets = tmp_path / "packets.jsonl"
    packets.write_text(json.dumps(packet), encoding="utf-8")
    output, ledger = tmp_path / "not-run.jsonl", tmp_path / "ledger"
    result = run(binary, "live", packets, output, "--live", "--max-attempts", "1",
                 "--ledger-dir", ledger)
    assert result.returncode == 0, result.stderr
    row = json.loads(output.read_text())
    assert row["status"] == "not_run"
    assert row["reason"] == "missing_key"
    assert row["attempted_calls"] == 0
    assert not ledger.exists()
