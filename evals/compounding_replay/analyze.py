"""Noise calibration and paired A/B analysis for compounding replay runs.

No model calls; reads runs/<run_id>/results.jsonl only.

Primary metric (decided 2026-10-23): cost per resolved task. Resolution rate is
a non-inferiority guard only (B >= A - delta), because 8-task rounds are far
too small to detect resolution-rate effects of practical size.

Usage:
  # noise calibration: several rounds of the same arm
  python evals/compounding_replay/analyze.py --a "*-A-split-v2-*"
  # paired A/B comparison
  python evals/compounding_replay/analyze.py --a "*-A-split-v2-*" --b "*-B-split-v2-*"
  # machine-readable summary
  python evals/compounding_replay/analyze.py --a ... --b ... --json out.json

Run patterns are globs relative to evals/compounding_replay/runs/.
"""
from __future__ import annotations

import argparse
import json
import math
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUNS = HERE / "runs"
Z95 = 1.959964
# One-sided 95%: for gates that only ever accept an improvement.
Z95_ONE_SIDED = 1.644854


def load_runs(pattern: str) -> list[dict]:
    runs = []
    for d in sorted(RUNS.glob(pattern)):
        f = d / "results.jsonl"
        if not f.exists():
            continue
        rows = [json.loads(l) for l in f.read_text(encoding="utf-8").splitlines() if l.strip()]
        if rows:
            runs.append({"run_id": d.name, "rows": {r["id"]: r for r in rows}})
    return runs


def common_tasks(*groups: list[dict]) -> list[str]:
    sets = [set(run["rows"]) for g in groups for run in g]
    if not sets:
        return []
    keep = set.intersection(*sets)
    # keep replay order of the first run
    first = groups[0][0]["rows"]
    return [t for t in first if t in keep]


def mean(xs: list[float]) -> float:
    return sum(xs) / len(xs) if xs else float("nan")


def sd(xs: list[float]) -> float:
    if len(xs) < 2:
        return 0.0
    m = mean(xs)
    return math.sqrt(sum((x - m) ** 2 for x in xs) / (len(xs) - 1))


def cell(run: dict, tid: str) -> tuple[int, int, float, int]:
    r = run["rows"][tid]
    judge_ok = int(bool((r.get("judge") or {}).get("resolved")))
    u = r.get("usage") or {}
    return int(bool(r["resolved"])), judge_ok, float(r["cost_cny"]), int(u.get("total_tokens", 0))


def arm_stats(runs: list[dict], tasks: list[str]) -> dict:
    per_round = []
    for run in runs:
        cells = [cell(run, t) for t in tasks]
        solved = sum(c[0] for c in cells)
        cost = sum(c[2] for c in cells)
        per_round.append({
            "run_id": run["run_id"], "resolved": solved,
            "judge_resolved": sum(c[1] for c in cells),
            "cost": round(cost, 4),
            "cost_per_resolved": round(cost / solved, 4) if solved else None,
            "tokens": sum(c[3] for c in cells),
        })
    per_task = {}
    for t in tasks:
        res = [cell(r, t)[0] for r in runs]
        cost = [cell(r, t)[2] for r in runs]
        per_task[t] = {"pass_rate": mean(res), "cost_mean": mean(cost), "cost_sd": sd(cost)}
    return {"rounds": per_round, "per_task": per_task}


def deltas(per_task: dict, k_rounds: int, n_obs: int) -> dict:
    """Noise floors for comparing two arms, each averaged over k rounds.

    Resolution: a round total is a sum of per-task Bernoullis, so
    Var(total) = sum p_i (1 - p_i). Cost: Var(total) = sum Var_i. The 95% noise
    floor of a difference of two k-round means is z * sqrt(2 Var / k).

    p_i is Jeffreys-smoothed ((s + 0.5) / (n + 1)) so a task that happened to
    pass (or fail) in all of a handful of rounds does not claim zero variance.
    """
    def smooth(p: float) -> float:
        return (p * n_obs + 0.5) / (n_obs + 1)

    var_res = sum(smooth(v["pass_rate"]) * (1 - smooth(v["pass_rate"])) for v in per_task.values())
    var_cost = sum(v["cost_sd"] ** 2 for v in per_task.values())
    k = max(k_rounds, 1)
    return {
        "delta_resolved_tasks": round(Z95 * math.sqrt(2 * var_res / k), 3),
        "delta_cost_cny": round(Z95 * math.sqrt(2 * var_cost / k), 4),
        "single_round_sd_resolved": round(math.sqrt(var_res), 3),
        "single_round_sd_cost": round(math.sqrt(var_cost), 4),
    }


