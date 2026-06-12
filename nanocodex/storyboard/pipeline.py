"""Storyboard pipeline: story text + images -> shots -> Seedance payloads -> video.

Seven stages, run in order by :func:`run_pipeline`. Each stage is a small pure-ish
function ``(state, deps) -> state`` that returns a NEW state (the running project
dict), mirroring the project's pure-logic house style (agent/schedule.py). The
only side effects live behind ``deps`` (the three injected clients) and the final
export write, so the whole thing runs offline in tests with fake clients.

Stages (matching the user's spec):
    1. ingest         - validate input, build the working state
    2. analyze_assets - VisionAnalyzer per image -> asset_analysis
    3. plan_storyboard- TextPlanner over story_text -> shots
    4. map_assets     - RULE-BASED: attach background/character images per shot
                        (embedding matching is a seam left for later)
    5. build_payloads - assemble one Seedance payload per shot
    6. render         - SeedanceClient per shot -> video_url   (OPT-IN, costs money)
    7. export         - write asset_analysis / storyboard / payloads json + urls

The render stage is OFF by default: Seedance bills real money per clip, so a
caller must explicitly pass ``render=True``.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from nanocodex.agent.pricing import SEEDANCE_PRICING_AS_OF, seedance_cost_cny
from nanocodex.storyboard.models import (
    AssetAnalysis,
    Chapter,
    ImageInput,
    Project,
    SeedancePayload,
    Shot,
    as_jsonable,
    project_from_dict,
    validate_project,
)


@dataclass
class PipelineDeps:
    """Injected capabilities. Any may be None when its stage is not exercised.

    Tests pass fakes; production wires real clients (clients.py). Keeping them
    optional lets the offline tests run analyze/plan with fakes and skip render.
    """

    vision: Any = None       # VisionAnalyzer-like: .analyze(image_id, path) -> AssetAnalysis
    chapters: Any = None     # ChapterPlanner-like: .plan(story_text, ...) -> list[Chapter]
    planner: Any = None      # TextPlanner-like: .plan(story_text, ...) -> list[Shot]
    seedance: Any = None     # SeedanceClient-like: .generate(payload, ...) -> SeedanceResult


@dataclass
class PipelineState:
    """The running project as it accretes through the stages."""

    project: Project
    images: list[ImageInput]
    story_text: str
    chapters: list[Chapter] = field(default_factory=list)
    asset_analysis: list[AssetAnalysis] = field(default_factory=list)
    shots: list[Shot] = field(default_factory=list)
    payloads: list[SeedancePayload] = field(default_factory=list)
    video_urls: dict[str, str] = field(default_factory=dict)  # shot_id -> url
    # Per-shot billing, captured at render: shot_id -> {total_tokens, cost_cny,
    # has_video_input}. Only successful shots get an entry (failures aren't billed).
    video_costs: dict[str, dict[str, Any]] = field(default_factory=dict)


def _payload_has_video_input(payload: dict[str, Any]) -> bool:
    """True if a Seedance payload's content includes a VIDEO reference block.

    Seedance charges a cheaper rate when the INPUT contains video (22 vs 37
    CNY/1M). This pipeline currently sends only text + image reference frames,
    so this is False today, but we detect it from the payload rather than
    hardcoding so the rate stays correct if video inputs are added later.
    """
    content = payload.get("content")
    if not isinstance(content, list):
        return False
    for block in content:
        if not isinstance(block, dict):
            continue
        btype = str(block.get("type", "")).lower()
        if "video" in btype:
            return True
    return False


# --- stage 1: ingest --------------------------------------------------------


def ingest(obj: dict[str, Any]) -> PipelineState:
    """Validate the raw project dict and build the initial state."""
    validate_project(obj)
    project, images = project_from_dict(obj)
    story_text = obj["inputs"]["story_text"]
    return PipelineState(project=project, images=images, story_text=story_text)


# --- stage 2: analyze assets ------------------------------------------------


async def analyze_assets(state: PipelineState, deps: PipelineDeps) -> PipelineState:
    """Run the vision analyzer over every input image."""
    if deps.vision is None:
        return state
    out: list[AssetAnalysis] = []
    for im in state.images:
        out.append(await deps.vision.analyze(im.image_id, im.path))
    state.asset_analysis = out
    return state


# --- stage 3a: plan chapters (story-detail layer above shots) ---------------


async def plan_chapters(state: PipelineState, deps: PipelineDeps) -> PipelineState:
    """Split the story into chapters (3-8) BEFORE it is broken into shots.

    Skipped when no chapter planner is injected (offline tests / callers that
    don't want the chapter layer), in which case shot planning falls back to
    planning straight from the full story text.
    """
    if deps.chapters is None:
        return state
    state.chapters = await deps.chapters.plan(
        state.story_text,
        language=state.project.language,
    )
    return state


# --- stage 3b: plan storyboard ----------------------------------------------


async def plan_storyboard(state: PipelineState, deps: PipelineDeps) -> PipelineState:
    """Turn the story text into shots via the text planner.

    When chapters were planned, they are passed through so shots are sliced
    chapter by chapter (continuity preserved); otherwise the planner reads the
    full story directly. Each shot is tagged with the chapter it falls under
    (best-effort, by order) so the GUI can group shots under their chapter.
    """
    if deps.planner is None:
        return state
    state.shots = await deps.planner.plan(
        state.story_text,
        aspect_ratio=state.project.aspect_ratio,
        global_style=state.project.global_style,
        chapters=state.chapters or None,
    )
    return state


# --- stage 4: map assets (rule-based) ---------------------------------------


def _classify(analysis: AssetAnalysis, declared_kind: str) -> str:
    """Decide whether an image is a character or a background.

    Prefer the user-declared ``kind`` from the input; otherwise infer from the
    VL ``usable_for`` / ``scene_tags`` tags. Defaults to background (a scene
    plate is the safer default than mislabeling something as a character).
    """
    if declared_kind in ("character", "background"):
        return declared_kind
    hay = " ".join(analysis.usable_for + analysis.scene_tags).lower()
    if "character" in hay or "角色" in hay or "person" in hay or "人物" in hay:
        return "character"
    return "background"


def map_assets(state: PipelineState) -> PipelineState:
    """Attach background/character image ids to each shot (rule-based MVP).

    The MVP rule: split images into character vs background buckets (by declared
    kind, else VL tags), then give every shot ALL characters + the first
    background. This is deliberately simple and deterministic; smarter
    per-shot embedding matching is a seam to add later without touching callers.
    """
    by_id = {a.image_id: a for a in state.asset_analysis}
    declared = {im.image_id: im.kind for im in state.images}

    characters: list[str] = []
    backgrounds: list[str] = []
    for im in state.images:
        analysis = by_id.get(im.image_id)
        kind = _classify(analysis, declared.get(im.image_id, "unknown")) if analysis \
            else (im.kind if im.kind in ("character", "background") else "background")
        (characters if kind == "character" else backgrounds).append(im.image_id)

    for shot in state.shots:
        if not shot.character_image_ids:
            shot.character_image_ids = list(characters)
        if not shot.background_image_ids and backgrounds:
            shot.background_image_ids = [backgrounds[0]]
    return state


# --- stage 5: build payloads ------------------------------------------------


def build_payloads(state: PipelineState) -> PipelineState:
    """Assemble one Seedance payload per shot.

    Mirrors the ARK content-shape verified live: a text block (prompt) plus
    optional reference_image blocks (first character + first background), with
    ratio/duration from the project/shot. Negative prompt is appended to the
    text since Seedance takes a single text directive.
    """
    from nanocodex.agent.images import ImageError, encode_image_block

    payloads: list[SeedancePayload] = []
    model_name = "doubao-seedance-2-0-fast-260128"
    img_path = {im.image_id: im.path for im in state.images}

    def _ref_url(p: str) -> str | None:
        """Turn a reference-image source into something ARK accepts.

        ARK's ``image_url`` takes a fetchable URL or a base64 data URI — NOT a
        local disk path (that returns HTTP 400 InvalidParameter). So: pass an
        http(s)/data URL straight through; encode a local file to a base64 data
        URI. Returns None when a local file can't be read so the shot still
        renders from its text prompt instead of failing the whole submit.
        """
        low = p.lower()
        if low.startswith(("http://", "https://", "data:")):
            return p
        try:
            return encode_image_block(p)["image_url"]["url"]
        except ImageError:
            return None

    for shot in state.shots:
        text = shot.prompt
        if shot.negative_prompt:
            text = f"{text}\n\nAvoid: {shot.negative_prompt}"
        content: list[dict[str, Any]] = [{"type": "text", "text": text}]
        # First character + first background as reference frames, if present.
        ref_ids: list[str] = []
        if shot.character_image_ids:
            ref_ids.append(shot.character_image_ids[0])
        if shot.background_image_ids:
            ref_ids.append(shot.background_image_ids[0])
        for rid in ref_ids:
            p = img_path.get(rid)
            if not p:
                continue
            url = _ref_url(p)
            if url:
                content.append({
                    "type": "image_url",
                    "image_url": {"url": url},
                    "role": "reference_image",
                })
        payload = {
            "model": model_name,
            "content": content,
            "ratio": state.project.aspect_ratio,
            "duration": int(round(shot.duration_sec)),
            "watermark": False,
        }
        payloads.append(SeedancePayload(shot_id=shot.shot_id, model=model_name, payload=payload))
    state.payloads = payloads
    return state


# --- stage 6: render (opt-in, costs money) ----------------------------------


def render_one(state: PipelineState, deps: PipelineDeps, shot_id: str,
               on_progress: Callable[[str, int, str], None] | None = None) -> bool:
    """Render (or RE-render) a single shot by id, updating *state* in place.

    Returns True on success (``video_urls[shot_id]`` is a real URL), False on
    failure (``video_urls[shot_id]`` holds a ``[failed: ...]`` marker). A retry
    of a previously-failed shot clears any stale cost entry first, so a shot is
    billed at most once per successful render. Each clip is real spend, so a
    failure is recorded but never raises — the caller keeps going.
    """
    if deps.seedance is None:
        return False
    payload_obj = next((p for p in state.payloads if p.shot_id == shot_id), None)
    if payload_obj is None:
        return False

    def _cb(i: int, st: str) -> None:
        if on_progress:
            on_progress(shot_id, i, st)

    try:
        result = deps.seedance.generate(payload_obj.payload, on_progress=_cb)
        state.video_urls[shot_id] = result.video_url
        # Register cost from the task's own usage. Only successful tasks reach
        # here (failures raise), and only those are billed.
        usage = result.usage or {}
        has_video = _payload_has_video_input(payload_obj.payload)
        cost = seedance_cost_cny(usage, has_video_input=has_video)
        if cost is not None:
            state.video_costs[shot_id] = {
                "total_tokens": int(usage.get("total_tokens", 0)),
                "has_video_input": has_video,
                "cost_cny": round(cost, 4),
            }
        return True
    except Exception as exc:  # noqa: BLE001 - record, keep going
        state.video_urls[shot_id] = f"[failed: {type(exc).__name__}: {exc}]"
        # A re-render that fails again must not leave a stale cost from a prior
        # (impossible-but-defensive) state — only successful renders are billed.
        state.video_costs.pop(shot_id, None)
        return False


def render(state: PipelineState, deps: PipelineDeps,
           on_progress: Callable[[str, int, str], None] | None = None,
           *, max_workers: int = 4) -> PipelineState:
    """Render each shot's payload to a video via Seedance (OPT-IN).

    Only called when the caller explicitly enables rendering. Each clip is real
    spend, so failures on one shot are recorded but don't abort the rest.

    Shots render CONCURRENTLY (up to ``max_workers`` at once): each Seedance
    task is submit-then-poll, so the wall-clock for N shots drops from the sum
    of their times to roughly the slowest single shot. ``render_one`` updates
    ``state`` in place writing distinct keys per shot (the GIL makes each dict
    assignment atomic), so concurrent writes don't clobber each other. Cost is
    unchanged — each shot is still billed once on its own success. ``max_workers
    <= 1`` falls back to serial. ``on_progress`` may fire from worker threads;
    GUI callers already marshal it onto the UI thread via a queue.
    """
    if deps.seedance is None:
        return state
    n = len(state.payloads)
    if n == 0:
        return state
    if max_workers <= 1 or n == 1:
        for p in state.payloads:
            render_one(state, deps, p.shot_id, on_progress=on_progress)
        return state

    from concurrent.futures import ThreadPoolExecutor

    workers = min(max_workers, n)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = [
            pool.submit(render_one, state, deps, p.shot_id, on_progress)
            for p in state.payloads
        ]
        for f in futures:
            f.result()  # render_one never raises (records failures in state)
    return state


# --- stage 7: export --------------------------------------------------------


def export(state: PipelineState, out_dir: Path) -> dict[str, Path]:
    """Write asset_analysis / storyboard / seedance_payloads / video urls to json.

    Returns the paths written. Video URLs are signed + expire (~24h) — noted in
    the urls file so a stale link is understood rather than mysterious.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    written: dict[str, Path] = {}

    files = {
        "chapters.json": [as_jsonable(c) for c in state.chapters],
        "asset_analysis.json": [as_jsonable(a) for a in state.asset_analysis],
        "storyboard.json": [as_jsonable(s) for s in state.shots],
        "seedance_payloads.json": [as_jsonable(p) for p in state.payloads],
    }
    for name, data in files.items():
        path = out_dir / name
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        written[name] = path

    if state.video_urls:
        urls_doc = {
            "_note": "Seedance video URLs are signed and expire (~24h). Download promptly.",
            "videos": state.video_urls,
        }
        path = out_dir / "video_urls.json"
        path.write_text(json.dumps(urls_doc, ensure_ascii=False, indent=2), encoding="utf-8")
        written["video_urls.json"] = path

    if state.video_costs:
        total_tokens = sum(int(c.get("total_tokens", 0)) for c in state.video_costs.values())
        total_cny = round(sum(float(c.get("cost_cny", 0.0)) for c in state.video_costs.values()), 4)
        cost_doc = {
            "_note": (
                "Seedance bills per task on the returned usage.total_tokens "
                f"(rates as of {SEEDANCE_PRICING_AS_OF}: 37 CNY/1M without video "
                "input, 22 CNY/1M with). Only successful tasks are billed."
            ),
            "currency": "CNY",
            "total_tokens": total_tokens,
            "total_cost_cny": total_cny,
            "per_shot": state.video_costs,
        }
        path = out_dir / "video_cost.json"
        path.write_text(json.dumps(cost_doc, ensure_ascii=False, indent=2), encoding="utf-8")
        written["video_cost.json"] = path

    return written


# --- orchestration ----------------------------------------------------------


async def run_planning(obj: dict[str, Any], deps: PipelineDeps, *,
                       out_dir: "Path | None" = None) -> PipelineState:
    """Run the PLANNING half only: ingest → analyze → chapters → shots → map →
    payloads. NEVER renders (never spends money). This is the "preview" path.

    Returns the planned state (chapters + shots + payloads filled, video_urls
    empty). ``out_dir`` None skips the export write; when given, writes the
    chapters/storyboard/payloads JSON (but no video files, since none rendered).
    """
    state = ingest(obj)
    state = await analyze_assets(state, deps)
    state = await plan_chapters(state, deps)
    state = await plan_storyboard(state, deps)
    state = map_assets(state)
    state = build_payloads(state)
    if out_dir is not None:
        export(state, out_dir)
    return state


def render_state(state: PipelineState, deps: PipelineDeps, *,
                 out_dir: "Path | None" = None,
                 on_progress: Callable[[str, int, str], None] | None = None,
                 max_workers: int = 4
                 ) -> tuple[PipelineState, dict[str, Path]]:
    """Render an ALREADY-PLANNED state (the "make video" path).

    Call this on a state returned by :func:`run_planning` once the user has
    reviewed the preview and chosen to spend. Runs the render stage then exports
    (so video_urls/video_costs land in the JSON). Returns (state, written).
    Shots render concurrently up to ``max_workers`` (see :func:`render`).
    """
    state = render(state, deps, on_progress=on_progress, max_workers=max_workers)
    written: dict[str, Path] = {}
    if out_dir is not None:
        written = export(state, out_dir)
    return state, written


async def run_pipeline(obj: dict[str, Any], deps: PipelineDeps, *,
                       out_dir: "Path | None" = None, render_video: bool = False,
                       on_progress: Callable[[str, int, str], None] | None = None
                       ) -> tuple[PipelineState, dict[str, Path]]:
    """Run all stages in order. Returns (final_state, exported_paths).

    ``render_video`` defaults False — Seedance billing is opt-in. ``out_dir``
    None skips the export write (used by tests that assert on state only).

    Thin wrapper over :func:`run_planning` (+ optional :func:`render`) so the
    one-shot CLI/agent entry points keep the same signature while the GUI can
    drive plan and render as two separate, user-gated steps.
    """
    state = await run_planning(obj, deps)
    if render_video:
        state = render(state, deps, on_progress=on_progress)
    written: dict[str, Path] = {}
    if out_dir is not None:
        written = export(state, out_dir)
    return state, written
