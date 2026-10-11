"""V2 source-verified registration and fixed statistics; no HTTP or Pass.

The new mixed-source intake authenticates original candidates and independent
worker receipts. Numerical helpers reuse V1's exact binomial metrics, without
claiming V1 provenance or reusing a V1 threshold. Only a new qualifying
calibration can authorize one separately recorded holdout.
"""
import argparse
import json
from pathlib import Path

from .artifact_gate import metrics
from .artifact_request_v2_formal import POLICY, TEMPLATE, request
from .artifact_scoring import sealed
from .calibration import create, digest, load, probability, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction

SCHEMA = "buglecat.verification.artifact-protocol.v2"
GRID = [.5, .6, .7, .8, .9, .95, .975, .99, 1.0]
GATES = {"min_supported": 150, "min_unsupported": 50, "max_false_block_rate": .02,
         "confidence": .95, "min_error_rejection_lower_bound": .5}


def protocol(value):
    require(value.get("schema") == SCHEMA and value.get("policy_version") == POLICY
            and value.get("template_version") == TEMPLATE
            and value.get("model") == "jev-1.13.0"
            and value.get("threshold_grid") == GRID
            and all(value.get(key) == expected for key, expected in GATES.items()),
            "new V2 fixed gates required; V1 threshold cannot migrate")
    require(value.get("categories") == ["artifact_acceptance"]
            and value.get("score") == "maximum_exact_question_noul"
            and value.get("source_kinds") == ["mbpp", "humaneval"]
            and value.get("candidate_oracle_timeout_s") == 5
            and value.get("reference_admission_timeout_s") == 10,
            "mixed-source execution and statistic contract changed")
    return value


def numeric(cases, observations, threshold, spec):
    """Numbers alone never authenticate labels, transport or certification."""
    protocol(spec)
    require(threshold in GRID, "threshold not predeclared")
    by_id = {case["id"]: case for case in cases}
    require(len(by_id) == len(cases) and len({case["group_id"] for case in cases}) == len(cases),
            "one original candidate per independent group")
    require(all(case["label"] in {"supported", "unsupported"} for case in cases),
            "Unknown cannot be binary")
    predictions = {row["id"]: row for row in observations}
    require(len(predictions) == len(observations) and predictions.keys() == by_id.keys(),
            "missing, repeated or foreign observation")
    rejected, failures = set(), {}
    for identity, row in predictions.items():
        if row["call_status"] != "ok":
            require(row.get("acceptance_violation") is None, "failed probability not permitted")
            failures[identity] = row["call_status"]
            continue
        require(row.get("model") == spec["model"]
                and row.get("transport_binding_status") == "original_transport_bytes_verified"
                and isinstance(row.get("request_hash"), str) and len(row["request_hash"]) == 64
                and row.get("actual_request_hash") == row.get("request_hash")
                and isinstance(row.get("response_sha256"), str) and len(row["response_sha256"]) == 64,
                "original versioned transport required")
        value = row.get("acceptance_violation")
        require(probability(value), "invalid exact question score")
        if value >= threshold:
            rejected.add(identity)
    supported = {identity for identity, case in by_id.items() if case["label"] == "supported"}
    unsupported = by_id.keys() - supported
    return {**metrics(len(supported), len(unsupported), len(supported & rejected),
                      len(unsupported & rejected), threshold, spec),
            "failed_attempts": failures, "failed_attempts_retained_in_denominators": True,
            "reject_signals": len(rejected), "unverified_signals": len(cases)-len(rejected),
            "verified_pass_signals": 0}


def calibrate_numbers(cases, observations, spec):
    trials = [numeric(cases, observations, threshold, spec) for threshold in protocol(spec)["threshold_grid"]]
    qualifying = [trial for trial in trials if trial["gate_met"]]
    chosen = max(qualifying, key=lambda trial: (trial["detected_unsupported"], trial["threshold"])) if qualifying else None
    return {"grid_results": trials, "calibration_numeric_gate_met": chosen is not None,
            "qualified_for_holdout_threshold": chosen["threshold"] if chosen else None,
            "certified_threshold": None, "formal_certification": False, "holdout_status": "not_run",
            "production_authorized": False, "native_claim_certification": False,
            "scope": "numeric fixed-suite classification only; provenance and new holdout required"}


