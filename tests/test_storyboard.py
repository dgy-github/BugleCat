"""Offline tests for the storyboard pipeline.

Everything runs with fakes — no network, no real keys, no Seedance spend:
* schema validation (valid + invalid shapes)
* rule-based asset mapping
* Seedance payload assembly
* the full pipeline via fake Vision/Planner/Seedance clients (render off + on)
* SeedanceClient submit/poll parsing via a scripted fake transport
"""

from __future__ import annotations

import base64
import json

import pytest

from nanocodex.storyboard.clients import (
    SeedanceClient,
    SeedanceError,
    SeedanceResult,
    _extract_json,
)
from nanocodex.storyboard.models import (
    AssetAnalysis,
    Chapter,
    Shot,
    StoryboardError,
    validate_project,
)
from nanocodex.storyboard.pipeline import (
    PipelineDeps,
    build_payloads,
    ingest,
    map_assets,
    render_state,
    run_pipeline,
    run_planning,
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
    from nanocodex.storyboard.pipeline import render_one
    from nanocodex.storyboard.clients import SeedanceError

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
