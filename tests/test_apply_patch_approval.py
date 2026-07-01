"""Tests for apply_patch's approval integration (out-of-sandbox writes)."""

from __future__ import annotations

from pathlib import Path

from nanocodex.sandbox.approval import NEVER, ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext
from nanocodex.tools.apply_patch import ApplyPatchTool


def _ctx(tmp_path: Path, approver: Approver) -> ToolContext:
    ws = tmp_path / "ws"
    ws.mkdir(exist_ok=True)
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=ws, allow_temp_write=False)
    return ToolContext(
        workspace=ws,
        policy=policy,
        approver=approver,
        executor=make_executor(policy),
        plan=[],
    )


def _inside_patch() -> str:
    return "*** Begin Patch\n*** Add File: inside.txt\n+hi\n*** End Patch"


def _outside_patch() -> str:
    # Escapes the workspace via '..'.
    return "*** Begin Patch\n*** Add File: ../escape.txt\n+pwned\n*** End Patch"


async def test_inside_workspace_needs_no_approval(tmp_path):
    asked: list = []

    async def cb(req):
        asked.append(req)
        return True

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    tool = ApplyPatchTool(ctx)
    result = await tool.execute(patch=_inside_patch())
    assert "applied successfully" in result
    assert asked == []  # never prompted for in-workspace writes
    assert (ctx.workspace / "inside.txt").read_text() == "hi\n"


async def test_outside_workspace_prompts_and_applies_on_approve(tmp_path):
    asked: list = []

    async def cb(req):
        asked.append(req)
        return True

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    tool = ApplyPatchTool(ctx)
    result = await tool.execute(patch=_outside_patch())
    assert len(asked) == 1
    assert asked[0].escalated is True
    assert "applied successfully" in result
    assert (tmp_path / "escape.txt").read_text() == "pwned\n"


async def test_outside_workspace_blocked_on_deny(tmp_path):
    async def cb(req):
        return False

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    tool = ApplyPatchTool(ctx)
    result = await tool.execute(patch=_outside_patch())
    assert "not approved" in result
    assert not (tmp_path / "escape.txt").exists()


async def test_never_policy_auto_denies_outside_write(tmp_path):
    async def cb(req):
        raise AssertionError("never policy must not prompt")

    ctx = _ctx(tmp_path, Approver(NEVER, cb))
    tool = ApplyPatchTool(ctx)
    result = await tool.execute(patch=_outside_patch())
    assert "denied by approval policy 'never'" in result
    assert not (tmp_path / "escape.txt").exists()


async def test_parse_error_skips_approval(tmp_path):
    async def cb(req):
        raise AssertionError("parse errors must not reach approval")

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    tool = ApplyPatchTool(ctx)
    result = await tool.execute(patch="not a patch at all")
    assert result.startswith("Error applying patch")


async def test_approval_request_carries_patch_diff(tmp_path):
    # diff preview: the out-of-sandbox approval request must include the full
    # patch in `details` so the human reviews the actual change, not just paths.
    captured: list = []

    async def cb(req):
        captured.append(req)
        return True

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    tool = ApplyPatchTool(ctx)
    await tool.execute(patch=_outside_patch())
    assert captured and captured[0].details == _outside_patch()


async def test_step_approval_request_carries_patch_diff(tmp_path):
    # With per-step confirmation on, even an in-workspace patch prompts AND
    # carries the diff in `details`.
    captured: list = []

    async def cb(req):
        captured.append(req)
        return True

    ctx = _ctx(tmp_path, Approver(ON_REQUEST, cb))
    ctx.require_step_approval = True
    tool = ApplyPatchTool(ctx)
    await tool.execute(patch=_inside_patch())
    assert captured and captured[0].details == _inside_patch()
