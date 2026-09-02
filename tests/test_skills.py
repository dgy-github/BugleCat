"""Tests for the skills system: pure parser/store + prompt injection + tool.

All offline and filesystem-scoped to a tmp dir (no real ~/.nanocodex touched),
mirroring the test style of test_schedule.py / test_gui_status.py.
"""

from __future__ import annotations

import asyncio

import pytest

from nanocodex.agent.skills_store import (
    BUILTIN_SKILLS_DIR,
    DEFAULT_SKILLS_DIR,
    SkillsStore,
    discover_skills,
    is_valid_skill_name,
    parse_skill,
)

# --- parse_skill (pure) ---------------------------------------------------

def test_parse_skill_reads_frontmatter_and_body():
    text = (
        "---\n"
        "name: wechat-reply\n"
        "description: Reply on WeChat in my voice.\n"
        "---\n\n"
        "# How to\n1. step one\n"
    )
    skill = parse_skill(text)
    assert skill is not None
    assert skill.name == "wechat-reply"
    assert skill.description == "Reply on WeChat in my voice."
    assert "step one" in skill.body


def test_parse_skill_uses_fallback_name_when_frontmatter_omits_it():
    text = "---\ndescription: does a thing\n---\nbody"
    skill = parse_skill(text, fallback_name="folder-name")
    assert skill is not None
    assert skill.name == "folder-name"


def test_parse_skill_returns_none_without_description():
    # No description => unusable (nothing to show the model).
    assert parse_skill("---\nname: x\n---\nbody") is None


def test_parse_skill_returns_none_without_frontmatter():
    assert parse_skill("just a plain file, no frontmatter") is None


def test_parse_skill_trims_overlong_description():
    long_desc = "x" * 800
    text = f"---\nname: big\ndescription: {long_desc}\n---\nbody"
    skill = parse_skill(text)
    assert skill is not None
    assert len(skill.description) <= 501  # 500 + the ellipsis char
    assert skill.description.endswith("…")


# --- is_valid_skill_name --------------------------------------------------

@pytest.mark.parametrize("name", ["wechat-reply", "skill_1", "a.b", "ABC"])
def test_valid_skill_names(name):
    assert is_valid_skill_name(name)


@pytest.mark.parametrize("name", ["", ".", "..", "a/b", "a\\b", "x y", "../evil"])
def test_invalid_skill_names(name):
    assert not is_valid_skill_name(name)


# --- SkillsStore CRUD (tmp dir) -------------------------------------------

def test_install_then_discover_roundtrip(tmp_path):
    store = SkillsStore(tmp_path)
    store.install("greet", "Say hello nicely.", "# Greet\nsay hi")
    coll = discover_skills(tmp_path)
    assert [s.name for s in coll.skills] == ["greet"]
    assert coll.skills[0].description == "Say hello nicely."
    assert coll.warnings == []


def test_install_rejects_bad_name(tmp_path):
    store = SkillsStore(tmp_path)
    with pytest.raises(ValueError):
        store.install("../escape", "desc", "body")


def test_install_requires_description(tmp_path):
    store = SkillsStore(tmp_path)
    with pytest.raises(ValueError):
        store.install("ok-name", "", "body")


def test_install_refuses_duplicate_without_overwrite(tmp_path):
    store = SkillsStore(tmp_path)
    store.install("dup", "first", "b1")
    with pytest.raises(ValueError):
        store.install("dup", "second", "b2")
    # overwrite=True replaces it
    store.install("dup", "second", "b2", overwrite=True)
    got = store.get("dup")
    assert got is not None and got.description == "second"


def test_get_returns_none_for_missing(tmp_path):
    store = SkillsStore(tmp_path)
    assert store.get("nope") is None


def test_remove(tmp_path):
    store = SkillsStore(tmp_path)
    store.install("temp", "desc", "body")
    assert store.remove("temp") is True
    assert store.remove("temp") is False  # already gone
    assert discover_skills(tmp_path).is_empty


def test_discover_warns_on_malformed_skill(tmp_path):
    # A skill dir whose SKILL.md lacks a description is skipped with a warning.
    bad = tmp_path / "broken"
    bad.mkdir()
    (bad / "SKILL.md").write_text("---\nname: broken\n---\nno description", encoding="utf-8")
    coll = discover_skills(tmp_path)
    assert coll.is_empty
    assert any("broken" in w for w in coll.warnings)


def test_discover_empty_dir_is_empty(tmp_path):
    assert discover_skills(tmp_path).is_empty


