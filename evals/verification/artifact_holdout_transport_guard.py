"""Read-only transport gate; qualification is reconstructed from original receipts.

This module never registers a cohort, reserves a slot, reads a key or sends HTTP.
Its proof grants permission only to the independently compiled holdout entry.
"""

import argparse
import json
from pathlib import Path

from . import artifact_cohort, artifact_holdout
from .artifact_formal_intake import PROFILE_FIELDS, verify_dataset_cases, verify_prepared
from .artifact_gate import protocol
from .artifact_scoring import sealed
from .calibration import digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction, verify_manifest

SCHEMA = "buglecat.verification.artifact-holdout-transport-proof.v1"


def _proof(registered, cohort_file, selected, **extra):
    body = {"schema": SCHEMA, "split": "holdout", "qualified_original_calibration": True,
            "artifact_cohort_file": str(Path(cohort_file).resolve()),
            "artifact_cohort_sha256": file_hash(cohort_file),
            "artifact_cohort_hash": registered["artifact_hash"],
            "holdout_lock": registered["holdout_lock"],
            "selected_ids": [case["id"] for case in selected],
            "selected_groups": [case["group_id"] for case in selected],
            **{field: registered[field] for field in PROFILE_FIELDS},
            "production_authorized": False, "threshold_retuning_allowed": False,
            "paid_calls": 0, "http_calls": 0, **extra}
    return {**body, "artifact_hash": digest(body)}


def _bound_manifest(manifest, registered):
    require(manifest["schema"] == "buglecat.verification.mbpp-collection.v1",
            "original collection manifest required")
    require(manifest["artifact_cohort_sha256"] == file_hash(manifest["artifact_cohort_file"])
            and manifest["artifact_cohort_hash"] == registered["artifact_hash"],
            "holdout cohort binding changed")
    roles = {entry["role"]: entry for entry in registered["bindings"]}
    for role in ("source", "catalog", "authorization", "protocol", "main_resource_profile"):
        require(Path(manifest[f"{role}_file"]).resolve() == Path(roles[role]["file"]).resolve()
                and manifest[f"{role}_sha256"] == roles[role]["sha256"]
                and file_hash(manifest[f"{role}_file"]) == roles[role]["sha256"],
                "holdout manifest source binding changed")
    require(all(manifest[field] == registered[field] for field in PROFILE_FIELDS),
            "holdout profile changed")
    settings = registered["main_collection_settings"]
    require(all(manifest[key] == value for key, value in settings.items()),
            "holdout main settings changed")
    # Shared source owner reconstructs grouping and exact public signatures.
    # Only the expensive immutable catalog proof is reused in this transaction.
    verify_manifest(manifest)


@source_transaction
def main_proof(manifest_file, selected_id=None):
    before = file_hash(manifest_file)
    manifest = load(manifest_file)
    registered = artifact_holdout.validate(manifest["artifact_cohort_file"])
    require(not artifact_holdout._marker(registered, ".labels.started").exists(),
            "holdout collection closed after label registration")
    _bound_manifest(manifest, registered)
    cases = manifest["cases"]
    require(1 <= len(cases) <= 8 and len({case["id"] for case in cases}) == len(cases)
            and len({case["group_id"] for case in cases}) == len(cases)
            and all(case in registered["cases"] and case["split"] == "holdout" for case in cases),
            "fixed fresh holdout batch required")
    snapshot, _ = artifact_cohort.ledger_snapshot(manifest["authorization_file"])
    consumed = set(snapshot["excluded_groups"])
    pending = [case for case in registered["cases"] if case["group_id"] not in consumed]
    if selected_id is None:
        selected = cases
        require(cases == pending[:len(cases)], "next fixed unstarted holdout prefix required")
    else:
        selected = [case for case in cases if case["id"] == selected_id]
        require(len(selected) == 1 and pending[:1] == selected,
                "selected holdout group already consumed or out of order")
        index = cases.index(selected[0])
        require(all(case["group_id"] in consumed for case in cases[:index]),
                "earlier batch member has not started")
    require(file_hash(manifest_file) == before
            and artifact_holdout.validate(manifest["artifact_cohort_file"]) == registered,
            "holdout gate changed during validation")
    return _proof(registered, manifest["artifact_cohort_file"], selected,
                  kind="main", manifest_sha256=before)


