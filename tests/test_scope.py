"""Tests for Scoped Agent (ncx-scope/v1) manifest loading, narrowing, and tool filters."""

from __future__ import annotations

from pathlib import Path

import pytest

from nanocodex.agent.scope import (
    ScopeError,
    ScopeManifest,
    resolve_scope,
)
from nanocodex.sandbox.policy import READ_ONLY, WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry

EXAMPLE_SCOPE = Path(__file__).resolve().parent.parent / "templates" / "scopes" / "refund.example.toml"


def test_example_refund_scope_manifest_loads_and_matches_keywords():
    raw = EXAMPLE_SCOPE.read_text(encoding="utf-8")
    m = ScopeManifest.from_toml_str(raw)
    assert m.id == "refund"
    assert m.version == 1
    assert m.matches_accepts("请审核这张退货申请")
    assert not m.matches_accepts("帮我重构编译器")
    assert m.tools.approval[0].matches("apply_patch", {"refund_amount": 600})
    assert not m.tools.approval[0].matches("apply_patch", {"refund_amount": 100})
    assert m.tools.approval[0].matches(
        "apply_patch", {"patch": '+{"order": "A1", "refund_amount": 999}'}
    )


def test_resolve_scope_workspace_shadows_home_and_narrows_sandbox(tmp_path):
    ws = tmp_path / "ws"
    home = tmp_path / "home"
    (ws / "agents").mkdir(parents=True)
    (home / ".nanocodex" / "agents").mkdir(parents=True)

    base_text = EXAMPLE_SCOPE.read_text(encoding="utf-8")
    (home / ".nanocodex" / "agents" / "refund.toml").write_text(
        base_text, encoding="utf-8"
    )
    (ws / "agents" / "refund.toml").write_text(
        base_text.replace("version = 1", "version = 4"), encoding="utf-8"
    )

    loaded = resolve_scope(ws, "refund", home=home)
    assert loaded.manifest.version == 4

    # Global read-only cannot be widened by scope writable_roots.
    ro = SandboxPolicy(mode=READ_ONLY, workspace=ws)
    narrowed_ro = loaded.narrow_sandbox(ro)
    assert narrowed_ro.mode == READ_ONLY
    assert not narrowed_ro.can_write(ws / "out" / "refund" / "x.json")

    # Global workspace-write is narrowed strictly to out/refund.
    rw = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws, network_access=True)
    narrowed_rw = loaded.narrow_sandbox(rw)
    assert narrowed_rw.mode == WORKSPACE_WRITE
    assert not narrowed_rw.network_access
    assert narrowed_rw.can_write(ws / "out" / "refund" / "decision.json")
    assert not narrowed_rw.can_write(ws / "agents" / "refund.toml")
    assert not narrowed_rw.can_write(ws / "src" / "app.py")


def test_tool_registry_honors_allow_and_deny_filters_and_env(tmp_path, monkeypatch):
    from nanocodex.sandbox.approval import Approver
    from nanocodex.sandbox.executor import PolicyExecutor

    monkeypatch.setenv("NCX_TOOL_ALLOW", "read_file,shell,web_search")
    monkeypatch.setenv("NCX_TOOL_DENY", "web_search")
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)
    ctx = ToolContext(
        workspace=tmp_path,
        policy=policy,
        approver=Approver("on-request", None),
        executor=PolicyExecutor(policy),
    )
    reg = ToolRegistry(ctx, allow_tools={"read_file", "apply_patch"}, deny_tools=set())
    # Intersection of allow_tools & NCX_TOOL_ALLOW minus NCX_TOOL_DENY => only read_file.
    assert reg.names == ["read_file"]


def test_rejects_bad_schema_or_path_traversal_id():
    with pytest.raises(ScopeError):
        ScopeManifest.from_toml_str('schema = "bad"\nid = "a"\nname = "a"\nversion = 1\n')
    with pytest.raises(ScopeError):
        ScopeManifest.from_toml_str(
            'schema = "ncx-scope/v1"\nid = "../bad"\nname = "a"\nversion = 1\n'
        )
