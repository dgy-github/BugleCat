"""Verify frozen original Jev response bytes against the recorded transport join."""

import argparse
import hashlib
import json
from pathlib import Path

from .annotation_audit import within_root
from .calibration import create, digest, load, require
from .collection_receipts import read_lines


def verify(started, submitted, finished, request, ledger_root, executable, runner_source):
    require(started["status"] == "attempt_started" and submitted["status"] == "request_submitted"
            and finished["status"] == "attempt_finished", "incomplete original transport join")
    request_hash = digest(request)
    require(started["request_hash"] == submitted["request_hash"]
            == submitted["actual_request_hash"] == request_hash, "original submitted body mismatch")
    for field, path in (("runner_exe_sha256", executable), ("runner_source_sha256", runner_source)):
        actual = hashlib.sha256(Path(path).read_bytes()).hexdigest()
        require(started[field] == submitted[field] == actual, "compiled transport attestation mismatch")
    require(finished["call_status"] == "ok", "not a successful original transport response")
    raw_path = Path(finished["raw_response_file"]).resolve()
    require(within_root(raw_path, ledger_root), "response file outside bound ledger")
    require(Path(submitted["planned_raw_response_file"]).resolve() == raw_path, "response path changed")
    require(hashlib.sha256(raw_path.read_bytes()).hexdigest() == finished["response_sha256"], "original response bytes mismatch")
    raw = load(raw_path)
    response = finished["response"]
    require(raw["model"] == response["model"], "actual model mismatch")
    require(raw["answers"] == response["answers"], "parsed answers do not match original transport")
    for key in ("input_tokens", "output_tokens"):
        require(raw["usage"][key] == response["usage"][key], "actual usage mismatch")
    return {"status": "original_transport_bytes_verified", "request_hash": request_hash,
            "actual_request_hash": submitted["actual_request_hash"],
            "response_sha256": finished["response_sha256"], "model": raw["model"],
            "raw_response_file": str(raw_path), "raw_response_verified": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("events", "ledger-root", "executable", "runner-source", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    rows = read_lines(args.events)
    require(len(rows) == 3 and rows[0].get("offline") is True, "CLI diagnostic accepts one offline receipt only")
    value = verify(*rows, rows[0]["request"], args.ledger_root, args.executable, args.runner_source)
    value.update(offline=True, eligible_for_formal=False, paid_attempts=0)
    create(args.output, value)
    print(json.dumps({"status": value["status"], "eligible_for_formal": False, "paid_attempts": 0}))


if __name__ == "__main__":
    main()
