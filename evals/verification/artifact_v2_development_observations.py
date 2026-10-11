"""Original V2 dev transport replay with Windows filesystem path identity.

The original paid sender/intake bytes stay frozen. This new reader corrects
lexical device-prefix comparisons only. It never changes original event bytes,
replays HTTP, creates reservations or supplies model probabilities.
"""
from pathlib import Path

from .artifact_cohort import reservations
from .artifact_request_v2 import TEMPLATE, signal
from .artifact_v2_development import validate
from .artifact_v2_development_cohort import same_path, within_root
from .budget_authorization_v2 import resolve
from .calibration import digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines
from .source_bindings import source_transaction


@source_transaction
def observe(prepared_file, attempt_directory, executable):
    packet = validate(prepared_file)
    directory = Path(attempt_directory).resolve(strict=True)
    events = read_lines(directory / "events.jsonl")
    require(len(events) in {2, 3}, "original single attempt events required")
    start, finish = events[0], events[-1]
    require(start["status"] == "attempt_started" and finish["status"] == "attempt_finished", "original event order")
    expected = {"id": packet["id"], "group_id": packet["group_id"], "request_hash": packet["request_hash"],
                "prepared_sha256": file_hash(prepared_file), "template_version": TEMPLATE,
                "automatic_retries": 0, "formal_certification": False, "offline": False}
    for event in events:
        require(all(event.get(key) == value for key, value in expected.items()), "attempt binding mismatch")
        require(same_path(event["attempt_directory"], directory)
                and same_path(event["raw_response_file"], directory / "raw-response.json"), "original attempt path mismatch")
    require(load(directory / "receipt.json") == finish, "original receipt differs from finish")
    reservation = Path(start["reservation_file"]).resolve(strict=True)
    chain = resolve(packet["authorization_file"])
    root = Path(chain["authorization"]["jev"]["ledger_dir"]).resolve()
    require(within_root(directory, root / "attempts"), "attempt outside shared ledger")
    require(same_path(reservation, root / "reservations" / (digest(packet["group_id"]) + ".json")),
            "original shared reservation required")
    reserved, _ = reservations(root, "jev", chain)
    row = next((row for row in reserved if row["group_id"] == packet["group_id"]), None)
    require(row is not None and row["request_hash"] == packet["request_hash"], "reservation request mismatch")
    require(load(reservation.with_suffix(".started")) == start, "original one-use marker mismatch")
    for event in events:
        require(same_path(event["reservation_file"], reservation)
                and event.get("reservation_sha256") == file_hash(reservation), "reservation event binding mismatch")
    submitted = len(events) == 3
    require(finish["submitted"] is submitted, "submission event mismatch")
    if submitted:
        require(events[1]["status"] == "request_submitted"
                and events[1]["actual_request_hash"] == packet["request_hash"], "actual submission required")
    else:
        require(finish["call_status"] in {"cancelled", "missing_key"}
                and finish.get("response_sha256") is None, "unsubmitted attempt status mismatch")
    require(file_hash(directory / "request.json") == packet["request_hash"]
            and load(directory / "request.json") == packet["request"], "submitted request bytes mismatch")
    source = Path(__file__).resolve().parents[2] / "rust/crates/ncx-core/examples/verification_artifact_v2_development.rs"
    owners = {"runner_source_sha256": source,
              "provider_source_sha256": source.parents[2] / "ncx-provider/src/jev.rs",
              "frozen_support_sha256": source.parent / "verification_artifact_profile_formal/support.rs",
              "frozen_formal_sha256": source.parent / "verification_artifact_profile_formal/formal.rs",
              "intake_source_sha256": Path(__file__).with_name("artifact_v2_development.py"),
              "builder_source_sha256": Path(__file__).with_name("artifact_request_v2.py"),
              "development_cohort_source_sha256": Path(__file__).with_name("artifact_v2_development_cohort.py")}
    for event in events:
        require(event["runner_exe_sha256"] == file_hash(executable)
                and all(event.get(key) == file_hash(path) for key, path in owners.items()), "actual compiled owner changed")
    raw = directory / "raw-response.json"
    require(finish.get("response_sha256") == file_hash(raw) if raw.exists()
            else finish.get("response_sha256") is None, "raw response bytes mismatch")
    value = {"schema": "buglecat.verification.artifact-v2-development-observation.v2",
             "id": packet["id"], "group_id": packet["group_id"], "split": "development",
             "request_hash": packet["request_hash"], "call_status": finish["call_status"],
             "acceptance_violation": None, "question_probabilities": None,
             "original_events_sha256": file_hash(directory / "events.jsonl"),
             "observer_source_sha256": file_hash(__file__),
             "formal_certification": False, "certified_threshold": None, "production_authorized": False}
    if finish["call_status"] == "ok":
        require(submitted and finish["response_sha256"] == file_hash(raw), "original response required")
        parsed = signal(packet["request"], load(raw))
        value.update(acceptance_violation=parsed["acceptance_violation"],
                     question_probabilities=parsed["question_probabilities"], model=parsed["model"],
                     usage=parsed["usage"], response_sha256=finish["response_sha256"])
    return {**value, "artifact_hash": digest(value)}