@source_transaction
def originals(dataset_file):
    """Lazy dependency: unavailable intake fails before registration or spending."""
    from .artifact_v2_formal_intake import verify_dataset
    data = sealed(load(dataset_file))
    require(data["schema"] == "buglecat.verification.artifact-dataset.formal.v2"
            and data["scores_hidden"] is True and data["jev_probabilities_read"] is False,
            "new prediction-blind source dataset required")
    verified, rows = verify_dataset(dataset_file, data["prepared_directory"], data["collector_executable_file"])
    require(verified == data, "dataset differs from original full allocation")
    spec = protocol(load(data["protocol_file"]))
    return data, rows, spec


def request_packets(dataset_file, data, rows):
    packets = []
    for row in rows:
        if row["call_eligible"] is not True:
            continue
        require(row["label"] in {"supported", "unsupported"}, "binary independent label required")
        require(isinstance(row.get("oracle_runtime_binding"), dict), "actual independently verified Oracle runtime required")
        body = request(row["task"], row["public_interface"], row["candidate_code"], row["source_kind"],
                       row["oracle_runtime_binding"])
        require(row.get("formal_request") == body, "worker-specific original intake request differs")
        value = {"schema": "buglecat.verification.artifact-prepared.formal.v2",
                 "id": row["id"], "group_id": row["group_id"], "split": data["split"],
                 "source_kind": row["source_kind"], "template_version": TEMPLATE,
                 "policy_version": POLICY, "request": body, "request_hash": digest(body),
                 "dataset_file": str(Path(dataset_file).resolve()), "dataset_sha256": file_hash(dataset_file),
                 "main_receipt_file": row["main_receipt_file"], "main_receipt_sha256": row["main_receipt_sha256"],
                 "oracle_file": row["oracle_file"], "oracle_sha256": row["oracle_sha256"],
                 "authorization_file": data["authorization_file"],
                 "authorization_sha256": data["authorization_sha256"],
                 "cohort_hash": data["cohort_hash"], "formal_certification": False,
                 "production_authorized": False}
        packets.append({**value, "artifact_hash": digest(value)})
    return packets


@source_transaction
def registration_value(dataset_file, calibration_file=None):
    data, rows, spec = originals(dataset_file)
    require(data["split"] in {"calibration", "holdout"}, "formal stage required")
    packets = request_packets(dataset_file, data, rows)
    binary = [row for row in rows if row["call_eligible"] is True]
    counts = {label: sum(row["label"] == label for row in binary) for label in ("supported", "unsupported")}
    require(all(counts[label] >= spec["min_" + label] for label in counts), "independent group quotas not met")
    parent = None
    if data["split"] == "holdout":
        require(calibration_file is not None, "genuine calibration required before holdout")
        parent, _, cal_data, _, _ = revalidate_calibration(calibration_file)
        require(parent["qualified_for_holdout_threshold"] is not None, "calibration failed; holdout prohibited")
        require(not {row["group_id"] for row in data["cases"]} & {row["group_id"] for row in cal_data["cases"]},
                "complete calibration allocation must be excluded")
        require(data["holdout_consumption_file"] and data["calibration_hash"] == parent["artifact_hash"],
                "original one-use holdout source opening required")
    else:
        require(calibration_file is None, "calibration cannot depend on later scores")
    body = {"schema": "buglecat.verification.artifact-registration.formal.v2", "split": data["split"],
            "protocol": spec, "protocol_file": data["protocol_file"], "protocol_sha256": file_hash(data["protocol_file"]),
            "dataset_file": str(Path(dataset_file).resolve()), "dataset_sha256": file_hash(dataset_file),
            "dataset_hash": digest(data), "prepared_hash": digest(packets),
            "case_ids": sorted(row["id"] for row in binary), "label_counts": counts,
            "all_allocated_groups": sorted(row["group_id"] for row in data["cases"]),
            "cohort_file": data["cohort_file"], "cohort_sha256": data["cohort_sha256"], "cohort_hash": data["cohort_hash"],
            "registered_before_jev": True, "production_authorized": False,
            "calibration_file": str(Path(calibration_file).resolve()) if calibration_file else None,
            "calibration_sha256": file_hash(calibration_file) if calibration_file else None,
            "calibration_hash": parent["artifact_hash"] if parent else None}
    return {**body, "artifact_hash": digest(body)}


def register(dataset_file, output, calibration_file=None):
    value = registration_value(dataset_file, calibration_file)
    from .artifact_cohort import reservations
    from .budget_authorization_v2 import resolve
    data = load(dataset_file)
    chain = resolve(data["authorization_file"])
    rows, _ = reservations(chain["authorization"]["jev"]["ledger_dir"], "jev", chain)
    require(not {row["group_id"] for row in rows} & set(value["all_allocated_groups"]),
            "registration must precede every Jev reservation in its complete allocation")
    create(output, value)
    return value


