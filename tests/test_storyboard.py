"""Offline tests for the storyboard pipeline.

Everything runs with fakes — no network, no real keys, no Seedance spend:
* schema validation (valid + invalid shapes)
* rule-based asset mapping
* Seedance payload assembly
* the full pipeline via fake Vision/Planner/Seedance clients (render off + on)
* SeedanceClient submit/poll parsing via a scripted fake transport
"""

from __future__ import annotations

import json

import pytest

from nanocodex.storyboard.clients import (
    ContinuityChecker,
    SeedanceClient,
    SeedanceError,
    SeedanceResult,
    _extract_json,
)
from nanocodex.storyboard.models import (
    AssetAnalysis,
    Chapter,
    ContinuityGap,
    ContinuityReport,
    Shot,
    StoryboardError,
    as_jsonable,
    validate_project,
)
from nanocodex.storyboard.pipeline import (
    PipelineDeps,
    build_payloads,
    check_continuity,
    concat_clips,
    ingest,
    insert_fill_shot,
    load_run_state,
    make_run_dir,
    map_assets,
    read_run_index,
    render_state,
    run_pipeline,
    run_planning,
    write_run_index,
)

# A tiny valid PNG (magic bytes + padding) so encode_image_block accepts it.
_PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 32


def _valid_obj(image_paths=None):
    images = []
    for i, p in enumerate(image_paths or [], 1):
        images.append({"image_id": f"img_{i:02d}", "path": str(p), "kind": "unknown"})
    return {
        "project": {
            "id": "p1",
            "title": "Test",
            "target_model": "seedance",
            "aspect_ratio": "16:9",
        },
        "inputs": {"story_text": "Once upon a time.", "images": images},
    }


# --- schema validation ------------------------------------------------------


def test_validate_accepts_minimal_valid():
    validate_project(_valid_obj())  # no raise


def test_validate_rejects_missing_title():
    obj = _valid_obj()
    del obj["project"]["title"]
    with pytest.raises(StoryboardError, match="title"):
        validate_project(obj)


def test_validate_rejects_bad_image_kind():
    obj = _valid_obj()
    obj["inputs"]["images"] = [{"image_id": "x", "path": "p", "kind": "banana"}]
    with pytest.raises(StoryboardError):
        validate_project(obj)


# --- _extract_json ----------------------------------------------------------


def test_extract_json_plain():
    assert _extract_json('{"a": 1}') == {"a": 1}


def test_extract_json_fenced():
    assert _extract_json('```json\n{"a": 2}\n```') == {"a": 2}


def test_extract_json_embedded_in_prose():
    assert _extract_json('here you go: {"a": 3} done') == {"a": 3}


def test_extract_json_raises_on_none():
    with pytest.raises(ValueError):
        _extract_json("no json here")


# --- rule-based map_assets --------------------------------------------------


def test_map_assets_splits_character_and_background():
    obj = _valid_obj()
    state = ingest(obj)
    state.images = []  # build images manually below via a fresh state
    # Two images: one declared character, one background.
    from nanocodex.storyboard.models import ImageInput

    state.images = [
        ImageInput(image_id="c1", path="c.png", kind="character"),
        ImageInput(image_id="b1", path="b.png", kind="background"),
    ]
    state.shots = [Shot(shot_id="s1", title="S1", duration_sec=5, prompt="x")]
    map_assets(state)
    assert state.shots[0].character_image_ids == ["c1"]
    assert state.shots[0].background_image_ids == ["b1"]


def test_map_assets_infers_from_vl_tags_when_kind_unknown():
    from nanocodex.storyboard.models import ImageInput

    obj = _valid_obj()
    state = ingest(obj)
    state.images = [
        ImageInput(image_id="a", path="a.png", kind="unknown"),
        ImageInput(image_id="b", path="b.png", kind="unknown"),
    ]
    state.asset_analysis = [
        AssetAnalysis(image_id="a", summary="", usable_for=["character close-up"]),
        AssetAnalysis(image_id="b", summary="", scene_tags=["corridor"]),
    ]
    state.shots = [Shot(shot_id="s1", title="S1", duration_sec=5, prompt="x")]
    map_assets(state)
    assert "a" in state.shots[0].character_image_ids
    assert state.shots[0].background_image_ids == ["b"]


# --- build_payloads ---------------------------------------------------------


def test_build_payloads_shape():
    from nanocodex.storyboard.models import ImageInput

    obj = _valid_obj()
    state = ingest(obj)
    # An http(s) URL must pass straight through to ARK's image_url.url.
    state.images = [ImageInput(image_id="c1", path="https://x/c.png", kind="character")]
    state.shots = [
        Shot(
            shot_id="s1", title="S1", duration_sec=8, prompt="a knight stands",
            negative_prompt="no modern objects", character_image_ids=["c1"],
        )
    ]
    build_payloads(state)
    assert len(state.payloads) == 1
    payload = state.payloads[0].payload
    assert payload["ratio"] == "16:9"
    assert payload["duration"] == 8
    assert payload["watermark"] is False
    # text block carries prompt + negative; reference_image carries the URL.
    text_block = payload["content"][0]
    assert text_block["type"] == "text"
    assert "no modern objects" in text_block["text"]
    ref = [c for c in payload["content"] if c.get("role") == "reference_image"]
    assert ref and ref[0]["image_url"]["url"] == "https://x/c.png"


def test_build_payloads_encodes_local_file_to_data_uri(tmp_path):
    # A local image path must be base64-encoded into a data URI (ARK rejects a
    # raw disk path with HTTP 400), NOT passed through verbatim.
    from nanocodex.storyboard.models import ImageInput

    img = tmp_path / "c.png"
    img.write_bytes(_PNG)
    obj = _valid_obj()
    state = ingest(obj)
    state.images = [ImageInput(image_id="c1", path=str(img), kind="character")]
    state.shots = [
        Shot(shot_id="s1", title="S1", duration_sec=5, prompt="x",
             character_image_ids=["c1"])
    ]
    build_payloads(state)
    ref = [c for c in state.payloads[0].payload["content"]
           if c.get("role") == "reference_image"]
    assert ref and ref[0]["image_url"]["url"].startswith("data:image/png;base64,")


