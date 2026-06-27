#!/usr/bin/env python3
"""Multi-objective (Pareto) helpers for ncx-forge M2.

A genome is scored on two competing objectives:
  * pass-rate  — MAXIMIZE (passes / runs)
  * cost       — MINIMIZE (mean wall-clock seconds per task; a token proxy —
                 ncx does not emit token usage, so latency stands in for it)

We keep the Pareto FRONT (non-dominated genomes): a genome that is no worse on
both objectives and strictly better on one. When the front exceeds the
population cap we trim by NSGA-II crowding distance to preserve spread (keep the
extremes + the most distinct interior points), not just the highest pass-rate —
so the search retains a cheap-but-decent option alongside a slow-but-strong one.

Pure functions, deterministic, unit-tested — this is where multi-objective bugs
(dominance direction, front extraction) would hide.
"""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Objectives:
    passrate: float  # maximize
    cost: float      # minimize (mean seconds)

    def dominates(self, other: "Objectives") -> bool:
        """True if self is >= other on BOTH objectives and > on at least one.

        (passrate: higher better; cost: lower better.)
        """
        no_worse = self.passrate >= other.passrate and self.cost <= other.cost
        strictly_better = self.passrate > other.passrate or self.cost < other.cost
        return no_worse and strictly_better


def pareto_front(items: list, key) -> list:
    """Return the non-dominated subset of `items`. `key(item) -> Objectives`.

    Stable: preserves input order among non-dominated items.
    """
    objs = [key(it) for it in items]
    front = []
    for i, it in enumerate(items):
        dominated = any(
            j != i and objs[j].dominates(objs[i])
            for j in range(len(items))
        )
        if not dominated:
            front.append(it)
    return front


def _crowding_distances(objs: list) -> list:
    """NSGA-II crowding distance per point (boundary points get +inf)."""
    n = len(objs)
    if n <= 2:
        return [float("inf")] * n
    dist = [0.0] * n
    for attr in ("passrate", "cost"):
        order = sorted(range(n), key=lambda i: getattr(objs[i], attr))
        lo = getattr(objs[order[0]], attr)
        hi = getattr(objs[order[-1]], attr)
        span = hi - lo
        dist[order[0]] = float("inf")
        dist[order[-1]] = float("inf")
        if span == 0:
            continue
        for k in range(1, n - 1):
            prev = getattr(objs[order[k - 1]], attr)
            nxt = getattr(objs[order[k + 1]], attr)
            dist[order[k]] += (nxt - prev) / span
    return dist


def crowding_trim(items: list, k: int, key) -> list:
    """Trim `items` (assumed a Pareto front) down to `k` by keeping the most
    crowding-distant points (boundaries first). Returns up to k items, order
    preserved from `items`."""
    if len(items) <= k:
        return items
    objs = [key(it) for it in items]
    dist = _crowding_distances(objs)
    # Rank indices by crowding distance desc; keep the top k, then restore order.
    keep = sorted(range(len(items)), key=lambda i: dist[i], reverse=True)[:k]
    keep_set = set(keep)
    return [it for i, it in enumerate(items) if i in keep_set]


def select_population(items: list, k: int, key) -> list:
    """One selection step: take the Pareto front, trim to k by crowding."""
    return crowding_trim(pareto_front(items, key), k, key)


def best(items: list, key) -> object:
    """The single 'champion' to report/ship: max pass-rate, tie-break min cost."""
    return max(items, key=lambda it: (key(it).passrate, -key(it).cost)) if items else None


if __name__ == "__main__":
    # Tiny demo.
    pts = [("A", Objectives(0.5, 10)), ("B", Objectives(0.5, 5)),
           ("C", Objectives(0.8, 20)), ("D", Objectives(0.3, 3))]
    f = pareto_front(pts, key=lambda p: p[1])
    print("front:", [p[0] for p in f])  # B (cheap), C (strong), D (cheapest) ; A dominated by B
    print("best :", best(pts, key=lambda p: p[1])[0])  # C
