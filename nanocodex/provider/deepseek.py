"""DeepSeek provider (OpenAI-compatible chat-completions with tool calling)."""

from __future__ import annotations

import asyncio
import json
import os
from copy import deepcopy
from typing import Any, Awaitable, Callable

from openai import APIError, AsyncOpenAI

from nanocodex.provider.base import ModelResponse, ToolCall

# Streaming delta callbacks. Each receives the incremental text fragment.
OnContentDelta = Callable[[str], Awaitable[None]]
OnReasoningDelta = Callable[[str], Awaitable[None]]

_REASONING_PLACEHOLDER = "(reasoning omitted)"
_DISABLED_REASONING_EFFORTS = {"off", "disabled", "none", "false"}

# How long to wait for the streaming response *headers* before giving up.
# This is deliberately shorter than the SDK's per-request timeout because it
# only covers connection setup + upstream header return, not model thinking
# time after streaming has started. On Windows/proxy networks that initial
# wait can hang before any chunk exists, leaving the UI stuck at "thinking…";
# failing fast with a clear hint beats a silent 120s stall. Ported from
# DeepSeek-TUI's DEEPSEEK_STREAM_OPEN_TIMEOUT_SECS.
_DEFAULT_STREAM_OPEN_TIMEOUT_S = 45
_STREAM_OPEN_TIMEOUT_MIN_S = 5
_STREAM_OPEN_TIMEOUT_MAX_S = 300


def _stream_open_timeout_s() -> int:
    """Bounded override for the streaming response-header wait (seconds)."""
    raw = os.environ.get("NANOCODEX_STREAM_OPEN_TIMEOUT_S")
    if not raw:
        return _DEFAULT_STREAM_OPEN_TIMEOUT_S
    try:
        secs = int(raw)
    except (TypeError, ValueError):
        return _DEFAULT_STREAM_OPEN_TIMEOUT_S
    return max(_STREAM_OPEN_TIMEOUT_MIN_S, min(_STREAM_OPEN_TIMEOUT_MAX_S, secs))


class ProviderError(RuntimeError):
    """Raised when the backend call fails irrecoverably."""


def _extract_usage(usage: Any) -> dict[str, int]:
    """Normalize an SDK usage object into a plain int dict.

    Captures prompt/completion tokens plus DeepSeek's cache-accounting fields.
    DeepSeek returns ``prompt_cache_hit_tokens`` / ``prompt_cache_miss_tokens``
    as TOP-LEVEL usage fields (not the OpenAI ``prompt_tokens_details``), and
    the SDK's strongly-typed object hides unknown fields under ``model_extra``.
    So we read attributes first, then fall back to model_extra, so the cache
    split survives regardless of how the SDK surfaces them. Missing fields
    become 0 — the cost layer treats absent cache info as "all a miss".
    """
    if usage is None:
        return {}
    extra = getattr(usage, "model_extra", None) or {}

    def _get(name: str) -> int:
        val = getattr(usage, name, None)
        if val is None and isinstance(extra, dict):
            val = extra.get(name)
        try:
            return int(val) if val is not None else 0
        except (TypeError, ValueError):
            return 0

    out = {
        "prompt_tokens": _get("prompt_tokens"),
        "completion_tokens": _get("completion_tokens"),
    }
    hit = _get("prompt_cache_hit_tokens")
    miss = _get("prompt_cache_miss_tokens")
    # Only record cache fields when the backend actually reports them, so a
    # provider without cache accounting doesn't look like "0 hits" (which the
    # cost layer would price as an all-miss prompt anyway, but being explicit
    # keeps the data honest).
    if hit or miss:
        out["prompt_cache_hit_tokens"] = hit
        out["prompt_cache_miss_tokens"] = miss
    return out


