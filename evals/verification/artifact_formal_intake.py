"""Zero-network intake of source-preregistered, original code artifacts.

Legacy viewed responses fail because their actual start has no cohort binding.
This executes independent assertions, never a main model or a Jev request.
"""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path

from .artifact_acceptance import POLICY, TEMPLATE, request
from .artifact_observations import frozen_row
from .artifact_scoring import sealed
from .benchmark_review import audit_candidate, code_block
from .budget_authorization import resolve
from .calibration import create, digest, load, require
from .collection_receipts import read_lines, verify
from .source_bindings import bind_prepared, check_unchanged, source_transaction, verify_manifest

PROFILE_FIELDS = ("main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash")


def profile_bindings(value, registered):
    if not any(field in value or field in registered for field in PROFILE_FIELDS):
        return {}
    require(all(field in value and field in registered and value[field] == registered[field] for field in PROFILE_FIELDS),
            "main resource profile binding mismatch")
    from .artifact_profile_cohort import validate_profile
    profile, bindings = validate_profile(value["main_resource_profile_file"])
    require(all(hashlib.sha256(Path(binding["file"]).read_bytes()).hexdigest() == binding["sha256"] for binding in bindings),
            "resource profile diagnostic bytes changed")
    require(profile["artifact_hash"] == value["main_resource_profile_hash"]
            and hashlib.sha256(Path(value["main_resource_profile_file"]).read_bytes()).hexdigest() == value["main_resource_profile_sha256"]
            and profile["main_collection_settings"] == registered["main_collection_settings"],
            "original resource profile changed")
    return {field: value[field] for field in PROFILE_FIELDS}


def cohort(path, split="calibration"):
    value = sealed(load(path))
    require(value["schema"] == "buglecat.verification.artifact-cohort.v1"
            and value["registration_kind"] == "collection_preregistration"
            and value["status"] == "registered" and value["formal_registration"] is True
            and value["full_campaign_source_gate"] is True,
            "formal source cohort is not registered")
    require(value["split"] == split and value["before_main_requests"] is True
            and value["scores_hidden"] is True and value["jev_probabilities_read"] is False
            and value["author_exposed"] is False, "fresh candidate prerequisites not met")
    for binding in value["bindings"]:
        require(hashlib.sha256(Path(binding["file"]).read_bytes()).hexdigest() == binding["sha256"],
                "formal cohort bound file changed")
    profile_bindings(value, value)
    return value


