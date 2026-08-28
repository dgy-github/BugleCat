#!/usr/bin/env python3
"""Three-way coding-agent benchmark: nanocodex / opencode / Claude Code.

Each task is a dir under tasks/ with prompt.txt (given to the agent), optional
seed files, and a hidden check.py (graded AFTER the agent runs; exit 0 = pass).
For each (agent, task): fresh temp workspace, seed it, run the agent on the
prompt in that workspace (with a timeout), copy check.py in, run it, record
pass/fail + wall-clock.

Each (agent, task) is run `--repeats` times (default 3) and reported as a
pass-rate (k/N), since agents are non-deterministic. A markdown + JSON report
is written under bench/reports/ unless --no-report is given.

Usage:
  python bench/run.py --agent nanocodex
  python bench/run.py --agent nanocodex-orch --repeats 5
  python bench/run.py --agent all --timeout 240
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path

BENCH = Path(__file__).resolve().parent
TASKS = BENCH / "tasks"
REPORTS = BENCH / "reports"
NCX = Path(os.environ.get(
    "NCX_FORGE_NCX_BIN", BENCH.parent / "rust" / "target" / "release" / "ncx.exe"
)).resolve()
HIDDEN = {"check.py", "prompt.txt"}
ALL_AGENTS = ["nanocodex", "nanocodex-orch", "opencode", "claude"]


def tasks(filt: str = "") -> list[Path]:
    dirs = sorted(d for d in TASKS.iterdir() if d.is_dir())
    if not filt:
        return dirs
    keys = [k.strip() for k in filt.split(",") if k.strip()]
    return [d for d in dirs if any(k in d.name for k in keys)]


def seed(task: Path, ws: Path) -> None:
    for f in task.iterdir():
        if f.name in HIDDEN:
            continue
        (shutil.copytree if f.is_dir() else shutil.copy)(f, ws / f.name)


def grade(task: Path, ws: Path) -> tuple[bool, str]:
    shutil.copy(task / "check.py", ws / "_check.py")
    try:
        r = subprocess.run([sys.executable, "_check.py"], cwd=ws,
                           capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
        return r.returncode == 0, (r.stdout + r.stderr).strip()
    except Exception as e:  # noqa: BLE001
        return False, f"grader error: {e}"


def agent_cmd(agent: str, prompt: str) -> list[str]:
    if agent == "nanocodex":
        return [str(NCX), "-s", "workspace-write", prompt]
    if agent == "nanocodex-orch":
        return [str(NCX), "-s", "workspace-write", "-o", prompt]
    if agent == "opencode":
        import os
        oc = os.environ.get("OPENCODE_BIN", "opencode")
        return [oc, "run", prompt]
    if agent == "claude":
        return ["claude", "-p", prompt, "--permission-mode", "acceptEdits"]
    raise SystemExit(f"unknown agent {agent}")


def run_once(agent: str, task: Path, timeout: int) -> tuple[bool, float, str]:
    """Run one (agent, task) attempt; return (passed, elapsed_s, last_log_line).

    Raises FileNotFoundError if the agent binary is missing (caller skips agent).
    """
    prompt = (task / "prompt.txt").read_text(encoding="utf-8")
    ws = Path(tempfile.mkdtemp(prefix=f"bench_{agent}_{task.name}_"))
    try:
        seed(task, ws)
        t0 = time.perf_counter()
        try:
            subprocess.run(agent_cmd(agent, prompt), cwd=str(ws),
                           capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
        except subprocess.TimeoutExpired:
            pass
        elapsed = time.perf_counter() - t0
        ok, log = grade(task, ws)
        tail = "" if ok else (log.splitlines()[-1][:70] if log else "no output")
        return ok, round(elapsed, 1), tail
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def run_agent(agent: str, timeout: int, repeats: int, filt: str = "") -> dict:
    """Return {task_name: {"passes": k, "runs": n, "mean_s": float, "note": str}}."""
    print(f"\n=== {agent}  (x{repeats}) ===")
    results: dict[str, dict] = {}
    for task in tasks(filt):
        passes = 0
        times: list[float] = []
        note = ""
        for _ in range(repeats):
            try:
                ok, elapsed, tail = run_once(agent, task, timeout)
            except FileNotFoundError:
                print(f"  {agent} not installed — skipping the rest")
                return results
            passes += 1 if ok else 0
            times.append(elapsed)
            if not ok and not note:
                note = tail
        mean_s = round(sum(times) / len(times), 1) if times else 0.0
        rate = f"{passes}/{repeats}"
        tail = "" if passes == repeats else f"   ({note})"
        print(f"  {task.name:14} {rate:>5}  {mean_s:6.1f}s{tail}")
        results[task.name] = {"passes": passes, "runs": repeats, "mean_s": mean_s, "note": note}
    tot_p = sum(r["passes"] for r in results.values())
    tot_n = sum(r["runs"] for r in results.values())
    solved = sum(1 for r in results.values() if r["passes"] == r["runs"])
    print(f"  -> {agent}: {tot_p}/{tot_n} runs passed; {solved}/{len(results)} tasks fully solved")
    return results


def write_report(summary: dict, repeats: int, timeout: int) -> Path:
    REPORTS.mkdir(exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    task_names = sorted({t for r in summary.values() for t in r})

    lines = [f"# nanocodex bench report — {stamp}", ""]
    lines.append(f"- repeats per task: **{repeats}**, per-task timeout: **{timeout}s**")
    lines.append(f"- agents: {', '.join(summary)}")
    lines.append("")
    # Per-task pass-rate matrix (rows = tasks, cols = agents).
    header = "| task | " + " | ".join(summary) + " |"
    sep = "|------|" + "|".join(["------"] * len(summary)) + "|"
    lines += [header, sep]
    for t in task_names:
        cells = []
        for a in summary:
            r = summary[a].get(t)
            cells.append(f"{r['passes']}/{r['runs']}" if r else "—")
        lines.append(f"| {t} | " + " | ".join(cells) + " |")
    lines.append("")
    # Per-agent totals.
    lines += ["## Totals", ""]
    lines.append("| agent | runs passed | tasks solved | mean s/run |")
    lines.append("|-------|-------------|--------------|------------|")
    for a, res in summary.items():
        tot_p = sum(r["passes"] for r in res.values())
        tot_n = sum(r["runs"] for r in res.values())
        solved = sum(1 for r in res.values() if r["passes"] == r["runs"])
        all_times = [r["mean_s"] for r in res.values() if r["mean_s"]]
        mean = round(sum(all_times) / len(all_times), 1) if all_times else 0.0
        lines.append(f"| {a} | {tot_p}/{tot_n} | {solved}/{len(res)} | {mean} |")

    md = REPORTS / f"report_{stamp}.md"
    md.write_text("\n".join(lines) + "\n", encoding="utf-8")
    js = REPORTS / f"report_{stamp}.json"
    js.write_text(json.dumps(
        {"stamp": stamp, "repeats": repeats, "timeout": timeout, "results": summary},
        indent=2), encoding="utf-8")
    return md


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--agent", default="nanocodex",
                    help="nanocodex | nanocodex-orch | opencode | claude | all")
    ap.add_argument("--timeout", type=int, default=180, help="per-task seconds")
    ap.add_argument("--repeats", type=int, default=3, help="runs per (agent, task)")
    ap.add_argument("--no-report", action="store_true", help="skip writing bench/reports/")
    ap.add_argument("--tasks", default="", help="comma-separated name filters (e.g. t5,t6)")
    args = ap.parse_args()
    agents = ALL_AGENTS if args.agent == "all" else [args.agent]

    summary: dict[str, dict] = {}
    for a in agents:
        res = run_agent(a, args.timeout, max(1, args.repeats), args.tasks)
        if res:  # skip agents that weren't installed
            summary[a] = res

    print("\n==== SUMMARY ====")
    for a, res in summary.items():
        tot_p = sum(r["passes"] for r in res.values())
        tot_n = sum(r["runs"] for r in res.values())
        solved = sum(1 for r in res.values() if r["passes"] == r["runs"])
        print(f"  {a:16} {tot_p}/{tot_n} runs   {solved}/{len(res)} tasks")

    if summary and not args.no_report:
        path = write_report(summary, max(1, args.repeats), args.timeout)
        print(f"\nreport written: {path}")


if __name__ == "__main__":
    main()
