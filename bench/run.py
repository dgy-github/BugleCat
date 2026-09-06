#!/usr/bin/env python3
"""Three-way coding-agent benchmark: nanocodex / opencode / Claude Code.

Each task is a dir under tasks/ with prompt.txt (given to the agent), optional
seed files, and a hidden check.py (graded AFTER the agent runs; exit 0 = pass).
For each (agent, task): fresh temp workspace, seed it, run the agent on the
prompt in that workspace (with a timeout), copy check.py in, run it, record
pass/fail + wall-clock + whether the agent hit the timeout.

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
import hashlib
import json
import os
import re
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
DEFAULT_POST_EDIT_TIMEOUT = 15
DEFAULT_EMPTY_RESPONSE_RETRIES = 2
TAIL_LINES = 12
TAIL_CHARS = 4000


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


def _file_snapshot(root: Path) -> dict[str, tuple[int, str]]:
    """Return a stable relative-file manifest used for post-edit evidence."""
    manifest: dict[str, tuple[int, str]] = {}
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        digest = hashlib.sha256()
        try:
            with path.open("rb") as handle:
                for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                    digest.update(chunk)
            manifest[rel] = (path.stat().st_size, digest.hexdigest())
        except OSError:
            # A concurrently removed file is represented as absent; the
            # resulting deleted_files evidence is more useful than aborting.
            continue
    return manifest


def _expected_files(prompt: str) -> list[str]:
    """Extract Python paths explicitly mentioned by a benchmark prompt."""
    found: set[str] = set()
    for token in re.findall(r"(?<![A-Za-z0-9_.-])([A-Za-z0-9_./\\-]+\.py)(?![A-Za-z0-9_.-])", prompt):
        normalized = Path(token.replace("\\", "/")).as_posix()
        if not Path(normalized).is_absolute() and normalized != ".py":
            found.add(normalized)
    return sorted(found)


def _tail(text: str) -> list[str]:
    """Sanitize and bound process output before persisting it in reports."""
    if isinstance(text, bytes):
        text = text.decode("utf-8", errors="replace")
    elif not isinstance(text, str):
        text = str(text)
    text = re.sub(r"(?i)(api[_-]?key|apikey|token)\s*[=:]\s*[^\s,;]+", r"\1=[REDACTED]", text)
    text = re.sub(r"(?i)Bearer\s+[^\s]+", "Bearer [REDACTED]", text)
    text = re.sub(r"sk-[A-Za-z0-9_-]+", "sk-[REDACTED]", text)
    lines = text.splitlines()[-TAIL_LINES:]
    joined = "\n".join(lines)
    if len(joined) > TAIL_CHARS:
        joined = joined[-TAIL_CHARS:]
        lines = joined.splitlines()
    return lines


def _post_edit_checks(ws: Path, baseline: dict[str, tuple[int, str]],
                      expected: list[str], timeout: int) -> dict:
    timeout = max(1, timeout)
    after = _file_snapshot(ws)
    before_names, after_names = set(baseline), set(after)
    created = sorted(after_names - before_names)
    deleted = sorted(before_names - after_names)
    modified = sorted(name for name in before_names & after_names if baseline[name] != after[name])
    missing = sorted(name for name in expected if name not in after_names)
    python_files = sorted({name for name in created + modified + expected
                           if name.endswith(".py") and name in after_names})
    compiles = []
    for name in python_files:
        try:
            result = subprocess.run(
                [sys.executable, "-m", "py_compile", name], cwd=ws,
                capture_output=True, text=True, encoding="utf-8", errors="replace",
                timeout=timeout,
            )
            compiles.append({"file": name, "ok": result.returncode == 0,
                             "returncode": result.returncode,
                             "stderr_tail": _tail(result.stderr),
                             "stdout_tail": _tail(result.stdout)})
        except subprocess.TimeoutExpired as exc:
            compiles.append({"file": name, "ok": False, "timed_out": True,
                             "returncode": None, "stderr_tail": _tail(exc.stderr or ""),
                             "stdout_tail": _tail(exc.stdout or "")})
    return {"created_files": created, "modified_files": modified,
            "deleted_files": deleted, "expected_files": expected,
            "missing_expected_files": missing, "python_compile": compiles}


def _failure_kind(evidence: dict) -> str | None:
    if evidence.get("missing_expected_files"):
        return "missing_expected_file"
    if any(not item.get("ok") for item in evidence.get("python_compile", [])):
        return "compile_failed"
    output = "\n".join(evidence.get("stdout_tail", []) + evidence.get("stderr_tail", []))
    if "空内容" in output or "empty response" in output.lower():
        return "empty_response"
    if evidence.get("timed_out"):
        return "timeout"
    if not evidence.get("passed"):
        return "grader_failed"
    return None


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


def _run_once_detailed(
    agent: str, task: Path, timeout: int, post_edit_timeout: int = DEFAULT_POST_EDIT_TIMEOUT,
) -> dict:
    """Run one attempt and return pass/failure plus structured evidence.

    Raises FileNotFoundError if the agent binary is missing (caller skips agent).
    """
    prompt = (task / "prompt.txt").read_text(encoding="utf-8")
    ws = Path(tempfile.mkdtemp(prefix=f"bench_{agent}_{task.name}_"))
    try:
        seed(task, ws)
        baseline = _file_snapshot(ws)
        expected = _expected_files(prompt)
        t0 = time.perf_counter()
        timed_out = False
        returncode = None
        stdout = stderr = ""
        try:
            result = subprocess.run(agent_cmd(agent, prompt), cwd=str(ws),
                                    capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
            returncode = result.returncode
            stdout, stderr = result.stdout or "", result.stderr or ""
        except subprocess.TimeoutExpired as exc:
            timed_out = True
            stdout = exc.stdout or ""
            stderr = exc.stderr or ""
        elapsed = time.perf_counter() - t0
        post_edit = _post_edit_checks(ws, baseline, expected, post_edit_timeout)
        ok, log = grade(task, ws)
        post_edit_failed = bool(post_edit["missing_expected_files"]) or any(
            not item.get("ok") for item in post_edit["python_compile"]
        )
        if post_edit_failed:
            ok = False
            if not log:
                log = "post-edit verification failed"
        tail = "" if ok else (log.splitlines()[-1][:200] if log else "no output")
        evidence = {
            "task": task.name, "agent": agent, "passed": ok,
            "timed_out": timed_out, "elapsed_s": round(elapsed, 1),
            "returncode": returncode, "grader_note": tail,
            "stdout_tail": _tail(stdout), "stderr_tail": _tail(stderr),
            **post_edit,
        }
        evidence["failure_kind"] = _failure_kind(evidence)
        return evidence
    finally:
        shutil.rmtree(ws, ignore_errors=True)


def run_once(agent: str, task: Path, timeout: int) -> tuple[bool, float, str]:
    """Compatibility wrapper returning the historical three-value result."""
    evidence = _run_once_detailed(agent, task, timeout)
    return evidence["passed"], evidence["elapsed_s"], evidence["grader_note"]


def run_once_detailed(agent: str, task: Path, timeout: int,
                      post_edit_timeout: int = DEFAULT_POST_EDIT_TIMEOUT) -> dict:
    """Public structured attempt API for the in-repository evals package."""
    return _run_once_detailed(agent, task, timeout, post_edit_timeout)


def run_agent(agent: str, timeout: int, repeats: int, filt: str = "",
              post_edit_timeout: int = DEFAULT_POST_EDIT_TIMEOUT,
              empty_response_retries: int = DEFAULT_EMPTY_RESPONSE_RETRIES) -> dict:
    """Return per-task pass, timing, timeout, and failure-note evidence."""
    print(f"\n=== {agent}  (x{repeats}) ===")
    results: dict[str, dict] = {}
    for task in tasks(filt):
        passes = 0
        times: list[float] = []
        note = ""
        timeouts = 0
        attempts = []
        for attempt_no in range(1, repeats + 1):
            try:
                evidence = _run_once_detailed(agent, task, timeout, post_edit_timeout)
            except FileNotFoundError:
                print(f"  {agent} not installed — skipping the rest")
                return results
            retry_no = 0
            while evidence.get("failure_kind") == "empty_response" and retry_no < empty_response_retries:
                retry_no += 1
                evidence = _run_once_detailed(agent, task, timeout, post_edit_timeout)
            evidence["attempt"] = attempt_no
            evidence["empty_response_retries"] = retry_no
            attempts.append(evidence)
            passes += int(evidence["passed"])
            times.append(evidence["elapsed_s"])
            timeouts += int(evidence["timed_out"])
            if not evidence["passed"] and not note:
                note = evidence["grader_note"]
        mean_s = round(sum(times) / len(times), 1) if times else 0.0
        rate = f"{passes}/{repeats}"
        details = []
        if note:
            details.append(note)
        if timeouts:
            details.append(f"timeouts={timeouts}")
        tail = f"   ({'; '.join(details)})" if details else ""
        print(f"  {task.name:14} {rate:>5}  {mean_s:6.1f}s{tail}")
        results[task.name] = {
            "passes": passes,
            "runs": repeats,
            "mean_s": mean_s,
            "timeouts": timeouts,
            "note": note,
            "attempts": attempts,
        }
    tot_p = sum(r["passes"] for r in results.values())
    tot_n = sum(r["runs"] for r in results.values())
    total_timeouts = sum(r["timeouts"] for r in results.values())
    solved = sum(1 for r in results.values() if r["passes"] == r["runs"])
    print(
        f"  -> {agent}: {tot_p}/{tot_n} runs passed; "
        f"{solved}/{len(results)} tasks fully solved; {total_timeouts} timed out"
    )
    return results


def write_report(summary: dict, repeats: int, timeout: int, post_edit_timeout: int = DEFAULT_POST_EDIT_TIMEOUT) -> Path:
    REPORTS.mkdir(exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    task_names = sorted({t for r in summary.values() for t in r})

    lines = [f"# nanocodex bench report — {stamp}", ""]
    lines.append(f"- repeats per task: **{repeats}**, per-task timeout: **{timeout}s**, post-edit checks: **{post_edit_timeout}s**")
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
            if not r:
                cells.append("—")
                continue
            cell = f"{r['passes']}/{r['runs']}"
            if r.get("timeouts", 0):
                cell += f" (timeout {r['timeouts']})"
            cells.append(cell)
        lines.append(f"| {t} | " + " | ".join(cells) + " |")
    lines.append("")
    # Per-agent totals.
    lines += ["## Totals", ""]
    lines.append("| agent | runs passed | tasks solved | timed out | mean s/run |")
    lines.append("|-------|-------------|--------------|-----------|------------|")
    for a, res in summary.items():
        tot_p = sum(r["passes"] for r in res.values())
        tot_n = sum(r["runs"] for r in res.values())
        total_timeouts = sum(r.get("timeouts", 0) for r in res.values())
        solved = sum(1 for r in res.values() if r["passes"] == r["runs"])
        all_times = [r["mean_s"] for r in res.values() if r["mean_s"]]
        mean = round(sum(all_times) / len(all_times), 1) if all_times else 0.0
        lines.append(
            f"| {a} | {tot_p}/{tot_n} | {solved}/{len(res)} | "
            f"{total_timeouts} | {mean} |"
        )

    trace = REPORTS / f"trace_{stamp}.jsonl"
    with trace.open("w", encoding="utf-8") as handle:
        for agent, results in summary.items():
            for task, result in results.items():
                for attempt in result.get("attempts", []):
                    handle.write(json.dumps(attempt, ensure_ascii=False) + "\n")
    failures = [
        (agent, task, attempt) for agent, results in summary.items()
        for task, result in results.items() for attempt in result.get("attempts", [])
        if not attempt.get("passed")
    ]
    lines += ["", "## Post-edit evidence", "", f"- structured trace: `{trace.name}`"]
    if failures:
        lines.append(f"- failed attempts: **{len(failures)}** (see trace for file/compile/output details)")
        for agent, task, attempt in failures[:10]:
            missing = ", ".join(attempt.get("missing_expected_files", [])) or "none"
            compile_failures = [c["file"] for c in attempt.get("python_compile", []) if not c.get("ok")]
            lines.append(f"- `{agent}/{task}` attempt {attempt.get('attempt', '?')}: {attempt.get('grader_note', 'failed')}; missing={missing}; compile_failures={', '.join(compile_failures) or 'none'}")

    md = REPORTS / f"report_{stamp}.md"
    md.write_text("\n".join(lines) + "\n", encoding="utf-8")
    js = REPORTS / f"report_{stamp}.json"
    js.write_text(json.dumps(
        {"stamp": stamp, "repeats": repeats, "timeout": timeout,
         "post_edit_timeout": post_edit_timeout, "trace": str(trace), "results": summary},
        indent=2), encoding="utf-8")
    return md


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--agent", default="nanocodex",
                    help="nanocodex | nanocodex-orch | opencode | claude | all")
    ap.add_argument("--timeout", type=int, default=240, help="per-task seconds")
    ap.add_argument("--post-edit-timeout", type=int, default=DEFAULT_POST_EDIT_TIMEOUT,
                    help="seconds per structured post-edit Python check")
    ap.add_argument("--empty-response-retries", type=int, default=DEFAULT_EMPTY_RESPONSE_RETRIES,
                    help="quick whole-attempt retries after a detected empty model response")
    ap.add_argument("--repeats", type=int, default=3, help="runs per (agent, task)")
    ap.add_argument("--no-report", action="store_true", help="skip writing bench/reports/")
    ap.add_argument("--tasks", default="", help="comma-separated name filters (e.g. t5,t6)")
    args = ap.parse_args()
    agents = ALL_AGENTS if args.agent == "all" else [args.agent]

    summary: dict[str, dict] = {}
    for a in agents:
        res = run_agent(a, args.timeout, max(1, args.repeats), args.tasks, args.post_edit_timeout,
                        max(0, args.empty_response_retries))
        if res:  # skip agents that weren't installed
            summary[a] = res

    print("\n==== SUMMARY ====")
    for a, res in summary.items():
        tot_p = sum(r["passes"] for r in res.values())
        tot_n = sum(r["runs"] for r in res.values())
        total_timeouts = sum(r.get("timeouts", 0) for r in res.values())
        solved = sum(1 for r in res.values() if r["passes"] == r["runs"])
        print(
            f"  {a:16} {tot_p}/{tot_n} runs   {solved}/{len(res)} tasks   "
            f"{total_timeouts} timed out"
        )

    if summary and not args.no_report:
        path = write_report(summary, max(1, args.repeats), args.timeout, args.post_edit_timeout)
        print(f"\nreport written: {path}")


if __name__ == "__main__":
    main()