class DeepSeekProvider:
    """Talk to DeepSeek (or any OpenAI-compatible endpoint) over the SDK."""

    # Advertise streaming capability so the loop can opt in.
    supports_streaming = True

    def __init__(
        self,
        api_key: str,
        base_url: str,
        model: str,
        *,
        timeout_s: int = 120,
        max_retries: int = 3,
    ) -> None:
        # The SDK retries transient failures itself (connection error / 408 /
        # 409 / 429 / >=500) with exponential backoff that honors Retry-After,
        # then raises APIError once exhausted. We set max_retries explicitly
        # (SDK default is 2) so a flaky/rate-limited endpoint recovers without
        # killing the turn; permanent 4xx (auth, bad-request) are never retried.
        self._client = AsyncOpenAI(
            api_key=api_key, base_url=base_url,
            timeout=timeout_s, max_retries=max_retries,
        )
        self.model = model
        self.max_retries = max_retries

    def _build_kwargs(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None,
        temperature: float | None,
        max_tokens: int | None,
        reasoning_effort: str | None = None,
    ) -> dict[str, Any]:
        kwargs: dict[str, Any] = {
            "model": self.model,
            "messages": _sanitize_reasoning_replay(messages, self.model, reasoning_effort),
        }
        if tools:
            kwargs["tools"] = tools
            kwargs["tool_choice"] = "auto"
        if temperature is not None:
            kwargs["temperature"] = temperature
        if max_tokens is not None:
            kwargs["max_tokens"] = max_tokens
        _apply_reasoning_effort(kwargs, reasoning_effort)
        return kwargs

    async def chat(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        *,
        temperature: float | None = None,
        max_tokens: int | None = None,
        reasoning_effort: str | None = None,
    ) -> ModelResponse:
        kwargs = self._build_kwargs(messages, tools, temperature, max_tokens, reasoning_effort)
        try:
            resp = await self._client.chat.completions.create(**kwargs)
        except APIError as exc:
            raise ProviderError(f"{type(exc).__name__}: {exc}") from exc

        choice = resp.choices[0]
        msg = choice.message

        tool_calls: list[ToolCall] = []
        for tc in msg.tool_calls or []:
            raw_args = tc.function.arguments or "{}"
            try:
                parsed = json.loads(raw_args)
            except json.JSONDecodeError:
                parsed = {}
            tool_calls.append(
                ToolCall(
                    id=tc.id,
                    name=tc.function.name,
                    arguments=parsed if isinstance(parsed, dict) else {},
                )
            )

        usage = _extract_usage(resp.usage)

        return ModelResponse(
            content=msg.content or "",
            tool_calls=tool_calls,
            finish_reason=choice.finish_reason or "stop",
            reasoning=_extract_reasoning(msg),
            usage=usage,
        )

    async def chat_stream(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        *,
        temperature: float | None = None,
        max_tokens: int | None = None,
        reasoning_effort: str | None = None,
        on_content_delta: OnContentDelta | None = None,
        on_reasoning_delta: OnReasoningDelta | None = None,
    ) -> ModelResponse:
        """Stream a completion, invoking delta callbacks, and return the aggregate.

        Mirrors :meth:`chat`'s return shape so the loop can treat both
        identically once the stream finishes.
        """
        kwargs = self._build_kwargs(messages, tools, temperature, max_tokens, reasoning_effort)
        kwargs["stream"] = True
        # Ask for usage in the final streamed chunk (OpenAI-compatible).
        kwargs["stream_options"] = {"include_usage": True}

        content_parts: list[str] = []
        reasoning_parts: list[str] = []
        finish_reason = "stop"
        usage: dict[str, int] = {}
        # tool_calls arrive as indexed fragments; aggregate by index.
        tc_acc: dict[int, dict[str, str]] = {}

        open_timeout = _stream_open_timeout_s()
        try:
            # Bound the wait for response headers. `create(stream=True)` resolves
            # once the upstream returns headers (before chunks flow), so this
            # times out a stalled connection setup without cutting off a model
            # that is legitimately still streaming.
            try:
                stream = await asyncio.wait_for(
                    self._client.chat.completions.create(**kwargs),
                    timeout=open_timeout,
                )
            except asyncio.TimeoutError as exc:
                raise ProviderError(
                    f"TimeoutError: no streaming response headers after {open_timeout}s. "
                    "On Windows or proxy networks, try a larger "
                    "NANOCODEX_STREAM_OPEN_TIMEOUT_S or check connectivity."
                ) from exc
            async for chunk in stream:
                if getattr(chunk, "usage", None):
                    usage = _extract_usage(chunk.usage)
                if not chunk.choices:
                    continue
                choice = chunk.choices[0]
                if choice.finish_reason:
                    finish_reason = choice.finish_reason
                delta = choice.delta
                if delta is None:
                    continue

                reasoning_piece = _extract_reasoning(delta)
                if reasoning_piece:
                    reasoning_parts.append(reasoning_piece)
                    if on_reasoning_delta is not None:
                        await on_reasoning_delta(reasoning_piece)

                if delta.content:
                    content_parts.append(delta.content)
                    if on_content_delta is not None:
                        await on_content_delta(delta.content)

                for tc in getattr(delta, "tool_calls", None) or []:
                    idx = tc.index if tc.index is not None else 0
                    slot = tc_acc.setdefault(idx, {"id": "", "name": "", "arguments": ""})
                    if tc.id:
                        slot["id"] = tc.id
                    if tc.function is not None:
                        if tc.function.name:
                            slot["name"] = tc.function.name
                        if tc.function.arguments:
                            slot["arguments"] += tc.function.arguments
        except APIError as exc:
            raise ProviderError(f"{type(exc).__name__}: {exc}") from exc

        tool_calls: list[ToolCall] = []
        for idx in sorted(tc_acc):
            slot = tc_acc[idx]
            if not slot["name"]:
                continue
            try:
                parsed = json.loads(slot["arguments"] or "{}")
            except json.JSONDecodeError:
                parsed = {}
            tool_calls.append(
                ToolCall(
                    id=slot["id"] or f"call_{idx}",
                    name=slot["name"],
                    arguments=parsed if isinstance(parsed, dict) else {},
                )
            )

        return ModelResponse(
            content="".join(content_parts),
            tool_calls=tool_calls,
            finish_reason=finish_reason,
            reasoning="".join(reasoning_parts),
            usage=usage,
        )


