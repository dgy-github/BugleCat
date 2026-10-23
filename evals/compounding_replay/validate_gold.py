"""Validate replay tasks against their gold fix. No model calls.

For each task: rebuild the base workspace exactly like replay.py does
(`git archive <base_commit>`), run the replay judge (expect fail_to_pass to
fail), then overlay the fix commit's source files and judge again (expect
resolved). Tasks that are not resolved by their own gold fix are invalid and
should go into curricula/INVALID_TASKS.txt.

Usage:
  python evals/compounding_replay/validate_gold.py --repo-name python-tabulate \
      --tasks evals/compounding_replay/tasks/tabulate.jsonl [--ids a,b,c]
"""
from __future__ import annotations

import argparse
import io
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import replay  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo-name", default="sqlparse")
    ap.add_argument("--tasks", default=str(HERE / "tasks" / "sqlparse.jsonl"))
    ap.add_argument("--ids", default="", help="comma-separated subset")
    args = ap.parse_args()

    cache = HERE / ".cache" / args.repo_name
    tasks = [json.loads(l) for l in Path(args.tasks).read_text(encoding="utf-8").splitlines() if l.strip()]
    if args.ids:
        want = set(args.ids.split(","))
        tasks = [t for t in tasks if t["id"] in want]
    invalid = []
    for t in tasks:
        ws = Path(tempfile.mkdtemp(prefix="gold_"))
        try:
            data = subprocess.run(["git", "-C", str(cache), "archive", "--format=tar", t["base_commit"]],
                                  capture_output=True, check=True).stdout
            with tarfile.open(fileobj=io.BytesIO(data)) as tar:
                tar.extractall(ws, filter="data")
            base_v = replay.judge(ws, cache, t)
            for f in t["src_files_in_fix"]:
                blob = subprocess.run(["git", "-C", str(cache), "show", f"{t['fix_commit']}:{f}"],
                                      capture_output=True, check=True).stdout
                (ws / f).parent.mkdir(parents=True, exist_ok=True)
                (ws / f).write_bytes(blob)
            gold_v = replay.judge(ws, cache, t)
        finally:
            shutil.rmtree(ws, ignore_errors=True)
        ok = bool(gold_v.get("resolved")) and not base_v.get("resolved")
        if not ok:
            invalid.append(t["id"])
        print(f"{t['id']:<24} {'OK ' if ok else 'BAD'} "
              f"base f2p={base_v.get('f2p_passed')}/{base_v.get('f2p_total')} "
              f"gold f2p={gold_v.get('f2p_passed')}/{gold_v.get('f2p_total')} "
              f"p2p_broken={gold_v.get('p2p_broken_count')} {gold_v.get('p2p_broken', [])[:2]}",
              flush=True)
    print(f"\n{len(tasks) - len(invalid)}/{len(tasks)} valid; invalid: {','.join(invalid) or '-'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