def jeffreys(passes: int, runs: int) -> float:
    """Jeffreys-smoothed pass rate, so 0/n and n/n do not claim zero variance."""
    return (passes + 0.5) / (runs + 1)


def pass_noise_floor(per_task: list[tuple[int, int, int, int]], z: float = Z95_ONE_SIDED) -> float:
    """Noise floor on the difference of total passes between two evaluations
    of the same tasks.

    Each element is `(passes_a, runs_a, passes_b, runs_b)` for one task. Under
    "no real difference" both evaluations share the task's pass rate, estimated
    from the pooled runs, so Var(S_a - S_b) = sum (runs_a + runs_b) p_i (1 - p_i).
    A difference must exceed `z * sqrt(Var)` passes before it is more than noise.
    """
    var = 0.0
    for passes_a, runs_a, passes_b, runs_b in per_task:
        p = jeffreys(passes_a + passes_b, runs_a + runs_b)
        var += (runs_a + runs_b) * p * (1 - p)
    return z * math.sqrt(var)


def mcnemar_exact(b: int, c: int) -> float:
    """Two-sided exact McNemar p-value on discordant counts b, c."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2 ** n
    return min(1.0, 2 * tail)


def paired(a_runs: list[dict], b_runs: list[dict], tasks: list[str], iters: int, seed: int) -> dict:
    k = min(len(a_runs), len(b_runs))
    # resolution: pair round r of A with round r of B, per task
    a_only = b_only = 0
    for r in range(k):
        for t in tasks:
            ra, rb = cell(a_runs[r], t)[0], cell(b_runs[r], t)[0]
            a_only += ra and not rb
            b_only += rb and not ra
    # cost per resolved: bootstrap over tasks (keeps the per-task pairing)
    def totals(sample: list[str]) -> tuple[float, float, float, float]:
        ca = sum(cell(r, t)[2] for r in a_runs[:k] for t in sample)
        sa = sum(cell(r, t)[0] for r in a_runs[:k] for t in sample)
        cb = sum(cell(r, t)[2] for r in b_runs[:k] for t in sample)
        sb = sum(cell(r, t)[0] for r in b_runs[:k] for t in sample)
        return ca, sa, cb, sb

    ca, sa, cb, sb = totals(tasks)
    rng = random.Random(seed)
    ratios, cost_diffs = [], []
    for _ in range(iters):
        sample = [rng.choice(tasks) for _ in tasks]
        xa, ya, xb, yb = totals(sample)
        cost_diffs.append((xb - xa) / k)
        if ya and yb:
            ratios.append((xb / yb) / (xa / ya))
    ratios.sort()
    cost_diffs.sort()

    def ci(xs: list[float]) -> list[float] | None:
        if not xs:
            return None
        return [round(xs[int(0.025 * len(xs))], 4), round(xs[int(0.975 * len(xs)) - 1], 4)]

    cpr_a = ca / sa if sa else None
    cpr_b = cb / sb if sb else None
    return {
        "rounds_paired": k,
        "resolved_a": sa, "resolved_b": sb, "cells": k * len(tasks),
        "discordant_a_only": a_only, "discordant_b_only": b_only,
        "mcnemar_p": round(mcnemar_exact(a_only, b_only), 4),
        "cost_per_resolved_a": round(cpr_a, 4) if cpr_a else None,
        "cost_per_resolved_b": round(cpr_b, 4) if cpr_b else None,
        "cpr_ratio_b_over_a": round(cpr_b / cpr_a, 4) if cpr_a and cpr_b else None,
        "cpr_ratio_ci95": ci(ratios),
        "round_cost_diff_b_minus_a": round((cb - ca) / k, 4),
        "round_cost_diff_ci95": ci(cost_diffs),
    }


def print_arm(name: str, stats: dict, d: dict) -> None:
    print(f"\n== Arm {name}: {len(stats['rounds'])} round(s)")
    for r in stats["rounds"]:
        print(f"  {r['run_id']:<34} resolved={r['resolved']} (judge {r['judge_resolved']}) "
              f"cost={r['cost']:.3f} cpr={r['cost_per_resolved']}")
    swing = [t for t, v in stats["per_task"].items() if 0 < v["pass_rate"] < 1]
    always = [t for t, v in stats["per_task"].items() if v["pass_rate"] == 1]
    never = [t for t, v in stats["per_task"].items() if v["pass_rate"] == 0]
    print(f"  swing={swing}\n  always={always}\n  never={never}")
    print(f"  noise: single-round sd resolved={d['single_round_sd_resolved']} tasks, "
          f"cost={d['single_round_sd_cost']} CNY")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--a", required=True, help="glob of arm A run dirs under runs/")
    ap.add_argument("--b", help="glob of arm B run dirs under runs/ (enables paired comparison)")
    ap.add_argument("--iters", type=int, default=5000)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--json", help="write summary JSON here")
    args = ap.parse_args()

    a_runs = load_runs(args.a)
    b_runs = load_runs(args.b) if args.b else []
    if not a_runs or (args.b and not b_runs):
        print("no matching runs with results.jsonl", file=sys.stderr)
        return 1
    tasks = common_tasks(a_runs, *([b_runs] if b_runs else []))
    print(f"tasks in every run ({len(tasks)}): {tasks}")

    out: dict = {"tasks": tasks}
    a_stats = arm_stats(a_runs, tasks)
    pooled = a_stats["per_task"]
    if b_runs:
        b_stats = arm_stats(b_runs, tasks)
        # pooled per-task noise (both arms) for the comparison floor
        pooled = {}
        for t in tasks:
            res = [cell(r, t)[0] for r in a_runs + b_runs]
            cost = [cell(r, t)[2] for r in a_runs + b_runs]
            pooled[t] = {"pass_rate": mean(res), "cost_mean": mean(cost), "cost_sd": sd(cost)}
    k = min(len(a_runs), len(b_runs)) if b_runs else len(a_runs)
    d = deltas(pooled, k, len(a_runs) + len(b_runs))
    print_arm("A", a_stats, deltas(a_stats["per_task"], len(a_runs), len(a_runs)))
    out["A"] = a_stats
    if b_runs:
        print_arm("B", b_stats, deltas(b_stats["per_task"], len(b_runs), len(b_runs)))
        out["B"] = b_stats
        p = paired(a_runs, b_runs, tasks, args.iters, args.seed)
        out["paired"] = p
        n_cells = p["cells"]
        rate_diff = (p["resolved_b"] - p["resolved_a"]) / n_cells
        delta_rate = d["delta_resolved_tasks"] / len(tasks)
        noninferior = rate_diff >= -delta_rate
        print(f"\n== Paired A vs B over {p['rounds_paired']} round(s), {n_cells} cells per arm")
        print(f"  resolved A={p['resolved_a']} B={p['resolved_b']}  discordant A-only={p['discordant_a_only']} "
              f"B-only={p['discordant_b_only']}  McNemar p={p['mcnemar_p']}")
        print(f"  [PRIMARY] cost per resolved: A={p['cost_per_resolved_a']} B={p['cost_per_resolved_b']} "
              f"ratio B/A={p['cpr_ratio_b_over_a']} CI95={p['cpr_ratio_ci95']}")
        print(f"  round cost diff (B-A)={p['round_cost_diff_b_minus_a']} CI95={p['round_cost_diff_ci95']}")
        print(f"  [GUARD] resolution diff={rate_diff:+.3f} vs -delta={-delta_rate:.3f} -> "
              f"{'non-inferior' if noninferior else 'NOT non-inferior'}")
        out["guard"] = {"rate_diff": round(rate_diff, 4), "delta_rate": round(delta_rate, 4),
                        "noninferior": noninferior}
    print(f"\n== Noise floor for comparing {k}-round means (95%): "
          f"delta_resolved={d['delta_resolved_tasks']} tasks/round, delta_cost={d['delta_cost_cny']} CNY/round")
    out["delta"] = d
    if args.json:
        Path(args.json).write_text(json.dumps(out, indent=1, ensure_ascii=False), encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
