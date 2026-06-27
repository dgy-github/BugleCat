#!/usr/bin/env python3
"""Deterministic test of the M2 population/Pareto loop + viz (no model calls).

Verifies the multi-objective behavior single-champion can't show: the front
KEEPS a trade-off (a slow-but-strong genome AND a cheap-but-decent one) and
drops a dominated incumbent; champion = max pass-rate; a lineage + viz render.
"""
from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
import forge  # noqa: E402
import genome as G  # noqa: E402
import teacher as T  # noqa: E402
import viz  # noqa: E402

BASE = G.Genome(system_prompt="base prompt", tool_desc={"read_file": "rf", "apply_patch": "ap"})


class FakeTeacher(T.TeacherBackend):
    def __init__(self, name, toml):
        self.name = name
        self._toml = toml
    def available(self):
        return True
    def propose(self, prompt, timeout=180):
        return f"```toml\n{self._toml}\n```"


def _mock_eval(genome_path, tasks, repeats, timeout, model=None):
    """t_a is the 'hard' task; t_b always passes. STRONG genome passes both
    (slow); CHEAP genome matches baseline pass-rate but is much faster."""
    content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
    strong = "STRONG" in content
    cheap = "CHEAP" in content
    r = ev.EvalResult(genome=str(genome_path))
    for t in tasks:
        if t == "t_a":
            p = 1 if strong else 0
        else:
            p = 1
        mean_s = 20.0 if strong else (2.0 if cheap else 10.0)
        r.tasks[t] = ev.TaskResult(task=t, passes=p, runs=1, mean_s=mean_s,
                                   failure_trajectory=("" if p else "failed t_a"))
    return r


def _install(monkey_eval, panel):
    orig = (forge.G.extract_current, forge.ev.evaluate, forge.T.build_panel,
            forge.GENOMES_DIR, forge.RUNS_DIR)
    tmp = Path(tempfile.mkdtemp(prefix="forge_pop_"))
    forge.GENOMES_DIR = tmp / "genomes"
    forge.RUNS_DIR = tmp / "runs"
    forge.G.extract_current = lambda: BASE.copy()
    forge.ev.evaluate = monkey_eval
    forge.T.build_panel = lambda verbose=True: panel

    def restore():
        (forge.G.extract_current, forge.ev.evaluate, forge.T.build_panel,
         forge.GENOMES_DIR, forge.RUNS_DIR) = orig
    return restore, tmp


def test_front_keeps_tradeoff_and_drops_dominated():
    panel = [FakeTeacher("strong", 'system_prompt = "STRONG disciplined prompt"'),
             FakeTeacher("cheap", 'system_prompt = "CHEAP terse prompt"')]
    restore, tmp = _install(_mock_eval, panel)
    try:
        lin = forge.evolve(rounds=1, train_tasks=["t_a", "t_b"], holdout_tasks=["t_b"],
                           repeats=1, timeout=10, budget_s=999, teachers="panel",
                           stamp="P1", pop_cap=3, test_tasks=None)
        front = {n["id"] for n in lin["nodes"] if n.get("on_front_final")}
        teachers_on_front = {n["teacher"] for n in lin["nodes"] if n["id"] in front}
        # Both a strong (slow) and a cheap (fast) child survive -> trade-off kept.
        assert "strong" in teachers_on_front and "cheap" in teachers_on_front, teachers_on_front
        # The dominated gen0 baseline (same pass-rate as cheap but pricier) is dropped.
        assert "gen0_start" not in front, front
        # Champion = highest pass-rate = the strong one.
        champ_id = lin["champion"]["id"]
        champ_node = next(n for n in lin["nodes"] if n["id"] == champ_id)
        assert champ_node["teacher"] == "strong" and lin["champion"]["passrate"] == 1.0, lin["champion"]
    finally:
        restore()


def test_empty_eval_is_worst_not_best():
    # Guard against the masked-misconfig bug: a zero-task EvalResult must map to
    # the WORST objective (cost=inf), so it can't silently win the Pareto front.
    empty = ev.EvalResult(genome="x")  # no tasks
    obj = forge._objectives(empty)
    assert obj.cost == float("inf") and obj.passrate == 0.0, obj
    # And it is dominated by any real genome.
    import pareto as P
    assert P.Objectives(0.3, 100.0).dominates(obj)


def test_reeval_parents_rescores_surviving_members():
    # With reeval on, gen>=2 re-scores surviving members under a fresh draw — so
    # the same genome can get a DIFFERENT objective than its first (lucky) draw.
    calls = {"n": 0}
    seen_costs = {}

    def drifting_eval(genome_path, tasks, repeats, timeout, model=None):
        calls["n"] += 1
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        r = ev.EvalResult(genome=str(genome_path))
        # cost drifts upward on each successive eval of the SAME genome -> a re-eval
        # produces a different objective than the first draw.
        seen_costs[genome_path] = seen_costs.get(genome_path, 0) + 5.0
        for t in tasks:
            p = 0 if t == "t_a" else 1  # t_a always fails -> always has children
            r.tasks[t] = ev.TaskResult(task=t, passes=p, runs=1, mean_s=seen_costs[genome_path],
                                       failure_trajectory=("" if p else "fail"))
        return r

    panel = [FakeTeacher("strong", 'system_prompt = "STRONG disciplined prompt"')]

    def run(reeval):
        restore, _ = _install(drifting_eval, panel)
        try:
            calls["n"] = 0; seen_costs.clear()
            forge.evolve(rounds=2, train_tasks=["t_a", "t_b"], holdout_tasks=["t_b"],
                         repeats=1, timeout=10, budget_s=999, teachers="panel",
                         stamp="P3", pop_cap=3, test_tasks=None, reeval_parents=reeval)
            return calls["n"]
        finally:
            restore()

    with_reeval = run(True)
    without = run(False)
    assert with_reeval > without, (with_reeval, without)  # re-eval adds eval passes


def test_evolve_writes_viz_html():
    panel = [FakeTeacher("strong", 'system_prompt = "STRONG disciplined prompt"')]
    restore, tmp = _install(_mock_eval, panel)
    try:
        forge.evolve(rounds=1, train_tasks=["t_a", "t_b"], holdout_tasks=["t_b"],
                     repeats=1, timeout=10, budget_s=999, teachers="panel",
                     stamp="P2", pop_cap=3, test_tasks=None)
        html = (tmp / "runs" / "lineage_P2.html").read_text(encoding="utf-8")
        assert "<svg" in html and "★champion" in html
    finally:
        restore()


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn(); print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1; print(f"FAIL {fn.__name__}: {e}")
        except Exception as e:  # noqa: BLE001
            failed += 1; print(f"ERR  {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