def test_build_payloads_drops_unreadable_local_file(tmp_path):
    # A local path that can't be read is DROPPED (shot still renders from text),
    # never sent as a raw path that would 400 the whole submit.
    from nanocodex.storyboard.models import ImageInput

    obj = _valid_obj()
    state = ingest(obj)
    state.images = [ImageInput(image_id="c1", path=str(tmp_path / "missing.png"),
                               kind="character")]
    state.shots = [
        Shot(shot_id="s1", title="S1", duration_sec=5, prompt="x",
             character_image_ids=["c1"])
    ]
    build_payloads(state)
    content = state.payloads[0].payload["content"]
    assert not [c for c in content if c.get("role") == "reference_image"]
    assert content[0]["type"] == "text"  # text block still present


def test_build_payloads_injects_chinese_dialogue():
    # Dialogue must reach the payload text in its ORIGINAL Chinese, quoted, with
    # an explicit 普通话 declaration — that's what drives Seedance's Chinese voice
    # (per ARK docs). The English画面 prompt is untouched.
    obj = _valid_obj()
    state = ingest(obj)
    state.shots = [
        Shot(shot_id="s1", title="S1", duration_sec=5, prompt="a vendor at a stall",
             dialogue=["老板:来一杯鲜爽", "顾客:好嘞"])
    ]
    build_payloads(state)
    text = state.payloads[0].payload["content"][0]["text"]
    assert "a vendor at a stall" in text          # English prompt preserved
    assert "「老板:来一杯鲜爽」" in text            # line 1 quoted, in Chinese
    assert "「顾客:好嘞」" in text                  # line 2 quoted, in Chinese
    assert "普通话" in text                         # explicit mandarin declaration


def test_build_payloads_no_dialogue_adds_no_speech_block():
    # No dialogue -> no 普通话 speech text injected (the prompt stays clean).
    obj = _valid_obj()
    state = ingest(obj)
    state.shots = [Shot(shot_id="s1", title="S1", duration_sec=5, prompt="silent scene")]
    build_payloads(state)
    text = state.payloads[0].payload["content"][0]["text"]
    assert "普通话" not in text


def test_dialogue_independent_of_caption_language():
    # caption_language governs RENDERED on-screen text; dialogue governs SPOKEN
    # audio. They're orthogonal: dialogue stays Chinese even when captions are off.
    from nanocodex.storyboard.models import Project

    obj = _valid_obj()
    state = ingest(obj)
    state.project = Project(id="p1", title="T", caption_language="none")
    state.shots = [Shot(shot_id="s1", title="S1", duration_sec=5, prompt="x",
                        dialogue=["老板:来一杯鲜爽"])]
    build_payloads(state)
    text = state.payloads[0].payload["content"][0]["text"]
    assert "「老板:来一杯鲜爽」" in text                                  # dialogue still spoken
    assert "Do not render any on-screen text" in text                  # captions still off


def test_scan_multi_action_flags_sequence_markers():
    # A shot whose camera/prompt/prompt_zh carries a sequence/multi-scene marker
    # ("montage" / "然后" / "第二天" …) is flagged; a single-instant shot isn't.
    from nanocodex.storyboard.pipeline import scan_multi_action_shots

    obj = _valid_obj()
    state = ingest(obj)
    state.shots = [
        Shot(shot_id="s1", title="clean", duration_sec=5,
             prompt="a rabbit tumbles in", prompt_zh="兔子滚进门"),
        Shot(shot_id="s2", title="montage", duration_sec=6,
             camera="quick montage, cut to kitchen",
             prompt="day one then day two", prompt_zh="第一天，然后第二天"),
    ]
    hits = scan_multi_action_shots(state)
    assert "s1" not in hits                       # single instant -> clean
    assert "s2" in hits                           # multiple ordered beats -> flagged
    assert any(m in hits["s2"] for m in ("montage", "then", "然后", "第二天"))


def test_scan_multi_action_all_clean_is_empty():
    # No markers anywhere -> empty dict (the GUI shows no warning).
    from nanocodex.storyboard.pipeline import scan_multi_action_shots

    obj = _valid_obj()
    state = ingest(obj)
    state.shots = [
        Shot(shot_id="s1", title="a", duration_sec=5, prompt="a cat sits",
             prompt_zh="一只猫坐着"),
        Shot(shot_id="s2", title="b", duration_sec=5, prompt="a door opens",
             prompt_zh="门打开"),
    ]
    assert scan_multi_action_shots(state) == {}


# --- full pipeline with fakes (offline) -------------------------------------


class _FakeVision:
    async def analyze(self, image_id, image_path):
        return AssetAnalysis(image_id=image_id, summary="a thing",
                             usable_for=["background"])


class _FakePlanner:
    # Accepts the optional `chapters` kwarg the pipeline now passes; records
    # whether it was given so a test can assert the chapter layer flowed through.
    def __init__(self):
        self.saw_chapters = None

    async def plan(self, story_text, *, aspect_ratio="16:9", global_style="",
                   chapters=None):
        self.saw_chapters = chapters
        return [
            Shot(shot_id="shot_01", title="Open", duration_sec=5, prompt="scene one"),
            Shot(shot_id="shot_02", title="Close", duration_sec=6, prompt="scene two"),
        ]


class _FakeChapters:
    async def plan(self, story_text, *, language="zh"):
        return [
            Chapter(chapter_id="ch_01", title="起", summary="开场",
                    setting="雪夜", characters=["猫"], key_moments=["客栈亮灯"]),
            Chapter(chapter_id="ch_02", title="承", summary="转折"),
        ]


class _FakeSeedance:
    def generate(self, payload, *, on_progress=None, **kw):
        if on_progress:
            on_progress(0, "succeeded")
        # Mirror the real client: return a SeedanceResult carrying usage so the
        # pipeline can register cost. 108900 is the live-verified 5s/720p count.
        return SeedanceResult(
            video_url="https://example.com/video.mp4?sig=abc",
            usage={"completion_tokens": 108900, "total_tokens": 108900},
        )


