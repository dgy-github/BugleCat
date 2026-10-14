"""Offline source pool census and protocol exclusion audit for claim-level evaluation.

Zero network or model calls. Audits unexposed task capacity against protocol quotas
and verifies that development-exposed pilot tasks and historical ledger attempts
are permanently excluded from formal calibration/holdout admission.
"""
import argparse
import hashlib
import json
import re
import sqlite3
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def runtime_policy_version(root: Path = ROOT) -> str:
    source = (root / "rust/crates/ncx-core/src/verification/types.rs").read_text(encoding="utf-8")
    match = re.search(r'pub const POLICY_VERSION: &str = "(host-rules-v[0-9.]+)";', source)
    if not match:
        raise ValueError("missing runtime POLICY_VERSION in verification/types.rs")
    return match.group(1)


def load_ledger_group_ids(ledger_path: Path) -> set[str]:
    resolved = str(ledger_path.resolve())
    if resolved.startswith("\\\\?\\"):
        resolved = resolved[4:]
    uri = Path(resolved).as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True) as connection:
        return {row[0] for row in connection.execute("SELECT group_id FROM attempts")}


def audit_source_pool(root: Path = ROOT) -> dict:
    protocol_path = root / "evals/verification/claim_production_protocol_v1.json"
    protocol_raw = protocol_path.read_bytes()
    protocol = json.loads(protocol_raw)

    active_policy = runtime_policy_version(root)
    if protocol.get("policy_version") != active_policy:
        raise ValueError(
            f"protocol policy_version {protocol.get('policy_version')} != runtime {active_policy}"
        )
    if protocol.get("production_authorized") is not False or protocol.get("certified_threshold") is not None:
        raise ValueError("protocol must remain uncertified before calibration and holdout")

    admission = protocol.get("source_admission", {})
    if not admission.get("exclude_author_exposed_development_and_synthetic_fixtures"):
        raise ValueError("protocol must exclude author-exposed and fixture tasks")
    if not admission.get("holdout_split_cannibalization_prohibited"):
        raise ValueError("protocol must prohibit cannibalizing holdout split for calibration")

    pilot_rel = admission.get("excluded_pilot_reports", ["evals/verification/reports/claim_pilot_20261013.json"])[0]
    pilot_path = root / pilot_rel
    pilot_raw = pilot_path.read_bytes()
    pilot = json.loads(pilot_raw)
    if pilot.get("formal_eligible") is not False or pilot.get("holdout_started") is not False:
        raise ValueError("pilot report must not be marked formal_eligible or holdout_started")

    pilot_task_ids = [case["task_id"] for case in pilot.get("cases", [])]
    pilot_group_ids = {case["group_id"] for case in pilot.get("cases", [])}
    if not all(case.get("author_exposed") is True for case in pilot.get("cases", [])):
        raise ValueError("all pilot cases must remain permanently marked author_exposed")
    if sorted(pilot_task_ids) != sorted(admission.get("excluded_pilot_task_ids", [])):
        raise ValueError("protocol excluded_pilot_task_ids mismatch with pilot report")

    ledger_rel = admission.get(
        "excluded_attempt_ledgers",
        ["evals/verification/reports/benchmark_400_budget_20261010/main/main-attempts.sqlite3"],
    )[0]
    ledger_used = load_ledger_group_ids(root / ledger_rel)
    excluded_groups = ledger_used | pilot_group_ids

    mbpp_index_path = root / "evals/verification/reports/artifact_expanded_mbpp_catalog_20261011_v2.json"
    mbpp_source_path = root / "evals/verification/data/mbpp/mbpp-expanded-20261011-v2.json"
    mbpp_catalog = json.loads(mbpp_index_path.read_bytes())
    if sha256_bytes(mbpp_source_path.read_bytes()) != mbpp_catalog["source_sha256"]:
        raise ValueError("MBPP source sha256 mismatch with catalog")

    split_counts = Counter()
    unexposed_by_split = {"calibration": [], "holdout": []}
    for row in sorted(mbpp_catalog["tasks"], key=lambda item: item["task_id"]):
        split = row["reserved_split"]
        split_counts[split] += 1
        if row["group_id"] in excluded_groups or row["task_id"] in set(pilot_task_ids):
            continue
        if split in unexposed_by_split:
            unexposed_by_split[split].append(row["group_id"])

    if pilot_group_ids & set(unexposed_by_split["calibration"]):
        raise AssertionError("pilot group leaked into unexposed calibration pool")

    min_groups_per_phase = int(protocol["min_supported"]) + int(protocol["min_unsupported"])
    cal_unexposed_count = len(unexposed_by_split["calibration"])
    hold_unexposed_count = len(unexposed_by_split["holdout"])
    calibration_pool_sufficient = cal_unexposed_count >= min_groups_per_phase

    return {
        "schema": "buglecat.verification.claim-source-pool-audit.v1",
        "policy_version": active_policy,
        "protocol_sha256": sha256_bytes(protocol_raw),
        "pilot_report_sha256": sha256_bytes(pilot_raw),
        "mbpp_catalog_sha256": sha256_bytes(mbpp_index_path.read_bytes()),
        "quotas_per_phase": {
            "min_supported": protocol["min_supported"],
            "min_unsupported": protocol["min_unsupported"],
            "min_total_independent_groups": min_groups_per_phase,
        },
        "exclusions": {
            "ledger_exposed_groups": len(ledger_used),
            "pilot_exposed_groups": len(pilot_group_ids),
            "pilot_exposed_task_ids": sorted(pilot_task_ids),
            "total_excluded_groups": len(excluded_groups),
        },
        "mbpp_expanded_v2": {
            "total_tasks": len(mbpp_catalog["tasks"]),
            "catalog_splits": dict(split_counts),
            "unexposed_calibration_groups": cal_unexposed_count,
            "unexposed_holdout_groups": hold_unexposed_count,
            "pilot_overlap_with_unexposed": 0,
        },
        "readiness": {
            "calibration_pool_sufficient_from_mbpp_alone": calibration_pool_sufficient,
            "calibration_group_deficit_min": max(0, min_groups_per_phase - cal_unexposed_count),
            "new_independent_source_required": not calibration_pool_sufficient,
            "holdout_opened": False,
            "production_authorized": False,
            "paid_calls": 0,
            "blocking_reason": (
                "Existing MBPP calibration split has only 29 unexposed groups after excluding 417 "
                "historical ledger attempts and 8 pilot tasks; holdout split (473 groups) cannot be "
                "cannibalized for calibration. A new independent, unexposed task family is required "
                "before formal calibration collection."
            ),
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=None, help="Optional new JSON path to write report")
    args = parser.parse_args()
    report = audit_source_pool(ROOT)
    if args.output is not None:
        with args.output.open("x", encoding="utf-8") as stream:
            json.dump(report, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
