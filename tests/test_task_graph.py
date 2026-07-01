"""Task-graph validation and scheduling — the pure-code DAG guarantees."""

from __future__ import annotations

import pytest

from nanocodex.agent.state import TaskNode
from nanocodex.agent.task_graph import (
    GraphError,
    propagate_skips,
    ready_nodes,
    topo_order,
    validate_graph,
)


def _n(nid, deps=(), kind="code", status="pending"):
    return TaskNode(id=nid, kind=kind, title=nid, depends_on=list(deps), status=status)


def test_valid_dag_passes():
    nodes = [_n("a"), _n("b", ["a"]), _n("c", ["a", "b"])]
    validate_graph(nodes)  # no raise
    assert topo_order(nodes) == ["a", "b", "c"]


def test_self_dependency_rejected():
    with pytest.raises(GraphError, match="depends on itself"):
        validate_graph([_n("a", ["a"])])


def test_dangling_dependency_rejected():
    with pytest.raises(GraphError, match="missing node"):
        validate_graph([_n("a", ["ghost"])])


def test_duplicate_id_rejected():
    with pytest.raises(GraphError, match="duplicate node id"):
        validate_graph([_n("a"), _n("a")])


def test_unknown_kind_rejected():
    with pytest.raises(GraphError, match="unknown kind"):
        validate_graph([_n("a", kind="frobnicate")])


def test_two_node_cycle_detected():
    with pytest.raises(GraphError, match="cycle"):
        validate_graph([_n("a", ["b"]), _n("b", ["a"])])


def test_three_node_cycle_detected():
    with pytest.raises(GraphError, match="cycle"):
        validate_graph([_n("a", ["c"]), _n("b", ["a"]), _n("c", ["b"])])


def test_ready_nodes_respects_dependencies():
    nodes = [_n("a", status="done"), _n("b", ["a"]), _n("c", ["b"])]
    ready = [n.id for n in ready_nodes(nodes)]
    assert ready == ["b"]  # a is done, c still waits on b


def test_ready_nodes_excludes_running_and_terminal():
    nodes = [_n("a", status="running"), _n("b", status="done"), _n("c")]
    ready = [n.id for n in ready_nodes(nodes)]
    assert ready == ["c"]


def test_propagate_skips_marks_descendants():
    nodes = [_n("a", status="failed"), _n("b", ["a"]), _n("c", ["b"]), _n("d")]
    skipped = propagate_skips(nodes)
    assert set(skipped) == {"b", "c"}
    assert {n.id: n.status for n in nodes}["d"] == "pending"  # unrelated untouched
