"""Read-only reconciliation inventory; never infers charges from success files."""
import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HISTORY = ROOT / "reports/.nanocodex/reuse-strong-oracle-20261013"
LEDGER = ROOT / "evals/verification/reports/benchmark_400_budget_20261010/jev"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(history=HISTORY, ledger=LEDGER):
    calls = sorted((history / "jev_calls").glob("*.json"))
    reservations = sorted((ledger / "reservations").glob("*.json"))
    registered_groups = set()
    for path in reservations:
        registered_groups.add(json.loads(path.read_text(encoding="utf-8"))["group_id"])
    rows, sources = [], Counter()
    for path in calls:
        value = json.loads(path.read_text(encoding="utf-8"))
        source = value.get("source", "legacy_direct_http")
        sources[source] += 1
        rows.append({"candidate_id": path.stem, "sha256": sha(path), "source": source,
                     "saved_ok": value.get("ok") is True,
                     "exact_candidate_group_in_ledger": path.stem in registered_groups})
    original = history / "certification_report.json"
    old = json.loads(original.read_text(encoding="utf-8")) if original.exists() else {}
    return {
        "schema": "buglecat.verification.historical-accounting-audit.v1",
        "status": "unreconciled_requires_provider_attempt_or_billing_export",
        "saved_response_files": len(rows), "sources": dict(sources),
        "shared_ledger_reservations": len(reservations),
        "legacy_report_sha256": sha(original) if original.exists() else None,
        "legacy_report_tasks_to_call": old.get("calls_made"),
        "actual_network_attempts": None, "failed_network_attempts": None,
        "automatic_retry_attempts": None, "charged_amount": None,
        "new_paid_calls": 0, "ledger_modified": False,
        "limitations": [
            "Legacy direct HTTP wrote only successful final responses; intermediate attempts were not retained.",
            "A saved response proves a retained result, not the number of HTTP attempts or billable requests.",
            "Candidate group name matching alone does not authenticate a reservation/request binding.",
            "Provider request/billing export is needed to resolve retries, failed charges and total spend.",
        ],
        "responses": rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = audit()
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2, ensure_ascii=False)
        stream.write("\n")
    print(json.dumps({k: v for k, v in result.items() if k not in {"responses", "limitations"}}, indent=2))


if __name__ == "__main__":
    main()
