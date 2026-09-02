"""Tests for the MCP marketplace: built-in catalog, remote parsing, install.

All pure / injected I/O — no network, no real ~/.nanocodex/mcp.toml. The remote
fetch path uses an injectable opener returning fixed bytes; the install path
uses an McpStore pointed at a tmp_path file (same isolation as test_mcp_store).
"""

from __future__ import annotations

import json

import pytest

from nanocodex.tools.marketplace import (
    BUILTIN_CATALOG,
    MARKETPLACE_URL_ENV,
    CatalogEntry,
    fetch_remote_catalog,
    install_entry,
    marketplace_url,
    parse_remote_catalog,
)
from nanocodex.tools.mcp_store import McpStore, is_valid_server_name

# --- built-in catalog -----------------------------------------------------


def test_builtin_catalog_nonempty_and_valid():
    assert BUILTIN_CATALOG, "built-in catalog must not be empty"
    for entry in BUILTIN_CATALOG:
        # Every shipped entry must be installable: valid bare-key name + command.
        assert is_valid_server_name(entry.name), entry.name
        assert entry.command, entry.name
        assert entry.source in ("builtin", "official")


def test_builtin_includes_project_local_server():
    names = {e.name for e in BUILTIN_CATALOG}
    assert "windows_computer_use" in names


def test_project_local_entry_needs_path():
    entry = next(e for e in BUILTIN_CATALOG if e.name == "windows_computer_use")
    # The project-local server's path is machine-specific (no cwd field on
    # McpServerConfig), so it must be marked needs-path.
    assert entry.path_arg_index is not None
    assert entry.path_label


# --- marketplace_url -------------------------------------------------------


def test_marketplace_url_unset(monkeypatch):
    monkeypatch.delenv(MARKETPLACE_URL_ENV, raising=False)
    assert marketplace_url() is None


def test_marketplace_url_blank_is_none(monkeypatch):
    monkeypatch.setenv(MARKETPLACE_URL_ENV, "   ")
    assert marketplace_url() is None


def test_marketplace_url_set(monkeypatch):
    monkeypatch.setenv(MARKETPLACE_URL_ENV, "https://example.com/catalog.json")
    assert marketplace_url() == "https://example.com/catalog.json"


# --- parse_remote_catalog --------------------------------------------------


def test_parse_remote_top_level_list():
    raw = json.dumps([
        {"name": "srv_a", "command": "npx", "args": ["-y", "pkg"]},
        {"name": "srv_b", "command": "uvx", "args": ["mcp-x"],
         "description": "x", "env_keys": ["TOKEN"]},
    ])
    out = parse_remote_catalog(raw)
    assert [e.name for e in out] == ["srv_a", "srv_b"]
    assert out[0].source == "remote"
    assert out[1].env_keys == ["TOKEN"]


def test_parse_remote_entries_key():
    raw = json.dumps({"entries": [{"name": "srv", "command": "python"}]})
    assert [e.name for e in parse_remote_catalog(raw)] == ["srv"]


def test_parse_remote_servers_key():
    raw = json.dumps({"servers": [{"name": "srv", "command": "python"}]})
    assert [e.name for e in parse_remote_catalog(raw)] == ["srv"]


def test_parse_remote_drops_bad_name():
    raw = json.dumps([
        {"name": "good", "command": "python"},
        {"name": "bad name", "command": "python"},   # space -> invalid
        {"name": "../evil", "command": "python"},     # traversal -> invalid
    ])
    assert [e.name for e in parse_remote_catalog(raw)] == ["good"]


def test_parse_remote_drops_missing_command():
    raw = json.dumps([
        {"name": "ok", "command": "python"},
        {"name": "nocmd"},                 # no command -> dropped
        {"name": "blankcmd", "command": "  "},  # blank -> dropped
    ])
    assert [e.name for e in parse_remote_catalog(raw)] == ["ok"]


def test_parse_remote_dedups_by_name():
    raw = json.dumps([
        {"name": "dup", "command": "a"},
        {"name": "dup", "command": "b"},   # second one dropped
    ])
    out = parse_remote_catalog(raw)
    assert len(out) == 1
    assert out[0].command == "a"


def test_parse_remote_malformed_json_is_empty():
    assert parse_remote_catalog("not json {{{") == []
    assert parse_remote_catalog(b"\xff\xfe not utf8") == []


def test_parse_remote_non_list_payload_is_empty():
    assert parse_remote_catalog(json.dumps({"foo": "bar"})) == []
    assert parse_remote_catalog(json.dumps(42)) == []


