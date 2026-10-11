"""Code fixtures verify registration guards; none count as benchmark labels."""

import json
import sqlite3
import subprocess
import sys
from pathlib import Path

import pytest

from evals.verification import artifact_cohort
from evals.verification.calibration import digest, load
from evals.verification.collection_manifest import file_hash
from evals.verification.extended_mbpp import SEED, derive, rebuild
from evals.verification.paid_budget import reserve
from evals.verification.public_benchmark import catalog

ROOT = Path(__file__).resolve().parents[1]
PROTOCOL = ROOT / "evals/verification/reports/artifact_protocol_20261011_v1.json"


def fixture(tmp_path, task_n=400, duplicate=False):
    source, catalog_file = tmp_path / "source.json", tmp_path / "catalog.json"
    tasks = [{"task_id": i, "prompt": f"Multiply n by {i}", "code": f"def f(n): return n * {i}",
              "test_list": [f"assert f(2) == {2*i}"], "test_imports": []}
             for i in range(1, task_n + 1)]
    if duplicate:
        tasks.append({**tasks[0], "task_id": task_n + 1, "code": "def other(value): return value * 1"})
    source.write_text(json.dumps(tasks), encoding="utf-8")
    index = catalog(source, "a" * 40)
    catalog_file.write_text(json.dumps(index), encoding="utf-8")
    main, jev = tmp_path / "main", tmp_path / "jev"
    main.mkdir()
    auth, protocol = tmp_path / "auth.json", tmp_path / "protocol.json"
    auth.write_text(json.dumps({"automatic_retries": 0,
                               "main_model": {"max_attempts": 400, "ledger_dir": str(main)},
                               "jev": {"max_attempts": 400, "ledger_dir": str(jev)}}), encoding="utf-8")
    protocol.write_bytes(PROTOCOL.read_bytes())
    with sqlite3.connect(main / "main-attempts.sqlite3") as db:
        db.execute("CREATE TABLE metadata (key TEXT,value TEXT)")
        db.execute("INSERT INTO metadata VALUES (?,?)", ("authorization_sha256", file_hash(auth)))
        db.execute("CREATE TABLE attempts (ordinal INTEGER,group_id TEXT,started_json TEXT)")
    return (source, catalog_file, auth, protocol), index, main


def start(paths, main, group):
    with sqlite3.connect(main / "main-attempts.sqlite3") as db:
        db.execute("INSERT INTO attempts VALUES (1,?,?)", (group, json.dumps(
            {"group_id": group, "authorization_sha256": file_hash(paths[2])})))


def check_seal(value):
    assert digest({k: v for k, v in value.items() if k != "artifact_hash"}) == value["artifact_hash"]
    assert all(file_hash(row["file"]) == row["sha256"] for row in value["bindings"])


def extend(paths):
    original = load(paths[2])
    extension = paths[2].parent / "extension.json"
    extension.write_text(json.dumps({"schema": "buglecat.verification.paid-budget-extension.v1",
                                    "previous_authorization_file": str(paths[2]),
                                    "previous_authorization_sha256": file_hash(paths[2]),
                                    "additional_main_attempt_cap": 100, "additional_jev_attempt_cap": 100,
                                    "main_model": {**original["main_model"], "max_attempts": 500},
                                    "jev": {**original["jev"], "max_attempts": 500},
                                    "automatic_retries": 0, "per_group_max_attempts": 1,
                                    "old_attempts_and_failures_count": True}), encoding="utf-8")
    return (paths[0], paths[1], extension, paths[3])


