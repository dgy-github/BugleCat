"""Zero-network, once-only holdout gate for the separate artifact track.

This freezes fresh sources only after original calibration receipts qualify.
It does not send requests, reserve paid slots, enable the existing calibration
collectors, retune a threshold or grant production permission.
"""

import argparse
import json
from pathlib import Path

from . import artifact_cohort
from .artifact_formal_intake import PROFILE_FIELDS, cohort, verify_dataset_cases, verify_prepared
from .artifact_gate import protocol, score
from .artifact_scoring import sealed
from .calibration import digest, load, require
from .collection_manifest import file_hash, interfaces
from .collection_receipts import read_lines
from .paid_budget import durable_new
from .source_bindings import source_transaction

LOCK_SCHEMA = "buglecat.verification.artifact-holdout-lock.v1"
REG_SCHEMA = "buglecat.verification.artifact-holdout-registration.v1"
RESULT_SCHEMA = "buglecat.verification.artifact-holdout-evaluation.v1"


@source_transaction
def calibration(path):
    # The shared owner verifies original bytes, compiled runners and statistics.
    # Import lazily so unavailable/incomplete upstream support fails closed.
    from .artifact_formal import revalidate_calibration
    value, registration, data, rows, observations = revalidate_calibration(path)
    require(value["calibration_numeric_gate_met"] is True
            and value["qualified_for_holdout_threshold"] in protocol(registration["protocol"])["threshold_grid"]
            and value["certified_threshold"] is None and value["production_authorized"] is False
            and value["holdout_status"] == "not_run", "qualified original calibration required")
    return value, registration, data, rows, observations


def _bound(role, path):
    return artifact_cohort.binding(role, path)


def _check_bindings(bindings):
    require(all(file_hash(item["file"]) == item["sha256"] for item in bindings),
            "holdout bound file changed")


def _preserve_ledger(before, after):
    """Earlier starts/reservations and original metadata cannot be deleted."""
    require(Path(before["main_ledger_file"]).resolve() == Path(after["main_ledger_file"]).resolve(),
            "shared ledger root changed")
    for key in ("main_started", "main_reservations", "jev_reservations"):
        current = {row["group_id"]: row for row in after[key]}
        require(all(current.get(row["group_id"]) == row for row in before[key]),
                "old start or reservation reset")
    current_metadata = dict(after["main_metadata"])
    require(all(current_metadata.get(key) == value for key, value in before["main_metadata"]),
            "original shared metadata reset")
    require(set(before["excluded_groups"]) <= set(after["excluded_groups"]), "prior consumed group restored")


def _gate_path(snapshot, calibration_hash):
    # Same global ledger + content seal: copying the input/output cannot reset it.
    return Path(snapshot["main_ledger_file"]).parent / "holdout-gates" / calibration_hash


def _marker(registered, suffix):
    lock = registered["holdout_lock"]
    return _gate_path(registered["ledger_snapshot"], lock["calibration_hash"]).with_suffix(suffix)


