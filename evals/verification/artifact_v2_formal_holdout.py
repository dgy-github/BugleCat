"""Once-only formal V2 holdout source opening; no transport or reservation.

Only original, qualified calibration evidence authorizes a fixed independent
source prefix. Public-reference admission does not open candidate predictions.
"""
import argparse
from pathlib import Path

from .annotation_audit import within_root
from .artifact_scoring import sealed
from .budget_authorization_v2 import resolve
from .calibration import create, digest, load, require
from .collection_manifest import file_hash
from .paid_budget import durable_new
from .source_bindings import source_transaction

SCHEMA = "buglecat.verification.artifact-holdout-consumption.formal.v2"


def same_path(left, right):
    return within_root(left, right) and within_root(right, left)


@source_transaction
def consumption_value(calibration_file, cohort_file):
    """Recompute all original calibration bytes before any source marker."""
    from .artifact_v2_formal import GRID, revalidate_calibration
    from .artifact_v2_formal_cohort import raw_ledger
    from .artifact_v2_formal_cohort import validate as source_validate
    calibration, registration, data, _, _ = revalidate_calibration(calibration_file)
    threshold = calibration["qualified_for_holdout_threshold"]
    require(threshold in GRID and calibration["calibration_numeric_gate_met"] is True,
            "original calibration failed; holdout prohibited")
    cohort, _, _ = source_validate(cohort_file)
    require(same_path(data["cohort_file"], cohort_file) and data["cohort_hash"] == cohort["artifact_hash"]
            and data["cohort_sha256"] == file_hash(cohort_file) and data["split"] == "calibration",
            "holdout must retain its original mixed-source calibration cohort")
    calibration_cases = [case for case in cohort["cases"] if case["split"] == "calibration"]
    cases = [case for case in cohort["cases"] if case["split"] == "holdout"]
    excluded = {case["group_id"] for case in calibration_cases} | set(cohort["excluded_groups"])
    require({case["group_id"] for case in data["cases"]} == {case["group_id"] for case in calibration_cases}
            and not excluded & {case["group_id"] for case in cases}, "complete calibration allocation must remain excluded")
    require(data["collected_groups"] == len(calibration_cases), "complete calibration collection must be terminal before holdout")
    auth = next(b["file"] for b in cohort["bindings"] if b["role"] == "authorization")
    current, _ = raw_ledger(auth)
    require(not {case["group_id"] for case in cases} & set(current["excluded_groups"]),
            "holdout source already started or reserved before opening")
    require(len(cases) >= 200 and len(cases) <= min(current["remaining_main_attempts"], current["remaining_jev_attempts"]),
            "shared remaining budget cannot fund complete frozen holdout")
    chain = resolve(auth)
    marker = Path(chain["authorization"]["main_model"]["ledger_dir"]) / "formal-v2-holdout-source" / (cohort["artifact_hash"] + ".json")
    body = {"schema": SCHEMA, "cohort_file": str(Path(cohort_file).resolve()), "cohort_sha256": file_hash(cohort_file),
            "cohort_hash": cohort["artifact_hash"], "cases": cases, "excluded_groups": sorted(excluded),
            "calibration_file": str(Path(calibration_file).resolve()), "calibration_sha256": file_hash(calibration_file),
            "calibration_hash": calibration["artifact_hash"], "threshold": threshold,
            "registration_file": calibration["registration_file"], "registration_sha256": file_hash(calibration["registration_file"]),
            "registration_hash": registration["artifact_hash"], "observations_file": calibration["observations_file"],
            "observations_sha256": file_hash(calibration["observations_file"]), "marker_file": str(marker.resolve()),
            "remaining_main_at_open": current["remaining_main_attempts"], "remaining_jev_at_open": current["remaining_jev_attempts"],
            "threshold_retuned": False, "before_holdout_main": True, "before_holdout_jev": True,
            "production_authorized": False, "native_claim_certification": False, "new_paid_calls": 0}
    return {**body, "artifact_hash": digest(body)}


def open_source(calibration_file, cohort_file, output):
    """Explicit Root operation. A partial output failure burns the fixed marker."""
    value = consumption_value(calibration_file, cohort_file)
    marker = Path(value["marker_file"])
    marker.parent.mkdir(parents=True, exist_ok=True)
    durable_new(marker, value)
    create(output, value)
    return value


@source_transaction
def validate(path):
    from .artifact_v2_formal import GRID, revalidate_calibration
    from .artifact_v2_formal_cohort import validate as source_validate
    value = sealed(load(path))
    require(value["schema"] == SCHEMA and value["threshold_retuned"] is False
            and value["before_holdout_main"] is True and value["before_holdout_jev"] is True
            and value["production_authorized"] is False and value["native_claim_certification"] is False and value["new_paid_calls"] == 0,
            "original once-only holdout opening required")
    calibration, registration, data, _, _ = revalidate_calibration(value["calibration_file"])
    require(calibration["qualified_for_holdout_threshold"] in GRID and calibration["calibration_numeric_gate_met"] is True
            and value["threshold"] == calibration["qualified_for_holdout_threshold"]
            and value["calibration_hash"] == calibration["artifact_hash"]
            and value["calibration_sha256"] == file_hash(value["calibration_file"]), "original qualified threshold changed")
    cohort, _, _ = source_validate(value["cohort_file"])
    auth = next(b["file"] for b in cohort["bindings"] if b["role"] == "authorization")
    chain = resolve(auth)
    marker = Path(chain["authorization"]["main_model"]["ledger_dir"]) / "formal-v2-holdout-source" / (cohort["artifact_hash"] + ".json")
    require(same_path(value["marker_file"], marker) and load(marker) == value,
            "original shared once-only source marker required")
    cases = [case for case in cohort["cases"] if case["split"] == "holdout"]
    cal_groups = {case["group_id"] for case in cohort["cases"] if case["split"] == "calibration"}
    require(value["cases"] == cases and set(value["excluded_groups"]) == cal_groups | set(cohort["excluded_groups"])
            and {case["group_id"] for case in data["cases"]} == cal_groups
            and data["cohort_hash"] == cohort["artifact_hash"] == value["cohort_hash"]
            and data["cohort_sha256"] == value["cohort_sha256"] == file_hash(value["cohort_file"])
            and same_path(data["cohort_file"], value["cohort_file"]), "frozen holdout allocation changed")
    require(data["collected_groups"] == len(cal_groups), "original calibration collection incomplete")
    require(value["registration_file"] == calibration["registration_file"]
            and value["registration_hash"] == registration["artifact_hash"]
            and value["registration_sha256"] == file_hash(value["registration_file"])
            and value["observations_file"] == calibration["observations_file"]
            and value["observations_sha256"] == file_hash(value["observations_file"]), "original calibration receipts changed")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["validate", "open-source"])
    parser.add_argument("--consumption")
    parser.add_argument("--calibration")
    parser.add_argument("--cohort")
    parser.add_argument("--output")
    parser.add_argument("--open", action="store_true")
    args = parser.parse_args()
    if args.action == "open-source":
        require(args.open and args.calibration and args.cohort and args.output, "explicit --open and original files required")
        value = open_source(args.calibration, args.cohort, args.output)
    else:
        require(args.consumption, "original consumption file required")
        value = validate(args.consumption)
    print({"artifact_hash": value["artifact_hash"], "production_authorized": False, "new_paid_calls": 0})


if __name__ == "__main__":
    main()
