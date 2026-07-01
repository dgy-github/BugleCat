"""Smoke test for the agent turn loop with a scripted (mocked) provider."""

from __future__ import annotations

from pathlib import Path

from nanocodex.agent.loop import AgentLoop
from nanocodex.agent.session import Session
from nanocodex.provider.base import ModelResponse, ToolCall
from nanocodex.sandbox.approval import ON_REQUEST, Approver
from nanocodex.sandbox.executor import make_executor
from nanocodex.sandbox.policy import WORKSPACE_WRITE, SandboxPolicy
from nanocodex.tools import ToolContext, ToolRegistry


class ScriptedProvider:
    """Returns a pre-scripted sequence of ModelResponses, one per chat() call."""

    model = "scripted"

    def __init__(self, responses: list[ModelResponse]) -> None:
        self._responses = list(responses)
        self.calls: list[list[dict]] = []

    async def chat(self, messages, tools=None, **kwargs):
        self.calls.append(list(messages))
        if not self._responses:
            return ModelResponse(content="(no more scripted responses)")
        return self._responses.pop(0)


def _build(tmp_path: Path, provider) -> AgentLoop:
    policy = SandboxPolicy(mode=WORKSPACE_WRITE, workspace=tmp_path)

    async def auto_yes(_req) -> bool:
        return True

    ctx = ToolContext(
        workspace=tmp_path,
        policy=policy,
        approver=Approver(ON_REQUEST, auto_yes),
        executor=make_executor(policy),
        plan=[],
    )
    tools = ToolRegistry(ctx)
    session = Session("system prompt", log_path=None)
    return AgentLoop(provider, tools, session, max_iterations=10)


async def test_loop_returns_final_text_without_tools(tmp_path):
    provider = ScriptedProvider([ModelResponse(content="All done.")])
    loop = _build(tmp_path, provider)
    result = await loop.run_turn("say hi")
    assert result.stop_reason == "completed"
    assert result.final_text == "All done."
    assert result.iterations == 1


async def test_loop_executes_apply_patch_then_finishes(tmp_path):
    patch = (
        "*** Begin Patch\n"
        "*** Add File: out.txt\n"
        "+hello\n"
        "*** End Patch"
    )
    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[ToolCall(id="c1", name="apply_patch", arguments={"patch": patch})],
            finish_reason="tool_calls",
        ),
        ModelResponse(content="Created out.txt."),
    ])
    loop = _build(tmp_path, provider)
    result = await loop.run_turn("create out.txt with hello")

    assert (tmp_path / "out.txt").read_text() == "hello\n"
    assert result.stop_reason == "completed"
    assert "apply_patch" in result.tools_used
    # Second model call should have seen the tool result in the message history.
    second_call_msgs = provider.calls[1]
    assert any(m.get("role") == "tool" for m in second_call_msgs)


async def test_loop_persists_reasoning_content_on_tool_call_turn(tmp_path):
    patch = (
        "*** Begin Patch\n"
        "*** Add File: reasoned.txt\n"
        "+ok\n"
        "*** End Patch"
    )
    provider = ScriptedProvider([
        ModelResponse(
            content="",
            reasoning="I need to create a file before answering.",
            tool_calls=[ToolCall(id="c1", name="apply_patch", arguments={"patch": patch})],
            finish_reason="tool_calls",
        ),
        ModelResponse(content="Created reasoned.txt."),
    ])
    loop = _build(tmp_path, provider)
    await loop.run_turn("create reasoned.txt")

    assistant_tool_msgs = [
        m for m in loop.session.messages
        if m.get("role") == "assistant" and m.get("tool_calls")
    ]
    assert assistant_tool_msgs
    assert assistant_tool_msgs[0]["reasoning_content"] == "I need to create a file before answering."


async def test_loop_runs_update_plan_and_records_state(tmp_path):
    provider = ScriptedProvider([
        ModelResponse(
            content="planning",
            tool_calls=[ToolCall(
                id="p1",
                name="update_plan",
                arguments={"plan": [
                    {"step": "write file", "status": "in_progress"},
                    {"step": "verify", "status": "pending"},
                ]},
            )],
            finish_reason="tool_calls",
        ),
        ModelResponse(content="done"),
    ])
    loop = _build(tmp_path, provider)
    result = await loop.run_turn("do a two step task")
    assert result.stop_reason == "completed"
    assert loop.tools.ctx.plan[0]["step"] == "write file"
    assert loop.tools.ctx.plan[0]["status"] == "in_progress"


