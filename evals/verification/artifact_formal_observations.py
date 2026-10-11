"""Verify formal artifact transport from original bytes, not a status string."""
import hashlib
from pathlib import Path

from .annotation_audit import within_root
from .artifact_formal_intake import PROFILE_FIELDS, verify_prepared
from .budget_authorization import resolve
from .calibration import digest, load, probability, require
from .collection_receipts import read_lines
from .transport_receipts import verify


def holdout_owner_sources(examples):
    root = Path(__file__).resolve().parents[2]
    owners = {
        "holdout_transport_guard_source_sha256": "artifact_holdout_transport_guard.py",
        "holdout_gate_owner_source_sha256": "artifact_holdout.py",
        "formal_calibration_owner_source_sha256": "artifact_formal.py",
        "formal_intake_owner_source_sha256": "artifact_formal_intake.py",
        "formal_observations_owner_source_sha256": "artifact_formal_observations.py",
        "artifact_numeric_gate_source_sha256": "artifact_gate.py",
        "source_binding_owner_source_sha256": "source_bindings.py",
    }
    return {**{field: root / "evals/verification" / filename for field, filename in owners.items()},
            "holdout_rust_guard_source_sha256": examples / "verification_artifact_holdout/guard.rs"}


def runner_sources(runner_source):
    """Exact entrypoint dispatch; all compiled helpers remain bound to bytes."""
    source = Path(runner_source)
    require(source.name in {"verification_artifact_formal.rs", "verification_artifact_profile_formal.rs",
                           "verification_artifact_holdout_formal.rs"},
            "unknown formal transport entrypoint")
    holdout = source.name == "verification_artifact_holdout_formal.rs"
    profile = source.name != "verification_artifact_formal.rs"
    files = {
        "runner_source_sha256": source,
        "runner_support_source_sha256": source.parent / source.stem / "support.rs",
        "formal_gate_source_sha256": source.parent / source.stem / "formal.rs",
        "collector_validator_source_sha256": source.parent / (
            "verification_artifact_holdout_collect/support.rs" if holdout else
            "verification_artifact_profile_collect/support.rs" if profile else "verification_artifact_collect/support.rs"),
        "legacy_collector_support_sha256": source.parent / "verification_collect/support.rs",
        "provider_transport_source_sha256": source.parents[1].parent / "ncx-provider/src/jev.rs",
    }
    if profile:
        files.update({
            "artifact_transport_engine_source_sha256": source.parent / source.stem / "engine.rs",
            "frozen_collector_support_sha256": source.parent / "verification_artifact_collect/support.rs",
            "profile_collection_engine_source_sha256": source.parent / "verification_artifact_profile_collect/engine.rs",
            "profile_collector_source_sha256": source.parent / "verification_artifact_profile_collect.rs",
        })
    if holdout:
        files.update(holdout_owner_sources(source.parent))
        files.update({
            "holdout_collection_engine_source_sha256": source.parent / "verification_artifact_holdout_collect/engine.rs",
            "holdout_collector_source_sha256": source.parent / "verification_artifact_holdout_collect.rs",
            "profile_collector_validator_source_sha256": source.parent / "verification_artifact_profile_collect/support.rs",
        })
    return files


