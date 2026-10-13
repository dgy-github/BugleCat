import importlib.util
import json
import sqlite3
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("claim_history", ROOT / "scripts/collect_claim_history.py")
intake = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(intake)
AUDIT_SPEC = importlib.util.spec_from_file_location("history_audit", ROOT / "scripts/audit_claim_history.py")
auditor = importlib.util.module_from_spec(AUDIT_SPEC)
AUDIT_SPEC.loader.exec_module(auditor)


def test_intake_keeps_original_and_never_binds_future_evidence(tmp_path):
    items = [{"type": "toolResult", "id": "past", "text": "exit 1"},
             {"type": "assistantMessage", "text": "All tests passed."},
             {"type": "toolResult", "id": "future", "text": "exit 0"}]
    source = tmp_path / "source.json"
    raw = json.dumps({"threads": {"one": {"metadata": {"workspace": "real"},
                                               "turns": [{"items": items}]}}}).encode()
    source.write_bytes(raw)
    output = tmp_path / "capture"
    result = intake.collect(source, output)
    assert source.read_bytes() == (output / "original-thread-store.json").read_bytes() == raw
    assert result["captured_turns"] == 1
    assert result["formal_admitted_groups"] == 0
    row = result["cases"][0]
    assert row["label"] == "unlabeled" and not row["formal_eligible"]
    original = json.loads((output / row["original_file"]).read_bytes())
    assert original["candidate"] == "All tests passed."
    assert [i["id"] for i in original["preceding_tool_items"]] == ["past"]
    with pytest.raises(FileExistsError):
        intake.collect(source, output)


def test_same_workspace_not_counted_as_independent_groups():
    thread = {"metadata": {"workspace": "D:\\project"}, "turns": [{"items": [
        {"type": "toolResult"}, {"type": "assistantMessage", "text": "done"}]}]}
    raw = json.dumps({"threads": {"one": thread, "two": thread}}).encode()
    rows = intake.extract(raw, {intake.sha(s.encode()) for s in ("one", "two")})
    assert len(rows) == 2
    assert len({r["task_family_provisional"] for r in rows}) == 1


def test_event_backup_preserves_committed_wal_without_certifying(tmp_path):
    directory = tmp_path / "workspace/.nanocodex/sessions"
    directory.mkdir(parents=True)
    origin = sqlite3.connect(directory / "events.sqlite3")
    try:
        origin.execute("PRAGMA journal_mode=WAL")
        origin.execute("CREATE TABLE event (id INTEGER)")
        origin.execute("INSERT INTO event VALUES (7)")
        origin.commit()
        raw = json.dumps({"threads": {"one": {"metadata": {"workspace": str(tmp_path / "workspace")}}}}).encode()
        output = tmp_path / "snapshot"
        output.mkdir()
        rows = intake.capture_host_sources(raw, {intake.sha(b"one")}, output)
        assert rows[0]["claim_binding_verified"] is False
        with sqlite3.connect(output / rows[0]["event_snapshot"]["file"]) as copied:
            assert copied.execute("SELECT id FROM event").fetchall() == [(7,)]
    finally:
        origin.close()


def test_source_audit_matches_original_without_inventing_label(tmp_path, monkeypatch):
    workspace = tmp_path / "workspace"
    sessions = workspace / ".nanocodex/sessions"
    sessions.mkdir(parents=True)
    (sessions / "thread-one.jsonl").write_text(json.dumps({"role": "assistant", "content": "done"}))
    source = tmp_path / "threads.json"
    source.write_text(json.dumps({"threads": {"one": {"metadata": {"workspace": str(workspace)},
        "turns": [{"items": [{"type": "toolCall", "id": "call"},
                              {"type": "toolResult", "callId": "call", "success": True},
                              {"type": "assistantMessage", "text": "done"}]}]}}}))
    output = tmp_path / "capture"
    # Temporary test paths intentionally contain pytest; test source binding
    # independently of the production fixture-admission filter.
    monkeypatch.setattr(intake, "thread_inventory", lambda _: {"rows": [
        {"thread_hash": intake.sha(b"one"), "category": "unreviewed_candidate"}]})
    intake.collect(source, output)
    result = auditor.audit(output)
    assert result["session_text_matched_candidates"] == 1
    assert result["candidates_with_all_tool_ids_bound"] == 1
    assert result["cases"][0]["label"] == "unlabeled"
    assert result["formal_admitted_groups"] == 0
    (output / "original-thread-store.json").write_text("{}")
    with pytest.raises(ValueError, match="source hash changed"):
        auditor.audit(output)
