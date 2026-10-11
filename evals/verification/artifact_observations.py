"""Join one real artifact attempt to frozen source and original Jev bytes.

Not a caller. It cannot create, refund or retry reservations. Failed requests
remain observations with no probability; only successful bytes can supply NOUL.
"""

import argparse
import hashlib
import json
from pathlib import Path

from .annotation_audit import within_root
from .artifact_acceptance import POLICY, TEMPLATE, request
from .artifact_scoring import sealed
from .benchmark_review import code_block
from .calibration import digest, load, probability, require
from .collection_receipts import read_lines
from .source_bindings import verify_manifest
from .transport_receipts import verify


def raw_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def frozen_row(row, expected_split="development"):
    sealed(row, "artifact_sha256")
    require(row["policy_version"] == POLICY and row["template_version"] == TEMPLATE
            and row["split"] == expected_split, "artifact track/version mismatch")
    for field in ("source", "collection_manifest", "main_receipt", "oracle_receipt", "protocol", "authorization"):
        require(raw_hash(row[f"{field}_file"]) == row[f"{field}_sha256"], "frozen artifact file changed")
    manifest = load(row["collection_manifest_file"])
    tasks, bindings = verify_manifest(manifest)
    case = next(c for c in manifest["cases"] if c["id"] == row["id"])
    require(case["group_id"] == row["group_id"] and case["task_id"] == row["task_id"], "artifact source identity mismatch")
    receipt = load(row["main_receipt_file"])
    sealed(receipt, "artifact_sha256")
    original_raw = Path(row["main_receipt_file"]).with_name("raw-response.json")
    require(raw_hash(original_raw) == receipt["response_sha256"], "original main response changed")
    raw = load(original_raw)
    candidate = raw["choices"][0]["message"]["content"]
    require(candidate == receipt["result"]["candidate"]
            and hashlib.sha256(candidate.encode()).hexdigest() == row["candidate_sha256"], "original candidate changed")
    code = code_block(candidate)
    require(hashlib.sha256(code.encode()).hexdigest() == row["code_sha256"], "original code changed")
    require(request(tasks[row["task_id"]], case["public_interface"], code) == row["request"]
            and digest(row["request"]) == row["request_hash"], "artifact body was changed")
    oracle = sealed(load(row["oracle_receipt_file"]))
    require(oracle["artifact_hash"] == row["oracle_sha256"] and oracle["candidate_sha256"] == row["candidate_sha256"],
            "oracle candidate binding mismatch")
    return bindings


