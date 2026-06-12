"""storyboard: turn story text + reference images into a storyboard (and video).

Wraps the storyboard sub-package (nanocodex/storyboard) as an agent tool. Given a
story and a few image paths, it analyses each image with Qwen-VL, plans 6-10
shots with the main model, maps images to shots, assembles one Seedance payload
per shot, and writes the three result JSON files. Real video rendering is OFF by
default (Seedance bills per clip); set ``render=true`` to actually generate.

Config comes from the same layered config as the rest of nanocodex:
* text planning uses the main provider (DeepSeek),
* image analysis uses the VL backend (vl_base_url / vl_api_key / vl_model — the
  same fields the GUI Settings "Vision (VL) backend" section writes),
* rendering uses the ARK key from env ARK_API_KEY.

Missing config is reported as a clear tool error rather than a crash, so the
agent can relay exactly what the user still needs to set.
"""

from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any

from nanocodex.tools.base import Tool


class StoryboardTool(Tool):
    @property
    def name(self) -> str:
        return "storyboard"

    @property
    def description(self) -> str:
        return (
            "Turn a STORY plus a few reference IMAGES into a storyboard: analyse "
            "each image (Qwen-VL), split the story into 6-10 shots, map images to "
            "shots, and build one Seedance video payload per shot. Writes "
            "asset_analysis.json / storyboard.json / seedance_payloads.json to "
            "out_dir. Use when the user wants to plan a text-to-video shoot from a "
            "story + images. Set render=true ONLY when the user explicitly wants "
            "real video clips generated — that calls Seedance and COSTS MONEY per "
            "clip (needs ARK_API_KEY). Image analysis needs a VL backend "
            "configured (vl_model in settings); without it, analysis is skipped."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "story_text": {
                    "type": "string",
                    "description": "The story to turn into a storyboard.",
                },
                "image_paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Local paths to reference images (characters / backgrounds).",
                },
                "out_dir": {
                    "type": "string",
                    "description": "Directory to write the result JSON into (relative to the workspace).",
                },
                "render": {
                    "type": "boolean",
                    "description": "Generate real video via Seedance (COSTS MONEY). Default false.",
                },
                "aspect_ratio": {
                    "type": "string",
                    "description": "Video aspect ratio, e.g. '16:9' or '9:16'. Default '16:9'.",
                },
            },
            "required": ["story_text"],
        }

    async def execute(self, **kwargs: Any) -> str:
        story_text = str(kwargs.get("story_text", "")).strip()
        if not story_text:
            return "Error: 'story_text' is required and must be non-empty."
        image_paths = [str(p) for p in (kwargs.get("image_paths") or [])]
        out_dir_arg = str(kwargs.get("out_dir") or "storyboard_out")
        render_video = bool(kwargs.get("render", False))
        aspect_ratio = str(kwargs.get("aspect_ratio") or "16:9")

        # Resolve out_dir under the workspace (don't let it escape the sandbox root).
        out_dir = (self.ctx.workspace / out_dir_arg).resolve()

        from nanocodex.config import load_config
        from nanocodex.provider.deepseek import DeepSeekProvider
        from nanocodex.storyboard.clients import (
            SeedanceClient,
            SeedanceError,
            TextPlanner,
            VisionAnalyzer,
        )
        from nanocodex.storyboard.models import StoryboardError
        from nanocodex.storyboard.pipeline import PipelineDeps, run_pipeline

        cfg = load_config(workspace=self.ctx.workspace)
        if not cfg.api_key:
            return "Error: no API key configured for the text planner (set DEEPSEEK_API_KEY)."

        # Text planner: main provider.
        planner = TextPlanner(DeepSeekProvider(
            api_key=cfg.api_key, base_url=cfg.base_url, model=cfg.model, timeout_s=cfg.timeout_s,
        ))

        # Vision analyzer: VL backend, only if configured AND images were given.
        vision = None
        notes: list[str] = []
        if image_paths and cfg.vl_model:
            vision = VisionAnalyzer(DeepSeekProvider(
                api_key=cfg.vl_api_key or cfg.api_key,
                base_url=cfg.vl_base_url or cfg.base_url,
                model=cfg.vl_model,
                timeout_s=cfg.timeout_s,
            ))
        elif image_paths and not cfg.vl_model:
            notes.append(
                "No VL backend configured (vl_model empty) — skipped image "
                "analysis; shots will have no per-image scene tags."
            )

        # Seedance: only needed when rendering. Key from config (GUI Settings /
        # ~/.nanocodex/config.toml) or env ARK_API_KEY.
        seedance = None
        if render_video:
            ark_key = cfg.ark_api_key
            try:
                seedance = SeedanceClient(ark_key)
            except SeedanceError as exc:
                return f"Error: render=true but {exc}"

        # Build the minimal project dict the schema expects.
        obj = {
            "project": {
                "id": f"sb_{uuid.uuid4().hex[:8]}",
                "title": story_text[:40] or "Untitled storyboard",
                "target_model": "seedance",
                "aspect_ratio": aspect_ratio,
                "language": "zh",
            },
            "inputs": {
                "story_text": story_text,
                "images": [
                    {"image_id": f"img_{i:02d}", "path": p, "kind": "unknown"}
                    for i, p in enumerate(image_paths, 1)
                ],
            },
        }

        deps = PipelineDeps(vision=vision, planner=planner, seedance=seedance)
        try:
            state, written = await run_pipeline(
                obj, deps, out_dir=out_dir, render_video=render_video,
            )
        except StoryboardError as exc:
            return f"Error: {exc}"
        except Exception as exc:  # noqa: BLE001 - surfaced to the model
            return f"Error running storyboard pipeline: {type(exc).__name__}: {exc}"

        lines = [
            f"Storyboard built: {len(state.shots)} shots, "
            f"{len(state.asset_analysis)} images analysed.",
            f"Wrote: {', '.join(str(p) for p in written.values())}",
        ]
        if render_video and state.video_urls:
            lines.append("Video URLs (signed, expire ~24h):")
            for sid, url in state.video_urls.items():
                lines.append(f"  {sid}: {url}")
        if render_video and state.video_costs:
            # Total this render's Seedance spend (CNY) and fold it into the
            # session-wide running total on the shared ToolContext, so the GUI
            # status bar can show it. Only successful shots are billed.
            run_cny = round(
                sum(float(c.get("cost_cny", 0.0)) for c in state.video_costs.values()), 4
            )
            run_tokens = sum(int(c.get("total_tokens", 0)) for c in state.video_costs.values())
            self.ctx.seedance_cost_cny += run_cny
            lines.append(
                f"Seedance cost: ¥{run_cny:.4f} CNY for {len(state.video_costs)} clip(s) "
                f"({run_tokens} tokens). Session total: ¥{self.ctx.seedance_cost_cny:.4f}."
            )
        if notes:
            lines.append("Notes: " + " ".join(notes))
        return "\n".join(lines)
