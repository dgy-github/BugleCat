"""Independent file/transport joins for real, unmodified benchmark candidates."""

import hashlib
import json
from pathlib import Path

from .calibration import digest, load, require


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def seal_check(row):
    require(row.get("artifact_sha256") == digest({k: v for k, v in row.items() if k != "artifact_sha256"}),
            "collection seal mismatch")


def read_lines(path):
    return [json.loads(line) for line in Path(path).read_text(encoding="utf-8-sig").splitlines() if line.strip()]


def verify(prepared, receipt_path, attempt_events, executable):
    seal_check(prepared)
    receipt = load(receipt_path)
    seal_check(receipt)
    require(receipt["schema"] == "buglecat.verification.collected-main.v1", "not collected main receipt")
    require(receipt["call_status"] == "ok", "main call did not produce a candidate")
    require(prepared["schema"] == "buglecat.verification.prepared-main.v1", "not prepared main")
    for field in ("id", "group_id", "task_id", "split", "source_sha256", "protocol_sha256",
                  "authorization_sha256", "request_hash", "settings", "build_info"):
        require(receipt[field] == prepared[field], f"receipt {field} mismatch")
    for field in ("resource_adjustment_sha256", "resource_cohort_id"):
        if field in prepared:
            require(receipt.get(field) == prepared[field], f"receipt {field} mismatch")
    require(receipt.get("resource_cohort") == prepared.get("resource_cohort"), "receipt resource cohort mismatch")
    root = Path(receipt_path).resolve().parent
    request_hash = file_hash(root / "request.json")
    require(request_hash == receipt["request_hash"] == receipt["actual_request_hash"], "actual main request mismatch")
    require(load(root / "request.json") == prepared["body"], "prepared body changed")
    require(file_hash(executable) == prepared["build_info"]["executable_sha256"], "actual main executable changed")
    events = [e for e in attempt_events if e["id"] == receipt["id"]]
    require([e["event"] for e in events] == ["attempt_started", "request_submitted", "attempt_finished"],
            "missing original start/submission/finish or retried main request")
    require(all(e["request_hash"] == request_hash and e["group_id"] == receipt["group_id"]
                and e["build_info"] == receipt["build_info"] for e in events), "original event binding mismatch")
    require(events[1]["actual_request_hash"] == request_hash, "submission body mismatch")
    require(events[2]["receipt_sha256"] == file_hash(receipt_path), "main receipt was changed after transport")
    result = receipt["result"]
    require(file_hash(root / "raw-response.json") == receipt["response_sha256"] == result["response_sha256"],
            "original response bytes changed")
    raw = load(root / "raw-response.json")
    require(raw.get("model") == result["confirmed_model"] == receipt["settings"]["model"],
            "actual main model mismatch")
    require(raw.get("usage") == result["usage"], "actual main usage mismatch")
    require(len(raw["choices"]) == 1 and raw["choices"][0]["message"]["content"] == result["candidate"],
            "candidate is not original model response")
    candidate = (root / "candidate.txt").read_bytes()
    require(candidate.decode() == result["candidate"] and hashlib.sha256(candidate).hexdigest() == result["candidate_sha256"],
            "original candidate text changed")
    log = Path(result["session_log"])
    require(file_hash(log) == result["session_sha256"], "original session changed")
    require(file_hash(log.with_suffix(".sources.jsonl")) == result["sources_sha256"], "original sources changed")
    messages = read_lines(log)
    require(messages[-1]["role"] == "assistant" and messages[-1]["content"] == result["candidate"],
            "original assistant not found at source position")
    require(file_hash(root / "packet.json") == result["packet_sha256"], "native packet bytes changed")
    packet = load(root / "packet.json")
    require(packet["candidate"] == result["candidate"] and packet["claims"] == result["claims"], "native candidate/claims changed")
    require(result["host_B"]["policy_version"] == prepared["build_info"]["policy_version"],
            "native host policy mismatch")
    require(len({c["id"] for c in packet["claims"]}) == len(packet["claims"]), "duplicate native claim ID")
    for claim in packet["claims"]:
        begin, end = claim["start"], claim["end"]
        require(type(begin) is int and type(end) is int and 0 <= begin < end <= len(candidate), "native claim span bounds")
        require(candidate[begin:end].decode() == claim["text"], "native claim span mismatch")
    require(not result["agent_loop_executed"] and not result["assertions_executed"]
            and result["tools_executed"] == 0, "single response scope was misreported")
    return receipt
