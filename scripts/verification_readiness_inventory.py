"""Offline metadata inventory. Neither thread counts nor usage totals certify results."""
import argparse
import csv
import hashlib
import json
import sqlite3
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def provider_summary(path):
    raw = path.read_bytes()
    rows = list(csv.DictReader(raw.decode("utf-8-sig").splitlines()))
    required = {"day", "requests", "input_tokens", "output_tokens"}
    if not rows or not required.issubset(rows[0]):
        raise ValueError("missing provider aggregate columns")
    totals = Counter()
    safe_rows = []
    for row in rows:
        safe = {"bucket": row["day"]}
        for field in required - {"day"}:
            value = int(row[field])
            if value < 0:
                raise ValueError("negative aggregate")
            safe[field] = value
            totals[field] += value
        safe_rows.append(safe)
    return {"sha256": digest(raw), "rows": safe_rows, "totals": dict(totals),
            "per_request_reconciled": False, "failed_billable_requests": None,
            "automatic_retries": None,
            "limitation": "Aggregate export has no authenticated per-request join; totals cannot prove retry or failure charges."}


def thread_inventory(path):
    raw = path.read_bytes()
    threads = json.loads(raw)["threads"]
    counts = Counter()
    rows = []
    for identity, thread in threads.items():
        metadata = thread.get("metadata", {})
        workspace = metadata.get("workspace", "")
        title = metadata.get("title", "")
        marker = (workspace + " " + title).lower()
        fixture = any(word in marker for word in
                      ("e2e", "selftest", "self-test", "self-repair", "fixture", "smoke", "pytest"))
        items = [item for turn in thread.get("turns", []) for item in turn.get("items", [])]
        kinds = Counter(item.get("type", "unknown") for item in items)
        candidate = not fixture and kinds["assistantMessage"] > 0 and kinds["toolResult"] > 0
        category = "fixture_marker" if fixture else "unreviewed_candidate" if candidate else "missing_answer_or_tool"
        counts[category] += 1
        rows.append({"thread_hash": digest(identity.encode()), "workspace_hash": digest(workspace.encode()),
                     "category": category, "turns": len(thread.get("turns", [])),
                     "assistant_messages": kinds["assistantMessage"], "tool_results": kinds["toolResult"]})
    return {"source_sha256": digest(raw), "threads": len(threads), "counts": dict(counts),
            "certification_eligible_groups": None, "rows": rows,
            "limitation": "Metadata filter only. Non-fixture candidates still require provenance, exposure, task independence, evidence and labels; no holdout opened."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider-csv", type=Path, required=True)
    parser.add_argument("--thread-store", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    provider = provider_summary(args.provider_csv)
    inventory = thread_inventory(args.thread_store)
    history = ROOT / "reports/.nanocodex/reuse-strong-oracle-20261013/jev_calls"
    retained = Counter()
    for path in history.glob("*.json"):
        value = json.loads(path.read_bytes())
        if value.get("source") or value.get("ok") is not True:
            continue
        retained["responses"] += 1
        for field, value in value.get("raw", {}).get("usage", {}).items():
            if field in {"input_tokens", "output_tokens"}:
                retained[field] += value
    ledger = ROOT / "evals/verification/reports/benchmark_400_budget_20261010/main/main-attempts.sqlite3"
    with sqlite3.connect(ledger.as_uri() + "?mode=ro", uri=True) as connection:
        main_started, unfinished = connection.execute(
            "SELECT COUNT(*), SUM(finished_json IS NULL) FROM attempts").fetchone()
    result = {"schema": "buglecat.verification.readiness-inventory.v1", "provider": provider,
              "threads": inventory, "legacy_direct_retained_usage": dict(retained),
              "main_reserved_attempts": main_started, "main_unfinished_attempts": unfinished,
              "paid_calls_this_run": 0, "certified": False,
              "budget_available": None,
              "budget_limitation": "Historical unreserved attempts and campaign scope must be resolved before reusing authority."}
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
    print(json.dumps({"provider_totals": provider["totals"], "thread_counts": inventory["counts"],
                      "legacy_direct_retained_usage": dict(retained), "main_reserved": main_started,
                      "main_unfinished": unfinished, "paid_calls": 0}))


if __name__ == "__main__":
    main()
