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
import json
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
import genome as G  # noqa: E402
import splits as S  # noqa: E402
import teacher as T  # noqa: E402

GENOMES_DIR = Path(__file__).resolve().parent / "genomes"
RUNS_DIR = Path(__file__).resolve().parent / "runs"

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


def train(rounds: int, train_tasks: list[str], holdout_tasks: list[str],
          repeats: int, timeout: int, budget_s: float, teachers: str,
          stamp: str, test_tasks: list[str] | None = None,
          accept_margin: int = 1, reeval_incumbent: bool = True) -> dict:
    """Single-champion hill-climb. Each round: every available teacher proposes a
    mutation of the current champion; candidates are evaluated on TRAIN; the best
    is promoted iff it beats the champion on TRAIN by `accept_margin` AND does not
    regress on the held-out (val) split.

    Noise-aware (M1): because the agent is non-deterministic, the incumbent is
    RE-EVALUATED each round under a fresh draw (when `reeval_incumbent`), so a
    candidate is compared against the champion's current noisy score rather than a
    single stale gen0 number — and must clear it by `accept_margin`, not just +0.

    A wall-clock governor bounds total spend. Returns a lineage dict (also written
    to runs/lineage_<stamp>.json). `test_tasks` (if given) are scored ONCE at the
    end for an unbiased final number — never used for acceptance.
    """
    GENOMES_DIR.mkdir(exist_ok=True)
    RUNS_DIR.mkdir(exist_ok=True)
    t0 = time.perf_counter()

    def elapsed() -> float:
        return time.perf_counter() - t0

    panel = [b for b in T.build_panel() if teachers == "panel" or b.name == teachers]
    if not panel:
        print(f"[forge] no teacher backend matches '{teachers}' — aborting.")
        return {"error": "no teacher"}

    baseline = G.extract_current()
    champion = baseline.copy()
    champ_path = GENOMES_DIR / f"{stamp}_gen0_baseline.toml"
    champion.save(champ_path)
    champ_train = ev.evaluate(str(champ_path), train_tasks, repeats, timeout)
    champ_hold = ev.evaluate(str(champ_path), holdout_tasks, repeats, timeout)
    print(f"[forge] gen0 baseline: train {champ_train.total_passes}/{champ_train.total_runs}, "
          f"holdout {champ_hold.total_passes}/{champ_hold.total_runs}")

    lineage = {"stamp": stamp, "train_tasks": train_tasks, "holdout_tasks": holdout_tasks,
               "repeats": repeats, "rounds": [],
               "gen0": {"train": champ_train.total_passes, "holdout": champ_hold.total_passes}}

    for rnd in range(1, rounds + 1):
        if elapsed() > budget_s:
            print(f"[forge] budget ({budget_s}s) exhausted — stopping at round {rnd}.")
            break
        # Noise-aware: re-score the incumbent this round so the candidate is
        # compared against a fresh draw, not a stale gen0 number.
        if reeval_incumbent and rnd > 1:
            champ_train = ev.evaluate(str(champ_path), train_tasks, repeats, timeout)
            champ_hold = ev.evaluate(str(champ_path), holdout_tasks, repeats, timeout)
            print(f"[forge]   round {rnd}: incumbent re-eval train "
                  f"{champ_train.total_passes}/{champ_train.total_runs}, "
                  f"holdout {champ_hold.total_passes}/{champ_hold.total_runs}")
        failures = champ_train.failing_trajectories(top_k=3)
        if not failures:
            print("[forge] champion fully solves the train set — nothing to improve "
                  "(grow the task set, M1). Stopping.")
            break
        prompt = T.build_teacher_prompt(champion, failures)
        round_log = {"round": rnd, "candidates": []}
        candidates = []  # (teacher_name, genome, eval)
        for backend in panel:
            if elapsed() > budget_s:
                break
            resp = backend.propose(prompt)
            cand, why = T.parse_candidate(resp or "", champion)
            if not cand:
                print(f"[forge]   round {rnd} {backend.name}: rejected ({why})")
                round_log["candidates"].append({"teacher": backend.name, "status": why})
                continue
            errs = G.validate(cand, baseline)
            if errs:
                print(f"[forge]   round {rnd} {backend.name}: invalid ({errs[0]})")
                round_log["candidates"].append({"teacher": backend.name, "status": f"invalid: {errs[0]}"})
                continue
            cpath = GENOMES_DIR / f"{stamp}_gen{rnd}_{backend.name}.toml"
            cand.save(cpath)
            cev = ev.evaluate(str(cpath), train_tasks, repeats, timeout)
            print(f"[forge]   round {rnd} {backend.name}: train "
                  f"{cev.total_passes}/{cev.total_runs}  changed[{G.diff(champion, cand)!r}]")
            candidates.append((backend.name, cand, cev, cpath))
            round_log["candidates"].append({
                "teacher": backend.name, "status": "evaluated",
                "train_passes": cev.total_passes, "path": str(cpath),
            })

        # Pick the best candidate; accept only if it beats champion on TRAIN and
        # does not regress on HELD-OUT (the real anti-overfit gate).
        accepted = None
        if candidates:
            best = max(candidates, key=lambda c: c[2].total_passes)
            tname, cand, cev, cpath = best
            margin = cev.total_passes - champ_train.total_passes
            if margin >= accept_margin:
                chold = ev.evaluate(str(cpath), holdout_tasks, repeats, timeout)
                if chold.total_passes >= champ_hold.total_passes:
                    champion, champ_train, champ_hold, champ_path = cand, cev, chold, cpath
                    accepted = tname
                    print(f"[forge]   round {rnd}: ACCEPT {tname} "
                          f"(train +{margin} -> {cev.total_passes}/{cev.total_runs}, "
                          f"holdout {chold.total_passes}/{chold.total_runs})")
                    round_log["accept"] = {"teacher": tname, "train": cev.total_passes,
                                           "holdout": chold.total_passes, "margin": margin}
                else:
                    print(f"[forge]   round {rnd}: REJECT {tname} (holdout regressed "
                          f"{chold.total_passes} < {champ_hold.total_passes})")
                    round_log["accept"] = {"teacher": tname, "status": "holdout-regressed"}
            else:
                print(f"[forge]   round {rnd}: no candidate cleared the margin "
                      f"(best +{margin} < {accept_margin} needed on train).")
        if accepted is None and "accept" not in round_log:
            round_log.setdefault("accept", {"status": "none-cleared-margin"})
        lineage["rounds"].append(round_log)

    champ_final = GENOMES_DIR / f"{stamp}_champion.toml"
    champion.save(champ_final)
    lineage["champion"] = {"path": str(champ_final),
                           "train": champ_train.total_passes, "holdout": champ_hold.total_passes,
                           "diff_vs_baseline": G.diff(baseline, champion)}

    # Final, unbiased number: score baseline vs champion ONCE on the frozen test
    # split (never used for acceptance). The only honest "did training help?".
    if test_tasks:
        base_test = ev.evaluate(str(GENOMES_DIR / f"{stamp}_gen0_baseline.toml"),
                                test_tasks, repeats, timeout)
        champ_test = ev.evaluate(str(champ_final), test_tasks, repeats, timeout)
        print(f"[forge] FINAL on test {test_tasks}: baseline "
              f"{base_test.total_passes}/{base_test.total_runs} -> champion "
              f"{champ_test.total_passes}/{champ_test.total_runs}")
        lineage["test"] = {"tasks": test_tasks,
                           "baseline": base_test.total_passes,
                           "champion": champ_test.total_passes,
                           "runs": base_test.total_runs}
    (RUNS_DIR / f"lineage_{stamp}.json").write_text(json.dumps(lineage, indent=2), encoding="utf-8")
    print(f"[forge] done in {elapsed():.0f}s. champion -> {champ_final}")
    print(f"[forge] champion vs baseline:\n{G.diff(baseline, champion)}")
    return lineage


