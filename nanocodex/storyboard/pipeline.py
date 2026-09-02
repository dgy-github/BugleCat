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
import re
from dataclasses import dataclass, field
from dataclasses import fields as dc_fields
from pathlib import Path
from typing import Any, Callable

from nanocodex.agent.pricing import SEEDANCE_PRICING_AS_OF, seedance_cost_cny
from nanocodex.storyboard.models import (
    AssetAnalysis,
    Chapter,
    ContinuityGap,
    ContinuityReport,
    ImageInput,
    Project,
    SeedancePayload,
    Shot,
    StoryboardError,
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
    continuity: Any = None   # ContinuityChecker-like: .check(shots, ...) -> ContinuityReport
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


# --- 本地告警: 一镜疑似塞了多个先后动作 (纯逻辑, 不调模型) -------------------

# A shot is meant to be ONE continuous instant. These markers, found in a
# shot's camera/prompt/prompt_zh, signal multiple ordered actions crammed into
# one shot (which makes the video model play them out of order). Pure
# substring/word matching — no network, no model — so the GUI can flag shots
# yellow right after planning for a human to eyeball.
_MULTI_ACTION_MARKERS: tuple[str, ...] = (
    # English narrative-sequence / multi-scene words
    "then", "after that", "next,", "montage", "cut to", "back and forth",
    "quick pan", "series of", "followed by",
    # Chinese sequence words
    "然后", "接着", "随后", "之后", "先", "再", "紧接着", "一连", "连续",
    "蒙太奇", "快速剪辑", "切到", "切换", "三天", "第一天", "第二天", "第三天",
)


def scan_multi_action_shots(state: "PipelineState",
                            ) -> dict[str, list[str]]:
    """Flag shots whose text suggests MULTIPLE ordered actions in one shot.

    Returns ``{shot_id: [matched markers]}`` for every shot whose
    ``camera`` / ``prompt`` / ``prompt_zh`` contains a sequence/multi-scene
    marker (see ``_MULTI_ACTION_MARKERS``). A shot should capture one
    continuous instant; markers like "then" / "然后" / "montage" / "cut to"
    mean several beats were packed in, which the video model can't order
    reliably. Shots with no markers are omitted (empty dict = all clean).

    Pure: substring match over the shot's own text, no model call. This is a
    heuristic HINT for human review, not a hard gate — false positives are
    fine (a human decides), so it errs toward flagging.
    """
    hits: dict[str, list[str]] = {}
    for s in state.shots:
        hay = " ".join([
            getattr(s, "camera", "") or "",
            getattr(s, "prompt", "") or "",
            getattr(s, "prompt_zh", "") or "",
        ]).lower()
        matched = [m for m in _MULTI_ACTION_MARKERS if m.lower() in hay]
        if matched:
            hits[s.shot_id] = matched
    return hits


# --- pre-merge continuity check (standalone, NOT in the planning chain) ------


async def check_continuity(state: PipelineState, deps: PipelineDeps, *,
                           available_ids: "list[str] | set[str] | None" = None
                           ) -> ContinuityReport:
    """Review the planned shots for missing story beats before a merge.

    Standalone — NOT part of :func:`run_planning` (it never spends and never
    blocks rendering): the GUI calls it just before stitching clips so the user
    can review剧情跳跃 and choose 仍然合并 / 去补镜. ``available_ids`` lists the
    shot_ids that actually have a rendered clip (un-rendered shots are real holes
    in the merged video); pass None to evaluate the whole sequence at plan time.

    When no checker is injected (offline tests / no DeepSeek key), returns a
    clean report rather than raising, so the merge path degrades gracefully.
    """
    if deps.continuity is None:
        return ContinuityReport(ok=True, summary="(no checker)")
    return await deps.continuity.check(
        state.shots, chapters=state.chapters or None,
        available_ids=available_ids)


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


def _build_shot_payload(shot: Shot, project: Project,
                        img_path: dict[str, str]) -> SeedancePayload:
    """Assemble ONE Seedance payload for a single shot.

    Shared by :func:`build_payloads` (the whole storyboard) and
    :func:`insert_fill_shot` (one補镜 added after the fact), so a fill-in shot
    is encoded byte-for-byte the same way a planned shot is — same dialogue/
    caption/negative handling, same reference-frame rules. ``img_path`` maps
    image_id -> source path (URL or local file).
    """
    from nanocodex.agent.images import ImageError, encode_image_block

    model_name = "doubao-seedance-2-0-fast-260128"

    # On-screen text language directive: any text the video renders (signs,
    # subtitles, dialogue captions) comes out in the chosen language —
    # independent of the (English) prompt language.
    cap_lang = (getattr(project, "caption_language", "zh") or "zh").lower()
    cap_directive = {
        "zh": "All on-screen text (signs, subtitles, captions) must be in Chinese.",
        "en": "All on-screen text (signs, subtitles, captions) must be in English.",
        "none": "Do not render any on-screen text, signs, subtitles, or captions.",
    }.get(cap_lang, "")

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

    # Feed the CHINESE 画面 description to the model when present (English
    # 主体描述 leaks English into on-screen text and overrides the caption
    # directive). Fall back to the English `prompt` only when no zh exists, so
    # older data / planners without prompt_zh still render.
    text = shot.prompt_zh or shot.prompt
    # Dialogue / spoken lines. Per ARK docs (verified from official guidance):
    # writing the line in Chinese and wrapping it in 「double quotes」 optimizes
    # speech generation, and an explicit "用普通话说" declaration is more robust
    # for mixed-language scenes. Seedance drives lip-sync + voice off this text,
    # so the spoken language follows the line's own language (kept Chinese here),
    # independent of the (English)画面 prompt and of caption_language (which only
    # governs RENDERED on-screen text, not the spoken audio).
    dlg = [d.strip() for d in getattr(shot, "dialogue", []) if d and d.strip()]
    if dlg:
        spoken = "；".join(f"角色用普通话说：「{d}」" for d in dlg)
        text = f"{text}\n\n{spoken}"
    if cap_directive:
        text = f"{text}\n\n{cap_directive}"
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
        "ratio": project.aspect_ratio,
        "duration": int(round(shot.duration_sec)),
        "watermark": False,
    }
    return SeedancePayload(shot_id=shot.shot_id, model=model_name, payload=payload)


