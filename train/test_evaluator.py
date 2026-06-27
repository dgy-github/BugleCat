#!/usr/bin/env python3
"""Fast, agent-free unit tests for the evaluator's trajectory capture + redaction.

Run: python train/test_evaluator.py   (or: python -m pytest train/test_evaluator.py)
The trust-critical property is that grader artifacts (check.py output) never reach
the teacher via a trajectory.
"""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev


def test_redact_drops_grader_lines():
    text = "normal line\nopened check.py for grading\nanother normal line\nran the hidden test"
    out = ev._redact(text)
    assert "check.py" not in out
    assert "hidden test" not in out
    assert "normal line" in out
    assert out.count("[redacted: grader-related line]") == 2


def test_redact_caps_length():
    out = ev._redact("x" * (ev.MAX_TRAJECTORY_CHARS + 500))
    assert len(out) <= ev.MAX_TRAJECTORY_CHARS + len("\n[...truncated]")
    assert out.endswith("[...truncated]")


def test_extract_trajectory_parses_assistant_and_tool_calls():
    ws = Path(tempfile.mkdtemp(prefix="p2_unit_"))
    log = ws / ".nanocodex" / "session.jsonl"
    log.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "do it"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"function": {"name": "apply_patch", "arguments": '{"patch":"..."}'}}
        ]},
        {"role": "tool", "content": "Patch applied"},
        {"role": "assistant", "content": "Done — created foo.py."},
    ]
    log.write_text("\n".join(json.dumps(m) for m in lines), encoding="utf-8")

    traj = ev.extract_trajectory(ws)
    assert "apply_patch" in traj
    assert "Done — created foo.py." in traj
    # last assistant message wins
    assert traj.strip().endswith("Done — created foo.py.")


def test_extract_trajectory_redacts_grader_mention():
    ws = Path(tempfile.mkdtemp(prefix="p2_unit2_"))
    log = ws / ".nanocodex" / "session.jsonl"
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(
        json.dumps({"role": "assistant", "content": "I will read _check.py to pass"}),
        encoding="utf-8",
    )
    traj = ev.extract_trajectory(ws)
    assert "_check.py" not in traj
    assert "redacted" in traj


def test_extract_trajectory_missing_log_is_empty():
    ws = Path(tempfile.mkdtemp(prefix="p2_unit3_"))
    assert ev.extract_trajectory(ws) == ""


def test_timeout_failure_synthesizes_trajectory():
    # A failure with no captured trajectory must still produce a signal, or forge
    # mistakes the failing task for a pass (the live-run bug this guards against).
    import subprocess
    task = Path(tempfile.mkdtemp(prefix="p2_to_"))
    (task / "prompt.txt").write_text("do something", encoding="utf-8")
    orig = (ev.subprocess.run, ev.bench.grade, ev.extract_trajectory, ev.bench.seed)

    def boom(*a, **k):
        raise subprocess.TimeoutExpired(cmd="ncx", timeout=1)
    ev.subprocess.run = boom
    ev.bench.grade = lambda t, ws: (False, "")
    ev.extract_trajectory = lambda ws: ""
    ev.bench.seed = lambda t, ws: None
    try:
        ok, elapsed, traj, tokens = ev._run_task_once(task, None, timeout=1)
        assert ok is False
        assert traj and "timed out" in traj.lower(), repr(traj)
        assert tokens == 0  # no usage line on a timeout
    finally:
        (ev.subprocess.run, ev.bench.grade, ev.extract_trajectory, ev.bench.seed) = orig


def test_parse_tokens_from_usage_line():
    line = "some output\n[ncx-usage] prompt_tokens=3340 completion_tokens=16 total_tokens=3356\nbye"
    assert ev._parse_tokens(line) == 3356
    assert ev._parse_tokens("no usage here") == 0


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"FAIL {fn.__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
