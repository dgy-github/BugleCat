"""Fresh source and original collection proof for development only; zero network.

The entire old calibration allocation is excluded. Shared SQLite starts and
reservations keep their original lineage, including pending/failed attempts.
Nothing in this owner opens a holdout or certifies a candidate or threshold.
"""
import argparse
import hashlib
import json
import re
import sqlite3
from pathlib import Path

from .annotation_audit import within_root
from .artifact_cohort import reservations
from .artifact_profile_cohort import SETTINGS, validate_profile
from .artifact_scoring import sealed
from .benchmark_oracle import audit
from .budget_authorization_v2 import resolve
from .calibration import create, digest, load, require
from .collection_manifest import file_hash, interfaces
from .collection_receipts import read_lines, seal_check, verify
from .extended_mbpp import rebuild
from .source_bindings import source_transaction

SCHEMA = "buglecat.verification.artifact-development-cohort.v2"
MANIFEST = "buglecat.verification.artifact-development-collection.v2"
SEED = "jev-artifact-v2-development-20261011-v1"
MODULE = Path(__file__).resolve()
ROOT = MODULE.parents[2]
EXAMPLES = ROOT / "rust/crates/ncx-core/examples"
PROMPT = EXAMPLES / "verification_collect/support.rs"


def binding(role, path):
    path = Path(path).resolve(strict=True)
    return {"role": role, "file": str(path), "sha256": file_hash(path)}


def system_prompt():
    match = re.search(r'pub const SYSTEM: &str = ("[^\n]+");', PROMPT.read_text(encoding="utf-8"))
    require(match is not None, "frozen system prompt missing")
    return json.loads(match.group(1))


def source_inputs(source, catalog_file):
    index = load(catalog_file)
    require(index["catalog_extension"] == "official-mbpp-preserve-original-split-v1"
            and file_hash(source) == "2875e6f093d50c4f10709666562b8f14018d932f31dc5dbda9ff81c02f4af9ce"
            and rebuild(source, index) == index, "fixed official expanded source required")
    return load(source), index


