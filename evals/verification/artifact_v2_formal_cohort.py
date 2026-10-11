"""Version-isolated mixed-source registration and append-only history proof.

Pure construction/validation. No model, reservation, source write or holdout open.
Public reference admission is distinct from independently executed candidates.
"""
import argparse
import hashlib
import json
import platform
import sqlite3
import sys
from copy import deepcopy
from pathlib import Path

from .artifact_profile_cohort import validate_profile
from .artifact_scoring import sealed
from .artifact_v2_development_cohort import binding, snapshot, system_prompt
from .benchmark_oracle import audit as mbpp_audit
from .budget_authorization_v2 import resolve
from .calibration import create, digest, load, require
from .collection_manifest import file_hash, interfaces
from .extended_mbpp import rebuild
from .humaneval_source import SCOPE as HUMAN_SCOPE
from .humaneval_source import audit as human_audit
from .humaneval_source import inventory, official_rows
from .source_bindings import _TRANSACTION, check_unchanged, source_transaction

MODULE = Path(__file__).resolve()
ROOT = MODULE.parents[2]
SCHEMA = "buglecat.verification.artifact-mixed-cohort.formal.v2"
SOURCE_SCHEMA = "buglecat.verification.artifact-mixed-source.v2"
MANIFEST_SCHEMA = "buglecat.verification.artifact-mixed-collection.formal.v2"
SETTINGS = {"model": "deepseek-flash", "reasoning_effort": "high", "max_tokens": 16384,
            "timeout_s": 120, "max_retries": 0, "provider_protocol": "openai"}
SEED = "jev-artifact-formal-mixed-v2-20261011-v1"


def hash_text(value):
    return hashlib.sha256(value.encode()).hexdigest()


def runtime_info():
    executable = Path(sys.executable).resolve(strict=True)
    return {"executable_file": str(executable), "executable_sha256": file_hash(executable),
            "version": sys.version, "version_info": list(sys.version_info[:3]), "implementation": sys.implementation.name,
            "platform": sys.platform, "platform_detail": platform.platform()}


def oracle_wrapper(task, code, source_kind, *, reference=False):
    before = runtime_info()
    callback = mbpp_audit if source_kind == "mbpp" else human_audit
    require(source_kind in {"mbpp", "humaneval"}, "explicit Oracle runtime required")
    timeout = 10 if reference else 5
    inner = callback(task, code, timeout=timeout)
    if "artifact_hash" not in inner:
        inner = {**inner, "artifact_hash": digest(inner)}
    require(runtime_info() == before, "actual Python runtime changed during independent assertions")
    oracle_gate(inner, task, code, source_kind, reference=reference)
    body = {"schema": "buglecat.verification.runtime-oracle-receipt.formal.v2", "source_kind": source_kind,
            "oracle_runtime_binding": before, "source_only": reference, "candidate_labels_assigned": not reference,
            "worker_flags": ["-I", "-S", "-B"],
            "requested_timeout_s": timeout, "receipt": inner, "new_paid_calls": 0}
    return {**body, "artifact_hash": digest(body)}


def wrapped_oracle_gate(wrapper, task, code, source_kind, expected_runtime, *, reference=False):
    sealed(wrapper)
    require(wrapper["schema"] == "buglecat.verification.runtime-oracle-receipt.formal.v2"
            and wrapper["source_kind"] == source_kind and wrapper["oracle_runtime_binding"] == expected_runtime == runtime_info()
            and wrapper["source_only"] is reference and wrapper["candidate_labels_assigned"] is (not reference)
            and wrapper["worker_flags"] == ["-I", "-S", "-B"]
            and wrapper["requested_timeout_s"] == (10 if reference else 5) and wrapper["new_paid_calls"] == 0,
            "original runtime or independent source scope changed")
    inner = sealed(wrapper["receipt"])
    return oracle_gate(inner, task, code, source_kind, reference=reference)


def protocol_gate(value):
    from .artifact_v2_formal import protocol
    return protocol(value)


