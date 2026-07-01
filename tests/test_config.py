"""Tests for config resolution and redaction."""

from __future__ import annotations

import textwrap
from pathlib import Path

import pytest

import nanocodex.config as config_mod
from nanocodex.config import (
    Config,
    ConfigError,
    dump_nanocodex_toml,
    load_config,
    write_nanocodex_config,
)


@pytest.fixture(autouse=True)
def _isolate_nanocodex_config(tmp_path, monkeypatch):
    """Keep load_config hermetic: point NANOCODEX_CONFIG at a non-existent path.

    The load chain now reads ~/.nanocodex/config.toml. Without this, file-based
    tests would pick up a developer's real settings file. Tests that exercise
    the nanocodex file on purpose re-point NANOCODEX_CONFIG themselves.
    """
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", tmp_path / "no-nano.toml")


def test_config_redacts_api_key():
    cfg = Config(api_key="sk-abcdef123456", base_url="u", model="m")
    red = cfg.redacted()
    assert red["api_key"] == "****3456"
    assert "abcdef" not in str(red)


def test_validate_rejects_bad_sandbox_mode():
    cfg = Config(api_key="k", base_url="u", model="m", sandbox_mode="banana")
    try:
        cfg.validate()
        assert False, "expected ConfigError"
    except ConfigError as exc:
        assert "sandbox_mode" in str(exc)


def test_validate_rejects_missing_key():
    cfg = Config(api_key="", base_url="u", model="m")
    try:
        cfg.validate()
        assert False, "expected ConfigError"
    except ConfigError as exc:
        assert "API key" in str(exc)


def test_compaction_defaults_on_with_1m_window():
    # Auto-compaction ships ON: a positive default budget at ~50% of the 1M
    # window. This is the "Codex-style, just works" behavior — a regression to
    # budget=0 (off) or the old 64K window estimate would silently let long
    # conversations balloon unbounded, so lock both defaults here.
    cfg = Config(api_key="k", base_url="u", model="m")
    assert cfg.context_token_budget > 0          # compaction enabled by default
    assert cfg.context_token_budget == 512_000   # ~50% of the 1M window
    assert cfg.context_window == 1_048_576        # deepseek-v4-pro official 1M


