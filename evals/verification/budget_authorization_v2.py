"""Approved 250-request extension, preserving the frozen V1 resolver bytes.

The old 400 -> 500 lineage is validated by its original owner. New requests
share its existing ledger roots, metadata and reservation indices. No budget
is reset and no second V2 extension or automatic retry is accepted.
"""
from pathlib import Path

from .budget_authorization import resolve as resolve_v1
from .calibration import load, require
from .collection_manifest import file_hash

SCHEMA = "buglecat.verification.paid-budget-extension.v2"


def resolve(path):
    path = Path(path).resolve()
    value = load(path)
    if value.get("schema") != SCHEMA:
        return resolve_v1(path)
    previous_file = Path(value["previous_authorization_file"]).resolve()
    require(previous_file != path and file_hash(previous_file) == value["previous_authorization_sha256"],
            "previous authorization changed")
    previous = resolve_v1(previous_file)
    require(len(previous["allowed_sha256"]) == 2, "original 400 -> 500 lineage required")
    require(value.get("user_authorization") == "批准各追加最多250次"
            and type(value.get("additional_main_attempt_cap")) is int
            and type(value.get("additional_jev_attempt_cap")) is int
            and value["additional_main_attempt_cap"] == value["additional_jev_attempt_cap"] == 250,
            "extension exceeds explicit additional authorization")
    require(type(value.get("automatic_retries")) is int and value["automatic_retries"] == 0
            and type(value.get("per_group_max_attempts")) is int and value["per_group_max_attempts"] == 1
            and value.get("old_attempts_and_failures_count") is True,
            "extension cannot reset attempts or permit retries")
    for name in ("main_model", "jev"):
        require(type(value[name]["max_attempts"]) is int and value[name]["max_attempts"] == 750
                and previous["authorization"][name]["max_attempts"] == 500, "extension cap must be 750")
        require(Path(value[name]["ledger_dir"]).resolve()
                == Path(previous["authorization"][name]["ledger_dir"]).resolve(),
                "extension cannot reset ledger root")
    current = file_hash(path)
    return {"authorization": value, "current_sha256": current,
            "allowed_sha256": [*previous["allowed_sha256"], current],
            "ledger_authorization_sha256": previous["ledger_authorization_sha256"],
            "bindings": [*previous["bindings"], {"file": str(path), "sha256": current}]}
