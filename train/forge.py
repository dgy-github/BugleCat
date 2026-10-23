#!/usr/bin/env python3
"""ncx-forge — agent-harness trainer (M0a/M0b/M2 + P1 RRSI Regularization).

M0a delivers the FOUNDATION and the safety gate:

  python train/forge.py --self-check         # prove NCX_GENOME is honored

P1 adds RRSI-inspired regularization to the optimization loop:
- Experiment ledger (`runs/ledger_<stamp>.jsonl`) + falsified hypothesis history
- Annealed edit budget (`--initial-max-surfaces`, `--max-changed-lines`)
- Anti-memorization critic gate (`teacher.critic_check`)
- Noise-calibrated token-cost acceptance gate (`--delta-tokens`, `--max-token-growth`)
- Stagnation surface steering (`--stagnation-limit`)
- Post-search structural ablation pruning (`--prune`, `prune_genome`)
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# Windows consoles / redirected stdout default to cp1252, which can't encode
# chars like '->' arrows and crashes mid-run. Force UTF-8 so progress prints
# never abort a long training run.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
import genome  # noqa: E402
import pareto  # noqa: E402
import splits  # noqa: E402
import teacher  # noqa: E402
from process_control import run_owned  # noqa: E402

GENOMES_DIR = Path(os.environ.get(
    "NCX_FORGE_GENOMES_DIR", Path(__file__).resolve().parent / "genomes"
)).resolve()
RUNS_DIR = Path(os.environ.get(
    "NCX_FORGE_RUNS_DIR", Path(__file__).resolve().parent / "runs"
)).resolve()

SENTINEL = "NCXFORGE_SENTINEL_4242"
SENTINEL_GENOME = (
    f'system_prompt = "You are a wiring self-test. Your secret codeword is '
    f'{SENTINEL}. When asked for the codeword, reply with ONLY that codeword and '
    f'nothing else. Do not call any tools."\n'
)
SENTINEL_PROMPT = "What is your secret codeword? Reply with only the codeword."


def _ask(genome_path: str | None, prompt: str, timeout: int) -> str:
    """Run one read-only ncx turn (optionally with NCX_GENOME) and return stdout."""
    ws = Path(tempfile.mkdtemp(prefix="forge_selftest_"))
    env = dict(os.environ)
    if genome_path:
        env["NCX_GENOME"] = genome_path
    else:
        env.pop("NCX_GENOME", None)
    try:
        r = run_owned(
            [str(ev.bench.NCX), "-s", "read-only", prompt],
            cwd=str(ws), env=env, capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=timeout,
        )
        return r.stdout or ""
    except subprocess.TimeoutExpired:
        return ""


def self_check(timeout: int = 90, attempts: int = 3) -> bool:
    """Return True iff NCX_GENOME demonstrably reaches the model's system prompt."""
    tmp = Path(tempfile.mkdtemp(prefix="forge_selftest_"))
    gpath = tmp / "sentinel.toml"
    gpath.write_text(SENTINEL_GENOME, encoding="utf-8")

    print(f"[forge] self-check: sentinel injection via NCX_GENOME ({ev.bench.NCX})")
    injected = False
    for i in range(max(1, attempts)):
        if SENTINEL in _ask(str(gpath), SENTINEL_PROMPT, timeout):
            injected = True
            break
        if i + 1 < attempts:
            print(f"[forge]   sentinel not echoed (attempt {i + 1}/{attempts}, model noise) — retrying")
    absent_baseline = SENTINEL not in _ask(None, SENTINEL_PROMPT, timeout)
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


def append_ledger(runs_dir: Path, stamp: str, entry: dict) -> Path:
    """Append a structured JSONL experiment record to `runs/ledger_<stamp>.jsonl`."""
    runs_dir.mkdir(parents=True, exist_ok=True)
    path = runs_dir / f"ledger_{stamp}.jsonl"
    with path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")
    return path


