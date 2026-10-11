"""Original mixed-source main/Oracle intake, before Jev probabilities.

No transport, reservation, candidate rewriting, threshold selection or labels
from model confidence. Original failed and unstarted denominators stay separate.
"""
import re
from pathlib import Path

from .annotation_audit import within_root
from .artifact_request_v2_formal import request
from .artifact_scoring import sealed
from .artifact_v2_development_cohort import same_path, user_prompt
from .artifact_v2_formal_cohort import (
    SETTINGS,
    hash_text,
    oracle_wrapper,
    raw_ledger,
    validate,
    validate_manifest,
    wrapped_oracle_gate,
)
from .calibration import create, digest, load, require
from .collection_manifest import file_hash
from .collection_receipts import read_lines, seal_check, verify
from .source_bindings import source_transaction

MODULE = Path(__file__).resolve()
ROOT = MODULE.parents[2]
EXAMPLES = ROOT / "rust/crates/ncx-core/examples"
SCHEMA = "buglecat.verification.artifact-dataset.formal.v2"


def runner_sources():
    return {"runner_source_sha256": EXAMPLES / "verification_artifact_v2_formal_collect.rs",
            "support_source_sha256": EXAMPLES / "verification_artifact_v2_formal_collect/support.rs",
            "collection_engine_source_sha256": EXAMPLES / "verification_artifact_v2_formal_collect/engine.rs",
            "frozen_development_support_sha256": EXAMPLES / "verification_artifact_v2_collect/support.rs",
            "frozen_collector_support_sha256": EXAMPLES / "verification_artifact_collect/support.rs",
            "profile_collector_validator_source_sha256": EXAMPLES / "verification_artifact_profile_collect/support.rs",
            "legacy_support_source_sha256": EXAMPLES / "verification_collect/support.rs",
            "request_builder_source_sha256": ROOT / "rust/crates/ncx-provider/src/request.rs",
            "response_parser_source_sha256": ROOT / "rust/crates/ncx-provider/src/response.rs",
            "formal_cohort_owner_source_sha256": MODULE.with_name("artifact_v2_formal_cohort.py"),
            "formal_intake_owner_source_sha256": MODULE,
            "holdout_gate_owner_source_sha256": MODULE.with_name("artifact_v2_formal_holdout.py"),
            "budget_v2_owner_source_sha256": MODULE.with_name("budget_authorization_v2.py"),
            "human_adapter_source_sha256": MODULE.with_name("humaneval_source.py"),
            "human_worker_source_sha256": MODULE.with_name("humaneval_oracle_worker.py"),
            "mbpp_worker_source_sha256": MODULE.with_name("benchmark_oracle_worker.py")}


def prepared_gate(row, case, manifest, directory, task, executable):
    seal_check(row)
    directory = Path(directory)
    require(same_path(row["case_directory"], directory) and within_root(row["workspace"], directory)
            and within_root(row["session_log"], row["workspace"]), "main prepared scope identity changed")
    require(all(row[key] == case[key] for key in ("id", "group_id", "task_id", "source_kind", "split", "public_interface"))
            and row["schema"] == "buglecat.verification.prepared-main.v1" and row["task_sha256"] == digest(task)
            and row["settings"] == SETTINGS and row["prompt"] == user_prompt(task, case), "prepared original task changed")
    require(all(row[key] == manifest[key] for key in
                ("artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash", "authorization_sha256",
                 "source_sha256", "catalog_sha256", "protocol_sha256", "main_resource_profile_file",
                 "main_resource_profile_sha256", "main_resource_profile_hash")), "main source binding changed")
    require(row["oracle_runtime_binding"] == manifest["oracle_runtime_binding"], "prepared Python runtime changed")
    require(all(row.get(key) == manifest.get(key) for key in
                ("holdout_consumption_file", "holdout_consumption_sha256", "holdout_consumption_hash", "calibration_hash")), "holdout preparation changed")
    require(row["manifest_sha256"] == file_hash(directory.parent / "collection-manifest.json")
            and all(row["build_info"][key] == file_hash(path) for key, path in runner_sources().items())
            and row["build_info"]["executable_sha256"] == file_hash(executable), "actual main source/exe changed")
    require(row["request_hash"] == file_hash(directory / "request.json") and load(directory / "request.json") == row["body"], "original request bytes changed")