async def test_loop_stops_at_max_iterations(tmp_path):
    # Always returns a tool call -> never finishes -> hits the cap.
    looping = [
        ModelResponse(
            content="",
            tool_calls=[ToolCall(id=f"c{i}", name="read_file", arguments={"path": "nope.txt"})],
            finish_reason="tool_calls",
        )
        for i in range(20)
    ]
    provider = ScriptedProvider(looping)
    loop = _build(tmp_path, provider)
    result = await loop.run_turn("loop forever")
    assert result.stop_reason == "max_iterations"
    assert result.iterations == 10


def _tool_call_message_is_answered(messages: list[dict]) -> bool:
    """Every assistant tool_call id has a matching tool message after it.

    This is exactly the backend contract whose violation raised the 400:
    'tool_calls must be followed by tool messages'.
    """
    answered = {
        m.get("tool_call_id")
        for m in messages
        if m.get("role") == "tool"
    }
    for m in messages:
        if m.get("role") == "assistant" and m.get("tool_calls"):
            for tc in m["tool_calls"]:
                if tc.get("id") not in answered:
                    return False
    return True


async def test_cancel_mid_tool_loop_backfills_tool_results(tmp_path):
    # Model asks for two tool calls; user hits Stop before they run. The
    # assistant tool_calls message is already recorded, so without backfill the
    # history has dangling tool_calls -> next request 400s. Verify the loop
    # leaves a valid history (each tool_call answered).
    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[
                ToolCall(id="c1", name="read_file", arguments={"path": "a.txt"}),
                ToolCall(id="c2", name="read_file", arguments={"path": "b.txt"}),
            ],
            finish_reason="tool_calls",
        ),
    ])
    loop = _build(tmp_path, provider)

    # False at the top-of-iteration check, True once we're inside the tool loop.
    calls = {"n": 0}

    def cancel_check() -> bool:
        n = calls["n"]
        calls["n"] += 1
        return n >= 1

    result = await loop.run_turn("read two files", cancel_check=cancel_check)
    assert result.stop_reason == "cancelled"
    # The whole session history must satisfy the tool_calls/tool pairing rule.
    assert _tool_call_message_is_answered(loop.session.messages)
    # Both unanswered calls were backfilled.
    tool_msgs = [m for m in loop.session.messages if m.get("role") == "tool"]
    assert {m["tool_call_id"] for m in tool_msgs} == {"c1", "c2"}


async def test_cancel_after_some_tools_ran_backfills_only_remaining(tmp_path):
    # First tool runs, then cancel before the second. The completed tool keeps
    # its real result; only the un-run one is backfilled (no duplicates).
    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[
                ToolCall(id="c1", name="read_file", arguments={"path": "a.txt"}),
                ToolCall(id="c2", name="read_file", arguments={"path": "b.txt"}),
            ],
            finish_reason="tool_calls",
        ),
    ])
    loop = _build(tmp_path, provider)

    # Allow: top check (False), first tc (False, runs), second tc (True, stop).
    calls = {"n": 0}

    def cancel_check() -> bool:
        n = calls["n"]
        calls["n"] += 1
        return n >= 2

    result = await loop.run_turn("read two files", cancel_check=cancel_check)
    assert result.stop_reason == "cancelled"
    assert _tool_call_message_is_answered(loop.session.messages)
    tool_msgs = [m for m in loop.session.messages if m.get("role") == "tool"]
    # Exactly one result per call id, no duplicates.
    assert sorted(m["tool_call_id"] for m in tool_msgs) == ["c1", "c2"]


