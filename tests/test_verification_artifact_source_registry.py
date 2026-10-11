import json
import sqlite3

import pytest

from evals.verification.artifact_acceptance import POLICY, TEMPLATE
from evals.verification.artifact_source_registry import inventory
from evals.verification.collection_manifest import file_hash
from evals.verification.public_benchmark import catalog


def setup(tmp_path):
    source = tmp_path / "source.json"
    source.write_text(json.dumps([{"task_id": i, "prompt": "Multiply n", "code": f"def f(n): return n * {i}",
                                   "test_list": [f"assert f(2) == {2*i}"], "test_imports": []}
                                  for i in range(1, 5)]), encoding="utf-8")
    index = catalog(source, "a" * 40)
    catalog_file = tmp_path / "catalog.json"
    catalog_file.write_text(json.dumps(index), encoding="utf-8")
    auth_file, protocol = tmp_path / "auth.json", tmp_path / "protocol.json"
    main, jev = tmp_path / "main", tmp_path / "jev"
    main.mkdir()
    auth_file.write_text(json.dumps({"automatic_retries": 0,
                                    "main_model": {"max_attempts": 400, "ledger_dir": str(main)},
                                    "jev": {"max_attempts": 400, "ledger_dir": str(jev)}}), encoding="utf-8")
    protocol.write_text(json.dumps({"policy_version": POLICY, "template_version": TEMPLATE,
                                    "categories": ["artifact_acceptance"], "min_supported": 150,
                                    "min_unsupported": 50}), encoding="utf-8")
    group = index["tasks"][0]["group_id"]
    with sqlite3.connect(main / "main-attempts.sqlite3") as db:
        db.execute("CREATE TABLE metadata (key TEXT,value TEXT)")
        db.execute("INSERT INTO metadata VALUES (?,?)", ("authorization_sha256", file_hash(auth_file)))
        db.execute("CREATE TABLE attempts (ordinal INTEGER,group_id TEXT,started_json TEXT)")
        db.execute("INSERT INTO attempts VALUES (1,?,?)", (group, json.dumps({"group_id": group,
                                                                            "authorization_sha256": file_hash(auth_file)})))
    return (source, catalog_file, auth_file, protocol), group, main


def test_failed_or_pending_started_group_never_returns_to_fresh_pool(tmp_path):
    paths, group, main = setup(tmp_path)
    ledger = main / "main-attempts.sqlite3"
    before = file_hash(ledger)
    result = inventory(*paths)
    assert result["excluded_started_or_reserved_groups"] == [group]
    assert sum(result["fresh_group_counts"].values()) == 3
    assert result["remaining_main_attempts"] == 399
    assert result["new_paid_calls"] == 0 and not result["formal_registration"]
    assert not result["full_formal_campaign_possible_under_current_source_and_budget"]
    assert file_hash(ledger) == before


def test_changed_authorization_cannot_increase_apparent_budget(tmp_path):
    paths, _, _ = setup(tmp_path)
    paths[2].write_text(paths[2].read_text() + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="authorization mismatch"):
        inventory(*paths)
