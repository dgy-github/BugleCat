"""Task-graph validation and scheduling — pure code, never the model.

The review's #1 and #2 high-severity findings: a model-produced task graph WILL
sometimes contain cycles, dangling dependencies, or duplicate ids, and "worker
self-declares done" must be gated by code, not prompt. This module is the code
half: it validates a graph the planner produced and decides which nodes are
runnable. The orchestrator must call :func:`validate_graph` before executing a
plan and refuse to run an invalid one.
"""

from __future__ import annotations

from nanocodex.agent.state import TASK_KINDS, TERMINAL_STATUSES, TaskNode


class GraphError(ValueError):
    """Raised when a task graph is structurally invalid (cycle, dangling, dup)."""


def validate_graph(nodes: list[TaskNode]) -> None:
    """Raise :class:`GraphError` if the graph is not a legal DAG.

    Checks, in order: duplicate ids, unknown kinds, dangling dependencies
    (depends_on an id that doesn't exist), self-dependency, and cycles. The
    orchestrator runs this on every planner output and every replan, so a model
    can never push the scheduler into a deadlock or an infinite loop.
    """
    ids: set[str] = set()
    for n in nodes:
        if not n.id:
            raise GraphError("a task node has an empty id")
        if n.id in ids:
            raise GraphError(f"duplicate node id: {n.id!r}")
        ids.add(n.id)
        if n.kind not in TASK_KINDS:
            raise GraphError(f"node {n.id!r} has unknown kind {n.kind!r}")

    for n in nodes:
        for dep in n.depends_on:
            if dep == n.id:
                raise GraphError(f"node {n.id!r} depends on itself")
            if dep not in ids:
                raise GraphError(
                    f"node {n.id!r} depends on missing node {dep!r}"
                )

    cycle = _find_cycle(nodes)
    if cycle:
        raise GraphError("dependency cycle detected: " + " -> ".join(cycle))


def _find_cycle(nodes: list[TaskNode]) -> list[str]:
    """Return one cycle as an id path, or [] if the graph is acyclic (DFS colors)."""
    adj = {n.id: list(n.depends_on) for n in nodes}
    WHITE, GRAY, BLACK = 0, 1, 2
    color = {nid: WHITE for nid in adj}
    stack: list[str] = []

    def dfs(u: str) -> list[str]:
        color[u] = GRAY
        stack.append(u)
        for v in adj.get(u, ()):
            if color[v] == GRAY:
                # Found a back-edge; slice the stack from v to close the cycle.
                i = stack.index(v)
                return stack[i:] + [v]
            if color[v] == WHITE:
                found = dfs(v)
                if found:
                    return found
        color[u] = BLACK
        stack.pop()
        return []

    for nid in adj:
        if color[nid] == WHITE:
            found = dfs(nid)
            if found:
                return found
    return []


def topo_order(nodes: list[TaskNode]) -> list[str]:
    """Kahn topological order of node ids. Assumes the graph already validated.

    Order is dependency-first: a node appears after every node it depends on.
    Raises :class:`GraphError` if a cycle slipped through (defensive).
    """
    adj = {n.id: list(n.depends_on) for n in nodes}
    indeg = {nid: 0 for nid in adj}
    # depends_on points at prerequisites; in-degree counts unmet prerequisites.
    for nid, deps in adj.items():
        indeg[nid] = len(deps)
    dependents: dict[str, list[str]] = {nid: [] for nid in adj}
    for nid, deps in adj.items():
        for d in deps:
            dependents[d].append(nid)

    # Stable order: process ready ids in their original plan order.
    order_index = {n.id: i for i, n in enumerate(nodes)}
    ready = sorted([nid for nid, d in indeg.items() if d == 0], key=order_index.get)
    out: list[str] = []
    while ready:
        u = ready.pop(0)
        out.append(u)
        new_ready: list[str] = []
        for w in dependents[u]:
            indeg[w] -= 1
            if indeg[w] == 0:
                new_ready.append(w)
        ready = sorted(ready + new_ready, key=order_index.get)
    if len(out) != len(nodes):
        raise GraphError("cycle detected during topological sort")
    return out


def ready_nodes(nodes: list[TaskNode]) -> list[TaskNode]:
    """Nodes whose every dependency is `done` and that are themselves runnable.

    A node is runnable when its status is pending/ready and all deps are done.
    Returned in plan order so the scheduler is deterministic.
    """
    done = {n.id for n in nodes if n.status == "done"}
    out: list[TaskNode] = []
    for n in nodes:
        if n.status not in ("pending", "ready"):
            continue
        if all(dep in done for dep in n.depends_on):
            out.append(n)
    return out


def propagate_skips(nodes: list[TaskNode]) -> list[str]:
    """Mark as `skipped` any non-terminal node with a failed/skipped ancestor.

    The review's missing-state finding: when a node fails, its descendants
    should become `skipped` (cannot run, but not themselves failures) rather
    than sitting `pending` forever or being miscounted as `blocked`. Returns the
    ids newly skipped. Idempotent — safe to call after every node completes.
    """
    by_id = {n.id: n for n in nodes}
    dead = {n.id for n in nodes if n.status in ("failed", "skipped", "cancelled")}
    newly: list[str] = []
    changed = True
    while changed:
        changed = False
        for n in nodes:
            if n.status in TERMINAL_STATUSES:
                continue
            if any(dep in dead for dep in n.depends_on):
                n.status = "skipped"
                dead.add(n.id)
                newly.append(n.id)
                changed = True
    return newly
