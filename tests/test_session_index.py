"""Tests for session snapshots (offline, pure logic + tmp-file store).

Covers the three layers kept deliberately separate (mirroring schedule.py):

* ``summarize`` — the DETERMINISTIC, zero-cost readout of a message list
  (title from first user line, snippet from last assistant line, counts, recent
  tool names), with no clock/network/model.
* ``SessionIndex`` — the JSONL store keyed by **session_id**: each conversation
  is its own row (re-opening a project mints a new id => a SEPARATE history
  entry, never an overwrite), newest-activity-first for the directory list.
* full-transcript SNAPSHOTS — frozen per-conversation message lists so the GUI
  detail view replays the real conversation, not a digest.

Legacy workspace-keyed rows (the pre-snapshot format) must still load.
"""

from __future__ import annotations

from nanocodex.agent.session_index import (
    SessionIndex,
    SessionSummary,
    new_session_id,
    summarize,
)

# --- summarize: deterministic readout -------------------------------------


def _msgs():
    return [
        {"role": "system", "content": "you are a helpful agent"},
        {"role": "user", "content": "fix the login bug"},
        {"role": "assistant", "content": "looking into it",
         "tool_calls": [{"id": "1", "type": "function",
                         "function": {"name": "read_file", "arguments": "{}"}}]},
        {"role": "tool", "tool_call_id": "1", "name": "read_file", "content": "..."},
        {"role": "assistant", "content": "fixed it and added a test"},
    ]


def test_summarize_pulls_title_snippet_and_counts():
    s = summarize("sid1", "/proj", _msgs(), log_path="/proj/.nanocodex/session.jsonl",
                  now_iso="2026-06-01T10:00:00")
    assert s.session_id == "sid1"
    assert s.workspace == "/proj"
    assert s.title == "fix the login bug"          # first user line
    assert s.snippet == "fixed it and added a test"  # last assistant line
    assert s.user_messages == 1
    assert s.assistant_messages == 2
    assert s.tool_calls == 1
    assert s.recent_tools == ["read_file"]
    assert s.updated_at == "2026-06-01T10:00:00"
    assert s.created_at == "2026-06-01T10:00:00"   # defaults to now when unset
    assert s.log_path == "/proj/.nanocodex/session.jsonl"


def test_summarize_ignores_system_and_handles_empty():
    # Only a system message -> no real conversation yet.
    s = summarize("sid", "/p", [{"role": "system", "content": "sys"}])
    assert s.title == "(no prompt yet)"
    assert s.user_messages == 0 and s.assistant_messages == 0


def test_summarize_skips_compaction_marker_as_title():
    # A compaction summary is injected as a user message; it must NOT become the
    # human-facing title — the real first prompt should win.
    msgs = [
        {"role": "user", "content": "[Earlier conversation compacted ...]"},
        {"role": "user", "content": "the real question"},
    ]
    assert summarize("sid", "/p", msgs).title == "the real question"


def test_summarize_flattens_block_content():
    # User content can be a block list (text + image); title uses the text.
    msgs = [{"role": "user", "content": [
        {"type": "text", "text": "describe this"},
        {"type": "image_url", "image_url": {"url": "data:..."}},
    ]}]
    assert summarize("sid", "/p", msgs).title == "describe this"


def test_summarize_clips_long_title():
    long = "x" * 500
    s = summarize("sid", "/p", [{"role": "user", "content": long}])
    assert len(s.title) <= 120
    assert s.title.endswith("…")


def test_summarize_keeps_only_last_8_tools():
    calls = [{"id": str(i), "type": "function",
              "function": {"name": f"t{i}", "arguments": "{}"}} for i in range(12)]
    msgs = [{"role": "assistant", "content": "", "tool_calls": calls}]
    s = summarize("sid", "/p", msgs)
    assert s.tool_calls == 12
    assert s.recent_tools == [f"t{i}" for i in range(4, 12)]  # last 8


def test_summarize_carries_created_at_and_has_snapshot():
    s = summarize("sid", "/p", _msgs(), now_iso="2026-06-01T11:00:00",
                  created_at="2026-06-01T09:00:00", has_snapshot=True)
    assert s.created_at == "2026-06-01T09:00:00"   # explicit start preserved
    assert s.updated_at == "2026-06-01T11:00:00"
    assert s.has_snapshot is True


def test_new_session_id_is_unique():
    assert new_session_id() != new_session_id()


# --- SessionIndex: JSONL store keyed by session_id ------------------------


def _index(tmp_path):
    return SessionIndex(path=tmp_path / "sessions.jsonl")


def test_record_turn_then_get(tmp_path):
    idx = _index(tmp_path)
    idx.record_turn("sid1", "/proj", _msgs(),
                    log_path="/proj/.nanocodex/session.jsonl",
                    now_iso="2026-06-01T10:00:00")
    got = idx.get("sid1")
    assert got is not None
    assert got.title == "fix the login bug"
    assert got.has_snapshot is True                # snapshot frozen on record


def test_same_session_id_upserts_one_row(tmp_path):
    idx = _index(tmp_path)
    idx.record_turn("sid1", "/proj", _msgs(), now_iso="2026-06-01T10:00:00")
    idx.record_turn("sid1", "/proj", _msgs() + [{"role": "user", "content": "more"}],
                    now_iso="2026-06-01T11:00:00")
    entries = idx.entries()
    assert len(entries) == 1                       # same id -> one row
    assert entries[0].updated_at == "2026-06-01T11:00:00"  # newer write won
    assert entries[0].created_at == "2026-06-01T10:00:00"  # start preserved
    assert entries[0].user_messages == 2