def build_payloads(state: PipelineState) -> PipelineState:
    """Assemble one Seedance payload per shot.

    Mirrors the ARK content-shape verified live: a text block (prompt) plus
    optional reference_image blocks (first character + first background), with
    ratio/duration from the project/shot. Negative prompt is appended to the
    text since Seedance takes a single text directive. Per-shot assembly lives
    in :func:`_build_shot_payload` (shared with :func:`insert_fill_shot`).
    """
    img_path = {im.image_id: im.path for im in state.images}
    state.payloads = [
        _build_shot_payload(shot, state.project, img_path)
        for shot in state.shots
    ]
    return state


# --- 补镜: turn a continuity gap into a real shot, inserted in order ---------


def _unique_fill_id(after_id: str, existing: "set[str]") -> str:
    """Pick a fresh shot_id for a 补镜 wedged after ``after_id``.

    ``shot_03`` → ``shot_03b`` (then ``shot_03c`` …) so the id sorts/reads right
    between ``after_id`` and the next shot. Falls back to a ``_fillN`` suffix if
    the alphabet runs out (won't in practice). Never collides with ``existing``.
    """
    base = after_id or "shot"
    for c in "bcdefghijklmnopqrstuvwxyz":
        cand = f"{base}{c}"
        if cand not in existing:
            return cand
    n = 2
    while f"{base}_fill{n}" in existing:
        n += 1
    return f"{base}_fill{n}"