@source_transaction
def jev_proof(prepared_file, selected_id, registration_file):
    registration_raw = file_hash(registration_file)
    prepared_raw = file_hash(prepared_file)
    registration, data, rows = artifact_holdout._label_registration(registration_file)
    registered = artifact_holdout.validate(registration["artifact_cohort_file"])
    require(not artifact_holdout._marker(registered, ".evaluation.started").exists(),
            "holdout evaluation already consumed")
    require(Path(registration["prepared_file"]).resolve() == Path(prepared_file).resolve()
            and prepared_raw == registration["prepared_file_sha256"]
            and read_lines(prepared_file) == rows, "original registered holdout requests required")
    verify_dataset_cases(data)
    cases = data["cases"]
    source = {case["id"]: case for case in registered["cases"]}
    require(len(source) == len(cases) and {case["id"] for case in cases} == set(source)
            and len({case["group_id"] for case in cases}) == len(cases),
            "all holdout source denominators required")
    counts = {label: sum(case["label"] == label for case in cases)
              for label in ("supported", "unsupported", "unknown")}
    spec = protocol(registration["protocol"])
    require(counts == registration["label_counts"] and counts["supported"] >= spec["min_supported"]
            and counts["unsupported"] >= spec["min_unsupported"], "independent holdout label quotas required")
    eligible = {case["id"] for case in cases if case["call_eligible"] is True}
    require(len(rows) == len({row["id"] for row in rows}) and eligible == {row["id"] for row in rows}
            and registration["case_ids"] == sorted(eligible), "holdout eligible set changed")
    by_id = {case["id"]: case for case in cases}
    for case in cases:
        require(case["group_id"] == source[case["id"]]["group_id"] and case["split"] == "holdout"
                and case["author_exposed"] is False
                and case["call_eligible"] is (case["label"] in {"supported", "unsupported"}),
                "unknown cannot be hidden or promoted")
    for row in rows:
        sealed(row, "artifact_sha256")
        verify_prepared(row)
        case = by_id[row["id"]]
        oracle = sealed(case["oracle"])
        require(row["split"] == "holdout" and row["group_id"] == case["group_id"]
                and case["candidate_sha256"] == row["candidate_sha256"]
                and case["main_receipt_sha256"] == row["main_receipt_sha256"]
                and oracle["artifact_hash"] == row["oracle_sha256"]
                and case["label"] == {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"]),
                "holdout original Oracle label changed")
        require(all(row[field] == registered[field] == registration[field] == data[field]
                    for field in PROFILE_FIELDS), "holdout registered profile changed")
    selected = [row for row in rows if row["id"] == selected_id]
    require(len(selected) == 1, "registered holdout row required")
    require(file_hash(registration_file) == registration_raw and file_hash(prepared_file) == prepared_raw
            and artifact_holdout._label_registration(registration_file) == (registration, data, rows),
            "holdout registration changed during validation")
    return _proof(registered, registration["artifact_cohort_file"], selected, kind="jev",
                  formal_registration_sha256=registration_raw,
                  formal_registration_hash=registration["artifact_hash"], prepared_sha256=prepared_raw)


def cli():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    main = sub.add_parser("main")
    main.add_argument("manifest")
    main.add_argument("selected_id", nargs="?")
    jev = sub.add_parser("jev")
    jev.add_argument("prepared")
    jev.add_argument("id")
    jev.add_argument("registration")
    args = parser.parse_args()
    try:
        result = (main_proof(args.manifest, args.selected_id) if args.command == "main"
                  else jev_proof(args.prepared, args.id, args.registration))
    except Exception:
        parser.exit(2, "holdout_transport_gate_rejected\n")
    print(json.dumps(result, sort_keys=True, separators=(",", ":"), ensure_ascii=False))


if __name__ == "__main__":
    cli()
