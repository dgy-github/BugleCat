"""Tests for the manage_schedule tool (offline; isolated store path)."""

from __future__ import annotations

import pytest

from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools.base import ToolContext
from nanocodex.tools.schedule_tool import ManageScheduleTool


@pytest.fixture
def tool(tmp_path, monkeypatch):
    # Redirect the schedule store to a temp file so tests don't touch the real
    # ~/.nanocodex/schedule.json. The tool constructs ScheduleStore() with the
    # default path, so patch that default.
    import nanocodex.agent.schedule as sched
    monkeypatch.setattr(sched, "DEFAULT_SCHEDULE_PATH", tmp_path / "schedule.json")

    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req):
        return True

    ctx = ToolContext(
        workspace=tmp_path, policy=policy,
        approver=Approver(ON_REQUEST, auto_yes),
        executor=make_executor(policy), plan=[],
    )
    return ManageScheduleTool(ctx)


async def test_add_daily_then_list(tool):
    out = await tool.execute(action="add", prompt="run the tests",
                             kind="daily", at_hour=9, at_minute=0)
    assert "added" in out.lower()
    assert "schedule run" in out          # reminds the user to start the runner

    listed = await tool.execute(action="list")
    assert "run the tests" in listed
    assert "daily" in listed


async def test_add_interval(tool):
    out = await tool.execute(action="add", prompt="check build",
                             kind="interval", every_seconds=1800)
    assert "added" in out.lower()


async def test_add_rejects_bad_input(tool):
    out = await tool.execute(action="add", prompt="x", kind="interval", every_seconds=0)
    assert out.lower().startswith("error")
    out2 = await tool.execute(action="add", prompt="", kind="once")
    assert out2.lower().startswith("error")


async def test_remove_and_missing_id(tool):
    await tool.execute(action="add", prompt="t", kind="daily", at_hour=8)
    listed = await tool.execute(action="list")
    task_id = listed.split()[0]                  # first token is the id
    removed = await tool.execute(action="remove", task_id=task_id)
    assert "removed" in removed.lower()
    # Removing again -> clear error.
    assert (await tool.execute(action="remove", task_id=task_id)).lower().startswith("error")


async def test_enable_disable(tool):
    await tool.execute(action="add", prompt="t", kind="daily", at_hour=8)
    tid = (await tool.execute(action="list")).split()[0]
    assert "disabled" in (await tool.execute(action="disable", task_id=tid)).lower()
    assert "enabled" in (await tool.execute(action="enable", task_id=tid)).lower()


async def test_remove_without_id_is_clear_error(tool):
    out = await tool.execute(action="remove")
    assert out.lower().startswith("error")
    assert "task_id" in out


async def test_unknown_action(tool):
    out = await tool.execute(action="frobnicate")
    assert out.lower().startswith("error")


async def test_empty_list(tool):
    assert "no scheduled tasks" in (await tool.execute(action="list")).lower()


def test_tool_registered_in_registry(tmp_path):
    # The registry should expose the tool to the model.
    from nanocodex.tools import ToolRegistry
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req):
        return True

    ctx = ToolContext(workspace=tmp_path, policy=policy,
                      approver=Approver(ON_REQUEST, auto_yes),
                      executor=make_executor(policy), plan=[])
    reg = ToolRegistry(ctx)
    assert "manage_schedule" in reg.names