async def test_pipeline_offline_no_render(tmp_path):
    p = tmp_path / "a.png"
    p.write_bytes(_PNG)
    obj = _valid_obj([p])
    deps = PipelineDeps(vision=_FakeVision(), planner=_FakePlanner(), seedance=_FakeSeedance())
    state, written = await run_pipeline(obj, deps, out_dir=tmp_path / "out", render_video=False)

    assert len(state.shots) == 2
    assert len(state.asset_analysis) == 1
    assert len(state.payloads) == 2
    assert state.video_urls == {}  # render off -> no spend
    # three JSON files exist and parse.
    for name in ("asset_analysis.json", "storyboard.json", "seedance_payloads.json"):
        data = json.loads((tmp_path / "out" / name).read_text(encoding="utf-8"))
        assert isinstance(data, list)
    assert not (tmp_path / "out" / "video_urls.json").exists()


async def test_pipeline_offline_with_render(tmp_path):
    p = tmp_path / "a.png"
    p.write_bytes(_PNG)
    obj = _valid_obj([p])
    deps = PipelineDeps(vision=_FakeVision(), planner=_FakePlanner(), seedance=_FakeSeedance())
    state, written = await run_pipeline(obj, deps, out_dir=tmp_path / "out", render_video=True)

    assert set(state.video_urls) == {"shot_01", "shot_02"}
    assert all(u.startswith("https://") for u in state.video_urls.values())
    urls_doc = json.loads((tmp_path / "out" / "video_urls.json").read_text(encoding="utf-8"))
    assert "expire" in urls_doc["_note"]


# --- chapter layer + two-phase plan/render ----------------------------------


async def test_plan_chapters_flow_into_shot_planner(tmp_path):
    # When a chapter planner is injected, its chapters land in state AND are
    # passed through to the shot planner (so shots are sliced chapter by chapter).
    obj = _valid_obj()
    planner = _FakePlanner()
    deps = PipelineDeps(chapters=_FakeChapters(), planner=planner)
    state = await run_planning(obj, deps)
    assert [c.chapter_id for c in state.chapters] == ["ch_01", "ch_02"]
    # The planner received the chapters list (not None) — chapter layer flowed.
    assert planner.saw_chapters is not None
    assert len(planner.saw_chapters) == 2


async def test_shot_planner_backward_compatible_without_chapters():
    # No chapter planner injected -> planner is called with chapters=None, so
    # callers predating the chapter layer behave exactly as before.
    obj = _valid_obj()
    planner = _FakePlanner()
    deps = PipelineDeps(planner=planner)  # no `chapters` client
    state = await run_planning(obj, deps)
    assert state.chapters == []
    assert planner.saw_chapters is None


async def test_run_planning_never_renders(tmp_path):
    # The "preview" path: chapters + shots + payloads filled, but NO video and
    # NO Seedance call even when a seedance client is present.
    obj = _valid_obj()

    class _BoomSeedance:
        def generate(self, *a, **k):
            raise AssertionError("run_planning must never call Seedance")

    deps = PipelineDeps(chapters=_FakeChapters(), planner=_FakePlanner(),
                        seedance=_BoomSeedance())
    state = await run_planning(obj, deps, out_dir=tmp_path / "out")
    assert len(state.shots) == 2
    assert len(state.payloads) == 2
    assert state.video_urls == {}
    assert state.video_costs == {}
    # chapters.json is written by the planning export; no video files.
    chapters_doc = json.loads((tmp_path / "out" / "chapters.json").read_text(encoding="utf-8"))
    assert [c["chapter_id"] for c in chapters_doc] == ["ch_01", "ch_02"]
    assert not (tmp_path / "out" / "video_urls.json").exists()


async def test_render_state_renders_already_planned(tmp_path):
    # The "make video" path: plan first (no spend), then render the SAME state.
    obj = _valid_obj()
    deps_plan = PipelineDeps(chapters=_FakeChapters(), planner=_FakePlanner())
    state = await run_planning(obj, deps_plan)
    assert state.video_urls == {}  # nothing rendered yet

    deps_render = PipelineDeps(seedance=_FakeSeedance())
    state2, written = render_state(state, deps_render, out_dir=tmp_path / "out")
    assert set(state2.video_urls) == {"shot_01", "shot_02"}
    assert set(state2.video_costs) == {"shot_01", "shot_02"}
    assert "video_urls.json" in written


async def test_render_one_reruns_single_failed_shot(tmp_path):
    # A failed shot can be re-rendered on its own: render_one updates only that
    # shot in place. First fail it, then retry with a good client -> success,
    # and the OTHER shot's state is untouched (no re-spend on the good one).
    from nanocodex.storyboard.clients import SeedanceError
    from nanocodex.storyboard.pipeline import render_one

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))

    class _BoomOnce:
        def generate(self, *a, **k):
            raise SeedanceError("RemoteDisconnected: connection closed")

    # shot_01 fails -> records a [failed: ...] marker, no cost, returns False.
    ok = render_one(state, PipelineDeps(seedance=_BoomOnce()), "shot_01")
    assert ok is False
    assert state.video_urls["shot_01"].startswith("[failed:")
    assert "shot_01" not in state.video_costs

    # Retry shot_01 with a working client -> success replaces the marker, the
    # stale failure is cleared, and a cost entry appears. shot_02 stays absent.
    ok2 = render_one(state, PipelineDeps(seedance=_FakeSeedance()), "shot_01")
    assert ok2 is True
    assert state.video_urls["shot_01"].startswith("https://")
    assert "shot_01" in state.video_costs
    assert "shot_02" not in state.video_urls  # untouched -> no re-spend


async def test_render_concurrent_renders_all_shots(tmp_path):
    # Concurrent render: with a slow client, N shots running in parallel finish
    # in roughly one shot's time, not N times it. Asserts BOTH that every shot
    # rendered (no lost/clobbered writes) and that wall-clock ~= one shot, not
    # the serial sum.
    import time as _time

    from nanocodex.storyboard.pipeline import render

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))

    class _SlowSeedance:
        def generate(self, payload, *, on_progress=None, **kw):
            _time.sleep(0.3)  # simulate the submit+poll latency of one clip
            return SeedanceResult(
                video_url="https://example.com/v.mp4?sig=x",
                usage={"completion_tokens": 108900, "total_tokens": 108900},
            )

    t0 = _time.monotonic()
    render(state, PipelineDeps(seedance=_SlowSeedance()), max_workers=4)
    elapsed = _time.monotonic() - t0

    # Both shots rendered (concurrent writes to distinct keys didn't clobber).
    assert set(state.video_urls) == {"shot_01", "shot_02"}
    assert all(u.startswith("https://") for u in state.video_urls.values())
    assert set(state.video_costs) == {"shot_01", "shot_02"}
    # 2 shots x 0.3s serial would be ~0.6s; concurrent should be well under that.
    assert elapsed < 0.5