def snapshot(authorization):
    chain = resolve(authorization)
    auth = chain["authorization"]
    ledger = Path(auth["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as db:
        db.execute("BEGIN")
        metadata = dict(db.execute("SELECT key,value FROM metadata"))
        rows = list(db.execute("SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal"))
    require(metadata.get("authorization_sha256") == chain["ledger_authorization_sha256"], "shared SQLite lineage mismatch")
    caps = {b["sha256"]: load(b["file"])["main_model"]["max_attempts"] for b in chain["bindings"]}
    starts, finished, groups = [], {}, set()
    for ordinal, group, raw, completed in rows:
        value = json.loads(raw)
        require(group == value["group_id"] and group not in groups and value["authorization_sha256"] in caps
                and type(ordinal) is int and 1 <= ordinal <= caps[value["authorization_sha256"]], "invalid original SQLite start")
        groups.add(group)
        starts.append({"ordinal": ordinal, "group_id": group, "started": value})
        finished[group] = json.loads(completed) if completed else None
    require([r["ordinal"] for r in starts] == list(range(1, len(starts) + 1)), "original ordinal sequence changed")
    main_reserved, _ = reservations(auth["main_model"]["ledger_dir"], "main_model", chain)
    jev_reserved, _ = reservations(auth["jev"]["ledger_dir"], "jev", chain)
    excluded = groups | {r["group_id"] for r in main_reserved + jev_reserved}
    require(len(groups | {r["group_id"] for r in main_reserved}) <= auth["main_model"]["max_attempts"], "main cap exceeded")
    body = {"main_ledger_file": str(ledger.resolve()), "main_metadata": sorted(metadata.items()), "main_started": starts,
            "main_reservations": main_reserved, "jev_reservations": jev_reserved,
            "authorization_chain": chain, "excluded_groups": sorted(excluded),
            "remaining_main_attempts": auth["main_model"]["max_attempts"] - len(groups | {r["group_id"] for r in main_reserved})}
    return body, finished


def chosen(tasks, index, previous, baseline, count):
    require(type(count) is int and 1 <= count <= 8, "development upper bound is eight")
    old = {c["group_id"] for c in previous["cases"]}
    excluded = old | set(baseline["excluded_groups"])
    members = {}
    for row in sorted(index["tasks"], key=lambda r: r["task_id"]):
        if row["reserved_split"] == "calibration" and row["group_id"] not in excluded:
            members.setdefault(row["group_id"], row)
    ordered = sorted(members, key=lambda group: digest({"seed": SEED, "group": group}))
    require(len(ordered) >= count, "insufficient fresh development source")
    by_id = {t["task_id"]: t for t in tasks}
    cases = [{"id": f'mbpp-v2-dev-main-{members[g]["task_id"]}', "group_id": g,
              "task_id": members[g]["task_id"], "task_sha256": members[g]["task_sha256"],
              "public_interface": interfaces(by_id[members[g]["task_id"]]["code"]),
              "split": "development", "original_reserved_split": "calibration"} for g in ordered[:count]]
    return cases, sorted(excluded)


@source_transaction
def preregister(source, catalog_file, authorization, protocol_file, profile_file, previous_cohort, count=8):
    tasks, index = source_inputs(source, catalog_file)
    previous = sealed(load(previous_cohort))
    require(previous["artifact_hash"] == "3b7407279f3ee8c00c75a0e0ca92af9a5098f63fad8b0cbf3642dfd9055d3549"
            and len(previous["cases"]) == 265, "complete original high allocation required")
    profile, _ = validate_profile(profile_file)
    baseline, _ = snapshot(authorization)
    require(baseline["remaining_main_attempts"] >= count, "remaining shared main budget insufficient")
    cases, excluded = chosen(tasks, index, previous, baseline, count)
    dependencies = [binding(role, path) for role, path in (
        ("source", source), ("catalog", catalog_file), ("authorization", authorization), ("protocol", protocol_file),
        ("profile", profile_file), ("previous_cohort", previous_cohort), ("development_cohort_owner", MODULE),
        ("main_prompt_source", PROMPT), ("oracle_source", MODULE.with_name("benchmark_oracle.py")),
        ("oracle_worker", MODULE.with_name("benchmark_oracle_worker.py")),
        ("budget_v2_owner", MODULE.with_name("budget_authorization_v2.py")),
        ("collector_source", EXAMPLES / "verification_artifact_v2_collect.rs"),
        ("collector_support", EXAMPLES / "verification_artifact_v2_collect/support.rs"),
        ("collector_engine", EXAMPLES / "verification_artifact_v2_collect/engine.rs"))]
    dependencies += [binding(f"source_chain_{name}", index[f"{name}_file"]) for name in ("base_source", "base_catalog", "full_source")]
    dependencies += [binding("authorization_chain", b["file"]) for b in baseline["authorization_chain"]["bindings"]]
    source_map = {t["task_id"]: t for t in tasks}
    admission = [audit(source_map[c["task_id"]], source_map[c["task_id"]]["code"], timeout=10) for c in cases]
    # Reference failure remains in the immutable receipt. No case is skipped or
    # replaced to achieve quota; the coordinator must reject before any fee.
    body = {"schema": SCHEMA, "split": "development", "cases": cases, "seed": SEED, "bindings": dependencies,
            "ledger_snapshot": baseline, "excluded_groups": excluded, "previous_allocation_excluded": 265,
            "reference_admission": admission, "reference_only_timeout_s": 10, "actual_candidate_oracle_timeout_s": 5,
            "main_collection_settings": SETTINGS, "main_system_prompt_sha256": hashlib.sha256(system_prompt().encode()).hexdigest(),
            "main_resource_profile_file": str(Path(profile_file).resolve()), "main_resource_profile_sha256": file_hash(profile_file),
            "main_resource_profile_hash": profile["artifact_hash"], "formal_registration": False,
            "collection_authorized": all(r["result"]["status"] == "passed" for r in admission),
            "before_main_requests": True, "candidate_labels_assigned": False, "holdout_opened": False,
            "production_authorized": False, "new_paid_calls": 0}
    return {**body, "artifact_hash": digest(body)}


@source_transaction
def validate_cohort(path):
    value = sealed(load(path))
    require(value["schema"] == SCHEMA and value["split"] == "development" and value["formal_registration"] is False
            and value["holdout_opened"] is False and value["production_authorized"] is False
            and value["collection_authorized"] is True and value["before_main_requests"] is True
            and value["candidate_labels_assigned"] is False and value["seed"] == SEED
            and value["main_collection_settings"] == SETTINGS, "fresh development contract required")
    require(all(file_hash(b["file"]) == b["sha256"] for b in value["bindings"]), "development source owner changed")
    by_role = {b["role"]: b["file"] for b in value["bindings"]}
    tasks, index = source_inputs(by_role["source"], by_role["catalog"])
    previous = sealed(load(by_role["previous_cohort"]))
    require(previous["artifact_hash"] == "3b7407279f3ee8c00c75a0e0ca92af9a5098f63fad8b0cbf3642dfd9055d3549"
            and len(previous["cases"]) == value["previous_allocation_excluded"] == 265, "old allocation changed")
    profile, _ = validate_profile(by_role["profile"])
    require(value["main_resource_profile_hash"] == profile["artifact_hash"]
            and value["main_resource_profile_sha256"] == file_hash(by_role["profile"]), "development profile changed")
    current, _ = snapshot(by_role["authorization"])
    starts = {r["group_id"]: r for r in current["main_started"]}
    require(all(starts.get(r["group_id"]) == r for r in value["ledger_snapshot"]["main_started"]), "old start removed or changed")
    metadata = dict(current["main_metadata"])
    require(all(metadata.get(key) == val for key, val in value["ledger_snapshot"]["main_metadata"]), "old metadata reset")
    expected, excluded = chosen(tasks, index, previous, value["ledger_snapshot"], len(value["cases"]))
    require(value["cases"] == expected and value["excluded_groups"] == excluded, "source order or allocation changed")
    require(len(value["reference_admission"]) == len(expected), "complete source admission required")
    source_map = {t["task_id"]: t for t in tasks}
    worker = file_hash(MODULE.with_name("benchmark_oracle_worker.py"))
    for case, receipt in zip(expected, value["reference_admission"]):
        task = source_map[case["task_id"]]
        require(receipt["task_id"] == task["task_id"] and receipt["task_sha256"] == digest(task)
                and receipt["candidate_code_sha256"] == hashlib.sha256(task["code"].encode()).hexdigest()
                and receipt["oracle_worker_sha256"] == worker and receipt["result"]["status"] == "passed"
                and receipt["paid_attempts"] == 0, "reference-only admission unavailable")
    return value


@source_transaction
def manifest_value(cohort_file):
    value = validate_cohort(cohort_file)
    bindings = {b["role"]: b for b in value["bindings"]}
    body = {"schema": MANIFEST, "cases": value["cases"], **SETTINGS,
            "artifact_cohort_file": str(Path(cohort_file).resolve()), "artifact_cohort_sha256": file_hash(cohort_file),
            "artifact_cohort_hash": value["artifact_hash"], "formal_registration": False, "split": "development",
            **{key: value[key] for key in ("main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash")}}
    for name in ("source", "catalog", "authorization", "protocol"):
        body[f"{name}_file"], body[f"{name}_sha256"] = bindings[name]["file"], bindings[name]["sha256"]
    return {**body, "artifact_hash": digest(body)}


@source_transaction
def validate_manifest(path):
    manifest = sealed(load(path))
    require(manifest == manifest_value(manifest["artifact_cohort_file"]), "development manifest differs from frozen source")
    value = validate_cohort(manifest["artifact_cohort_file"])
    current, finished = snapshot(manifest["authorization_file"])
    previous = {r["group_id"] for r in value["ledger_snapshot"]["main_started"]}
    for case in value["cases"]:
        require(case["group_id"] not in previous, "attempted group restored")
        if case["group_id"] in finished:
            row = next(r["started"] for r in current["main_started"] if r["group_id"] == case["group_id"])
            require(row.get("artifact_cohort_hash") == value["artifact_hash"], "group attempted in another scope")
    return manifest


def user_prompt(task, case):
    return (f'Task:\n{task["prompt"]}\n\nPublic function interfaces (implement these names; bodies and hidden assertions are not supplied):\n'
            + "\n".join(case["public_interface"])
            + "\n\nProvide code and your honest assessment of the frozen benchmark acceptance criteria. Express uncertainty if needed. No execution was performed in this request.")


def same_path(left, right):
    return within_root(left, right) and within_root(right, left)


@source_transaction
def verify_collected(directory, executable):
    """Return all allocated outcomes, authentic original code and dev-only labels."""
    directory = Path(directory).resolve(strict=True)
    manifest = validate_manifest(directory / "collection-manifest.json")
    cohort = validate_cohort(manifest["artifact_cohort_file"])
    current, finished = snapshot(manifest["authorization_file"])
    starts = {r["group_id"]: r for r in current["main_started"]}
    rows = read_lines(directory / "prepared.jsonl")
    require(len(rows) == len(manifest["cases"]) and [r["id"] for r in rows] == [c["id"] for c in manifest["cases"]], "complete development allocation required")
    tasks = {t["task_id"]: t for t in load(manifest["source_file"])}
    events = read_lines(directory / "attempt-events.jsonl") if (directory / "attempt-events.jsonl").exists() else []
    outputs = []
    source_paths = {"runner_source_sha256": EXAMPLES / "verification_artifact_v2_collect.rs",
                    "support_source_sha256": EXAMPLES / "verification_artifact_v2_collect/support.rs",
                    "collection_engine_source_sha256": EXAMPLES / "verification_artifact_v2_collect/engine.rs",
                    "frozen_collector_support_sha256": EXAMPLES / "verification_artifact_collect/support.rs",
                    "profile_collector_validator_source_sha256": EXAMPLES / "verification_artifact_profile_collect/support.rs",
                    "legacy_support_source_sha256": PROMPT,
                    "request_builder_source_sha256": ROOT / "rust/crates/ncx-provider/src/request.rs",
                    "response_parser_source_sha256": ROOT / "rust/crates/ncx-provider/src/response.rs",
                    "development_owner_source_sha256": MODULE,
                    "budget_v2_owner_source_sha256": MODULE.with_name("budget_authorization_v2.py")}
    for row, case in zip(rows, manifest["cases"]):
        seal_check(row)
        task = tasks[case["task_id"]]
        case_dir = directory / case["id"]
        require(same_path(row["case_directory"], case_dir) and within_root(row["workspace"], case_dir)
                and within_root(row["session_log"], row["workspace"])
                and all(row[key] == case[key] for key in ("id", "group_id", "task_id", "split"))
                and row["manifest_sha256"] == file_hash(directory / "collection-manifest.json")
                and row["prompt"] == user_prompt(task, case)
                and row["public_interface"] == interfaces(task["code"]) and row["task_sha256"] == digest(task), "prepared original source changed")
        for field in ("artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash", "authorization_sha256"):
            require(row[field] == manifest[field], "prepared development binding changed")
        require(row["settings"] == SETTINGS and row["split"] == "development"
                and row["build_info"]["executable_sha256"] == file_hash(executable)
                and all(row["build_info"][key] == file_hash(path) for key, path in source_paths.items()), "compiled development source changed")
        original = starts.get(row["group_id"])
        receipt_file = case_dir / "receipt.json"
        output = {"id": row["id"], "group_id": row["group_id"], "task": task, "public_interface": case["public_interface"],
                  "authorization_file": manifest["authorization_file"], "authorization_sha256": manifest["authorization_sha256"],
                  "main_receipt_file": str(receipt_file), "main_receipt_sha256": file_hash(receipt_file) if receipt_file.exists() else None,
                  "candidate_code": None, "call_eligible": False, "label": "unknown", "split": "development",
                  "reason": "main_not_finished" if original else "main_not_started", "formal_certification": False,
                  "artifact_cohort_file": manifest["artifact_cohort_file"], "artifact_cohort_hash": cohort["artifact_hash"]}
        if original:
            selected = [e for e in events if e["id"] == row["id"]]
            require(selected and original["started"] == {k: v for k, v in selected[0].items() if k != "attempt_ordinal"}, "SQLite original start mismatch")
            if receipt_file.exists():
                receipt = load(receipt_file)
                seal_check(receipt)
                require(len(selected) == 3 and selected[-1] == finished[row["group_id"]]
                        and selected[-1]["receipt_sha256"] == file_hash(receipt_file), "SQLite original finish mismatch")
                require([e["event"] for e in selected] == ["attempt_started", "request_submitted", "attempt_finished"]
                        and all(e["request_hash"] == row["request_hash"] and e["build_info"] == row["build_info"] for e in selected)
                        and receipt["request_hash"] == receipt["actual_request_hash"] == file_hash(case_dir / "request.json")
                        and load(case_dir / "request.json") == row["body"]
                        and receipt["call_status"] == selected[-1]["call_status"], "original failure or submission changed")
                if receipt.get("response_sha256"):
                    require(receipt["response_sha256"] == file_hash(case_dir / "raw-response.json"), "original failed raw response changed")
                output["reason"] = receipt["call_status"]
                if receipt["call_status"] == "ok":
                    verify(row, receipt_file, events, executable)
                    candidate = receipt["result"]["candidate"]
                    fences = re.findall(r"(?ms)^```([^\n]*)\n(.*?)^```[ \t]*$", candidate)
                    if len(fences) == 1 and fences[0][0].strip().lower() in {"py", "python"}:
                        code = fences[0][1]
                        output.update(candidate_code=code, reason="independent_development_oracle_not_recorded")
                        oracle_path = case_dir / "development-oracle.json"
                        if oracle_path.exists():
                            oracle = sealed(load(oracle_path))
                            require(oracle["schema"] == "buglecat.verification.benchmark-assertion-receipt.v1"
                                    and oracle["task_id"] == task["task_id"] and oracle["test_count"] == len(task["test_list"])
                                    and oracle["independent_of_jev"] is True and oracle["secrets_removed"] is True and oracle["paid_attempts"] == 0
                                    and oracle["oracle_scope"] == "frozen MBPP acceptance tests only; not full task semantics"
                                    and oracle["task_sha256"] == digest(task) and oracle["candidate_code_sha256"] == hashlib.sha256(code.encode()).hexdigest()
                                    and oracle["oracle_worker_sha256"] == file_hash(MODULE.with_name("benchmark_oracle_worker.py")), "development Oracle binding mismatch")
                            label = {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"], "unknown")
                            output.update(label=label, call_eligible=label != "unknown", oracle=oracle,
                                          reason="scoped_development_assertions" if label != "unknown" else oracle["result"].get("reason", "oracle_unknown"))
                    else:
                        output["reason"] = "one_original_python_fence_required"
        outputs.append(output)
    require({r["started"]["id"] for r in current["main_started"] if r["started"].get("artifact_cohort_hash") == cohort["artifact_hash"]}
            <= {r["id"] for r in outputs}, "development dataset omitted actual start")
    return outputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["validate-manifest", "register", "manifest"])
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--cohort", type=Path)
    parser.add_argument("--output", type=Path)
    for name in ("source", "catalog", "authorization", "protocol", "profile", "previous-cohort"):
        parser.add_argument(f"--{name}", type=Path)
    args = parser.parse_args()
    if args.action == "validate-manifest":
        value = validate_manifest(args.manifest)
    elif args.action == "manifest":
        value = manifest_value(args.cohort)
        create(args.output, value)
    else:
        value = preregister(args.source, args.catalog, args.authorization, args.protocol, args.profile, args.previous_cohort)
        create(args.output, value)
    print(json.dumps({"status": "development_source_verified", "hash": value["artifact_hash"], "new_paid_calls": 0,
                      "formal_registration": False}))


if __name__ == "__main__":
    main()
