"""Time-ordered issue replay runner for the intelligence-compounding experiment.

One invocation = one round = one arm replaying a curriculum in order.

Arms:
  A  no cross-task memory: workspace state (.ncx/, .nanocodex/) wiped before every task.
  B  compounding: workspace state carried from task to task (only that differs from A).

Isolation per task:
  * the workspace is rebuilt from `git archive <base_commit>` into a fresh one-commit
    git repo, so the agent cannot see future history or the fix commit;
  * the hidden judge tests are copied in only after the agent exits;
  * HOME/USERPROFILE points at a per-round fake home holding a copy of the user's
    ~/.nanocodex/config.toml, so global session/skill state never leaks across arms.

Cost: computed from the `[ncx-usage]` line with DeepSeek-style cache-aware prices
(override with --price-*). The round stops before a task whose worst case
(task cap) would exceed --round-budget.

Usage:
  python evals/compounding_replay/replay.py --arm B --limit 2          # smoke test
  python evals/compounding_replay/replay.py --arm A --limit 12 --seed-label r1
"""
from __future__ import annotations

import argparse
import io
import json
import os
import random
import re
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import time
import xml.etree.ElementTree as ET
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
STATE_DIRS = (".ncx", ".nanocodex")
USAGE_RE = re.compile(r"\[ncx-usage\]\s+(.*)")

PROMPT = """You are working in a checkout of the `{repo}` Python project (workspace root is the repo root).
Resolve the following GitHub issue by changing the library source code.

Rules:
- Make the smallest correct fix in the library code; keep existing behaviour otherwise.
- Verify with a single targeted pytest command (e.g. `python -m pytest -q tests/test_<module>.py -k <test_name>`). Do NOT run the full test suite (`pytest` without path) or linters (`flake8`/`tox`), and do not need to edit test files unless strictly necessary.
- Once your targeted pytest check passes and any temporary scratch files are removed, stop calling tools and provide your concise final summary immediately.
- Work strictly from the checked-out repository files; never use `pip download`/`pip install` or network access.
- Do not ask questions; finish the fix autonomously.

GitHub issue #{num}:
---
{problem}
---
"""


def sh(cmd: list[str], cwd: Path | None = None, check: bool = True, **kw) -> subprocess.CompletedProcess:
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, **kw)
    if check and r.returncode != 0:
        err = r.stderr.decode("utf-8", "replace") if isinstance(r.stderr, bytes) else r.stderr
        raise RuntimeError(f"{' '.join(cmd)} failed: {err.strip()[:500]}")
    return r


def load_tasks(path: Path, cache_repo: Path, order: str) -> list[dict]:
    tasks = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
    for t in tasks:
        t["fixed_at"] = sh(["git", "-C", str(cache_repo), "show", "-s", "--format=%cI",
                            t["fix_commit"]], text=True).stdout.strip()
    key = "fixed_at" if order == "fix" else "created_at"
    tasks.sort(key=lambda t: t[key])
    return tasks


def reset_workspace(ws: Path, cache_repo: Path, commit: str, keep_state: bool, state_store: Path) -> None:
    if ws.exists():
        shutil.rmtree(ws, onerror=_force_remove)
    ws.mkdir(parents=True)
    data = sh(["git", "-C", str(cache_repo), "archive", "--format=tar", commit]).stdout
    with tarfile.open(fileobj=io.BytesIO(data)) as tar:
        tar.extractall(ws, filter="data")
    if keep_state:
        src_mem = state_store / ".ncx" / "memory"
        if src_mem.exists():
            shutil.copytree(src_mem, ws / ".ncx" / "memory")
        src_ev = state_store / ".nanocodex" / "sessions" / "events.sqlite3"
        if src_ev.exists():
            dst_sess = ws / ".nanocodex" / "sessions"
            dst_sess.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src_ev, dst_sess / "events.sqlite3")
    sh(["git", "init", "-q"], cwd=ws)
    (ws / ".git" / "info").mkdir(parents=True, exist_ok=True)
    (ws / ".git" / "info" / "exclude").write_text("\n".join(STATE_DIRS) + "\n", encoding="utf-8")
    sh(["git", "add", "-A"], cwd=ws)
    sh(["git", "-c", "user.name=replay", "-c", "user.email=replay@local",
        "commit", "-q", "-m", "base"], cwd=ws)


