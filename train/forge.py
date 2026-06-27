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
import pareto as PA  # noqa: E402
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
          accept_margin: int = 1, reeval_incumbent: bool = True,
          from_genome: str | None = None) -> dict:
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

    # `baseline` is ALWAYS the real default genome — it defines the validation
    # caps and the legal tool set. The starting `champion` is normally the same,
    # but `--from-genome` lets a run start from a DEGRADED scaffold to create
    # real headroom (an honest capability test: can the optimizer recover it?).
    baseline = G.extract_current()
    if from_genome:
        champion = G.Genome.load(Path(from_genome))
        print(f"[forge] starting from supplied genome {from_genome} "
              f"(diff vs default: {G.diff(baseline, champion)})")
    else:
        champion = baseline.copy()
    champ_path = GENOMES_DIR / f"{stamp}_gen0_start.toml"
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
            if champ_train.total_passes >= champ_train.total_runs:
                print("[forge] champion fully solves the train set — nothing to improve "
                      "(grow/harden the task set). Stopping.")
            else:
                # Should not happen now that the evaluator synthesizes a trajectory
                # for trajectory-less failures, but guard against a silent no-op.
                print(f"[forge] train has failures ({champ_train.total_passes}/"
                      f"{champ_train.total_runs}) but no usable trajectories to feed the "
                      f"teacher — cannot propose. Stopping.")
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
        base_test = ev.evaluate(str(GENOMES_DIR / f"{stamp}_gen0_start.toml"),
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


def _objectives(ev_result) -> PA.Objectives:
    """Map an EvalResult to (pass-rate ↑, cost ↓).

    Cost = mean total TOKENS per task when ncx reported usage (the real cost);
    otherwise mean per-task SECONDS (a latency proxy). ncx prints a
    `[ncx-usage]` line in one-shot mode, which the evaluator parses.

    An EMPTY eval (no tasks/runs) is mapped to the WORST objective (cost=+inf),
    never the best — otherwise a zero-task misconfiguration would be undominated
    and silently win the front (masking the misconfig as a green champion)."""
    if ev_result.total_runs == 0 or not ev_result.tasks:
        return PA.Objectives(passrate=0.0, cost=float("inf"))
    pr = ev_result.total_passes / ev_result.total_runs
    tokens = ev_result.mean_tokens
    if tokens > 0:
        cost = tokens                     # real token cost
    else:
        times = [t.mean_s for t in ev_result.tasks.values()]
        cost = round(sum(times) / len(times), 1)  # latency fallback
    return PA.Objectives(passrate=pr, cost=cost)


def evolve(rounds: int, train_tasks: list[str], holdout_tasks: list[str],
           repeats: int, timeout: int, budget_s: float, teachers: str, stamp: str,
           pop_cap: int = 4, test_tasks: list[str] | None = None,
           from_genome: str | None = None, reeval_parents: bool = True,
           model: str | None = None) -> dict:
    """Small-population, multi-objective (Pareto) search (M2).

    Maintains a population that is the Pareto front (pass-rate ↑ vs cost ↓),
    capped to `pop_cap` by crowding distance. Each generation, every population
    member is mutated by every available teacher; parents+children are reduced to
    the next front. Unlike single-champion `train()`, this KEEPS trade-offs (a
    cheap-decent genome alongside a slow-strong one). Writes a lineage JSON for
    `viz.py`.

    Noise-aware: with `reeval_parents` (default on), surviving members are
    re-scored under a fresh draw each generation, so a lucky early evaluation
    can't permanently pin the front (mirrors train()'s reeval_incumbent).
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
    counter = {"n": 0}

    def new_id(tag: str) -> str:
        counter["n"] += 1
        return f"g{counter['n']:02d}_{tag}"

    def make_member(genome, gen, parent, teacher, tag):
        gid = "gen0_start" if gen == 0 else new_id(tag)
        path = GENOMES_DIR / f"{stamp}_{gid}.toml"
        genome.save(path)
        ev_r = ev.evaluate(str(path), train_tasks, repeats, timeout, model)
        obj = _objectives(ev_r)
        return {"id": gid, "genome": genome, "path": path, "ev": ev_r, "obj": obj,
                "gen": gen, "parent": parent, "teacher": teacher}

    start_genome = G.Genome.load(Path(from_genome)) if from_genome else baseline.copy()
    if from_genome:
        print(f"[forge] population start from {from_genome}")
    seed_member = make_member(start_genome, 0, None, None, "start")
    if seed_member["ev"].total_runs == 0:
        print(f"[forge] train tasks {train_tasks} resolved to ZERO bench runs — "
              f"check task names / splits.json. Aborting.")
        return {"error": "no train tasks"}
    population = [seed_member]
    nodes = [_node(seed_member)]
    print(f"[forge] gen0: pass {seed_member['obj'].passrate:.2f} cost {seed_member['obj'].cost}s "
          f"(pop_cap={pop_cap}, teachers={[b.name for b in panel]})")

    gens_log = []
    for rnd in range(1, rounds + 1):
        if elapsed() > budget_s:
            print(f"[forge] budget ({budget_s}s) exhausted — stopping before gen {rnd}.")
            break
        # Noise-aware (mirrors train()): re-score surviving members under a fresh
        # draw so a lucky early evaluation can't permanently pin the front.
        if reeval_parents and rnd > 1:
            for m in population:
                m["ev"] = ev.evaluate(str(m["path"]), train_tasks, repeats, timeout, model)
                m["obj"] = _objectives(m["ev"])
            print(f"[forge]   gen{rnd}: re-eval front "
                  f"{[(m['id'], round(m['obj'].passrate, 2), m['obj'].cost) for m in population]}")
        children = []
        for parent in population:
            failures = parent["ev"].failing_trajectories(top_k=3)
            if not failures:
                continue  # nothing to learn from this parent this round
            prompt = T.build_teacher_prompt(parent["genome"], failures)
            for backend in panel:
                if elapsed() > budget_s:
                    break
                cand, why = T.parse_candidate(backend.propose(prompt) or "", parent["genome"])
                if not cand:
                    continue
                if G.validate(cand, baseline):
                    continue
                child = make_member(cand, rnd, parent["id"], backend.name, backend.name)
                children.append(child)
                nodes.append(_node(child))
                print(f"[forge]   gen{rnd} {backend.name}: pass {child['obj'].passrate:.2f} "
                      f"cost {child['obj'].cost}s  (parent {parent['id']})")
        combined = population + children
        population = PA.select_population(combined, pop_cap, key=lambda m: m["obj"])
        gens_log.append({"gen": rnd, "evaluated": [c["id"] for c in children],
                         "front": [m["id"] for m in population]})
        print(f"[forge]   gen{rnd}: front = {[(m['id'], round(m['obj'].passrate,2), m['obj'].cost) for m in population]}")
        if not children:
            print("[forge] no new candidates produced — stopping.")
            break

    front_ids = {m["id"] for m in population}
    champ = PA.best(population, key=lambda m: m["obj"])
    for nd in nodes:
        nd["on_front_final"] = nd["id"] in front_ids

    lineage = {"stamp": stamp, "mode": "population", "pop_cap": pop_cap,
               "train_tasks": train_tasks, "holdout_tasks": holdout_tasks,
               "repeats": repeats, "nodes": nodes, "generations": gens_log}

    if champ:
        champ_final = GENOMES_DIR / f"{stamp}_champion.toml"
        champ["genome"].save(champ_final)
        lineage["champion"] = {"id": champ["id"], "passrate": champ["obj"].passrate,
                               "cost": champ["obj"].cost}
        print(f"[forge] champion = {champ['id']} (pass {champ['obj'].passrate:.2f}, cost {champ['obj'].cost}s)")
        print(f"[forge] champion vs baseline:\n{G.diff(baseline, champ['genome'])}")
        if test_tasks:
            base_test = ev.evaluate(str(GENOMES_DIR / f"{stamp}_gen0_start.toml"),
                                    test_tasks, repeats, timeout, model)
            champ_test = ev.evaluate(str(champ_final), test_tasks, repeats, timeout, model)
            print(f"[forge] FINAL on test {test_tasks}: baseline {base_test.total_passes}/"
                  f"{base_test.total_runs} → champion {champ_test.total_passes}/{champ_test.total_runs}")
            lineage["test"] = {"tasks": test_tasks, "baseline": base_test.total_passes,
                               "champion": champ_test.total_passes, "runs": base_test.total_runs}

    lin_path = RUNS_DIR / f"lineage_{stamp}.json"
    lin_path.write_text(json.dumps(lineage, indent=2), encoding="utf-8")
    # Render the visualization next to the lineage.
    try:
        import viz
        html_path = lin_path.with_suffix(".html")
        html_path.write_text(viz.build_html(lineage), encoding="utf-8")
        print(f"[forge] lineage -> {lin_path}\n[forge] viz     -> {html_path}")
    except Exception as e:  # noqa: BLE001
        print(f"[forge] lineage -> {lin_path} (viz failed: {e})")
    print(f"[forge] done in {elapsed():.0f}s.")
    return lineage


def _node(member: dict) -> dict:
    return {"id": member["id"], "gen": member["gen"], "parent": member["parent"],
            "teacher": member["teacher"], "passrate": round(member["obj"].passrate, 3),
            "cost": member["obj"].cost}


def main() -> int:
    ap = argparse.ArgumentParser(description="ncx-forge harness trainer (M0b).")
    ap.add_argument("--self-check", action="store_true",
                    help="prove NCX_GENOME injection works, then exit")
    ap.add_argument("--baseline", action="store_true",
                    help="report the gen0 baseline pass-rate")
    ap.add_argument("--train", action="store_true",
                    help="single-champion hill-climb optimizer (self-check gates it)")
    ap.add_argument("--population", action="store_true",
                    help="multi-objective Pareto population search (M2; self-check gates it)")
    ap.add_argument("--pop-cap", type=int, default=4, help="population size cap (--population)")
    ap.add_argument("--base-model", default="",
                    help="override the AGENT's base model (e.g. a weaker one with more "
                         "harness headroom); applies to --population")
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
    ap.add_argument("--from-genome", default="",
                    help="start the champion from this genome (e.g. a degraded scaffold) "
                         "instead of the real default — a capability test for the optimizer")
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
              accept_margin=a.accept_margin, reeval_incumbent=not a.no_reeval,
              from_genome=(a.from_genome or None))
        return 0
    if a.population:
        if not a.no_gate and not self_check(a.timeout):
            print("[forge] self-check failed — refusing to train (use --no-gate to override).")
            return 1
        stamp = time.strftime("%Y%m%d_%H%M%S")
        sp = S.load_splits()
        train_tasks = [t.strip() for t in a.train_tasks.split(",") if t.strip()] or sp["train"]
        holdout_tasks = [t.strip() for t in a.holdout_tasks.split(",") if t.strip()] or sp["val"]
        test_tasks = [t.strip() for t in a.test_tasks.split(",") if t.strip()] or sp["test"]
        print(f"[forge] splits — train={train_tasks} val={holdout_tasks} test={test_tasks}")
        evolve(a.rounds, train_tasks, holdout_tasks, a.repeats, a.timeout,
               a.budget_s, a.teacher, stamp, pop_cap=a.pop_cap, test_tasks=test_tasks,
               from_genome=(a.from_genome or None), reeval_parents=not a.no_reeval,
               model=(a.base_model or None))
        return 0
    print("nothing to do — pass --self-check | --baseline | --train | --population. See train/DESIGN.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
