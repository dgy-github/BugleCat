"""Layer 4: OrchestratorLoop — verification gate, retries, breaker, plan validation."""

from __future__ import annotations

import json
from pathlib import Path

from nanocodex.agent.orchestrator import OrchestratorLoop
from nanocodex.agent.store import AgentStateStore
from nanocodex.provider.base import ModelResponse, ToolCall
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext


def _ctx(tmp_path: Path) -> ToolContext:
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req) -> bool:
        return True

    return ToolContext(
        workspace=tmp_path,
        policy=policy,
        approver=Approver(ON_REQUEST, auto_yes),
        executor=make_executor(policy),
        plan=[],
    )


def _plan_json(nodes) -> str:
    return json.dumps({"constraints": [], "nodes": nodes})


def _good_worker_turn(messages):
    """A compliant worker: checkpoint + request_verification, then finish.

    Detects whether tools have already run this turn (a tool message exists);
    if so it ends the turn, otherwise it issues its two tool calls.
    """
    if any(m.get("role") == "tool" for m in messages):
        return ModelResponse(content="Done with the node.")
    return ModelResponse(
        content="",
        tool_calls=[
            ToolCall(id="c1", name="write_checkpoint",
                     arguments={"summary": "made the edit", "tests_run": ["pytest"]}),
            ToolCall(id="c2", name="request_verification",
                     arguments={"claims": ["it works"], "how_to_check": "pytest"}),
        ],
        finish_reason="tool_calls",
    )


def _silent_worker_turn(messages):
    """A worker that NEVER requests verification — the fake-done path."""
    return ModelResponse(content="I believe this is complete.")


class RoutingProvider:
    """Routes chat() by the session's system prompt to a planner/worker/verifier."""

    model = "routing"
    supports_streaming = False

    def __init__(self, *, planner, worker, verifier):
        self._planner = list(planner)
        self._worker = worker  # callable(messages) -> ModelResponse
        self._verifier = list(verifier)
        self.planner_calls = 0

    async def chat(self, messages, tools=None, **kwargs):
        sys = str(messages[0].get("content", ""))
        if "PLANNER" in sys:
            self.planner_calls += 1
            item = self._planner.pop(0)
            return item if isinstance(item, ModelResponse) else ModelResponse(content=item)
        if "INDEPENDENT verifier" in sys:
            return self._verifier.pop(0) if self._verifier else ModelResponse(
                content='{"status":"fail_with_action"}')
        return self._worker(messages)


def _orch(tmp_path, provider):
    store = AgentStateStore(tmp_path / ".nanocodex")
    return OrchestratorLoop(provider, _ctx(tmp_path), store, worker_max_iterations=6)


async def test_happy_path_completes(tmp_path):
    provider = RoutingProvider(
        planner=[_plan_json([
            {"id": "n1", "kind": "code", "title": "edit", "acceptance": ["works"]}
        ])],
        worker=_good_worker_turn,
        verifier=[ModelResponse(content='{"status":"pass","summary":"verified"}')],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "completed"
    assert result.state.node("n1").status == "done"


async def test_fake_done_is_blocked_by_gate(tmp_path):
    """Worker stops without requesting verification -> never reaches done."""
    provider = RoutingProvider(
        planner=[_plan_json([
            {"id": "n1", "kind": "code", "title": "edit",
             "acceptance": ["works"], "max_retries": 0}
        ])],
        worker=_silent_worker_turn,
        verifier=[],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "failed"
    assert result.state.node("n1").status == "failed"
    # The verifier was never even consulted — the gate fired first.
    assert result.state.latest_verify_for("n1") is None


async def test_verifier_rejection_then_retry_passes(tmp_path):
    provider = RoutingProvider(
        planner=[_plan_json([
            {"id": "n1", "kind": "code", "title": "edit",
             "acceptance": ["works"], "max_retries": 1}
        ])],
        worker=_good_worker_turn,
        verifier=[
            ModelResponse(content='{"status":"fail_with_action","next_actions":["fix the off-by-one"]}'),
            ModelResponse(content='{"status":"pass","summary":"now correct"}'),
        ],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "completed"
    assert result.state.node("n1").status == "done"
    assert result.state.node("n1").retries == 1
    # Two verdicts recorded (reject then pass).
    assert len(result.state.verify_results) == 2


async def test_verifier_blocked_stalls(tmp_path):
    provider = RoutingProvider(
        planner=[_plan_json([
            {"id": "n1", "kind": "code", "title": "edit", "acceptance": ["works"]}
        ])],
        worker=_good_worker_turn,
        verifier=[ModelResponse(content='{"status":"blocked","summary":"cannot run tests"}')],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "stalled"
    assert result.state.node("n1").status == "blocked"


async def test_invalid_plan_is_rejected(tmp_path):
    """Planner keeps emitting a cyclic graph -> plan_failed, never executed."""
    cyclic = _plan_json([
        {"id": "a", "kind": "code", "title": "a", "depends_on": ["b"], "acceptance": ["x"]},
        {"id": "b", "kind": "code", "title": "b", "depends_on": ["a"], "acceptance": ["x"]},
    ])
    provider = RoutingProvider(
        planner=[cyclic, cyclic, cyclic],  # plan_retries=2 -> 3 attempts
        worker=_good_worker_turn,
        verifier=[],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "plan_failed"
    assert provider.planner_calls == 3


async def test_failed_node_skips_dependents(tmp_path):
    provider = RoutingProvider(
        planner=[_plan_json([
            {"id": "n1", "kind": "code", "title": "edit",
             "acceptance": ["works"], "max_retries": 0},
            {"id": "n2", "kind": "code", "title": "depends on n1",
             "depends_on": ["n1"], "acceptance": ["works"]},
        ])],
        worker=_silent_worker_turn,  # n1 fails the gate
        verifier=[],
    )
    result = await _orch(tmp_path, provider).run("do the thing")
    assert result.status == "failed"
    assert result.state.node("n1").status == "failed"
    assert result.state.node("n2").status == "skipped"  # not pending, not failed
