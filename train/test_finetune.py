#!/usr/bin/env python3
"""Tests for the SFT data-shaping + RL reward (no GPU, no model calls).

Only finetune's `.train()` path needs trl/torch; everything tested here is pure.
"""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import finetune as F  # noqa: E402  (must import WITHOUT trl/torch installed)

_REC_PASS = {
    "schema": "ncx-forge-trajectory/v1", "task": "t_a", "reward": 1, "tokens": 100,
    "system_prompt": "You are a precise agent.",
    "messages": [
        {"role": "system", "content": "STALE should be dropped"},
        {"role": "user", "content": "create foo.py"},
        {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "apply_patch"}}]},
        {"role": "tool", "content": "Patch applied"},
        {"role": "assistant", "content": "Done."},
    ],
    "final": "Done.",
}
_REC_FAIL = {**_REC_PASS, "task": "t_b", "reward": 0}


def _write(tmp: Path, recs) -> Path:
    p = tmp / "data.jsonl"
    p.write_text("\n".join(json.dumps(r) for r in recs), encoding="utf-8")
    return p


def test_load_records_filters_reward_pass_only():
    tmp = Path(tempfile.mkdtemp(prefix="ft_"))
    p = _write(tmp, [_REC_PASS, _REC_FAIL])
    assert len(F.load_records(p, reward_pass_only=True)) == 1
    assert len(F.load_records(p, reward_pass_only=False)) == 2


def test_to_chat_prepends_system_and_keeps_tool_calls():
    ex = F.to_chat(_REC_PASS)
    roles = [m["role"] for m in ex["messages"]]
    assert roles[0] == "system" and ex["messages"][0]["content"] == "You are a precise agent."
    # the STALE in-body system message is dropped
    assert roles.count("system") == 1
    assert roles == ["system", "user", "assistant", "tool", "assistant"]
    # tool_calls preserved on the assistant turn
    tc = next(m for m in ex["messages"] if m.get("tool_calls"))
    assert tc["tool_calls"][0]["function"]["name"] == "apply_patch"


def test_build_sft_dataset_passes_only():
    tmp = Path(tempfile.mkdtemp(prefix="ft2_"))
    p = _write(tmp, [_REC_PASS, _REC_FAIL])
    ex = F.build_sft_dataset([p], reward_pass_only=True)
    assert len(ex) == 1 and ex[0]["task"] == "t_a"


def test_bench_reward_unknown_task_is_zero():
    assert F.bench_reward("does_not_exist_task", Path(tempfile.mkdtemp())) == 0.0


def test_rl_design_is_documented():
    d = F.rl_design()
    assert "bench_reward" in d and "episode" in d


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
