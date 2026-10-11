import json

import pytest

from evals.verification.budget_authorization import resolve
from evals.verification.collection_manifest import file_hash
from evals.verification.paid_budget import reserve


def files(tmp_path):
    original = tmp_path / "old.json"
    base = {"automatic_retries": 0, "main_model": {"max_attempts": 400, "ledger_dir": str(tmp_path / "main")},
            "jev": {"max_attempts": 400, "ledger_dir": str(tmp_path / "jev")}}
    original.write_text(json.dumps(base), encoding="utf-8")
    extended = tmp_path / "new.json"
    value = {**base, "schema": "buglecat.verification.paid-budget-extension.v1",
             "previous_authorization_file": str(original), "previous_authorization_sha256": file_hash(original),
             "additional_main_attempt_cap": 100, "additional_jev_attempt_cap": 100,
             "old_attempts_and_failures_count": True, "per_group_max_attempts": 1,
             "main_model": {**base["main_model"], "max_attempts": 500},
             "jev": {**base["jev"], "max_attempts": 500}}
    extended.write_text(json.dumps(value), encoding="utf-8")
    return original, extended, value


def test_extension_keeps_existing_reservation_index_and_original_metadata(tmp_path):
    old, new, _ = files(tmp_path)
    first = reserve(old, "jev", "old-group", "a" * 64)
    metadata = tmp_path / "jev/authorization.json"
    before = metadata.read_bytes()
    second = reserve(new, "jev", "new-group", "b" * 64)
    assert first["reservation_index"] == 1 and second["reservation_index"] == 2
    assert first["authorization_sha256"] == file_hash(old)
    assert second["authorization_sha256"] == file_hash(new)
    assert metadata.read_bytes() == before
    with pytest.raises(FileExistsError):
        reserve(new, "jev", "old-group", "c" * 64)
    assert resolve(new)["allowed_sha256"] == [file_hash(old), file_hash(new)]


@pytest.mark.parametrize("change", ["root", "cap", "prior", "retries"])
def test_extension_cannot_reset_root_or_expand_unapproved_caps(tmp_path, change):
    old, new, value = files(tmp_path)
    if change == "root":
        value["jev"]["ledger_dir"] = str(tmp_path / "reset")
    elif change == "cap":
        value["jev"]["max_attempts"] = 600
    elif change == "prior":
        old.write_text(old.read_text() + "\n", encoding="utf-8")
    else:
        value["per_group_max_attempts"] = 2
    new.write_text(json.dumps(value), encoding="utf-8")
    with pytest.raises(ValueError):
        resolve(new)