def outcome(row, task, original, finished, events, executable):
    """Recompute one raw outcome; candidate Oracle never runs on this path."""
    directory = Path(row["case_directory"])
    receipt_path = directory / "receipt.json"
    result = {"id": row["id"], "group_id": row["group_id"], "task_id": row["task_id"], "source_kind": row["source_kind"],
              "split": row["split"], "task": task, "public_interface": row["public_interface"],
              "task_sha256": digest(task), "candidate_code": None, "candidate_code_sha256": None,
              "label": "unknown" if original else "unlabeled", "call_eligible": False,
              "main_status": "main_not_finished" if original else "main_not_started",
              "reason": "main_not_finished" if original else "main_not_started",
              "main_receipt_file": str(receipt_path), "main_receipt_sha256": None,
              "oracle_file": None, "oracle_sha256": None, "formal_request": None}
    if not original:
        require(not receipt_path.exists(), "receipt without original SQLite start")
        return result
    selected = [event for event in events if event["id"] == row["id"]]
    require(selected and original == {key: value for key, value in selected[0].items() if key != "attempt_ordinal"}, "original SQLite start mismatch")
    if not receipt_path.exists():
        require(finished is None, "terminal finish without original receipt")
        return result
    receipt = load(receipt_path)
    seal_check(receipt)
    require([event["event"] for event in selected] == ["attempt_started", "request_submitted", "attempt_finished"]
            and selected[-1] == finished and selected[-1]["receipt_sha256"] == file_hash(receipt_path), "original SQLite finish mismatch")
    require(all(event["build_info"] == row["build_info"] and event["request_hash"] == row["request_hash"]
                and event["oracle_runtime_binding"] == row["oracle_runtime_binding"]
                and event["manifest_sha256"] == row["manifest_sha256"]
                and same_path(event["case_directory"], row["case_directory"])
                and all(event.get(key) == row.get(key) for key in
                        ("holdout_consumption_file", "holdout_consumption_sha256", "holdout_consumption_hash", "calibration_hash")) for event in selected)
            and selected[1]["actual_request_hash"] == receipt["actual_request_hash"] == row["request_hash"]
            and all(receipt[key] == row[key] for key in ("id", "group_id", "task_id", "source_kind", "split", "build_info", "settings",
                 "artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash", "authorization_sha256", "oracle_runtime_binding"))
            and receipt["call_status"] == selected[-1]["call_status"], "original terminal source/transport changed")
    if receipt.get("response_sha256"):
        require(receipt["response_sha256"] == file_hash(directory / "raw-response.json"), "original response bytes changed")
    result.update(main_status=receipt["call_status"], reason=receipt["call_status"], main_receipt_sha256=file_hash(receipt_path))
    if receipt["call_status"] != "ok":
        return result
    verify(row, receipt_path, events, executable)
    fences = re.findall(r"(?ms)^```([^\n]*)\n(.*?)^```[ \t]*$", receipt["result"]["candidate"])
    if len(fences) != 1 or fences[0][0].strip().lower() not in {"py", "python"}:
        result["reason"] = "one_original_python_fence_required"
        return result
    code = fences[0][1]
    result.update(candidate_code=code, candidate_code_sha256=hash_text(code), reason="independent_oracle_not_recorded")
    oracle_path = directory / "formal-oracle.json"
    if not oracle_path.exists():
        return result
    oracle = sealed(load(oracle_path))
    inner = wrapped_oracle_gate(oracle, task, code, row["source_kind"], row["oracle_runtime_binding"])
    label = {"passed": "supported", "failed": "unsupported"}.get(inner["result"]["status"], "unknown")
    result.update(label=label, call_eligible=label != "unknown", oracle_file=str(oracle_path), oracle_sha256=file_hash(oracle_path),
                  reason="frozen_assertions_only" if label != "unknown" else inner["result"].get("reason", "oracle_unknown"), oracle=oracle,
                  oracle_runtime_binding=row["oracle_runtime_binding"])
    if label != "unknown":
        result["formal_request"] = request(task, row["public_interface"], code, row["source_kind"], oracle_runtime_binding=row["oracle_runtime_binding"])
    return result


