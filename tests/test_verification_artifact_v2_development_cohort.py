"""Zero-network source order and original shared SQLite proof fixtures."""
import json
import sqlite3
from pathlib import Path

import pytest

from evals.verification import artifact_v2_development_cohort as owner
from evals.verification.calibration import digest


def sources():
    tasks = [{"task_id": n, "prompt": f"task {n}", "code": f"def f{n}(x):\n    return x\n",
              "test_imports": [], "test_list": [f"assert f{n}(1) == 1"]} for n in range(20)]
    rows = [{"task_id": t["task_id"], "task_sha256": digest(t), "group_id": f"group-{t['task_id']}",
             "reserved_split": "holdout" if t["task_id"] >= 15 else "calibration"} for t in tasks]
    return tasks, {"tasks": rows}


def test_complete_old_allocation_and_all_started_reserved_excluded():
    tasks, index = sources()
    previous = {"cases": [{"group_id": "group-0"}, {"group_id": "group-1"}]}
    baseline = {"excluded_groups": ["group-2", "group-3"]}
    cases, excluded = owner.chosen(tasks, index, previous, baseline, 8)
    assert len(cases) == 8
    assert not {c["group_id"] for c in cases} & set(excluded)
    assert all(c["task_id"] < 15 and c["split"] == "development" for c in cases)
    assert cases == owner.chosen(tasks, index, previous, baseline, 8)[0]


@pytest.mark.parametrize("count", [True, 0, 9, -1])
def test_eight_only_no_budget_or_pool_expansion(count):
    tasks, index = sources()
    with pytest.raises(ValueError):
        owner.chosen(tasks, index, {"cases": []}, {"excluded_groups": []}, count)


def test_unavailable_fresh_prefix_refuses_not_rename_holdout():
    tasks, index = sources()
    with pytest.raises(ValueError):
        owner.chosen(tasks, index, {"cases": [{"group_id": f"group-{n}"} for n in range(13)]}, {"excluded_groups": []}, 8)
    assert sum(r["reserved_split"] == "holdout" for r in index["tasks"]) == 5


def ledger_fixture(tmp_path, monkeypatch):
    main, jev = tmp_path / "main", tmp_path / "jev"
    main.mkdir()
    jev.mkdir()
    auth_file = tmp_path / "auth.json"
    auth = {"main_model": {"ledger_dir": str(main), "max_attempts": 750}, "jev": {"ledger_dir": str(jev), "max_attempts": 750}}
    auth_file.write_text(json.dumps(auth))
    chain = {"authorization": auth, "ledger_authorization_sha256": "original", "allowed_sha256": ["original", "current"],
             "bindings": [{"file": str(auth_file), "sha256": "current"}]}
    monkeypatch.setattr(owner, "resolve", lambda _: chain)
    monkeypatch.setattr(owner, "reservations", lambda root, kind, chain: ([], []))
    db = sqlite3.connect(main / "main-attempts.sqlite3")
    db.executescript("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT, group_id TEXT UNIQUE, started_json TEXT, finished_json TEXT);")
    db.execute("INSERT INTO metadata VALUES('authorization_sha256','original')")
    for n in range(267):
        started = {"id": f"old-{n}", "group_id": f"group-{n}", "authorization_sha256": "current"}
        db.execute("INSERT INTO attempts(group_id,started_json) VALUES(?,?)", (started["group_id"], json.dumps(started)))
    db.commit()
    return auth_file, db


def test_actual_sqlite267_pending_preserved_cap750(tmp_path, monkeypatch):
    auth, db = ledger_fixture(tmp_path, monkeypatch)
    before = list(db.execute("SELECT * FROM attempts"))
    snap, finished = owner.snapshot(auth)
    assert len(snap["main_started"]) == 267 and snap["remaining_main_attempts"] == 483
    assert all(value is None for value in finished.values())
    assert list(db.execute("SELECT * FROM attempts")) == before
    db.close()


