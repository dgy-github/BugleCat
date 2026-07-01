"""The agent turn loop: call model, run tools, feed results, repeat until done."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable

from nanocodex.agent.auto_reasoning import resolve_effort
from nanocodex.agent.compaction import CompactionConfig, compact
from nanocodex.agent.pricing import add_usage
from nanocodex.provider.base import Provider, ToolCall
from nanocodex.tools import ToolRegistry

# Hooks let the CLI render activity without the loop knowing about the console.
OnAssistantText = Callable[[str], Awaitable[None]]
OnToolStart = Callable[[ToolCall], Awaitable[None]]
OnToolResult = Callable[[str, str], Awaitable[None]]  # (tool_name, result)
OnContentDelta = Callable[[str], Awaitable[None]]
OnReasoningDelta = Callable[[str], Awaitable[None]]
OnStreamEnd = Callable[[], Awaitable[None]]


@dataclass
class LoopHooks:
    on_assistant_text: OnAssistantText | None = None
    on_tool_start: OnToolStart | None = None
    on_tool_result: OnToolResult | None = None
    # Streaming hooks. When on_content_delta is set and the provider supports
    # streaming, the loop streams tokens instead of waiting for the full reply.
    on_content_delta: OnContentDelta | None = None
    on_reasoning_delta: OnReasoningDelta | None = None
    on_stream_end: OnStreamEnd | None = None

    @property
    def wants_streaming(self) -> bool:
        return self.on_content_delta is not None or self.on_reasoning_delta is not None


@dataclass
class TurnResult:
    final_text: str
    iterations: int
    stop_reason: str
    tools_used: list[str] = field(default_factory=list)
    # Accumulated token usage across every model call this turn (a turn can
    # make many calls — one per tool-use round). Summed via pricing.add_usage.
    usage: dict[str, int] = field(default_factory=dict)


class AgentLoop:
    """Drive one user turn to completion."""

    def __init__(
        self,
        provider: Provider,
        tools: ToolRegistry,
        session,  # nanocodex.agent.session.Session
        *,
        max_iterations: int = 60,
        reasoning_effort: str | None = None,
        compaction: "CompactionConfig | None" = None,
        vision_provider: Provider | None = None,
    ) -> None:
        self.provider = provider
        self.tools = tools
        self.session = session
        self.max_iterations = max_iterations
        self.reasoning_effort = reasoning_effort
        self.compaction = compaction or CompactionConfig()
        # Optional vision backend. When set AND a turn carries an image, that
        # turn's model calls route here instead of `provider` (text/coding stays
        # on the main model; only image-bearing turns hit the VL model). The
        # routing flag is set per-turn in run_turn and read in _call_model.
        self.vision_provider = vision_provider
        self._use_vision_this_turn = False

    def _active_provider(self) -> Provider:
        """The provider for THIS turn: the vision backend when routing, else main.

        Set per-turn in run_turn (`_use_vision_this_turn`) so only image-bearing
        turns hit the VL model; text/coding turns keep using the main provider.
        """
        if self._use_vision_this_turn and self.vision_provider is not None:
            return self.vision_provider
        return self.provider

    def _streaming_active(self, hooks: LoopHooks) -> bool:
        """True when the caller wants streaming and the active provider can do it."""
        return hooks.wants_streaming and getattr(
            self._active_provider(), "supports_streaming", False
        )

    async def _prepared_messages(self) -> list[dict[str, Any]]:
        """Session messages, compacted to the token budget when configured."""
        messages = self.session.for_model()
        if self.compaction.enabled:
            messages = await compact(messages, self.compaction)
        return messages

    async def _call_model(self, hooks: LoopHooks, schemas: list[dict[str, Any]]):
        """Call the provider, streaming deltas through hooks when enabled."""
        messages = await self._prepared_messages()
        # Resolve a configured "auto" tier to a concrete one from the last user
        # message (DeepSeek-TUI #663). An explicit tier or None passes through
        # unchanged, so a user who pins a tier keeps it.
        effort = resolve_effort(self.reasoning_effort, messages)
        provider = self._active_provider()
        if self._streaming_active(hooks):
            response = await provider.chat_stream(
                messages,
                tools=schemas,
                reasoning_effort=effort,
                on_content_delta=hooks.on_content_delta,
                on_reasoning_delta=hooks.on_reasoning_delta,
            )
            if hooks.on_stream_end is not None:
                await hooks.on_stream_end()
            return response
        return await provider.chat(
            messages,
            tools=schemas,
            reasoning_effort=effort,
        )

    def _is_parallel_safe(self, tc: ToolCall) -> bool:
        """True when this call's tool is read-only (safe to run concurrently)."""
        tool = self.tools.get(tc.name)
        return bool(tool and getattr(tool, "read_only", False))

    async def _execute_cancellable(self, tc, cancelled) -> str:
        """Run one tool call, but abandon (and let it be killed) on Stop.

        Tool execution can block for a long time — a hung shell command, a
        launched process that never returns. Cooperative cancellation between
        iterations isn't enough then: the user presses Stop and nothing happens
        until the command finishes. So we run the tool as a task and poll the
        cancel flag; when it flips we cancel the task. The shell executor turns
        that CancelledError into a real subprocess kill, so the command dies
        instead of lingering.
        """
        task = asyncio.ensure_future(self.tools.execute(tc.name, tc.arguments))
        while True:
            done, _ = await asyncio.wait({task}, timeout=0.1)
            if task in done:
                return task.result()
            if cancelled():
                task.cancel()
                try:
                    await task
                except asyncio.CancelledError:
                    pass
                except Exception:  # noqa: BLE001 - tool teardown error, ignore
                    pass
                return "[interrupted: stopped by user mid-command]"

    async def run_turn(
        self,
        user_input: "str | list[dict[str, Any]]",
        hooks: LoopHooks | None = None,
        cancel_check: "Callable[[], bool] | None" = None,
    ) -> TurnResult:
        hooks = hooks or LoopHooks()
        # Route THIS turn to the vision backend only when the input carries an
        # image block AND a vision provider is configured. Text/coding turns —
        # and image turns with no VL configured — stay on the main provider.
        self._use_vision_this_turn = (
            self.vision_provider is not None and _has_image_block(user_input)
        )
        self.session.add_user(user_input)
        tools_used: list[str] = []
        schemas = self.tools.schemas()
        # Real token usage summed across every model call this turn. DeepSeek
        # returns usage per call, and a single turn can call the model many
        # times (once per tool-call round), so accumulate to price the whole
        # turn. Stays a plain dict so TurnResult carries it back unmodified.
        turn_usage: dict[str, int] = {}

        def _cancelled() -> bool:
            return bool(cancel_check and cancel_check())

        for iteration in range(self.max_iterations):
            # Cooperative cancellation: a Python thread can't be force-killed,
            # so we check a flag at the top of each iteration and exit cleanly,
            # keeping whatever was already done this turn.
            if _cancelled():
                text = "Stopped by user."
                self.session.add_assistant(text)
                return TurnResult(text, iteration + 1, "cancelled", tools_used, turn_usage)

            response = await self._call_model(hooks, schemas)
            # Accumulate usage right after the call so every return path below
            # (completed / error / cancelled / max_iterations) reports the cost
            # incurred up to that point, not just on the happy path.
            turn_usage = add_usage(turn_usage, response.usage)

            if response.finish_reason == "error":
                text = response.content or "Model call failed."
                self.session.add_assistant(text)
                return TurnResult(text, iteration + 1, "error", tools_used, turn_usage)

            if not response.has_tool_calls:
                text = response.content or ""
                self.session.add_assistant(text, reasoning=response.reasoning)
                # When streaming, the text was already emitted token-by-token;
                # only push the whole reply in non-streaming mode.
                if hooks.on_assistant_text and text and not self._streaming_active(hooks):
                    await hooks.on_assistant_text(text)
                return TurnResult(text, iteration + 1, "completed", tools_used, turn_usage)

            # Persist the assistant message that carries the tool calls.
            openai_tool_calls = [
                {
                    "id": tc.id,
                    "type": "function",
                    "function": {"name": tc.name, "arguments": _dump_args(tc.arguments)},
                }
                for tc in response.tool_calls
            ]
            self.session.add_assistant(
                response.content or "",
                tool_calls=openai_tool_calls,
                reasoning=response.reasoning,
            )
            if hooks.on_assistant_text and response.content and not self._streaming_active(hooks):
                await hooks.on_assistant_text(response.content)

            calls = response.tool_calls

            def _cancelled_result(*, before: bool) -> TurnResult:
                # The assistant tool_calls message is already in the session;
                # some calls may be unanswered. Backfill synthetic results so the
                # history stays valid (every tool_call has a tool reply) —
                # otherwise the next request 400s.
                self.session.backfill_unanswered_tool_calls(
                    "[interrupted: stopped by user before this tool ran]" if before
                    else "[interrupted: stopped by user]"
                )
                text = "Stopped by user."
                self.session.add_assistant(text)
                return TurnResult(text, iteration + 1, "cancelled", tools_used, turn_usage)

            async def _run_one(tc: ToolCall) -> None:
                tools_used.append(tc.name)
                if hooks.on_tool_start:
                    await hooks.on_tool_start(tc)
                result = await self._execute_cancellable(tc, _cancelled)
                self.session.add_tool_result(tc.id, tc.name, result)
                if hooks.on_tool_result:
                    await hooks.on_tool_result(tc.name, result)

            idx = 0
            n_calls = len(calls)
            while idx < n_calls:
                # Check cancellation BEFORE starting the next tool / batch — a
                # turn with many tool calls would otherwise run them all before
                # reaching the next iteration boundary, making Stop unresponsive.
                if _cancelled():
                    return _cancelled_result(before=True)

                # A3: run a RUN of >=2 consecutive read-only calls concurrently
                # (e.g. several read_file). A write/unknown tool stays serial and
                # in order, so a read never races a write issued in the same turn.
                if (self._is_parallel_safe(calls[idx]) and idx + 1 < n_calls
                        and self._is_parallel_safe(calls[idx + 1])):
                    batch: list[ToolCall] = []
                    while idx < n_calls and self._is_parallel_safe(calls[idx]):
                        batch.append(calls[idx])
                        idx += 1
                    for tc in batch:
                        tools_used.append(tc.name)
                        if hooks.on_tool_start:
                            await hooks.on_tool_start(tc)
                    results = await asyncio.gather(
                        *(self._execute_cancellable(tc, _cancelled) for tc in batch)
                    )
                    for tc, result in zip(batch, results):
                        self.session.add_tool_result(tc.id, tc.name, result)
                        if hooks.on_tool_result:
                            await hooks.on_tool_result(tc.name, result)
                else:
                    await _run_one(calls[idx])
                    idx += 1

                # A command can hang for a long time; if Stop was pressed while
                # it ran, honor it right after instead of looping to the next
                # tool / iteration boundary.
                if _cancelled():
                    return _cancelled_result(before=False)

        text = (
            f"Reached the maximum of {self.max_iterations} steps without finishing. "
            "The task may be incomplete."
        )
        self.session.add_assistant(text)
        return TurnResult(text, self.max_iterations, "max_iterations", tools_used, turn_usage)


def _has_image_block(user_input: "str | list[dict[str, Any]]") -> bool:
    """True when the user content carries at least one image_url block.

    A plain string is text-only. A multimodal content list (built by
    images.build_user_content) carries image_url blocks; that's the signal to
    route this turn to the vision backend.
    """
    if not isinstance(user_input, list):
        return False
    for block in user_input:
        if isinstance(block, dict) and block.get("type") == "image_url":
            return True
    return False


def _dump_args(arguments: dict[str, Any]) -> str:
    import json

    try:
        return json.dumps(arguments, ensure_ascii=False)
    except (TypeError, ValueError):
        return "{}"
