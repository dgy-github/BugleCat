"""Tests for adaptive reasoning-effort selection (the ``auto`` tier).

Pure-function tests, mirroring tests/test_skills.py / test_memory.py: no network,
no provider, just the string→tier rules and the resolve_effort wrapper.
"""

from __future__ import annotations

from nanocodex.agent.auto_reasoning import (
    EFFORT_HIGH,
    EFFORT_LOW,
    EFFORT_MAX,
    last_user_text,
    resolve_effort,
    select_auto_effort,
)

# --- select_auto_effort: keyword rules -----------------------------------

def test_high_effort_keyword_english_gives_max():
    assert select_auto_effort("please debug this crash") == EFFORT_MAX
    assert select_auto_effort("I keep getting an error") == EFFORT_MAX


def test_high_effort_keyword_chinese_gives_max():
    # The whole point of porting this: a Chinese user reporting a bug should
    # get Max, not the silent default.
    assert select_auto_effort("帮我调试一下这个函数") == EFFORT_MAX
    assert select_auto_effort("这里报错了怎么办") == EFFORT_MAX
    assert select_auto_effort("程序崩溃了") == EFFORT_MAX


def test_low_effort_keyword_gives_low():
    assert select_auto_effort("search the docs for this api") == EFFORT_LOW
    assert select_auto_effort("帮我查找一下用法") == EFFORT_LOW


def test_plain_request_defaults_to_high():
    assert select_auto_effort("add a logout button to the navbar") == EFFORT_HIGH
    assert select_auto_effort("把这个函数重构一下") == EFFORT_HIGH


def test_subagent_always_low():
    # Sub-agent context wins even over a high-effort keyword.
    assert select_auto_effort("debug this", is_subagent=True) == EFFORT_LOW


def test_high_beats_low_when_both_present():
    # Rule order: high-effort keywords are checked before low-effort ones.
    assert select_auto_effort("search for the error in the logs") == EFFORT_MAX


def test_empty_message_defaults_to_high():
    assert select_auto_effort("") == EFFORT_HIGH


def test_case_insensitive():
    assert select_auto_effort("DEBUG THIS") == EFFORT_MAX


# --- last_user_text -------------------------------------------------------

def test_last_user_text_plain_string():
    messages = [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "first"},
        {"role": "assistant", "content": "ok"},
        {"role": "user", "content": "second"},
    ]
    assert last_user_text(messages) == "second"


def test_last_user_text_block_list():
    messages = [
        {"role": "user", "content": [
            {"type": "text", "text": "look at this"},
            {"type": "image_url", "image_url": {"url": "data:..."}},
        ]},
    ]
    assert last_user_text(messages) == "look at this"


def test_last_user_text_none_when_no_user():
    assert last_user_text([{"role": "system", "content": "sys"}]) == ""


# --- resolve_effort: the loop entry point ---------------------------------

def test_resolve_auto_picks_concrete_tier():
    messages = [{"role": "user", "content": "debug the crash"}]
    assert resolve_effort("auto", messages) == EFFORT_MAX


def test_resolve_explicit_tier_passes_through():
    # A user who pinned "high" keeps it, even if the message says "search".
    messages = [{"role": "user", "content": "search the docs"}]
    assert resolve_effort("high", messages) == "high"
    assert resolve_effort("off", messages) == "off"


def test_resolve_none_passes_through():
    assert resolve_effort(None, [{"role": "user", "content": "x"}]) is None


def test_resolve_auto_case_insensitive():
    messages = [{"role": "user", "content": "just add a feature"}]
    assert resolve_effort("AUTO", messages) == EFFORT_HIGH


def test_resolve_auto_subagent():
    messages = [{"role": "user", "content": "debug this"}]
    assert resolve_effort("auto", messages, is_subagent=True) == EFFORT_LOW
