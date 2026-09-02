"""Tests for user memory: pure parsing/rendering, the store, and prompt injection.

Mirrors test_skills.py — everything is offline (a tmp_path memory file), no
network, no clocks (timestamps are injected).
"""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest

from nanocodex.agent import memory_store as memory_store
from nanocodex.agent.memory_store import MemoryStore, format_bullet

# --- pure: load -----------------------------------------------------------

def test_load_returns_none_for_missing_file(tmp_path):
    assert memory_store.load(tmp_path / "nope.md") is None


def test_load_returns_none_for_blank_file(tmp_path):
    p = tmp_path / "memory.md"
    p.write_text("   \n\t\n", encoding="utf-8")
    assert memory_store.load(p) is None


def test_load_returns_content(tmp_path):
    p = tmp_path / "memory.md"
    p.write_text("- a fact", encoding="utf-8")
    assert memory_store.load(p) == "- a fact"


# --- pure: as_system_block ------------------------------------------------

def test_as_system_block_wraps_in_user_memory_tag():
    block = memory_store.as_system_block("- likes Chinese replies", source=Path("/x/memory.md"))
    assert block.startswith("<user_memory ")
    assert 'source="' in block
    assert "- likes Chinese replies" in block
    assert block.rstrip().endswith("</user_memory>")


def test_as_system_block_empty_content_returns_empty_string():
    assert memory_store.as_system_block("   ") == ""


def test_as_system_block_truncates_oversized_content():
    big = "x" * (memory_store._MAX_MEMORY_CHARS + 5000)
    block = memory_store.as_system_block(big)
    assert "truncated" in block
    # The body is capped near the limit (plus tags/marker overhead).
    assert len(block) < memory_store._MAX_MEMORY_CHARS + 1000


# --- pure: render_for_prompt ----------------------------------------------

def test_render_for_prompt_empty_when_no_file(tmp_path):
    assert memory_store.render_for_prompt(tmp_path / "nope.md") == ""


def test_render_for_prompt_wraps_existing(tmp_path):
    p = tmp_path / "memory.md"
    p.write_text("- a durable fact", encoding="utf-8")
    out = memory_store.render_for_prompt(p)
    assert "<user_memory" in out
    assert "- a durable fact" in out


# --- pure: format_bullet --------------------------------------------------

def test_format_bullet_is_timestamped_and_single_line():
    b = format_bullet("hello\nworld", now="2026-06-06 10:30")
    assert b == "- [2026-06-06 10:30] hello world"


# --- store: append --------------------------------------------------------

def test_append_creates_file_with_heading(tmp_path):
    store = MemoryStore(tmp_path / "memory.md")
    bullet = store.append("partner is called X", now="2026-06-06 10:30")
    assert bullet == "- [2026-06-06 10:30] partner is called X"
    text = (tmp_path / "memory.md").read_text(encoding="utf-8")
    assert text.startswith("# nanocodex user memory")
    assert "- [2026-06-06 10:30] partner is called X" in text


def test_append_adds_second_bullet(tmp_path):
    store = MemoryStore(tmp_path / "memory.md")
    store.append("first", now="2026-06-06 10:30")
    store.append("second", now="2026-06-06 10:31")
    text = (tmp_path / "memory.md").read_text(encoding="utf-8")
    assert "- [2026-06-06 10:30] first" in text
    assert "- [2026-06-06 10:31] second" in text
    # Heading written exactly once.
    assert text.count("# nanocodex user memory") == 1


def test_append_rejects_empty_note(tmp_path):
    store = MemoryStore(tmp_path / "memory.md")
    with pytest.raises(ValueError):
        store.append("   ")


def test_store_render_for_prompt_roundtrip(tmp_path):
    store = MemoryStore(tmp_path / "memory.md")
    store.append("reply in Chinese", now="2026-06-06 10:30")
    out = store.render_for_prompt()
    assert "<user_memory" in out
    assert "reply in Chinese" in out


# --- prompt injection -----------------------------------------------------

def _policy(tmp_path):
    from nanocodex.sandbox.policy import SandboxPolicy
    return SandboxPolicy("workspace-write", workspace=tmp_path)


def test_build_system_prompt_includes_memory_block(tmp_path):
    from nanocodex.agent.prompt import build_system_prompt

    memory = memory_store.as_system_block("- partner is X", source=tmp_path / "memory.md")
    prompt = build_system_prompt(_policy(tmp_path), "on-request", memory=memory)
    assert "# User memory" in prompt
    assert "- partner is X" in prompt
    assert "<user_memory" in prompt


def test_build_system_prompt_omits_memory_section_when_empty(tmp_path):
    from nanocodex.agent.prompt import build_system_prompt

    prompt = build_system_prompt(_policy(tmp_path), "on-request", memory="")
    assert "# User memory" not in prompt


# --- remember tool --------------------------------------------------------

def test_remember_tool_appends_and_reports(tmp_path, monkeypatch):
    # Point the default memory path at a tmp file so the tool writes there.
    monkeypatch.setattr(memory_store, "DEFAULT_MEMORY_PATH", tmp_path / "memory.md")
    from nanocodex.tools.remember_tool import RememberTool

    tool = RememberTool(ctx=None)
    out = asyncio.run(tool.execute(note="user prefers pytest"))
    assert "Remembered:" in out
    assert "user prefers pytest" in (tmp_path / "memory.md").read_text(encoding="utf-8")


def test_remember_tool_rejects_empty_note(tmp_path, monkeypatch):
    monkeypatch.setattr(memory_store, "DEFAULT_MEMORY_PATH", tmp_path / "memory.md")
    from nanocodex.tools.remember_tool import RememberTool

    tool = RememberTool(ctx=None)
    out = asyncio.run(tool.execute(note="   "))
    assert out.startswith("Error")