@source_transaction
def collect_value(cohort_file, prepared_directory, executable, collections, split="calibration"):
    """All allocated sources plus every currently started original receipt."""
    require(split in {"calibration", "holdout"}, "independent formal stage required")
    root = Path(prepared_directory).resolve(strict=True)
    cohort, tasks, _ = validate(cohort_file)
    roles = {b["role"]: b["file"] for b in cohort["bindings"]}
    current, finished = raw_ledger(roles["authorization"])
    starts = {row["group_id"]: row["started"] for row in current["main_started"]}
    task_map = {task["task_id"]: task for task in tasks}
    allocated = [case for case in cohort["cases"] if case["split"] == split]
    prepared = {}
    originals = {}
    bound_starts = set()
    consumption_file = None
    source_collections = []
    for source in collections:
        directory = Path(source["directory"]).resolve(strict=True)
        require(within_root(directory, root), "collection directory outside sealed root")
        manifest_path, prepared_path = directory / "collection-manifest.json", directory / "prepared.jsonl"
        require(file_hash(manifest_path) == source["manifest_sha256"] and file_hash(prepared_path) == source["prepared_sha256"], "original collection files changed")
        manifest = validate_manifest(manifest_path)
        require(manifest["artifact_cohort_hash"] == cohort["artifact_hash"] and manifest.get("split", split) == split, "foreign collection scope")
        if split == "holdout":
            if consumption_file is None:
                consumption_file = manifest["holdout_consumption_file"]
            require(same_path(consumption_file, manifest["holdout_consumption_file"]), "foreign holdout opening")
        rows = read_lines(prepared_path)
        require(len(rows) == len(manifest["cases"]), "partial source preparation")
        events = read_lines(directory / "attempt-events.jsonl") if (directory / "attempt-events.jsonl").exists() else []
        if events:
            terminal = sealed(load(directory / "batch-terminal.json"))
            require(terminal["schema"] == "buglecat.verification.main-batch-terminal.v2"
                    and terminal["terminal_main"] is True and terminal["events_sha256"] == file_hash(directory / "attempt-events.jsonl")
                    and terminal["manifest_sha256"] == file_hash(manifest_path) and terminal["prepared_sha256"] == file_hash(prepared_path)
                    and terminal["artifact_cohort_hash"] == cohort["artifact_hash"], "main batch still running or terminal bytes changed")
        require(all(event["id"] in {row["id"] for row in rows} for event in events), "foreign original event")
        for row, case in zip(rows, manifest["cases"]):
            prepared_gate(row, case, manifest, directory / case["id"], task_map[case["task_id"]], executable)
            original = starts.get(case["group_id"])
            belongs = original is not None and same_path(original["case_directory"], row["case_directory"])
            if belongs:
                bound_starts.add(row["id"])
            if row["id"] in originals:
                previous = originals[row["id"]]
                require(all(previous[key] == row[key] for key in
                            ("task_id", "group_id", "source_kind", "split", "task_sha256", "public_interface", "body", "request_hash", "settings",
                             "oracle_runtime_binding", "artifact_cohort_hash", "build_info")), "unstarted preparation changed source/body")
                require(not (prepared[row["id"]]["main_status"] != "main_not_started" and belongs), "duplicate actual original start")
            originals[row["id"]] = row
            own_events = [event for event in events if event["id"] == row["id"]]
            require(belongs or not own_events, "events outside actual original preparation")
            result = outcome(row, task_map[case["task_id"]], original if belongs else None,
                             finished.get(case["group_id"]) if belongs else None, events, executable)
            result.update(authorization_file=roles["authorization"], authorization_sha256=file_hash(roles["authorization"]),
                          original_prepared=row, prepared_collection_directory=str(directory))
            if row["id"] not in prepared or belongs or prepared[row["id"]]["main_status"] == "main_not_started":
                prepared[row["id"]] = result
        source_collections.append({"directory": str(directory), "manifest_sha256": file_hash(manifest_path), "prepared_sha256": file_hash(prepared_path)})
    rows = []
    for case in allocated:
        if case["id"] in prepared:
            rows.append(prepared[case["id"]])
        else:
            require(case["group_id"] not in starts, "complete current-started denominator omitted a batch")
            rows.append({**case, "task": task_map[case["task_id"]], "candidate_code": None, "candidate_code_sha256": None,
                         "label": "unlabeled", "call_eligible": False, "main_status": "main_not_started", "reason": "main_not_started",
                         "main_receipt_file": None, "main_receipt_sha256": None, "oracle_file": None, "oracle_sha256": None, "formal_request": None})
    same_scope = {row["started"]["id"] for row in current["main_started"]
                  if row["started"].get("artifact_cohort_hash") == cohort["artifact_hash"] and row["started"].get("split") == split}
    require(same_scope <= bound_starts, "complete current-started denominator omitted a batch or original preparation")
    require(all(row["main_status"] != "main_not_finished" for row in rows), "in-flight original main group cannot be registered")
    cases = [{key: row.get(key) for key in ("id", "group_id", "task_id", "source_kind", "split", "label", "call_eligible", "main_status", "reason",
              "task_sha256", "candidate_code_sha256", "main_receipt_file", "main_receipt_sha256", "oracle_file", "oracle_sha256")} for row in rows]
    body = {"schema": SCHEMA, "split": split, "cases": cases, "collections": source_collections,
            "cohort_file": str(Path(cohort_file).resolve()), "cohort_sha256": file_hash(cohort_file), "cohort_hash": cohort["artifact_hash"],
            "protocol_file": roles["protocol"], "protocol_sha256": file_hash(roles["protocol"]),
            "authorization_file": roles["authorization"], "authorization_sha256": file_hash(roles["authorization"]),
            "oracle_runtime_binding": cohort["oracle_runtime_binding"],
            "collector_executable_file": str(Path(executable).resolve()), "collector_executable_sha256": file_hash(executable),
            "prepared_directory": str(root), "scores_hidden": True, "jev_probabilities_read": False,
            "allocated_groups": len(rows), "collected_groups": len(same_scope), "uncollected_groups": len(rows) - len(same_scope),
            "production_authorized": False, "native_claim_certification": False, "new_paid_calls": 0}
    if split == "holdout":
        from .artifact_v2_formal_holdout import validate as holdout_validate
        require(consumption_file is not None, "original holdout opening required")
        consumption = holdout_validate(consumption_file)
        body.update(holdout_consumption_file=consumption_file, holdout_consumption_sha256=file_hash(consumption_file),
                    holdout_consumption_hash=consumption["artifact_hash"], calibration_hash=consumption["calibration_hash"])
    return {**body, "artifact_hash": digest(body)}, rows


@source_transaction
def verify_dataset(dataset_file, prepared_directory, executable):
    data = sealed(load(dataset_file))
    require(data["schema"] == SCHEMA and same_path(data["prepared_directory"], prepared_directory)
            and same_path(data["collector_executable_file"], executable), "original formal v2 dataset required")
    actual, rows = collect_value(data["cohort_file"], prepared_directory, executable, data["collections"], data["split"])
    require(actual == data, "dataset differs from original mixed-source/transport/Oracle outcome")
    return actual, rows


@source_transaction
def record_oracles(cohort_file, prepared_directory, executable, collections, split="calibration"):
    """Root-only independent CPU assertions, after all batches are terminal.

    Reuses existing sealed Oracle receipts. Does not overwrite or rerun them,
    and never constructs any model transport or reservation.
    """
    _, rows = collect_value(cohort_file, prepared_directory, executable, collections, split)
    for row in rows:
        if row["candidate_code"] is not None and row["oracle_file"] is None:
            receipt = oracle_wrapper(row["task"], row["candidate_code"], row["source_kind"])
            create(Path(row["main_receipt_file"]).with_name("formal-oracle.json"), receipt)
    return collect_value(cohort_file, prepared_directory, executable, collections, split)