async def test_render_serial_when_max_workers_one(tmp_path):
    # max_workers <= 1 falls back to the serial path (still renders everything).
    from nanocodex.storyboard.pipeline import render

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))
    render(state, PipelineDeps(seedance=_FakeSeedance()), max_workers=1)
    assert set(state.video_urls) == {"shot_01", "shot_02"}


# --- 前后帧衔接: chain_frames (first_frame <- previous shot's last frame) ------


def test_set_first_frame_replaces_image_blocks():
    # _set_first_frame drops existing image_url blocks (subject reference_image)
    # and adds exactly one first_frame, leaving the text block untouched.
    from nanocodex.storyboard.pipeline import _set_first_frame

    payload = {"content": [
        {"type": "text", "text": "a knight stands"},
        {"type": "image_url", "image_url": {"url": "https://x/c.png"},
         "role": "reference_image"},
    ]}
    _set_first_frame(payload, "data:image/jpeg;base64,AAAA")
    imgs = [b for b in payload["content"] if b.get("type") == "image_url"]
    assert len(imgs) == 1
    assert imgs[0]["role"] == "first_frame"
    assert imgs[0]["image_url"]["url"] == "data:image/jpeg;base64,AAAA"
    # text block survives; no leftover reference_image.
    assert payload["content"][0]["type"] == "text"
    assert not [b for b in payload["content"]
                if b.get("role") == "reference_image"]


async def test_render_chained_injects_first_frame_from_prev_last_frame():
    # chain_frames=True: shot_01 renders first (no first_frame), its extracted
    # last frame is injected as shot_02's first_frame. Order is serial.
    from nanocodex.storyboard.pipeline import render

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))

    rendered_order: list[str] = []

    class _RecSeedance:
        def generate(self, payload, *, on_progress=None, **kw):
            # Record which shot (by its prompt text) ran, in order.
            text = payload["content"][0]["text"]
            rendered_order.append(text)
            return SeedanceResult(
                video_url=f"https://example.com/{len(rendered_order)}.mp4",
                usage={"total_tokens": 108900})

    def _fake_extract(url: str) -> str:
        # Every clip yields a deterministic data-URI standing in for its last frame.
        return f"data:image/jpeg;base64,FRAME_OF_{url[-5:]}"

    render(state, PipelineDeps(seedance=_RecSeedance()),
           chain_frames=True, extract_frame=_fake_extract)

    assert set(state.video_urls) == {"shot_01", "shot_02"}
    # shot_01 rendered before shot_02 (serial chain). The prompt text gets a
    # caption directive appended, so match the leading scene description.
    assert len(rendered_order) == 2
    assert rendered_order[0].startswith("scene one")
    assert rendered_order[1].startswith("scene two")
    # shot_01's payload has NO first_frame (nothing precedes it); shot_02 does,
    # and it is the frame extracted from shot_01's clip.
    p1 = next(p for p in state.payloads if p.shot_id == "shot_01").payload
    p2 = next(p for p in state.payloads if p.shot_id == "shot_02").payload
    assert not [b for b in p1["content"] if b.get("role") == "first_frame"]
    ff = [b for b in p2["content"] if b.get("role") == "first_frame"]
    assert ff and ff[0]["image_url"]["url"].startswith("data:image/jpeg;base64,FRAME_OF_")


async def test_render_chained_breaks_chain_when_extract_fails():
    # If the previous shot's frame can't be extracted (None), the next shot
    # renders independently — no first_frame injected — and the run still
    # completes every shot.
    from nanocodex.storyboard.pipeline import render

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))

    render(state, PipelineDeps(seedance=_FakeSeedance()),
           chain_frames=True, extract_frame=lambda url: None)

    assert set(state.video_urls) == {"shot_01", "shot_02"}
    for p in state.payloads:
        assert not [b for b in p.payload["content"]
                    if b.get("role") == "first_frame"]


async def test_run_pipeline_forwards_chain_frames(tmp_path, monkeypatch):
    # A5: run_pipeline(chain_frames=True) must drive the SERIAL chained render,
    # not the concurrent path. Stub the ffmpeg frame extractor so no subprocess
    # runs; chain degrades gracefully (None frame) but every shot still renders.
    import nanocodex.storyboard.pipeline as pipe

    monkeypatch.setattr(pipe, "_default_frame_extractor", lambda url: None)

    obj = _valid_obj()
    deps = PipelineDeps(planner=_FakePlanner(), seedance=_FakeSeedance())
    state, _ = await pipe.run_pipeline(obj, deps, render_video=True, chain_frames=True)
    assert set(state.video_urls) == {"shot_01", "shot_02"}


async def test_run_pipeline_chapters_flow_through(tmp_path):
    # A5: a chapter planner injected into run_pipeline produces chapters (the CLI
    # now wires ChapterPlanner so the command line gets the same chapter layer).
    obj = _valid_obj()
    deps = PipelineDeps(chapters=_FakeChapters(), planner=_FakePlanner())
    state, _ = await run_pipeline(obj, deps, render_video=False)
    assert [c.chapter_id for c in state.chapters] == ["ch_01", "ch_02"]


