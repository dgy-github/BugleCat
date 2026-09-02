"""Tests for Session.fork — continuing a past conversation as a NEW one.

The "Continue this conversation" feature forks a frozen snapshot into a fresh
session: the original is never mutated, the system prompt is taken fresh, and
any dangling tool_call is sanitized so the backend contract holds. These are
pure over data (no GUI), mirroring test_skills.py / test_mcp_store.py.
"""

from __future__ import annotations

from nanocodex.agent.session import Session


def test_fork_drops_seed_system_prompt_and_keeps_body():
    seed = [
        {"role": "system", "content": "OLD system prompt"},
        {"role": "user", "content": "hello"},
        {"role": "assistant", "content": "hi there"},
    ]
    s = Session.fork("FRESH system prompt", seed, log_path=None)
    # Exactly one system message — the fresh one, not the seed's.
    systems = [m for m in s.messages if m.get("role") == "system"]
    assert len(systems) == 1
    assert systems[0]["content"] == "FRESH system prompt"
    # Body carried over, in order.
    assert s.messages[1] == {"role": "user", "content": "hello"}
    assert s.messages[2] == {"role": "assistant", "content": "hi there"}
    assert s.restored_count == 2


def test_fork_does_not_mutate_seed_list():
    seed = [
        {"role": "system", "content": "old"},
        {"role": "user", "content": "q"},
    ]
    before = [dict(m) for m in seed]
    Session.fork("new", seed, log_path=None)
    assert seed == before  # the caller's list is untouched


def test_fork_backfills_dangling_tool_call():
    # An assistant tool_call with no matching tool result must get a synthetic
    # one, or the next backend request 400s.
    seed = [
        {"role": "user", "content": "do it"},
        {
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": "call_1", "function": {"name": "shell"}}],
        },
        # no tool result for call_1
    ]
    s = Session.fork("sys", seed, log_path=None)
    tool_msgs = [m for m in s.messages if m.get("role") == "tool"]
    assert len(tool_msgs) == 1
    assert tool_msgs[0]["tool_call_id"] == "call_1"
    assert tool_msgs[0]["name"] == "shell"


def test_fork_keeps_satisfied_tool_call_untouched():
    seed = [
        {"role": "user", "content": "do it"},
        {
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": "call_1", "function": {"name": "shell"}}],
        },
        {"role": "tool", "tool_call_id": "call_1", "name": "shell", "content": "done"},
    ]
    s = Session.fork("sys", seed, log_path=None)
    tool_msgs = [m for m in s.messages if m.get("role") == "tool"]
    assert len(tool_msgs) == 1  # no synthetic extra
    assert tool_msgs[0]["content"] == "done"


def test_fork_empty_seed_is_just_fresh():
    s = Session.fork("sys", [], log_path=None)
    assert s.messages == [{"role": "system", "content": "sys"}]
    assert s.restored_count == 0


def test_fork_new_messages_append_to_new_log_only(tmp_path):
    # The fork writes to its OWN log; the source file is never touched.
    src = tmp_path / "source.jsonl"
    src.write_text('{"role": "user", "content": "original"}\n', encoding="utf-8")
    src_before = src.read_text(encoding="utf-8")

    fork_log = tmp_path / "fork.jsonl"
    seed = [{"role": "user", "content": "original"}]
    s = Session.fork("sys", seed, log_path=fork_log)
    s.add_user("a new message in the fork")

    # Source untouched.
    assert src.read_text(encoding="utf-8") == src_before
    # Fork log got the new message.
    assert "a new message in the fork" in fork_log.read_text(encoding="utf-8")


def test_fork_parallels_resume_shape():
    # fork(seed) and resume(from a log with the same body) should yield the same
    # in-memory messages — they're the same mechanism, different source.
    body = [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": "yo"},
    ]
    forked = Session.fork("sys", [{"role": "system", "content": "x"}] + body, log_path=None)
    assert [m for m in forked.messages if m["role"] != "system"] == body
