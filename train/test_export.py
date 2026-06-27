#!/usr/bin/env python3
"""Deterministic tests for M3 trajectory export (no model calls)."""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import export as X  # noqa: E402
import evaluator as ev  # noqa: E402


def test_read_messages_strips_internal_fields():
    ws = Path(tempfile.mkdtemp(prefix="exp_"))
    log = ws / ev.SESSION_LOG_REL
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text("\n".join(json.dumps(m) for m in [
        {"role": "system", "content": "sys", "_ts": 123},
        {"role": "user", "content": "do it", "_ts": 124},
        {"role": "assistant", "content": "done", "tool_calls": [{"function": {"name": "apply_patch"}}]},
    ]), encoding="utf-8")
    msgs = X._read_messages(ws)
    assert [m["role"] for m in msgs] == ["system", "user", "assistant"]
    assert all("_ts" not in m for m in msgs), "internal _ fields must be stripped"
    assert msgs[2]["tool_calls"][0]["function"]["name"] == "apply_patch"


def test_export_writes_and_filters():
    # Monkeypatch the per-task runner to return canned records (one pass, one fail).
    out = Path(tempfile.mkdtemp(prefix="exp_out_")) / "d.jsonl"
    recs = iter([
        {"task": "t_a", "reward": 1, "tokens": 100, "system_prompt": "s", "messages": [{"role": "system", "content": "s"}], "final": "ok"},
        {"task": "t_b", "reward": 0, "tokens": 50, "system_prompt": "s", "messages": [], "final": ""},
    ])
    orig = (X._run_and_capture, X.ev.bench.tasks, X._resolve_system_prompt)
    X._run_and_capture = lambda task, gp, to, model, sp="": next(recs)
    X.ev.bench.tasks = lambda: [type("T", (), {"name": "t_a"})(), type("T", (), {"name": "t_b"})()]
    X._resolve_system_prompt = lambda gp: "stub system"
    try:
        summ = X.export(None, ["t_a", "t_b"], out, repeats=1, reward_pass_only=True)
        lines = out.read_text(encoding="utf-8").splitlines()
        assert len(lines) == 1, lines           # only the passing record kept
        rec = json.loads(lines[0])
        assert rec["task"] == "t_a" and rec["reward"] == 1
        assert rec["schema"] == X.SCHEMA and rec["genome_id"] == "baseline"
        assert summ["records"] == 1 and summ["passed"] == 1
    finally:
        X._run_and_capture, X.ev.bench.tasks, X._resolve_system_prompt = orig


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn(); print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1; print(f"FAIL {fn.__name__}: {e}")
        except Exception as e:  # noqa: BLE001
            failed += 1; print(f"ERR  {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