def oracle_gate(receipt, task, code, source_kind, *, reference=False):
    """Exact source runtime receipt; no probability or generalized task label."""
    require(source_kind in {"mbpp", "humaneval"}, "unrecognized source runtime")
    require(receipt["task_id"] == task["task_id"] and receipt["task_sha256"] == digest(task)
            and receipt["candidate_code_sha256"] == hash_text(code) and receipt["independent_of_jev"] is True
            and receipt["secrets_removed"] is True and receipt["paid_attempts"] == 0
            and receipt["result"]["status"] in {"passed", "failed", "unknown"}, "original independent Oracle binding changed")
    if source_kind == "mbpp":
        require(receipt["schema"] == "buglecat.verification.benchmark-assertion-receipt.v1"
                and receipt["oracle_worker_sha256"] == file_hash(MODULE.with_name("benchmark_oracle_worker.py"))
                and receipt["test_count"] == len(task["test_list"])
                and receipt["oracle_scope"] == "frozen MBPP acceptance tests only; not full task semantics", "MBPP runtime proof changed")
    else:
        expected = [{"file": str(MODULE.with_name(name)), "sha256": file_hash(MODULE.with_name(name))}
                    for name in ("humaneval_oracle_worker.py", "benchmark_oracle_worker.py")]
        require(receipt["schema"] == "buglecat.verification.humaneval-assertion-receipt.v1"
                and receipt["source_task_id"] == task["source_task_id"] and receipt["original_test_sha256"] == task["original_test_sha256"]
                and receipt["test_block_count"] == len(task["test_list"]) and receipt["oracle_scope"] == HUMAN_SCOPE
                and receipt["oracle_bindings"] == expected and receipt["timeout_s"] == (10 if reference else 5), "HumanEval runtime proof changed")
    return receipt


@source_transaction
def source_value(mbpp_source, mbpp_catalog, human_manifest, human_source, human_catalog, human_admission, *, human_calibration_groups, reference_pool_file=None):
    """Rebuild complete pinned sources, then freeze new HumanEval splits only."""
    mbpp, catalog = load(mbpp_source), load(mbpp_catalog)
    require(file_hash(mbpp_source) == "2875e6f093d50c4f10709666562b8f14018d932f31dc5dbda9ff81c02f4af9ce"
            and rebuild(mbpp_source, catalog) == catalog, "original expanded MBPP source required")
    official, download = official_rows(human_manifest)
    human, expected = inventory(official, mbpp)
    stored = sealed(load(human_catalog))
    require(human == load(human_source)
            and all(stored[key] == value for key, value in expected.items() if key != "artifact_hash")
            and stored["derived_source_sha256"] == file_hash(human_source)
            and all(file_hash(b["file"]) == b["sha256"] for b in stored["bindings"]), "HumanEval derived chain changed")
    admission = sealed(load(human_admission))
    require(admission["source_sha256"] == file_hash(human_source) and admission["source_only"] is True
            and admission["candidate_labels_assigned"] is False and admission["new_paid_calls"] == 0
            and len(admission["receipts"]) == len(human), "complete HumanEval reference admission required")
    received = {r["task_id"]: r for r in admission["receipts"]}
    require(len(received) == len(human), "duplicate HumanEval reference receipt")
    admissible, admissible_tasks = set(), set()
    for task, entry in zip(human, expected["tasks"]):
        receipt = sealed(received[task["task_id"]])
        oracle_gate(receipt, task, task["code"], "humaneval")
        if entry["source_admission"] == "static_supported" and receipt["result"]["status"] == "passed":
            admissible.add(entry["group_id"])
            admissible_tasks.add(entry["task_id"])
    human_splits = split_new_groups(admissible, human_calibration_groups)
    rows = [{**entry, "source_kind": "mbpp", "public_interface": interfaces(task["code"]), "source_available": True}
            for entry, task in zip(catalog["tasks"], mbpp)]
    rows += [{**entry, "source_kind": "humaneval", "reserved_split": human_splits.get(entry["group_id"], "unavailable"),
              "source_available": entry["task_id"] in admissible_tasks} for entry in expected["tasks"]]
    require(len({row["task_id"] for row in rows}) == len(rows), "cross-source task identity collision")
    sources = [binding(role, path) for role, path in (
        ("mbpp_source", mbpp_source), ("mbpp_catalog", mbpp_catalog), ("human_manifest", human_manifest),
        ("human_source", human_source), ("human_catalog", human_catalog), ("human_reference_admission", human_admission))]
    sources += [binding(f"mbpp_{name}", catalog[f"{name}_file"]) for name in ("base_source", "base_catalog", "full_source")]
    sources += [binding(f"human_official_{name}", b["file"]) for name, b in download["files"].items()]
    if reference_pool_file is not None:
        pool = sealed(load(reference_pool_file))
        require(pool["schema"] == "buglecat.verification.fresh-reference-pool.formal.v2"
                and pool["source_only"] is True and pool["candidate_labels_assigned"] is False and pool["new_paid_calls"] == 0
                and pool["before_new_formal_candidates"] is True and pool["reference_only_timeout_s"] == 10
                and pool["actual_candidate_oracle_timeout_s"] == 5
                and pool["oracle_runtime_binding"] == runtime_info()
                and all(file_hash(b["file"]) == b["sha256"] for b in pool["bindings"]), "fresh reference-pool proof changed")
        covered, available = set(), set()
        task_map = {task["task_id"]: task for task in mbpp + human}
        entry_map = {entry["task_id"]: entry for entry in rows}
        for case in pool["cases"]:
            task, entry = task_map[case["task_id"]], entry_map[case["task_id"]]
            require(case["group_id"] == entry["group_id"] and case["source_kind"] == entry["source_kind"]
                    and case["task_sha256"] == digest(task)
                    and case["task_id"] not in covered, "reference-pool task/group changed")
            inner = wrapped_oracle_gate(case["reference_receipt"], task, task["code"], entry["source_kind"], pool["oracle_runtime_binding"], reference=True)
            covered.add(case["task_id"])
            if inner["result"]["status"] == "passed":
                available.add(case["task_id"])
        for entry in rows:
            entry["source_available"] = entry["source_available"] and entry["task_id"] in available
        sources.append(binding("fresh_reference_pool", reference_pool_file))
    body = {"schema": SOURCE_SCHEMA, "tasks": rows, "bindings": sources, "seed": SEED,
            "human_calibration_groups": human_calibration_groups, "old_mbpp_splits_changed": False,
            "candidate_labels_assigned": False, "new_paid_calls": 0}
    return mbpp + human, {**body, "artifact_hash": digest(body)}


