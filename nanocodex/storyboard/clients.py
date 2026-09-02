"""Model adapters for the storyboard pipeline (injectable, offline-testable).

Three clients, one per external capability the pipeline needs:

* :class:`VisionAnalyzer` — wraps an OpenAI-compatible provider pointed at a
  vision model (DashScope Qwen-VL). Turns an image into an ``AssetAnalysis``.
* :class:`TextPlanner` — wraps the main provider. Turns story text into a list
  of ``Shot`` objects.
* :class:`SeedanceClient` — talks to Volcengine ARK's video-generation API,
  which is NOT chat-completions: you POST a task, get an id, then poll until it
  succeeds and read ``content.video_url``. Shape verified against the live API.

House style mirrors web_search.py / schedule.py: the network calls are behind
small seams (a provider object for the LLM clients, an injectable ``transport``
for Seedance) so the whole pipeline runs offline in tests with fakes. Keys are
read from config/env by the caller and passed in — never hardcoded here.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

# --- prompt templates (loaded from files next to this module) ---------------
from pathlib import Path
from typing import Any, Callable, Protocol

from nanocodex.agent.images import encode_image_block
from nanocodex.storyboard.models import (
    AssetAnalysis,
    Chapter,
    ContinuityGap,
    ContinuityReport,
    Shot,
)

_PROMPT_DIR = Path(__file__).parent / "prompts"


def _load_prompt(name: str) -> str:
    return (_PROMPT_DIR / name).read_text(encoding="utf-8")


# --- LLM client protocol (so tests can inject a fake provider) --------------


class ChatProvider(Protocol):
    """The subset of provider/deepseek.py:DeepSeekProvider we rely on."""

    model: str

    async def chat(
        self,
        messages: list[dict[str, Any]],
        tools: list[dict[str, Any]] | None = None,
        *,
        temperature: float | None = None,
        max_tokens: int | None = None,
        reasoning_effort: str | None = None,
    ) -> Any: ...


def _extract_json(text: str) -> Any:
    """Pull the first JSON object/array out of a model reply.

    Models often wrap JSON in prose or ```json fences. Be lenient: strip fences,
    then grab the outermost {...} or [...]. Raises ValueError if none found.
    """
    if not text:
        raise ValueError("empty model reply")
    # Strip ```json ... ``` fences if present.
    fenced = re.search(r"```(?:json)?\s*(.+?)```", text, re.DOTALL)
    if fenced:
        text = fenced.group(1)
    text = text.strip()
    # Fast path: whole thing is JSON.
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    # Fall back to the first balanced object/array span.
    for opener, closer in (("{", "}"), ("[", "]")):
        start = text.find(opener)
        end = text.rfind(closer)
        if start != -1 and end != -1 and end > start:
            try:
                return json.loads(text[start : end + 1])
            except json.JSONDecodeError:
                continue
    raise ValueError("no JSON found in model reply")


class VisionAnalyzer:
    """Analyze one image into an AssetAnalysis via a vision-capable provider."""

    def __init__(self, provider: ChatProvider) -> None:
        self._provider = provider
        self._prompt = _load_prompt("analyze_image.txt")

    async def analyze(self, image_id: str, image_path: str) -> AssetAnalysis:
        # Build a multimodal user message: the analysis instruction + the image.
        block = encode_image_block(image_path)
        messages = [
            {
                "role": "user",
                "content": [{"type": "text", "text": self._prompt}, block],
            }
        ]
        resp = await self._provider.chat(messages)
        data = _extract_json(getattr(resp, "content", "") or "")
        if not isinstance(data, dict):
            raise ValueError(f"vision analysis for {image_id} was not a JSON object")
        return AssetAnalysis(
            image_id=image_id,
            summary=str(data.get("summary", "")),
            scene_tags=[str(t) for t in data.get("scene_tags", [])],
            mood_tags=[str(t) for t in data.get("mood_tags", [])],
            usable_for=[str(t) for t in data.get("usable_for", [])],
        )


def _chapters_for_prompt(chapters: "list[Chapter] | None") -> str:
    """Render chapters as a compact numbered outline for the shot-planner prompt.

    Returns "(none)" when there are no chapters, so the prompt's fallback branch
    (plan straight from the full story) kicks in. Otherwise one block per
    chapter with its title/summary/setting/cast/key beats — enough structure for
    the planner to slice shots chapter by chapter without re-reading raw text.
    """
    if not chapters:
        return "(none)"
    blocks: list[str] = []
    for i, ch in enumerate(chapters, 1):
        lines = [f"{i}. {ch.title}"]
        if ch.summary:
            lines.append(f"   概要: {ch.summary}")
        if ch.setting:
            lines.append(f"   场景: {ch.setting}")
        if ch.characters:
            lines.append(f"   角色: {', '.join(ch.characters)}")
        for km in ch.key_moments:
            lines.append(f"   - {km}")
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks)


class ChapterPlanner:
    """Split a story into chapters (the story-detail layer above shots)."""

    def __init__(self, provider: ChatProvider) -> None:
        self._provider = provider
        self._prompt = _load_prompt("plan_chapters.txt")

    async def plan(self, story_text: str, *, language: str = "zh") -> list[Chapter]:
        # str.replace (not str.format) — the prompt embeds literal JSON braces.
        filled = (
            self._prompt
            .replace("{story_text}", story_text)
            .replace("{language}", language or "zh")
        )
        resp = await self._provider.chat([{"role": "user", "content": filled}])
        data = _extract_json(getattr(resp, "content", "") or "")
        if isinstance(data, dict):
            chapters_raw = data.get("chapters", data)
        else:
            chapters_raw = data
        if not isinstance(chapters_raw, list):
            raise ValueError("chapter planner did not return a list of chapters")
        chapters: list[Chapter] = []
        for i, c in enumerate(chapters_raw, 1):
            if not isinstance(c, dict):
                continue
            chapters.append(
                Chapter(
                    chapter_id=str(c.get("chapter_id") or f"ch_{i:02d}"),
                    title=str(c.get("title", f"Chapter {i}")),
                    summary=str(c.get("summary", "")),
                    setting=str(c.get("setting", "")),
                    characters=[str(x) for x in c.get("characters", [])],
                    key_moments=[str(x) for x in c.get("key_moments", [])],
                    source_excerpt=str(c.get("source_excerpt", "")),
                )
            )
        if not chapters:
            raise ValueError("chapter planner returned no usable chapters")
        return chapters


class TextPlanner:
    """Turn story text into a list of Shot objects via the main provider."""

    def __init__(self, provider: ChatProvider) -> None:
        self._provider = provider
        self._prompt = _load_prompt("plan_storyboard.txt")

    async def plan(self, story_text: str, *, aspect_ratio: str = "16:9",
                   global_style: str = "",
                   chapters: "list[Chapter] | None" = None) -> list[Shot]:
        # NB: substitute named placeholders with str.replace, NOT str.format —
        # the prompt embeds a literal JSON example with many { } braces, which
        # str.format would try to parse as fields (KeyError). replace touches
        # only our real placeholders and leaves the JSON braces intact.
        # `chapters` is optional: when given, the shots are sliced chapter by
        # chapter; when None the prompt's "(none)" branch plans from the full
        # story (backward-compatible with callers that never pass chapters).
        filled = (
            self._prompt
            .replace("{story_text}", story_text)
            .replace("{aspect_ratio}", aspect_ratio)
            .replace("{global_style}", global_style or "(none)")
            .replace("{chapters}", _chapters_for_prompt(chapters))
        )
        resp = await self._provider.chat([{"role": "user", "content": filled}])
        data = _extract_json(getattr(resp, "content", "") or "")
        # The prompt asks for {"shots": [...]}, but be lenient: accept the
        # "storyboard" key too, or a bare top-level list, so a minor model
        # deviation doesn't blow up the whole render.
        if isinstance(data, dict):
            shots_raw = data.get("shots", data.get("storyboard", data))
        else:
            shots_raw = data
        if not isinstance(shots_raw, list):
            raise ValueError("planner did not return a list of shots")
        shots: list[Shot] = []
        for i, s in enumerate(shots_raw, 1):
            if not isinstance(s, dict):
                continue
            shots.append(
                Shot(
                    shot_id=str(s.get("shot_id") or f"shot_{i:02d}"),
                    title=str(s.get("title", f"Shot {i}")),
                    duration_sec=float(s.get("duration_sec", 5) or 5),
                    prompt=str(s.get("prompt", "")),
                    prompt_zh=str(s.get("prompt_zh", "")),
                    characters=[str(c) for c in s.get("characters", [])],
                    camera=str(s.get("camera", "")),
                    action=str(s.get("action", "")),
                    negative_prompt=str(s.get("negative_prompt", "")),
                    chapter_id=str(s.get("chapter_id", "")),
                    dialogue=[str(d) for d in s.get("dialogue", [])],
                )
            )
        if not shots:
            raise ValueError("planner returned no usable shots")
        return shots


def _shots_for_prompt(shots: "list[Shot]") -> str:
    """Render shots as a compact ordered outline for the continuity prompt.

    One block per shot (shot_id · title, then its 中文画面 prompt_zh — or the
    English prompt as a fallback — and any dialogue), so the reviewer reads the
    sequence the way it will play. Returns "(none)" when there are no shots.
    """
    if not shots:
        return "(none)"
    blocks: list[str] = []
    for i, s in enumerate(shots, 1):
        lines = [f"{i}. [{s.shot_id}] {s.title} ({s.duration_sec:g}s)"]
        desc = getattr(s, "prompt_zh", "") or s.prompt
        if desc:
            lines.append(f"   画面: {desc}")
        dlg = [d for d in getattr(s, "dialogue", []) if d and str(d).strip()]
        if dlg:
            lines.append(f"   台词: {'  '.join(dlg)}")
        blocks.append("\n".join(lines))
    return "\n\n".join(blocks)


def _available_for_prompt(shots: "list[Shot]",
                          available_ids: "list[str] | set[str] | None") -> str:
    """Describe which shots actually have a rendered clip (vs. real gaps).

    ``available_ids is None`` means the caller is checking at planning time with
    no render yet — return "(all)" so the prompt evaluates the whole sequence.
    Otherwise mark each shot 有片/缺片 in order; a 缺片 shot is a true hole in the
    merged video (its neighbours hard-cut across it), which the prompt flags as
    a place to scrutinize.
    """
    if available_ids is None:
        return "(all)"
    have = set(available_ids)
    if not shots:
        return "(none)"
    return "\n".join(
        f"[{s.shot_id}] {'有片' if s.shot_id in have else '缺片'}"
        for s in shots
    )


class ContinuityChecker:
    """Flag missing story beats between consecutive shots (pre-merge review).

    Unlike the planners, an EMPTY result is a valid good outcome: clean shots
    return ``ok=True`` with no gaps and must NOT raise. The provider reply is
    parsed leniently (``_extract_json``) and coerced field by field, so a minor
    model deviation degrades to a usable report rather than blowing up the merge.
    """

    def __init__(self, provider: ChatProvider) -> None:
        self._provider = provider
        self._prompt = _load_prompt("check_continuity.txt")

    async def check(self, shots: "list[Shot]", *,
                    chapters: "list[Chapter] | None" = None,
                    available_ids: "list[str] | set[str] | None" = None
                    ) -> ContinuityReport:
        # str.replace (not str.format) — the prompt embeds literal JSON braces.
        filled = (
            self._prompt
            .replace("{shots}", _shots_for_prompt(shots))
            .replace("{chapters}", _chapters_for_prompt(chapters))
            .replace("{available}", _available_for_prompt(shots, available_ids))
        )
        resp = await self._provider.chat([{"role": "user", "content": filled}])
        data = _extract_json(getattr(resp, "content", "") or "")
        # dict → read gaps/ok/summary; bare list → treat the list as gaps.
        if isinstance(data, dict):
            gaps_raw = data.get("gaps", [])
            summary = str(data.get("summary", ""))
            ok = bool(data.get("ok", not gaps_raw))
        elif isinstance(data, list):
            gaps_raw = data
            summary = ""
            ok = not gaps_raw
        else:
            return ContinuityReport(gaps=[], summary="", ok=True)
        if not isinstance(gaps_raw, list):
            gaps_raw = []
        gaps: list[ContinuityGap] = []
        for g in gaps_raw:
            if not isinstance(g, dict):
                continue
            gaps.append(
                ContinuityGap(
                    after_shot_id=str(g.get("after_shot_id", "")),
                    before_shot_id=str(g.get("before_shot_id", "")),
                    missing=str(g.get("missing", "")),
                    severity=str(g.get("severity", "medium")),
                    suggested_title=str(g.get("suggested_title", "")),
                    suggested_prompt_zh=str(g.get("suggested_prompt_zh", "")),
                    suggested_prompt=str(g.get("suggested_prompt", "")),
                    suggested_duration_sec=float(
                        g.get("suggested_duration_sec", 5) or 5),
                    suggested_dialogue=[
                        str(d) for d in g.get("suggested_dialogue", [])],
                )
            )
        # ok defaults to "no gaps" but trust an explicit false even if parsing
        # dropped malformed gap rows; never raise on an empty result.
        return ContinuityReport(gaps=gaps, summary=summary,
                                ok=ok if gaps_raw else True)


# --- Seedance (Volcengine ARK) video client ---------------------------------

ARK_BASE_URL = "https://ark.cn-beijing.volces.com/api/v3"

# (method, url, headers, json_body) -> (status_code, response_text)
Transport = Callable[[str, str, dict[str, str], "dict | None"], tuple[int, str]]


def _urllib_transport(method: str, url: str, headers: dict[str, str],
                      body: "dict | None") -> tuple[int, str]:
    """Default transport: stdlib urllib (no extra deps), used in production."""
    import urllib.error
    import urllib.request

    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return resp.status, resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read().decode("utf-8", "replace")
    except Exception as exc:  # noqa: BLE001 - surfaced to the caller as a failure
        return -1, f"{type(exc).__name__}: {exc}"


class SeedanceError(RuntimeError):
    """Raised when a Seedance task fails to submit or render."""


@dataclass
class SeedanceResult:
    """Outcome of a finished Seedance task.

    Carries the signed ``video_url`` plus the raw ``usage`` dict from the task
    response. The live API returns ``usage.total_tokens`` on success (verified
    2026-06-10), which is what Seedance bills on — so we keep it rather than
    throwing it away. ``usage`` is ``{}`` when the response omitted it.
    """

    video_url: str
    usage: dict[str, Any]


class SeedanceClient:
    """Submit a video task to ARK and poll until it renders.

    The ARK video API is asynchronous: ``submit`` returns a task id, then you
    ``poll`` that id until status is ``succeeded`` (then read the video URL) or
    ``failed``. ``generate`` ties the two together with a bounded poll loop.

    The HTTP call is behind ``transport`` so tests drive submit/poll parsing with
    a scripted fake — no network, no real key, no spend.
    """

    def __init__(self, api_key: str, *, base_url: str = ARK_BASE_URL,
                 transport: Transport | None = None,
                 sleep: Callable[[float], None] | None = None) -> None:
        if not api_key:
            raise SeedanceError("Seedance needs an ARK API key (env ARK_API_KEY).")
        self._key = api_key
        self._base = base_url.rstrip("/")
        self._transport = transport or _urllib_transport
        import time
        self._sleep = sleep or time.sleep

    def _headers(self) -> dict[str, str]:
        return {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self._key}",
        }

    def submit(self, payload: dict[str, Any]) -> str:
        """POST a generation task; return its task id."""
        status, body = self._transport(
            "POST", f"{self._base}/contents/generations/tasks",
            self._headers(), payload,
        )
        if status != 200:
            raise SeedanceError(f"submit failed (HTTP {status}): {body[:300]}")
        try:
            return json.loads(body)["id"]
        except (json.JSONDecodeError, KeyError) as exc:
            raise SeedanceError(f"submit returned no task id: {body[:300]}") from exc

    def poll_once(self, task_id: str) -> tuple[str, str, dict[str, Any]]:
        """GET task status once. Return (status, video_url_or_empty, usage).

        ``usage`` is the raw usage dict from the response (``{}`` if absent). On
        success it carries ``total_tokens``, which is what Seedance bills on.
        """
        status, body = self._transport(
            "GET", f"{self._base}/contents/generations/tasks/{task_id}",
            self._headers(), None,
        )
        if status != 200:
            raise SeedanceError(f"poll failed (HTTP {status}): {body[:300]}")
        try:
            obj = json.loads(body)
        except json.JSONDecodeError as exc:
            raise SeedanceError(f"poll returned non-JSON: {body[:200]}") from exc
        st = str(obj.get("status", ""))
        url = ""
        usage = obj.get("usage") if isinstance(obj.get("usage"), dict) else {}
        if st == "succeeded":
            url = str((obj.get("content") or {}).get("video_url", ""))
        return st, url, usage

    def generate(self, payload: dict[str, Any], *, max_polls: int = 60,
                 interval_s: float = 6.0,
                 on_progress: Callable[[int, str], None] | None = None
                 ) -> SeedanceResult:
        """Submit then poll until the video is ready; return a SeedanceResult.

        The result carries the signed video URL plus the response ``usage`` dict
        (with ``total_tokens`` for billing). Raises SeedanceError on
        failure/timeout. ``on_progress(i, status)`` is called each poll so a
        UI/CLI can show liveness.
        """
        task_id = self.submit(payload)
        for i in range(max_polls):
            st, url, usage = self.poll_once(task_id)
            if on_progress:
                on_progress(i, st)
            if st == "succeeded":
                if not url:
                    raise SeedanceError(f"task {task_id} succeeded but had no video_url")
                return SeedanceResult(video_url=url, usage=usage)
            if st in ("failed", "cancelled"):
                raise SeedanceError(f"task {task_id} ended as {st}")
            self._sleep(interval_s)
        raise SeedanceError(f"task {task_id} still running after {max_polls} polls")
