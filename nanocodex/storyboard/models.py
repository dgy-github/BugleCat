"""Data models + JSON-Schema validation for the storyboard pipeline.

House style mirrors agent/schedule.py: plain dataclasses for the typed shape,
pure functions over data, no I/O. The dataclasses mirror the draft-07 schema in
``schemas/project.schema.json`` (the single source of truth the user supplied);
``validate_project`` enforces that schema on raw input dicts using the
``jsonschema`` library.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from functools import lru_cache
from pathlib import Path
from typing import Any

_SCHEMA_PATH = Path(__file__).parent / "schemas" / "project.schema.json"


class StoryboardError(ValueError):
    """Raised when a project fails schema validation or a stage cannot proceed."""


@lru_cache(maxsize=1)
def _load_schema() -> dict[str, Any]:
    with _SCHEMA_PATH.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def validate_project(obj: dict[str, Any]) -> None:
    """Validate a raw project dict against the draft-07 schema.

    Raises :class:`StoryboardError` with a path-qualified message on the first
    violation. ``jsonschema`` is an optional-but-declared dependency; if it is
    missing we say so clearly rather than silently skipping validation.
    """
    try:
        import jsonschema
    except ImportError as exc:  # pragma: no cover - dependency missing
        raise StoryboardError(
            "The 'jsonschema' package is required for storyboard validation. "
            "Install it with: python -m pip install jsonschema"
        ) from exc

    try:
        jsonschema.validate(instance=obj, schema=_load_schema())
    except jsonschema.ValidationError as exc:
        loc = "/".join(str(p) for p in exc.absolute_path) or "(root)"
        raise StoryboardError(f"Invalid project at {loc}: {exc.message}") from exc


# --- typed shape (mirrors the schema) ---------------------------------------


@dataclass
class Project:
    id: str
    title: str
    target_model: str = "seedance"
    aspect_ratio: str = "16:9"
    genre: str = ""
    language: str = "zh"
    global_style: str = ""


@dataclass
class ImageInput:
    image_id: str
    path: str
    kind: str = "unknown"  # unknown | character | background | composition
    notes: str = ""


@dataclass
class Character:
    id: str
    name: str
    role: str
    gender: str = ""
    appearance_lock: str = ""
    reference_image_ids: list[str] = field(default_factory=list)


@dataclass
class Chapter:
    """A story chapter — the "story-detail" layer that sits ABOVE shots.

    A long story is first split into a handful of chapters (3-8), each carrying
    its plot summary, setting, cast and key beats, so the user can review the
    narrative structure BEFORE it is broken down into camera shots. The shot
    planner then slices each chapter into shots, keeping continuity.
    """

    chapter_id: str
    title: str
    summary: str = ""               # 剧情概要
    setting: str = ""               # 场景 / 环境
    characters: list[str] = field(default_factory=list)
    key_moments: list[str] = field(default_factory=list)  # 关键细节 / 节拍
    source_excerpt: str = ""        # 对应原文片段（可空）


@dataclass
class AssetAnalysis:
    image_id: str
    summary: str
    scene_tags: list[str] = field(default_factory=list)
    mood_tags: list[str] = field(default_factory=list)
    usable_for: list[str] = field(default_factory=list)


@dataclass
class Shot:
    shot_id: str
    title: str
    duration_sec: float
    prompt: str              # English画面描述，给视频模型读（出片用）
    prompt_zh: str = ""      # 中文画面描述，给人看的预览（不进 Seedance payload）
    characters: list[str] = field(default_factory=list)
    background_image_ids: list[str] = field(default_factory=list)
    character_image_ids: list[str] = field(default_factory=list)
    camera: str = ""
    action: str = ""
    negative_prompt: str = ""
    chapter_id: str = ""  # which Chapter this shot belongs to (for grouped preview)


@dataclass
class SeedancePayload:
    shot_id: str
    model: str
    payload: dict[str, Any]


def project_from_dict(obj: dict[str, Any]) -> tuple[Project, list[ImageInput]]:
    """Build the typed Project + image inputs from a validated dict.

    Call :func:`validate_project` first; this assumes the shape is already
    schema-valid and only pulls the fields the pipeline needs to start.
    """
    p = obj["project"]
    project = Project(
        id=p["id"],
        title=p["title"],
        target_model=p.get("target_model", "seedance"),
        aspect_ratio=p.get("aspect_ratio", "16:9"),
        genre=p.get("genre", ""),
        language=p.get("language", "zh"),
        global_style=p.get("global_style", ""),
    )
    images = [
        ImageInput(
            image_id=im["image_id"],
            path=im["path"],
            kind=im.get("kind", "unknown"),
            notes=im.get("notes", ""),
        )
        for im in obj["inputs"].get("images", [])
    ]
    return project, images


def as_jsonable(value: Any) -> Any:
    """Recursively convert dataclasses to plain dicts for JSON export."""
    if hasattr(value, "__dataclass_fields__"):
        return {k: as_jsonable(v) for k, v in asdict(value).items()}
    if isinstance(value, list):
        return [as_jsonable(v) for v in value]
    if isinstance(value, dict):
        return {k: as_jsonable(v) for k, v in value.items()}
    return value
