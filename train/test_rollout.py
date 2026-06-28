#!/usr/bin/env python3
"""Tests for the agentic-RL rollout collector + GRPO advantages (no GPU/model)."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rollout as R  # noqa: E402


def test_grpo_advantages_normalises_group():
    adv = R.grpo_advantages([1.0, 1.0, 0.0, 0.0])
    # mean 0.5, std 0.5 -> +1,+1,-1,-1 (within eps)
    assert adv[0] > 0.99 and adv[2] < -0.99, adv
    assert abs(sum(adv)) < 1e-6, adv  # advantages sum to ~0


def test_grpo_advantages_all_equal_is_zero():
    assert all(abs(a) < 1e-3 for a in R.grpo_advantages([1.0, 1.0, 1.0])), "uninformative group -> 0"
    assert R.grpo_advantages([]) == []


def test_collect_rollout_runs_tool_loop_then_finishes():
    # Scripted policy: turn 1 calls a tool, turn 2 finishes (no tool_calls).
    turns = iter([
        {"content": "", "tool_calls": [{"function": {"name": "apply_patch", "arguments": "{}"}}]},
        {"content": "all done", "tool_calls": []},
    ])
    calls = {"tool": 0}

    def chat_fn(messages):
        return next(turns)

    def tool_exec(tc, ws):
        calls["tool"] += 1
        return "Patch applied"

    orig = R.bench_reward
    R.bench_reward = lambda task, ws: 1.0   # don't require a real solve
    try:
        ro = R.collect_rollout("t1_mathutils", "SYS", chat_fn, tool_exec, max_turns=5)
    finally:
        R.bench_reward = orig

    assert ro.stopped == "final" and ro.turns == 2, (ro.stopped, ro.turns)
    assert calls["tool"] == 1
    assert ro.reward == 1.0
    roles = [m["role"] for m in ro.messages]
    assert roles == ["system", "user", "assistant", "tool", "assistant"], roles
    assert len(ro.assistant_turns) == 2


def test_collect_rollout_unknown_task_errors_gracefully():
    ro = R.collect_rollout("no_such_task", "SYS", lambda m: {"content": "x"}, lambda tc, ws: "")
    assert ro.stopped == "error" and ro.reward == 0.0


def test_collect_group_sizes_and_advantages():
    # All rollouts get reward 1 -> solve_rate 1.0, advantages all ~0.
    R_bench = R.bench_reward
    R.bench_reward = lambda task, ws: 1.0
    try:
        g = R.collect_group("t1_mathutils", "SYS",
                            lambda m: {"content": "done", "tool_calls": []},
                            lambda tc, ws: "", n=4)
    finally:
        R.bench_reward = R_bench
    assert len(g.rollouts) == 4 and g.solve_rate == 1.0
    assert all(abs(a) < 1e-3 for a in g.advantages)


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn(); print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1; print(f"FAIL {fn.__name__}: {e}")
        except Exception as e:  # noqa: BLE001
            failed += 1; print(f"ERR  {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