def main() -> int:
    ap = argparse.ArgumentParser(description="ncx-forge harness trainer (M0b).")
    ap.add_argument("--self-check", action="store_true",
                    help="prove NCX_GENOME injection works, then exit")
    ap.add_argument("--baseline", action="store_true",
                    help="report the gen0 baseline pass-rate")
    ap.add_argument("--train", action="store_true",
                    help="run the optimizer loop (self-check gates it)")
    ap.add_argument("--tasks", default="", help="comma-separated task names (for --baseline)")
    ap.add_argument("--train-tasks", default="",
                    help="override train split (comma-separated); default = splits.json train")
    ap.add_argument("--holdout-tasks", default="",
                    help="override val/holdout split; default = splits.json val")
    ap.add_argument("--test-tasks", default="",
                    help="override test split; default = splits.json test (scored once at end)")
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--budget-s", type=float, default=1800.0,
                    help="wall-clock governor (seconds); stops cleanly when exceeded")
    ap.add_argument("--teacher", default="panel",
                    help="panel | codex | claude | api")
    ap.add_argument("--accept-margin", type=int, default=1,
                    help="train passes a candidate must clear the incumbent by (noise band)")
    ap.add_argument("--no-reeval", action="store_true",
                    help="do NOT re-evaluate the incumbent each round (cheaper, noisier)")
    ap.add_argument("--no-gate", action="store_true",
                    help="skip the self-check gate before --train (NOT recommended)")
    a = ap.parse_args()
    names = [t.strip() for t in a.tasks.split(",") if t.strip()] or None

    if a.self_check:
        return 0 if self_check(a.timeout) else 1
    if a.baseline:
        baseline_report(names, a.repeats, a.timeout)
        return 0
    if a.train:
        if not a.no_gate and not self_check(a.timeout):
            print("[forge] self-check failed — refusing to train (use --no-gate to override).")
            return 1
        stamp = time.strftime("%Y%m%d_%H%M%S")
        sp = S.load_splits()
        train_tasks = [t.strip() for t in a.train_tasks.split(",") if t.strip()] or sp["train"]
        holdout_tasks = [t.strip() for t in a.holdout_tasks.split(",") if t.strip()] or sp["val"]
        test_tasks = [t.strip() for t in a.test_tasks.split(",") if t.strip()] or sp["test"]
        print(f"[forge] splits — train={train_tasks} val={holdout_tasks} test={test_tasks}")
        train(a.rounds, train_tasks, holdout_tasks, a.repeats, a.timeout,
              a.budget_s, a.teacher, stamp, test_tasks=test_tasks,
              accept_margin=a.accept_margin, reeval_incumbent=not a.no_reeval)
        return 0
    print("nothing to do — pass --self-check | --baseline | --train. See train/DESIGN.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