def test_original_metadata_reset_and_foreign_auth_rejected(tmp_path, monkeypatch):
    auth, db = ledger_fixture(tmp_path, monkeypatch)
    db.execute("UPDATE metadata SET value='reset'")
    db.commit()
    with pytest.raises(ValueError):
        owner.snapshot(auth)
    db.execute("UPDATE metadata SET value='original'")
    db.execute("UPDATE attempts SET started_json=? WHERE ordinal=267", (json.dumps({"group_id": "group-266", "authorization_sha256": "foreign"}),))
    db.commit()
    with pytest.raises(ValueError):
        owner.snapshot(auth)
    db.close()


def test_original_source_prompt_has_no_test_or_reference_leak():
    task = {"prompt": "Compute double", "code": "SECRET_REFERENCE", "test_list": ["SECRET_ASSERT"]}
    prompt = owner.user_prompt(task, {"public_interface": ["def double(x):"]})
    assert "SECRET_" not in prompt and "No execution was performed" in prompt
    assert prompt.startswith("Task:\nCompute double")


def test_windows_extended_identity_and_outside_scope(tmp_path):
    assert owner.same_path(tmp_path, Path("\\\\?\\" + str(tmp_path)))
    assert not owner.same_path(tmp_path, tmp_path / "child")
    assert not owner.within_root(tmp_path.parent / "foreign", tmp_path)


def collected_fixture(tmp_path, monkeypatch):
    """Owner-level denominator/Oracle fixtures; binary suite tests transport."""
    task = {"task_id": 1, "prompt": "Return x", "code": "def f(x):\n    return x\n",
            "test_imports": [], "test_list": ["assert f(1) == 1"]}
    source = tmp_path / "source.json"
    source.write_text(json.dumps([task]))
    executable = tmp_path / "binary.exe"
    executable.write_bytes(b"offline-test-fixture")
    manifest = {"cases": [{"id": f"case-{n}", "group_id": f"g-{n}", "task_id": 1,
                           "split": "development", "public_interface": owner.interfaces(task["code"])} for n in range(4)],
                "source_file": str(source), "authorization_file": "unused-fixture",
                "artifact_cohort_file": "unused-cohort", "artifact_cohort_sha256": "raw-cohort",
                "artifact_cohort_hash": "cohort", "authorization_sha256": "auth"}
    (tmp_path / "collection-manifest.json").write_text(json.dumps(manifest))
    monkeypatch.setattr(owner, "validate_manifest", lambda _: manifest)
    monkeypatch.setattr(owner, "validate_cohort", lambda _: {"artifact_hash": "cohort"})
    monkeypatch.setattr(owner, "verify", lambda *args: None)
    paths = {"runner_source_sha256": owner.EXAMPLES / "verification_artifact_v2_collect.rs",
             "support_source_sha256": owner.EXAMPLES / "verification_artifact_v2_collect/support.rs",
             "collection_engine_source_sha256": owner.EXAMPLES / "verification_artifact_v2_collect/engine.rs",
             "frozen_collector_support_sha256": owner.EXAMPLES / "verification_artifact_collect/support.rs",
             "profile_collector_validator_source_sha256": owner.EXAMPLES / "verification_artifact_profile_collect/support.rs",
             "legacy_support_source_sha256": owner.PROMPT,
             "request_builder_source_sha256": owner.ROOT / "rust/crates/ncx-provider/src/request.rs",
             "response_parser_source_sha256": owner.ROOT / "rust/crates/ncx-provider/src/response.rs",
             "development_owner_source_sha256": owner.MODULE,
             "budget_v2_owner_source_sha256": owner.MODULE.with_name("budget_authorization_v2.py")}
    info = {key: owner.file_hash(path) for key, path in paths.items()}
    info["executable_sha256"] = owner.file_hash(executable)
    rows, events, starts, finished = [], [], [], {}
    for n, case in enumerate(manifest["cases"]):
        directory = tmp_path / case["id"]
        workspace = directory / "workspace"
        workspace.mkdir(parents=True)
        request = directory / "request.json"
        request.write_text(json.dumps({"fixture": n}))
        row = {**case, "case_directory": str(directory), "workspace": str(workspace),
               "session_log": str(workspace / "session.jsonl"), "manifest_sha256": owner.file_hash(tmp_path / "collection-manifest.json"),
               "prompt": owner.user_prompt(task, case), "task_sha256": digest(task),
               "settings": owner.SETTINGS, "build_info": info, "body": {"fixture": n},
               "request_hash": owner.file_hash(request), **{key: manifest[key] for key in
                  ("artifact_cohort_file", "artifact_cohort_sha256", "artifact_cohort_hash", "authorization_sha256")}}
        rows.append({**row, "artifact_sha256": digest(row)})
        if n == 0:
            continue
        start = {"event": "attempt_started", "id": case["id"], "group_id": case["group_id"],
                 "artifact_cohort_hash": "cohort", "request_hash": row["request_hash"], "build_info": info}
        starts.append({"group_id": case["group_id"], "started": start})
        events.append({**start, "attempt_ordinal": n})
        finished[case["group_id"]] = None
        if n == 1:
            continue
        status = "timeout" if n == 2 else "ok"
        receipt = {"request_hash": row["request_hash"], "actual_request_hash": row["request_hash"],
                   "call_status": status, "result": {"candidate": "```python\ndef f(x):\n    return x\n```\n"}}
        receipt_path = directory / "receipt.json"
        receipt_path.write_text(json.dumps({**receipt, "artifact_sha256": digest(receipt)}))
        events.append({**start, "event": "request_submitted"})
        finish = {**start, "event": "attempt_finished", "call_status": status, "receipt_sha256": owner.file_hash(receipt_path)}
        events.append(finish)
        finished[case["group_id"]] = finish
    (tmp_path / "prepared.jsonl").write_text("\n".join(json.dumps(r) for r in rows))
    (tmp_path / "attempt-events.jsonl").write_text("\n".join(json.dumps(e) for e in events))
    monkeypatch.setattr(owner, "snapshot", lambda _: ({"main_started": starts}, finished))
    return executable, task


