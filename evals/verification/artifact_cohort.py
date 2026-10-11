"""Zero-network collection preregistration, distinct from source inventory.

A rejected campaign records blockers and emits no collectible cases. Only new
calibration candidates can be preregistered; this does not open the holdout,
assign labels, reserve requests, certify thresholds or enable production.
"""

import argparse
import hashlib
import json
import re
import sqlite3
from pathlib import Path

from . import artifact_source_registry
from .artifact_acceptance import POLICY, TEMPLATE
from .budget_authorization import resolve
from .calibration import digest, load, require
from .collection_manifest import file_hash, interfaces
from .paid_budget import durable_new

SCHEMA = "buglecat.verification.artifact-cohort.v1"
OFFICIAL_SOURCE_SHA = "ca95deaa9a01ef0a6f439f88bcf0dd3db3563d22f22aad6cae04ebb9a8d8c8e9"
OFFICIAL_COMMIT = "f82046ba5aabbbb427dbfd38a254d26bff08b533"
OFFICIAL_FULL_SHA = "ccf64ceae9c5403bf50a044cb6d505bfd2a2963ee58338ba268fd65beab92a9f"
GRID = [0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.975, 0.99, 1.0]
MODULE_ROOT = Path(__file__).resolve().parent
ORACLE_SOURCES = [MODULE_ROOT / "benchmark_oracle.py", MODULE_ROOT / "benchmark_oracle_worker.py"]
PROMPT_SOURCE = MODULE_ROOT.parents[1] / "rust/crates/ncx-core/examples/verification_collect/support.rs"


def binding(role, path):
    path = Path(path).resolve(strict=True)
    return {"role": role, "file": str(path), "sha256": file_hash(path)}


def protocol_gate(spec):
    expected = {"schema": "buglecat.verification.artifact-protocol.v1",
                "policy_version": POLICY, "template_version": TEMPLATE,
                "categories": ["artifact_acceptance"], "model": "jev-1.13.0",
                "requested_model": "jev-latest", "question_name": "acceptance_violation",
                "threshold_grid": GRID, "min_supported": 150, "min_unsupported": 50,
                "max_false_block_rate": 0.02, "min_error_rejection_lower_bound": 0.5,
                "confidence": 0.95, "formal_registration_before_jev_scores": True,
                "holdout_requires_qualified_calibration": True, "production_authorized": False}
    require(all(spec.get(k) == v and type(spec.get(k)) is type(v)
                for k, v in expected.items()), "frozen artifact protocol mismatch")


def external_bindings(source, index, fixture):
    """Expanded source is reproducible from both pinned official byte streams."""
    if index.get("catalog_extension") == "official-mbpp-preserve-original-split-v1":
        from .extended_mbpp import rebuild
        if not fixture:
            require(index["base_source_sha256"] == OFFICIAL_SOURCE_SHA
                    and index["full_source_sha256"] == OFFICIAL_FULL_SHA
                    and index["source_commit"] == OFFICIAL_COMMIT,
                    "unrecognized expanded official source")
        require(rebuild(source, index) == index, "expanded source reconstruction mismatch")
        return [binding(f"expanded_{name}", index[f"{name}_file"])
                for name in ("base_source", "base_catalog", "full_source")] + [
                    binding("expansion_source", MODULE_ROOT / "extended_mbpp.py")]
    if not fixture:
        require(file_hash(source) == OFFICIAL_SOURCE_SHA and index["source_commit"] == OFFICIAL_COMMIT,
                "unrecognized external source; synthetic inputs require --fixture")
    return []


