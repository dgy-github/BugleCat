#!/usr/bin/env python3
"""M3 — export agent trajectories as SFT/RL training data.

Runs a genome (default = current baseline) over bench tasks and writes one JSONL
record per run with the FULL message trajectory + a verifiable reward, in a
shape ready for supervised fine-tuning (imitate reward==1 trajectories) or RL
(reward = bench pass). This is the bridge from harness-evolution (prompt-level)
to weight-level training (DESIGN §10): the genome's evolved system prompt can
seed the model's system prompt, and these graded trajectories are the dataset.

Record (schema ncx-forge-trajectory/v1):
  { "schema", "genome_id", "model", "task", "reward" (0|1), "tokens",
    "system_prompt", "messages": [ {role, content, [tool_calls]} ], "final" }

The full session log is captured (NOT the redacted teacher view) — this is
training data, not a teacher prompt, so no grader-leak concern applies (the
grader still never runs during the agent's turn).

Usage:
  python train/export.py --tasks t1_mathutils,t3_fizzbuzz --out train/data/sft.jsonl
  python train/export.py --genome g.toml --model deepseek-chat --reward-pass-only
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
from process_control import run_owned  # noqa: E402
import genome as G  # noqa: E402

SCHEMA = "ncx-forge-trajectory/v1"
DATA_DIR = Path(__file__).resolve().parent / "data"


def _read_messages(ws: Path) -> list[dict]:
    """Parse the agent's full session.jsonl into clean messages (drop internal
    `_`-prefixed fields like `_ts`)."""
    log = ws / ev.SESSION_LOG_REL
    if not log.exists():
        return []
    msgs = []
    for raw in log.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            m = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if isinstance(m, dict):
            msgs.append({k: v for k, v in m.items() if not k.startswith("_")})
    return msgs


def _resolve_system_prompt(genome_path: str | None) -> str:
    """The base system prompt the agent ran with — the genome's override if set,
    else the agent's default (via `ncx --dump-genome`). ncx does not LOG the
    composed system message, so we record the evolvable base (the artifact under
    study); project-instruction/memory/skill suffixes are run-context, not data."""
    base = ""
    try:
        base = G.extract_current().system_prompt
    except Exception:  # noqa: BLE001
        pass
    if genome_path:
        try:
            g = G.Genome.load(Path(genome_path))
            if g.system_prompt.strip():
                base = g.system_prompt
        except Exception:  # noqa: BLE001
            pass
    return base


def _run_and_capture(task: Path, genome_path: str | None, timeout: int,
                     model: str | None, system_prompt: str = "") -> dict:
    """Run one task, return a trajectory record (reward + full messages + tokens)."""
    prompt = (task / "prompt.txt").read_text(encoding="utf-8")
    ws = Path(tempfile.mkdtemp(prefix=f"export_{task.name}_"))
    try:
        ev.bench.seed(task, ws)
        env = dict(os.environ)
        if genome_path:
            env["NCX_GENOME"] = genome_path
        else:
            env.pop("NCX_GENOME", None)
        tokens = 0
        try:
            proc = run_owned(
                ev._agent_cmd(prompt, model),
                cwd=str(ws), env=env, capture_output=True, text=True,
                encoding="utf-8", errors="replace", timeout=timeout,
            )
            tokens = ev._parse_tokens(proc.stderr)
        except subprocess.TimeoutExpired:
            pass
        messages = _read_messages(ws)          # full trajectory, BEFORE grading
        ok, _ = ev.bench.grade(task, ws)
        # Prefer a logged system message if present; else the resolved base.
        system = next((m.get("content", "") for m in messages if m.get("role") == "system"),
                      system_prompt)
        final = next((m.get("content", "") for m in reversed(messages)
                      if m.get("role") == "assistant" and m.get("content")), "")
        return {"task": task.name, "reward": 1 if ok else 0, "tokens": tokens,
                "system_prompt": system, "messages": messages, "final": final}
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def export(genome_path: str | None, task_names: list[str] | None, out_path: Path,
           repeats: int = 1, timeout: int = 180, model: str | None = None,
           reward_pass_only: bool = False, genome_id: str = "") -> dict:
    """Run tasks and write JSONL trajectory records. Returns a summary dict."""
    all_tasks = ev.bench.tasks()
    if task_names:
        all_tasks = [t for t in all_tasks if t.name in task_names]
    out_path.parent.mkdir(parents=True, exist_ok=True)
    n_written = n_pass = 0
    gid = genome_id or (Path(genome_path).stem if genome_path else "baseline")
    system_prompt = _resolve_system_prompt(genome_path)
    with out_path.open("w", encoding="utf-8") as f:
        for task in all_tasks:
            for _ in range(max(1, repeats)):
                rec = _run_and_capture(task, genome_path, timeout, model, system_prompt)
                rec.update({"schema": SCHEMA, "genome_id": gid, "model": model or "default"})
                n_pass += rec["reward"]
                if reward_pass_only and rec["reward"] == 0:
                    continue
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                n_written += 1
    summary = {"out": str(out_path), "records": n_written, "passed": n_pass,
               "tasks": [t.name for t in all_tasks], "reward_pass_only": reward_pass_only}
    print(f"[export] wrote {n_written} record(s) ({n_pass} passing) -> {out_path}")
    return summary


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Export graded agent trajectories (SFT/RL data).")
    ap.add_argument("--genome", default="", help="genome.toml (omit = baseline)")
    ap.add_argument("--tasks", default="", help="comma-separated task names (default: all)")
    ap.add_argument("--out", default=str(DATA_DIR / "trajectories.jsonl"))
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--model", default="", help="agent base model override")
    ap.add_argument("--reward-pass-only", action="store_true",
                    help="only write reward==1 trajectories (SFT imitation set)")
    a = ap.parse_args()
    names = [t.strip() for t in a.tasks.split(",") if t.strip()] or None
    export(a.genome or None, names, Path(a.out), a.repeats, a.timeout,
           a.model or None, a.reward_pass_only)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
