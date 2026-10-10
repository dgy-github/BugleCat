import json

import pytest

from evals.verification.paid_budget import reserve


def authorization(tmp_path):
    path = tmp_path / "auth.json"
    path.write_text(json.dumps({"jev": {"max_attempts": 400, "ledger_dir": str(tmp_path / "ledger")},
                                "automatic_retries": 0}))
    return path


def test_reservation_survives_and_duplicate_group_cannot_retry(tmp_path):
    auth = authorization(tmp_path)
    first = reserve(auth, "jev", "group", "a" * 64)
    assert first["reservation_index"] == 1
    with pytest.raises(FileExistsError):
        reserve(auth, "jev", "group", "b" * 64)
    assert reserve(auth, "jev", "other", "c" * 64)["reservation_index"] == 2


def test_budget_exhaustion_counts_interrupted_files(tmp_path):
    auth = authorization(tmp_path)
    root = tmp_path / "ledger" / "reservations"
    root.mkdir(parents=True)
    for i in range(400):
        (root / f"{i}.json").write_text("")
    with pytest.raises(ValueError, match="exhausted"):
        reserve(auth, "jev", "next", "a" * 64)


def test_changed_authorization_and_stale_lock_block_spending(tmp_path):
    auth = authorization(tmp_path)
    reserve(auth, "jev", "one", "a" * 64)
    auth.write_text(auth.read_text() + " ")
    with pytest.raises(ValueError, match="authorization changed"):
        reserve(auth, "jev", "two", "a" * 64)
    (tmp_path / "ledger" / "reservation.lock").write_text("interrupted")
    with pytest.raises(FileExistsError):
        reserve(auth, "jev", "two", "a" * 64)
