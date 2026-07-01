"""Slash-command parsing for the REPL."""

from __future__ import annotations

from nanocodex.agent.slash import (
    DEFAULT_LOOP_INTERVAL_S,
    SLASH_HELP,
    parse_duration,
    parse_slash,
    split_loop_arg,
)


def test_plain_text_is_not_a_command():
    assert parse_slash("fix the bug in foo.py") == (None, "")
    assert parse_slash("") == (None, "")


def test_bare_command():
    assert parse_slash("/status") == ("/status", "")
    assert parse_slash("  /help  ") == ("/help", "")


def test_command_with_argument():
    assert parse_slash("/model deepseek-chat") == ("/model", "deepseek-chat")
    assert parse_slash("/approvals never") == ("/approvals", "never")


def test_quit_normalizes_to_exit():
    assert parse_slash("/quit") == ("/exit", "")


def test_case_insensitive_command():
    assert parse_slash("/MODEL Foo") == ("/model", "Foo")


def test_help_table_covers_core_commands():
    for c in ("/help", "/model", "/approvals", "/diff", "/loop", "/compact",
              "/clear", "/exit"):
        assert c in SLASH_HELP


def test_parse_duration_units():
    assert parse_duration("30s") == 30
    assert parse_duration("5m") == 300
    assert parse_duration("1h") == 3600
    assert parse_duration("90") == 90          # bare number = seconds
    assert parse_duration("1.5m") == 90        # fractional


def test_parse_duration_rejects_non_durations():
    for bad in ("", "abc", "run", "5x", "0", "-3", "5 m extra"):
        assert parse_duration(bad) is None, bad


def test_split_loop_arg_with_leading_interval():
    assert split_loop_arg("5m run the tests") == (300, "run the tests")
    assert split_loop_arg("30s /diff") == (30, "/diff")


def test_split_loop_arg_without_interval_uses_default():
    assert split_loop_arg("run the tests") == (DEFAULT_LOOP_INTERVAL_S, "run the tests")
    # A non-duration leading word stays part of the prompt.
    assert split_loop_arg("check status now") == (DEFAULT_LOOP_INTERVAL_S, "check status now")


def test_split_loop_arg_empty():
    assert split_loop_arg("") == (DEFAULT_LOOP_INTERVAL_S, "")