def insert_fill_shot(state: PipelineState, gap: ContinuityGap) -> str:
    """Adopt one continuity-gap suggestion as a REAL shot, inserted in order.

    Turns a :class:`ContinuityGap`'s 补镜 suggestion into a :class:`Shot`, gives
    it a fresh id wedged right after ``gap.after_shot_id`` (so the merge stitches
    it between the two shots it bridges), copies that neighbour's reference-frame
    image ids (same characters/background as its surroundings), builds its
    payload, and slots both shot + payload into ``state`` at the matching index.
    Returns the new shot_id. Does NOT render — the caller renders it via
    :func:`render_one` (real spend, user-gated). If ``after_shot_id`` isn't found
    the shot is appended at the end (still valid, just not wedged).
    """
    existing = {s.shot_id for s in state.shots}
    new_id = _unique_fill_id(gap.after_shot_id, existing)

    # Find the anchor shot to (a) inherit its reference images and (b) decide
    # where to splice. Default to the end when the anchor id isn't present.
    idx = next((i for i, s in enumerate(state.shots)
                if s.shot_id == gap.after_shot_id), None)
    anchor = state.shots[idx] if idx is not None else None

    shot = Shot(
        shot_id=new_id,
        title=gap.suggested_title or "补镜",
        duration_sec=float(gap.suggested_duration_sec or 5) or 5.0,
        prompt=gap.suggested_prompt or "",
        prompt_zh=gap.suggested_prompt_zh or "",
        negative_prompt="",
        dialogue=list(gap.suggested_dialogue or []),
        chapter_id=getattr(anchor, "chapter_id", "") if anchor else "",
        character_image_ids=list(getattr(anchor, "character_image_ids", []))
        if anchor else [],
        background_image_ids=list(getattr(anchor, "background_image_ids", []))
        if anchor else [],
    )

    img_path = {im.image_id: im.path for im in state.images}
    payload = _build_shot_payload(shot, state.project, img_path)

    pos = idx + 1 if idx is not None else len(state.shots)
    state.shots.insert(pos, shot)
    # Keep payloads aligned with shots so render_one finds it; insert at the same
    # logical spot (payload order is otherwise just informational).
    state.payloads.insert(min(pos, len(state.payloads)), payload)
    return new_id


# --- 前后帧衔接: chain each shot's first_frame to the previous shot's last frame

# Pull the LAST frame of a rendered clip as a data-URI so it can be injected as
# the NEXT shot's first_frame (画面前后衔接). Behind a seam (FrameExtractor) so
# tests don't shell out to ffmpeg.
FrameExtractor = Callable[[str], "str | None"]


def _set_first_frame(payload: dict[str, Any], frame_uri: str) -> None:
    """Make ``frame_uri`` the shot's ARK ``first_frame`` reference image.

    Removes any existing image_url blocks (the subject ``reference_image`` plus
    any earlier first_frame) before adding the new one: ARK's 首尾帧 frame-control
    and 主体参考 are distinct modes, and the previous shot's last frame already
    carries the character + scene, so we anchor continuity on it rather than mix
    the two. The text block (prompt/dialogue/caption directive) is left intact.
    ``role: "first_frame"`` follows the same documented role-field convention as
    the ``reference_image`` blocks build_payloads already emits.
    """
    content = payload.get("content")
    if not isinstance(content, list):
        return
    kept = [b for b in content
            if not (isinstance(b, dict) and b.get("type") == "image_url")]
    kept.append({
        "type": "image_url",
        "image_url": {"url": frame_uri},
        "role": "first_frame",
    })
    payload["content"] = kept


def _default_frame_extractor(video_url: str) -> "str | None":
    """Extract a clip's LAST frame as a base64 JPEG data URI (None on failure).

    ffmpeg reads the (signed) video URL directly — no local download needed —
    seeks 1s before the end (``-sseof -1``) and grabs one frame, which we
    base64-encode into a ``data:`` URI ARK accepts as an image_url. Returns None
    on any failure (ffmpeg missing, network, empty output) so the caller falls
    back to an independent, un-chained render rather than aborting the run.
    (``_default_runner`` is defined below in the concat section — referenced at
    call time, so definition order doesn't matter.)
    """
    import base64
    import os
    import tempfile

    fd, tmp_path = tempfile.mkstemp(suffix=".jpg", prefix="_nsb_lf_")
    os.close(fd)
    tmp = Path(tmp_path)
    argv = ["ffmpeg", "-y", "-sseof", "-1", "-i", video_url,
            "-frames:v", "1", "-q:v", "2", str(tmp)]
    try:
        rc, _out = _default_runner(argv)
        if rc != 0 or not tmp.exists() or tmp.stat().st_size == 0:
            return None
        data = tmp.read_bytes()
    except OSError:
        return None
    finally:
        try:
            tmp.unlink()
        except OSError:
            pass
    b64 = base64.b64encode(data).decode("ascii")
    return f"data:image/jpeg;base64,{b64}"