async def test_render_chained_failed_shot_does_not_anchor_next():
    # A shot that FAILS produces no last frame, so the chain breaks there: the
    # following shot renders without a first_frame rather than inheriting a
    # stale/empty one.
    from nanocodex.storyboard.clients import SeedanceError
    from nanocodex.storyboard.pipeline import render

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))

    class _FailFirst:
        def __init__(self):
            self.n = 0

        def generate(self, payload, *, on_progress=None, **kw):
            self.n += 1
            if self.n == 1:
                raise SeedanceError("boom on shot_01")
            return SeedanceResult(video_url="https://example.com/ok.mp4",
                                  usage={"total_tokens": 108900})

    extracted: list[str] = []

    def _extract(url: str) -> str:
        extracted.append(url)
        return "data:image/jpeg;base64,X"

    render(state, PipelineDeps(seedance=_FailFirst()),
           chain_frames=True, extract_frame=_extract)

    assert state.video_urls["shot_01"].startswith("[failed:")
    assert state.video_urls["shot_02"].startswith("https://")
    # The failed shot_01 has no clip, so its (failure) marker is never handed to
    # extract_frame — the chain breaks and shot_02 renders with no first_frame.
    assert not any("[failed" in u for u in extracted)
    p2 = next(p for p in state.payloads if p.shot_id == "shot_02").payload
    assert not [b for b in p2["content"] if b.get("role") == "first_frame"]


# --- continuity checker (pre-merge review) ----------------------------------


class _FakeChatResp:
    """Minimal stand-in for a provider ModelResponse (only .content is read)."""

    def __init__(self, content):
        self.content = content


class _FakeProvider:
    """Provider-level fake: its async chat() returns a canned reply object.

    The ContinuityChecker calls provider.chat(...) and reads resp.content, so a
    fake that returns a fixed JSON string drives the whole parse path offline.
    Records the last messages so a test can assert what was sent.
    """

    model = "fake"

    def __init__(self, content):
        self._content = content
        self.last_messages = None

    async def chat(self, messages, tools=None, *, temperature=None,
                   max_tokens=None, reasoning_effort=None):
        self.last_messages = messages
        return _FakeChatResp(self._content)


_SHOTS = [
    Shot(shot_id="shot_01", title="A", duration_sec=5, prompt="one",
         prompt_zh="一"),
    Shot(shot_id="shot_02", title="B", duration_sec=5, prompt="two",
         prompt_zh="二"),
]


async def test_continuity_checker_parses_gaps():
    reply = json.dumps({
        "ok": False,
        "summary": "中间缺一个过渡。",
        "gaps": [{
            "after_shot_id": "shot_01",
            "before_shot_id": "shot_02",
            "missing": "缺少人物走向门口的过渡",
            "severity": "high",
            "suggested_title": "走向门口",
            "suggested_prompt_zh": "人物起身走向木门",
            "suggested_prompt": "the figure rises and walks to the door",
            "suggested_duration_sec": 4,
            "suggested_dialogue": ["甲:我们走"],
        }],
    })
    report = await ContinuityChecker(_FakeProvider(reply)).check(_SHOTS)
    assert report.ok is False
    assert report.summary == "中间缺一个过渡。"
    assert len(report.gaps) == 1
    g = report.gaps[0]
    assert g.after_shot_id == "shot_01"
    assert g.before_shot_id == "shot_02"
    assert g.severity == "high"
    assert g.suggested_duration_sec == 4.0
    assert g.suggested_dialogue == ["甲:我们走"]


async def test_continuity_checker_ok_no_gaps():
    # A clean storyboard: ok=true, empty gaps -> a valid GOOD result, NO raise.
    reply = json.dumps({"ok": True, "summary": "连贯。", "gaps": []})
    report = await ContinuityChecker(_FakeProvider(reply)).check(_SHOTS)
    assert report.ok is True
    assert report.gaps == []
    assert report.summary == "连贯。"


async def test_continuity_checker_lenient_bare_list():
    # A bare JSON list (model dropped the wrapper object) is treated as gaps.
    reply = json.dumps([{
        "after_shot_id": "shot_01", "before_shot_id": "shot_02",
        "missing": "缺过渡",
    }])
    report = await ContinuityChecker(_FakeProvider(reply)).check(_SHOTS)
    assert report.ok is False  # non-empty gaps -> not ok
    assert len(report.gaps) == 1
    assert report.gaps[0].missing == "缺过渡"
    # Missing suggestion fields fall back to dataclass defaults (no raise).
    assert report.gaps[0].suggested_duration_sec == 5.0


async def test_check_continuity_none_dep_returns_ok():
    # No checker injected (no DeepSeek key / offline) -> clean report, no raise.
    state = await run_planning(_valid_obj(), PipelineDeps(planner=_FakePlanner()))
    report = await check_continuity(state, PipelineDeps())
    assert report.ok is True
    assert report.gaps == []


async def test_check_continuity_passes_available_ids():
    # available_ids must flow through to the checker untouched (un-rendered shots
    # are real holes the reviewer should weigh).
    class _RecordingChecker:
        def __init__(self):
            self.saw_available = "unset"
            self.saw_shots = None

        async def check(self, shots, *, chapters=None, available_ids=None):
            self.saw_available = available_ids
            self.saw_shots = shots
            return ContinuityReport(ok=True)

    state = await run_planning(_valid_obj(), PipelineDeps(planner=_FakePlanner()))
    rec = _RecordingChecker()
    await check_continuity(state, PipelineDeps(continuity=rec),
                           available_ids=["shot_01"])
    assert rec.saw_available == ["shot_01"]
    assert [s.shot_id for s in rec.saw_shots] == ["shot_01", "shot_02"]


def test_continuity_report_jsonable_round_trip():
    # as_jsonable handles the new dataclasses recursively (export needs no change).
    report = ContinuityReport(
        ok=False, summary="x",
        gaps=[ContinuityGap(after_shot_id="a", before_shot_id="b",
                            missing="m", suggested_dialogue=["甲:hi"])],
    )
    d = as_jsonable(report)
    assert d["ok"] is False
    assert d["gaps"][0]["after_shot_id"] == "a"
    assert d["gaps"][0]["suggested_dialogue"] == ["甲:hi"]
    # JSON-serializable end to end.
    assert json.loads(json.dumps(d))["summary"] == "x"


# --- insert_fill_shot (采纳建议 -> 补镜 -> 重新合并 闭环) --------------------