def split_new_groups(groups, calibration_count):
    require(type(calibration_count) is int and 0 <= calibration_count <= len(groups), "invalid new-source split allocation")
    ordered = sorted(groups, key=lambda group: digest({"seed": SEED, "human_group": group}))
    return {group: "calibration" if n < calibration_count else "holdout" for n, group in enumerate(ordered)}


def rebuild_sources(source_file, catalog_file):
    # Reuse the existing transaction boundary, never its ledger/probability data.
    # Every hit rehashes all transitive source bytes and the actual interpreter.
    cache = _TRANSACTION.get()
    identity = ("formal_v2_mixed_source", str(Path(source_file).resolve()), file_hash(source_file),
                str(Path(catalog_file).resolve()), file_hash(catalog_file), digest(runtime_info()))
    if cache is not None and identity in cache:
        tasks, catalog, bindings = cache[identity]
        check_unchanged(bindings)
        return deepcopy(tasks), deepcopy(catalog)
    catalog = sealed(load(catalog_file))
    require(catalog["schema"] == SOURCE_SCHEMA and catalog["seed"] == SEED, "mixed formal source required")
    roles = {b["role"]: b["file"] for b in catalog["bindings"]}
    tasks, expected = source_value(*(roles[key] for key in
        ("mbpp_source", "mbpp_catalog", "human_manifest", "human_source", "human_catalog", "human_reference_admission")),
        human_calibration_groups=catalog["human_calibration_groups"], reference_pool_file=roles.get("fresh_reference_pool"))
    require(catalog == expected and load(source_file) == tasks, "mixed source bytes or fixed split changed")
    bindings = [(str(Path(source_file).resolve()), file_hash(source_file)),
                (str(Path(catalog_file).resolve()), file_hash(catalog_file))]
    bindings += [(b["file"], b["sha256"]) for b in catalog["bindings"]]
    for role in ("human_catalog", "fresh_reference_pool"):
        if role in roles:
            bindings += [(b["file"], b["sha256"]) for b in load(roles[role])["bindings"]]
    bindings += [(str(MODULE.with_name(name)), file_hash(MODULE.with_name(name))) for name in
                 ("artifact_v2_formal_cohort.py", "humaneval_source.py", "humaneval_oracle_worker.py", "benchmark_oracle_worker.py")]
    actual_runtime = runtime_info()
    bindings.append((actual_runtime["executable_file"], actual_runtime["executable_sha256"]))
    check_unchanged(bindings)
    if cache is not None:
        cache[identity] = deepcopy((tasks, catalog, bindings))
    return tasks, catalog