def _render_chained(state: PipelineState, deps: PipelineDeps,
                    on_progress: "Callable[[str, int, str], None] | None",
                    extract_frame: FrameExtractor) -> PipelineState:
    """Render shots IN ORDER, threading each shot's last frame into the next.

    For 画面前后衔接: shot N renders, its last frame is extracted and injected as
    shot N+1's ``first_frame`` (baked into N+1's stored payload, so a later
    re-render of N+1 keeps the same anchor). Necessarily SERIAL — N+1 can't start
    until N's frame exists — which trades the concurrent path's wall-clock for
    continuity (the caller opted in). A shot that fails, or whose frame can't be
    extracted, simply breaks the chain there: the next shot renders independently
    from its own reference images, so one bad clip never aborts the rest.
    """
    by_id = {p.shot_id: p for p in state.payloads}
    prev_frame: str | None = None
    for shot in state.shots:
        payload_obj = by_id.get(shot.shot_id)
        if payload_obj is None:
            continue
        if prev_frame:
            _set_first_frame(payload_obj.payload, prev_frame)
        ok = render_one(state, deps, shot.shot_id, on_progress=on_progress)
        prev_frame = None
        if ok:
            url = state.video_urls.get(shot.shot_id, "")
            if url and not str(url).startswith("[failed"):
                prev_frame = extract_frame(url)
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
           *, max_workers: int = 4, chain_frames: bool = False,
           extract_frame: "FrameExtractor | None" = None) -> PipelineState:
    """Render each shot's payload to a video via Seedance (OPT-IN).

    Only called when the caller explicitly enables rendering. Each clip is real
    spend, so failures on one shot are recorded but don't abort the rest.

    Two modes:

    * Default (``chain_frames=False``): shots render CONCURRENTLY (up to
      ``max_workers`` at once). Each Seedance task is submit-then-poll, so the
      wall-clock for N shots drops from the sum of their times to roughly the
      slowest single shot. ``render_one`` updates ``state`` in place writing
      distinct keys per shot (the GIL makes each dict assignment atomic), so
      concurrent writes don't clobber each other. ``max_workers <= 1`` falls
      back to serial.
    * ``chain_frames=True``: shots render SERIALLY, each shot's first_frame
      anchored to the previous shot's last frame for 前后帧衔接 (see
      :func:`_render_chained`). ``extract_frame`` overrides the ffmpeg frame
      extractor (tests inject a fake); ``max_workers`` is ignored (inherently
      serial).

    Cost is unchanged in both modes — each shot is still billed once on its own
    success. ``on_progress`` may fire from worker threads in the concurrent
    mode; GUI callers already marshal it onto the UI thread via a queue.
    """
    if deps.seedance is None:
        return state
    n = len(state.payloads)
    if n == 0:
        return state
    if chain_frames:
        return _render_chained(state, deps, on_progress,
                               extract_frame or _default_frame_extractor)
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


# --- run archiving: one directory + index entry per 出片 -------------------
#
# Each render used to overwrite a single fixed storyboard_out/ directory, so
# only the LAST run's json survived. These helpers give every 出片 its own
# timestamped sub-directory under <base>/runs/ plus an append-only index.json,
# turning renders into a browsable history (山海经 run, next-story run, …).
# Pure path/JSON logic only — no rendering, no network — so it unit-tests with
# a tmp dir. The GUI calls make_run_dir() at render start and write_run_index()
# at render end; read_run_index() backs the panel's 历史 list.


# Windows-illegal filename chars (\ / : * ? " < > |) plus control chars; each
# is collapsed to "_" so a story title is safe as a directory-name segment.
_ILLEGAL_NAME = re.compile(r'[\\/:*?"<>|\x00-\x1f]+')


def _slug_title(title: str, *, max_len: int = 20) -> str:
    """Turn a story title into a filesystem-safe slug of at most ``max_len`` chars.

    Illegal chars → "_", runs of whitespace → "_", trimmed of leading/trailing
    separators. Empty/blank titles fall back to "untitled" so a run dir always
    has a readable suffix.
    """
    s = _ILLEGAL_NAME.sub("_", title or "")
    s = re.sub(r"\s+", "_", s).strip("_. ")
    s = s[:max_len].strip("_. ")
    return s or "untitled"


