"""Read global paid attempts, including failed response usage, without transport."""

import argparse
import hashlib
import json
import sqlite3
from pathlib import Path

from .calibration import create, load


def status(authorization, directories):
    auth = load(authorization)
    ledger = Path(auth["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        metadata = dict(connection.execute("SELECT key,value FROM metadata"))
        attempts = list(connection.execute("SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal"))
    receipts, response_usage = [], []
    for directory in directories:
        for path in Path(directory).glob("*/receipt.json"):
            receipt = load(path)
            receipts.append({"id": receipt["id"], "status": receipt["call_status"],
                             "attempt_ordinal": receipt["attempt_ordinal"],
                             "receipt_sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
            raw_path = path.with_name("raw-response.json")
            if raw_path.exists():
                raw = load(raw_path)
                response_usage.append({"id": receipt["id"], "status": receipt["call_status"],
                                       "finish_reason": raw["choices"][0]["finish_reason"], "usage": raw.get("usage", {})})
    return {"schema": "buglecat.verification.collection-budget-status.v1",
            "global_main_attempts": len(attempts), "remaining_main_attempts": 400 - len(attempts),
            "global_main_finished": sum(finished is not None for _, _, _, finished in attempts),
            "main_metadata": metadata, "raw_2xx_responses": len(response_usage),
            "receipt_statuses": receipts, "usage_including_truncated_failures": response_usage,
            "usage_totals": {key: sum(r["usage"].get(key, 0) for r in response_usage)
                             for key in ("prompt_tokens", "completion_tokens", "total_tokens")},
            "missing_usage_is_not_zero_cost": True, "amount": None}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--authorization", type=Path, required=True)
    parser.add_argument("--directory", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    value = status(args.authorization, args.directory)
    create(args.output, value)
    print(json.dumps({k: value[k] for k in ("global_main_attempts", "remaining_main_attempts", "raw_2xx_responses", "usage_totals")}))


if __name__ == "__main__":
    main()