def test_extension_counts_mixed_original_and_new_authorizations_without_reset(tmp_path):
    paths, index, main = fixture(tmp_path, task_n=404)
    groups = [row["group_id"] for row in index["tasks"]][:4]
    start(paths, main, groups[0])
    old_slot = reserve(paths[2], "jev", groups[0], "a" * 64)
    metadata = main / "main-attempts.sqlite3"
    with sqlite3.connect(metadata) as db:
        old_sql_metadata = db.execute("SELECT key,value FROM metadata").fetchall()
    old_slot_hash = file_hash(old_slot["path"])
    extended = extend(paths)
    with sqlite3.connect(metadata) as db:
        db.execute("INSERT INTO attempts VALUES (2,?,?)", (groups[1], json.dumps(
            {"group_id": groups[1], "authorization_sha256": file_hash(extended[2])})))
    reserve(extended[2], "jev", groups[2], "b" * 64)
    reserve(extended[2], "main_model", groups[3], "c" * 64)
    before = file_hash(metadata)
    value = artifact_cohort.preregister(*extended, "extension-fixture", fixture=True)
    snapshot = value["ledger_snapshot"]
    assert snapshot["remaining_main_attempts"] == 497
    assert snapshot["remaining_jev_reservations"] == 498
    assert snapshot["main_attempt_cap"] == snapshot["jev_reservation_cap"] == 500
    assert snapshot["authorization_chain"]["allowed_sha256"] == [file_hash(paths[2]), file_hash(extended[2])]
    assert snapshot["authorization_chain"]["ledger_authorization_sha256"] == file_hash(paths[2])
    assert len(value["excluded_started_or_reserved_groups"]) == 4
    assert value["full_campaign_source_gate"] and not value["formal_registration"]
    assert file_hash(metadata) == before and file_hash(old_slot["path"]) == old_slot_hash
    with sqlite3.connect(metadata) as db:
        assert db.execute("SELECT key,value FROM metadata").fetchall() == old_sql_metadata
    assert load(Path(load(paths[2])["jev"]["ledger_dir"]) / "authorization.json")["authorization_sha256"] == file_hash(paths[2])
    check_seal(value)


@pytest.mark.parametrize("mutation", ["root", "reset", "previous", "metadata"])
def test_extension_cannot_change_root_previous_hash_or_reset_original_metadata(tmp_path, mutation):
    paths, _, main = fixture(tmp_path)
    extended = extend(paths)
    value = load(extended[2])
    if mutation == "root":
        value["main_model"]["ledger_dir"] = str(tmp_path / "new-budget")
    elif mutation == "reset":
        value["old_attempts_and_failures_count"] = False
    elif mutation == "previous":
        paths[2].write_text(paths[2].read_text(encoding="utf-8") + "\n", encoding="utf-8")
    else:
        with sqlite3.connect(main / "main-attempts.sqlite3") as db:
            db.execute("UPDATE metadata SET value=? WHERE key='authorization_sha256'", (file_hash(extended[2]),))
    if mutation in {"root", "reset"}:
        extended[2].write_text(json.dumps(value), encoding="utf-8")
    with pytest.raises(ValueError, match="reset|previous authorization changed|ledger authorization mismatch"):
        artifact_cohort.preregister(*extended, "illegal-extension", fixture=True)


def test_old_authorization_cannot_retroactively_authorize_attempt_above_400(tmp_path):
    paths, index, main = fixture(tmp_path, task_n=404)
    group = index["tasks"][0]["group_id"]
    with sqlite3.connect(main / "main-attempts.sqlite3") as db:
        db.execute("INSERT INTO attempts VALUES (401,?,?)", (group, json.dumps(
            {"group_id": group, "authorization_sha256": file_hash(paths[2])})))
    extended = extend(paths)
    with pytest.raises(ValueError, match="invalid main attempt group/index"):
        artifact_cohort.preregister(*extended, "retroactive", fixture=True)