def raw_ledger(authorization):
    value, finished = snapshot(authorization)
    with sqlite3.connect(Path(value["main_ledger_file"]).as_uri() + "?mode=ro", uri=True) as db:
        db.execute("BEGIN")
        raw = list(db.execute("SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal"))
    require(len(raw) == len(value["main_started"])
            and all(json.loads(row[2]) == start["started"] for row, start in zip(raw, value["main_started"])), "history changed during snapshot")
    value["raw_main_rows"] = raw
    chain = resolve(authorization)
    value["reservation_bindings"] = [binding(f"{kind}_original_reservation", path)
        for kind in ("main_model", "jev") for path in sorted((Path(chain["authorization"][kind]["ledger_dir"]) / "reservations").glob("*.json"))]
    value["reservation_bindings"] += [binding(f"{kind}_original_marker_or_metadata", path)
        for kind in ("main_model", "jev") for path in
        [Path(chain["authorization"][kind]["ledger_dir"]) / "authorization.json",
         *sorted((Path(chain["authorization"][kind]["ledger_dir"]) / "reservations").glob("*.started"))] if path.exists()]
    value["remaining_jev_attempts"] = chain["authorization"]["jev"]["max_attempts"] - len(value["jev_reservations"])
    return value, finished


def history_gate(before, current):
    """Old starts/terminal finishes and reservation bytes cannot be rewritten."""
    require(before["main_ledger_file"] == current["main_ledger_file"], "history ledger root changed")
    old = before["raw_main_rows"]
    require(len(current["raw_main_rows"]) >= len(old), "history prefix removed")
    for expected, actual in zip(old, current["raw_main_rows"]):
        require(list(expected[:3]) == list(actual[:3]), "original start bytes changed")
        if expected[3] is not None:
            require(expected[3] == actual[3], "original terminal finish bytes changed")
    metadata = dict(current["main_metadata"])
    require(all(metadata.get(key) == value for key, value in before["main_metadata"]), "original metadata reset")
    require(all(file_hash(b["file"]) == b["sha256"] for b in before["reservation_bindings"]), "old reservation bytes changed")


def allocation(catalog, excluded, calibration_count, holdout_count, remaining_main, remaining_jev):
    require(type(calibration_count) is int and type(holdout_count) is int
            and calibration_count >= 200 and holdout_count >= 200, "independent source allocation cannot lower 200-group floor")
    require(calibration_count + holdout_count <= min(remaining_main, remaining_jev), "shared remaining budget cannot fund fixed source allocation")
    members = {}
    for row in sorted(catalog["tasks"], key=lambda entry: entry["task_id"]):
        if row["source_available"] and row["group_id"] not in excluded:
            members.setdefault(row["group_id"], row)
    cases = []
    for split, count in (("calibration", calibration_count), ("holdout", holdout_count)):
        groups = [group for group, row in members.items() if row["reserved_split"] == split]
        groups.sort(key=lambda group: digest({"seed": SEED, "split": split, "group": group}))
        require(len(groups) >= count, f"insufficient fresh {split} source; no old-split borrowing")
        cases += [{"id": f'artifact-v2-formal-{members[group]["task_id"]}', "group_id": group,
                   "task_id": members[group]["task_id"], "task_sha256": members[group]["task_sha256"],
                   "source_kind": members[group]["source_kind"], "public_interface": members[group]["public_interface"],
                   "split": split} for group in groups[:count]]
    return cases


