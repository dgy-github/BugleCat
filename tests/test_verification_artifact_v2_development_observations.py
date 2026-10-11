"""Windows original path identity; no paid calls or altered raw events."""
import os

import pytest

from evals.verification import artifact_v2_development as intake
from evals.verification import artifact_v2_development_observations as owner
from evals.verification.artifact_v2_development_cohort import same_path, within_root
from tests import test_verification_artifact_v2_development as fixtures

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows device-prefix filesystem regression")


@pytest.fixture
def attempt(tmp_path, monkeypatch):
    return fixtures.attempt.__wrapped__(tmp_path, monkeypatch)


def device(path):
    return "\\\\?\\" + str(path)


def test_device_prefix_is_same_windows_path_without_event_rewriting(attempt, monkeypatch):
    prepared, directory, exe, events, save = attempt
    monkeypatch.setattr(owner, "validate", lambda path: intake.validate(path))
    monkeypatch.setattr(owner, "resolve", lambda path: {"authorization": {"jev": {"ledger_dir": str(directory.parents[1])}}})
    monkeypatch.setattr(owner, "reservations", lambda *args: ([{
        "group_id": "group", "request_hash": events[0]["request_hash"]}], None))
    for event in events:
        for key in ("attempt_directory", "raw_response_file", "reservation_file"):
            event[key] = device(event[key])
    save()
    original = (directory / "events.jsonl").read_bytes()
    result = owner.observe(prepared, directory, exe)
    assert result["call_status"] == "ok" and result["acceptance_violation"] == .1
    assert (directory / "events.jsonl").read_bytes() == original
    assert result["formal_certification"] is False


@pytest.mark.parametrize("left,right", [
    (r"D:\ledger\attempts-other\case", r"D:\ledger\attempts"),
    (r"D:\ledger\attempts\..\outside", r"D:\ledger\attempts")])
def test_device_normalization_never_accepts_other_location(left, right):
    assert not same_path(left, right)
    assert not within_root(left, right)


def test_different_drive_fails_closed():
    with pytest.raises(ValueError):
        same_path(r"E:\ledger\attempts", r"D:\ledger\attempts")
