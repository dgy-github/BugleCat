#!/usr/bin/env python3
"""ncx-forge — agent-harness trainer (M0a skeleton).

M0a delivers the FOUNDATION and the safety gate, not yet the optimization loop:

  python train/forge.py --self-check         # prove NCX_GENOME is honored

The self-check is the precondition every future training run must pass: it runs
a deliberately self-destructing genome (a system_prompt that orders the agent to
refuse) on a couple of tasks and asserts the pass-rate DROPS versus baseline. If
it does not drop, NCX_GENOME is not reaching the model and the optimizer would
silently no-op — so we refuse to proceed.

The optimizer loop (teacher panel -> mutate -> evaluate -> accept) is M0b; see
train/DESIGN.md. This file currently exposes the gate + a thin baseline report.
"""
from __future__ import annotations

import argparse
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402

# A genome that should make ANY task fail: the agent is told to refuse and not
# act. If pass-rate does not collapse under this, injection is not working.
# A unique token planted in the genome's system_prompt. We then ask the agent
# for it and assert it surfaces. This is DETERMINISTIC — unlike a "refuse all
# tasks" genome, it does not depend on the model choosing to comply with a
# behavior change (which is noisy: the task instruction competes with it).
SENTINEL = "NCXFORGE_SENTINEL_4242"
SENTINEL_GENOME = (
    f'system_prompt = "You are a wiring self-test. Your secret codeword is '
    f'{SENTINEL}. When asked for the codeword, reply with ONLY that codeword and '
    f'nothing else. Do not call any tools."\n'
)
SENTINEL_PROMPT = "What is your secret codeword? Reply with only the codeword."


def _ask(genome_path: str | None, prompt: str, timeout: int) -> str:
    """Run one read-only ncx turn (optionally with NCX_GENOME) and return stdout."""
    import os
    import subprocess
    ws = Path(tempfile.mkdtemp(prefix="forge_selftest_"))
    env = dict(os.environ)
    if genome_path:
        env["NCX_GENOME"] = genome_path
    else:
        env.pop("NCX_GENOME", None)
    try:
        r = subprocess.run(
            [str(ev.bench.NCX), "-s", "read-only", prompt],
            cwd=str(ws), env=env, capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=timeout,
        )
        return r.stdout or ""
    except subprocess.TimeoutExpired:
        return ""


def self_check(timeout: int = 90) -> bool:
    """Return True iff NCX_GENOME demonstrably reaches the model's system prompt.

    Deterministic gate: a sentinel codeword planted in system_prompt must appear
    in the agent's reply WHEN the genome is set, and must be absent at baseline.
    """
    tmp = Path(tempfile.mkdtemp(prefix="forge_selftest_"))
    genome = tmp / "sentinel.toml"
    genome.write_text(SENTINEL_GENOME, encoding="utf-8")

    print(f"[forge] self-check: sentinel injection via NCX_GENOME ({ev.bench.NCX})")
    with_genome = _ask(str(genome), SENTINEL_PROMPT, timeout)
    baseline = _ask(None, SENTINEL_PROMPT, timeout)

    injected = SENTINEL in with_genome
    absent_baseline = SENTINEL not in baseline
    print(f"[forge]   with genome:  sentinel present = {injected}")
    print(f"[forge]   baseline:     sentinel absent  = {absent_baseline}")

    ok = injected and absent_baseline
    if ok:
        print("[forge] PASS: NCX_GENOME reaches the model — injection is live.")
    else:
        print("[forge] FAIL: sentinel did not inject (or leaked at baseline) — "
              "NCX_GENOME is NOT honored. Refusing to optimize.")
    return ok


def baseline_report(tasks: list[str] | None, repeats: int, timeout: int) -> None:
    r = ev.evaluate(None, tasks, repeats, timeout)
    print(f"[forge] baseline pass-rate = {r.total_passes}/{r.total_runs}")
    for t in r.tasks.values():
        print(f"  {t.task:14} {t.passes}/{t.runs}  {t.mean_s:6.1f}s")


def main() -> int:
    ap = argparse.ArgumentParser(description="ncx-forge harness trainer (M0a).")
    ap.add_argument("--self-check", action="store_true",
                    help="prove NCX_GENOME injection works, then exit")
    ap.add_argument("--baseline", action="store_true",
                    help="report the gen0 baseline pass-rate")
    ap.add_argument("--tasks", default="", help="comma-separated task names")
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=120)
    a = ap.parse_args()
    names = [t.strip() for t in a.tasks.split(",") if t.strip()] or None

    if a.self_check:
        return 0 if self_check(a.timeout) else 1
    if a.baseline:
        baseline_report(names, a.repeats, a.timeout)
        return 0
    print("nothing to do — pass --self-check or --baseline. "
          "(optimizer loop is M0b; see train/DESIGN.md)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