@source_transaction
def register_source(calibration_file, cohort_id, output):
    """Freeze source-order holdout before any new main candidate or Jev score.

    A qualification grants only one attempt to create a holdout cohort. A crash
    after its marker cannot silently reopen it under another output filename.
    """
    require(not Path(output).exists(), "new holdout cohort output required")
    require(isinstance(cohort_id, str) and bool(cohort_id.strip()), "missing holdout cohort ID")
    value, registration, _, _, _ = calibration(calibration_file)
    original = cohort(registration["artifact_cohort_file"])
    require(original.get("fixture_only") is False and original["holdout_opened"] is False,
            "fresh original calibration cohort required")
    roles = {item["role"]: item["file"] for item in original["bindings"]}
    source, catalog_file, authorization, protocol_file = [roles[k] for k in
                                                         ("source", "catalog", "authorization", "protocol")]
    index = load(catalog_file)
    artifact_cohort.protocol_gate(load(protocol_file))
    require(load(protocol_file) == registration["protocol"], "calibration/source protocol changed")
    external = artifact_cohort.external_bindings(source, index, False)
    # Reuse exact catalog reconstruction, not merely a caller-provided seal.
    inventory = artifact_cohort.artifact_source_registry.inventory(source, catalog_file, authorization, protocol_file)
    snapshot, reservation_bindings = artifact_cohort.ledger_snapshot(authorization)
    _preserve_ledger(original["ledger_snapshot"], snapshot)
    # Every allocation member is excluded, including uncollected/Unknown cases.
    calibration_groups = {case["group_id"] for case in original["cases"]}
    require(len(calibration_groups) == len(original["cases"])
            and set(value["calibration_groups"]) <= calibration_groups,
            "calibration group allocation mismatch")
    excluded = calibration_groups | set(snapshot["excluded_groups"])
    fresh = {}
    for task in sorted(index["tasks"], key=lambda item: item["task_id"]):
        if task["reserved_split"] == "holdout" and task["group_id"] not in excluded:
            fresh.setdefault(task["group_id"], task)
    allocation = min(len(fresh), snapshot["remaining_main_attempts"], snapshot["remaining_jev_reservations"])
    require(allocation >= 200, "insufficient fresh holdout sources or shared budget")
    tasks = {task["task_id"]: task for task in load(source)}
    ordered = sorted(fresh, key=lambda group: digest({"seed": index["split_seed"], "group": group}))
    cases = []
    for group in ordered[:allocation]:
        row = fresh[group]
        public = interfaces(tasks[row["task_id"]]["code"])
        cases.append({"id": f"mbpp-main-{row['task_id']}", "group_id": group, "task_id": row["task_id"],
                      "split": "holdout", "source_sha256": index["source_sha256"],
                      "task_sha256": row["task_sha256"], "public_interface": public,
                      "public_interface_sha256": digest(public), "category": "artifact_acceptance",
                      "label": "unlabeled", "source_kind": "external_mbpp_fresh_collection",
                      "author_exposed": False, "main_model_candidate": "not_collected"})
    bindings = [_bound("calibration", calibration_file), _bound("calibration_registration", value["registration_file"]),
                _bound("calibration_observations", value["observations_file"]),
                _bound("calibration_cohort", registration["artifact_cohort_file"]), _bound("holdout_gate_source", __file__)]
    bindings += original["bindings"] + external + reservation_bindings
    lock = {"schema": LOCK_SCHEMA, "calibration_file": str(Path(calibration_file).resolve()),
            "calibration_sha256": file_hash(calibration_file), "calibration_hash": value["artifact_hash"],
            "registration_sha256": value["registration_file_sha256"], "registration_hash": registration["artifact_hash"],
            "observations_sha256": value["observations_file_sha256"], "observations_hash": value["observations_hash"],
            "threshold": value["qualified_for_holdout_threshold"], "protocol_hash": digest(registration["protocol"]),
            "calibration_allocation_groups": sorted(calibration_groups), "threshold_retuning_allowed": False,
            "production_authorized": False}
    lock = {**lock, "artifact_hash": digest(lock)}
    body = {"schema": artifact_cohort.SCHEMA, "cohort_id": cohort_id,
            "registration_kind": "collection_preregistration", "status": "registered", "split": "holdout",
            "formal_registration": True, "fixture_only": False, "full_campaign_source_gate": True,
            "before_main_requests": True, "scores_hidden": True, "jev_probabilities_read": False,
            "author_exposed": False, "holdout_opened": True, "holdout_cases_emitted": True,
            "labels_assigned": False, "negative_group_count_unknown": True,
            "main_collection_settings": original["main_collection_settings"],
            **{field: original[field] for field in PROFILE_FIELDS if field in original},
            "main_system_prompt_sha256": original["main_system_prompt_sha256"],
            "holdout_max_groups": allocation, "cases": cases, "bindings": bindings,
            "excluded_started_or_reserved_groups": sorted(excluded), "ledger_snapshot": snapshot,
            "holdout_lock": lock, "collection_authorized": False,
            "collection_authorization_status": "new_holdout_transport_entry_required",
            "certified_threshold": None, "production_authorized": False, "new_paid_calls": 0}
    result = {**body, "artifact_hash": digest(body)}
    require(artifact_cohort.ledger_snapshot(authorization) == (snapshot, reservation_bindings),
            "shared ledger changed while freezing holdout")
    require(artifact_cohort.artifact_source_registry.inventory(source, catalog_file, authorization, protocol_file) == inventory,
            "source inventory changed while freezing holdout")
    _check_bindings(bindings)
    require(calibration(calibration_file)[0] == value, "calibration changed while freezing holdout")
    marker = _marker(result, ".opened")
    marker.parent.mkdir(exist_ok=True)
    durable_new(marker, {"schema": "buglecat.verification.artifact-holdout-opened.v1",
                         "calibration_sha256": lock["calibration_sha256"],
                         "cohort_file": str(Path(output).resolve()), "cohort_hash": result["artifact_hash"],
                         "no_retry": True})
    durable_new(output, result)
    return result


