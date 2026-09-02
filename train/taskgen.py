#!/usr/bin/env python3
"""TaskGen — self-verifying bench-task generation for ncx-forge (M1).

A teacher model proposes a new Python coding task as strict JSON:

    {
      "name": "snake_case",
      "prompt": "<instructions given to the agent>",
      "check": "<python source for the hidden grader; exit 0 = pass>",
      "seed":      {"buggy.py": "..."},   # optional: files the agent STARTS with
      "reference": {"buggy.py": "...", "solution.py": "..."}  # CORRECT final state
    }

A task is admitted to bench/tasks/ only if it SELF-VALIDATES, deterministically:
  1. reference state PASSES the check (twice — guards against flaky/random checks)
  2. the starting state (seed only, i.e. what the agent gets) FAILS the check
     (proves the task is non-trivial — there is real work to do)
This is the same discipline used to hand-write t1–t13; here it gates machine
output so a malformed, non-deterministic, or already-solved task never enters
the corpus. The grader is never shown to the agent (it lands as hidden check.py).
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import teacher  # noqa: E402
from process_control import run_owned  # noqa: E402

BENCH_TASKS = Path(__file__).resolve().parent.parent / "bench" / "tasks"
NAME_RE = re.compile(r"[^a-z0-9_]")
_JSON_FENCE = re.compile(r"```(?:json)?\s*(\{.*\})\s*```", re.DOTALL)

DIMENSIONS = [
    "string/parsing algorithm with tricky edge cases (empty, unicode, overlap)",
    "fix a subtly buggy function (off-by-one / boundary / mutation aliasing)",
    "small stateful data structure (cache/queue/graph) with invariant checks",
    "recursive or dynamic-programming problem with base-case edge handling",
    "numeric/format conversion that must round-trip exactly",
    "multi-function module where the functions must compose correctly",
]

GEN_PROMPT = """You are authoring ONE self-contained Python coding task to benchmark a coding agent.
Theme/difficulty: {dimension}

The task must be HARDER than "write add(a,b)": include real edge cases. It must be
fully deterministic (no randomness, no clock, no network, no file/OS access beyond
importing the solution module).

