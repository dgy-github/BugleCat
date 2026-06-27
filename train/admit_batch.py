#!/usr/bin/env python3
"""Admit a batch of candidate tasks (JSON array) through TaskGen's self-validation.

Input: a JSON file holding a list of task dicts ({name, prompt, check, seed,
reference}) — e.g. produced by parallel task-author agents. Each is run through
the SAME deterministic gate as taskgen.generate (reference passes check twice +
seed state fails) before being admitted to bench/tasks/gen_*. Untrusted authors
do not bypass the gate.

Usage: python train/admit_batch.py <tasks.json> [--overwrite]
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import taskgen as tg  # noqa: E402


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("tasks_json")
    ap.add_argument("--overwrite", action="store_true")
    a = ap.parse_args()

    data = json.loads(Path(a.tasks_json).read_text(encoding="utf-8"))
    if isinstance(data, dict):
        data = [data]
    admitted, rejected = [], []
    for i, task in enumerate(data):
        if not isinstance(task, dict):
            rejected.append((f"#{i}", "not an object"))
            continue
        task.setdefault("seed", {})
        ok, reason = tg.validate(task)
        if not ok:
            rejected.append((task.get("name", f"#{i}"), reason))
            print(f"[admit] REJECT {task.get('name', f'#{i}')}: {reason}")
            continue
        name = reason
        try:
            dest = tg.admit(task, name, a.overwrite)
        except FileExistsError:
            rejected.append((name, "already exists"))
            print(f"[admit] SKIP {name}: already exists")
            continue
        admitted.append(name)
        print(f"[admit] ADMITTED {name} -> {dest}")
    print(f"\n[admit] {len(admitted)}/{len(data)} admitted: {admitted}")
    if rejected:
        print(f"[admit] rejected: {rejected}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
