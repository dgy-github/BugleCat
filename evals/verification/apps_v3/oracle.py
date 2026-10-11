"""Bounded local subprocess proof for APPS original paired acceptance.

Execution receipts are not source admission, independent-group deduplication,
model labels or threshold certification. First-reference admission is separate.
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from .semantics import CONTRACT, OFFICIAL_COMMIT, OFFICIAL_EVALUATOR_SHA256


def raw_sha(value):
    return hashlib.sha256(value).hexdigest()


def digest(value):
    return raw_sha(json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode())


def source_graph():
    folder = Path(__file__).resolve().parent
    return {"worker": folder / "worker.py", "comparison": folder / "semantics.py",
            "ast_policy": folder.parent / "benchmark_oracle_worker.py",
            "wrapper": folder / "oracle.py", "runtime": Path(sys.executable).resolve()}


def checked_result(result, pair_count):
    """Require actual ordered pair evidence before accepting worker verdicts."""
    invalid = {"status": "unknown", "reason": "invalid_worker_output"}
    if type(result) is not dict or result.get("status") not in {"passed", "failed", "unknown"}:
        return invalid
    status = result["status"]
    pairs = result.get("pair_results", [])
    if type(pairs) is not list:
        return invalid
    for index, item in enumerate(pairs):
        if type(item) is not dict or type(item.get("index")) is not int or item["index"] != index or type(item.get("passed")) is not bool:
            return invalid
    executed = result.get("pairs_executed")
    if status == "unknown":
        if not isinstance(result.get("reason"), str) or not result["reason"]:
            return invalid
        if pairs or executed is not None:
            if type(executed) is not int or type(pair_count) is not int or not len(pairs) <= executed <= pair_count:
                return invalid
        return result
    if result.get("failure_phase") == "compile_or_lookup":
        return result if status == "failed" and type(executed) is int and executed == 0 and not pairs and isinstance(result.get("error_kind"), str) else invalid
    if type(pair_count) is not int or pair_count < 1 or type(executed) is not int or executed != pair_count or len(pairs) != pair_count:
        return invalid
    expected_status = "passed" if all(item["passed"] for item in pairs) else "failed"
    return result if status == expected_status else invalid


def execute(code, input_output, *, timeout=5):
    """Execute one artifact, without credentials, in one persistent namespace."""
    if type(timeout) is not int or not 1 <= timeout <= 10:
        raise ValueError("bounded timeout required")
    if type(code) is not str:
        raise ValueError("code must be original text")
    graph = source_graph()
    before = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    inputs = input_output.get("inputs") if isinstance(input_output, dict) else None
    pair_count = len(inputs) if type(inputs) is list else None
    payload = {"code": code, "input_output": input_output}
    environment = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP") if key in os.environ}
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="ncx-apps-v3-") as work:
        try:
            child = subprocess.run([sys.executable, "-I", "-S", "-B", str(graph["worker"])],
                                   input=json.dumps(payload, allow_nan=False), text=True,
                                   capture_output=True, cwd=work, env=environment,
                                   timeout=timeout, check=False)
            result = json.loads(child.stdout) if child.returncode == 0 else {
                "status": "unknown", "reason": "worker_failed"}
            result = checked_result(result, pair_count)
        except subprocess.TimeoutExpired:
            result = {"status": "unknown", "reason": "worker_timeout", "pairs_executed": None}
        except json.JSONDecodeError:
            result = {"status": "unknown", "reason": "invalid_worker_output"}
    after = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    if after != before:
        raise ValueError("oracle or runtime changed during execution")
    return {"schema": "buglecat.verification.apps-execution-receipt.v3",
            "comparison_contract": CONTRACT, "official_commit": OFFICIAL_COMMIT,
            "official_evaluator_sha256": OFFICIAL_EVALUATOR_SHA256,
            "scope": "restricted explicit top-level FunctionDef call-based original pairs; no aliases/STDIN/class/general correctness",
            "code_sha256": raw_sha(code.encode()), "input_output_sha256": digest(input_output),
            "source_graph_sha256": before, "python_version": sys.version,
            "pair_count": pair_count, "timeout_seconds": timeout,
            "resource_policy": "whole-child deadline; no per-case SIGALRM or OS memory sandbox",
            "elapsed_ms": round((time.monotonic() - started) * 1000), "result": result,
            "credentials_removed": True, "independent_of_jev": True, "paid_calls": 0}