@source_transaction
def preregister(source_file, catalog_file, authorization, protocol_file, profile_file, exclusion_cohort_files,
                *, calibration_count, holdout_count, historical_calibration_file):
    tasks, catalog = rebuild_sources(source_file, catalog_file)
    require(any(b["role"] == "fresh_reference_pool" for b in catalog["bindings"]), "full fresh calibration source admission must precede preregistration")
    protocol_gate(load(protocol_file))
    current, _ = raw_ledger(authorization)
    require(resolve(authorization)["authorization"]["main_model"]["max_attempts"] == 750, "new formal scope requires cap750 chain")
    profile, _ = validate_profile(profile_file)
    require(profile["main_collection_settings"] == SETTINGS, "fixed high resource profile required")
    exclusions = [sealed(load(path)) for path in exclusion_cohort_files]
    require(any(value["artifact_hash"] == "3b7407279f3ee8c00c75a0e0ca92af9a5098f63fad8b0cbf3642dfd9055d3549"
                and len(value["cases"]) == 265 for value in exclusions), "complete original 265 allocation required")
    require(any(value["schema"] == "buglecat.verification.artifact-development-cohort.v2"
                and len(value["cases"]) == 8 for value in exclusions), "complete fresh dev-eight allocation required")
    excluded = set(current["excluded_groups"]) | {case["group_id"] for value in exclusions for case in value["cases"]}
    cases = allocation(catalog, excluded, calibration_count, holdout_count, current["remaining_main_attempts"], current["remaining_jev_attempts"])
    previous = sealed(load(historical_calibration_file))
    require(previous["qualified_for_holdout_threshold"] is None, "expected immutable failed v1 calibration result")
    paths = [("source", source_file), ("catalog", catalog_file), ("authorization", authorization), ("protocol", protocol_file),
             ("profile", profile_file), ("historical_calibration", historical_calibration_file), ("cohort_owner", MODULE),
             ("intake_owner", MODULE.with_name("artifact_v2_formal_intake.py")),
             ("holdout_owner", MODULE.with_name("artifact_v2_formal_holdout.py")),
             ("human_adapter", MODULE.with_name("humaneval_source.py")), ("human_worker", MODULE.with_name("humaneval_oracle_worker.py")),
             ("mbpp_oracle", MODULE.with_name("benchmark_oracle.py")), ("mbpp_worker", MODULE.with_name("benchmark_oracle_worker.py")),
             ("budget_v2_owner", MODULE.with_name("budget_authorization_v2.py"))]
    paths += [("formal_statistical_owner", MODULE.with_name("artifact_v2_formal.py")),
              ("formal_request_owner", MODULE.with_name("artifact_request_v2_formal.py")),
              ("formal_observation_owner", MODULE.with_name("artifact_v2_formal_observations.py")),
              ("request_support_owner", MODULE.with_name("artifact_request_v2.py")),
              ("source_adapter_owner", MODULE.with_name("extended_mbpp.py")),
              ("frozen_development_owner", MODULE.with_name("artifact_v2_development_cohort.py"))]
    paths += [("frozen_transitive_owner", MODULE.with_name(name)) for name in
              ("artifact_cohort.py", "collection_receipts.py", "collection_manifest.py", "source_bindings.py",
               "budget_authorization.py", "paid_budget.py", "artifact_scoring.py", "calibration.py",
               "annotation_audit.py", "artifact_gate.py", "artifact_profile_cohort.py",
               "budget_authorization_v2.py", "artifact_source_registry.py")]
    paths += [("exclusion_cohort", path) for path in exclusion_cohort_files]
    paths += [(f"collector_{name}", ROOT / "rust/crates/ncx-core/examples" / name) for name in
              ("verification_artifact_v2_formal_collect.rs", "verification_artifact_v2_formal_collect/support.rs", "verification_artifact_v2_formal_collect/engine.rs")]
    body = {"schema": SCHEMA, "cases": cases, "bindings": [binding(role, path) for role, path in paths],
            "oracle_runtime_binding": runtime_info(),
            "history_baseline": current, "excluded_groups": sorted(excluded), "seed": SEED,
            "main_collection_settings": SETTINGS, "main_system_prompt_sha256": hash_text(system_prompt()),
            "main_resource_profile_file": str(Path(profile_file).resolve()), "main_resource_profile_sha256": file_hash(profile_file),
            "main_resource_profile_hash": profile["artifact_hash"], "calibration_allocation": calibration_count,
            "holdout_allocation": holdout_count, "formal_registration": True, "before_main_requests": True,
            "scores_hidden": True, "jev_probabilities_read": False, "candidate_labels_assigned": False,
            "holdout_opened": False, "production_authorized": False, "new_paid_calls": 0,
            "legacy_v1_global_proof_extended": False, "history_proof_scope": "append-only original starts/finishes/reservation bytes; original failed result unchanged"}
    return {**body, "artifact_hash": digest(body)}


