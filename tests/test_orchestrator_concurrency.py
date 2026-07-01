"""Layer 5: concurrent research + fact reconciliation + visual verification."""

from __future__ import annotations

import json
from pathlib import Path

from nanocodex.agent.orchestrator import OrchestratorLoop
from nanocodex.agent.store import AgentStateStore
from nanocodex.agent.verifier import Verifier
from nanocodex.provider.base import ModelResponse, ToolCall
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry


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


def _title_of(messages) -> str:
    brief = ""
    for m in messages:
        if m.get("role") == "user":
            brief = str(m.get("content", ""))
    for line in brief.splitlines():
        if line.startswith("# Task node"):
            return line.split(":", 1)[1].strip()
    return ""


def _worker(messages):
    """Routes by role: research records a fact = its node title; code self-verifies."""
    sys = str(messages[0].get("content", ""))
    if any(m.get("role") == "tool" for m in messages):
        return ModelResponse(content="Done.")
    if "RESEARCH worker" in sys:
        return ModelResponse(
            content="",
            tool_calls=[ToolCall(id="f1", name="record_fact",
                                 arguments={"fact": _title_of(messages)})],
            finish_reason="tool_calls",
        )
    return ModelResponse(
        content="",
        tool_calls=[
            ToolCall(id="c1", name="write_checkpoint", arguments={"summary": "did it"}),
            ToolCall(id="c2", name="request_verification",
                     arguments={"claims": ["ok"], "how_to_check": "pytest"}),
        ],
        finish_reason="tool_calls",
    )


class RoutingProvider:
    model = "routing"
    supports_streaming = False

    def __init__(self, *, planner, worker, verifier):
        self._planner = list(planner)
        self._worker = worker
        self._verifier = list(verifier)

    async def chat(self, messages, tools=None, **kwargs):
        sys = str(messages[0].get("content", ""))
        if "PLANNER" in sys:
            item = self._planner.pop(0)
            return item if isinstance(item, ModelResponse) else ModelResponse(content=item)
        if "INDEPENDENT verifier" in sys:
            return self._verifier.pop(0) if self._verifier else ModelResponse(
                content='{"status":"pass"}')
        return self._worker(messages)


async def test_concurrent_research_conflict_routes_to_clarification(tmp_path):
    plan = json.dumps({"constraints": [], "nodes": [
        {"id": "r1", "kind": "research", "title": "the test runner is pytest"},
        {"id": "r2", "kind": "research", "title": "the test runner is unittest"},
    ]})
    provider = RoutingProvider(planner=[plan], worker=_worker, verifier=[])
    store = AgentStateStore(tmp_path / ".nanocodex")
    orch = OrchestratorLoop(provider, _ctx(tmp_path), store, worker_max_iterations=6)
    result = await orch.run("investigate")

    # Both research nodes ran and recorded conflicting facts.
    assert result.state.node("r1").status == "done"
    assert result.state.node("r2").status == "done"
    # The conflict was reconciled: facts disputed + a clarification node added.
    assert any(f.disputed for f in result.state.repo_facts)
    clar = [n for n in result.state.current_plan if n.status == "needs_clarification"]
    assert clar, "expected a needs_clarification node for the conflict"
    # A run with an unresolved clarification stalls, not silently completes.
    assert result.status == "stalled"


# --- visual verification ----------------------------------------------------

class RecordingProvider:
    model = "recording"
    supports_streaming = False

    def __init__(self, verdict: str):
        self._verdict = verdict
        self.calls = []

    async def chat(self, messages, tools=None, **kwargs):
        self.calls.append(list(messages))
        return ModelResponse(content=self._verdict)


async def test_visual_verification_routes_to_vision_provider(tmp_path):
    from nanocodex.agent.state import TaskNode

    text_provider = RecordingProvider('{"status":"fail_with_action"}')
    vision_provider = RecordingProvider('{"status":"pass","summary":"looks right"}')
    verifier = Verifier(
        text_provider, ToolRegistry(_ctx(tmp_path)),
        max_iterations=2, vision_provider=vision_provider,
    )
    node = TaskNode(id="v1", kind="vision", title="render", acceptance=["matches mockup"])
    node.inputs["images"] = ["data:image/png;base64,iVBORw0KGgo="]

    verdict = await verifier.verify(node)
    # The image-bearing turn routed to the vision backend, not the text model.
    assert verdict.passed
    assert vision_provider.calls
    assert not text_provider.calls
    # And the image block actually reached the vision model.
    user_msg = vision_provider.calls[0][-1]
    blocks = user_msg["content"]
    assert any(b.get("type") == "image_url" for b in blocks)
