import hashlib
import json
import sqlite3

import pytest

from evals.verification.collection_status import status


def ledger(tmp_path):
    root = tmp_path / "ledger"
    root.mkdir()
    auth = tmp_path / "authorization.json"
    auth.write_text(json.dumps({"main_model": {"max_attempts": 400, "ledger_dir": str(root)}, "automatic_retries": 0}))
    digest = hashlib.sha256(auth.read_bytes()).hexdigest()
    start = {"id": "case-1", "group_id": "group-1", "authorization_sha256": digest}
    with sqlite3.connect(root / "main-attempts.sqlite3") as db:
        db.executescript("CREATE TABLE metadata(key TEXT,value TEXT); CREATE TABLE attempts(ordinal INTEGER,group_id TEXT,started_json TEXT,finished_json TEXT);")
        db.execute("INSERT INTO metadata VALUES('authorization_sha256',?)", (digest,))
        db.execute("INSERT INTO attempts VALUES(1,?,?,NULL)", ("group-1", json.dumps(start)))
    return auth, root


def test_unfinished_attempt_is_spent_without_fabricating_usage(tmp_path):
    auth, root = ledger(tmp_path)
    before = hashlib.sha256((root / "main-attempts.sqlite3").read_bytes()).hexdigest()
    result = status(auth, [])
    assert result["global_main_attempts"] == 1 and result["remaining_main_attempts"] == 399
    assert result["global_main_finished"] == 0 and result["amount"] is None
    assert result["missing_usage_is_not_zero_cost"] is True
    assert hashlib.sha256((root / "main-attempts.sqlite3").read_bytes()).hexdigest() == before


def test_auth_change_cannot_read_old_ledger_as_fresh_budget(tmp_path):
    auth, _ = ledger(tmp_path)
    auth.write_text(auth.read_text() + "\n")
    with pytest.raises(ValueError, match="authorization mismatch"):
        status(auth, [])


def test_detached_receipt_cannot_be_counted_as_completed_response(tmp_path):
    auth, _ = ledger(tmp_path)
    case = tmp_path / "collection" / "case-1"
    case.mkdir(parents=True)
    (case / "receipt.json").write_text(json.dumps({"attempt_ordinal": 1, "id": "case-1", "group_id": "group-1"}))
    with pytest.raises(ValueError, match="no matching finished"):
        status(auth, [case.parent])
