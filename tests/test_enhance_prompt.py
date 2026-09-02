"""Tests for the pure prompt-enhancement helpers (no Tk, no network).

Mirrors test_auto_reasoning.py / test_memory.py: the message construction and
response cleanup are pure functions over strings, so they unit-test offline.
"""

from __future__ import annotations

from nanocodex.agent.enhance_prompt import (
    build_enhance_messages,
    clean_enhanced,
    should_enhance,
)

# --- should_enhance ---------------------------------------------------------

def test_should_enhance_accepts_normal_text():
    assert should_enhance("fix the login bug") is True


def test_should_enhance_rejects_empty_and_whitespace():
    assert should_enhance("") is False
    assert should_enhance("   \n  ") is False
    assert should_enhance(None) is False  # type: ignore[arg-type]


def test_should_enhance_rejects_overlong_input():
    # A huge paste is already explicit; rewriting it just burns tokens.
    assert should_enhance("x" * 4001) is False
    assert should_enhance("x" * 3999) is True


# --- build_enhance_messages -------------------------------------------------

def test_build_enhance_messages_shape():
    msgs = build_enhance_messages("帮我看下登录报错")
    assert len(msgs) == 2
    assert msgs[0]["role"] == "system"
    assert msgs[1]["role"] == "user"
    assert msgs[1]["content"] == "帮我看下登录报错"


def test_build_enhance_messages_strips_user_text():
    msgs = build_enhance_messages("  hello  ")
    assert msgs[1]["content"] == "hello"


def test_build_enhance_system_forbids_answering():
    # The system instruction must tell the model to rewrite, not solve.
    sys = build_enhance_messages("x")[0]["content"].lower()
    assert "do not answer" in sys or "not answer" in sys


# --- clean_enhanced ---------------------------------------------------------

def test_clean_enhanced_passthrough():
    assert clean_enhanced("a clear prompt", original="raw") == "a clear prompt"


def test_clean_enhanced_strips_code_fence():
    raw = "```\nrewritten prompt\n```"
    assert clean_enhanced(raw, original="x") == "rewritten prompt"


def test_clean_enhanced_strips_fence_with_lang_tag():
    raw = "```text\nrewritten\nmore\n```"
    assert clean_enhanced(raw, original="x") == "rewritten\nmore"


def test_clean_enhanced_strips_wrapping_quotes():
    assert clean_enhanced('"quoted prompt"', original="x") == "quoted prompt"


def test_clean_enhanced_strips_smart_quotes():
    assert clean_enhanced("“prompt”", original="x") == "prompt"


def test_clean_enhanced_keeps_internal_quotes():
    # A legitimately quoted phrase mid-text must not be mangled.
    text = 'set the flag to "true" then run'
    assert clean_enhanced(text, original="x") == text


def test_clean_enhanced_falls_back_to_original_when_blank():
    assert clean_enhanced("", original="the original") == "the original"
    assert clean_enhanced("   ", original="the original") == "the original"


def test_clean_enhanced_blank_and_blank_original_is_empty():
    assert clean_enhanced("", original="  ") == ""
