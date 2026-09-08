"""Retry a healthy-baseline preflight failure that made zero model calls.

Never use this entrypoint to replace a valid failed model attempt. All original
preflight evidence is retained in the summary's preflight_history.
"""
import argparse
import json
from pathlib import Path

from .cases import CASES
from .suite import MODEL, ROOT, configured_environment, execute_case, write_json
from .suite_support import digest, git


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", type=Path)
    parser.add_argument("work_root", type=Path)
    parser.add_argument("case_id")
    args = parser.parse_args()
    state = json.loads(args.summary.read_text(encoding="utf-8"))
    if state["model"] != MODEL or state["binary_sha256"] != digest(
        (ROOT / "rust/target/release/ncx.exe").read_bytes()
    ) or state["source_head"] != git(ROOT, "rev-parse", "HEAD").strip():
        raise SystemExit("Executor/model/source HEAD differs from the original run")
    index = next(i for i, c in enumerate(state["cases"]) if c["id"] == args.case_id)
    old = state["cases"][index]
    if old["status"] != "invalid_healthy_baseline" or old.get("attempts"):
        raise SystemExit("Only a preflight failure with zero model calls may be retried")
    workspace = args.work_root.resolve()
    if workspace.is_relative_to(ROOT) or ROOT.is_relative_to(workspace):
        raise SystemExit("Candidate root must be outside the source repository")
    case = next(c for c in CASES if c.id == args.case_id)
    if (workspace / case.id).exists():
        raise SystemExit("Refusing to replace an existing candidate")
    env, credential = configured_environment()
    cache = ROOT / "rust/target"
    env["CARGO_TARGET_DIR"] = str(cache)
    state.setdefault("preflight_history", []).append(old)
    # Checkpoint original failure before any further execution.
    write_json(args.summary, state)
    state["cases"][index] = execute_case(case, workspace, cache, env, credential,
                                       args.summary.parent, ROOT / "rust/target/release/ncx.exe")
    write_json(args.summary, state)
    return int(state["cases"][index]["status"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
