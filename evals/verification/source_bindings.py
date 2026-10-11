"""Verify frozen source bytes and grouping before executing an oracle.

This authenticates recorded bindings, not the quality of acceptance assertions.
It has no transport and never edits a collected response or budget ledger.
"""

from pathlib import Path

from .calibration import digest, load, require
from .collection_manifest import file_hash, interfaces
from .public_benchmark import catalog


def verify_manifest(manifest):
    require(manifest.get("schema") == "buglecat.verification.mbpp-collection.v1", "collection schema mismatch")
    bindings = [(manifest[f"{name}_file"], manifest[f"{name}_sha256"])
                for name in ("source", "catalog", "authorization", "protocol")]
    if "resource_adjustment_file" in manifest:
        bindings.append((manifest["resource_adjustment_file"], manifest["resource_adjustment_sha256"]))
    check_unchanged(bindings)
    index = load(manifest["catalog_file"])
    rebuilt = catalog(manifest["source_file"], index["source_commit"], index["split_seed"])
    require(digest(rebuilt) == digest(index), "catalog does not match frozen source/grouping")
    source = load(manifest["source_file"])
    tasks = {t["task_id"]: t for t in source}
    rows = {r["task_id"]: r for r in index["tasks"]}
    ids, groups = set(), set()
    require(bool(manifest.get("cases")), "empty collection")
    for case in manifest["cases"]:
        identity, task_id, group = case["id"], case["task_id"], case["group_id"]
        require(identity == f"mbpp-main-{task_id}", "case identity mismatch")
        require(identity not in ids and group not in groups, "duplicate source task/group")
        ids.add(identity)
        groups.add(group)
        require(task_id in tasks and task_id in rows, "foreign source task")
        row = rows[task_id]
        require(digest(tasks[task_id]) == row["task_sha256"], "task bytes mismatch")
        require(case["split"] == row["reserved_split"] and group == row["group_id"], "source split/group mismatch")
        require(case["public_interface"] == interfaces(tasks[task_id]["code"]), "public interface mismatch")
    check_unchanged(bindings)
    return tasks, bindings


def check_unchanged(bindings):
    for path, expected in bindings:
        require(Path(path).is_file() and file_hash(path) == expected, "frozen source binding changed")


def bind_prepared(manifest, original):
    case = next((c for c in manifest["cases"] if c["id"] == original["id"]), None)
    require(case is not None, "prepared case absent from manifest")
    for field in ("id", "task_id", "group_id", "split"):
        require(original[field] == case[field], f"prepared {field} mismatch")
    for field in ("source_sha256", "authorization_sha256", "protocol_sha256"):
        require(original[field] == manifest[field], f"prepared {field} mismatch")
    for field in ("model", "reasoning_effort", "max_tokens", "timeout_s"):
        require(original["settings"][field] == manifest[field], f"prepared settings {field} mismatch")
    require(original["settings"]["max_retries"] == 0, "automatic retry not permitted")
    if "resource_adjustment_sha256" in manifest:
        adjustment = load(manifest["resource_adjustment_file"])
        cohort = original.get("resource_cohort", {})
        require(cohort.get("resource_adjustment_sha256") == manifest["resource_adjustment_sha256"]
                and cohort.get("cohort_id") == adjustment["cohort_id"]
                and cohort.get("previous_settings_sha256") == adjustment["previous_settings_sha256"],
                "prepared resource cohort mismatch")
