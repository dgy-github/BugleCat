"""Resolve one explicit 100-request extension without rewriting old ledgers."""
from pathlib import Path

from .calibration import load, require
from .collection_manifest import file_hash


def resolve(path):
    path = Path(path).resolve()
    value = load(path)
    current = file_hash(path)
    require(value["automatic_retries"] == 0, "automatic retry not authorized")
    if value.get("schema") != "buglecat.verification.paid-budget-extension.v1":
        services = [name for name in ("main_model", "jev") if name in value]
        require(services and all(value[name]["max_attempts"] == 400 for name in services),
                "unsupported original authorization")
        return {"authorization": value, "current_sha256": current, "allowed_sha256": [current],
                "ledger_authorization_sha256": current, "bindings": [{"file": str(path), "sha256": current}]}
    previous_file = Path(value["previous_authorization_file"]).resolve()
    require(previous_file != path and file_hash(previous_file) == value["previous_authorization_sha256"],
            "previous authorization changed")
    previous = resolve(previous_file)
    require(len(previous["allowed_sha256"]) == 1, "only one approved extension supported")
    old = previous["authorization"]
    require(value["additional_main_attempt_cap"] == value["additional_jev_attempt_cap"] == 100,
            "extension exceeds explicit additional authorization")
    for name in ("main_model", "jev"):
        require(value[name]["max_attempts"] == 500 and old[name]["max_attempts"] == 400,
                "extension cap must be 500")
        require(Path(value[name]["ledger_dir"]).resolve() == Path(old[name]["ledger_dir"]).resolve(),
                "extension cannot reset ledger root")
    require(value["old_attempts_and_failures_count"] is True and value["per_group_max_attempts"] == 1,
            "extension cannot reset attempts or permit retries")
    return {"authorization": value, "current_sha256": current,
            "allowed_sha256": [*previous["allowed_sha256"], current],
            "ledger_authorization_sha256": previous["ledger_authorization_sha256"],
            "bindings": [*previous["bindings"], {"file": str(path), "sha256": current}]}