async def test_insert_fill_shot_wedges_between_neighbours():
    # A gap's suggestion becomes a REAL shot inserted right after after_shot_id,
    # so the merge stitches it between the two shots it bridges. Its payload is
    # built too (so render_one can find it).
    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))
    assert [s.shot_id for s in state.shots] == ["shot_01", "shot_02"]
    n_payloads = len(state.payloads)

    gap = ContinuityGap(
        after_shot_id="shot_01", before_shot_id="shot_02",
        missing="缺过渡", suggested_title="过渡镜",
        suggested_prompt="a transition", suggested_prompt_zh="过渡画面",
        suggested_duration_sec=4, suggested_dialogue=["甲:走"],
    )
    new_id = insert_fill_shot(state, gap)

    # Wedged between shot_01 and shot_02 with a sortable id.
    assert new_id == "shot_01b"
    assert [s.shot_id for s in state.shots] == ["shot_01", "shot_01b", "shot_02"]
    fill = state.shots[1]
    assert fill.title == "过渡镜"
    assert fill.prompt == "a transition"
    assert fill.prompt_zh == "过渡画面"
    assert fill.duration_sec == 4.0
    assert fill.dialogue == ["甲:走"]
    # A payload was built for the new shot so render_one can render it.
    assert len(state.payloads) == n_payloads + 1
    assert any(p.shot_id == new_id for p in state.payloads)


async def test_insert_fill_shot_inherits_neighbour_reference_images():
    # The fill shot copies its anchor (after) shot's character/background image
    # ids, so it renders with the same cast/scene as its surroundings.
    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))
    state.shots[0].character_image_ids = ["c1"]
    state.shots[0].background_image_ids = ["b1"]

    gap = ContinuityGap(after_shot_id="shot_01", before_shot_id="shot_02",
                        missing="x", suggested_title="t")
    new_id = insert_fill_shot(state, gap)
    fill = next(s for s in state.shots if s.shot_id == new_id)
    assert fill.character_image_ids == ["c1"]
    assert fill.background_image_ids == ["b1"]


async def test_insert_fill_shot_appends_when_anchor_missing():
    # An unknown after_shot_id falls back to appending at the end (still valid).
    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))
    gap = ContinuityGap(after_shot_id="nope", before_shot_id="shot_02",
                        missing="x", suggested_title="t")
    new_id = insert_fill_shot(state, gap)
    assert state.shots[-1].shot_id == new_id


async def test_insert_fill_shot_then_render_one(tmp_path):
    # The full loop: insert a fill shot, then render JUST it via render_one
    # (real client path with a fake transport). Only the new shot gets a clip.
    from nanocodex.storyboard.pipeline import render_one

    obj = _valid_obj()
    state = await run_planning(obj, PipelineDeps(planner=_FakePlanner()))
    gap = ContinuityGap(after_shot_id="shot_01", before_shot_id="shot_02",
                        missing="x", suggested_title="t", suggested_duration_sec=5)
    new_id = insert_fill_shot(state, gap)

    ok = render_one(state, PipelineDeps(seedance=_FakeSeedance()), new_id)
    assert ok is True
    assert state.video_urls[new_id].startswith("https://")
    assert new_id in state.video_costs
    # The originals weren't rendered by this call (no re-spend).
    assert "shot_01" not in state.video_urls
    assert "shot_02" not in state.video_urls


def _fake_ark_transport(total_tokens=108900):
    """A SeedanceClient transport that scripts ARK submit/poll with NO network.

    POST (submit) -> a per-task id; GET (poll) -> 'succeeded' with a video_url and
    a usage.total_tokens, so the WHOLE real client path (submit -> poll -> parse
    usage) runs offline. Drives the end-to-end cost-registration test below.
    """
    counter = {"n": 0}

    def _t(method, url, headers, body):
        if method == "POST":
            counter["n"] += 1
            return (200, json.dumps({"id": f"task_{counter['n']}"}))
        # GET poll: succeed immediately with a billable usage block.
        return (200, json.dumps({
            "status": "succeeded",
            "content": {"video_url": "https://ark/clip.mp4?sig=x"},
            "usage": {"completion_tokens": total_tokens, "total_tokens": total_tokens},
        }))
    return _t


async def test_pipeline_render_registers_cost_via_real_client(tmp_path):
    # End-to-end through the REAL SeedanceClient (only the HTTP transport is fake),
    # so submit -> poll -> usage parsing -> seedance_cost_cny -> video_cost.json
    # is exercised for real. No network, no key, no spend.
    p = tmp_path / "a.png"
    p.write_bytes(_PNG)
    obj = _valid_obj([p])
    client = SeedanceClient("k", transport=_fake_ark_transport(108900),
                            sleep=lambda s: None)
    deps = PipelineDeps(vision=_FakeVision(), planner=_FakePlanner(), seedance=client)
    state, written = await run_pipeline(obj, deps, out_dir=tmp_path / "out",
                                        render_video=True)

    # Both shots rendered and BOTH got a cost entry (only successes are billed).
    assert set(state.video_urls) == {"shot_01", "shot_02"}
    assert set(state.video_costs) == {"shot_01", "shot_02"}

    # Per-shot: 108900 tok, no video input (text+image payload), 37 CNY/1M ->
    #   108900 * 37 / 1e6 = 4.0293 CNY each.
    for sid in ("shot_01", "shot_02"):
        c = state.video_costs[sid]
        assert c["total_tokens"] == 108900
        assert c["has_video_input"] is False
        assert abs(c["cost_cny"] - 4.0293) < 1e-9

    # video_cost.json is written and its aggregates are correct: 2 shots ->
    #   total_tokens = 217800, total_cost_cny = 8.0586 CNY.
    assert "video_cost.json" in written
    cost_doc = json.loads((tmp_path / "out" / "video_cost.json").read_text(encoding="utf-8"))
    assert cost_doc["currency"] == "CNY"
    assert cost_doc["total_tokens"] == 217800
    assert abs(cost_doc["total_cost_cny"] - 8.0586) < 1e-9
    assert set(cost_doc["per_shot"]) == {"shot_01", "shot_02"}