def test_default_skills_dir_points_under_nanocodex():
    # Guard the well-known location the GUI/CLI rely on.
    assert DEFAULT_SKILLS_DIR.name == "skills"
    assert DEFAULT_SKILLS_DIR.parent.name == ".nanocodex"


# --- built-in skills (shipped in the package) -----------------------------


def test_builtin_skills_dir_ships_general_skills():
    # The package ships read-only general-purpose skills under builtin_skills/.
    coll = discover_skills(BUILTIN_SKILLS_DIR)
    names = {s.name for s in coll.skills}
    assert {"code-review", "debug", "write-tests"} <= names
    assert not coll.warnings


def test_default_discovery_includes_builtins_when_user_dir_empty(monkeypatch, tmp_path):
    # No-arg discovery merges the (empty) user dir with the built-ins, so a
    # fresh install still sees the shipped skills.
    import nanocodex.agent.skills_store as ss

    monkeypatch.setattr(ss, "DEFAULT_SKILLS_DIR", tmp_path)
    names = {s.name for s in discover_skills().skills}
    assert {"code-review", "debug", "write-tests"} <= names


def test_user_skill_shadows_same_named_builtin(monkeypatch, tmp_path):
    # A user skill with a built-in's name wins; the built-in body is hidden.
    import nanocodex.agent.skills_store as ss

    monkeypatch.setattr(ss, "DEFAULT_SKILLS_DIR", tmp_path)
    SkillsStore(tmp_path).install(
        "debug", "My own debug playbook.", body="user body"
    )
    coll = discover_skills()
    debug_skills = [s for s in coll.skills if s.name == "debug"]
    assert len(debug_skills) == 1  # not duplicated
    assert debug_skills[0].description == "My own debug playbook."


# --- prompt injection -----------------------------------------------------

def test_build_system_prompt_includes_installed_skills(tmp_path):
    from nanocodex.agent.prompt import build_system_prompt
    from nanocodex.sandbox.policy import SandboxPolicy

    store = SkillsStore(tmp_path)
    store.install("wechat-reply", "Reply on WeChat in my voice.", "# body")
    coll = discover_skills(tmp_path)

    policy = SandboxPolicy("workspace-write", workspace=tmp_path)
    prompt = build_system_prompt(policy, "on-request", None, coll)
    assert "# Skills" in prompt
    assert "wechat-reply" in prompt
    assert "Reply on WeChat in my voice." in prompt


def test_build_system_prompt_omits_skills_section_when_empty(tmp_path):
    from nanocodex.agent.prompt import build_system_prompt
    from nanocodex.sandbox.policy import SandboxPolicy

    coll = discover_skills(tmp_path)  # empty
    policy = SandboxPolicy("workspace-write", workspace=tmp_path)
    prompt = build_system_prompt(policy, "on-request", None, coll)
    assert "# Skills" not in prompt


# --- ManageSkillsTool (async execute, monkeypatched store dir) ------------

def _make_tool(tmp_path, monkeypatch):
    """Build a ManageSkillsTool whose SkillsStore() resolves to tmp_path."""
    import nanocodex.agent.skills_store as ss
    from nanocodex.tools.skills_tool import ManageSkillsTool

    # Point the default dir at tmp so SkillsStore() (no arg) is isolated.
    monkeypatch.setattr(ss, "DEFAULT_SKILLS_DIR", tmp_path)
    # The tool builds a SkillsStore() with no args inside execute(); that reads
    # the module-level default we just patched.
    return ManageSkillsTool(ctx=None)


def test_tool_install_list_show_remove(tmp_path, monkeypatch):
    tool = _make_tool(tmp_path, monkeypatch)

    out = asyncio.run(tool.execute(
        action="install", name="greet",
        description="Greet warmly.", body="# Greet\nsay hi",
    ))
    assert "installed" in out

    out = asyncio.run(tool.execute(action="list"))
    assert "greet" in out and "Greet warmly." in out

    out = asyncio.run(tool.execute(action="show", name="greet"))
    assert "say hi" in out and "greet" in out

    out = asyncio.run(tool.execute(action="remove", name="greet"))
    assert "removed" in out

    out = asyncio.run(tool.execute(action="list"))
    assert "No skills installed" in out


def test_tool_install_validation_errors(tmp_path, monkeypatch):
    tool = _make_tool(tmp_path, monkeypatch)
    assert "needs a 'name'" in asyncio.run(tool.execute(action="install", description="d"))
    assert "needs a 'description'" in asyncio.run(
        tool.execute(action="install", name="ok"))


def test_tool_unknown_action(tmp_path, monkeypatch):
    tool = _make_tool(tmp_path, monkeypatch)
    assert "unknown action" in asyncio.run(tool.execute(action="frobnicate"))