@source_transaction
def validate(path):
    registered = cohort(path, "holdout")
    require(registered.get("fixture_only") is False and registered["holdout_opened"] is True,
            "original formal holdout cohort required")
    lock = sealed(registered["holdout_lock"])
    require(lock["schema"] == LOCK_SCHEMA and lock["threshold_retuning_allowed"] is False
            and lock["production_authorized"] is False, "locked holdout threshold required")
    value, registration, _, _, _ = calibration(lock["calibration_file"])
    require(value["artifact_hash"] == lock["calibration_hash"]
            and file_hash(lock["calibration_file"]) == lock["calibration_sha256"]
            and value["qualified_for_holdout_threshold"] == lock["threshold"]
            and value["registration_file_sha256"] == lock["registration_sha256"]
            and registration["artifact_hash"] == lock["registration_hash"]
            and value["observations_file_sha256"] == lock["observations_sha256"]
            and value["observations_hash"] == lock["observations_hash"]
            and digest(registration["protocol"]) == lock["protocol_hash"], "original holdout lock changed")
    original = cohort(registration["artifact_cohort_file"])
    require(lock["calibration_allocation_groups"] == sorted(case["group_id"] for case in original["cases"]),
            "full calibration allocation changed")
    authorization = next(item["file"] for item in registered["bindings"] if item["role"] == "authorization")
    current_snapshot, _ = artifact_cohort.ledger_snapshot(authorization)
    _preserve_ledger(registered["ledger_snapshot"], current_snapshot)
    opened = load(_marker(registered, ".opened"))
    require(Path(opened["cohort_file"]).resolve() == Path(path).resolve()
            and opened["cohort_hash"] == registered["artifact_hash"] and opened["no_retry"] is True,
            "holdout reopening or alternate cohort forbidden")
    cases = registered["cases"]
    groups = {case["group_id"] for case in cases}
    require(len(groups) == len(cases) and len({case["id"] for case in cases}) == len(cases)
            and all(case["split"] == "holdout" for case in cases)
            and groups.isdisjoint(lock["calibration_allocation_groups"])
            and groups.isdisjoint(registered["excluded_started_or_reserved_groups"]),
            "holdout/calibration or prior attempt group overlap")
    return registered


