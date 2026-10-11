import json

import pytest

from evals.verification.budget_authorization_v2 import resolve
from evals.verification.collection_manifest import file_hash
from evals.verification.paid_budget import reserve
from tests.test_verification_budget_authorization import files


def extended(tmp_path):
    original, previous, base = files(tmp_path)
    current = tmp_path / "approved-250.json"
    value = {**base, "schema": "buglecat.verification.paid-budget-extension.v2",
             "user_authorization": "批准各追加最多250次",
             "previous_authorization_file": str(previous), "previous_authorization_sha256": file_hash(previous),
             "additional_main_attempt_cap": 250, "additional_jev_attempt_cap": 250,
             "main_model": {**base["main_model"], "max_attempts": 750},
             "jev": {**base["jev"], "max_attempts": 750}}
    current.write_text(json.dumps(value), encoding="utf-8")
    return original, previous, current, value


def test_new_extension_uses_original_root_index_and_failure_reservation(tmp_path):
    original, previous, current, _ = extended(tmp_path)
    # Reserved but unfinished groups remain spent; no finish/refund is required.
    first = reserve(original, "jev", "unfinished-old", "a" * 64)
    metadata = tmp_path / "jev/authorization.json"
    before = metadata.read_bytes()
    second = reserve(previous, "jev", "unfinished-500", "b" * 64)
    third = reserve(current, "jev", "fresh-750", "c" * 64)
    assert [row["reservation_index"] for row in (first, second, third)] == [1, 2, 3]
    assert metadata.read_bytes() == before
    assert resolve(current)["allowed_sha256"] == [file_hash(original), file_hash(previous), file_hash(current)]
    with pytest.raises(FileExistsError):
        reserve(current, "jev", "unfinished-old", "d" * 64)


@pytest.mark.parametrize("change", [
    lambda value, root: value["jev"].update(ledger_dir=str(root / "reset")),
    lambda value, root: value["main_model"].update(max_attempts=751),
    lambda value, root: value.update(additional_main_attempt_cap=251),
    lambda value, root: value.update(per_group_max_attempts=2),
    lambda value, root: value.update(automatic_retries=1),
    lambda value, root: value.update(old_attempts_and_failures_count=False),
    lambda value, root: value.update(user_authorization="unapproved"),
    lambda value, root: value.update(per_group_max_attempts=True),
])
def test_unapproved_expansion_or_reset_rejected(tmp_path, change):
    _, _, current, value = extended(tmp_path)
    change(value, tmp_path)
    current.write_text(json.dumps(value), encoding="utf-8")
    with pytest.raises(ValueError):
        resolve(current)
    assert not (tmp_path / "jev").exists()


def test_ancestor_tampering_and_repeat_extension_rejected(tmp_path):
    _, previous, current, value = extended(tmp_path)
    newer = tmp_path / "unapproved-repeat.json"
    repeat = {**value, "previous_authorization_file": str(current),
              "previous_authorization_sha256": file_hash(current)}
    newer.write_text(json.dumps(repeat), encoding="utf-8")
    with pytest.raises(ValueError):
        resolve(newer)
    previous.write_bytes(previous.read_bytes() + b"\n")
    with pytest.raises(ValueError, match="previous authorization changed"):
        resolve(current)
