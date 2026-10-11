"""Authenticate V2 original once-only bytes; no HTTP and no verdict authority."""
from pathlib import Path

from .annotation_audit import within_root
from .artifact_cohort import reservations
from .artifact_request_v2_formal import TEMPLATE, signal
from .artifact_scoring import sealed
from .artifact_v2_formal import revalidate
from .budget_authorization_v2 import resolve
from .calibration import digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs"
PYTHON_OWNERS = ["artifact_v2_formal.py", "artifact_v2_formal_cohort.py", "artifact_v2_formal_intake.py",
                 "artifact_v2_formal_observations.py", "artifact_v2_formal_holdout.py",
                 "artifact_request_v2_formal.py", "artifact_request_v2.py",
                 "budget_authorization_v2.py", "humaneval_source.py", "humaneval_oracle_worker.py",
                 "benchmark_oracle_worker.py", "artifact_gate.py", "calibration.py", "annotation_audit.py"]


def same_path(left, right):
    return within_root(left, right) and within_root(right, left)


@source_transaction
def observe(packet, registration_file, attempt_directory, executable):
    registered, _, _, packets = revalidate(registration_file)
    require(packet in packets and packet["split"] == registered["split"], "original registered packet required")
    return _observe_registered(packet, registration_file, registered, attempt_directory, executable)


def _observe_registered(packet, registration_file, registered, attempt_directory, executable):
    """Internal transport join after the caller verifies the entire registration.

    The statistical owner calls this only with its freshly authenticated packets.
    This function still reads every original attempt byte and shared reservation;
    it is not a standalone source verifier or a certification entry point.
    """
    sealed(packet)
    require(packet["split"] == registered["split"], "original registered split required")
    directory = Path(attempt_directory).resolve(strict=True)
    events_file = directory / "events.jsonl"
    events = read_lines(events_file)
    require(len(events) in {2, 3}, "original once-only attempt events required")
    start, finish = events[0], events[-1]
    require(start["status"] == "attempt_started" and finish["status"] == "attempt_finished", "original event order")
    packet_file = Path(start["prepared_file"]).resolve(strict=True)
    require(load(packet_file) == packet, "actual original prepared bytes required")
    expected = {"id": packet["id"], "group_id": packet["group_id"], "split": packet["split"],
                "request_hash": packet["request_hash"], "prepared_sha256": file_hash(packet_file),
                "template_version": TEMPLATE, "automatic_retries": 0, "formal_certification": False,
                "offline": False, "formal_registration_sha256": file_hash(registration_file),
                "formal_registration_hash": registered["artifact_hash"]}
    python_bindings = [{"file": "evals/verification/" + name,
                        "sha256": file_hash(Path(__file__).with_name(name))} for name in PYTHON_OWNERS]
    owners = {"runner_source_sha256": SOURCE,
              "provider_source_sha256": ROOT / "rust/crates/ncx-provider/src/jev.rs",
              "frozen_support_sha256": SOURCE.parent / "verification_artifact_profile_formal/support.rs",
              "frozen_formal_sha256": SOURCE.parent / "verification_artifact_profile_formal/formal.rs"}
    for event in events:
        require(all(event.get(key) == value for key, value in expected.items()), "attempt binding mismatch")
        require(same_path(event["attempt_directory"], directory)
                and same_path(event["raw_response_file"], directory / "raw-response.json")
                and same_path(event["formal_registration_file"], registration_file)
                and same_path(event["prepared_file"], packet_file), "original path mismatch")
        require(event["runner_exe_sha256"] == file_hash(executable)
                and all(event.get(key) == file_hash(path) for key, path in owners.items())
                and event.get("compiled_python_owners") == python_bindings, "actual compiled owner changed")
    require(load(directory / "receipt.json") == finish, "original receipt differs from finish")
    reservation = Path(start["reservation_file"]).resolve(strict=True)
    chain = resolve(packet["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    require(within_root(directory, root / "attempts")
            and same_path(reservation, root / "reservations" / (digest(packet["group_id"]) + ".json")),
            "original shared ledger paths required")
    reserved, _ = reservations(root, "jev", chain)
    row = next((row for row in reserved if row["group_id"] == packet["group_id"]), None)
    require(row is not None and row["request_hash"] == packet["request_hash"], "reservation request mismatch")
    require(load(reservation.with_suffix(".started")) == start, "original one-use marker mismatch")
    require(all(same_path(event["reservation_file"], reservation)
                and event["reservation_sha256"] == file_hash(reservation) for event in events), "original reservation changed")
    submitted = len(events) == 3
    require(finish["submitted"] is submitted, "submission mismatch")
    if submitted:
        require(events[1]["status"] == "request_submitted" and events[1]["actual_request_hash"] == packet["request_hash"],
                "actual original submission required")
    else:
        require(finish["call_status"] in {"cancelled", "missing_key"}
                and finish.get("response_sha256") is None, "unsubmitted failure cannot claim response")
    require(file_hash(directory / "request.json") == packet["request_hash"]
            and load(directory / "request.json") == packet["request"], "original transmitted request changed")
    raw = directory / "raw-response.json"
    require(finish.get("response_sha256") == file_hash(raw) if raw.exists()
            else finish.get("response_sha256") is None, "original response bytes changed")
    value = {"schema": "buglecat.verification.artifact-observation.formal.v2",
             "id": packet["id"], "group_id": packet["group_id"], "split": packet["split"],
             "request_hash": packet["request_hash"], "actual_request_hash": packet["request_hash"] if submitted else None,
             "call_status": finish["call_status"], "model": None, "acceptance_violation": None,
             "question_probabilities": None, "response_sha256": finish.get("response_sha256"),
             "formal_registration_hash": registered["artifact_hash"], "original_events_sha256": file_hash(events_file),
             "attempt_directory": str(directory), "runner_executable_file": str(Path(executable).resolve(strict=True)),
             "runner_executable_sha256": file_hash(executable), "observer_source_sha256": file_hash(__file__),
             "transport_binding_status": "original_transport_bytes_verified",
             "formal_certification": False, "production_authorized": False}
    if finish["call_status"] == "ok":
        require(submitted and finish["response_sha256"] == file_hash(raw), "actual response required")
        parsed = signal(packet["request"], load(raw))
        value.update(model=parsed["model"], acceptance_violation=parsed["acceptance_violation"],
                     question_probabilities=parsed["question_probabilities"], usage=parsed["usage"])
    return {**value, "artifact_hash": digest(value)}