@source_transaction
def validate(path):
    value = sealed(load(path))
    require(value["schema"] == SCHEMA and value["formal_registration"] is True and value["before_main_requests"] is True
            and value["scores_hidden"] is True and value["jev_probabilities_read"] is False
            and value["candidate_labels_assigned"] is False and value["holdout_opened"] is False
            and value["production_authorized"] is False and value["main_collection_settings"] == SETTINGS, "mixed preregistration contract changed")
    require(value["oracle_runtime_binding"] == runtime_info(), "frozen independent Python runtime changed")
    require(all(file_hash(b["file"]) == b["sha256"] for b in value["bindings"]), "mixed owner binding changed")
    roles = {b["role"]: b["file"] for b in value["bindings"]}
    tasks, catalog = rebuild_sources(roles["source"], roles["catalog"])
    profile, _ = validate_profile(roles["profile"])
    require(value["main_resource_profile_hash"] == profile["artifact_hash"]
            and value["main_resource_profile_sha256"] == file_hash(roles["profile"])
            and value["main_resource_profile_file"] == roles["profile"]
            and value["main_system_prompt_sha256"] == hash_text(system_prompt()), "fixed original main prompt/profile changed")
    protocol_gate(load(roles["protocol"]))
    current, _ = raw_ledger(roles["authorization"])
    history_gate(value["history_baseline"], current)
    exclusions = [sealed(load(b["file"])) for b in value["bindings"] if b["role"] == "exclusion_cohort"]
    actual_excluded = set(value["history_baseline"]["excluded_groups"]) | {case["group_id"] for old in exclusions for case in old["cases"]}
    require(actual_excluded == set(value["excluded_groups"]), "old whole source allocation removed")
    expected = allocation(catalog, set(value["excluded_groups"]), value["calibration_allocation"], value["holdout_allocation"],
                          value["history_baseline"]["remaining_main_attempts"], value["history_baseline"]["remaining_jev_attempts"])
    require(value["seed"] == SEED and value["cases"] == expected, "fixed source order or allocation changed")
    return value, tasks, catalog


def manifest_value(cohort_file, count=8, *, holdout_consumption_file=None):
    value, tasks, _ = validate(cohort_file)
    require(type(count) is int and 1 <= count <= 8, "main batches bounded to eight")
    roles = {b["role"]: b for b in value["bindings"]}
    current, _ = raw_ledger(roles["authorization"]["file"])
    started = set(current["excluded_groups"])
    split = "calibration"
    consumption = None
    if holdout_consumption_file is not None:
        from .artifact_v2_formal_holdout import validate as holdout_validate
        consumption = holdout_validate(holdout_consumption_file)
        require(consumption["cohort_hash"] == value["artifact_hash"], "foreign holdout source opening")
        split = "holdout"
    cases = [case for case in value["cases"] if case["split"] == split and case["group_id"] not in started][:count]
    require(cases, "no fresh fixed source groups remain")
    by_id = {task["task_id"]: task for task in tasks}
    admission = [oracle_wrapper(by_id[case["task_id"]], by_id[case["task_id"]]["code"], case["source_kind"], reference=True) for case in cases]
    body = {"schema": MANIFEST_SCHEMA, "cases": cases, **SETTINGS, "split": split, "formal_registration": True,
            "reference_admission": admission, "reference_only_timeout_s": 10, "candidate_oracle_timeout_s": 5,
            "candidate_labels_assigned": False, "new_paid_calls": 0,
            "oracle_runtime_binding": value["oracle_runtime_binding"],
            "artifact_cohort_file": str(Path(cohort_file).resolve()), "artifact_cohort_sha256": file_hash(cohort_file), "artifact_cohort_hash": value["artifact_hash"],
            **{key: value[key] for key in ("main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash")}}
    for role in ("source", "catalog", "authorization", "protocol"):
        body[f"{role}_file"], body[f"{role}_sha256"] = roles[role]["file"], roles[role]["sha256"]
    if consumption is not None:
        body.update(holdout_consumption_file=str(Path(holdout_consumption_file).resolve()),
                    holdout_consumption_sha256=file_hash(holdout_consumption_file), holdout_consumption_hash=consumption["artifact_hash"],
                    calibration_hash=consumption["calibration_hash"])
    return {**body, "artifact_hash": digest(body)}


