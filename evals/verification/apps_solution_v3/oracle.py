"""Bounded credential-free child for independently reviewed Solution scope."""
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from ..apps_v3.oracle import checked_result, digest, raw_sha
from ..apps_v3.semantics import CONTRACT, OFFICIAL_COMMIT, OFFICIAL_EVALUATOR_SHA256


def source_graph():
    folder = Path(__file__).resolve().parent
    return {"solution_worker": folder / "worker.py", "solution_policy": folder / "policy.py",
            "solution_wrapper": folder / "oracle.py", "pair_worker": folder.parent / "apps_v3/worker.py",
            "comparison": folder.parent / "apps_v3/semantics.py", "pair_wrapper": folder.parent / "apps_v3/oracle.py",
            "typing_policy": folder.parent / "humaneval_oracle_worker.py",
            "ast_policy": folder.parent / "benchmark_oracle_worker.py", "runtime": Path(sys.executable).resolve()}


def execute(code, input_output, *, timeout=5):
    """Execute original code once, same instance for every original pair."""
    if type(timeout) is not int or not 1 <= timeout <= 10:
        raise ValueError("bounded timeout required")
    if type(code) is not str:
        raise ValueError("code must be original text")
    graph = source_graph()
    before = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    inputs = input_output.get("inputs") if isinstance(input_output, dict) else None
    pair_count = len(inputs) if type(inputs) is list else None
    environment = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP") if key in os.environ}
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="ncx-apps-solution-v3-") as work:
        try:
            child = subprocess.run([sys.executable, "-I", "-S", "-B", str(graph["solution_worker"])],
                                   input=json.dumps({"code": code, "input_output": input_output}, allow_nan=False),
                                   text=True, capture_output=True, cwd=work, env=environment,
                                   timeout=timeout, check=False)
            result = json.loads(child.stdout) if child.returncode == 0 else {"status": "unknown", "reason": "worker_failed"}
            result = checked_result(result, pair_count)
        except subprocess.TimeoutExpired:
            result = {"status": "unknown", "reason": "worker_timeout", "pairs_executed": None}
        except json.JSONDecodeError:
            result = {"status": "unknown", "reason": "invalid_worker_output"}
    after = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    if after != before:
        raise ValueError("oracle or runtime changed during execution")
    return {"schema": "buglecat.verification.apps-solution-execution-receipt.v3",
            "comparison_contract": CONTRACT, "official_commit": OFFICIAL_COMMIT,
            "official_evaluator_sha256": OFFICIAL_EVALUATOR_SHA256,
            "scope": "restricted unique top-level Solution instance methods; original ordered call-based pairs; no inheritance/decorators/STDIN/general correctness",
            "code_sha256": raw_sha(code.encode()), "input_output_sha256": digest(input_output),
            "source_graph_sha256": before, "python_version": sys.version,
            "pair_count": pair_count, "timeout_seconds": timeout,
            "resource_policy": "whole-child deadline; no per-case SIGALRM or OS memory sandbox",
            "elapsed_ms": round((time.monotonic() - started) * 1000), "result": result,
            "credentials_removed": True, "independent_of_jev": True, "paid_calls": 0}