async def test_image_turn_routes_to_vision_provider(tmp_path):
    # B 分流: a turn carrying an image_url block must go to the vision provider;
    # the main (text) provider must NOT be called for that turn.
    main = ScriptedProvider([ModelResponse(content="text-provider reply")])
    vision = ScriptedProvider([ModelResponse(content="vision reply: I see a cat")])
    loop = _build(tmp_path, main)
    loop.vision_provider = vision

    content = [
        {"type": "text", "text": "what's in this image?"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
    ]
    result = await loop.run_turn(content)
    assert result.stop_reason == "completed"
    assert result.final_text == "vision reply: I see a cat"
    # The vision provider handled it; the main provider was untouched this turn.
    assert len(vision.calls) == 1
    assert len(main.calls) == 0


async def test_text_turn_stays_on_main_provider_even_with_vision_configured(tmp_path):
    # A plain-text turn must stay on the main provider even when a vision
    # backend is configured (only image-bearing turns route to VL).
    main = ScriptedProvider([ModelResponse(content="main reply")])
    vision = ScriptedProvider([ModelResponse(content="should not be called")])
    loop = _build(tmp_path, main)
    loop.vision_provider = vision

    result = await loop.run_turn("just text, no image")
    assert result.final_text == "main reply"
    assert len(main.calls) == 1
    assert len(vision.calls) == 0


async def test_read_only_calls_run_concurrently(tmp_path):
    # A3: a run of consecutive read-only tool calls runs in parallel, so N slow
    # reads finish in ~one read's time, not N times it. A fake read_only tool
    # sleeps, so wall-clock proves concurrency.
    import asyncio
    import time as _time

    from nanocodex.tools.base import Tool

    class SlowReadTool(Tool):
        read_only = True

        @property
        def name(self):
            return "slow_read"

        @property
        def description(self):
            return "A read-only tool that sleeps (test only)."

        @property
        def parameters(self):
            return {"type": "object", "properties": {"i": {"type": "integer"}}}

        async def execute(self, **kwargs):
            await asyncio.sleep(0.3)
            return f"read {kwargs.get('i')}"

    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[
                ToolCall(id=f"c{i}", name="slow_read", arguments={"i": i})
                for i in range(4)
            ],
            finish_reason="tool_calls",
        ),
        ModelResponse(content="done"),
    ])
    loop = _build(tmp_path, provider)
    loop.tools.register(SlowReadTool(loop.tools.ctx))

    t0 = _time.monotonic()
    result = await loop.run_turn("read four things")
    elapsed = _time.monotonic() - t0

    assert result.stop_reason == "completed"
    # 4 reads x 0.3s serial would be ~1.2s; concurrent should be well under.
    assert elapsed < 0.8
    # Every read produced a result, in order (results keyed by call id).
    tool_msgs = [m for m in loop.session.messages if m.get("role") == "tool"]
    assert [m["tool_call_id"] for m in tool_msgs] == ["c0", "c1", "c2", "c3"]


async def test_write_between_reads_stays_serial_and_ordered(tmp_path):
    # A read-only call, then a write (apply_patch), then a read. The write must
    # NOT be batched/parallelized with the reads around it: each segment runs in
    # issue order so a read never races the write. Result order is preserved.
    patch = (
        "*** Begin Patch\n"
        "*** Add File: mid.txt\n"
        "+x\n"
        "*** End Patch"
    )
    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[
                ToolCall(id="r1", name="read_file", arguments={"path": "none1.txt"}),
                ToolCall(id="w1", name="apply_patch", arguments={"patch": patch}),
                ToolCall(id="r2", name="read_file", arguments={"path": "none2.txt"}),
            ],
            finish_reason="tool_calls",
        ),
        ModelResponse(content="done"),
    ])
    loop = _build(tmp_path, provider)
    result = await loop.run_turn("read, write, read")

    assert result.stop_reason == "completed"
    assert (tmp_path / "mid.txt").read_text() == "x\n"
    tool_msgs = [m for m in loop.session.messages if m.get("role") == "tool"]
    assert [m["tool_call_id"] for m in tool_msgs] == ["r1", "w1", "r2"]
    assert result.tools_used == ["read_file", "apply_patch", "read_file"]


async def test_stop_interrupts_a_hanging_tool(tmp_path):
    # A tool that never returns on its own. Stop must abandon it and end the
    # turn as 'cancelled' instead of blocking forever — the "Stop 停止不了" bug.
    import asyncio

    from nanocodex.tools.base import Tool

    class HangingTool(Tool):
        @property
        def name(self):
            return "hang"

        @property
        def description(self):
            return "Blocks forever (test only)."

        @property
        def parameters(self):
            return {"type": "object", "properties": {}}

        async def execute(self, **kwargs):
            await asyncio.Event().wait()   # never completes
            return "unreachable"

    provider = ScriptedProvider([
        ModelResponse(
            content="",
            tool_calls=[ToolCall(id="h1", name="hang", arguments={})],
            finish_reason="tool_calls",
        ),
    ])
    loop = _build(tmp_path, provider)
    loop.tools.register(HangingTool(loop.tools.ctx))

    # cancel_check is False for the top-of-iteration and pre-tool checks (so the
    # tool actually starts), then True once we're polling during execution, so
    # Stop lands mid-command.
    calls = {"n": 0}

    def cancel_check() -> bool:
        n = calls["n"]
        calls["n"] += 1
        return n >= 2

    result = await asyncio.wait_for(
        loop.run_turn("do the hang", cancel_check=cancel_check),
        timeout=5.0,   # must finish well under this despite the infinite tool
    )
    assert result.stop_reason == "cancelled"
    # History stays valid: the hung tool_call got a synthetic tool result.
    tool_msgs = [m for m in loop.session.messages if m.get("role") == "tool"]
    assert any(m.get("tool_call_id") == "h1" for m in tool_msgs)
