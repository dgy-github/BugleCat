"""Layer 2: checkpoint / verification / fact tools + verifier independence."""

from __future__ import annotations

from pathlib import Path

from nanocodex.agent.state import AgentState, TaskNode
from nanocodex.agent.store import AgentStateStore
from nanocodex.agent.verifier import Verifier, parse_verdict
from nanocodex.provider.base import ModelResponse
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry
from nanocodex.tools.record_fact import RecordFactTool
from nanocodex.tools.request_verification import RequestVerificationTool
from nanocodex.tools.write_checkpoint import WriteCheckpointTool


def _ctx(tmp_path: Path, state: AgentState | None = None, node_id: str | None = None):
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req) -> bool:
        return True

    return ToolContext(
        workspace=tmp_path,
        policy=policy,
        approver=Approver(ON_REQUEST, auto_yes),
        executor=make_executor(policy),
        plan=[],
        agent_state=state,
        current_node_id=node_id,
    )


async def test_write_checkpoint_persists_and_updates_state(tmp_path):
    state = AgentState(goal="g", current_plan=[TaskNode(id="n1", kind="code", title="t")])
    tool = WriteCheckpointTool(_ctx(tmp_path, state, "n1"))
    out = await tool.execute(summary="did it", files_touched=["a.py"], tests_run=["pytest"])
    assert "recorded" in out.lower()
    assert len(state.checkpoints) == 1
    # Persisted to disk and reloadable.
    loaded = AgentStateStore(tmp_path / ".nanocodex").load()
    assert loaded.checkpoints[0].summary == "did it"


async def test_write_checkpoint_noop_without_state(tmp_path):
    tool = WriteCheckpointTool(_ctx(tmp_path, None, None))
    out = await tool.execute(summary="x")
    assert "not persisted" in out.lower()


async def test_request_verification_flips_node_to_verify(tmp_path):
    state = AgentState(goal="g", current_plan=[TaskNode(id="n1", kind="code", title="t")])
    tool = RequestVerificationTool(_ctx(tmp_path, state, "n1"))
    out = await tool.execute(claims=["builds clean"], how_to_check="run pytest")
    assert state.node("n1").status == "verify"
    assert "verification" in out.lower()


async def test_request_verification_rejects_empty_claims(tmp_path):
    state = AgentState(goal="g", current_plan=[TaskNode(id="n1", kind="code", title="t")])
    tool = RequestVerificationTool(_ctx(tmp_path, state, "n1"))
    out = await tool.execute(claims=[])
    assert out.startswith("Error")
    assert state.node("n1").status == "pending"  # unchanged


async def test_record_fact_appends(tmp_path):
    state = AgentState(goal="g")
    tool = RecordFactTool(_ctx(tmp_path, state, "r1"))
    await tool.execute(fact="repo uses pytest")
    assert state.repo_facts[0].text == "repo uses pytest"
    assert state.repo_facts[0].source_node == "r1"


# --- verifier ---------------------------------------------------------------

class ScriptedProvider:
    model = "scripted"

    def __init__(self, responses):
        self._responses = list(responses)
        self.calls = []
        self.supports_streaming = False

    async def chat(self, messages, tools=None, **kwargs):
        self.calls.append(list(messages))
        return self._responses.pop(0) if self._responses else ModelResponse(content="{}")


def _ro_registry(tmp_path):
    return ToolRegistry(_ctx(tmp_path))


async def test_verifier_uses_fresh_session_without_worker_history(tmp_path):
    provider = ScriptedProvider([
        ModelResponse(content='{"status":"pass","summary":"ok","evidence":["ran pytest: 3 passed"]}')
    ])
    verifier = Verifier(provider, _ro_registry(tmp_path), max_iterations=3)
    node = TaskNode(id="n1", kind="code", title="t", acceptance=["tests pass"])
    node.outputs["verification_request"] = {"claims": ["tests pass"], "how_to_check": "pytest"}

    verdict = await verifier.verify(node)
    assert verdict.passed
    assert verdict.reask_count == 0  # clean verdict on first try
    # Independence: the verifier's first model call sees only its own system
    # prompt + the brief — no worker reasoning leaked in.
    msgs = provider.calls[0]
    assert msgs[0]["role"] == "system"
    assert "INDEPENDENT verifier" in msgs[0]["content"]
    joined = "\n".join(str(m.get("content", "")) for m in msgs)
    assert "WORKER_SECRET_REASONING" not in joined


async def test_verifier_non_json_fails_conservatively(tmp_path):
    provider = ScriptedProvider([ModelResponse(content="Looks good to me, ship it!")])
    verifier = Verifier(provider, _ro_registry(tmp_path), max_iterations=2)
    node = TaskNode(id="n1", kind="code", title="t", acceptance=["x"])
    verdict = await verifier.verify(node)
    assert verdict.status == "fail_with_action"  # never a pass


async def test_verifier_reask_recovers_clean_verdict(tmp_path):
    """A first unparseable reply is re-asked in-session; a valid verdict follows."""
    provider = ScriptedProvider([
        ModelResponse(content="Sure, looks correct to me!"),  # not JSON
        ModelResponse(content='{"status":"pass","summary":"confirmed"}'),  # after re-ask
    ])
    verifier = Verifier(provider, _ro_registry(tmp_path), max_iterations=2)
    node = TaskNode(id="n1", kind="code", title="t", acceptance=["x"])
    verdict = await verifier.verify(node)
    assert verdict.passed
    assert verdict.reask_count == 1
    assert len(provider.calls) == 2  # original + one re-ask, no worker re-run


class AlwaysProseProvider:
    model = "prose"
    supports_streaming = False

    def __init__(self):
        self.calls = []

    async def chat(self, messages, tools=None, **kwargs):
        self.calls.append(list(messages))
        return ModelResponse(content="It all looks fine, ship it.")


async def test_verifier_exhausted_reask_returns_blocked(tmp_path):
    """If it never produces JSON, the node is blocked (not a worker failure)."""
    provider = AlwaysProseProvider()
    verifier = Verifier(provider, _ro_registry(tmp_path), max_iterations=2, format_retries=1)
    node = TaskNode(id="n1", kind="code", title="t", acceptance=["x"])
    verdict = await verifier.verify(node)
    assert verdict.status == "blocked"
    assert verdict.reask_count == 1
    assert len(provider.calls) == 2  # original + one re-ask, then give up


def test_parse_verdict_extracts_fenced_json():
    text = "Here is my verdict:\n```json\n{\"status\":\"pass\",\"summary\":\"good\"}\n```"
    v = parse_verdict("n1", text)
    assert v.status == "pass"
    assert v.node_id == "n1"


def test_parse_verdict_trailing_object():
    text = 'reasoning... {"status":"fail_with_action","next_actions":["fix import"]}'
    v = parse_verdict("n1", text)
    assert v.status == "fail_with_action"
    assert v.next_actions == ["fix import"]


def test_parse_verdict_unknown_status_not_pass():
    v = parse_verdict("n1", '{"status":"looks_fine"}')
    assert v.status == "fail_with_action"