def make_run_dir(base: "Path | str", title: str, *,
                 when: "Any" = None, max_title: int = 20) -> Path:
    """Create and return a unique run directory ``<base>/runs/<ts>_<slug>/``.

    ``ts`` is ``YYYYMMDD-HHMM`` (local time) so runs sort chronologically; the
    slug is the cleaned story title (<= ``max_title`` chars). If that name is
    already taken (two renders in the same minute), ``-2``, ``-3`` … is appended
    so we never reuse — and thus never overwrite — an existing run.
    """
    from datetime import datetime

    base = Path(base)
    when = when or datetime.now()
    ts = when.strftime("%Y%m%d-%H%M")
    slug = _slug_title(title, max_len=max_title)
    runs = base / "runs"
    runs.mkdir(parents=True, exist_ok=True)
    name = f"{ts}_{slug}"
    run_dir = runs / name
    n = 2
    while run_dir.exists():
        run_dir = runs / f"{name}-{n}"
        n += 1
    run_dir.mkdir(parents=True)
    return run_dir


def write_run_index(base: "Path | str", meta: dict[str, Any]) -> Path:
    """Append a run summary to ``<base>/runs/index.json`` (append-only history).

    The index is a list of run-meta dicts (newest appended last). A run is keyed
    by ``run_id``: writing the same ``run_id`` again REPLACES its entry in place
    (so a 重试 that updates success-count/cost refreshes the same history row
    rather than adding a duplicate). Best-effort: a malformed/missing index is
    treated as empty rather than raising.
    """
    base = Path(base)
    runs = base / "runs"
    runs.mkdir(parents=True, exist_ok=True)
    index_path = runs / "index.json"
    entries = read_run_index(base)
    rid = meta.get("run_id")
    replaced = False
    if rid:
        for i, e in enumerate(entries):
            if e.get("run_id") == rid:
                entries[i] = meta
                replaced = True
                break
    if not replaced:
        entries.append(meta)
    index_path.write_text(
        json.dumps(entries, ensure_ascii=False, indent=2), encoding="utf-8")
    return index_path


def read_run_index(base: "Path | str") -> list[dict[str, Any]]:
    """Read ``<base>/runs/index.json`` into a list; [] when missing/unreadable."""
    index_path = Path(base) / "runs" / "index.json"
    try:
        if index_path.is_file():
            data = json.loads(index_path.read_text(encoding="utf-8"))
            if isinstance(data, list):
                return [e for e in data if isinstance(e, dict)]
    except (OSError, json.JSONDecodeError):
        pass
    return []


# --- load a past run back into a PipelineState ------------------------------
#
# export() writes a run's chapters/storyboard/seedance_payloads/video_urls/
# video_cost JSON. These read it back so a past 出片 can be REOPENED in the
# panel — replay clips, retry failed shots, merge — without re-planning or
# re-spending. project/story_text aren't exported, so project fields come from
# the history index `meta` row (title/ratio/caption_language/run_id) when given.


def _dataclass_from_dict(cls: Any, d: Any) -> Any:
    """Build a dataclass instance from a dict, keeping only known fields.

    Unknown keys are dropped and missing keys fall back to the dataclass's own
    defaults, so JSON written by an older export still loads as a newer
    dataclass gains fields. Required fields with no default must be present
    (they always are in our own export); a malformed row raises TypeError,
    which callers filter out per-item.
    """
    if not isinstance(d, dict):
        d = {}
    known = {f.name for f in dc_fields(cls)}
    return cls(**{k: v for k, v in d.items() if k in known})