def save_state(ws: Path, state_store: Path) -> None:
    if state_store.exists():
        shutil.rmtree(state_store, onerror=_force_remove)
    state_store.mkdir(parents=True, exist_ok=True)
    src_mem = ws / ".ncx" / "memory"
    if src_mem.exists():
        shutil.copytree(src_mem, state_store / ".ncx" / "memory")
    src_ev = ws / ".nanocodex" / "sessions" / "events.sqlite3"
    if src_ev.exists():
        dst_sess = state_store / ".nanocodex" / "sessions"
        dst_sess.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src_ev, dst_sess / "events.sqlite3")


def _force_remove(func, path, _exc):
    os.chmod(path, 0o700)
    func(path)


def make_home(home: Path, arm: str) -> None:
    real = Path(os.environ.get("USERPROFILE") or os.environ["HOME"]) / ".nanocodex" / "config.toml"
    if not real.exists():
        sys.exit(f"missing {real}; the replay reuses your real ncx config (api key, model)")
    (home / ".nanocodex").mkdir(parents=True, exist_ok=True)
    cfg_text = real.read_text(encoding="utf-8")
    mem_mode = "on" if arm in ("B", "D") else "off"
    if "[feedback]" in cfg_text:
        cfg_text = re.sub(r'(?m)^(\s*memory\s*=\s*).*$', f'\\1"{mem_mode}"', cfg_text)
    else:
        cfg_text = cfg_text.rstrip() + f'\n\n[feedback]\nmemory = "{mem_mode}"\n'
    (home / ".nanocodex" / "config.toml").write_text(cfg_text, encoding="utf-8")
    providers = real.with_name("providers.json")
    if providers.exists():
        shutil.copy(providers, home / ".nanocodex" / "providers.json")


def parse_usage(stderr: str) -> dict[str, int]:
    usage: dict[str, int] = {}
    for m in USAGE_RE.finditer(stderr):
        for kv in m.group(1).split():
            k, _, v = kv.partition("=")
            if v.isdigit():
                usage[k] = int(v)
    return usage


def cost_cny(u: dict[str, int], a: argparse.Namespace) -> float:
    prompt = u.get("prompt_tokens", 0)
    hit = u.get("prompt_cache_hit_tokens", 0)
    miss = u.get("prompt_cache_miss_tokens", 0) or max(prompt - hit, 0)
    out = u.get("completion_tokens", 0)
    return (hit * a.price_hit + miss * a.price_miss + out * a.price_out) / 1e6


def _norm_tid(tid: str) -> str:
    """Repo-relative test id. Checkouts without tests/__init__.py make pytest
    derive module names from an outer rootdir (e.g. task ids mined from the
    cache repo look like "evals.compounding_replay..cache.sqlparse.tests.x::y");
    both judge results and task ids are reduced to "tests.x::y" (or "test.x::y"
    for repos whose test dir is `test/`, e.g. python-tabulate)."""
    cls, sep, name = tid.partition("::")
    for marker in (".tests.", ".test."):
        k = cls.rfind(marker)
        if k >= 0:
            cls = cls[k + 1:]
            break
    return f"{cls}{sep}{name}"


def judge(ws: Path, cache_repo: Path, task: dict) -> dict:
    for f in task["test_files"]:
        blob = sh(["git", "-C", str(cache_repo), "show", f"{task['fix_commit']}:{f}"]).stdout
        (ws / f).parent.mkdir(parents=True, exist_ok=True)
        (ws / f).write_bytes(blob)
    with tempfile.TemporaryDirectory() as td:
        xml = Path(td) / "r.xml"
        env = dict(os.environ, PYTHONPATH=str(ws), PYTHONDONTWRITEBYTECODE="1")
        try:
            sh([sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider", "--tb=no",
                f"--junitxml={xml}", *task["test_files"]], cwd=ws, check=False, env=env, timeout=300)
        except subprocess.TimeoutExpired:
            return {"resolved": False, "error": "judge timeout"}
        res: dict[str, str] = {}
        if xml.exists():
            for c in ET.parse(xml).iter("testcase"):
                tid = _norm_tid(f"{c.get('classname')}::{c.get('name')}")
                res[tid] = "failed" if any(x.tag in ("failure", "error") for x in c) else "passed"
    f2p_ok = [t for t in task["fail_to_pass"] if res.get(_norm_tid(t)) == "passed"]
    p2p_broken = [t for t in task["pass_to_pass"] if res.get(_norm_tid(t)) != "passed"]
    return {
        "resolved": len(f2p_ok) == len(task["fail_to_pass"]) and not p2p_broken,
        "f2p_passed": len(f2p_ok), "f2p_total": len(task["fail_to_pass"]),
        "p2p_broken": p2p_broken[:20], "p2p_broken_count": len(p2p_broken),
    }


