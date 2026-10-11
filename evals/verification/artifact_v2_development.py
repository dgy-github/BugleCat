"""Source-verified development packets for the assertion-specific template.

No transport. Reuses the frozen V1 intake only to authenticate the original
candidate and public source, then builds a distinct development request. Old
Jev-paid groups cannot be resent by the single-use reservation owner.
"""
import argparse
import json
from pathlib import Path

from .artifact_formal_intake import verify_prepared
from .artifact_request_v2 import TEMPLATE, request, signal, validate_request
from .artifact_scoring import sealed
from .budget_authorization_v2 import resolve
from .calibration import create, digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction

SCHEMA = "buglecat.verification.artifact-v2-development-prepared.v1"


def prepare_collection(directory, identity, executable):
    """Authenticate a new dev-only collector; old formal intake cannot admit it."""
    from .artifact_v2_development_cohort import verify_collected
    directory, executable = Path(directory).resolve(strict=True), Path(executable).resolve(strict=True)
    outcomes = verify_collected(directory, executable)
    selected = [row for row in outcomes if row["id"] == identity]
    require(len(selected) == 1 and selected[0]["call_eligible"] is True,
            "one independently labeled eligible development candidate required")
    original = selected[0]
    body = request(original["task"], original["public_interface"], original["candidate_code"])
    owners = [Path(__file__).resolve(), Path(__file__).with_name("artifact_request_v2.py").resolve(),
              Path(__file__).with_name("artifact_v2_development_cohort.py").resolve()]
    value = {"schema": SCHEMA, "id": identity, "group_id": original["group_id"],
             "split": "development", "template_version": TEMPLATE,
             "source_kind": "fresh_development_collection", "original_collection_directory": str(directory),
             "main_executable": str(executable), "main_executable_sha256": file_hash(executable),
             "main_receipt_file": original["main_receipt_file"],
             "main_receipt_sha256": original["main_receipt_sha256"],
             "source_cohort_hash": original["artifact_cohort_hash"],
             "authorization_file": original["authorization_file"],
             "authorization_sha256": original["authorization_sha256"],
             "request": body, "request_hash": digest(body),
             "source_bindings": [{"file": str(path), "sha256": file_hash(path)} for path in owners],
             "registered_for_formal_evaluation": False, "production_authorized": False}
    return {**value, "artifact_hash": digest(value)}


@source_transaction
def prepare(original_file, identity):
    original_file = Path(original_file).resolve(strict=True)
    before = file_hash(original_file)
    rows = read_lines(original_file)
    require(len({row["id"] for row in rows}) == len(rows), "duplicate original ID")
    selected = [row for row in rows if row["id"] == identity]
    require(len(selected) == 1, "one original source case required")
    original = selected[0]
    sealed(original, "artifact_sha256")
    verify_prepared(original)
    public = original["request"]["state"]
    task = {"prompt": public["task_prompt"], "test_list": public["acceptance_assertions"],
            "test_imports": public["test_imports"]}
    body = request(task, public["public_interface"], public["candidate_code"])
    owners = [Path(__file__).resolve(), Path(__file__).with_name("artifact_request_v2.py").resolve()]
    value = {"schema": SCHEMA, "id": identity, "group_id": original["group_id"],
             "split": "development", "template_version": TEMPLATE,
             "original_prepared_file": str(original_file), "original_prepared_sha256": before,
             "original_artifact_hash": original["artifact_sha256"],
             "authorization_file": original["authorization_file"],
             "authorization_sha256": original["authorization_sha256"],
             "request": body, "request_hash": digest(body),
             "source_bindings": [{"file": str(path), "sha256": file_hash(path)} for path in owners],
             "registered_for_formal_evaluation": False, "production_authorized": False}
    require(file_hash(original_file) == before, "original changed during preparation")
    return {**value, "artifact_hash": digest(value)}


