"""Prediction-blind assertion review of original benchmark responses.

Native host extraction decides eligibility. A separate explicit scope review
must bind the exact claim; this module never invents a positive statement.
"""

import argparse
import hashlib
import json
import re
from pathlib import Path

from .benchmark_oracle import audit
from .calibration import create, digest, load, require
from .collection_receipts import read_lines, verify
from .source_bindings import bind_prepared, check_unchanged, verify_manifest


def code_block(candidate):
    fences = re.findall(r"(?m)^```([^\n]*)\n(.*?)^```[ \t]*$", candidate, re.DOTALL)
    require(len(fences) == 1 and fences[0][0].strip().lower() in {"python", "py"},
            "one original Python fence required; otherwise Unknown")
    return fences[0][1]


def source_baseline(manifest):
    tasks, bindings = verify_manifest(manifest)
    rows = []
    for case in manifest["cases"]:
        task = tasks[case["task_id"]]
        receipt = audit(task, task["code"])
        check_unchanged(bindings)
        rows.append({"id": case["id"], "group_id": case["group_id"],
                     "task_id": task["task_id"], "result": receipt["result"],
                     "task_sha256": digest(task), "oracle_worker_sha256": receipt["oracle_worker_sha256"]})
    return {"schema": "buglecat.verification.benchmark-source-baseline.v1",
            "purpose": "oracle admission only; no main-model candidate or label exists",
            "manifest_sha256": digest(manifest), "paid_calls": 0, "cases": rows}


def audit_candidate(task, candidate):
    try:
        code = code_block(candidate)
        receipt = audit(task, code)
    except ValueError:
        receipt = {"schema": "buglecat.verification.benchmark-assertion-receipt.v1",
                   "task_id": task["task_id"], "task_sha256": digest(task),
                   "result": {"status": "unknown", "reason": "ambiguous_code_fences"},
                   "independent_of_jev": True, "paid_attempts": 0}
    receipt["candidate_sha256"] = hashlib.sha256(candidate.encode()).hexdigest()
    return {**receipt, "artifact_hash": digest(receipt)}


def review_collection(directory, executable, output):
    directory = Path(directory).resolve()
    manifest = load(directory / "collection-manifest.json")
    tasks, bindings = verify_manifest(manifest)
    prepared = read_lines(directory / "prepared.jsonl")
    events = read_lines(directory / "attempt-events.jsonl")
    rows = []
    require(len({r["id"] for r in prepared}) == len(prepared), "duplicate prepared case")
    require({r["id"] for r in prepared} == {r["id"] for r in manifest["cases"]}, "prepared case set mismatch")
    for original in prepared:
        bind_prepared(manifest, original)
        receipt_path = directory / original["id"] / "receipt.json"
        if not receipt_path.exists():
            rows.append({"id": original["id"], "status": "not_finished", "label": "unlabeled"})
            continue
        receipt = load(receipt_path)
        if receipt["call_status"] != "ok":
            rows.append({"id": original["id"], "status": receipt["call_status"], "label": "unknown"})
            continue
        receipt = verify(original, receipt_path, events, executable)
        result = receipt["result"]
        assertion = audit_candidate(tasks[original["task_id"]], result["candidate"])
        check_unchanged(bindings)
        # A new oracle version produces an additional immutable receipt; it
        # must not overwrite an earlier worker's acceptance facts.
        oracle_path = directory / original["id"] / f"independent-assertion-{assertion['artifact_hash']}.json"
        if oracle_path.exists():
            require(load(oracle_path) == assertion, "original assertion receipt changed")
        else:
            create(oracle_path, assertion)
        decisions = {d["claim_id"]: d for d in result["host_B"]["decisions"]}
        eligible = [c for c in result["claims"] if c["kind"] == "semantic"
                    and c["context"] == "narrative" and c["polarity"] == "positive"
                    and decisions[c["id"]]["verdict"] == "unknown"]
        rows.append({"id": original["id"], "task_id": original["task_id"],
                     "group_id": original["group_id"], "split": original["split"],
                     "candidate_sha256": result["candidate_sha256"],
                     "session_sha256": result["session_sha256"], "sources_sha256": result["sources_sha256"],
                     "receipt_path": str(receipt_path), "receipt_sha256": hashlib.sha256(receipt_path.read_bytes()).hexdigest(),
                     "oracle_path": str(oracle_path), "oracle_artifact_hash": assertion["artifact_hash"],
                     "acceptance_result": assertion["result"], "eligible_native_claims": eligible,
                     "status": "original_transport_verified_and_assertions_audited",
                     "label": "unlabeled", "review_status": "pending_scope_review",
                     "model": result["confirmed_model"], "usage": result["usage"]})
    value = {"schema": "buglecat.verification.benchmark-review-queue.v1", "cases": rows,
             "scores_hidden": True, "jev_probabilities_read": False,
             "purpose": "independent assertion facts; exact original claim scope still needs review"}
    check_unchanged(bindings)
    create(output, {**value, "artifact_hash": digest(value)})
    print(json.dumps({"cases": len(rows), "original_transport_verified": sum("acceptance_result" in r for r in rows),
                      "eligible_native_claims": sum(len(r.get("eligible_native_claims", [])) for r in rows),
                      "jev_calls": 0}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    base = sub.add_parser("source-baseline")
    base.add_argument("--manifest", type=Path, required=True)
    base.add_argument("--output", type=Path, required=True)
    review = sub.add_parser("collection")
    review.add_argument("--directory", type=Path, required=True)
    review.add_argument("--executable", type=Path, required=True)
    review.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.action == "source-baseline":
        value = source_baseline(load(args.manifest))
        create(args.output, value)
        print(f"source groups {len(value['cases'])}; passed {sum(r['result']['status'] == 'passed' for r in value['cases'])}; calls 0")
    else:
        review_collection(args.directory, args.executable, args.output)


if __name__ == "__main__":
    main()