def test_expanded_fixture_preserves_original_splits_and_binds_all_official_inputs(tmp_path):
    paths, original, _ = fixture(tmp_path, task_n=4)
    full = tmp_path / "full.jsonl"
    supplemental = [{"task_id": i, "text": f"Multiply n by {i}", "code": f"def f(n): return n * {i}",
                     "test_list": [f"assert f(2) == {2*i}"], "test_setup_code": ""}
                    for i in range(5, 405)]
    full.write_text("\n".join(json.dumps(row) for row in supplemental), encoding="utf-8")
    tasks, _ = derive(load(paths[0]), supplemental)
    source, index_path = tmp_path / "expanded.json", tmp_path / "expanded-catalog.json"
    source.write_text(json.dumps(tasks), encoding="utf-8")
    description = {"source_commit": original["source_commit"], "split_seed": SEED}
    for name, path in (("base_source", paths[0]), ("base_catalog", paths[1]), ("full_source", full)):
        description[f"{name}_file"], description[f"{name}_sha256"] = str(path), file_hash(path)
    index_path.write_text(json.dumps(rebuild(source, description)), encoding="utf-8")
    value = artifact_cohort.preregister(source, index_path, paths[2], paths[3], "expanded-fixture", fixture=True)
    assert value["full_campaign_source_gate"] and not value["formal_registration"]
    roles = {row["role"] for row in value["bindings"]}
    assert {"expanded_base_source", "expanded_base_catalog", "expanded_full_source", "expansion_source"} <= roles
    check_seal(value)
    with pytest.raises(ValueError, match="unrecognized expanded official source"):
        artifact_cohort.preregister(source, index_path, paths[2], paths[3], "not-official")
    full.write_text(full.read_text(encoding="utf-8") + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="provenance changed"):
        artifact_cohort.preregister(source, index_path, paths[2], paths[3], "tampered", fixture=True)


def test_full_source_fixture_is_not_formal_and_emits_no_holdout(tmp_path):
    paths, index, main = fixture(tmp_path, duplicate=True)
    before = file_hash(main / "main-attempts.sqlite3")
    value = artifact_cohort.register(*paths, "unit-fixture", tmp_path / "new.json", fixture=True)
    assert value["status"] == "fixture_only" and value["full_campaign_source_gate"]
    assert not value["formal_registration"] and not value["collection_authorized"]
    assert len(value["cases"]) == 200 and len({c["group_id"] for c in value["cases"]}) == 200
    assert {c["split"] for c in value["cases"]} == {"calibration"}
    for case in value["cases"]:
        assert case["public_interface"] == ["def f(n):"]
        assert case["public_interface_sha256"] == digest(case["public_interface"])
        assert case["source_kind"] == "code_fixture" and case["label"] == "unlabeled"
        assert case["group_id"] in {row["group_id"] for row in index["tasks"]}
        assert not any(k in case for k in ("code", "test_list", "candidate", "noul", "reference"))
    assert not value["holdout_opened"] and not value["jev_probabilities_read"]
    assert value["negative_group_count_unknown"] and value["certified_threshold"] is None
    assert file_hash(main / "main-attempts.sqlite3") == before
    check_seal(value)
    with pytest.raises(FileExistsError):
        artifact_cohort.register(*paths, "unit-fixture", tmp_path / "new.json", fixture=True)


def test_started_failed_pending_and_reserved_groups_are_all_excluded(tmp_path):
    paths, index, main = fixture(tmp_path)
    groups = [r["group_id"] for r in index["tasks"]][:3]
    start(paths, main, groups[0])  # No finish is required: even pending consumes a slot.
    reserve(paths[2], "main_model", groups[1], "a" * 64)
    reservation = reserve(paths[2], "jev", groups[2], "b" * 64)
    Path(reservation["path"]).with_suffix(".started").write_text("interrupted", encoding="utf-8")
    value = artifact_cohort.preregister(*paths, "blocked", fixture=True)
    assert set(value["excluded_started_or_reserved_groups"]) == set(groups)
    assert value["ledger_snapshot"]["remaining_main_attempts"] == 398
    assert value["ledger_snapshot"]["remaining_jev_reservations"] == 399
    assert value["status"] == "rejected" and value["cases"] == []
    assert not value["formal_registration"] and not value["full_campaign_source_gate"]


def test_main_reservation_and_same_started_group_consume_one_slot(tmp_path):
    paths, index, main = fixture(tmp_path)
    group = index["tasks"][0]["group_id"]
    reserve(paths[2], "main_model", group, "a" * 64)
    start(paths, main, group)
    value = artifact_cohort.preregister(*paths, "overlap", fixture=True)
    assert value["ledger_snapshot"]["remaining_main_attempts"] == 399