def test_parse_remote_caps_entries():
    big = [{"name": f"s{i}", "command": "c"} for i in range(500)]
    out = parse_remote_catalog(json.dumps(big))
    assert len(out) == 200  # _MAX_REMOTE_ENTRIES


def test_parse_remote_ignores_unknown_fields():
    raw = json.dumps([{"name": "s", "command": "c", "rating": 5, "junk": [1]}])
    out = parse_remote_catalog(raw)
    assert len(out) == 1 and out[0].name == "s"


# --- fetch_remote_catalog (injected opener, no network) --------------------


def test_fetch_uses_injected_opener():
    payload = json.dumps([{"name": "fetched", "command": "python"}]).encode()
    calls: list[tuple[str, float]] = []

    def fake_opener(url: str, timeout: float) -> bytes:
        calls.append((url, timeout))
        return payload

    out = fetch_remote_catalog("https://x/catalog.json", opener=fake_opener)
    assert [e.name for e in out] == ["fetched"]
    assert calls == [("https://x/catalog.json", 10.0)]


def test_fetch_propagates_opener_error():
    def boom(url: str, timeout: float) -> bytes:
        raise OSError("connection refused")

    with pytest.raises(OSError):
        fetch_remote_catalog("https://x", opener=boom)


# --- install_entry (tmp_path store, no real mcp.toml) ----------------------


def test_install_simple_entry_round_trips(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = CatalogEntry(name="fetch", command="uvx", args=["mcp-server-fetch"],
                         source="official")
    cfg = install_entry(entry, store=store)
    assert cfg.name == "fetch"
    # Reload from disk: the manual MCP section sees the same file.
    listed = McpStore(path=tmp_path / "mcp.toml").list()
    assert any(s.name == "fetch" and s.command == "uvx" for s in listed)


def test_install_keeps_only_declared_env_keys(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = CatalogEntry(name="srv", command="npx", args=["pkg"],
                         env_keys=["TOKEN"], source="remote")
    cfg = install_entry(entry, {"TOKEN": "secret", "EXTRA": "ignored"}, store=store)
    assert cfg.env == {"TOKEN": "secret"}
    assert "EXTRA" not in cfg.env


def test_install_path_entry_fills_arg_slot(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = CatalogEntry(
        name="local", command="python", args=["<path>"],
        source="builtin", path_arg_index=0, path_label="path to server.py",
    )
    cfg = install_entry(entry, path_value="C:/srv/server.py", store=store)
    assert cfg.args == ["C:/srv/server.py"]


def test_install_path_entry_without_path_raises(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = next(e for e in BUILTIN_CATALOG if e.name == "windows_computer_use")
    with pytest.raises(ValueError):
        install_entry(entry, store=store)


def test_install_official_path_entry_fills_nonzero_slot(tmp_path):
    # The official filesystem server takes a directory at args index 2 (after
    # `-y` and the package name) — not index 0 like the project-local server.
    # The path must land in that exact slot without disturbing the npx flags.
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = next(e for e in BUILTIN_CATALOG if e.name == "filesystem")
    assert entry.path_arg_index == 2  # guard: the catalog still shapes it this way
    cfg = install_entry(entry, path_value="D:/projects/work", store=store)
    assert cfg.args == ["-y", "@modelcontextprotocol/server-filesystem",
                        "D:/projects/work"]
    # Reload from disk: the placeholder is gone, the real dir is persisted.
    listed = McpStore(path=tmp_path / "mcp.toml").list()
    fs = next(s for s in listed if s.name == "filesystem")
    assert fs.args[2] == "D:/projects/work"
    assert "<allowed-dir>" not in fs.args


def test_install_official_path_entry_without_path_raises(tmp_path):
    # Same guardrail as the project-local server: an official entry marked with
    # path_arg_index must refuse to install with a blank path rather than
    # writing the literal "<allowed-dir>" placeholder into mcp.toml.
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = next(e for e in BUILTIN_CATALOG if e.name == "filesystem")
    with pytest.raises(ValueError):
        install_entry(entry, path_value="   ", store=store)


def test_install_rejects_duplicate(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    entry = CatalogEntry(name="dup", command="python", source="official")
    install_entry(entry, store=store)
    with pytest.raises(ValueError):
        install_entry(entry, store=store)  # already exists, no overwrite


def test_install_overwrite_allows_replace(tmp_path):
    store = McpStore(path=tmp_path / "mcp.toml")
    e1 = CatalogEntry(name="dup", command="python", source="official")
    e2 = CatalogEntry(name="dup", command="node", source="official")
    install_entry(e1, store=store)
    cfg = install_entry(e2, store=store, overwrite=True)
    assert cfg.command == "node"