def test_new_session_id_same_workspace_keeps_separate_history(tmp_path):
    # The whole point of snapshots: re-opening the same project is a NEW
    # conversation, not an overwrite of the old one.
    idx = _index(tmp_path)
    idx.record_turn("sidA", "/proj", _msgs(), now_iso="2026-06-01T10:00:00")
    idx.record_turn("sidB", "/proj", _msgs(), now_iso="2026-06-02T10:00:00")
    entries = idx.entries()
    assert len(entries) == 2                        # two separate histories
    assert {e.session_id for e in entries} == {"sidA", "sidB"}


def test_entries_sorted_newest_first(tmp_path):
    idx = _index(tmp_path)
    idx.record_turn("old", "/a", _msgs(), now_iso="2026-06-01T09:00:00")
    idx.record_turn("new", "/b", _msgs(), now_iso="2026-06-01T12:00:00")
    idx.record_turn("mid", "/c", _msgs(), now_iso="2026-06-01T10:30:00")
    order = [e.session_id for e in idx.entries()]
    assert order == ["new", "mid", "old"]


def test_persists_across_instances(tmp_path):
    p = tmp_path / "sessions.jsonl"
    idx1 = SessionIndex(path=p)
    idx1.record_turn("sid1", "/proj", _msgs(), now_iso="2026-06-01T10:00:00")
    # A fresh instance reading the same file sees the entry.
    idx2 = SessionIndex(path=p)
    assert idx2.get("sid1") is not None
    assert idx2.get("sid1").title == "fix the login bug"


def test_reload_folds_duplicate_lines_last_wins(tmp_path):
    # Two lines for one session_id (e.g. a crash mid-rewrite): LAST wins on load.
    p = tmp_path / "sessions.jsonl"
    p.write_text(
        '{"session_id": "s", "workspace": "/p", "title": "old", "updated_at": "2026-06-01T09:00:00"}\n'
        '{"session_id": "s", "workspace": "/p", "title": "new", "updated_at": "2026-06-01T11:00:00"}\n',
        encoding="utf-8",
    )
    idx = SessionIndex(path=p)
    assert len(idx.entries()) == 1
    assert idx.get("s").title == "new"


def test_legacy_workspace_rows_still_load(tmp_path):
    # Pre-snapshot rows had only a workspace (no session_id). They must still
    # list, under a synthetic legacy:<workspace> id, with no snapshot.
    p = tmp_path / "sessions.jsonl"
    p.write_text(
        '{"workspace": "/old", "title": "legacy work", "updated_at": "2026-06-01T10:00:00"}\n',
        encoding="utf-8",
    )
    idx = SessionIndex(path=p)
    entries = idx.entries()
    assert len(entries) == 1
    assert entries[0].session_id == "legacy:/old"
    assert entries[0].title == "legacy work"
    assert entries[0].has_snapshot is False
    assert idx.get("legacy:/old") is not None


def test_load_tolerates_garbage_lines(tmp_path):
    p = tmp_path / "sessions.jsonl"
    p.write_text(
        'not json\n'
        '{"neither_id_nor_workspace": true}\n'
        '{"session_id": "ok", "workspace": "/p", "title": "ok", "updated_at": "2026-06-01T10:00:00"}\n'
        '{partial',
        encoding="utf-8",
    )
    idx = SessionIndex(path=p)
    assert [e.session_id for e in idx.entries()] == ["ok"]


def test_missing_file_is_empty(tmp_path):
    idx = SessionIndex(path=tmp_path / "nope.jsonl")
    assert idx.entries() == []
    assert idx.get("anything") is None


def test_record_summary_directly(tmp_path):
    idx = _index(tmp_path)
    idx.record(SessionSummary(session_id="s", workspace="/p", title="hand-built",
                              updated_at="2026-06-01T10:00:00"))
    assert idx.get("s").title == "hand-built"


# --- full-transcript snapshots --------------------------------------------


def test_snapshot_round_trip(tmp_path):
    idx = _index(tmp_path)
    idx.record_turn("sid1", "/proj", _msgs(), now_iso="2026-06-01T10:00:00")
    loaded = idx.load_snapshot("sid1")
    assert loaded is not None
    # Full transcript preserved verbatim (system + user + assistant + tool).
    assert loaded == _msgs()


def test_snapshot_redacts_image_data(tmp_path):
    idx = _index(tmp_path)
    msgs = [{"role": "user", "content": [
        {"type": "text", "text": "describe this"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
    ]}]
    idx.record_turn("sid1", "/p", msgs, now_iso="2026-06-01T10:00:00")
    loaded = idx.load_snapshot("sid1")
    # Text kept; the base64 image swapped for a placeholder.
    blocks = loaded[0]["content"]
    assert {"type": "text", "text": "describe this"} in blocks
    assert any(b.get("text") == "[image omitted from snapshot]" for b in blocks)
    assert all("data:" not in str(b) for b in blocks)


def test_snapshot_rewritten_in_full_each_turn(tmp_path):
    idx = _index(tmp_path)
    idx.record_turn("sid1", "/p", _msgs(), now_iso="2026-06-01T10:00:00")
    grown = _msgs() + [{"role": "user", "content": "and one more thing"}]
    idx.record_turn("sid1", "/p", grown, now_iso="2026-06-01T11:00:00")
    loaded = idx.load_snapshot("sid1")
    assert loaded == grown                          # latest write holds it all


def test_load_snapshot_missing_returns_none(tmp_path):
    idx = _index(tmp_path)
    assert idx.load_snapshot("nope") is None
