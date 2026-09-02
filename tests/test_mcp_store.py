"""Tests for the MCP plugin store: pure TOML serializer + CRUD over mcp.toml.

The serializer's contract is round-trip safety: dump_mcp_toml(servers) must
produce text that tomllib can read back into the same server definitions. We
assert on the parsed result, never on fragile string shapes.
"""

from __future__ import annotations

import tomllib

import pytest

from nanocodex.tools.mcp import McpServerConfig, parse_mcp_servers
from nanocodex.tools.mcp_store import (
    McpStore,
    dump_mcp_toml,
    is_valid_server_name,
)

# --- name validation ------------------------------------------------------


def test_valid_server_names():
    assert is_valid_server_name("windows_computer_use")
    assert is_valid_server_name("playwright")
    assert is_valid_server_name("srv-1.2_x")


def test_invalid_server_names():
    assert not is_valid_server_name("")
    assert not is_valid_server_name(".")
    assert not is_valid_server_name("..")
    assert not is_valid_server_name("bad name")      # space
    assert not is_valid_server_name("a/b")           # separator
    assert not is_valid_server_name("a]b")           # TOML structure char
    assert not is_valid_server_name('a"b')           # quote


# --- pure serializer round-trip ------------------------------------------


def _roundtrip(servers: list[McpServerConfig]) -> list[McpServerConfig]:
    text = dump_mcp_toml(servers)
    data = tomllib.loads(text)
    return parse_mcp_servers(data)


def test_dump_roundtrips_basic_server():
    s = McpServerConfig(name="srv", command="python", args=["server.py"])
    out = _roundtrip([s])
    assert len(out) == 1
    assert out[0].name == "srv"
    assert out[0].command == "python"
    assert out[0].args == ["server.py"]
    assert out[0].enabled is True
    assert out[0].env == {}


def test_dump_roundtrips_env_and_disabled():
    s = McpServerConfig(
        name="win", command="py.exe", args=["a", "b"],
        env={"ALLOW": "Code.exe,WeChat.exe"}, enabled=False,
    )
    out = _roundtrip([s])
    assert out[0].enabled is False
    assert out[0].env == {"ALLOW": "Code.exe,WeChat.exe"}
    assert out[0].args == ["a", "b"]


def test_dump_roundtrips_multiple_servers():
    servers = [
        McpServerConfig(name="a", command="cmd-a"),
        McpServerConfig(name="b", command="cmd-b", enabled=False),
    ]
    out = _roundtrip(servers)
    names = {s.name: s for s in out}
    assert names["a"].enabled is True
    assert names["b"].enabled is False


def test_dump_escapes_special_chars_in_paths():
    # Windows paths have backslashes — they must survive the round-trip.
    s = McpServerConfig(
        name="win", command=r"C:\Python\python.exe",
        args=[r"D:\proj\server.py"],
    )
    out = _roundtrip([s])
    assert out[0].command == r"C:\Python\python.exe"
    assert out[0].args == [r"D:\proj\server.py"]


def test_dump_empty_list_is_parseable():
    text = dump_mcp_toml([])
    data = tomllib.loads(text)          # must not raise
    assert parse_mcp_servers(data) == []


def test_enabled_only_written_when_false():
    # An enabled server should not carry an explicit `enabled` key in its TABLE
    # (stays visually identical to a hand-written config); a disabled one must.
    # Check the parsed table, not the raw text — the header comment legitimately
    # mentions the word "enabled", so a substring check would be a false signal.
    import tomllib

    on = tomllib.loads(dump_mcp_toml([McpServerConfig(name="x", command="c")]))
    off = tomllib.loads(
        dump_mcp_toml([McpServerConfig(name="x", command="c", enabled=False)])
    )
    assert "enabled" not in on["mcp_servers"]["x"]
    assert off["mcp_servers"]["x"]["enabled"] is False


# --- store CRUD over a temp file -----------------------------------------


def _store(tmp_path) -> McpStore:
    return McpStore(tmp_path / "mcp.toml")


def test_add_and_list(tmp_path):
    store = _store(tmp_path)
    store.add("srv", "python", ["server.py"], {"K": "V"})
    servers = store.list()
    assert len(servers) == 1
    assert servers[0].name == "srv"
    assert servers[0].command == "python"
    assert servers[0].env == {"K": "V"}
    assert servers[0].enabled is True


def test_add_rejects_invalid_name(tmp_path):
    store = _store(tmp_path)
    with pytest.raises(ValueError):
        store.add("bad name", "python")


def test_add_rejects_empty_command(tmp_path):
    store = _store(tmp_path)
    with pytest.raises(ValueError):
        store.add("srv", "")


def test_add_rejects_duplicate_without_overwrite(tmp_path):
    store = _store(tmp_path)
    store.add("srv", "python")
    with pytest.raises(ValueError):
        store.add("srv", "node")


def test_add_overwrite_replaces(tmp_path):
    store = _store(tmp_path)
    store.add("srv", "python")
    store.add("srv", "node", overwrite=True)
    servers = store.list()
    assert len(servers) == 1
    assert servers[0].command == "node"


def test_remove(tmp_path):
    store = _store(tmp_path)
    store.add("a", "cmd-a")
    store.add("b", "cmd-b")
    assert store.remove("a") is True
    assert [s.name for s in store.list()] == ["b"]
    assert store.remove("missing") is False


def test_set_enabled(tmp_path):
    store = _store(tmp_path)
    store.add("srv", "python")
    assert store.set_enabled("srv", False) is True
    assert store.get("srv").enabled is False
    assert store.set_enabled("srv", True) is True
    assert store.get("srv").enabled is True
    assert store.set_enabled("missing", False) is False


def test_disabled_server_persists_across_reload(tmp_path):
    # A disabled server must remain in the file (not be dropped), so it can be
    # re-enabled later without re-entering its config.
    path = tmp_path / "mcp.toml"
    McpStore(path).add("srv", "python")
    McpStore(path).set_enabled("srv", False)
    reloaded = McpStore(path).get("srv")
    assert reloaded is not None
    assert reloaded.enabled is False


def test_add_preserves_existing_servers(tmp_path):
    path = tmp_path / "mcp.toml"
    McpStore(path).add("a", "cmd-a", env={"X": "1"})
    McpStore(path).add("b", "cmd-b")
    servers = {s.name: s for s in McpStore(path).list()}
    assert set(servers) == {"a", "b"}
    assert servers["a"].env == {"X": "1"}  # not clobbered by the second add