def load_run_state(run_dir: "Path | str", *,
                   meta: "dict[str, Any] | None" = None) -> PipelineState:
    """Reconstruct a :class:`PipelineState` from a run's exported JSON files.

    Reads chapters/asset_analysis/storyboard/seedance_payloads/video_urls/
    video_cost from ``run_dir`` (each missing/unreadable file degrades to
    empty, so a partial run loads what it has). ``project``/``story_text``
    aren't exported: project fields are taken from the history index ``meta``
    row when available, else minimal defaults; ``images`` is empty and
    ``story_text`` blank (payloads already embed any reference-image data, so
    replay/retry/merge work without them).
    """
    run_dir = Path(run_dir)

    def _read_json(name: str, default: Any) -> Any:
        p = run_dir / name
        try:
            if p.is_file():
                return json.loads(p.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            pass
        return default

    def _rows(name: str, cls: Any) -> list:
        out = []
        for row in _read_json(name, []):
            if not isinstance(row, dict):
                continue
            try:
                out.append(_dataclass_from_dict(cls, row))
            except TypeError:
                pass  # skip a row that can't satisfy required fields
        return out

    chapters = _rows("chapters.json", Chapter)
    asset_analysis = _rows("asset_analysis.json", AssetAnalysis)
    shots = _rows("storyboard.json", Shot)
    payloads = _rows("seedance_payloads.json", SeedancePayload)

    urls_doc = _read_json("video_urls.json", {})
    video_urls: dict[str, str] = {}
    if isinstance(urls_doc, dict) and isinstance(urls_doc.get("videos"), dict):
        video_urls = {str(k): str(v) for k, v in urls_doc["videos"].items()}

    cost_doc = _read_json("video_cost.json", {})
    video_costs: dict[str, dict[str, Any]] = {}
    if isinstance(cost_doc, dict) and isinstance(cost_doc.get("per_shot"), dict):
        video_costs = {str(k): v for k, v in cost_doc["per_shot"].items()
                       if isinstance(v, dict)}

    m = meta or {}
    project = Project(
        id=str(m.get("run_id") or "loaded"),
        title=str(m.get("title") or "(loaded run)"),
        aspect_ratio=str(m.get("ratio") or "16:9"),
        caption_language=str(m.get("caption_language") or "zh"),
    )

    return PipelineState(
        project=project,
        images=[],
        story_text="",
        chapters=chapters,
        asset_analysis=asset_analysis,
        shots=shots,
        payloads=payloads,
        video_urls=video_urls,
        video_costs=video_costs,
    )


# --- concat: stitch a run's shot clips into one full video ------------------

# Probe a clip to (width, height, avg_frame_rate) so we can decide whether the
# fast stream-copy concat is safe or we must re-encode. Behind a seam so tests
# don't shell out to ffprobe.
ClipProbe = Callable[[Path], "tuple[int, int, str] | None"]
# Run an ffmpeg/ffprobe argv; return (returncode, stdout+stderr). Injectable.
Runner = Callable[[list[str]], tuple[int, str]]


def _default_runner(argv: list[str]) -> tuple[int, str]:
    """Default Runner: run argv, capture combined output (no shell).

    On Windows each ffmpeg/ffprobe child would otherwise flash its own black
    console window (a concat probes every clip then runs ffmpeg, so several pop
    in a row). CREATE_NO_WINDOW suppresses them so a merge stays silent — the
    flag doesn't exist off-Windows, where there's nothing to suppress anyway.
    """
    import subprocess
    kwargs: dict[str, Any] = {}
    no_window = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    if no_window:
        kwargs["creationflags"] = no_window
    try:
        proc = subprocess.run(argv, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", **kwargs)
        return proc.returncode, (proc.stdout or "") + (proc.stderr or "")
    except FileNotFoundError as exc:
        return -1, str(exc)


def _ffprobe_params(path: Path, *, runner: Runner) -> "tuple[int, int, str] | None":
    """Probe (width, height, avg_frame_rate) via ffprobe; None on any failure."""
    argv = [
        "ffprobe", "-v", "error", "-select_streams", "v:0",
        "-show_entries", "stream=width,height,avg_frame_rate",
        "-of", "csv=p=0:s=,", str(path),
    ]
    rc, out = runner(argv)
    if rc != 0:
        return None
    parts = [p.strip() for p in out.strip().splitlines()[0].split(",")] if out.strip() else []
    if len(parts) < 3:
        return None
    try:
        return int(parts[0]), int(parts[1]), parts[2]
    except ValueError:
        return None


def concat_clips(run_dir: "Path | str", shot_ids: list[str], *,
                 dest_name: str = "full.mp4",
                 runner: Runner | None = None) -> "Path | None":
    """Stitch a run's shot clips into one ``<run_dir>/<dest_name>``, in order.

    Picks ``<run_dir>/<shot_id>.mp4`` for each id in ``shot_ids`` THAT EXISTS
    (missing shots — e.g. ones that failed to render — are skipped silently, so
    the full video covers只 the shots you actually have). Returns the output
    path on success, or None when there are fewer than 2 clips to join (nothing
    meaningful to merge).

    Strategy: if every clip shares the same width/height/frame-rate, use the
    fast lossless concat demuxer with ``-c copy``; otherwise fall back to
    re-encoding (libx264 + aac) so mismatched clips still join cleanly. The
    ffmpeg/ffprobe calls go through ``runner`` so tests drive it without a real
    binary. Raises StoryboardError when ffmpeg itself fails.
    """
    run_dir = Path(run_dir)
    run = runner or _default_runner

    clips = [run_dir / f"{sid}.mp4" for sid in shot_ids]
    clips = [c for c in clips if c.exists() and c.stat().st_size > 0]
    if len(clips) < 2:
        return None

    dest = run_dir / dest_name

    # Decide copy vs re-encode by probing each clip's video params.
    params = [_ffprobe_params(c, runner=run) for c in clips]
    uniform = all(p is not None for p in params) and len(set(params)) == 1

    if uniform:
        # Fast path: concat demuxer + stream copy (no re-encode).
        listing = "".join(f"file '{c.as_posix()}'\n" for c in clips)
        list_path = run_dir / "_concat_list.txt"
        list_path.write_text(listing, encoding="utf-8")
        argv = [
            "ffmpeg", "-y", "-f", "concat", "-safe", "0",
            "-i", str(list_path), "-c", "copy", str(dest),
        ]
        rc, out = run(argv)
        try:
            list_path.unlink()
        except OSError:
            pass
        if rc != 0:
            raise StoryboardError(f"ffmpeg concat (copy) failed: {out[-400:]}")
        return dest

    # Mismatched params: re-encode with the concat filter so they join cleanly.
    argv = ["ffmpeg", "-y"]
    for c in clips:
        argv += ["-i", str(c)]
    n = len(clips)
    streams = "".join(f"[{i}:v:0][{i}:a:0]" for i in range(n))
    filt = f"{streams}concat=n={n}:v=1:a=1[v][a]"
    argv += ["-filter_complex", filt, "-map", "[v]", "-map", "[a]",
             "-c:v", "libx264", "-c:a", "aac", str(dest)]
    rc, out = run(argv)
    if rc != 0:
        raise StoryboardError(f"ffmpeg concat (re-encode) failed: {out[-400:]}")
    return dest


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
                 max_workers: int = 4, chain_frames: bool = False
                 ) -> tuple[PipelineState, dict[str, Path]]:
    """Render an ALREADY-PLANNED state (the "make video" path).

    Call this on a state returned by :func:`run_planning` once the user has
    reviewed the preview and chosen to spend. Runs the render stage then exports
    (so video_urls/video_costs land in the JSON). Returns (state, written).
    Shots render concurrently up to ``max_workers``; pass ``chain_frames=True``
    for serial 前后帧衔接 (see :func:`render`).
    """
    state = render(state, deps, on_progress=on_progress, max_workers=max_workers,
                   chain_frames=chain_frames)
    written: dict[str, Path] = {}
    if out_dir is not None:
        written = export(state, out_dir)
    return state, written


async def run_pipeline(obj: dict[str, Any], deps: PipelineDeps, *,
                       out_dir: "Path | None" = None, render_video: bool = False,
                       on_progress: Callable[[str, int, str], None] | None = None,
                       chain_frames: bool = False
                       ) -> tuple[PipelineState, dict[str, Path]]:
    """Run all stages in order. Returns (final_state, exported_paths).

    ``render_video`` defaults False — Seedance billing is opt-in. ``out_dir``
    None skips the export write (used by tests that assert on state only).
    ``chain_frames`` (only when rendering) anchors each shot's first_frame to the
    previous shot's last frame for 前后帧衔接 — serial, see :func:`render`.

    Thin wrapper over :func:`run_planning` (+ optional :func:`render`) so the
    one-shot CLI/agent entry points keep the same signature while the GUI can
    drive plan and render as two separate, user-gated steps.
    """
    state = await run_planning(obj, deps)
    if render_video:
        state = render(state, deps, on_progress=on_progress, chain_frames=chain_frames)
    written: dict[str, Path] = {}
    if out_dir is not None:
        written = export(state, out_dir)
    return state, written
