#!/usr/bin/env python3
"""Task-level train / val / test split for ncx-forge (M1).

Why task-level (not run-level): a candidate genome must be *scored* on tasks the
teacher saw failing (train), *accepted* only if it also holds on tasks it never
saw (val), and *reported* once at the end on a frozen set it was never tuned
against (test). Splitting runs of the same task would leak the task's signal
across all three and defeat the anti-overfit purpose (DESIGN §7).

Split source of truth is `train/splits.json` (editable). When absent it is
derived deterministically from the sorted task list (stable as tasks are added)
and written out, so a run is always reproducible and auditable.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

BENCH = Path(__file__).resolve().parent.parent / "bench"
sys.path.insert(0, str(BENCH))
import run as bench  # noqa: E402

SPLITS_FILE = Path(__file__).resolve().parent / "splits.json"
# Round-robin assignment pattern over sorted task names. Train-heavy.
_PATTERN = ["train", "train", "train", "val", "train", "train", "test", "val"]


def all_task_names() -> list[str]:
    """Every task bench can see (bench/tasks/*, including generated gen_* dirs)."""
    return [t.name for t in bench.tasks()]


def _derive(names: list[str]) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {"train": [], "val": [], "test": []}
    for i, name in enumerate(sorted(names)):
        out[_PATTERN[i % len(_PATTERN)]].append(name)
    return out


def load_splits(persist: bool = True) -> dict[str, list[str]]:
    """Return {'train':[...], 'val':[...], 'test':[...]}.

    Uses splits.json if present. Any NEW tasks not yet in the file are folded in
    deterministically and the file is rewritten, so adding tasks never silently
    drops them and the assignment stays stable for tasks already placed.
    """
    names = set(all_task_names())
    existing: dict[str, list[str]] = {}
    if SPLITS_FILE.exists():
        try:
            existing = json.loads(SPLITS_FILE.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            existing = {}
    placed = {n for v in existing.values() for n in v}
    splits = {k: [n for n in existing.get(k, []) if n in names] for k in ("train", "val", "test")}
    # Assign brand-new tasks deterministically by global sorted index.
    new = sorted(names - placed)
    if new:
        derived = _derive(sorted(names))  # stable global assignment
        for n in new:
            for k in ("train", "val", "test"):
                if n in derived[k]:
                    splits[k].append(n)
                    break
    for k in splits:
        splits[k] = sorted(set(splits[k]))
    if persist and (not SPLITS_FILE.exists() or new):
        SPLITS_FILE.write_text(json.dumps(splits, indent=2), encoding="utf-8")
    return splits


if __name__ == "__main__":
    s = load_splits()
    total = sum(len(v) for v in s.values())
    print(f"splits ({total} tasks) -> {SPLITS_FILE}")
    for k in ("train", "val", "test"):
        print(f"  {k:5} ({len(s[k])}): {', '.join(s[k]) or '(none)'}")