def reservations(root, kind, chain):
    """Read existing durable reservations; .started markers never add slots."""
    root = Path(root).resolve()
    require(not (root / "reservation.lock").exists(), "reservation transaction in progress")
    paths = sorted((root / "reservations").glob("*.json"))
    rows, bindings, groups, indices = [], [], set(), set()
    metadata = root / "authorization.json"
    if paths or metadata.exists():
        require(load(metadata) == {"authorization_sha256": chain["ledger_authorization_sha256"], "kind": kind},
                "reservation authorization mismatch")
        bindings.append(binding(f"{kind}_reservation_authorization", metadata))
    cap = chain["authorization"][kind]["max_attempts"]
    allowed_caps = {row["sha256"]: load(row["file"])[kind]["max_attempts"] for row in chain["bindings"]}
    for path in paths:
        row = load(path)
        group, ordinal = row.get("group_id"), row.get("reservation_index")
        request_hash = row.get("request_hash")
        require(row.get("schema") == "buglecat.verification.paid-reservation.v1"
                and row.get("kind") == kind and row.get("authorization_sha256") in chain["allowed_sha256"]
                and row.get("automatic_retries") == 0
                and row.get("status") == "reserved_before_transport", "invalid reservation")
        require(isinstance(group, str) and bool(group) and group not in groups
                and type(ordinal) is int and 1 <= ordinal <= allowed_caps[row["authorization_sha256"]]
                and ordinal not in indices,
                "duplicate or invalid reservation group/index")
        require(isinstance(request_hash, str) and len(request_hash) == 64
                and all(c in "0123456789abcdef" for c in request_hash), "invalid reserved request hash")
        require(path.name == digest(group) + ".json", "reservation filename/group mismatch")
        rows.append(row)
        bindings.append(binding(f"{kind}_reservation", path))
        groups.add(group)
        indices.add(ordinal)
    require(len(rows) <= cap, "shared reservation budget exceeded")
    return rows, bindings


def ledger_snapshot(auth_file):
    """Snapshot logical SQLite rows, including WAL-visible starts, read-only."""
    chain = resolve(auth_file)
    auth = chain["authorization"]
    main_cap, jev_cap = auth["main_model"]["max_attempts"], auth["jev"]["max_attempts"]
    allowed_caps = {row["sha256"]: load(row["file"])["main_model"]["max_attempts"] for row in chain["bindings"]}
    path = (Path(auth["main_model"]["ledger_dir"]) / "main-attempts.sqlite3").resolve(strict=True)
    with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as db:
        db.execute("BEGIN")
        metadata = list(db.execute("SELECT key,value FROM metadata ORDER BY key"))
        raw = list(db.execute("SELECT ordinal,group_id,started_json FROM attempts ORDER BY ordinal"))
    require(len(dict(metadata)) == len(metadata)
            and dict(metadata).get("authorization_sha256") == chain["ledger_authorization_sha256"],
            "main ledger authorization mismatch")
    rows, groups, indices = [], set(), set()
    for ordinal, group, encoded in raw:
        started = json.loads(encoded)
        require(started.get("group_id") == group and started.get("authorization_sha256") in chain["allowed_sha256"],
                "main started binding mismatch")
        require(type(ordinal) is int and 1 <= ordinal <= allowed_caps[started["authorization_sha256"]]
                and ordinal not in indices
                and isinstance(group, str) and bool(group) and group not in groups,
                "duplicate or invalid main attempt group/index")
        groups.add(group)
        indices.add(ordinal)
        rows.append({"ordinal": ordinal, "group_id": group, "started": started})
    main_reserved, main_bind = reservations(auth["main_model"]["ledger_dir"], "main_model", chain)
    jev_reserved, jev_bind = reservations(auth["jev"]["ledger_dir"], "jev", chain)
    main_groups = groups | {row["group_id"] for row in main_reserved}
    excluded = main_groups | {row["group_id"] for row in jev_reserved}
    require(len(main_groups) <= main_cap, "shared main budget exceeded")
    body = {"main_ledger_file": str(path), "main_metadata": metadata, "main_started": rows,
            "main_reservations": main_reserved, "jev_reservations": jev_reserved,
            "authorization_chain": {k: chain[k] for k in ("current_sha256", "allowed_sha256",
                                                         "ledger_authorization_sha256", "bindings")},
            "main_attempt_cap": main_cap, "jev_reservation_cap": jev_cap,
            "excluded_groups": sorted(excluded), "remaining_main_attempts": main_cap - len(main_groups),
            "remaining_jev_reservations": jev_cap - len(jev_reserved)}
    return {**body, "snapshot_hash": digest(body)}, main_bind + jev_bind