def memory_snapshot(ws: Path) -> dict:
    snap: dict = {}
    mem = ws / ".ncx" / "memory"
    if mem.exists():
        snap["memory_files"] = sum(1 for p in mem.rglob("*.md"))
        md = mem / "LEARNINGS.md"
        if md.exists():
            snap["learnings_entries"] = md.read_text(encoding="utf-8", errors="replace").count("<!-- ts:")
        db = mem / "index.sqlite3"
        if db.exists():
            try:
                con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
                tables = [r[0] for r in con.execute("select name from sqlite_master where type='table'")]
                snap["index_rows"] = {t: con.execute(f'select count(*) from "{t}"').fetchone()[0] for t in tables}
                if "memory_index" in tables:
                    snap["by_status"] = dict(con.execute(
                        "select status, count(*) from memory_index group by status").fetchall())
                if "memory_feedback" in tables:
                    row = con.execute(
                        "select coalesce(sum(injected),0), coalesce(sum(accepted),0), coalesce(sum(rejected),0) from memory_feedback"
                    ).fetchone()
                    snap["feedback_totals"] = {"injected": row[0], "accepted": row[1], "rejected": row[2]}
                con.close()
            except sqlite3.Error as e:
                snap["index_error"] = str(e)
    ev_db = ws / ".nanocodex" / "sessions" / "events.sqlite3"
    if ev_db.exists():
        try:
            con = sqlite3.connect(f"file:{ev_db}?mode=ro", uri=True)
            row = con.execute(
                "select payload_json from context_events where event_type='task_outcome' order by timestamp desc limit 1"
            ).fetchone()
            if row:
                payload = json.loads(row[0])
                snap["last_recalled_note_ids"] = payload.get("recalled_note_ids", [])
            con.close()
        except Exception:
            pass
    return snap


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", choices=["A", "B", "D"], required=True,
                    help="A=no memory, B=carry memory, D=carry memory with shuffled order")
    ap.add_argument("--tasks", default=str(HERE / "tasks" / "sqlparse.jsonl"))
    ap.add_argument("--repo-name", default="sqlparse")
    ap.add_argument("--order", choices=["fix", "created"], default="fix")
    ap.add_argument("--offset", type=int, default=0)
    ap.add_argument("--limit", type=int, default=12)
    ap.add_argument("--ids", default="", help="comma-separated task ids (overrides offset/limit)")
    ap.add_argument("--seed-label", default="r1", help="repeat label; also seeds arm D shuffle")
    ap.add_argument("--ncx", default=str(ROOT / "rust" / "target" / "release" / "ncx.exe"))
    ap.add_argument("--genome", default="",
                    help="optional path to an NCX_GENOME .toml file (e.g. from train/forge.py)")
    ap.add_argument("--max-iterations", type=int, default=20)
    ap.add_argument("--timeout", type=int, default=900, help="seconds per agent run")
    ap.add_argument("--round-budget", type=float, default=2.0, help="CNY per round")
    ap.add_argument("--task-cap", type=float, default=0.30, help="CNY; over-cap tasks count as failed")
    # CNY per 1M tokens. miss/out mirror price_in/price_out in ~/.nanocodex/config.toml;\n    # cache-hit price is an assumption (1/10 of miss) -- verify on the provider price page.
    ap.add_argument("--price-hit", type=float, default=0.3)
    ap.add_argument("--price-miss", type=float, default=3.0)
    ap.add_argument("--price-out", type=float, default=9.0)
    args = ap.parse_args()

    cache_repo = HERE / ".cache" / args.repo_name
    tasks = load_tasks(Path(args.tasks), cache_repo, args.order)
    if args.ids:
        # Keep chronological (fix-time) order regardless of the order ids were
        # given in, so memory never flows from a later fix into an earlier base.
        want = set(args.ids.split(","))
        tasks = [t for t in tasks if t["id"] in want]
    else:
        tasks = tasks[args.offset:args.offset + args.limit]
    if args.arm == "D":
        random.Random(args.seed_label).shuffle(tasks)

    run_id = f"{datetime.now():%Y%m%d-%H%M%S}-{args.arm}-{args.seed_label}"
    run_dir = HERE / "runs" / run_id
    ws, home, state = run_dir / "ws", run_dir / "home", run_dir / "state"
    for d in (home, state):
        d.mkdir(parents=True, exist_ok=True)
    make_home(home, args.arm)
    (run_dir / "config.json").write_text(json.dumps(
        {**vars(args), "task_ids": [t["id"] for t in tasks]}, indent=1), encoding="utf-8")

    env = dict(
        os.environ,
        USERPROFILE=str(home),
        HOME=str(home),
        PIP_NO_INDEX="1",
        PIP_DISABLE_PIP_VERSION_CHECK="1",
    )
    if args.genome:
        genome_path = Path(args.genome).resolve()
        if not genome_path.is_file():
            sys.exit(f"missing --genome file: {genome_path}")
        env["NCX_GENOME"] = str(genome_path)
    spent, results = 0.0, []
    out_f = (run_dir / "results.jsonl").open("w", encoding="utf-8")
    for idx, task in enumerate(tasks):
        # Worst-case guard: the larger of the per-task cap and the observed
        # average, so a round never knowingly overruns --round-budget.
        expect = max(args.task_cap, spent / len(results)) if results else args.task_cap
        if spent + expect > args.round_budget:
            print(f"budget stop before {task['id']}: spent {spent:.3f} CNY")
            break
        keep = args.arm in ("B", "D")
        reset_workspace(ws, cache_repo, task["base_commit"], keep, state)
        mem_before = memory_snapshot(ws)
        prompt = PROMPT.format(repo=task["repo"], num=task["issue_number"], problem=task["problem"])
        t0 = time.time()
        try:
            r = sh([args.ncx, "-w", str(ws), "-s", "workspace-write", "-a", "never",
                    "--max-iterations", str(args.max_iterations), prompt],
                   cwd=ws, check=False, env=env, timeout=args.timeout)
            stdout = r.stdout.decode("utf-8", "replace")
            stderr = r.stderr.decode("utf-8", "replace")
            exit_code = r.returncode
        except subprocess.TimeoutExpired as e:
            stdout = (e.stdout or b"").decode("utf-8", "replace")
            stderr = (e.stderr or b"").decode("utf-8", "replace")
            exit_code = "timeout"
        secs = round(time.time() - t0, 1)
        usage = parse_usage(stderr)
        cost = cost_cny(usage, args)
        spent += cost
        diff = sh(["git", "diff", "--stat"], cwd=ws, check=False, text=True).stdout
        verdict = judge(ws, cache_repo, task)
        over_cap = cost > args.task_cap
        resolved = verdict["resolved"] and not over_cap
        if keep:
            sh([args.ncx, "-w", str(ws), "--outcome-label", "good" if verdict["resolved"] else "bad"],
               cwd=ws, check=False, env=env, timeout=30)
            save_state(ws, state)
        mem_after = memory_snapshot(ws)
        row = {
            "idx": idx, "id": task["id"], "fixed_at": task["fixed_at"], "arm": args.arm,
            "resolved": resolved, "over_cap": over_cap,
            "judge": verdict, "exit_code": exit_code, "seconds": secs,
            "usage": usage, "cost_cny": round(cost, 4), "spent_cny": round(spent, 4),
            "diff_stat": diff.strip().splitlines()[-1:] if diff.strip() else [],
            "memory_before": mem_before, "memory_after": mem_after,
        }
        results.append(row)
        out_f.write(json.dumps(row, ensure_ascii=False) + "\n"); out_f.flush()
        log_dir = run_dir / "logs"; log_dir.mkdir(exist_ok=True)
        (log_dir / f"{idx:02d}-{task['id']}.out.txt").write_text(stdout, encoding="utf-8")
        (log_dir / f"{idx:02d}-{task['id']}.err.txt").write_text(stderr, encoding="utf-8")
        print(f"[{idx:02d}] {task['id']:<16} resolved={row['resolved']!s:<5} "
              f"f2p={verdict.get('f2p_passed')}/{verdict.get('f2p_total')} "
              f"p2p_broken={verdict.get('p2p_broken_count')} cost={cost:.3f} spent={spent:.3f} "
              f"{secs}s mem={mem_after}", flush=True)
    out_f.close()

    n = len(results)
    solved = sum(r["resolved"] for r in results)
    summary = {"run_id": run_id, "arm": args.arm, "tasks": n, "resolved": solved,
               "rate": round(solved / n, 3) if n else None, "spent_cny": round(spent, 4),
               "curve": [int(r["resolved"]) for r in results]}
    (run_dir / "summary.json").write_text(json.dumps(summary, indent=1), encoding="utf-8")
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