@source_transaction
def revalidate(registration_file):
    registered = sealed(load(registration_file))
    require(registered["schema"] == "buglecat.verification.artifact-registration.formal.v2"
            and file_hash(registered["dataset_file"]) == registered["dataset_sha256"]
            and file_hash(registered["protocol_file"]) == registered["protocol_sha256"], "original registered bytes changed")
    require(registration_value(registered["dataset_file"], registered["calibration_file"]) == registered,
            "registration differs from original independent source labels")
    data, rows, _ = originals(registered["dataset_file"])
    return registered, data, rows, request_packets(registered["dataset_file"], data, rows)


@source_transaction
def verified_observations(registration_file, observations_file):
    from .artifact_v2_formal_observations import _observe_registered
    registered, data, rows, packets = revalidate(registration_file)
    observations = read_lines(observations_file)
    by_id = {row["id"]: row for row in packets}
    require(len(observations) == len(by_id) and {row["id"] for row in observations} == by_id.keys(),
            "all eligible original attempts must be retained")
    for observed in observations:
        require(_observe_registered(by_id[observed["id"]], registration_file, registered,
                                    observed["attempt_directory"], observed["runner_executable_file"]) == observed,
                "observation differs from actual request and raw response")
    return registered, data, rows, packets, observations


@source_transaction
def calibration_value(registration_file, observations_file):
    original = verified_observations(registration_file, observations_file)
    return _calibration_from_originals(registration_file, observations_file, original)


def _calibration_from_originals(registration_file, observations_file, original):
    registered, data, rows, _, observations = original
    require(registered["split"] == data["split"] == "calibration", "new calibration required")
    result = calibrate_numbers([row for row in rows if row["call_eligible"]], observations, registered["protocol"])
    body = {"schema": "buglecat.verification.artifact-calibration.formal.v2", **result,
            "registration_file": str(Path(registration_file).resolve()), "registration_sha256": file_hash(registration_file),
            "registration_hash": registered["artifact_hash"],
            "observations_file": str(Path(observations_file).resolve()), "observations_sha256": file_hash(observations_file),
            "all_allocated_groups": registered["all_allocated_groups"],
            "baseline_framework_A": "unavailable", "baseline_rules_B": "unavailable"}
    return {**body, "artifact_hash": digest(body)}


def calibrate(registration_file, observations_file, output):
    value = calibration_value(registration_file, observations_file)
    create(output, value)
    return value


def revalidate_calibration(path):
    value = sealed(load(path))
    require(value["schema"] == "buglecat.verification.artifact-calibration.formal.v2"
            and file_hash(value["registration_file"]) == value["registration_sha256"]
            and file_hash(value["observations_file"]) == value["observations_sha256"], "original V2 calibration bytes required")
    original = verified_observations(value["registration_file"], value["observations_file"])
    require(value == _calibration_from_originals(value["registration_file"], value["observations_file"], original),
            "calibration differs from immutable original evidence")
    registered, data, rows, _, observations = original
    return value, registered, data, rows, observations


@source_transaction
def holdout_value(registration_file, observations_file):
    registered, data, rows, _, observations = verified_observations(registration_file, observations_file)
    require(registered["split"] == data["split"] == "holdout", "new original holdout required")
    parent, _, cal_data, _, _ = revalidate_calibration(registered["calibration_file"])
    require(parent["artifact_hash"] == registered["calibration_hash"]
            and file_hash(registered["calibration_file"]) == registered["calibration_sha256"],
            "holdout calibration changed")
    require(not {row["group_id"] for row in data["cases"]} & {row["group_id"] for row in cal_data["cases"]},
            "all calibration groups must remain excluded")
    threshold = parent["qualified_for_holdout_threshold"]
    require(threshold in GRID, "qualified V2 threshold required; no holdout tuning")
    result = numeric([row for row in rows if row["call_eligible"]], observations, threshold, registered["protocol"])
    body = {"schema": "buglecat.verification.artifact-holdout.formal.v2", **result,
            "registration_file": str(Path(registration_file).resolve()), "registration_sha256": file_hash(registration_file),
            "observations_file": str(Path(observations_file).resolve()), "observations_sha256": file_hash(observations_file),
            "calibration_file": registered["calibration_file"], "calibration_sha256": registered["calibration_sha256"],
            "calibration_hash": registered["calibration_hash"],
            "certified_threshold": threshold if result["gate_met"] else None,
            "formal_certification": result["gate_met"], "holdout_status": "completed",
            "production_authorized": False, "native_claim_certification": False,
            "overall_agent_benefit_certification": False,
            "scope": "exact frozen mixed-source code acceptance classifier only",
            "limitations": ["Public benchmark training contamination is not ruled out.",
                            "Source proportions and restricted runtimes define this evaluation scope.",
                            "A non-rejection is unverified; it never proves correct agent reasoning.",
                            "Framework and rules baselines are unavailable; no overall benefit claim."],
            "baseline_framework_A": "unavailable", "baseline_rules_B": "unavailable"}
    return {**body, "artifact_hash": digest(body)}