@source_transaction
def register_labels(cohort_file, dataset_file, prepared_file, output):
    """Freeze independent Oracle labels before any holdout Jev probability."""
    require(not Path(output).exists(), "new holdout label registration output required")
    registered = validate(cohort_file)
    # One label registration is tied to the original cohort, not output filename.
    marker = _marker(registered, ".labels.started")
    require(not marker.exists(), "holdout labels already frozen")
    data = sealed(load(dataset_file))
    verify_dataset_cases(data)
    require(data["schema"] == "buglecat.verification.artifact-formal-dataset.v1"
            and data["scores_hidden"] is True and data["jev_probabilities_read"] is False,
            "holdout labels must precede probabilities")
    require(Path(data["artifact_cohort_file"]).resolve() == Path(cohort_file).resolve()
            and data["artifact_cohort_sha256"] == file_hash(cohort_file)
            and data["artifact_cohort_hash"] == registered["artifact_hash"], "holdout dataset source changed")
    cases = {case["id"]: case for case in data["cases"]}
    rows = read_lines(prepared_file)
    source_cases = {case["id"]: case for case in registered["cases"]}
    require(len(cases) == len(data["cases"]) and len({c["group_id"] for c in cases.values()}) == len(cases)
            and set(cases) == set(source_cases), "all allocated holdout cases must remain in denominators")
    require(len({row["id"] for row in rows}) == len(rows)
            and {row["id"] for row in rows} == {c["id"] for c in cases.values() if c["call_eligible"] is True},
            "holdout eligible set changed")
    for case in cases.values():
        require(case["split"] == "holdout" and case["group_id"] == source_cases[case["id"]]["group_id"]
                and case["author_exposed"] is False and case["label"] in {"supported", "unsupported", "unknown"},
                "fresh holdout label required")
        require(case["call_eligible"] is (case["label"] in {"supported", "unsupported"})
                and case["source_kind"] == "source_preregistered_original_model_code",
                "unknown cannot become binary or hide an eligible label")
    for row in rows:
        sealed(row, "artifact_sha256")
        verify_prepared(row)
        case = cases[row["id"]]
        require(row["split"] == "holdout" and row["group_id"] == case["group_id"]
                and row["artifact_cohort_hash"] == registered["artifact_hash"]
                and row["candidate_sha256"] == case["candidate_sha256"]
                and row["main_receipt_sha256"] == case["main_receipt_sha256"], "holdout candidate changed")
        oracle = sealed(case["oracle"])
        require(oracle["artifact_hash"] == row["oracle_sha256"]
                and case["label"] == {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"]),
                "holdout label cannot override original Oracle")
    spec_file = next(item["file"] for item in registered["bindings"] if item["role"] == "protocol")
    spec = protocol(load(spec_file))
    counts = {label: sum(case["label"] == label for case in cases.values())
              for label in ("supported", "unsupported", "unknown")}
    require(counts["supported"] >= spec["min_supported"] and counts["unsupported"] >= spec["min_unsupported"],
            "holdout independent binary label quotas not met")
    body = {"schema": REG_SCHEMA, "protocol": spec, "label_counts": counts,
            "artifact_cohort_file": str(Path(cohort_file).resolve()), "artifact_cohort_sha256": file_hash(cohort_file),
            "artifact_cohort_hash": registered["artifact_hash"], "threshold": registered["holdout_lock"]["threshold"],
            "dataset_file": str(Path(dataset_file).resolve()), "dataset_file_sha256": file_hash(dataset_file),
            "dataset_hash": digest(data), "prepared_file": str(Path(prepared_file).resolve()),
            "prepared_file_sha256": file_hash(prepared_file), "prepared_hash": digest(rows),
            "case_ids": sorted(row["id"] for row in rows), "registered_before_jev": True,
            "threshold_retuning_allowed": False, "production_authorized": False,
            **{field: data[field] for field in PROFILE_FIELDS if field in data}}
    result = {**body, "artifact_hash": digest(body)}
    validate(cohort_file)
    durable_new(marker, {"registration_file": str(Path(output).resolve()), "registration_hash": result["artifact_hash"]})
    durable_new(output, result)
    return result


@source_transaction
def _label_registration(path):
    value = sealed(load(path))
    require(value["schema"] == REG_SCHEMA and value["registered_before_jev"] is True
            and value["threshold_retuning_allowed"] is False and value["production_authorized"] is False,
            "original holdout label registration required")
    registered = validate(value["artifact_cohort_file"])
    require(value["artifact_cohort_sha256"] == file_hash(value["artifact_cohort_file"])
            and value["artifact_cohort_hash"] == registered["artifact_hash"]
            and value["threshold"] == registered["holdout_lock"]["threshold"], "holdout registration lock changed")
    marker = load(_marker(registered, ".labels.started"))
    require(marker["registration_hash"] == value["artifact_hash"]
            and Path(marker["registration_file"]).resolve() == Path(path).resolve(), "alternate label registration forbidden")
    for name in ("dataset", "prepared"):
        require(file_hash(value[f"{name}_file"]) == value[f"{name}_file_sha256"], "holdout labels/requests changed")
    data, rows = load(value["dataset_file"]), read_lines(value["prepared_file"])
    verify_dataset_cases(data)
    require(digest(data) == value["dataset_hash"] and digest(rows) == value["prepared_hash"],
            "holdout original contents changed")
    for row in rows:
        verify_prepared(row)
    return value, data, rows


def consume(registration_file, observations_file, output):
    """Once-only original observations; failures consume the marker as well."""
    require(not Path(output).exists(), "new holdout evaluation output required")
    registration, data, rows = _label_registration(registration_file)
    registered = validate(registration["artifact_cohort_file"])
    marker = _marker(registered, ".evaluation.started")
    durable_new(marker, {"schema": "buglecat.verification.artifact-holdout-consumption.v1",
                         "registration_hash": registration["artifact_hash"], "threshold": registration["threshold"],
                         "observations_file": str(Path(observations_file).resolve()),
                         "output_file": str(Path(output).resolve()), "no_retry": True})
    value = evaluation_value(registration_file, observations_file, output)
    durable_new(output, value)
    return value


@source_transaction
def evaluation_value(registration_file, observations_file, output):
    """Reconstruct the fixed result from original receipts, without consuming again."""
    registration, data, rows = _label_registration(registration_file)
    registered = validate(registration["artifact_cohort_file"])
    consumed = load(_marker(registered, ".evaluation.started"))
    require(consumed["registration_hash"] == registration["artifact_hash"]
            and consumed["threshold"] == registration["threshold"] and consumed["no_retry"] is True
            and Path(consumed["observations_file"]).resolve() == Path(observations_file).resolve()
            and Path(consumed["output_file"]).resolve() == Path(output).resolve(),
            "holdout consumption binding mismatch")
    # No observations are opened before the durable once-only marker.
    observations = read_lines(observations_file)
    observations_sha = file_hash(observations_file)
    from .artifact_formal_observations import observe
    by_id = {row["id"]: row for row in rows}
    for observed in observations:
        require(observed["id"] in by_id, "foreign holdout observation")
        verified = observe(by_id[observed["id"]], registration_file, observed["original_events_file"],
                           observed["reservation_file"], observed["runner_executable_file"], observed["runner_source_file"])
        require(verified == observed, "holdout observation differs from original transport bytes")
    cases = [case for case in data["cases"] if case["id"] in by_id]
    result = score(cases, observations, registration["threshold"], registration["protocol"])
    require(_label_registration(registration_file)[0] == registration
            and file_hash(observations_file) == observations_sha, "holdout artifacts changed during evaluation")
    body = {"schema": RESULT_SCHEMA, **result, "registration_file": str(Path(registration_file).resolve()),
            "registration_sha256": file_hash(registration_file), "registration_hash": registration["artifact_hash"],
            "observations_file": str(Path(observations_file).resolve()), "observations_file_sha256": observations_sha,
            "observations_hash": digest(observations), "allocated_groups": len(data["cases"]),
            "unknown_source_groups": sum(case["label"] == "unknown" for case in data["cases"]),
            "threshold_retuning_allowed": False, "holdout_numeric_gate_met": result["gate_met"],
            "artifact_benchmark_certified": result["gate_met"],
            "certified_artifact_threshold": registration["threshold"] if result["gate_met"] else None,
            "certification_scope": "frozen MBPP source groups, main resource profile, assertion Oracle and Jev template only",
            "overall_agent_decision_benefit_certified": False,
            "certified_threshold": None, "production_authorized": False, "native_claim_certification": False,
            "baseline_framework_A": "unavailable", "baseline_rules_B": "unavailable", "new_paid_calls": 0}
    return {**body, "artifact_hash": digest(body)}


def revalidate_evaluation(output):
    value = sealed(load(output))
    require(value["schema"] == RESULT_SCHEMA, "original holdout evaluation required")
    require(value == evaluation_value(value["registration_file"], value["observations_file"], output),
            "holdout result differs from original once-only evidence")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["register-source", "validate", "register-labels", "consume"])
    for name in ("calibration", "cohort", "dataset", "prepared", "registration", "observations", "output"):
        parser.add_argument(f"--{name}", type=Path)
    parser.add_argument("--cohort-id")
    args = parser.parse_args()
    if args.action == "register-source":
        require(args.calibration and args.cohort_id and args.output, "calibration/cohort-id/output required")
        result = register_source(args.calibration, args.cohort_id, args.output)
    elif args.action == "validate":
        require(args.cohort, "cohort required")
        result = validate(args.cohort)
    elif args.action == "register-labels":
        require(args.cohort and args.dataset and args.prepared and args.output, "cohort/dataset/prepared/output required")
        result = register_labels(args.cohort, args.dataset, args.prepared, args.output)
    else:
        require(args.registration and args.observations and args.output, "registration/observations/output required")
        result = consume(args.registration, args.observations, args.output)
    print(json.dumps({"schema": result["schema"], "artifact_hash": result["artifact_hash"],
                      "certified_threshold": None, "production_authorized": False, "new_paid_calls": 0}))


if __name__ == "__main__":
    main()
