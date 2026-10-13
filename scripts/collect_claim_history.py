"""Capture original local Agent turns for blind review, without any model call.

Thread-store records are legacy evidence, not authenticated current host packets.
Outputs are private: preserve original text but never print it or publish it.
"""
import argparse
import hashlib
import json
import sqlite3
from collections import Counter
from pathlib import Path

from verification_readiness_inventory import thread_inventory


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True).encode("utf-8")


def capture_host_sources(raw, allowed, output):
    """Retain real event stores using SQLite backup (including committed WAL).

    Presence is not proof of claim binding/freshness. Do not promote admission.
    """
    sources = []
    databases = {}
    for identity, thread in json.loads(raw)["threads"].items():
        thread_hash = sha(identity.encode())
        if thread_hash not in allowed:
            continue
        # IDs become filenames only after traversal/separator validation.
        if not identity or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in identity):
            continue
        workspace = thread.get("metadata", {}).get("workspace", "")
        if not workspace or not Path(workspace).is_absolute():
            continue
        directory = Path(workspace) / ".nanocodex/sessions"
        retained = []
        for stem in (identity, "thread-" + identity):
            for suffix in (".jsonl", ".sources.jsonl"):
                source = directory / (stem + suffix)
                if source.is_file():
                    content = source.read_bytes()
                    name = sha(content) + suffix
                    destination = output / name
                    if not destination.exists():
                        destination.write_bytes(content)
                    retained.append({"file": name, "sha256": sha(content), "kind": suffix})
        database = directory / "events.sqlite3"
        database_file = None
        if database.is_file():
            database_key = str(database.resolve())
            if database_key not in databases:
                name = "events-" + sha(database_key.encode()) + ".sqlite3"
                target = output / name
                resolved = str(database.resolve())
                if resolved.startswith("\\\\?\\"):
                    resolved = resolved[4:]
                with sqlite3.connect(Path(resolved).as_uri() + "?mode=ro", uri=True) as origin:
                    with sqlite3.connect(target) as destination:
                        origin.backup(destination)
                        if destination.execute("PRAGMA quick_check").fetchone() != ("ok",):
                            raise ValueError("event snapshot integrity check failed")
                databases[database_key] = {"file": name, "sha256": sha(target.read_bytes())}
            database_file = databases[database_key]
        sources.append({"thread_hash": thread_hash, "session_sources": retained,
                        "event_snapshot": database_file, "claim_binding_verified": False})
    return sources


def extract(raw, allowed):
    store = json.loads(raw)
    rows = []
    for thread_id, thread in store["threads"].items():
        thread_hash = sha(thread_id.encode())
        if thread_hash not in allowed:
            continue
        workspace = thread.get("metadata", {}).get("workspace", "")
        # Conservative grouping: several conversations about one workspace do
        # not establish independent tasks. Review may refine, never silently split.
        family = sha(workspace.casefold().replace("\\\\?\\", "").replace("\\", "/").encode())
        for turn_index, turn in enumerate(thread.get("turns", [])):
            items = turn.get("items", [])
            candidates = [i for i, item in enumerate(items)
                          if item.get("type") == "assistantMessage" and isinstance(item.get("text"), str)
                          and item["text"].strip()]
            if not candidates:
                continue
            index = candidates[-1]
            candidate = items[index]["text"]
            evidence = [item for item in items[:index] if item.get("type") in {"toolCall", "toolResult"}]
            if not any(item.get("type") == "toolResult" for item in evidence):
                continue
            identity = sha(encoded([thread_hash, turn_index, candidate]))
            rows.append({"id": identity, "thread_hash": thread_hash, "turn_index": turn_index,
                         "task_family_provisional": family, "candidate": candidate,
                         "candidate_sha256": sha(candidate.encode()), "candidate_item_index": index,
                         "turn_status": turn.get("status"), "original_turn": turn,
                         "preceding_tool_items": evidence, "turn_sha256": sha(encoded(turn)),
                         "evidence_sha256": sha(encoded(evidence)),
                         "source_sha256": sha(raw), "source_kind": "original_local_thread_store",
                         "raw_candidate_unedited": True})
    return rows


def collect(source, output):
    # Read once; both admission and extraction must refer to the same snapshot.
    raw = source.read_bytes()
    output.mkdir(parents=True, exist_ok=False)
    snapshot = output / "original-thread-store.json"
    snapshot.write_bytes(raw)
    inventory = thread_inventory(snapshot)
    allowed = {r["thread_hash"] for r in inventory["rows"] if r["category"] == "unreviewed_candidate"}
    host_sources = capture_host_sources(raw, allowed, output)
    originals = extract(raw, allowed)
    counts = Counter()
    queue = []
    for row in originals:
        target = output / (row["id"] + ".json")
        target.write_bytes(encoded(row))
        counts[row["task_family_provisional"]] += 1
        queue.append({key: row[key] for key in ("id", "thread_hash", "turn_index", "task_family_provisional",
                                               "candidate_sha256", "turn_sha256", "evidence_sha256", "source_sha256")})
        queue[-1].update({"original_file": target.name, "original_file_sha256": sha(target.read_bytes()),
                          "split": "development", "label": "unlabeled", "review_status": "pending",
                          "host_provenance": "not_established", "execution_time_file_snapshots": "not_established",
                          "formal_eligible": False, "eligibility_reason": "legacy_source_requires_provenance_and_exposure_review",
                          "assertion_receipt": None, "scope_review": None,
                          "human_reviewed": False, "jev_score_read": False})
    report = {"schema": "buglecat.verification.claim-history-intake.v1", "source_sha256": sha(raw),
              "selected_threads": len(allowed), "captured_turns": len(originals),
              "provisional_workspace_families": len(counts), "formal_admitted_groups": 0,
              "labels_from_exit_code_alone": False, "calibration_started": False, "holdout_opened": False,
              "paid_calls": 0, "cases": queue}
    report["host_sources"] = host_sources
    (output / "blind-review-queue.json").write_bytes(encoded(report))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--thread-store", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = collect(args.thread_store, args.output)
    summary = {k: v for k, v in result.items() if k not in {"cases", "host_sources"}}
    summary["threads_with_session_sources"] = sum(bool(r["session_sources"]) for r in result["host_sources"])
    summary["threads_with_event_snapshot"] = sum(r["event_snapshot"] is not None for r in result["host_sources"])
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