def test_complete_allocation_preserves_unstarted_pending_failure_and_unlabeled(tmp_path, monkeypatch):
    executable, _ = collected_fixture(tmp_path, monkeypatch)
    rows = owner.verify_collected(tmp_path, executable)
    assert len(rows) == 4
    assert [r["reason"] for r in rows] == ["main_not_started", "main_not_finished", "timeout", "independent_development_oracle_not_recorded"]
    assert all(r["label"] == "unknown" and not r["call_eligible"] for r in rows)
    assert rows[-1]["candidate_code"] == "def f(x):\n    return x\n"


@pytest.mark.parametrize("status,label", [("passed", "supported"), ("failed", "unsupported"), ("unknown", "unknown")])
def test_independent_oracle_only_assigns_scoped_binary_label(tmp_path, monkeypatch, status, label):
    executable, task = collected_fixture(tmp_path, monkeypatch)
    receipt = {"schema": "buglecat.verification.benchmark-assertion-receipt.v1", "task_id": 1,
               "task_sha256": digest(task), "candidate_code_sha256": owner.hashlib.sha256(task["code"].encode()).hexdigest(),
               "oracle_worker_sha256": owner.file_hash(owner.MODULE.with_name("benchmark_oracle_worker.py")),
               "oracle_scope": "frozen MBPP acceptance tests only; not full task semantics", "test_count": 1,
               "result": {"status": status}, "independent_of_jev": True, "secrets_removed": True, "paid_attempts": 0}
    target = tmp_path / "case-3/development-oracle.json"
    target.write_text(json.dumps({**receipt, "artifact_hash": digest(receipt)}))
    row = owner.verify_collected(tmp_path, executable)[-1]
    assert row["label"] == label and row["call_eligible"] == (label != "unknown")
    receipt["independent_of_jev"] = False
    target.write_text(json.dumps({**receipt, "artifact_hash": digest(receipt)}))
    with pytest.raises(ValueError, match="Oracle binding"):
        owner.verify_collected(tmp_path, executable)