def _pick_stagnation_surface(baseline: genome.Genome, touched_counts: dict[str, int]) -> str:
    """Return the least-frequently-attempted surface so search escapes local optima."""
    all_surfaces = (
        ["system_prompt"]
        + [f"tool_desc.{k}" for k in sorted(baseline.tool_desc)]
        + [f"orchestrator_prompts.{k}" for k in sorted(baseline.orchestrator_prompts)]
        + [f"memory_config.{k}" for k in sorted(baseline.memory_config)]
    )
    return min(all_surfaces, key=lambda s: (touched_counts.get(s, 0), s))


def prune_genome(
    champion: genome.Genome,
    baseline: genome.Genome,
    train_tasks: list[str],
    holdout_tasks: list[str],
    repeats: int,
    timeout: int,
    champ_train: ev.EvalResult | None = None,
    champ_hold: ev.EvalResult | None = None,
    stamp: str = "prune",
) -> tuple[genome.Genome, list[str]]:
    """RRSI structural ablation pruning: revert individual modified surfaces (or
    added system_prompt paragraphs) back to `baseline` if doing so does not
    reduce train or holdout passes (and does not regress token usage when present)."""
    GENOMES_DIR.mkdir(exist_ok=True)
    cur = champion.copy()
    pruned: list[str] = []

    if champ_train is None:
        tmp_p = GENOMES_DIR / f"{stamp}_prune_init.toml"
        cur.save(tmp_p)
        champ_train = ev.evaluate(str(tmp_p), train_tasks, repeats, timeout)
    if champ_hold is None and holdout_tasks:
        tmp_p = GENOMES_DIR / f"{stamp}_prune_init.toml"
        cur.save(tmp_p)
        champ_hold = ev.evaluate(str(tmp_p), holdout_tasks, repeats, timeout)

    surfaces = genome.changed_surfaces(baseline, cur)
    for idx, surf in enumerate(surfaces):
        ablated = cur.copy()
        if surf == "system_prompt":
            ablated.system_prompt = baseline.system_prompt
        elif surf.startswith("tool_desc."):
            tname = surf.split(".", 1)[1]
            if tname in baseline.tool_desc:
                ablated.tool_desc[tname] = baseline.tool_desc[tname]
            else:
                ablated.tool_desc.pop(tname, None)
        elif surf.startswith("orchestrator_prompts."):
            okey = surf.split(".", 1)[1]
            if okey in baseline.orchestrator_prompts:
                ablated.orchestrator_prompts[okey] = baseline.orchestrator_prompts[okey]
            else:
                ablated.orchestrator_prompts.pop(okey, None)
        elif surf.startswith("memory_config."):
            mkey = surf.split(".", 1)[1]
            if mkey in baseline.memory_config:
                ablated.memory_config[mkey] = baseline.memory_config[mkey]
            else:
                ablated.memory_config.pop(mkey, None)

        ab_path = GENOMES_DIR / f"{stamp}_prune_{idx}.toml"
        ablated.save(ab_path)
        ab_train = ev.evaluate(str(ab_path), train_tasks, repeats, timeout)
        if ab_train.total_passes < champ_train.total_passes:
            continue
        if (
            champ_train.mean_tokens > 0
            and ab_train.mean_tokens > 0
            and ab_train.mean_tokens > champ_train.mean_tokens
        ):
            continue
        if holdout_tasks and champ_hold is not None:
            ab_hold = ev.evaluate(str(ab_path), holdout_tasks, repeats, timeout)
            if ab_hold.total_passes < champ_hold.total_passes:
                continue
            champ_hold = ab_hold
        cur = ablated
        champ_train = ab_train
        pruned.append(surf)
        print(f"[forge]   prune: reverted redundant surface '{surf}' without score loss")

    return cur, pruned