def ledger_start(row, cohort_path, *, optional=False):
    chain = resolve(row["authorization_file"])
    ledger = Path(chain["authorization"]["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as db:
        metadata = dict(db.execute("SELECT key,value FROM metadata"))
        found = list(db.execute("SELECT started_json FROM attempts WHERE group_id=?", (row["group_id"],)))
    require(metadata["authorization_sha256"] == chain["ledger_authorization_sha256"], "original main ledger authorization changed")
    if optional and not found:
        return None
    require(len(found) == 1, "one original main start required")
    start = json.loads(found[0][0])
    require(start["authorization_sha256"] in chain["allowed_sha256"], "foreign authorization start")
    current = cohort(cohort_path, row["split"])
    # A merely prepared old cohort may later have a fresh start in a distinct
    # profile. That newer paid attempt is not retroactively an old-cohort start.
    if optional and start.get("artifact_cohort_hash") != current["artifact_hash"]:
        return None
    profile_bindings(row, current)
    profile_bindings(start, current)
    require(start.get("artifact_cohort_sha256") == hashlib.sha256(Path(cohort_path).read_bytes()).hexdigest()
            and start.get("artifact_cohort_hash") == current["artifact_hash"],
            "original main start lacks preregistered cohort")
    require(start["id"] == row["id"] and start["request_hash"] == row["request_hash"], "main ledger identity mismatch")
    return start


def finish_proof(row, receipt_path, start):
    require(start is not None, "receipt has no original main start")
    chain = resolve(row["authorization_file"])
    ledger = Path(chain["authorization"]["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as db:
        found = list(db.execute("SELECT finished_json FROM attempts WHERE group_id=?", (row["group_id"],)))
    require(len(found) == 1 and found[0][0] is not None, "main receipt has no original ledger finish")
    finish = json.loads(found[0][0])
    receipt = sealed(load(receipt_path), "artifact_sha256")
    require(finish["receipt_sha256"] == hashlib.sha256(Path(receipt_path).read_bytes()).hexdigest()
            and finish["call_status"] == receipt["call_status"] and finish["id"] == row["id"],
            "original main finish/status bytes mismatch")
    return receipt


def verify_main_origin(original, receipt_path):
    """Rejoin the compiled main entrypoint and every original transport byte."""
    root = Path(__file__).resolve().parents[2]
    profile = "main_resource_profile_hash" in original
    holdout = original["split"] == "holdout"
    require(not holdout or profile, "holdout compiled entry requires registered resource profile")
    stem = ("verification_artifact_holdout_collect" if holdout else
            "verification_artifact_profile_collect" if profile else "verification_artifact_collect")
    source = root / f"rust/crates/ncx-core/examples/{stem}.rs"
    executable = root / f"rust/target/debug/examples/{stem}.exe"
    sources = {
        "runner_source_sha256": source,
        "support_source_sha256": source.parent / stem / "support.rs",
        "legacy_support_source_sha256": source.parent / "verification_collect/support.rs",
        "request_builder_source_sha256": root / "rust/crates/ncx-provider/src/request.rs",
        "response_parser_source_sha256": root / "rust/crates/ncx-provider/src/response.rs",
        "executable_sha256": executable,
    }
    if profile:
        sources.update({"collection_engine_source_sha256": source.parent / stem / "engine.rs",
                        "frozen_collector_support_sha256": source.parent / "verification_artifact_collect/support.rs"})
    if holdout:
        from .artifact_formal_observations import holdout_owner_sources
        sources.update(holdout_owner_sources(source.parent))
        sources["profile_collector_validator_source_sha256"] = source.parent / "verification_artifact_profile_collect/support.rs"
    require(all(original["build_info"].get(field) == hashlib.sha256(path.read_bytes()).hexdigest()
                for field, path in sources.items()), "original main compiled source/runner changed")
    events = read_lines(Path(receipt_path).parent.parent / "attempt-events.jsonl")
    return verify(original, receipt_path, events, executable)


@source_transaction
def prepare(directory, executable, cohort_path):
    directory, cohort_path = Path(directory).resolve(), Path(cohort_path).resolve()
    manifest_file = directory / "collection-manifest.json"
    manifest = load(manifest_file)
    split = load(cohort_path)["split"]
    require(split in {"calibration", "holdout"}, "formal collection split required")
    if split == "holdout":
        from .artifact_holdout import validate
        registered = validate(cohort_path)
    else:
        registered = cohort(cohort_path)
    profile = profile_bindings(manifest, registered)
    tasks, bindings = verify_manifest(manifest)
    cohort_raw = hashlib.sha256(cohort_path.read_bytes()).hexdigest()
    for field, expected in (("artifact_cohort_sha256", cohort_raw), ("artifact_cohort_hash", registered["artifact_hash"])):
        require(manifest.get(field) == expected, "collection has no preregistered artifact source")
    originals = read_lines(directory / "prepared.jsonl")
    events = read_lines(directory / "attempt-events.jsonl")
    require(len({row["id"] for row in originals}) == len(originals)
            and {row["id"] for row in originals} == {case["id"] for case in manifest["cases"]},
            "formal original prepared set mismatch")
    source_cases = {case["id"]: case for case in registered["cases"]}
    protocol_file = Path(manifest["protocol_file"])
    spec = load(protocol_file)
    require(spec["policy_version"] == POLICY and spec["template_version"] == TEMPLATE,
            "formal artifact protocol mismatch")
    bindings += [(str(manifest_file), hashlib.sha256(manifest_file.read_bytes()).hexdigest()),
                 (str(cohort_path), cohort_raw)]
    cases, prepared = [], []
    for original in originals:
        bind_prepared(manifest, original)
        profile_bindings(original, registered)
        source_case = source_cases.get(original["id"])
        require(source_case is not None and source_case["group_id"] == original["group_id"]
                and source_case["task_id"] == original["task_id"], "candidate outside preregistered source")
        for field, expected in (("artifact_cohort_sha256", cohort_raw), ("artifact_cohort_hash", registered["artifact_hash"])):
            require(original.get(field) == expected, "main prepared cohort mismatch")
        receipt_path = directory / original["id"] / "receipt.json"
        base = {"id": original["id"], "group_id": original["group_id"], "task_id": original["task_id"],
                "split": original["split"], "category": "artifact_acceptance", "author_exposed": False,
                "source_kind": "source_preregistered_original_model_code", "artifact_cohort_sha256": cohort_raw,
                "artifact_cohort_hash": registered["artifact_hash"], **profile,
                "collection_manifest_file": str(manifest_file),
                "collection_manifest_sha256": hashlib.sha256(manifest_file.read_bytes()).hexdigest(),
                "main_prepared_artifact_sha256": original["artifact_sha256"],
                "main_receipt_file": str(receipt_path)}
        start = ledger_start({**original, "authorization_file": manifest["authorization_file"]}, cohort_path, optional=True)
        if not receipt_path.exists():
            cases.append({**base, "label": "unknown", "call_eligible": False,
                          "reason": "main_not_finished" if start else "main_not_started"})
            continue
        receipt = finish_proof({**original, "authorization_file": manifest["authorization_file"]}, receipt_path, start)
        profile_bindings(receipt, registered)
        base["main_receipt_sha256"] = hashlib.sha256(receipt_path.read_bytes()).hexdigest()
        # Failure and pending records are retained but never given binary labels.
        if receipt["call_status"] != "ok":
            cases.append({**base, "label": "unknown", "call_eligible": False, "reason": receipt["call_status"]})
            continue
        require(receipt.get("artifact_cohort_sha256") == cohort_raw
                and receipt.get("artifact_cohort_hash") == registered["artifact_hash"], "main final cohort mismatch")
        ledger_start({**original, "authorization_file": manifest["authorization_file"]}, cohort_path)
        verify(original, receipt_path, events, executable)
        result, task = receipt["result"], tasks[original["task_id"]]
        oracle = audit_candidate(task, result["candidate"])
        check_unchanged(bindings)
        oracle_file = receipt_path.with_name(f"formal-artifact-oracle-{oracle['artifact_hash']}.json")
        if oracle_file.exists():
            require(load(oracle_file) == oracle, "formal oracle immutable receipt changed")
        else:
            create(oracle_file, oracle)
        label = {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"], "unknown")
        case = {**base, "label": label, "call_eligible": label != "unknown", "oracle": oracle,
                "candidate_sha256": result["candidate_sha256"],
                "main_receipt_sha256": hashlib.sha256(receipt_path.read_bytes()).hexdigest()}
        cases.append(case)
        if label == "unknown":
            continue
        code = code_block(result["candidate"])
        body = request(task, source_case["public_interface"], code)
        record = {"schema": "buglecat.verification.prepared-artifact.v1", **base,
                  "statement_origin": "host_artifact_hypothesis", "candidate_sha256": result["candidate_sha256"],
                  "code_sha256": hashlib.sha256(code.encode()).hexdigest(), "request": body, "request_hash": digest(body),
                  "native_policy_version": result["host_B"]["policy_version"], "policy_version": POLICY,
                  "template_version": TEMPLATE, "expected_actual_model": spec["model"],
                  "collection_manifest_file": str(manifest_file), "collection_manifest_sha256": bindings[-2][1],
                  "main_receipt_file": str(receipt_path), "main_receipt_sha256": case["main_receipt_sha256"],
                  "oracle_receipt_file": str(oracle_file), "oracle_receipt_sha256": hashlib.sha256(oracle_file.read_bytes()).hexdigest(),
                  "oracle_sha256": oracle["artifact_hash"], "artifact_cohort_file": str(cohort_path)}
        for name in ("source", "protocol", "authorization"):
            record[f"{name}_file"] = manifest[f"{name}_file"]
            record[f"{name}_sha256"] = manifest[f"{name}_sha256"]
        prepared.append({**record, "artifact_sha256": digest(record)})
    check_unchanged(bindings)
    data = {"schema": "buglecat.verification.artifact-formal-dataset.v1", "cases": cases,
            "artifact_cohort_file": str(cohort_path), "artifact_cohort_sha256": cohort_raw,
            "artifact_cohort_hash": registered["artifact_hash"], **profile, "scores_hidden": True,
            "jev_probabilities_read": False, "new_paid_calls": 0, "native_claim_certification": False}
    return {**data, "artifact_hash": digest(data)}, prepared


def verify_prepared(row):
    registered = cohort(row["artifact_cohort_file"], row["split"])
    require(row["artifact_cohort_hash"] == registered["artifact_hash"]
            and row["artifact_cohort_sha256"] == hashlib.sha256(Path(row["artifact_cohort_file"]).read_bytes()).hexdigest(),
            "formal prepared source registration changed")
    frozen_row(row, row["split"])
    original = next(value for value in read_lines(Path(row["collection_manifest_file"]).with_name("prepared.jsonl"))
                    if value["id"] == row["id"])
    ledger_start({**original, "authorization_file": row["authorization_file"]}, row["artifact_cohort_file"])
    verify_main_origin(original, row["main_receipt_file"])
    return registered


@source_transaction
def verify_dataset_cases(data):
    """Every actual start remains represented; Unknown cannot hide valid labels."""
    registered = cohort(data["artifact_cohort_file"], load(data["artifact_cohort_file"])["split"])
    profile_bindings(data, registered)
    source_cases = {case["id"]: case for case in registered["cases"]}
    require(len({case["id"] for case in data["cases"]}) == len(data["cases"])
            and len({case["group_id"] for case in data["cases"]}) == len(data["cases"]), "duplicate formal dataset source")
    for case in data["cases"]:
        require(case["id"] in source_cases and case["group_id"] == source_cases[case["id"]]["group_id"],
                "dataset case outside registered source")
        manifest = load(case["collection_manifest_file"])
        require(hashlib.sha256(Path(case["collection_manifest_file"]).read_bytes()).hexdigest() == case["collection_manifest_sha256"],
                "dataset original manifest changed")
        originals = read_lines(Path(case["collection_manifest_file"]).with_name("prepared.jsonl"))
        original = next(row for row in originals if row["id"] == case["id"])
        bind_prepared(manifest, original)
        require(original["artifact_sha256"] == case["main_prepared_artifact_sha256"], "dataset prepared origin changed")
        row = {**original, "authorization_file": manifest["authorization_file"]}
        start = ledger_start(row, data["artifact_cohort_file"], optional=True)
        receipt_file = Path(case["main_receipt_file"])
        require(receipt_file.resolve() == Path(case["collection_manifest_file"]).parent / case["id"] / "receipt.json",
                "dataset original receipt path changed")
        if not receipt_file.exists():
            require(case["label"] == "unknown" and case["call_eligible"] is False
                    and case["reason"] == ("main_not_finished" if start else "main_not_started"),
                    "unfinished attempt cannot become binary")
            continue
        receipt = finish_proof(row, receipt_file, start)
        require(hashlib.sha256(receipt_file.read_bytes()).hexdigest() == case["main_receipt_sha256"], "dataset receipt changed")
        if receipt["call_status"] != "ok":
            require(case["label"] == "unknown" and case["call_eligible"] is False and case["reason"] == receipt["call_status"],
                    "failed main attempt cannot be binary")
            continue
        verify_main_origin(original, receipt_file)
        oracle = sealed(case["oracle"])
        oracle_file = receipt_file.with_name(f"formal-artifact-oracle-{oracle['artifact_hash']}.json")
        require(load(oracle_file) == oracle and oracle["candidate_sha256"] == receipt["result"]["candidate_sha256"]
                == case["candidate_sha256"], "dataset original Oracle changed")
        label = {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"], "unknown")
        require(case["label"] == label and case["call_eligible"] is (label != "unknown"), "dataset label overrides original Oracle")
    authorization = next(binding["file"] for binding in registered["bindings"] if binding["role"] == "authorization")
    chain = resolve(authorization)
    ledger = Path(chain["authorization"]["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as db:
        starts = [json.loads(row[0]) for row in db.execute("SELECT started_json FROM attempts")]
    started = {row["id"] for row in starts if row.get("artifact_cohort_hash") == registered["artifact_hash"]}
    require(started <= {case["id"] for case in data["cases"]}, "formal dataset omitted an actual main attempt")
    return registered


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("directory", "executable", "cohort", "dataset", "prepared"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    require(not args.dataset.exists() and not args.prepared.exists(), "new formal outputs required")
    data, rows = prepare(args.directory, args.executable, args.cohort)
    create(args.dataset, data)
    with args.prepared.open("x", encoding="utf-8") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(json.dumps({"prepared": len(rows), "new_paid_calls": 0, "certified_threshold": None}))


if __name__ == "__main__":
    main()
