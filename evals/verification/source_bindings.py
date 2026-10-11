"""Verify frozen source bytes and grouping before executing an oracle.

This authenticates recorded bindings, not the quality of acceptance assertions.
It has no transport and never edits a collected response or budget ledger.
"""

from contextlib import contextmanager
from contextvars import ContextVar
from copy import deepcopy
from functools import wraps
from pathlib import Path

from .calibration import digest, load, require
from .collection_manifest import file_hash, interfaces
from .public_benchmark import catalog

_TRANSACTION = ContextVar("verified_source_manifests", default=None)


@contextmanager
def verified_sources():
    """Reuse a complete source proof only within one read-only operation.

    Ledger, reservations, candidate/Oracle and transport proofs are not cached.
    Each hit still hashes every original bound input. Returned data is copied.
    """
    current = _TRANSACTION.get()
    if current is not None:
        yield
        return
    token = _TRANSACTION.set({})
    try:
        yield
    finally:
        _TRANSACTION.reset(token)


def source_transaction(function):
    @wraps(function)
    def wrapped(*args, **kwargs):
        with verified_sources():
            return function(*args, **kwargs)
    return wrapped


def verify_manifest(manifest):
    cache = _TRANSACTION.get()
    if cache is None:
        return _verify_manifest(manifest)
    identity = digest(manifest)
    if identity in cache:
        tasks, bindings = cache[identity]
        check_unchanged(bindings)
        return deepcopy(tasks), list(bindings)
    result = _verify_manifest(manifest)
    cache[identity] = deepcopy(result)
    return result


def _source_proof(manifest):
    bindings = [(manifest[f"{name}_file"], manifest[f"{name}_sha256"])
                for name in ("source", "catalog", "authorization", "protocol")]
    if "resource_adjustment_file" in manifest:
        bindings.append((manifest["resource_adjustment_file"], manifest["resource_adjustment_sha256"]))
    check_unchanged(bindings)
    cache = _TRANSACTION.get()
    identity = ("catalog", digest(bindings))
    if cache is not None and identity in cache:
        tasks, rows, original_bindings = cache[identity]
        check_unchanged(original_bindings)
        return deepcopy(tasks), deepcopy(rows), list(original_bindings)
    index = load(manifest["catalog_file"])
    if index.get("catalog_extension") == "official-mbpp-preserve-original-split-v1":
        from .extended_mbpp import rebuild
        bindings.extend((index[f"{name}_file"], index[f"{name}_sha256"])
                        for name in ("base_source", "base_catalog", "full_source"))
        rebuilt = rebuild(manifest["source_file"], index)
    else:
        rebuilt = catalog(manifest["source_file"], index["source_commit"], index["split_seed"])
    require(digest(rebuilt) == digest(index), "catalog does not match frozen source/grouping")
    source = load(manifest["source_file"])
    tasks = {t["task_id"]: t for t in source}
    rows = {r["task_id"]: r for r in index["tasks"]}
    check_unchanged(bindings)
    result = tasks, rows, bindings
    if cache is not None:
        cache[identity] = deepcopy(result)
    return result


def _verify_manifest(manifest):
    require(manifest.get("schema") == "buglecat.verification.mbpp-collection.v1", "collection schema mismatch")
    tasks, rows, bindings = _source_proof(manifest)
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