def validate(path):
    packet = sealed(load(path))
    require(packet["schema"] == SCHEMA and packet["split"] == "development"
            and packet["registered_for_formal_evaluation"] is False
            and packet["production_authorized"] is False, "development packet required")
    require(all(file_hash(binding["file"]) == binding["sha256"] for binding in packet["source_bindings"]),
            "development owner changed")
    if packet.get("source_kind") == "fresh_development_collection":
        require(file_hash(packet["main_executable"]) == packet["main_executable_sha256"], "main executable changed")
        expected = prepare_collection(packet["original_collection_directory"], packet["id"], packet["main_executable"])
    else:
        require(file_hash(packet["original_prepared_file"]) == packet["original_prepared_sha256"],
                "original prepared bytes changed")
        expected = prepare(packet["original_prepared_file"], packet["id"])
    require(expected == packet, "packet differs from original candidate and public assertions")
    validate_request(packet["request"])
    return packet


def validate_reservation(prepared_file, reservation_file):
    packet = validate(prepared_file)
    chain = resolve(packet["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    path = Path(reservation_file).resolve(strict=True)
    require(path == root / "reservations" / (digest(packet["group_id"]) + ".json"),
            "original shared reservation required")
    from .artifact_cohort import reservations
    rows, _ = reservations(root, "jev", chain)
    original = next((row for row in rows if row["group_id"] == packet["group_id"]), None)
    require(original is not None and original["request_hash"] == packet["request_hash"],
            "reservation request mismatch")
    return {"status": "development_reservation_verified", "request_hash": packet["request_hash"],
            "reservation_file": str(path), "reservation_sha256": file_hash(path),
            "marker_file": str(path.with_suffix(".started")), "ledger_root": str(root),
            "new_paid_calls": 0}


def observe(prepared_file, attempt_directory, executable):
    """Join immutable submitted bytes, raw response and compiled owner evidence."""
    packet = validate(prepared_file)
    directory = Path(attempt_directory).resolve(strict=True)
    events = read_lines(directory / "events.jsonl")
    require(len(events) in {2, 3}, "original single attempt events required")
    start, finish = events[0], events[-1]
    require(start["status"] == "attempt_started" and finish["status"] == "attempt_finished",
            "original event order")
    expected = {"id": packet["id"], "group_id": packet["group_id"], "request_hash": packet["request_hash"],
                "prepared_sha256": file_hash(prepared_file), "template_version": TEMPLATE,
                "automatic_retries": 0, "formal_certification": False, "offline": False}
    for event in events:
        require(all(event.get(key) == value for key, value in expected.items()), "attempt binding mismatch")
        require(Path(event["attempt_directory"]).resolve() == directory
                and Path(event["raw_response_file"]).resolve() == directory / "raw-response.json",
                "original attempt path mismatch")
    require(load(directory / "receipt.json") == finish, "original receipt differs from finish")
    reservation_file = Path(start["reservation_file"]).resolve(strict=True)
    chain = resolve(packet["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    require(directory.is_relative_to(root / "attempts"), "attempt outside shared ledger")
    require(reservation_file == root / "reservations" / (digest(packet["group_id"]) + ".json"),
            "original shared reservation required")
    from .artifact_cohort import reservations
    reserved, _ = reservations(root, "jev", chain)
    original = next((row for row in reserved if row["group_id"] == packet["group_id"]), None)
    require(original is not None and original["request_hash"] == packet["request_hash"],
            "reservation request mismatch")
    marker = reservation_file.with_suffix(".started")
    require(load(marker) == start, "original one-use marker mismatch")
    require(all(event.get("reservation_file") == str(reservation_file)
                and event.get("reservation_sha256") == file_hash(reservation_file) for event in events),
            "reservation event binding mismatch")
    submitted = len(events) == 3
    require(finish["submitted"] is submitted, "submission event mismatch")
    if submitted:
        require(events[1]["status"] == "request_submitted"
                and events[1]["actual_request_hash"] == packet["request_hash"], "actual submission required")
    else:
        require(finish["call_status"] in {"cancelled", "missing_key"}
                and finish.get("response_sha256") is None, "unsubmitted attempt status mismatch")
    request_file = directory / "request.json"
    require(file_hash(request_file) == packet["request_hash"] and load(request_file) == packet["request"],
            "submitted request bytes mismatch")
    source = Path(__file__).resolve().parents[2] / "rust/crates/ncx-core/examples/verification_artifact_v2_development.rs"
    provider = source.parents[2] / "ncx-provider/src/jev.rs"
    for event in events:
        require(event["runner_exe_sha256"] == file_hash(executable)
                and event["runner_source_sha256"] == file_hash(source)
                and event["provider_source_sha256"] == file_hash(provider), "actual compiled owner changed")
        owners = {"frozen_support_sha256": source.parent / "verification_artifact_profile_formal/support.rs",
                  "frozen_formal_sha256": source.parent / "verification_artifact_profile_formal/formal.rs",
                  "intake_source_sha256": Path(__file__).resolve(),
                  "builder_source_sha256": Path(__file__).with_name("artifact_request_v2.py"),
                  "development_cohort_source_sha256": Path(__file__).with_name("artifact_v2_development_cohort.py")}
        require(all(event.get(key) == file_hash(path) for key, path in owners.items()),
                "compiled dependency changed")
    raw_file = directory / "raw-response.json"
    require((finish.get("response_sha256") == file_hash(raw_file)) if raw_file.exists()
            else finish.get("response_sha256") is None, "raw response bytes mismatch")
    value = {"schema": "buglecat.verification.artifact-v2-development-observation.v1",
             "id": packet["id"], "group_id": packet["group_id"], "split": "development",
             "request_hash": packet["request_hash"], "call_status": finish["call_status"],
             "acceptance_violation": None, "question_probabilities": None,
             "original_events_sha256": file_hash(directory / "events.jsonl"),
             "formal_certification": False, "certified_threshold": None, "production_authorized": False}
    if finish["call_status"] == "ok":
        require(submitted and finish["response_sha256"] == file_hash(directory / "raw-response.json"),
                "original response required")
        verdict = signal(packet["request"], load(directory / "raw-response.json"))
        value.update(acceptance_violation=verdict["acceptance_violation"],
                     question_probabilities=verdict["question_probabilities"], model=verdict["model"],
                     usage=verdict["usage"], response_sha256=finish["response_sha256"])
    return {**value, "artifact_hash": digest(value)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    building = sub.add_parser("prepare")
    sources = building.add_mutually_exclusive_group(required=True)
    sources.add_argument("--original", type=Path)
    sources.add_argument("--collection", type=Path)
    building.add_argument("--main-executable", type=Path)
    building.add_argument("--id", required=True)
    building.add_argument("--output", type=Path, required=True)
    checking = sub.add_parser("validate")
    checking.add_argument("--prepared", type=Path, required=True)
    reserving = sub.add_parser("validate-reservation")
    reserving.add_argument("--prepared", type=Path, required=True)
    reserving.add_argument("--reservation", type=Path, required=True)
    args = parser.parse_args()
    if args.action == "validate-reservation":
        print(json.dumps(validate_reservation(args.prepared, args.reservation)))
        return
    if args.action == "prepare":
        if args.collection:
            require(args.main_executable is not None, "actual main executable required")
            packet = prepare_collection(args.collection, args.id, args.main_executable)
        else:
            packet = prepare(args.original, args.id)
        create(args.output, packet)
    else:
        packet = validate(args.prepared)
    print(json.dumps({"status": "development_source_verified", "id": packet["id"],
                      "request_hash": packet["request_hash"], "new_paid_calls": 0,
                      "formal_certification": False}))


if __name__ == "__main__":
    main()
