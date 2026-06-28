#!/usr/bin/env python3
"""Agentic-RL rollout collector for ncx-forge (the piece rl_design() specified).

GRPO/PPO over a coding agent needs EPISODES, not single completions: the policy
model takes a turn, tools execute in a sandbox, repeat, and the reward is the
bench pass (0/1) at the end. This module collects those episodes and the
group-normalised advantages GRPO consumes. The policy and tool executor are
INJECTED (chat_fn / tool_exec), so the loop + advantage math run and are tested
here without a GPU; only the actual weight update (policy_update) needs torch.

Two ways to produce a rollout:
  * collect_rollout(...) — a minimal from-scratch model<->tools loop (full
    token-level control; you supply chat_fn = policy.generate, tool_exec).
  * ncx_episode(...) — REUSE ncx's real loop/tools/sandbox by pointing it at an
    OpenAI-compatible server (e.g. vLLM) that serves the policy model, then read
    its session.jsonl as the trajectory. Pragmatic + battle-tested; recommended.

Reward is always bench_reward (the hidden check) — verifiable, not learned.

GPU prereqs for the update step: torch + the served/loaded policy model.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
from finetune import bench_reward  # noqa: E402  (reuse the verifiable reward)


@dataclass
class Rollout:
    task: str
    system_prompt: str
    messages: list[dict]            # full chat: system, user, assistant/tool turns
    assistant_turns: list[dict]     # the model's OWN generated turns (policy-grad targets)
    reward: float
    turns: int
    stopped: str                    # "final" | "max_turns" | "error"


# ── from-scratch agentic loop (policy + tools injected) ─────────────────────────

def collect_rollout(task_name: str, system_prompt: str, chat_fn, tool_exec,
                    max_turns: int = 12) -> Rollout:
    """Run one episode: model<->tools in a fresh sandbox; reward = bench pass.

    chat_fn(messages) -> {"content": str, "tool_calls": [ {id?, function:{name,arguments}} ]}
        the policy model's next turn (production: vLLM/HF generate; tests: scripted).
    tool_exec(tool_call, workspace) -> str
        execute one tool call in `workspace`, return the tool result string.
    """
    task = next((t for t in ev.bench.tasks() if t.name == task_name), None)
    if task is None:
        return Rollout(task_name, system_prompt, [], [], 0.0, 0, "error")
    ws = Path(tempfile.mkdtemp(prefix=f"rollout_{task_name}_"))
    try:
        ev.bench.seed(task, ws)
        user = (task / "prompt.txt").read_text(encoding="utf-8")
        messages: list[dict] = [{"role": "system", "content": system_prompt},
                                {"role": "user", "content": user}]
        assistant_turns: list[dict] = []
        stopped = "max_turns"
        for _ in range(max_turns):
            resp = chat_fn(messages)
            assistant_turns.append(resp)
            asst = {"role": "assistant", "content": resp.get("content", "")}
            tcs = resp.get("tool_calls") or []
            if tcs:
                asst["tool_calls"] = tcs
            messages.append(asst)
            if not tcs:
                stopped = "final"
                break
            for tc in tcs:
                result = tool_exec(tc, ws)
                messages.append({"role": "tool", "content": result})
        reward = bench_reward(task_name, ws)
        return Rollout(task_name, system_prompt, messages, assistant_turns,
                       reward, len(assistant_turns), stopped)
    finally:
        shutil.rmtree(ws, ignore_errors=True)


# ── ncx-as-episode-engine adapter (recommended production path) ─────────────────

def ncx_episode(task_name: str, model: str, base_url: str | None = None,
                genome_path: str | None = None, timeout: int = 180) -> Rollout:
    """Run a real episode via ncx pointed at the policy model (served OpenAI-style,
    e.g. vLLM at `base_url`). Reuses ncx's loop/tools/sandbox; reads session.jsonl
    as the trajectory. The system_prompt is the genome base (what ncx composed)."""
    task = next((t for t in ev.bench.tasks() if t.name == task_name), None)
    if task is None:
        return Rollout(task_name, "", [], [], 0.0, 0, "error")
    ws = Path(tempfile.mkdtemp(prefix=f"ncxep_{task_name}_"))
    try:
        ev.bench.seed(task, ws)
        env = dict(os.environ)
        if genome_path:
            env["NCX_GENOME"] = genome_path
        if base_url:
            env["DEEPSEEK_BASE_URL"] = base_url  # ncx provider is OpenAI-compatible
        prompt = (task / "prompt.txt").read_text(encoding="utf-8")
        try:
            subprocess.run(ev._agent_cmd(prompt, model), cwd=str(ws), env=env,
                           capture_output=True, text=True, encoding="utf-8",
                           errors="replace", timeout=timeout)
            stopped = "final"
        except subprocess.TimeoutExpired:
            stopped = "max_turns"
        # Read trajectory BEFORE grade() copies _check.py in.
        log = ws / ev.SESSION_LOG_REL
        messages = []
        if log.exists():
            for raw in log.read_text(encoding="utf-8", errors="replace").splitlines():
                raw = raw.strip()
                if raw:
                    try:
                        m = json.loads(raw)
                        messages.append({k: v for k, v in m.items() if not k.startswith("_")})
                    except json.JSONDecodeError:
                        pass
        reward = bench_reward(task_name, ws)
        asst = [m for m in messages if m.get("role") == "assistant"]
        return Rollout(task_name, "", messages, asst, reward, len(asst), stopped)
    finally:
        shutil.rmtree(ws, ignore_errors=True)


# ── GRPO grouping (pure, tested here) ───────────────────────────────────────────

def grpo_advantages(rewards: list[float], eps: float = 1e-6) -> list[float]:
    """Group-relative advantages (GRPO): (r - mean) / (std + eps). With no spread
    (all equal), advantages are 0 — the group is uninformative, as expected."""
    n = len(rewards)
    if n == 0:
        return []
    mean = sum(rewards) / n
    var = sum((r - mean) ** 2 for r in rewards) / n
    std = var ** 0.5
    return [(r - mean) / (std + eps) for r in rewards]


@dataclass
class Group:
    task: str
    rollouts: list[Rollout]
    advantages: list[float] = field(default_factory=list)

    @property
    def solve_rate(self) -> float:
        return sum(r.reward for r in self.rollouts) / max(1, len(self.rollouts))


def collect_group(task_name: str, system_prompt: str, chat_fn, tool_exec,
                  n: int = 8, max_turns: int = 12) -> Group:
    """N episodes for one task -> group-normalised advantages (GRPO step input)."""
    rollouts = [collect_rollout(task_name, system_prompt, chat_fn, tool_exec, max_turns)
                for _ in range(n)]
    return Group(task_name, rollouts, grpo_advantages([r.reward for r in rollouts]))


def run_grpo(model, tasks: list[str], system_prompt: str, chat_fn, tool_exec,
             steps: int = 100, group_size: int = 8, **kw) -> None:  # pragma: no cover (GPU)
    """GRPO training skeleton. Per step: pick a task, collect a group, compute
    advantages, and update the policy on its OWN assistant tokens weighted by the
    group advantage (zero-advantage groups contribute nothing). The token-level
    update is the GPU/torch part — wire `policy_update` to your trainer (trl GRPO
    with a custom rollout, or a hand-rolled PG step over assistant_turns)."""
    raise NotImplementedError(
        "Wire policy_update(model, group) on a GPU. Inputs ready: collect_group() "
        "gives rollouts + advantages; assistant_turns are the policy-grad targets; "
        "bench_reward is the terminal reward. See module docstring + rl_design().")


if __name__ == "__main__":
    # Demo the GRPO advantage on a toy group (no model needed).
    rs = [1.0, 1.0, 0.0, 0.0]
    print("rewards   :", rs)
    print("advantages:", [round(a, 3) for a in grpo_advantages(rs)])
    print("all-pass  :", grpo_advantages([1.0, 1.0, 1.0]), "(zero -> uninformative)")
