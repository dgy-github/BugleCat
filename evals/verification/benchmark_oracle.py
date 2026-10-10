"""Run frozen benchmark assertions separately from models and their confidence.

Only the explicitly scoped frozen acceptance suite can receive a binary fact
label. The result is not a proof of full algorithm correctness.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from .calibration import create, digest, load, require


def audit(task, candidate_code, timeout=5):
    require(type(timeout) is int and 1 <= timeout <= 10, "bounded timeout required")
    worker = Path(__file__).with_name("benchmark_oracle_worker.py").resolve()
    worker_bytes = worker.read_bytes()
    payload = {"reference": task["code"], "imports": task["test_imports"],
               "tests": task["test_list"], "candidate": candidate_code}
    environment = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP") if key in os.environ}
    with tempfile.TemporaryDirectory(prefix="ncx-mbpp-oracle-") as work:
        try:
            child = subprocess.run([sys.executable, "-I", "-S", "-B", str(worker)],
                                   input=json.dumps(payload), text=True, capture_output=True,
                                   cwd=work, env=environment, timeout=timeout, check=False)
            result = json.loads(child.stdout) if child.returncode == 0 else {"status": "unknown", "reason": "worker_failed"}
        except (subprocess.TimeoutExpired, json.JSONDecodeError):
            result = {"status": "unknown", "reason": "worker_timeout_or_invalid_output"}
    require(worker.read_bytes() == worker_bytes, "oracle changed during execution")
    return {"schema": "buglecat.verification.benchmark-assertion-receipt.v1",
            "task_id": task["task_id"], "task_sha256": digest(task),
            "candidate_code_sha256": hashlib.sha256(candidate_code.encode()).hexdigest(),
            "oracle_worker_sha256": hashlib.sha256(worker_bytes).hexdigest(),
            "oracle_scope": "frozen MBPP acceptance tests only; not full task semantics",
            "test_count": len(task["test_list"]), "result": result,
            "independent_of_jev": True, "secrets_removed": True, "paid_attempts": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--task-id", type=int, required=True)
    parser.add_argument("--candidate-code", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    tasks = load(args.source)
    task = next(t for t in tasks if t["task_id"] == args.task_id)
    value = audit(task, args.candidate_code.read_text(encoding="utf-8"))
    create(args.output, {**value, "artifact_hash": digest(value)})
    print(json.dumps({"task_id": args.task_id, "result": value["result"], "paid_attempts": 0}))


if __name__ == "__main__":
    main()
