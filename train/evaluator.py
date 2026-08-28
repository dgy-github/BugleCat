#!/usr/bin/env python3
"""Evaluator (ncx-forge M0a / P2).

Runs the Rust agent (ncx.exe) on bench tasks with a candidate genome injected
via NCX_GENOME, then grades each run and — crucially — harvests a FAILURE
TRAJECTORY from the agent's own session log before the temp workspace is
deleted. That trajectory (the last assistant message + the tool calls it made)
is the signal the teacher reads to propose a better genome; without it the
teacher is blind (bench/run.py keeps only a 70-char grader tail and rmtrees the
workspace).

Trust boundary: the grader's own output is NEVER surfaced to the teacher (it
would leak check.py). We read only the agent's session messages, and defensively
redact any line that references the grader artifacts.

Reuses bench/run.py's task discovery / seed / grade / agent_cmd helpers.
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

# Import bench helpers without running its main().
BENCH = Path(__file__).resolve().parent.parent / "bench"
sys.path.insert(0, str(BENCH))
import run as bench  # noqa: E402  (bench/run.py)
from process_control import run_owned  # noqa: E402

SESSION_LOG_REL = Path(".nanocodex") / "session.jsonl"
# Substrings whose presence in a trajectory line marks it as grader-tainted.
GRADER_MARKERS = ("check.py", "_check.py", "grader", "hidden test")
MAX_TRAJECTORY_CHARS = 2000


@dataclass
class TaskResult:
    task: str
    passes: int
    runs: int
    mean_s: float
    # Mean total tokens per run (0 if ncx did not report a [ncx-usage] line).
    mean_tokens: float = 0.0
    # One redacted failure trajectory (from the first failing run), or "".
    failure_trajectory: str = ""


@dataclass
class EvalResult:
    genome: str  # path or "<baseline>"
    tasks: dict[str, TaskResult] = field(default_factory=dict)

    @property
    def total_passes(self) -> int:
        return sum(t.passes for t in self.tasks.values())

    @property
    def total_runs(self) -> int:
        return sum(t.runs for t in self.tasks.values())

    @property
    def mean_tokens(self) -> float:
        """Mean total tokens per task (0 if ncx reported no usage). The Pareto
        cost axis when real token usage is available."""
        toks = [t.mean_tokens for t in self.tasks.values() if t.mean_tokens > 0]
        return round(sum(toks) / len(toks), 1) if toks else 0.0

    @property
    def passrate(self) -> float:
        n = self.total_runs
        return self.total_passes / n if n else 0.0

    def failing_trajectories(self, top_k: int = 3) -> list[tuple[str, str]]:
        """(task, trajectory) for tasks that did not fully pass, with text."""
        out = [
            (t.task, t.failure_trajectory)
            for t in self.tasks.values()
            if t.passes < t.runs and t.failure_trajectory
        ]
        return out[:top_k]


def _redact(text: str) -> str:
    """Drop lines referencing grader artifacts; hard-cap length."""
    kept = []
    for line in text.splitlines():
        low = line.lower()
        if any(m in low for m in GRADER_MARKERS):
            kept.append("[redacted: grader-related line]")
        else:
            kept.append(line)
    out = "\n".join(kept).strip()
    if len(out) > MAX_TRAJECTORY_CHARS:
        out = out[:MAX_TRAJECTORY_CHARS] + "\n[...truncated]"
    return out


def extract_trajectory(ws: Path) -> str:
    """Pull the agent's last assistant message + tool-call names from its
    session log. Returns a compact, redacted string ("" if no log)."""
    log = ws / SESSION_LOG_REL
    if not log.exists():
        return ""
    last_assistant = ""
    tool_calls: list[str] = []
    try:
        for raw in log.read_text(encoding="utf-8", errors="replace").splitlines():
            raw = raw.strip()
            if not raw:
                continue
            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                continue
            role = msg.get("role")
            if role == "assistant":
                content = msg.get("content")
                if isinstance(content, str) and content.strip():
                    last_assistant = content.strip()
                for tc in msg.get("tool_calls") or []:
                    fn = (tc.get("function") or {}).get("name")
                    args = (tc.get("function") or {}).get("arguments")
                    if fn:
                        arg_preview = (str(args)[:120]) if args else ""
                        tool_calls.append(f"{fn}({arg_preview})")
    except OSError:
        return ""
    parts = []
    if tool_calls:
        parts.append("Tool calls the agent made:\n- " + "\n- ".join(tool_calls[-12:]))
    if last_assistant:
        parts.append("Agent's final message:\n" + last_assistant)
    return _redact("\n\n".join(parts))


_USAGE_RE = re.compile(r"\[ncx-usage\][^\n]*\btotal_tokens=(\d+)")


def _parse_tokens(stderr: str) -> int:
    """Pull total_tokens from ncx's `[ncx-usage]` stderr line (0 if absent)."""
    m = _USAGE_RE.search(stderr or "")
    return int(m.group(1)) if m else 0


