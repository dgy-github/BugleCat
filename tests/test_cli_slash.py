"""Slash-command dispatch acts on the live loop (mutating commands).

The parser is tested in test_slash.py; here we verify _dispatch_slash actually
changes loop state for /approvals, /model, and /clear, and that /exit signals
quit. Uses a minimal hand-built loop (no network).
"""

from __future__ import annotations

from pathlib import Path

import pytest

from nanocodex.agent.loop import AgentLoop
from nanocodex.agent.session import Session
import nanocodex.cli as cli_mod
from nanocodex.cli import _dispatch_slash, _run_loop_command
from nanocodex.config import Config
from nanocodex.provider.base import ModelResponse
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry


class _Provider:
    model = "scripted"

    async def chat(self, messages, tools=None, **kwargs):
        return ModelResponse(content="ok")


def _loop(tmp_path: Path) -> AgentLoop:
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req):
        return True

    ctx = ToolContext(workspace=tmp_path, policy=policy,
                      approver=Approver(ON_REQUEST, auto_yes),
                      executor=make_executor(policy), plan=[])
    loop = AgentLoop(_Provider(), ToolRegistry(ctx), Session("sys", log_path=None))
    loop._cfg = Config(api_key="sk", base_url="u", model="deepseek-chat")
    return loop


async def test_exit_returns_true(tmp_path):
    assert await _dispatch_slash(_loop(tmp_path), "/exit", "", {}) is True


async def test_approvals_sets_policy(tmp_path):
    loop = _loop(tmp_path)
    quit_ = await _dispatch_slash(loop, "/approvals", "never", {})
    assert quit_ is False
    assert loop.tools.ctx.approver.policy == "never"


async def test_approvals_rejects_bad_policy(tmp_path):
    loop = _loop(tmp_path)
    await _dispatch_slash(loop, "/approvals", "banana", {})
    assert loop.tools.ctx.approver.policy == ON_REQUEST  # unchanged


async def test_model_switches_provider(tmp_path):
    loop = _loop(tmp_path)
    await _dispatch_slash(loop, "/model", "deepseek-v4-pro", {})
    assert loop.provider.model == "deepseek-v4-pro"
    assert loop._cfg.model == "deepseek-v4-pro"


async def test_clear_resets_to_system_only(tmp_path):
    loop = _loop(tmp_path)
    loop.session.add_user("hi")
    loop.session.add_assistant("hello")
    assert len(loop.session.messages) > 1
    await _dispatch_slash(loop, "/clear", "", {})
    assert len(loop.session.messages) == 1
    assert loop.session.messages[0]["role"] == "system"


async def test_unknown_command_is_noop(tmp_path):
    assert await _dispatch_slash(_loop(tmp_path), "/bogus", "", {}) is False


async def test_loop_runs_iterations_until_interrupted(tmp_path, monkeypatch):
    # /loop re-runs the prompt each interval; we make the interval sleep raise
    # KeyboardInterrupt on the 2nd call so the loop stops after 2 iterations.
    loop = _loop(tmp_path)

    calls = {"turns": 0, "sleeps": 0}
    orig_run_turn = loop.run_turn

    async def counting_run_turn(*a, **k):
        calls["turns"] += 1
        return await orig_run_turn(*a, **k)

    loop.run_turn = counting_run_turn

    async def fake_sleep(_s):
        calls["sleeps"] += 1
        if calls["sleeps"] >= 2:
            raise KeyboardInterrupt
    monkeypatch.setattr(cli_mod.asyncio, "sleep", fake_sleep)

    # Should stop cleanly (KeyboardInterrupt caught), not propagate.
    await _run_loop_command(loop, "5m do the thing", {}, None)
    assert calls["turns"] == 2


async def test_loop_rejects_empty_prompt(tmp_path):
    loop = _loop(tmp_path)
    calls = {"turns": 0}

    async def counting_run_turn(*a, **k):
        calls["turns"] += 1
    loop.run_turn = counting_run_turn

    await _run_loop_command(loop, "", {}, None)
    assert calls["turns"] == 0  # never ran — empty prompt rejected