def holdout(registration_file, observations_file, output):
    value = holdout_value(registration_file, observations_file)
    # Output paths are user-selected; they cannot be the once-only authority.
    # A fixed marker in the original shared ledger makes alternate outputs
    # unable to reopen this evaluation. A crashed write remains consumed.
    from .budget_authorization_v2 import resolve
    from .paid_budget import durable_new
    registered, data, _, _ = revalidate(registration_file)
    chain = resolve(data["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    marker = root / "formal-v2-holdout-evaluations" / (data["cohort_hash"] + ".json")
    marker.parent.mkdir(parents=True, exist_ok=True)
    require(not Path(output).exists(), "fresh holdout result output required")
    claim = {"schema": "buglecat.verification.artifact-holdout-evaluation-consumption.v2",
             "cohort_hash": data["cohort_hash"], "registration_hash": registered["artifact_hash"],
             "calibration_hash": registered["calibration_hash"], "result_hash": value["artifact_hash"],
             "observations_sha256": file_hash(observations_file),
             "output_file": str(Path(output).resolve()), "threshold_tuning": False,
             "holdout_evaluation_consumptions": 1, "new_paid_calls": 0}
    durable_new(marker, {**claim, "artifact_hash": digest(claim)})
    create(output, value)
    return value


@source_transaction
def validate_packet(packet_file, registration_file):
    packet = sealed(load(packet_file))
    registered, _, _, packets = revalidate(registration_file)
    matches = [row for row in packets if row["id"] == packet["id"]]
    require(len(matches) == 1 and matches[0] == packet and packet["split"] == registered["split"],
            "packet differs from prediction-blind original registration")
    return packet, registered


def validate_reservation(packet_file, registration_file, reservation_file):
    from .artifact_cohort import reservations
    from .budget_authorization_v2 import resolve
    packet, registered = validate_packet(packet_file, registration_file)
    chain = resolve(packet["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    path = Path(reservation_file).resolve(strict=True)
    require(path == root / "reservations" / (digest(packet["group_id"]) + ".json"),
            "original shared reservation required")
    rows, _ = reservations(root, "jev", chain)
    row = next((row for row in rows if row["group_id"] == packet["group_id"]), None)
    require(row is not None and row["request_hash"] == packet["request_hash"], "shared reservation request changed")
    return {"status": "formal_v2_reservation_verified", "request_hash": packet["request_hash"],
            "registration_hash": registered["artifact_hash"], "reservation_file": str(path),
            "reservation_sha256": file_hash(path), "marker_file": str(path.with_suffix(".started")),
            "ledger_root": str(root), "new_paid_calls": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["validate-packet", "validate-reservation", "register", "calibrate", "holdout"])
    for name in ("packet", "registration", "reservation", "dataset", "calibration", "observations", "output"):
        parser.add_argument("--" + name, type=Path)
    args = parser.parse_args()
    if args.action == "validate-reservation":
        require(args.packet and args.registration and args.reservation, "original packet, registration and reservation required")
        print(json.dumps(validate_reservation(args.packet, args.registration, args.reservation)))
        return
    if args.action == "validate-packet":
        require(args.packet and args.registration, "packet and registration required")
        packet, registered = validate_packet(args.packet, args.registration)
        print(json.dumps({"status": "formal_v2_source_verified", "request_hash": packet["request_hash"],
                          "registration_hash": registered["artifact_hash"], "split": registered["split"], "new_paid_calls": 0}))
        return
    require(args.output, "create-new output required")
    if args.action == "register":
        require(args.dataset, "original dataset required")
        value = register(args.dataset, args.output, args.calibration)
    else:
        require(args.registration and args.observations, "original registration and observations required")
        value = (calibrate if args.action == "calibrate" else holdout)(args.registration, args.observations, args.output)
    print(json.dumps({"schema": value["schema"], "new_paid_calls": 0, "production_authorized": False}))


if __name__ == "__main__":
    main()