def test_unrecognized_author_constructed_source_cannot_be_formal(tmp_path):
    paths, _, _ = fixture(tmp_path)
    with pytest.raises(ValueError, match="unrecognized external source"):
        artifact_cohort.preregister(*paths, "not-real")


@pytest.mark.parametrize("field,value", [("min_supported", 149), ("min_unsupported", 49),
                                       ("confidence", 0.9), ("threshold_grid", [0.99]),
                                       ("formal_registration_before_jev_scores", False)])
def test_gates_cannot_be_lowered(tmp_path, field, value):
    paths, _, _ = fixture(tmp_path)
    spec = load(paths[3])
    spec[field] = value
    paths[3].write_text(json.dumps(spec), encoding="utf-8")
    with pytest.raises(ValueError, match="protocol mismatch"):
        artifact_cohort.preregister(*paths, "weak", fixture=True)


def test_tampered_catalog_or_oracle_source_during_registration_rejected(tmp_path, monkeypatch):
    paths, _, _ = fixture(tmp_path)
    oracle = tmp_path / "oracle.py"
    oracle.write_text("original", encoding="utf-8")
    monkeypatch.setattr(artifact_cohort, "ORACLE_SOURCES", [oracle])
    original = artifact_cohort.ledger_snapshot
    calls = []

    def mutate(auth):
        result = original(auth)
        calls.append(True)
        if len(calls) == 2:
            oracle.write_text("changed", encoding="utf-8")
        return result

    monkeypatch.setattr(artifact_cohort, "ledger_snapshot", mutate)
    with pytest.raises(ValueError, match="bound file changed"):
        artifact_cohort.preregister(*paths, "race", fixture=True)
    index = load(paths[1])
    index["tasks"][0]["reserved_split"] = "holdout" if index["tasks"][0]["reserved_split"] == "calibration" else "calibration"
    paths[1].write_text(json.dumps(index), encoding="utf-8")
    with pytest.raises(ValueError, match="catalog changed"):
        artifact_cohort.preregister(*paths, "split-tamper", fixture=True)


def test_new_reservation_during_registration_rejected(tmp_path, monkeypatch):
    paths, index, _ = fixture(tmp_path)
    original = artifact_cohort.ledger_snapshot
    calls = []

    def mutate(auth):
        calls.append(True)
        if len(calls) == 2:
            reserve(auth, "jev", index["tasks"][0]["group_id"], "a" * 64)
        return original(auth)

    monkeypatch.setattr(artifact_cohort, "ledger_snapshot", mutate)
    with pytest.raises(ValueError, match="shared ledger changed"):
        artifact_cohort.preregister(*paths, "race", fixture=True)


def test_reservation_rename_and_transaction_lock_rejected(tmp_path):
    paths, index, _ = fixture(tmp_path)
    slot = reserve(paths[2], "jev", index["tasks"][0]["group_id"], "a" * 64)
    path = Path(slot["path"])
    path.rename(path.parent / "foreign.json")
    with pytest.raises(ValueError, match="filename/group mismatch"):
        artifact_cohort.preregister(*paths, "renamed", fixture=True)
    (path.parent.parent / "reservation.lock").write_text("active", encoding="utf-8")
    with pytest.raises(ValueError, match="transaction in progress"):
        artifact_cohort.preregister(*paths, "lock", fixture=True)


def test_cli_refusal_emits_sealed_rejection_and_no_collection(tmp_path):
    paths, _, _ = fixture(tmp_path, task_n=398)
    output = tmp_path / "rejection.json"
    command = [sys.executable, "-m", "evals.verification.artifact_cohort"]
    for flag, path in zip(("source", "catalog", "authorization", "protocol"), paths):
        command += [f"--{flag}", str(path)]
    command += ["--cohort-id", "cli-fixture", "--fixture", "--output", str(output)]
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False)
    assert result.returncode == 2
    value = load(output)
    assert value["fresh_group_counts"] == {"calibration": 199, "holdout": 199}
    assert value["cases"] == [] and value["new_paid_calls"] == 0
    check_seal(value)
    assert "--split" not in subprocess.run(command[:3] + ["--help"], cwd=ROOT, capture_output=True, text=True).stdout
