"""Bind reviewed original claims and recorded Rust replay requests to scoring.

Never generates a label or a probability. The scope interpretation is a frozen
Codex review backed by a separate executable receipt, not a human attestation.
"""

import argparse
import hashlib
import json
from pathlib import Path

from .calibration import DATA_SCHEMA, create, digest, load, require


def raw_hash(value):
    return hashlib.sha256(value).hexdigest()


def jsonl(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines() if line.strip()]


def sealed_packet(row):
    body = {k: v for k, v in row.items() if k != "artifact_sha256"}
    require(row.get("artifact_sha256") == digest(body), "prepared packet seal mismatch")
    require(row.get("schema") == "buglecat.verification.prepared-replay.v1", "packet schema")
    require(row.get("status") == "prepared" and row.get("candidate_mode") == "original_transcript", "not original prepared candidate")
    require(row.get("eligible_semantic") is True, "no eligible semantic claim")
    require(row.get("request_hash") == digest(row.get("request")), "request hash mismatch")
    require(row.get("packet_redacted_sha256") == digest(row.get("packet")), "redacted packet mismatch")
    require(row["request"]["state"] == row["state"], "state/request mismatch")
    claim = next(c for c in row["packet"]["claims"] if c["id"] == row["selected_claim_id"])
    require(claim["context"] == "narrative" and claim["polarity"] == "positive" and claim["kind"] == "semantic", "not positive narrative semantic")
    candidate = row["packet"]["candidate"]
    require(raw_hash(candidate.encode()) == row["candidate_sha256"], "candidate hash mismatch")
    require(candidate.encode()[claim["start"]:claim["end"]].decode() == claim["text"], "claim span mismatch")
    require(row["state"]["claim"] == claim["text"], "request claim mismatch")
    return claim


def prepare(packets, reviewed, selections):
    source = {c["id"]: c for c in reviewed["cases"]}
    selected = {c["id"]: c for c in selections["cases"]}
    require(len(selected) == len(selections["cases"]), "duplicate selection")
    rows = {r["id"]: r for r in packets}
    require(len(rows) == len(packets), "duplicate packet")
    cases, kept, groups = [], [], set()
    for identity, interpretation in selected.items():
        row, label = rows[identity], source[identity]
        claim = sealed_packet(row)
        require(label["source_sha256"] == row["source_sha256"] and label["candidate_sha256"] == row["candidate_sha256"], "review source/candidate mismatch")
        require(label["group_id"] == row["group_id"], "group mismatch")
        require(row["group_id"] not in groups, "one claim per original group")
        groups.add(row["group_id"])
        require(label["review_status"] == "assertion_reviewed" and label["label"] in {"supported", "unsupported"}, "missing executable label")
        require(interpretation.get("reviewer_kind") == "model" and bool(interpretation.get("scope_basis")), "missing honest scope review")
        require(interpretation["claim_sha256"] == raw_hash(claim["text"].encode()), "scope interpretation changed")
        require(interpretation["assertion_receipt_sha256"] == label["assertion_receipt_sha256"], "assertion receipt mismatch")
        require(row["host_B"]["claim_verdict"] == "unknown", "already decisive rules: no incremental call needed")
        cases.append({"id": identity, "group_id": row["group_id"], "split": "development",
                      "category": "semantic", "label": label["label"], "label_basis": interpretation["scope_basis"],
                      "review_status": "assertion_reviewed", "interpretation_review_status": "model_reviewed",
                      "human_reviewed": False, "author_exposed": True, "context": "narrative", "polarity": "positive",
                      "source_kind": "real_main_model_author_designed_development_task", "source_sha256": row["source_sha256"],
                      "candidate_hash": row["candidate_sha256"], "input_hash": row["input_hash"],
                      "request_hash": row["request_hash"], "claim_sha256": interpretation["claim_sha256"],
                      "policy_version": row["policy_version"], "template_version": row["template_version"],
                      "assertion_receipt_sha256": label["assertion_receipt_sha256"],
                      "input_binding_status": "recorded_current_rust_prepared_request", "prepared_packet_sha256": row["artifact_sha256"]})
        kept.append(row)
    require(bool(cases), "empty selection")
    return {"schema": DATA_SCHEMA, "cases": cases, "purpose": "source-bound development exploration; no blind certification"}, kept