def train(rounds: int, train_tasks: list[str], holdout_tasks: list[str],
          repeats: int, timeout: int, budget_s: float, teachers: str,
          stamp: str, test_tasks: list[str] | None = None,
          accept_margin: int = 1, reeval_incumbent: bool = True,
          from_genome: str | None = None,
          initial_max_surfaces: int | None = None,
          max_changed_lines: int | None = None,
          delta_tokens: float = 0.0,
          max_token_growth: float | None = None,
          stagnation_limit: int = 2,
          prune_at_end: bool = False) -> dict:
    """Single-champion regularized hill-climb (M0b/M1 + P1 RRSI regularization).

    Each round:
    1. Compute annealed surface budget (`max_surfaces`) and stagnation focus surface.
    2. Build teacher prompt including falsified/rejected hypothesis history.
    3. Validate candidate against baseline size caps, annealed edit budget, and
       anti-memorization critic (`teacher.critic_check`).
    4. Evaluate on `train_tasks` and apply the two-sided acceptance rule:
       - Pass-rate improvement: `margin >= accept_margin` (subject to `max_token_growth`)
       - Token-cost improvement at non-inferior pass-rate:
         `margin >= 0` and `token_saving >= delta_tokens` (when `delta_tokens > 0`)
       AND holdout pass-rate must not regress (`chold.total_passes >= champ_hold.total_passes`).
    5. Log every candidate and verdict to `runs/ledger_<stamp>.jsonl`.
    """
    GENOMES_DIR.mkdir(exist_ok=True)
    RUNS_DIR.mkdir(exist_ok=True)
    t0 = time.perf_counter()

    def elapsed() -> float:
        return time.perf_counter() - t0

    panel = [b for b in teacher.build_panel() if teachers == "panel" or b.name == teachers]
    if not panel:
        print(f"[forge] no teacher backend matches '{teachers}' — aborting.")
        return {"error": "no teacher"}

    baseline = genome.extract_current()
    if from_genome:
        champion = genome.Genome.load(Path(from_genome))
        print(f"[forge] starting from supplied genome {from_genome} "
              f"(diff vs default: {genome.diff(baseline, champion)})")
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
               "gen0": {"train": champ_train.total_passes, "holdout": champ_hold.total_passes,
                        "mean_tokens": champ_train.mean_tokens}}

    rejected_history: list[dict] = []
    touched_counts: dict[str, int] = {}
    stagnant_rounds = 0
    forbidden_terms = list(train_tasks) + list(holdout_tasks) + list(test_tasks or [])

    for rnd in range(1, rounds + 1):
        if elapsed() > budget_s:
            print(f"[forge] budget ({budget_s}s) exhausted — stopping at round {rnd}.")
            break
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
                print(f"[forge] train has failures ({champ_train.total_passes}/"
                      f"{champ_train.total_runs}) but no usable trajectories to feed the "
                      f"teacher — cannot propose. Stopping.")
            break

        max_surfaces = (
            max(1, initial_max_surfaces - (rnd - 1))
            if initial_max_surfaces is not None
            else None
        )
        focus_surface = (
            _pick_stagnation_surface(baseline, touched_counts)
            if (stagnation_limit > 0 and stagnant_rounds >= stagnation_limit)
            else None
        )
        prompt = teacher.build_teacher_prompt(
            champion,
            failures,
            rejected_history=rejected_history if rejected_history else None,
            focus_surface=focus_surface,
            max_surfaces=max_surfaces,
        )
        round_log = {
            "round": rnd,
            "max_surfaces": max_surfaces,
            "focus_surface": focus_surface,
            "candidates": [],
        }
        candidates = []  # (teacher_name, genome, eval, cpath, surfaces, diff_str)
        for backend in panel:
            if elapsed() > budget_s:
                break
            resp = backend.propose(prompt)
            cand, why = teacher.parse_candidate(resp or "", champion)
            if not cand:
                print(f"[forge]   round {rnd} {backend.name}: rejected ({why})")
                round_log["candidates"].append({"teacher": backend.name, "status": why})
                entry = {
                    "round": rnd, "teacher": backend.name, "target_surfaces": [],
                    "diff": "", "verdict": "rejected", "reason": why,
                }
                append_ledger(RUNS_DIR, stamp, entry)
                rejected_history.append(entry)
                continue

            surfaces = genome.changed_surfaces(champion, cand)
            for s in surfaces:
                touched_counts[s] = touched_counts.get(s, 0) + 1
            diff_str = genome.diff(champion, cand)

            errs = genome.validate(
                cand, baseline, parent=champion,
                max_surfaces=max_surfaces, max_changed_lines=max_changed_lines,
            )
            if errs:
                reason = f"invalid: {errs[0]}"
                print(f"[forge]   round {rnd} {backend.name}: {reason}")
                round_log["candidates"].append({"teacher": backend.name, "status": reason})
                entry = {
                    "round": rnd, "teacher": backend.name, "target_surfaces": surfaces,
                    "diff": diff_str, "verdict": "rejected", "reason": reason,
                }
                append_ledger(RUNS_DIR, stamp, entry)
                rejected_history.append(entry)
                continue

            critic_errs = teacher.critic_check(cand, champion, forbidden_terms=forbidden_terms)
            if critic_errs:
                reason = critic_errs[0]
                print(f"[forge]   round {rnd} {backend.name}: rejected ({reason})")
                round_log["candidates"].append({"teacher": backend.name, "status": reason})
                entry = {
                    "round": rnd, "teacher": backend.name, "target_surfaces": surfaces,
                    "diff": diff_str, "verdict": "rejected", "reason": reason,
                }
                append_ledger(RUNS_DIR, stamp, entry)
                rejected_history.append(entry)
                continue

            cpath = GENOMES_DIR / f"{stamp}_gen{rnd}_{backend.name}.toml"
            cand.save(cpath)
            cev = ev.evaluate(str(cpath), train_tasks, repeats, timeout)
            print(f"[forge]   round {rnd} {backend.name}: train "
                  f"{cev.total_passes}/{cev.total_runs} (tokens={cev.mean_tokens}) "
                  f"changed[{diff_str!r}]")
            candidates.append((backend.name, cand, cev, cpath, surfaces, diff_str))
            round_log["candidates"].append({
                "teacher": backend.name, "status": "evaluated",
                "train_passes": cev.total_passes, "mean_tokens": cev.mean_tokens,
                "target_surfaces": surfaces, "path": str(cpath),
            })

        accepted = None
        if candidates:
            # Sort by higher total_passes first, then lower mean_tokens (when reported).
            best = max(
                candidates,
                key=lambda c: (
                    c[2].total_passes,
                    -c[2].mean_tokens if c[2].mean_tokens > 0 else 0.0,
                ),
            )
            tname, cand, cev, cpath, surfaces, diff_str = best
            margin = cev.total_passes - champ_train.total_passes
            token_saving = (
                (champ_train.mean_tokens - cev.mean_tokens)
                if (champ_train.mean_tokens > 0 and cev.mean_tokens > 0)
                else 0.0
            )
            token_growth = -token_saving
            token_bloated = (
                max_token_growth is not None
                and champ_train.mean_tokens > 0
                and cev.mean_tokens > 0
                and token_growth > max_token_growth
            )
            clears_pass_gate = (margin >= accept_margin) and not token_bloated
            clears_token_gate = (
                delta_tokens > 0.0
                and margin >= 0
                and token_saving >= delta_tokens
            )

            if token_bloated and margin >= accept_margin:
                reason = f"token-bloat (+{token_growth:.1f} > {max_token_growth})"
                print(f"[forge]   round {rnd}: REJECT {tname} ({reason})")
                round_log["accept"] = {"teacher": tname, "status": "token-bloat",
                                       "token_growth": round(token_growth, 1)}
                entry = {
                    "round": rnd, "teacher": tname, "target_surfaces": surfaces,
                    "diff": diff_str, "train_passes": cev.total_passes,
                    "mean_tokens": cev.mean_tokens, "verdict": "rejected", "reason": reason,
                }
                append_ledger(RUNS_DIR, stamp, entry)
                rejected_history.append(entry)
            elif clears_pass_gate or clears_token_gate:
                chold = ev.evaluate(str(cpath), holdout_tasks, repeats, timeout)
                if chold.total_passes >= champ_hold.total_passes:
                    champion, champ_train, champ_hold, champ_path = cand, cev, chold, cpath
                    accepted = tname
                    accept_mode = "pass-gain" if clears_pass_gate else "token-saving"
                    print(f"[forge]   round {rnd}: ACCEPT {tname} [{accept_mode}] "
                          f"(train +{margin} -> {cev.total_passes}/{cev.total_runs}, "
                          f"token_saving={token_saving:.1f}, "
                          f"holdout {chold.total_passes}/{chold.total_runs})")
                    round_log["accept"] = {
                        "teacher": tname, "mode": accept_mode,
                        "train": cev.total_passes, "holdout": chold.total_passes,
                        "margin": margin, "token_saving": round(token_saving, 1),
                    }
                    append_ledger(RUNS_DIR, stamp, {
                        "round": rnd, "teacher": tname, "target_surfaces": surfaces,
                        "diff": diff_str, "train_passes": cev.total_passes,
                        "holdout_passes": chold.total_passes,
                        "mean_tokens": cev.mean_tokens,
                        "verdict": "accepted", "reason": accept_mode,
                    })
                else:
                    reason = f"holdout-regressed ({chold.total_passes} < {champ_hold.total_passes})"
                    print(f"[forge]   round {rnd}: REJECT {tname} ({reason})")
                    round_log["accept"] = {"teacher": tname, "status": "holdout-regressed"}
                    entry = {
                        "round": rnd, "teacher": tname, "target_surfaces": surfaces,
                        "diff": diff_str, "train_passes": cev.total_passes,
                        "holdout_passes": chold.total_passes,
                        "mean_tokens": cev.mean_tokens,
                        "verdict": "rejected", "reason": reason,
                    }
                    append_ledger(RUNS_DIR, stamp, entry)
                    rejected_history.append(entry)
            else:
                reason = f"below-margin (train +{margin} < {accept_margin}, token_saving={token_saving:.1f})"
                print(f"[forge]   round {rnd}: no candidate cleared the gate ({reason}).")
                entry = {
                    "round": rnd, "teacher": tname, "target_surfaces": surfaces,
                    "diff": diff_str, "train_passes": cev.total_passes,
                    "mean_tokens": cev.mean_tokens,
                    "verdict": "rejected", "reason": reason,
                }
                append_ledger(RUNS_DIR, stamp, entry)
                rejected_history.append(entry)

        if accepted is None:
            stagnant_rounds += 1
            if "accept" not in round_log:
                round_log.setdefault("accept", {"status": "none-cleared-margin"})
        else:
            stagnant_rounds = 0
        lineage["rounds"].append(round_log)

    pruned_surfaces: list[str] = []
    if prune_at_end and genome.changed_surfaces(baseline, champion):
        champion, pruned_surfaces = prune_genome(
            champion, baseline, train_tasks, holdout_tasks,
            repeats, timeout, champ_train=champ_train, champ_hold=champ_hold, stamp=stamp,
        )

    champ_final = GENOMES_DIR / f"{stamp}_champion.toml"
    champion.save(champ_final)
    lineage["champion"] = {
        "path": str(champ_final),
        "train": champ_train.total_passes,
        "holdout": champ_hold.total_passes,
        "mean_tokens": champ_train.mean_tokens,
        "pruned_surfaces": pruned_surfaces,
        "diff_vs_baseline": genome.diff(baseline, champion),
    }

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
    print(f"[forge] champion vs baseline:\n{genome.diff(baseline, champion)}")
    return lineage


