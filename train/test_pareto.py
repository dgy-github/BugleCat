#!/usr/bin/env python3
"""Deterministic unit tests for the Pareto/population core (no model calls)."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pareto as P  # noqa: E402

O = P.Objectives
KEY = lambda x: x[1]  # noqa: E731  (item = (name, Objectives))


def test_dominance_direction():
    # higher passrate + lower cost dominates
    assert O(0.8, 5).dominates(O(0.5, 5))      # better passrate, equal cost
    assert O(0.5, 3).dominates(O(0.5, 10))     # equal passrate, cheaper
    assert O(0.8, 3).dominates(O(0.5, 10))     # better on both
    assert not O(0.5, 5).dominates(O(0.5, 5))  # identical -> no strict win
    assert not O(0.8, 20).dominates(O(0.5, 5)) # better passrate but pricier -> tradeoff
    assert not O(0.5, 5).dominates(O(0.8, 20)) # symmetric


def test_pareto_front_keeps_tradeoffs_drops_dominated():
    pts = [("A", O(0.5, 10)), ("B", O(0.5, 5)), ("C", O(0.8, 20)), ("D", O(0.3, 3))]
    front = [p[0] for p in P.pareto_front(pts, KEY)]
    assert "A" not in front           # dominated by B (same passrate, cheaper)
    assert set(front) == {"B", "C", "D"}, front


def test_front_preserves_input_order():
    pts = [("C", O(0.8, 20)), ("B", O(0.5, 5)), ("D", O(0.3, 3))]
    front = [p[0] for p in P.pareto_front(pts, KEY)]
    assert front == ["C", "B", "D"]


def test_best_is_max_passrate_then_min_cost():
    pts = [("B", O(0.5, 5)), ("C", O(0.8, 20)), ("C2", O(0.8, 9))]
    assert P.best(pts, KEY)[0] == "C2"   # ties on passrate -> cheaper wins


def test_crowding_trim_keeps_boundaries():
    # 4 front points spread across cost; trim to 2 must keep the extremes.
    pts = [("cheap", O(0.4, 1)), ("mid1", O(0.6, 5)), ("mid2", O(0.7, 8)), ("strong", O(0.9, 20))]
    kept = {p[0] for p in P.crowding_trim(pts, 2, KEY)}
    assert "cheap" in kept and "strong" in kept, kept


def test_select_population_combines_front_and_trim():
    pts = [("A", O(0.5, 10)), ("B", O(0.5, 5)), ("C", O(0.8, 20)), ("D", O(0.3, 3))]
    sel = {p[0] for p in P.select_population(pts, 2, KEY)}
    assert "A" not in sel and len(sel) == 2  # A dominated; trimmed to 2


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn(); print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1; print(f"FAIL {fn.__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