async def test_pipeline_render_failure_is_not_billed(tmp_path):
    # A shot whose task fails (raises in the real client) must NOT get a cost
    # entry -- the package bills only successful generations.
    def _failing_transport(method, url, headers, body):
        if method == "POST":
            return (200, json.dumps({"id": "task_x"}))
        return (200, json.dumps({"status": "failed"}))

    p = tmp_path / "a.png"
    p.write_bytes(_PNG)
    obj = _valid_obj([p])
    client = SeedanceClient("k", transport=_failing_transport, sleep=lambda s: None)
    deps = PipelineDeps(vision=_FakeVision(), planner=_FakePlanner(), seedance=client)
    state, written = await run_pipeline(obj, deps, out_dir=tmp_path / "out",
                                        render_video=True)

    # URLs record the failure marker; costs stay empty -> no video_cost.json.
    assert all(v.startswith("[failed:") for v in state.video_urls.values())
    assert state.video_costs == {}
    assert not (tmp_path / "out" / "video_cost.json").exists()


# --- SeedanceClient submit/poll parsing via fake transport ------------------


def _scripted_transport(responses):
    """Return a transport callable that pops (status, body) per call."""
    calls = list(responses)

    def _t(method, url, headers, body):
        return calls.pop(0)
    return _t


def test_seedance_requires_key():
    with pytest.raises(SeedanceError, match="ARK API key"):
        SeedanceClient("")


def test_seedance_generate_happy_path():
    transport = _scripted_transport([
        (200, json.dumps({"id": "task_1"})),                         # submit
        (200, json.dumps({"status": "running"})),                    # poll 1
        (200, json.dumps({"status": "succeeded",
                          "content": {"video_url": "https://v/clip.mp4"},
                          "usage": {"total_tokens": 108900}})),      # poll 2
    ])
    client = SeedanceClient("k", transport=transport, sleep=lambda s: None)
    result = client.generate({"model": "m"}, max_polls=5, interval_s=0)
    # generate now returns a SeedanceResult carrying the URL + billing usage.
    assert result.video_url == "https://v/clip.mp4"
    assert result.usage.get("total_tokens") == 108900


def test_seedance_generate_raises_on_failed():
    transport = _scripted_transport([
        (200, json.dumps({"id": "task_2"})),
        (200, json.dumps({"status": "failed"})),
    ])
    client = SeedanceClient("k", transport=transport, sleep=lambda s: None)
    with pytest.raises(SeedanceError, match="failed"):
        client.generate({"model": "m"}, max_polls=5, interval_s=0)


def test_seedance_submit_http_error():
    transport = _scripted_transport([(400, '{"error": "bad"}')])
    client = SeedanceClient("k", transport=transport, sleep=lambda s: None)
    with pytest.raises(SeedanceError, match="submit failed"):
        client.submit({"model": "m"})


# --- run archiving: make_run_dir / index / concat --------------------------


def test_make_run_dir_names_with_timestamp_and_slug(tmp_path):
    from datetime import datetime

    when = datetime(2026, 6, 11, 17, 9)
    d = make_run_dir(tmp_path, "山海经客栈的故事", when=when)
    assert d.parent == tmp_path / "runs"
    assert d.name == "20260611-1709_山海经客栈的故事"
    assert d.is_dir()


def test_make_run_dir_sanitizes_illegal_chars_and_caps_length(tmp_path):
    from datetime import datetime

    when = datetime(2026, 6, 11, 17, 9)
    # Illegal chars -> "_"; title longer than 20 chars is truncated.
    d = make_run_dir(tmp_path, 'a/b:c*?"<>|d ' + "x" * 40, when=when)
    slug = d.name.split("_", 1)[1]
    assert len(slug) <= 20
    for bad in '\\/:*?"<>|':
        assert bad not in slug


def test_make_run_dir_blank_title_falls_back_to_untitled(tmp_path):
    from datetime import datetime

    when = datetime(2026, 6, 11, 17, 9)
    d = make_run_dir(tmp_path, "   ", when=when)
    assert d.name == "20260611-1709_untitled"


def test_make_run_dir_dedupes_same_minute(tmp_path):
    from datetime import datetime

    when = datetime(2026, 6, 11, 17, 9)
    d1 = make_run_dir(tmp_path, "story", when=when)
    d2 = make_run_dir(tmp_path, "story", when=when)
    # Second run in the same minute must NOT reuse the first dir.
    assert d1 != d2
    assert d2.name.endswith("-2")
    assert d1.is_dir() and d2.is_dir()


def test_run_index_append_and_read(tmp_path):
    write_run_index(tmp_path, {"run_id": "r1", "title": "A", "cost_cny": 1.0})
    write_run_index(tmp_path, {"run_id": "r2", "title": "B", "cost_cny": 2.0})
    entries = read_run_index(tmp_path)
    assert [e["run_id"] for e in entries] == ["r1", "r2"]


def test_run_index_same_run_id_replaces_in_place(tmp_path):
    write_run_index(tmp_path, {"run_id": "r1", "ok": 3, "cost_cny": 1.0})
    # A 重试 updates the same run's success count/cost — same row, not a dup.
    write_run_index(tmp_path, {"run_id": "r1", "ok": 5, "cost_cny": 1.5})
    entries = read_run_index(tmp_path)
    assert len(entries) == 1
    assert entries[0]["ok"] == 5
    assert entries[0]["cost_cny"] == 1.5


def test_read_run_index_missing_is_empty(tmp_path):
    assert read_run_index(tmp_path) == []