def _agent_cmd(prompt: str, model: str | None) -> list[str]:
    """bench's nanocodex command, optionally with a base-model override (`-m`)
    so a WEAKER base agent can be trained (where harness headroom is larger)."""
    cmd = bench.agent_cmd("nanocodex", prompt)
    if model:
        cmd = [cmd[0], "-m", model] + cmd[1:]
    return cmd


def _run_task_once(task: Path, genome_path: str | None, timeout: int,
                   model: str | None = None) -> tuple[bool, float, str, int]:
    """One (task, genome) attempt. Returns (passed, elapsed_s, trajectory_if_failed, tokens)."""
    prompt = (task / "prompt.txt").read_text(encoding="utf-8")
    ws = Path(tempfile.mkdtemp(prefix=f"forge_{task.name}_"))
    try:
        bench.seed(task, ws)
        env = dict(os.environ)
        if genome_path:
            env["NCX_GENOME"] = genome_path
        else:
            env.pop("NCX_GENOME", None)  # baseline: ensure no stray genome
        t0 = time.perf_counter()
        timed_out = False
        tokens = 0
        try:
            proc = run_owned(
                _agent_cmd(prompt, model),
                cwd=str(ws), env=env, capture_output=True, text=True,
                encoding="utf-8", errors="replace", timeout=timeout,
            )
            tokens = _parse_tokens(proc.stderr)
        except subprocess.TimeoutExpired:
            timed_out = True
        elapsed = time.perf_counter() - t0
        # Harvest the trajectory BEFORE grade() copies _check.py into the ws.
        trajectory = extract_trajectory(ws)
        ok, _ = bench.grade(task, ws)
        if not ok and not trajectory:
            # A failure with no captured trajectory (e.g. a timeout killed the
            # agent before it logged, or it produced nothing) must STILL be a
            # signal to the teacher — otherwise forge mistakes it for a pass.
            trajectory = (f"The agent did NOT finish within {timeout}s (timed out)."
                          if timed_out else
                          "The agent produced no usable session log and the task did not pass.")
        return ok, round(elapsed, 1), ("" if ok else trajectory), tokens
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def evaluate(genome_path: str | None, task_names: list[str] | None,
             repeats: int = 3, timeout: int = 180, model: str | None = None) -> EvalResult:
    """Evaluate a genome (or baseline when None) over the given tasks. `model`
    overrides the agent's base model (e.g. a weaker one with more headroom)."""
    all_tasks = bench.tasks()
    if task_names:
        all_tasks = [t for t in all_tasks if t.name in task_names]
    res = EvalResult(genome=genome_path or "<baseline>")
    for task in all_tasks:
        passes, times, toks, traj = 0, [], [], ""
        for _ in range(max(1, repeats)):
            ok, elapsed, t, tokens = _run_task_once(task, genome_path, timeout, model)
            passes += 1 if ok else 0
            times.append(elapsed)
            if tokens > 0:
                toks.append(tokens)
            if not ok and not traj:
                traj = t
        res.tasks[task.name] = TaskResult(
            task=task.name, passes=passes, runs=max(1, repeats),
            mean_s=round(sum(times) / len(times), 1) if times else 0.0,
            mean_tokens=round(sum(toks) / len(toks), 1) if toks else 0.0,
            failure_trajectory=traj,
        )
    return res


if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser(description="Evaluate a genome on bench tasks.")
    ap.add_argument("--genome", default=None, help="path to genome.toml (omit = baseline)")
    ap.add_argument("--tasks", default="", help="comma-separated task names")
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=180)
    a = ap.parse_args()
    names = [t.strip() for t in a.tasks.split(",") if t.strip()] or None
    r = evaluate(a.genome, names, a.repeats, a.timeout)
    print(f"genome={r.genome}  passrate={r.total_passes}/{r.total_runs}")
    for t in r.tasks.values():
        print(f"  {t.task:14} {t.passes}/{t.runs}  {t.mean_s:6.1f}s")
        if t.failure_trajectory:
            print(f"    trajectory: {t.failure_trajectory[:200]!r}")