def _extract_reasoning(obj: Any) -> str:
    """Read DeepSeek/OpenAI-compatible reasoning fields from SDK objects."""
    return (getattr(obj, "reasoning_content", None) or getattr(obj, "reasoning", None) or "")


def _is_deepseek_model(model: str) -> bool:
    """True for DeepSeek's own models (their thinking-mode API shape applies)."""
    return model.strip().lower().startswith("deepseek")


def _apply_reasoning_effort(kwargs: dict[str, Any], effort: str | None) -> None:
    """Translate a reasoning-effort tier into the right request fields per backend.

    DeepSeek's thinking-mode API (the default backend) only understands
    enabled/disabled plus ``reasoning_effort`` high|max, so low/medium collapse
    to high there — it cannot honor finer tiers. For any OTHER OpenAI-compatible
    model we instead pass the tier through to the STANDARD top-level
    ``reasoning_effort`` field (minimal|low|medium|high), so a server that
    supports fine-grained effort actually receives the requested tier rather
    than DeepSeek-shaped ``extra_body`` it would ignore. (A server that doesn't
    support reasoning_effort at all typically ignores the unknown field.)
    """
    if not effort:
        return
    normalized = effort.strip().lower()
    if not normalized or normalized == "auto":
        return

    if _is_deepseek_model(str(kwargs.get("model", ""))):
        extra_body = dict(kwargs.get("extra_body") or {})
        if normalized in _DISABLED_REASONING_EFFORTS:
            extra_body["thinking"] = {"type": "disabled"}
        elif normalized in {"xhigh", "max", "highest"}:
            extra_body["reasoning_effort"] = "max"
            extra_body["thinking"] = {"type": "enabled"}
        elif normalized in {"low", "minimal", "medium", "mid", "high"}:
            # DeepSeek maps low/medium to high in its current thinking-mode API.
            extra_body["reasoning_effort"] = "high"
            extra_body["thinking"] = {"type": "enabled"}
        else:
            return
        kwargs["extra_body"] = extra_body
        return

    # Generic OpenAI-compatible endpoint: standard reasoning_effort field with
    # the real tier. Disabling isn't standardized, so "off" omits the field and
    # lets the server use its own default.
    if normalized in _DISABLED_REASONING_EFFORTS:
        return
    tier = {"xhigh": "high", "max": "high", "highest": "high",
            "mid": "medium"}.get(normalized, normalized)
    if tier in {"minimal", "low", "medium", "high"}:
        kwargs["reasoning_effort"] = tier


def _sanitize_reasoning_replay(
    messages: list[dict[str, Any]],
    model: str,
    reasoning_effort: str | None = None,
) -> list[dict[str, Any]]:
    """Ensure DeepSeek thinking-mode tool-call history replays reasoning_content.

    DeepSeek V4/reasoner rejects a later request when an assistant history item
    carries tool_calls but lacks non-empty reasoning_content. This final pass
    protects restored or older sessions that were recorded before reasoning was
    persisted.
    """
    if not _should_replay_reasoning_content(model, reasoning_effort):
        return messages

    sanitized = deepcopy(messages)
    for msg in sanitized:
        if msg.get("role") != "assistant" or not msg.get("tool_calls"):
            continue
        if not str(msg.get("reasoning_content") or "").strip():
            msg["reasoning_content"] = _REASONING_PLACEHOLDER
    return sanitized


def _should_replay_reasoning_content(model: str, reasoning_effort: str | None = None) -> bool:
    if reasoning_effort and reasoning_effort.strip().lower() in _DISABLED_REASONING_EFFORTS:
        return False
    return _requires_reasoning_content(model)


def _requires_reasoning_content(model: str) -> bool:
    normalized = model.strip().lower()
    return (
        normalized.startswith("deepseek-chat")
        or normalized.startswith("deepseek-reasoner")
        or normalized.startswith("deepseek-v4")
    )
