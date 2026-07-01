"""Layer 3: role-based tool isolation is enforced in code."""

from __future__ import annotations

from pathlib import Path

from nanocodex.agent.roles import ROLES, build_role_registry
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import READ_ONLY, WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext


def _ctx(tmp_path: Path) -> ToolContext:
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req) -> bool:
        return True

    return ToolContext(
        workspace=tmp_path,
        policy=policy,
        approver=Approver(ON_REQUEST, auto_yes),
        executor=make_executor(policy),
        plan=[],
    )


def test_planner_cannot_get_apply_patch(tmp_path):
    reg = build_role_registry(_ctx(tmp_path), "planner")
    assert "apply_patch" not in reg.names
    assert "read_file" in reg.names
    assert "update_plan" in reg.names


def test_verifier_cannot_get_apply_patch(tmp_path):
    reg = build_role_registry(_ctx(tmp_path), "verifier")
    assert "apply_patch" not in reg.names
    assert "read_file" in reg.names


def test_research_cannot_get_apply_patch(tmp_path):
    reg = build_role_registry(_ctx(tmp_path), "research")
    assert "apply_patch" not in reg.names
    assert "record_fact" in reg.names


def test_code_role_gets_editing_and_verify_request(tmp_path):
    reg = build_role_registry(_ctx(tmp_path), "code")
    assert "apply_patch" in reg.names
    assert "write_checkpoint" in reg.names
    assert "request_verification" in reg.names


def test_read_only_roles_get_read_only_policy(tmp_path):
    """A planner's shell runs under a read-only sandbox — writes are impossible."""
    for role in ("planner", "research", "verifier"):
        reg = build_role_registry(_ctx(tmp_path), role)
        assert reg.ctx.policy.mode == READ_ONLY
        assert reg.ctx.policy.writes_allowed is False


def test_code_role_keeps_writable_policy(tmp_path):
    reg = build_role_registry(_ctx(tmp_path), "code")
    assert reg.ctx.policy.mode == WORKSPACE_WRITE
    assert reg.ctx.policy.writes_allowed is True


def test_every_role_table_entry_builds(tmp_path):
    for role in ROLES:
        reg = build_role_registry(_ctx(tmp_path), role)
        assert reg.names  # non-empty