def preregister(source, catalog_file, authorization, protocol, cohort_id, *, fixture=False):
    """Return a source-bound preregistration or sealed rejection, without writes.

    Fixture mode exercises the same quotas but is never formal registration.
    Exposure flags refer to future model candidates, not public task familiarity.
    Consumer must recheck all seals and reserve groups atomically before sending.
    """
    require(isinstance(cohort_id, str) and bool(cohort_id.strip()), "missing cohort ID")
    paths = [Path(p).resolve(strict=True) for p in (source, catalog_file, authorization, protocol)]
    source, catalog_file, authorization, protocol = paths
    inventory = artifact_source_registry.inventory(*paths)
    index, spec = load(catalog_file), load(protocol)
    protocol_gate(spec)
    source_bindings = external_bindings(source, index, fixture)
    snapshot, ledger_bindings = ledger_snapshot(authorization)
    require(set(inventory["excluded_started_or_reserved_groups"]) <= set(snapshot["excluded_groups"]),
            "ledger changed during preregistration")
    bindings = [binding(role, path) for role, path in zip(
        ("source", "catalog", "authorization", "protocol"), paths)]
    bindings += [binding("oracle_source", path) for path in ORACLE_SOURCES]
    bindings += [binding("registry_source", artifact_source_registry.__file__),
                 binding("cohort_source", __file__),
                 binding("budget_resolver_source", MODULE_ROOT / "budget_authorization.py")]
    bindings += source_bindings
    bindings.append(binding("main_prompt_source", PROMPT_SOURCE))
    prompt_match = re.search(r'pub const SYSTEM: &str = ("[^\n]+");', PROMPT_SOURCE.read_text(encoding="utf-8"))
    require(prompt_match is not None, "original main prompt missing")
    system_prompt_hash = hashlib.sha256(json.loads(prompt_match.group(1)).encode()).hexdigest()
    bindings += [binding("authorization_ancestor", row["file"])
                 for row in snapshot["authorization_chain"]["bindings"][:-1]] + ledger_bindings
    excluded, fresh = set(snapshot["excluded_groups"]), {}
    tasks = {task["task_id"]: task for task in load(source)}
    require(excluded <= {row["group_id"] for row in index["tasks"]}, "foreign previously reserved source")
    for row in sorted(index["tasks"], key=lambda item: item["task_id"]):
        if row["group_id"] not in excluded:
            fresh.setdefault(row["group_id"], row)
    counts = {split: sum(row["reserved_split"] == split for row in fresh.values())
              for split in ("calibration", "holdout")}
    blockers = []
    if snapshot["remaining_main_attempts"] < 400:
        blockers.append("insufficient_remaining_main_attempts_even_if_all_valid")
    if snapshot["remaining_jev_reservations"] < 400:
        blockers.append("insufficient_remaining_jev_reservations")
    for split, count in counts.items():
        if count < 200:
            blockers.append(f"insufficient_fresh_{split}_source_groups")
    cases = []
    allocation = min(counts["calibration"], max(0, snapshot["remaining_main_attempts"] - 200),
                     max(0, snapshot["remaining_jev_reservations"] - 200))
    if not blockers:
        ordered = sorted(fresh, key=lambda g: digest({"seed": index["split_seed"], "group": g}))
        for group in [g for g in ordered if fresh[g]["reserved_split"] == "calibration"][:allocation]:
            row, task = fresh[group], tasks[fresh[group]["task_id"]]
            public = interfaces(task["code"])
            cases.append({"id": f"mbpp-main-{row['task_id']}", "group_id": group,
                          "task_id": row["task_id"], "split": "calibration",
                          "source_sha256": index["source_sha256"], "task_sha256": row["task_sha256"],
                          "public_interface": public, "public_interface_sha256": digest(public),
                          "category": "artifact_acceptance", "label": "unlabeled",
                          "source_kind": "code_fixture" if fixture else "external_mbpp_fresh_collection",
                          "author_exposed": False, "main_model_candidate": "not_collected"})
    # A logical snapshot joins WAL rows; hashing just the SQLite file is not enough.
    final_snapshot, final_bindings = ledger_snapshot(authorization)
    require(snapshot == final_snapshot and ledger_bindings == final_bindings,
            "shared ledger changed during preregistration")
    require(all(file_hash(row["file"]) == row["sha256"] for row in bindings),
            "bound file changed during preregistration")
    require(artifact_source_registry.inventory(*paths) == inventory, "inventory changed during preregistration")
    registered = not blockers and not fixture
    body = {"schema": SCHEMA, "cohort_id": cohort_id, "registration_kind": "collection_preregistration",
            "status": "rejected" if blockers else "fixture_only" if fixture else "registered",
            "main_collection_settings": {"model": "deepseek-flash", "reasoning_effort": "ultra",
                                         "max_tokens": 8192, "timeout_s": 60, "max_retries": 0,
                                         "provider_protocol": "openai"},
            "main_system_prompt_sha256": system_prompt_hash,
            "main_allocation": {"calibration_max_groups": allocation,
                                "holdout_min_groups_reserved": 200,
                                "remaining_main_at_registration": snapshot["remaining_main_attempts"],
                                "remaining_jev_at_registration": snapshot["remaining_jev_reservations"]},
            "split": "calibration", "cases": cases, "bindings": bindings,
            "inventory": inventory, "inventory_hash": inventory["artifact_hash"],
            "ledger_snapshot": snapshot, "excluded_started_or_reserved_groups": sorted(excluded),
            "fresh_group_counts": counts, "blockers": blockers, "full_campaign_source_gate": not blockers,
            "formal_registration": registered, "fixture_only": fixture,
            "before_main_requests": True, "scores_hidden": True, "jev_probabilities_read": False,
            "author_exposed": False, "exposure_scope": "future_main_model_candidates_only",
            "holdout_opened": False, "holdout_cases_emitted": False,
            "holdout_requires_qualified_calibration": True, "labels_assigned": False,
            "negative_group_count_unknown": True, "source_admission_executed": False,
            "collection_authorized": registered, "certified_threshold": None,
            "new_paid_calls": 0, "production_authorized": False,
            "limitations": ["Task pools can overlap model training; this is not deployment certification.",
                            "Source counts cannot guarantee 150 supported and 50 unsupported results.",
                            "Preregistration is not Oracle admission, label registration or calibration.",
                            "Reservation/transport consumers must revalidate seals and uniqueness."]}
    return {**body, "artifact_hash": digest(body)}


def register(source, catalog_file, authorization, protocol, cohort_id, output, *, fixture=False):
    value = preregister(source, catalog_file, authorization, protocol, cohort_id, fixture=fixture)
    durable_new(output, value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "catalog", "authorization", "protocol", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--cohort-id", required=True)
    parser.add_argument("--fixture", action="store_true", help="code regression only, never formal data")
    args = parser.parse_args()
    value = register(args.source, args.catalog, args.authorization, args.protocol,
                     args.cohort_id, args.output, fixture=args.fixture)
    print(json.dumps({k: value[k] for k in ("status", "fresh_group_counts", "blockers",
                                           "formal_registration", "artifact_hash", "new_paid_calls")}))
    return 0 if value["formal_registration"] or (args.fixture and not value["blockers"]) else 2


if __name__ == "__main__":
    raise SystemExit(main())
