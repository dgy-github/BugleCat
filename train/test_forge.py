#!/usr/bin/env python3
"""Deterministic unit tests for the forge optimizer's accept gate (no model calls).

The live run already exercised self-check + panel + gen0 eval + the clean stop
path. These tests pin the GLUE that a real run with a strong baseline can't
reach: teacher mutation -> candidate eval -> accept-iff-(train-improves AND
holdout-holds), and lineage output.
"""
from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
import forge  # noqa: E402
import genome as G  # noqa: E402
import teacher as T  # noqa: E402

BASELINE = G.Genome(system_prompt="base prompt", tool_desc={"read_file": "rf", "apply_patch": "ap"})


class FakeTeacher(T.TeacherBackend):
    def __init__(self, name, override_toml):
        self.name = name
        self._toml = override_toml
    def available(self):
        return True
    def propose(self, prompt, timeout=180):
        return f"```toml\n{self._toml}\n```"


def _install(monkey_eval):
    """Point forge at deterministic stand-ins. Returns a restore() callable."""
    orig = (forge.G.extract_current, forge.ev.evaluate, forge.T.build_panel,
            forge.GENOMES_DIR, forge.RUNS_DIR)
    tmp = Path(tempfile.mkdtemp(prefix="forge_test_"))
    forge.GENOMES_DIR = tmp / "genomes"
    forge.RUNS_DIR = tmp / "runs"
    forge.G.extract_current = lambda: BASELINE.copy()
    forge.ev.evaluate = monkey_eval

    def restore():
        (forge.G.extract_current, forge.ev.evaluate, forge.T.build_panel,
         forge.GENOMES_DIR, forge.RUNS_DIR) = orig
    return restore


def _eval_from_content(improves_train: bool, holdout_ok: bool):
    """Build an evaluate() stand-in: baseline fails train; the 'improved' genome
    passes train. Holdout passes for the improved genome iff holdout_ok."""
    def fake_eval(genome_path, tasks, repeats, timeout):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        improved = "IMPROVED" in content
        r = ev.EvalResult(genome=str(genome_path))
        for t in tasks:
            is_holdout = t.startswith("hold")
            if improved:
                p = 1 if (is_holdout and holdout_ok) or (not is_holdout and improves_train) else 0
            else:
                # baseline: fails train (so there is a failure to feed the teacher),
                # passes holdout (so a holdout regression is detectable).
                p = 0 if not is_holdout else 1
            r.tasks[t] = ev.TaskResult(task=t, passes=p, runs=1, mean_s=1.0,
                                       failure_trajectory=("" if p else "agent did the wrong thing"))
        return r
    return fake_eval


def test_accepts_when_train_improves_and_holdout_holds():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    forge.T.build_panel = lambda verbose=True: [FakeTeacher("fake", 'system_prompt = "IMPROVED prompt"')]
    try:
        lin = forge.train(rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
                          repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T1")
        assert lin["rounds"][0]["accept"]["teacher"] == "fake", lin
        assert "champion" in lin
        # champion adopted the improved system_prompt (diff vs baseline mentions it)
        assert "system_prompt" in lin["champion"]["diff_vs_baseline"], lin["champion"]
    finally:
        restore()


def test_rejects_when_holdout_regresses():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=False))
    forge.T.build_panel = lambda verbose=True: [FakeTeacher("fake", 'system_prompt = "IMPROVED prompt"')]
    try:
        lin = forge.train(rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
                          repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T2")
        acc = lin["rounds"][0].get("accept", {})
        assert acc.get("status") == "holdout-regressed", lin
        # champion stays the baseline (no system_prompt change)
        assert "system_prompt" not in lin["champion"]["diff_vs_baseline"]
    finally:
        restore()


def test_invalid_candidate_is_skipped():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    # Override blanks a load-bearing description -> validate() rejects it.
    forge.T.build_panel = lambda verbose=True: [
        FakeTeacher("bad", '[tool_desc]\napply_patch = ""')]
    try:
        lin = forge.train(rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
                          repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T3")
        statuses = [c["status"] for c in lin["rounds"][0]["candidates"]]
        assert any("invalid" in s for s in statuses), statuses
    finally:
        restore()


def test_self_check_retries_past_model_noise():
    # The agent fails to echo the sentinel twice (noise), succeeds on the 3rd —
    # the gate must retry and PASS, not block the run on a single miss.
    calls = {"g": 0}

    def fake_ask(genome_path, prompt, timeout):
        if genome_path:                       # the with-genome probe
            calls["g"] += 1
            return forge.SENTINEL if calls["g"] >= 3 else "sorry, no codeword"
        return "plain reply"                  # baseline: sentinel absent
    orig = forge._ask
    forge._ask = fake_ask
    try:
        assert forge.self_check(timeout=1, attempts=3) is True
        assert calls["g"] == 3, calls
    finally:
        forge._ask = orig


def test_self_check_fails_if_never_injects():
    force = lambda gp, p, t: "never has it"  # noqa: E731
    orig = forge._ask
    forge._ask = force
    try:
        assert forge.self_check(timeout=1, attempts=2) is False
    finally:
        forge._ask = orig


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