Return ONLY a single ```json fenced block with EXACTLY these keys:
- "name": short snake_case id (letters/digits/underscore only).
- "prompt": the instructions GIVEN TO THE AGENT. Tell it exactly which file(s) to
  create or edit and the required function/class signatures + behavior. Do NOT
  mention tests, grading, or check files.
- "check": Python SOURCE for the hidden grader. It imports the agent's module(s)
  and uses `assert` to verify correctness across edge cases, then prints "ok".
  Exit code 0 = pass. Deterministic only. Do not print the expected answers in prose.
- "seed": object mapping filename->contents the agent STARTS with (e.g. a buggy
  file to fix). Use {{}} for create-from-scratch tasks.
- "reference": object mapping filename->contents representing a CORRECT final
  solution (this must make "check" pass). Include every file "check" imports.

Hard requirements (your task is REJECTED if violated):
- Running "check" against "reference" MUST pass (exit 0).
- Running "check" against "seed" alone MUST fail (there must be real work to do).
- "check" must be deterministic: same result every run.

Output the ```json block now."""


def _sanitize_name(raw: str) -> str:
    n = NAME_RE.sub("_", (raw or "").strip().lower()).strip("_")
    return f"gen_{n}" if n else ""


def _parse(resp: str) -> dict | None:
    if not resp:
        return None
    m = _JSON_FENCE.search(resp)
    blob = m.group(1) if m else resp[resp.find("{"): resp.rfind("}") + 1]
    try:
        d = json.loads(blob)
    except (json.JSONDecodeError, ValueError):
        return None
    if not isinstance(d, dict) or "check" not in d or "prompt" not in d or "reference" not in d:
        return None
    d.setdefault("seed", {})
    return d


def _run_check(check_src: str, files: dict[str, str], timeout: int = 60) -> tuple[bool, str]:
    """Write files + the check into a temp dir, run it. Return (passed, last_line)."""
    ws = Path(tempfile.mkdtemp(prefix="taskgen_v_"))
    try:
        for rel, content in files.items():
            p = ws / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
        (ws / "_check.py").write_text(check_src, encoding="utf-8")
        try:
            r = run_owned([sys.executable, "_check.py"], cwd=str(ws),
                               capture_output=True, text=True, encoding="utf-8",
                               errors="replace", timeout=timeout)
            tail = (r.stdout + r.stderr).strip().splitlines()
            return r.returncode == 0, (tail[-1][:120] if tail else "")
        except subprocess.TimeoutExpired:
            return False, "check timed out"
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def validate(task: dict) -> tuple[bool, str]:
    """Self-validation gate. Returns (admitted, reason)."""
    name = _sanitize_name(task.get("name", ""))
    if not name:
        return False, "bad/empty name"
    check = task.get("check") or ""
    seed = {k: str(v) for k, v in (task.get("seed") or {}).items()}
    reference = {k: str(v) for k, v in (task.get("reference") or {}).items()}
    if not check.strip() or not reference:
        return False, "missing check or reference"
    # 1. reference passes — run twice to catch non-determinism/flakiness.
    ok1, why1 = _run_check(check, reference)
    if not ok1:
        return False, f"reference does not pass check: {why1}"
    ok2, _ = _run_check(check, reference)
    if not ok2:
        return False, "check is non-deterministic (reference passed then failed)"
    # 2. starting state (seed only) must fail — otherwise the task is already solved.
    oks, _ = _run_check(check, seed)
    if oks:
        return False, "seed already passes the check (task is trivial)"
    return True, name


def admit(task: dict, name: str, overwrite: bool = False) -> Path:
    """Write a validated task into bench/tasks/<name>/."""
    dest = BENCH_TASKS / name
    if dest.exists() and not overwrite:
        raise FileExistsError(f"task {name} already exists")
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "prompt.txt").write_text(str(task["prompt"]).rstrip() + "\n", encoding="utf-8")
    (dest / "check.py").write_text(str(task["check"]), encoding="utf-8")
    for rel, content in (task.get("seed") or {}).items():
        p = dest / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(str(content), encoding="utf-8")
    return dest


def generate(n: int, backend_name: str = "", timeout: int = 240,
             overwrite: bool = False) -> list[str]:
    """Generate up to n self-validated tasks; return admitted task names."""
    panel = teacher.build_panel()
    if backend_name:
        panel = [b for b in panel if b.name == backend_name]
    if not panel:
        print("[taskgen] no teacher backend available — aborting.")
        return []
    admitted: list[str] = []
    for i in range(n):
        backend = panel[i % len(panel)]
        dim = DIMENSIONS[i % len(DIMENSIONS)]
        print(f"[taskgen] {i + 1}/{n} via {backend.name}: {dim[:48]}…")
        resp = backend.propose(GEN_PROMPT.format(dimension=dim), timeout=timeout)
        task = _parse(resp or "")
        if not task:
            print("[taskgen]   rejected: unparseable / missing keys")
            continue
        ok, reason = validate(task)
        if not ok:
            print(f"[taskgen]   rejected: {reason}")
            continue
        name = reason
        try:
            dest = admit(task, name, overwrite)
        except FileExistsError:
            print(f"[taskgen]   skip: {name} already exists")
            continue
        admitted.append(name)
        print(f"[taskgen]   ADMITTED {name} -> {dest}")
    print(f"[taskgen] admitted {len(admitted)}/{n}: {admitted}")
    return admitted


if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser(description="Generate self-validated bench tasks.")
    ap.add_argument("-n", type=int, default=3, help="how many to attempt")
    ap.add_argument("--teacher", default="", help="restrict to one backend (codex|claude|api)")
    ap.add_argument("--timeout", type=int, default=240)
    ap.add_argument("--overwrite", action="store_true")
    a = ap.parse_args()
    generate(a.n, a.teacher, a.timeout, a.overwrite)
