"""Prediction-blind source checks for preserved history; never guess semantic labels."""
import argparse
import hashlib
import json
from collections import Counter
from pathlib import Path


def verified(root, name, expected):
    path = (root / name).resolve()
    if path.parent != root.resolve():
        raise ValueError("source outside intake directory")
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError("source hash changed")
    return raw


def audit(directory):
    queue = json.loads((directory / "blind-review-queue.json").read_bytes())
    verified(directory, "original-thread-store.json", queue["source_sha256"])
    hosts = {}
    for host in queue["host_sources"]:
        texts = Counter()
        for source in host["session_sources"]:
            raw = verified(directory, source["file"], source["sha256"])
            if source["kind"] != ".jsonl":
                continue
            for line in raw.decode("utf-8").splitlines():
                if not line.strip():
                    continue
                message = json.loads(line)
                if message.get("role") == "assistant" and isinstance(message.get("content"), str):
                    texts[message["content"]] += 1
        if host["event_snapshot"]:
            db = host["event_snapshot"]
            verified(directory, db["file"], db["sha256"])
        hosts[host["thread_hash"]] = texts
    rows = []
    for case in queue["cases"]:
        original = json.loads(verified(directory, case["original_file"], case["original_file_sha256"]))
        text = original["candidate"]
        if hashlib.sha256(text.encode()).hexdigest() != case["candidate_sha256"]:
            raise ValueError("candidate hash mismatch")
        evidence = original["preceding_tool_items"]
        calls = Counter(i["id"] for i in evidence if i.get("type") == "toolCall" and "id" in i)
        results = [i for i in evidence if i.get("type") == "toolResult"]
        unbound = sum(calls.get(i.get("callId"), 0) != 1 for i in results)
        rows.append({"id": case["id"], "candidate_sha256": case["candidate_sha256"],
                     "source_sha256": case["source_sha256"],
                     "original_session_exact_text_matches": hosts.get(case["thread_hash"], {}).get(text, 0),
                     "tool_results": len(results), "unbound_or_ambiguous_tool_results": unbound,
                     "tool_success_flags": dict(Counter(str(i.get("success")) for i in results)),
                     "label": "unlabeled", "reviewer_kind": "executable_source_integrity_only",
                     "semantic_scope_review_required": True, "formal_eligible": False})
    return {"schema": "buglecat.verification.claim-history-source-audit.v1", "cases": rows,
            "candidate_count": len(rows), "session_text_matched_candidates": sum(r["original_session_exact_text_matches"] > 0 for r in rows),
            "candidates_with_all_tool_ids_bound": sum(r["unbound_or_ambiguous_tool_results"] == 0 for r in rows),
            "formal_admitted_groups": 0, "jev_scores_read": False, "paid_calls": 0,
            "limitation": "Matching saved text and call IDs does not establish truthful completion, independent groups, or contemporaneous file freshness."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = audit(args.directory)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=False, indent=2)
    print(json.dumps({k: v for k, v in result.items() if k != "cases"}))


if __name__ == "__main__":
    main()