def test_load_run_state_roundtrips_exported_run(tmp_path):
    # Plan a state, render it with a fake Seedance, export to a run dir, then
    # load it back — the reloaded state should carry the same shots, payloads,
    # video urls and costs so a past 出片 can be replayed/retried/merged.
    from nanocodex.storyboard.pipeline import export

    state = ingest(_valid_obj())
    state.chapters = [Chapter(chapter_id="ch_01", title="第一章", summary="开场")]
    state.shots = [
        Shot(shot_id="shot_01", title="镜一", duration_sec=5.0,
             prompt="a cat", prompt_zh="一只猫", chapter_id="ch_01",
             dialogue=["猫:喵"]),
        Shot(shot_id="shot_02", title="镜二", duration_sec=5.0, prompt="a dog"),
    ]
    state = build_payloads(state)
    state.video_urls = {"shot_01": "https://x/1.mp4",
                        "shot_02": "[failed: SeedanceError: boom]"}
    state.video_costs = {"shot_01": {"total_tokens": 1000, "has_video_input": False,
                                     "cost_cny": 0.037}}
    run_dir = make_run_dir(tmp_path, "山海经客栈")
    export(state, run_dir)

    meta = {"run_id": "r1", "title": "山海经客栈", "ratio": "9:16",
            "caption_language": "en"}
    loaded = load_run_state(run_dir, meta=meta)

    assert [s.shot_id for s in loaded.shots] == ["shot_01", "shot_02"]
    assert loaded.shots[0].prompt_zh == "一只猫"
    assert loaded.shots[0].dialogue == ["猫:喵"]
    assert [c.chapter_id for c in loaded.chapters] == ["ch_01"]
    assert [p.shot_id for p in loaded.payloads] == ["shot_01", "shot_02"]
    assert loaded.video_urls == state.video_urls
    assert loaded.video_costs["shot_01"]["cost_cny"] == 0.037
    # project fields come from the meta row (project/story_text aren't exported)
    assert loaded.project.title == "山海经客栈"
    assert loaded.project.aspect_ratio == "9:16"
    assert loaded.project.caption_language == "en"


def test_load_run_state_partial_run_degrades_to_empty(tmp_path):
    # A run dir with only storyboard.json (no urls/costs/payloads) still loads:
    # missing files become empty rather than raising.
    run_dir = make_run_dir(tmp_path, "partial")
    (run_dir / "storyboard.json").write_text(
        json.dumps([{"shot_id": "shot_01", "title": "x", "duration_sec": 5.0,
                     "prompt": "p"}], ensure_ascii=False),
        encoding="utf-8")
    loaded = load_run_state(run_dir)
    assert [s.shot_id for s in loaded.shots] == ["shot_01"]
    assert loaded.video_urls == {}
    assert loaded.video_costs == {}
    assert loaded.payloads == []
    # No meta -> minimal default project.
    assert loaded.project.title == "(loaded run)"


def test_load_run_state_drops_unknown_fields(tmp_path):
    # JSON written by a future export with extra keys still loads (unknown keys
    # dropped, missing optional fields fall back to dataclass defaults).
    run_dir = make_run_dir(tmp_path, "fwd")
    (run_dir / "storyboard.json").write_text(
        json.dumps([{"shot_id": "shot_01", "title": "x", "duration_sec": 5.0,
                     "prompt": "p", "brand_new_field": "ignored"}]),
        encoding="utf-8")
    loaded = load_run_state(run_dir)
    assert loaded.shots[0].shot_id == "shot_01"
    assert not hasattr(loaded.shots[0], "brand_new_field")


def _touch_mp4(d, name, size=64):
    p = d / name
    p.write_bytes(b"\x00" * size)
    return p


def test_concat_clips_returns_none_with_fewer_than_two(tmp_path):
    _touch_mp4(tmp_path, "shot_01.mp4")
    # Only one real clip -> nothing meaningful to merge.
    assert concat_clips(tmp_path, ["shot_01", "shot_02"]) is None


def test_concat_clips_uniform_uses_copy(tmp_path):
    _touch_mp4(tmp_path, "shot_01.mp4")
    _touch_mp4(tmp_path, "shot_02.mp4")
    calls = []

    def fake_runner(argv):
        calls.append(argv)
        if argv[0] == "ffprobe":
            return 0, "1280,720,30/1"          # identical params for every clip
        # ffmpeg concat: pretend it produced the output file
        (tmp_path / "full.mp4").write_bytes(b"\x00" * 128)
        return 0, ""

    out = concat_clips(tmp_path, ["shot_01", "shot_02"], runner=fake_runner)
    assert out == tmp_path / "full.mp4"
    ffmpeg = [c for c in calls if c[0] == "ffmpeg"][0]
    assert "-c" in ffmpeg and "copy" in ffmpeg          # fast lossless path
    assert "concat" in ffmpeg


def test_concat_clips_mismatch_reencodes(tmp_path):
    _touch_mp4(tmp_path, "shot_01.mp4")
    _touch_mp4(tmp_path, "shot_02.mp4")
    seq = ["1280,720,30/1", "1920,1080,24/1"]           # differing params
    calls = []

    def fake_runner(argv):
        calls.append(argv)
        if argv[0] == "ffprobe":
            return 0, seq.pop(0)
        (tmp_path / "full.mp4").write_bytes(b"\x00" * 128)
        return 0, ""

    out = concat_clips(tmp_path, ["shot_01", "shot_02"], runner=fake_runner)
    assert out == tmp_path / "full.mp4"
    ffmpeg = [c for c in calls if c[0] == "ffmpeg"][0]
    assert "-filter_complex" in ffmpeg                  # re-encode path
    assert "libx264" in ffmpeg


def test_concat_clips_skips_missing_shots(tmp_path):
    # shot_02 is missing; only 01 and 03 exist -> they're the ones concatenated.
    _touch_mp4(tmp_path, "shot_01.mp4")
    _touch_mp4(tmp_path, "shot_03.mp4")
    listed = []

    def fake_runner(argv):
        if argv[0] == "ffprobe":
            return 0, "1280,720,30/1"
        # Capture the concat list file's contents before ffmpeg "runs".
        if "concat" in argv:
            lp = tmp_path / "_concat_list.txt"
            listed.append(lp.read_text(encoding="utf-8"))
        (tmp_path / "full.mp4").write_bytes(b"\x00" * 128)
        return 0, ""

    concat_clips(tmp_path, ["shot_01", "shot_02", "shot_03"], runner=fake_runner)
    assert listed and "shot_01.mp4" in listed[0]
    assert "shot_03.mp4" in listed[0]
    assert "shot_02.mp4" not in listed[0]


def test_concat_clips_raises_when_ffmpeg_fails(tmp_path):
    _touch_mp4(tmp_path, "shot_01.mp4")
    _touch_mp4(tmp_path, "shot_02.mp4")

    def fake_runner(argv):
        if argv[0] == "ffprobe":
            return 0, "1280,720,30/1"
        return 1, "some ffmpeg error"

    with pytest.raises(StoryboardError, match="ffmpeg concat"):
        concat_clips(tmp_path, ["shot_01", "shot_02"], runner=fake_runner)