def validate_manifest(path):
    value = sealed(load(path))
    require(value["schema"] == MANIFEST_SCHEMA and value["split"] in {"calibration", "holdout"} and value["formal_registration"] is True,
            "formal mixed source contract required")
    cohort, tasks, _ = validate(value["artifact_cohort_file"])
    require(value["artifact_cohort_sha256"] == file_hash(value["artifact_cohort_file"])
            and value["artifact_cohort_hash"] == cohort["artifact_hash"] and 1 <= len(value["cases"]) <= 8, "cohort binding changed")
    roles = {b["role"]: b for b in cohort["bindings"]}
    require(all(value[f"{role}_file"] == roles[role]["file"] and value[f"{role}_sha256"] == roles[role]["sha256"]
                for role in ("source", "catalog", "authorization", "protocol")), "source manifest binding changed")
    require(all(value[key] == expected for key, expected in SETTINGS.items())
            and all(value[key] == cohort[key] for key in ("main_resource_profile_file", "main_resource_profile_sha256", "main_resource_profile_hash")), "main settings changed")
    require(value["oracle_runtime_binding"] == cohort["oracle_runtime_binding"], "manifest runtime binding changed")
    if value["split"] == "holdout":
        from .artifact_v2_formal_holdout import validate as holdout_validate
        consumption = holdout_validate(value["holdout_consumption_file"])
        require(value["holdout_consumption_sha256"] == file_hash(value["holdout_consumption_file"])
                and value["holdout_consumption_hash"] == consumption["artifact_hash"]
                and value["calibration_hash"] == consumption["calibration_hash"] and consumption["cohort_hash"] == cohort["artifact_hash"],
                "original qualified holdout opening required")
    else:
        require(not any(key.startswith("holdout_consumption") or key == "calibration_hash" for key in value), "calibration cannot use later holdout evidence")
    allocated = [case for case in cohort["cases"] if case["split"] == value["split"]]
    require(any(allocated[n:n + len(value["cases"])] == value["cases"] for n in range(len(allocated))), "fixed contiguous source prefix required")
    current, _ = raw_ledger(value["authorization_file"])
    started = {r["group_id"]: r["started"] for r in current["main_started"]}
    first = allocated.index(value["cases"][0])
    require(all(case["group_id"] in started for case in allocated[:first]), "earlier allocated source skipped")
    require(all(case["group_id"] not in started or started[case["group_id"]].get("artifact_cohort_hash") == cohort["artifact_hash"] for case in value["cases"]), "group started in another scope")
    require(len(value["reference_admission"]) == len(value["cases"]) and value["reference_only_timeout_s"] == 10
            and value["candidate_oracle_timeout_s"] == 5 and value["candidate_labels_assigned"] is False and value["new_paid_calls"] == 0, "reference-only admission required")
    by_id = {task["task_id"]: task for task in tasks}
    for case, receipt in zip(value["cases"], value["reference_admission"]):
        task = by_id[case["task_id"]]
        inner = wrapped_oracle_gate(receipt, task, task["code"], case["source_kind"], cohort["oracle_runtime_binding"], reference=True)
        require(inner["result"]["status"] == "passed", "source reference unavailable; no source substitution or fee")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["validate-manifest", "manifest"])
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--cohort", type=Path)
    parser.add_argument("--count", type=int, default=8)
    parser.add_argument("--holdout-consumption", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    value = validate_manifest(args.manifest) if args.action == "validate-manifest" else manifest_value(args.cohort, args.count, holdout_consumption_file=args.holdout_consumption)
    if args.action == "manifest":
        create(args.output, value)
    print(json.dumps({"status": "formal_mixed_source_verified", "hash": value["artifact_hash"], "new_paid_calls": 0, "holdout_opened": value["split"] == "holdout"}))


if __name__ == "__main__":
    main()
