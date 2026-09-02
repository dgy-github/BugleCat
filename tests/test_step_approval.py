"""Tests for per-step approval (the 'confirm each write' mode).

Covers the pure decision layer (step_decision) and the tool integration: with
require_step_approval ON, an in-sandbox shell command / apply_patch must PROMPT
(not silently auto-run), and a Deny must block the action.
"""

from __future__ import annotations

from nanocodex.sandbox.approval import (
    ON_REQUEST,
    Approver,
    Decision,
    step_decision,
)
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools.apply_patch import ApplyPatchTool
from nanocodex.tools.base import ToolContext
from nanocodex.tools.shell import ShellTool

# --- pure decision layer --------------------------------------------------

def test_step_decision_upgrades_write_when_required():
    # In-sandbox write would auto-run, but step approval forces a prompt.
    out = step_decision(Decision.AUTO_APPROVE, is_write=True, require_step_approval=True)
    assert out is Decision.ASK


def test_step_decision_leaves_reads_alone():
    out = step_decision(Decision.AUTO_APPROVE, is_write=False, require_step_approval=True)
    assert out is Decision.AUTO_APPROVE


def test_step_decision_off_is_noop():
    out = step_decision(Decision.AUTO_APPROVE, is_write=True, require_step_approval=False)
    assert out is Decision.AUTO_APPROVE


def test_step_decision_never_denies_stay_denied():
    out = step_decision(Decision.AUTO_DENY, is_write=True, require_step_approval=True)
    assert out is Decision.AUTO_DENY


def test_step_decision_existing_ask_stays_ask():
    out = step_decision(Decision.ASK, is_write=True, require_step_approval=False)
    assert out is Decision.ASK


# --- tool integration -----------------------------------------------------

def _ctx(tmp_path, *, answers, policy_name=ON_REQUEST, require_step=True):
    """Build a ToolContext whose approver records prompts and returns scripted
    answers. `answers` is a list popped per prompt (default Deny if empty)."""
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)
    asked = {"prompts": []}

    async def cb(req):
        asked["prompts"].append(req)
        return answers.pop(0) if answers else False

    ctx = ToolContext(
        workspace=tmp_path, policy=policy,
        approver=Approver(policy_name, cb),
        executor=make_executor(policy), plan=[],
        require_step_approval=require_step,
    )
    return ctx, asked


async def test_shell_prompts_when_step_approval_on(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[True])
    tool = ShellTool(ctx)
    # An in-sandbox, harmless command that would normally auto-run.
    out = await tool.execute(command="echo hello")
    assert asked["prompts"], "expected an approval prompt"
    assert "Exit code" in out                     # ran after approval


async def test_shell_denied_blocks_run(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[False])   # user denies
    tool = ShellTool(ctx)
    out = await tool.execute(command="echo hello")
    assert "not approved" in out.lower()
    assert len(asked["prompts"]) == 1


async def test_shell_no_prompt_when_step_approval_off(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[], require_step=False)
    tool = ShellTool(ctx)
    out = await tool.execute(command="echo hello")
    assert asked["prompts"] == []                  # auto-ran, no prompt
    assert "Exit code" in out


async def test_apply_patch_prompts_for_in_sandbox_write(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[True])
    tool = ApplyPatchTool(ctx)
    patch = (
        "*** Begin Patch\n"
        "*** Add File: note.txt\n"
        "+hello\n"
        "*** End Patch"
    )
    await tool.execute(patch=patch)
    assert asked["prompts"], "expected an approval prompt for the write"
    assert (tmp_path / "note.txt").exists()        # applied after approval


async def test_apply_patch_denied_does_not_write(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[False])
    tool = ApplyPatchTool(ctx)
    patch = (
        "*** Begin Patch\n"
        "*** Add File: note.txt\n"
        "+hello\n"
        "*** End Patch"
    )
    out = await tool.execute(patch=patch)
    assert "not approved" in out.lower()
    assert not (tmp_path / "note.txt").exists()    # nothing written


async def test_apply_patch_no_prompt_when_off(tmp_path):
    ctx, asked = _ctx(tmp_path, answers=[], require_step=False)
    tool = ApplyPatchTool(ctx)
    patch = (
        "*** Begin Patch\n"
        "*** Add File: note.txt\n"
        "+hello\n"
        "*** End Patch"
    )
    await tool.execute(patch=patch)
    assert asked["prompts"] == []                  # auto-applied in sandbox
    assert (tmp_path / "note.txt").exists()
