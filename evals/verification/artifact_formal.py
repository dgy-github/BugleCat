"""Source-verified artifact registration and calibration, never native claims.

Registration rejects old development data before reading any probability.
Numerical qualification is for a new holdout only, never production permission.
"""
import argparse
import json
from pathlib import Path

from .artifact_formal_intake import PROFILE_FIELDS, cohort, verify_dataset_cases, verify_prepared
from .artifact_gate import calibrate as numeric_calibrate
from .artifact_gate import protocol
from .artifact_scoring import sealed
from .calibration import create, digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction


@source_transaction
def registration_value(data_file, prepared_file):
    data = sealed(load(data_file))
    require(data["schema"] == "buglecat.verification.artifact-formal-dataset.v1"
            and data["scores_hidden"] is True and data["jev_probabilities_read"] is False,
            "formal original dataset required")
    registered = cohort(data["artifact_cohort_file"])
    verify_dataset_cases(data)
    require(file_hash(data["artifact_cohort_file"]) == data["artifact_cohort_sha256"]
            and registered["artifact_hash"] == data["artifact_cohort_hash"], "dataset source registration mismatch")
    spec_file = next(binding["file"] for binding in registered["bindings"] if binding["role"] == "protocol")
    spec = protocol(load(spec_file))
    rows = read_lines(prepared_file)
    cases = {case["id"]: case for case in data["cases"]}
    require(len(cases) == len(data["cases"]), "duplicate formal dataset case")
    require(len({case["group_id"] for case in cases.values()}) == len(cases), "repeated formal source group")
    require({row["id"] for row in rows} == {identity for identity, case in cases.items() if case["call_eligible"] is True}
            and len({row["id"] for row in rows}) == len(rows), "formal selected set changed")
    eligible = []
    for row in rows:
        sealed(row, "artifact_sha256")
        verify_prepared(row)
        case = cases[row["id"]]
        require(case["split"] == row["split"] == "calibration" and case["author_exposed"] is False
                and case["source_kind"] == "source_preregistered_original_model_code", "fresh formal source required")
        require(case["candidate_sha256"] == row["candidate_sha256"]
                and case["main_receipt_sha256"] == row["main_receipt_sha256"], "case candidate binding mismatch")
        oracle = sealed(case["oracle"])
        require(oracle["artifact_hash"] == row["oracle_sha256"]
                and case["label"] == {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"]),
                "formal label cannot override original Oracle")
        eligible.append(case)
    counts = {label: sum(case["label"] == label for case in eligible) for label in ("supported", "unsupported")}
    require(counts["supported"] >= spec["min_supported"] and counts["unsupported"] >= spec["min_unsupported"],
            "formal independently labeled group quotas not met")
    body = {"schema": "buglecat.verification.artifact-formal-registration.v1", "protocol": spec,
            "dataset_file": str(Path(data_file).resolve()), "dataset_file_sha256": file_hash(data_file),
            "prepared_file": str(Path(prepared_file).resolve()), "prepared_file_sha256": file_hash(prepared_file),
            "dataset_hash": digest(data), "prepared_hash": digest(rows), "case_ids": sorted(row["id"] for row in rows),
            "artifact_cohort_file": data["artifact_cohort_file"], "artifact_cohort_sha256": data["artifact_cohort_sha256"],
            "artifact_cohort_hash": data["artifact_cohort_hash"], "label_counts": counts,
            **{field: data[field] for field in PROFILE_FIELDS if field in data},
            "registered_before_jev": True, "holdout_opened": False, "production_authorized": False}
    return {**body, "artifact_hash": digest(body)}


def register(data_file, prepared_file, output):
    require(not Path(output).exists(), "new formal registration output required")
    value = registration_value(data_file, prepared_file)
    create(output, value)
    return value


@source_transaction
def revalidate(registration):
    sealed(registration)
    require(registration["schema"] == "buglecat.verification.artifact-formal-registration.v1"
            and registration["registered_before_jev"] is True, "formal registration required")
    for name in ("dataset", "prepared"):
        require(file_hash(registration[f"{name}_file"]) == registration[f"{name}_file_sha256"],
                "registered artifact changed")
    data, rows = load(registration["dataset_file"]), read_lines(registration["prepared_file"])
    require(digest(data) == registration["dataset_hash"] and digest(rows) == registration["prepared_hash"],
            "registered contents changed")
    for row in rows:
        verify_prepared(row)
    require(registration == registration_value(registration["dataset_file"], registration["prepared_file"]),
            "formal registration differs from original labels and source protocol")
    return data, rows


@source_transaction
def calibration_value(registration_file, observations_file):
    registration_raw = file_hash(registration_file)
    observations_raw = file_hash(observations_file)
    registration = load(registration_file)
    data, rows = revalidate(registration)
    observations = read_lines(observations_file)
    from .artifact_formal_observations import observe, runner_sources
    by_id = {row["id"]: row for row in rows}
    for observed in observations:
        require(observed["id"] in by_id and observed["request_hash"] == by_id[observed["id"]]["request_hash"],
                "observation original request changed")
        require(observed.get("formal_registration_hash") == registration["artifact_hash"],
                "formal original transport has no prediction-blind registration")
        verified = observe(by_id[observed["id"]], registration_file, observed["original_events_file"],
                           observed["reservation_file"], observed["runner_executable_file"], observed["runner_source_file"])
        require(verified == observed, "formal observation differs from original transport bytes")
    cases = [case for case in data["cases"] if case["id"] in by_id]
    result = numeric_calibrate(cases, observations, registration["protocol"])
    revalidate(registration)
    require(file_hash(registration_file) == registration_raw and file_hash(observations_file) == observations_raw,
            "original calibration evidence changed during verification")
    runner_bindings = []
    for source in sorted({observed["runner_source_file"] for observed in observations}):
        for role, bound in runner_sources(source).items():
            runner_bindings.append({"role": role, "file": str(bound.resolve()), "sha256": file_hash(bound)})
    for executable in sorted({observed["runner_executable_file"] for observed in observations}):
        runner_bindings.append({"role": "runner_executable", "file": str(Path(executable).resolve()),
                                "sha256": file_hash(executable)})
    body = {"schema": "buglecat.verification.artifact-formal-calibration.v1", **result,
            "registration_file": str(Path(registration_file).resolve()), "registration_hash": registration["artifact_hash"],
            "registration_file_sha256": registration_raw, "observations_hash": digest(observations),
            "observations_file": str(Path(observations_file).resolve()), "observations_file_sha256": observations_raw,
            "runner_bindings": runner_bindings,
            "calibration_groups": sorted(case["group_id"] for case in cases),
            "holdout_status": "not_run", "baseline_framework_A": "unavailable", "baseline_rules_B": "unavailable"}
    return {**body, "artifact_hash": digest(body)}


def calibrate(registration_file, observations_file, output):
    require(not Path(output).exists(), "new calibration output required")
    value = calibration_value(registration_file, observations_file)
    create(output, value)
    return value


def revalidate_calibration(calibration_file):
    value = sealed(load(calibration_file))
    require(value["schema"] == "buglecat.verification.artifact-formal-calibration.v1", "formal calibration required")
    for name in ("registration", "observations"):
        require(file_hash(value[f"{name}_file"]) == value[f"{name}_file_sha256"], "original calibration bytes changed")
    require(value == calibration_value(value["registration_file"], value["observations_file"]),
            "calibration result differs from original transport and fixed statistics")
    registration = load(value["registration_file"])
    data, rows = revalidate(registration)
    observations = read_lines(value["observations_file"])
    return value, registration, data, rows, observations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["register", "calibrate"])
    parser.add_argument("--dataset", type=Path)
    parser.add_argument("--prepared", type=Path)
    parser.add_argument("--registration", type=Path)
    parser.add_argument("--observations", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.action == "register":
        require(args.dataset and args.prepared, "dataset/prepared paths required")
        result = register(args.dataset, args.prepared, args.output)
    else:
        require(args.registration and args.observations, "registration/observation paths required")
        result = calibrate(args.registration, args.observations, args.output)
    print(json.dumps({"schema": result["schema"], "certified_threshold": None, "new_paid_calls": 0}))


if __name__ == "__main__":
    main()