def observe(row, registration_file, events_file, reservation_file, executable, runner_source):
    verify_prepared(row)
    registration = load(registration_file)
    require(registration["artifact_hash"] == digest({key: value for key, value in registration.items() if key != "artifact_hash"}),
            "formal registration seal mismatch")
    require(row["id"] in registration["case_ids"] and registration["registered_before_jev"] is True,
            "formal row not prediction-blind registered")
    expected_runner = ("verification_artifact_holdout_formal.rs" if row["split"] == "holdout" else
                       "verification_artifact_profile_formal.rs" if "main_resource_profile_hash" in row else
                       "verification_artifact_formal.rs")
    require(Path(runner_source).name == expected_runner,
            "resource profile and formal runner mismatch")
    for field in PROFILE_FIELDS:
        if field in row:
            require(registration.get(field) == row[field], "formal registered resource profile mismatch")
    chain = resolve(row["authorization_file"])
    ledger_root = Path(chain["authorization"]["jev"]["ledger_dir"])
    require(within_root(events_file, ledger_root) and within_root(reservation_file, ledger_root / "reservations"),
            "formal original transport outside global ledger")
    events = read_lines(events_file)
    require(len(events) in {2, 3}, "one original formal attempt required")
    start, finish = events[0], events[-1]
    require(start["status"] == "attempt_started" and finish["status"] == "attempt_finished"
            and start["offline"] is False, "real original formal transport required")
    for event in events:
        for field in PROFILE_FIELDS:
            if field in row:
                require(event.get(field) == row[field], "formal transport resource profile mismatch")
        for field, expected in (("id", row["id"]), ("group_id", row["group_id"]), ("request_hash", row["request_hash"]),
                                ("prepared_artifact_sha256", row["artifact_sha256"]),
                                ("artifact_cohort_sha256", row["artifact_cohort_sha256"]),
                                ("artifact_cohort_hash", row["artifact_cohort_hash"]),
                                ("formal_registration_hash", registration["artifact_hash"]),
                                ("formal_registration_sha256", hashlib.sha256(Path(registration_file).read_bytes()).hexdigest()),
                                ("split", row["split"]), ("category", row["category"]),
                                ("policy_version", row["policy_version"]), ("template_version", row["template_version"]),
                                ("native_policy_version", row["native_policy_version"]),
                                ("candidate_hash", row["candidate_sha256"]), ("code_sha256", row["code_sha256"]),
                                ("input_hash", digest(row["request"]["state"])),
                                ("source_sha256", row["source_sha256"]), ("oracle_sha256", row["oracle_sha256"]),
                                ("main_receipt_sha256", row["main_receipt_sha256"]),
                                ("protocol_sha256", row["protocol_sha256"]),
                                ("authorization_sha256", row["authorization_sha256"]),
                                ("expected_actual_model", row["expected_actual_model"]),
                                ("requested_model", "jev-latest"), ("automatic_retries", 0), ("offline", False)):
            require(event.get(field) == expected, "formal transport provenance mismatch")
        require(event["request"] == row["request"], "formal original submitted body mismatch")
    reservation = load(reservation_file)
    require(reservation == start["reservation"] and reservation["authorization_sha256"] == chain["current_sha256"]
            and reservation["group_id"] == row["group_id"] and reservation["request_hash"] == row["request_hash"]
            and reservation["kind"] == "jev", "formal reservation binding mismatch")
    require(hashlib.sha256(Path(reservation_file).read_bytes()).hexdigest() == start["reservation_file_sha256"],
            "formal original reservation bytes mismatch")
    require(load(ledger_root / "authorization.json") == {
        "authorization_sha256": chain["ledger_authorization_sha256"], "kind": "jev"}, "formal global ledger mismatch")
    require(Path(reservation_file).name == digest(row["group_id"]) + ".json", "formal reservation filename mismatch")
    records = [load(path) for path in (ledger_root / "reservations").glob("*.json")]
    ordinals = [record["reservation_index"] for record in records]
    require(len(ordinals) == len(set(ordinals)) and sorted(ordinals) == list(range(1, len(records) + 1)),
            "formal reservation counter discontinuity")
    require(len(records) <= chain["authorization"]["jev"]["max_attempts"]
            and len({record["group_id"] for record in records}) == len(records), "formal reservation cap or group reuse")
    for record in records:
        require(record["authorization_sha256"] in chain["allowed_sha256"]
                and record["kind"] == "jev", "foreign formal ledger reservation")
        cap = chain["authorization"]["jev"]["max_attempts"] if record["authorization_sha256"] == chain["current_sha256"] else 400
        require(1 <= record["reservation_index"] <= cap, "reservation exceeds its original authorization")
    require(load(Path(reservation_file).with_suffix(".started")) == start, "original formal marker mismatch")
    for field, path in {**runner_sources(runner_source), "runner_exe_sha256": executable}.items():
        actual = hashlib.sha256(Path(path).read_bytes()).hexdigest()
        require(all(event["build_info"][field] == actual for event in events), "formal compiled source/runner changed")
    receipt_file = Path(events_file).with_name("receipt.json")
    require(hashlib.sha256(receipt_file.read_bytes()).hexdigest() == finish["receipt_file_sha256"]
            and load(receipt_file) == {key: value for key, value in finish.items() if key != "receipt_file_sha256"},
            "formal original final receipt mismatch")
    if len(events) == 3:
        require(events[1]["status"] == "request_submitted"
                and events[1]["actual_request_hash"] == row["request_hash"], "formal submit binding mismatch")
    else:
        require(finish["call_status"] == "cancelled" and finish["submitted"] is False,
                "missing formal submission must be cancellation")
    raw_file = finish.get("raw_response_file")
    if raw_file:
        require(within_root(raw_file, ledger_root)
                and hashlib.sha256(Path(raw_file).read_bytes()).hexdigest() == finish["response_sha256"],
                "original formal response bytes mismatch")
        require(len(events) == 3 and Path(events[1]["planned_raw_response_file"]).resolve() == Path(raw_file).resolve(),
                "formal original response path changed")
    result = {"id": row["id"], "request_hash": row["request_hash"],
              "actual_request_hash": events[1].get("actual_request_hash") if len(events) == 3 else None,
              "formal_registration_hash": registration["artifact_hash"], "call_status": finish["call_status"],
              "model": finish.get("model"), "usage": finish.get("usage"),
              "response_sha256": finish.get("response_sha256"), "acceptance_violation": None,
              "transport_binding_status": "failed_original_attempt_verified",
              "original_events_file": str(Path(events_file).resolve()),
              "reservation_file": str(Path(reservation_file).resolve()),
              "runner_executable_file": str(Path(executable).resolve()),
              "runner_source_file": str(Path(runner_source).resolve())}
    if finish["call_status"] == "ok":
        require(len(events) == 3, "formal successful transport lacks submission")
        verify(start, events[1], finish, row["request"], ledger_root, executable, runner_source)
        response = finish["response"]
        require(response["model"] == row["expected_actual_model"] == finish["model"]
                and response["usage"] == finish["usage"], "formal actual model/usage mismatch")
        require(set(response["answers"]) == {"acceptance_violation"}, "formal single NOUL question required")
        answer = response["answers"]["acceptance_violation"]
        require(answer["type"] == "noul" and probability(answer["noul"]), "invalid formal NOUL")
        result.update(acceptance_violation=answer["noul"], transport_binding_status="original_transport_bytes_verified")
    verify_prepared(row)
    return result