def observe(row, events, ledger_root, executable, runner_source, reservation_file, events_file):
    frozen_row(row)
    require(len(events) in {2, 3}, "one original attempt required")
    started, finished = events[0], events[-1]
    require(read_lines(events_file) == events, "original event file mismatch")
    require(within_root(events_file, ledger_root),
            "artifact events outside global ledger")
    require(started["schema"] == "buglecat.verification.artifact-attempt.v1"
            and started["status"] == "attempt_started" and started["offline"] is False
            and finished["status"] == "attempt_finished", "not a real original artifact attempt")
    if len(events) == 3:
        require(events[1]["status"] == "request_submitted"
                and events[1]["actual_request_hash"] == row["request_hash"], "original submission mismatch")
    else:
        require(finished["call_status"] == "cancelled" and finished["submitted"] is False,
                "two events require cancellation before submission")
    for event in events:
        for event_field, row_field in (("id", "id"), ("group_id", "group_id"), ("request_hash", "request_hash"),
                                     ("candidate_hash", "candidate_sha256"), ("code_sha256", "code_sha256"),
                                     ("prepared_artifact_sha256", "artifact_sha256"), ("source_sha256", "source_sha256"),
                                     ("protocol_sha256", "protocol_sha256"), ("oracle_sha256", "oracle_sha256"),
                                     ("main_receipt_sha256", "main_receipt_sha256"),
                                     ("authorization_sha256", "authorization_sha256")):
            require(event[event_field] == row[row_field], f"artifact event {event_field} mismatch")
        require(event["policy_version"] == POLICY and event["template_version"] == TEMPLATE, "artifact event version mismatch")
        require(event["request"] == row["request"], "original artifact body mismatch")
    require(raw_hash(reservation_file) == started["reservation_file_sha256"], "original reservation changed")
    reservation = load(reservation_file)
    require(reservation == started["reservation"] and reservation["kind"] == "jev"
            and reservation["group_id"] == row["group_id"] and reservation["request_hash"] == row["request_hash"]
            and reservation["authorization_sha256"] == row["authorization_sha256"], "shared reservation binding mismatch")
    marker = Path(reservation_file).with_suffix(".started")
    require(load(marker) == started, "original start marker mismatch")
    auth = load(row["authorization_file"])
    require(Path(auth["jev"]["ledger_dir"]).resolve() == Path(ledger_root).resolve(), "global Jev ledger mismatch")
    require(Path(reservation_file).resolve().parent == Path(ledger_root).resolve() / "reservations", "foreign reservation path")
    for field, path in (("runner_source_sha256", runner_source), ("runner_exe_sha256", executable),
                        ("runner_support_source_sha256", Path(runner_source).parent / "verification_artifact/support.rs"),
                        ("provider_transport_source_sha256", Path(runner_source).parents[1].parent / "ncx-provider/src/jev.rs")):
        require(all(event["build_info"][field] == raw_hash(path) for event in events), "compiled artifact transport source changed")
    require(within_root(finished["raw_response_file"], ledger_root)
            if finished.get("raw_response_file") else True, "artifact response outside global ledger")
    # The finished event binds the unmodified receipt file written before it.
    receipt_file = Path(events_file).with_name("receipt.json")
    require(raw_hash(receipt_file) == finished["receipt_file_sha256"], "artifact final receipt changed")
    require(load(receipt_file) == {k: v for k, v in finished.items() if k != "receipt_file_sha256"},
            "artifact final event/receipt mismatch")
    raw_file = finished.get("raw_response_file")
    if raw_file:
        require(len(events) == 3 and Path(events[1]["planned_raw_response_file"]).resolve() == Path(raw_file).resolve(),
                "failed or successful response path changed")
        require(raw_hash(raw_file) == finished["response_sha256"], "original response bytes mismatch")
    value = {"id": row["id"], "request_hash": row["request_hash"], "call_status": finished["call_status"],
             "model": finished.get("model"), "usage": finished.get("usage"), "elapsed_ms": finished["elapsed_ms"],
             "response_sha256": finished.get("response_sha256"), "acceptance_violation": None,
             "actual_request_hash": events[1].get("actual_request_hash") if len(events) == 3 else None,
             "transport_binding_status": "failed_original_attempt_verified", "native_claim_certification": False}
    if finished["call_status"] == "ok":
        require(len(events) == 3 and events[1]["status"] == "request_submitted", "successful attempt missing submit")
        join = verify(started, events[1], finished, row["request"], ledger_root, executable, runner_source)
        response = finished["response"]
        require(finished["model"] == response["model"] and finished["usage"] == response["usage"],
                "artifact final model/usage mismatch")
        require(response["model"] == row["expected_actual_model"], "artifact actual model mismatch")
        require(set(response["answers"]) == {"acceptance_violation"}, "artifact question mismatch")
        answer = response["answers"]["acceptance_violation"]
        require(answer["type"] == "noul" and probability(answer["noul"]), "artifact answer type mismatch")
        value.update(acceptance_violation=answer["noul"], model=join["model"],
                     transport_binding_status="original_transport_bytes_verified")
    frozen_row(row)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("prepared", "events", "ledger-root", "executable", "runner-source", "reservation", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--id", required=True)
    args = parser.parse_args()
    rows = read_lines(args.prepared)
    require(len({r["id"] for r in rows}) == len(rows), "duplicate prepared ID")
    row = next(r for r in rows if r["id"] == args.id)
    value = observe(row, read_lines(args.events), args.ledger_root, args.executable, args.runner_source,
                    args.reservation, args.events)
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")
    print(json.dumps({"call_status": value["call_status"], "transport_binding_status": value["transport_binding_status"], "paid_calls": 0}))


if __name__ == "__main__":
    main()
