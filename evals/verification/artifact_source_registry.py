"""Read-only inventory of fresh artifact sources before new collection.

No model request, reservation, labels or holdout opening. Existing failed and
pending groups remain excluded; changing lanes cannot restore their budget.
"""
import argparse
import json
import sqlite3
from pathlib import Path

from .artifact_acceptance import POLICY, TEMPLATE
from .budget_authorization import resolve
from .calibration import create, digest, load, require
from .collection_manifest import file_hash
from .public_benchmark import catalog


def inventory(source, catalog_file, authorization, protocol):
    paths = [Path(path).resolve() for path in (source, catalog_file, authorization, protocol)]
    hashes = [file_hash(path) for path in paths]
    index, auth, spec = map(load, paths[1:])
    chain = resolve(paths[2])
    extra = list(chain["bindings"])
    if index.get("catalog_extension") == "official-mbpp-preserve-original-split-v1":
        from .extended_mbpp import rebuild
        extra += [{"file": index[f"{name}_file"], "sha256": index[f"{name}_sha256"]}
                  for name in ("base_source", "base_catalog", "full_source")]
        rebuilt = rebuild(paths[0], index)
    else:
        rebuilt = catalog(paths[0], index["source_commit"], index["split_seed"])
    require(digest(rebuilt) == digest(index),
            "source catalog changed")
    require(spec["policy_version"] == POLICY and spec["template_version"] == TEMPLATE
            and spec["categories"] == ["artifact_acceptance"]
            and spec["min_supported"] == 150 and spec["min_unsupported"] == 50,
            "artifact source protocol mismatch")
    ledger = Path(auth["main_model"]["ledger_dir"]) / "main-attempts.sqlite3"
    with sqlite3.connect(ledger.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        metadata = dict(connection.execute("SELECT key,value FROM metadata"))
        attempts = list(connection.execute("SELECT group_id,started_json FROM attempts ORDER BY ordinal"))
    require(metadata["authorization_sha256"] == chain["ledger_authorization_sha256"], "main ledger authorization mismatch")
    excluded = set()
    for group, raw in attempts:
        started = json.loads(raw)
        require(started["group_id"] == group and started["authorization_sha256"] in chain["allowed_sha256"],
                "main started group binding mismatch")
        require(group not in excluded, "repeated main group")
        excluded.add(group)
    jev_root = Path(auth["jev"]["ledger_dir"])
    reservations = list((jev_root / "reservations").glob("*.json"))
    if reservations:
        require(load(jev_root / "authorization.json") == {"authorization_sha256": chain["ledger_authorization_sha256"], "kind": "jev"},
                "Jev ledger authorization mismatch")
    reserved_groups = set()
    for path in reservations:
        value = load(path)
        require(value["kind"] == "jev" and value["authorization_sha256"] in chain["allowed_sha256"]
                and value["automatic_retries"] == 0 and value["group_id"] not in reserved_groups,
                "Jev reservation group binding mismatch")
        reserved_groups.add(value["group_id"])
    main_cap, jev_cap = auth["main_model"]["max_attempts"], auth["jev"]["max_attempts"]
    require(len(attempts) <= main_cap and len(reservations) <= jev_cap, "shared budget exceeded")
    # All already started groups are excluded, including unknown/pending/failed.
    excluded |= reserved_groups
    fresh = {}
    for row in index["tasks"]:
        if row["group_id"] not in excluded:
            fresh.setdefault(row["group_id"], row)
    require(excluded <= {row["group_id"] for row in index["tasks"]}, "foreign previously started source")
    counts = {split: sum(row["reserved_split"] == split for row in fresh.values())
              for split in ("calibration", "holdout")}
    per_stage = spec["min_supported"] + spec["min_unsupported"]
    main_remaining, jev_remaining = main_cap - len(attempts), jev_cap - len(reservations)
    blockers = []
    if main_remaining < 2 * per_stage:
        blockers.append("insufficient_remaining_main_attempts_even_if_all_valid")
    if jev_remaining < 2 * per_stage:
        blockers.append("insufficient_remaining_jev_reservations")
    for split, count in counts.items():
        if count < per_stage:
            blockers.append(f"insufficient_fresh_{split}_source_groups")
    require([file_hash(path) for path in paths] == hashes, "source files changed during registry")
    require(all(file_hash(binding["file"]) == binding["sha256"] for binding in extra),
            "source or authorization chain changed during registry")
    body = {"schema": "buglecat.verification.artifact-source-inventory.v1",
            "bindings": [{"file": str(path), "sha256": hash_value} for path, hash_value in zip(paths, hashes)],
            "dependency_bindings": extra,
            "excluded_started_or_reserved_groups": sorted(excluded), "fresh_group_counts": counts,
            "remaining_main_attempts": main_remaining, "remaining_jev_reservations": jev_remaining,
            "minimum_binary_groups_per_stage": per_stage, "blockers": blockers,
            "full_formal_campaign_possible_under_current_source_and_budget": not blockers,
            "negative_group_count_unknown": True, "formal_registration": False,
            "holdout_opened": False, "new_paid_calls": 0, "production_authorized": False}
    return {**body, "artifact_hash": digest(body)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "catalog", "authorization", "protocol", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    result = inventory(args.source, args.catalog, args.authorization, args.protocol)
    create(args.output, result)
    print(json.dumps({key: result[key] for key in ("fresh_group_counts", "remaining_main_attempts",
                                                  "remaining_jev_reservations", "blockers", "new_paid_calls")}))


if __name__ == "__main__":
    main()