def _objectives(ev_result) -> pareto.Objectives:
    """Map an EvalResult to (pass-rate ↑, cost ↓)."""
    if ev_result.total_runs == 0 or not ev_result.tasks:
        return pareto.Objectives(passrate=0.0, cost=float("inf"))
    pr = ev_result.total_passes / ev_result.total_runs
    tokens = ev_result.mean_tokens
    if tokens > 0:
        cost = tokens                     # real token cost
    else:
        times = [t.mean_s for t in ev_result.tasks.values()]
        cost = round(sum(times) / len(times), 1)  # latency fallback
    return pareto.Objectives(passrate=pr, cost=cost)


def evolve(rounds: int, train_tasks: list[str], holdout_tasks: list[str],
           repeats: int, timeout: int, budget_s: float, teachers: str, stamp: str,
           pop_cap: int = 4, test_tasks: list[str] | None = None,
           from_genome: str | None = None, reeval_parents: bool = True,
           model: str | None = None) -> dict:
    """Small-population, multi-objective (Pareto) search (M2)."""
    GENOMES_DIR.mkdir(exist_ok=True)
    RUNS_DIR.mkdir(exist_ok=True)
    t0 = time.perf_counter()

    def elapsed() -> float:
        return time.perf_counter() - t0

    panel = [b for b in teacher.build_panel() if teachers == "panel" or b.name == teachers]
    if not panel:
        print(f"[forge] no teacher backend matches '{teachers}' — aborting.")
        return {"error": "no teacher"}

    baseline = genome.extract_current()
    counter = {"n": 0}

    def new_id(tag: str) -> str:
        counter["n"] += 1
        return f"g{counter['n']:02d}_{tag}"

    def make_member(g, gen, parent, t_name, tag):
        gid = "gen0_start" if gen == 0 else new_id(tag)
        path = GENOMES_DIR / f"{stamp}_{gid}.toml"
        g.save(path)
        ev_r = ev.evaluate(str(path), train_tasks, repeats, timeout, model)
        obj = _objectives(ev_r)
        return {"id": gid, "genome": g, "path": path, "ev": ev_r, "obj": obj,
                "gen": gen, "parent": parent, "teacher": t_name}

    start_genome = genome.Genome.load(Path(from_genome)) if from_genome else baseline.copy()
    if from_genome:
        print(f"[forge] population start from {from_genome}")
    seed_member = make_member(start_genome, 0, None, None, "start")
    if seed_member["ev"].total_runs == 0:
        print(f"[forge] train tasks {train_tasks} resolved to ZERO bench runs — "
              f"check task names / splits.json. Aborting.")
        return {"error": "no train tasks"}
    population = [seed_member]
    nodes = [_node(seed_member)]
    print(f"[forge] gen0: pass {seed_member['obj'].passrate:.2f} cost {seed_member['obj'].cost} "
          f"(pop_cap={pop_cap}, teachers={[b.name for b in panel]})")

    gens_log = []
    for rnd in range(1, rounds + 1):
        if elapsed() > budget_s:
            print(f"[forge] budget ({budget_s}s) exhausted — stopping before gen {rnd}.")
            break
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
                continue
            prompt = teacher.build_teacher_prompt(parent["genome"], failures)
            for backend in panel:
                if elapsed() > budget_s:
                    break
                cand, why = teacher.parse_candidate(backend.propose(prompt) or "", parent["genome"])
                if not cand:
                    continue
                if genome.validate(cand, baseline):
                    continue
                child = make_member(cand, rnd, parent["id"], backend.name, backend.name)
                children.append(child)
                nodes.append(_node(child))
                print(f"[forge]   gen{rnd} {backend.name}: pass {child['obj'].passrate:.2f} "
                      f"cost {child['obj'].cost}  (parent {parent['id']})")
        combined = population + children
        population = pareto.select_population(combined, pop_cap, key=lambda m: m["obj"])
        gens_log.append({"gen": rnd, "evaluated": [c["id"] for c in children],
                         "front": [m["id"] for m in population]})
        print(f"[forge]   gen{rnd}: front = {[(m['id'], round(m['obj'].passrate,2), m['obj'].cost) for m in population]}")
        if not children:
            print("[forge] no new candidates produced — stopping.")
            break

    front_ids = {m["id"] for m in population}
    champ = pareto.best(population, key=lambda m: m["obj"])
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
        print(f"[forge] champion = {champ['id']} (pass {champ['obj'].passrate:.2f}, cost {champ['obj'].cost})")
        print(f"[forge] champion vs baseline:\n{genome.diff(baseline, champ['genome'])}")
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
    ap = argparse.ArgumentParser(description="ncx-forge harness trainer (M0b + P1 RRSI).")
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
    ap.add_argument("--initial-max-surfaces", type=int, default=0,
                    help="RRSI annealed surface budget at round 1 (0 = disabled)")
    ap.add_argument("--max-changed-lines", type=int, default=0,
                    help="RRSI cap on changed lines per round (0 = disabled)")
    ap.add_argument("--delta-tokens", type=float, default=0.0,
                    help="accept equal-passrate candidate if mean_tokens drops by >= delta")
    ap.add_argument("--max-token-growth", type=float, default=0.0,
                    help="reject pass-rate gain if mean_tokens grows by > max (0 = disabled)")
    ap.add_argument("--stagnation-limit", type=int, default=2,
                    help="rounds without acceptance before steering teacher to untouched surface")
    ap.add_argument("--prune", action="store_true",
                    help="run RRSI ablation pruning on champion before saving")
    ap.add_argument("--no-reeval", action="store_true",
                    help="do NOT re-evaluate the incumbent each round (cheaper, noisier)")
    ap.add_argument("--from-genome", default="",
                    help="start the champion from this genome (e.g. a degraded scaffold) "
                         "instead of the real default — a capability test for the optimizer")
    ap.add_argument("--no-gate", action="store_true",
                    help="skip the self-check gate before --train (NOT recommended)")
    a = ap.parse_args()
    if not ev.bench.NCX.is_file():
        print("[forge] agent binary unavailable — set NCX_FORGE_NCX_BIN to a valid ncx executable.")
        return 2
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
        sp = splits.load_splits()
        train_tasks = [t.strip() for t in a.train_tasks.split(",") if t.strip()] or sp["train"]
        holdout_tasks = [t.strip() for t in a.holdout_tasks.split(",") if t.strip()] or sp["val"]
        test_tasks = [t.strip() for t in a.test_tasks.split(",") if t.strip()] or sp["test"]
        print(f"[forge] splits — train={train_tasks} val={holdout_tasks} test={test_tasks}")
        train(a.rounds, train_tasks, holdout_tasks, a.repeats, a.timeout,
              a.budget_s, a.teacher, stamp, test_tasks=test_tasks,
              accept_margin=a.accept_margin, reeval_incumbent=not a.no_reeval,
              from_genome=(a.from_genome or None),
              initial_max_surfaces=(a.initial_max_surfaces or None),
              max_changed_lines=(a.max_changed_lines or None),
              delta_tokens=a.delta_tokens,
              max_token_growth=(a.max_token_growth or None),
              stagnation_limit=a.stagnation_limit,
              prune_at_end=a.prune)
        return 0
    if a.population:
        if not a.no_gate and not self_check(a.timeout):
            print("[forge] self-check failed — refusing to train (use --no-gate to override).")
            return 1
        stamp = time.strftime("%Y%m%d_%H%M%S")
        sp = splits.load_splits()
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