def test_load_config_reads_deepseek_file(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text(textwrap.dedent("""
        api_key = "sk-fromfile"
        base_url = "https://api.deepseek.com/beta"
        default_text_model = "deepseek-v4-pro"
        sandbox_mode = "workspace-write"
        approval_policy = "on-request"
    """))
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    for var in ("DEEPSEEK_API_KEY", "NANOCODEX_API_KEY", "DEEPSEEK_BASE_URL",
                "NANOCODEX_BASE_URL", "NANOCODEX_MODEL", "NANOCODEX_SANDBOX",
                "NANOCODEX_APPROVAL"):
        monkeypatch.delenv(var, raising=False)

    cfg = load_config(workspace=tmp_path)
    cfg.validate()
    assert cfg.api_key == "sk-fromfile"
    assert cfg.base_url == "https://api.deepseek.com/beta"
    assert cfg.model == "deepseek-v4-pro"


def test_overrides_win_over_file(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text('api_key = "k"\ndefault_text_model = "deepseek-v4-pro"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    for var in ("NANOCODEX_MODEL", "NANOCODEX_SANDBOX"):
        monkeypatch.delenv(var, raising=False)

    cfg = load_config(workspace=tmp_path, overrides={"model": "deepseek-chat", "sandbox_mode": "read-only"})
    assert cfg.model == "deepseek-chat"
    assert cfg.sandbox_mode == "read-only"


def test_deepseek_nested_provider_key(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text(textwrap.dedent("""
        base_url = "u"
        [providers.deepseek]
        api_key = "sk-nested"
    """))
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    for var in ("DEEPSEEK_API_KEY", "NANOCODEX_API_KEY"):
        monkeypatch.delenv(var, raising=False)
    cfg = load_config(workspace=tmp_path)
    assert cfg.api_key == "sk-nested"


def test_max_iterations_default_and_override(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text('api_key = "k"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    monkeypatch.delenv("NANOCODEX_MAX_ITERATIONS", raising=False)

    # Default is 60.
    assert load_config(workspace=tmp_path).max_iterations == 60
    # CLI override wins.
    cfg = load_config(workspace=tmp_path, overrides={"max_iterations": 100})
    assert cfg.max_iterations == 100


def test_max_iterations_from_env(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text('api_key = "k"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    monkeypatch.setenv("NANOCODEX_MAX_ITERATIONS", "80")
    assert load_config(workspace=tmp_path).max_iterations == 80


# --- nanocodex's own config file (GUI Settings) -----------------------------


def test_dump_nanocodex_toml_round_trips():
    import tomllib

    text = dump_nanocodex_toml({
        "api_key": 'sk-with"quote',
        "base_url": "https://api.deepseek.com/beta",
        "model": "deepseek-v4-pro",
    })
    parsed = tomllib.loads(text)
    assert parsed["api_key"] == 'sk-with"quote'      # escaping survives
    assert parsed["base_url"] == "https://api.deepseek.com/beta"
    assert parsed["model"] == "deepseek-v4-pro"


def test_dump_nanocodex_toml_skips_empty_and_unknown():
    import tomllib

    text = dump_nanocodex_toml({"api_key": "", "model": "m", "bogus": "x"})
    parsed = tomllib.loads(text)
    assert "api_key" not in parsed   # empty value not written
    assert "bogus" not in parsed     # unknown key ignored
    assert parsed["model"] == "m"


def test_write_nanocodex_config_creates_and_merges(tmp_path):
    target = tmp_path / "nano" / "config.toml"
    # First write: just the API key.
    write_nanocodex_config({"api_key": "sk-1"}, path=target)
    assert target.is_file()
    # Second write: a different field. The API key must be preserved (merge).
    write_nanocodex_config({"model": "deepseek-chat"}, path=target)

    import tomllib
    parsed = tomllib.loads(target.read_text(encoding="utf-8"))
    assert parsed["api_key"] == "sk-1"
    assert parsed["model"] == "deepseek-chat"


def test_write_nanocodex_config_ignores_unknown_keys(tmp_path):
    target = tmp_path / "config.toml"
    write_nanocodex_config({"api_key": "sk-1", "bogus": "nope"}, path=target)
    import tomllib
    parsed = tomllib.loads(target.read_text(encoding="utf-8"))
    assert "bogus" not in parsed


def test_nanocodex_file_wins_over_deepseek(tmp_path, monkeypatch):
    ds = tmp_path / "deepseek.toml"
    ds.write_text('api_key = "sk-ds"\ndefault_text_model = "deepseek-v4-pro"\n')
    nano = tmp_path / "nano.toml"
    nano.write_text('api_key = "sk-nano"\nmodel = "deepseek-chat"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", ds)
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope.toml")
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", nano)
    for var in ("DEEPSEEK_API_KEY", "NANOCODEX_API_KEY", "NANOCODEX_MODEL"):
        monkeypatch.delenv(var, raising=False)

    cfg = load_config(workspace=tmp_path)
    assert cfg.api_key == "sk-nano"   # nanocodex file wins over DeepSeek file
    assert cfg.model == "deepseek-chat"


def test_env_wins_over_nanocodex_file(tmp_path, monkeypatch):
    nano = tmp_path / "nano.toml"
    nano.write_text('api_key = "sk-nano"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", tmp_path / "nope-ds.toml")
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope-cx.toml")
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", nano)
    monkeypatch.setenv("DEEPSEEK_API_KEY", "sk-env")

    cfg = load_config(workspace=tmp_path)
    assert cfg.api_key == "sk-env"    # environment still overrides the file


def test_max_retries_default_and_env_override(tmp_path, monkeypatch):
    # Defaults to 3 (a notch above the SDK's built-in 2); tunable via env.
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", tmp_path / "nope-ds.toml")
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope-cx.toml")
    monkeypatch.setenv("DEEPSEEK_API_KEY", "sk-env")
    monkeypatch.delenv("NANOCODEX_MAX_RETRIES", raising=False)
    assert load_config(workspace=tmp_path).max_retries == 3

    monkeypatch.setenv("NANOCODEX_MAX_RETRIES", "5")
    assert load_config(workspace=tmp_path).max_retries == 5

    # Garbage falls back to the default rather than raising.
    monkeypatch.setenv("NANOCODEX_MAX_RETRIES", "not-a-number")
    assert load_config(workspace=tmp_path).max_retries == 3


def test_profile_overrides_base_but_below_env(tmp_path, monkeypatch):
    nano = tmp_path / "nano.toml"
    nano.write_text(
        'api_key = "sk-base"\n'
        'model = "deepseek-chat"\n'
        'reasoning_effort = "auto"\n'
        '\n'
        '[profiles.fast]\n'
        'model = "deepseek-v4-pro"\n'
        'reasoning_effort = "high"\n'
        'sandbox_mode = "read-only"\n'
    )
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", tmp_path / "nope-ds.toml")
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope-cx.toml")
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", nano)
    for var in ("DEEPSEEK_API_KEY", "NANOCODEX_API_KEY", "NANOCODEX_MODEL",
                "NANOCODEX_PROFILE", "NANOCODEX_SANDBOX"):
        monkeypatch.delenv(var, raising=False)

    # Profile applied: its values win over the file base.
    cfg = load_config(workspace=tmp_path, profile="fast")
    assert cfg.model == "deepseek-v4-pro"
    assert cfg.reasoning_effort == "high"
    assert cfg.sandbox_mode == "read-only"

    # Env still beats the profile.
    monkeypatch.setenv("NANOCODEX_MODEL", "deepseek-reasoner")
    cfg2 = load_config(workspace=tmp_path, profile="fast")
    assert cfg2.model == "deepseek-reasoner"


def test_profile_name_from_env_and_unknown_raises(tmp_path, monkeypatch):
    nano = tmp_path / "nano.toml"
    nano.write_text('api_key = "sk-base"\n[profiles.fast]\nmodel = "m-fast"\n')
    monkeypatch.setattr(config_mod, "DEEPSEEK_CONFIG", tmp_path / "nope-ds.toml")
    monkeypatch.setattr(config_mod, "CODEX_CONFIG", tmp_path / "nope-cx.toml")
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", nano)
    monkeypatch.delenv("NANOCODEX_MODEL", raising=False)

    monkeypatch.setenv("NANOCODEX_PROFILE", "fast")
    assert load_config(workspace=tmp_path).model == "m-fast"

    monkeypatch.setenv("NANOCODEX_PROFILE", "ghost")
    with pytest.raises(ConfigError, match="ghost"):
        load_config(workspace=tmp_path)


def test_list_profiles(tmp_path, monkeypatch):
    nano = tmp_path / "nano.toml"
    nano.write_text('[profiles.a]\nmodel="x"\n[profiles.b]\nmodel="y"\n')
    monkeypatch.setattr(config_mod, "NANOCODEX_CONFIG", nano)
    assert config_mod.list_profiles() == ["a", "b"]