def observe(dataset, events, allow_unrecorded_actual_hash=False):
    expected = {c["id"]: c for c in dataset["cases"]}
    if allow_unrecorded_actual_hash:
        require(all(c["split"] == "development" for c in expected.values()), "legacy receipt only allowed in development")
    starts, submitted, finished = {}, {}, {}
    for event in events:
        identity = event.get("id")
        if identity not in expected:
            raise ValueError("foreign attempt event")
        status = event.get("status")
        require(status in {"attempt_started", "request_submitted", "attempt_finished"}, "unexpected attempt status")
        case = expected[identity]
        for field in ("request_hash", "candidate_hash", "input_hash", "group_id", "policy_version", "template_version"):
            require(event[field] == case[field], f"attempt {field} mismatch")
        target = {"attempt_started": starts, "request_submitted": submitted, "attempt_finished": finished}[status]
        require(identity not in target, "duplicate attempt event: no retries")
        if status == "request_submitted":
            require(identity in starts, "submission before durable attempt")
            actual_hash = event.get("actual_request_hash")
            if actual_hash is None:
                require(allow_unrecorded_actual_hash, "actual transport hash unrecorded")
            else:
                require(actual_hash == case["request_hash"], "actual transport request mismatch")
        elif status == "attempt_finished":
            require(identity in starts, "result without attempt")
        target[identity] = event
    require(starts.keys() == expected.keys(), "missing attempts")
    require(finished.keys() == expected.keys(), "unfinished attempt cannot silently disappear")
    result = []
    for identity, case in expected.items():
        end = finished[identity]
        response = end.get("response", {})
        if end["call_status"] == "ok":
            require(identity in submitted and end.get("response_sha256"), "successful result missing transport receipt")
            require(response.get("model") == end.get("model") and response.get("usage") == end.get("usage"), "response metadata mismatch")
        else:
            require(not response, "failed attempt cannot carry successful response")
        result.append({"id": identity, "rules_verdict": "unknown", "input_hash": case["input_hash"],
                       "candidate_hash": case["candidate_hash"], "request_hash": case["request_hash"],
                       "policy_version": end["policy_version"], "template_version": end["template_version"],
                       "actual_request_hash": submitted.get(identity, {}).get("actual_request_hash"),
                       "transport_binding_status": "actual_transport_hash_recorded" if submitted.get(identity, {}).get("actual_request_hash") else "actual_transport_hash_unrecorded_development_only",
                       "call_status": end["call_status"], "model": end.get("model"),
                       "usage": end.get("usage"), "elapsed_ms": end.get("elapsed_ms"),
                       "questions": [{"name": name, "kind": "noul", "noul": answer["noul"]}
                                     for name, answer in response.get("answers", {}).items()]})
    return result


def write_lines(path, rows):
    with Path(path).open("x", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False, separators=(",", ":"), allow_nan=False) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    prep = sub.add_parser("prepare")
    for name in ("packets", "reviewed", "selections", "dataset", "selected-packets"):
        prep.add_argument(f"--{name}", type=Path, required=True)
    obs = sub.add_parser("observe")
    for name in ("dataset", "events", "output"):
        obs.add_argument(f"--{name}", type=Path, required=True)
    obs.add_argument("--development-allow-unrecorded-actual-hash", action="store_true")
    args = parser.parse_args()
    if args.action == "prepare":
        require(not args.dataset.exists() and not args.selected_packets.exists(), "new outputs required")
        data, kept = prepare(jsonl(args.packets), load(args.reviewed), load(args.selections))
        create(args.dataset, data)
        write_lines(args.selected_packets, kept)
        print(json.dumps({"selected_claims": len(kept), "groups": len(data["cases"]), "paid_attempts": 0}))
    else:
        result = observe(load(args.dataset), jsonl(args.events), args.development_allow_unrecorded_actual_hash)
        write_lines(args.output, result)
        print(json.dumps({"observations": len(result), "successful": sum(r["call_status"] == "ok" for r in result)}))


if __name__ == "__main__":
    main()
