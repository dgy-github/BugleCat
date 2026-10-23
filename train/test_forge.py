#!/usr/bin/env python3
"""Deterministic unit tests for the forge optimizer's accept gate + P1 RRSI regularization.

Covers:
- Basic train/holdout accept & reject gates and self-check retry logic
- Annealed surface & line edit budgets (`genome.validate`)
- Anti-memorization critic gate (`teacher.critic_check`)
- Falsified hypothesis ledger (`runs/ledger_<stamp>.jsonl`) + teacher prompt injection
- Stagnation surface steering (`stagnation_limit`)
- Token-cost acceptance (`delta_tokens`) and token-bloat rejection (`max_token_growth`)
- Post-search structural ablation pruning (`prune_genome` / `prune_at_end`)
"""
from __future__ import annotations

import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import evaluator as ev  # noqa: E402
import forge  # noqa: E402
import genome  # noqa: E402
import teacher  # noqa: E402

BASELINE = genome.Genome(
    system_prompt="base prompt",
    tool_desc={"read_file": "rf", "apply_patch": "ap"},
)


class FakeTeacher(teacher.TeacherBackend):
    def __init__(self, name, override_toml):
        self.name = name
        self._toml = override_toml
        self.prompts: list[str] = []

    def available(self):
        return True

    def propose(self, prompt, timeout=180):
        self.prompts.append(prompt)
        if callable(self._toml):
            body = self._toml(len(self.prompts), prompt)
        else:
            body = self._toml
        return f"```toml\n{body}\n```"


def _install(monkey_eval):
    """Point forge at deterministic stand-ins. Returns a restore() callable."""
    orig = (
        forge.genome.extract_current,
        forge.ev.evaluate,
        forge.teacher.build_panel,
        forge.GENOMES_DIR,
        forge.RUNS_DIR,
    )
    tmp = Path(tempfile.mkdtemp(prefix="forge_test_"))
    forge.GENOMES_DIR = tmp / "genomes"
    forge.RUNS_DIR = tmp / "runs"
    forge.genome.extract_current = lambda: BASELINE.copy()
    forge.ev.evaluate = monkey_eval

    def restore():
        (
            forge.genome.extract_current,
            forge.ev.evaluate,
            forge.teacher.build_panel,
            forge.GENOMES_DIR,
            forge.RUNS_DIR,
        ) = orig

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
                p = 0 if not is_holdout else 1
            r.tasks[t] = ev.TaskResult(
                task=t,
                passes=p,
                runs=1,
                mean_s=1.0,
                failure_trajectory=("" if p else "agent did the wrong thing"),
            )
        return r

    return fake_eval


def test_accepts_when_train_improves_and_holdout_holds():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("fake", 'system_prompt = "IMPROVED prompt"')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T1",
        )
        assert lin["rounds"][0]["accept"]["teacher"] == "fake", lin
        assert "champion" in lin
        assert "system_prompt" in lin["champion"]["diff_vs_baseline"], lin["champion"]
    finally:
        restore()


def test_rejects_when_holdout_regresses():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=False))
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("fake", 'system_prompt = "IMPROVED prompt"')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T2",
        )
        acc = lin["rounds"][0].get("accept", {})
        assert acc.get("status") == "holdout-regressed", lin
        assert "system_prompt" not in lin["champion"]["diff_vs_baseline"]
    finally:
        restore()


def test_invalid_candidate_is_skipped():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("bad", '[tool_desc]\napply_patch = ""')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T3",
        )
        statuses = [c["status"] for c in lin["rounds"][0]["candidates"]]
        assert any("invalid" in s for s in statuses), statuses
    finally:
        restore()


def test_self_check_retries_past_model_noise():
    calls = {"g": 0}

    def fake_ask(genome_path, prompt, timeout):
        if genome_path:
            calls["g"] += 1
            return forge.SENTINEL if calls["g"] >= 3 else "sorry, no codeword"
        return "plain reply"

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


def test_annealed_edit_budget_rejects_oversized_mutation():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    # Modifies 2 surfaces (system_prompt + tool_desc.read_file) when initial_max_surfaces=1
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher(
            "too_wide",
            'system_prompt = "IMPROVED prompt"\n[tool_desc]\nread_file = "rf v2"',
        )
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_BUDGET",
            initial_max_surfaces=1,
        )
        statuses = [c["status"] for c in lin["rounds"][0]["candidates"]]
        assert any("surface budget" in s for s in statuses), statuses
    finally:
        restore()


def test_critic_rejects_memorized_task_name():
    restore = _install(_eval_from_content(improves_train=True, holdout_ok=True))
    # Teacher tries to hardcode the benchmark task name `task_alpha` into system_prompt
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("cheater", 'system_prompt = "IMPROVED: special case for task_alpha"')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["task_alpha"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_CRITIC",
        )
        statuses = [c["status"] for c in lin["rounds"][0]["candidates"]]
        assert any("critic:" in s and "task_alpha" in s for s in statuses), statuses
        assert "system_prompt" not in lin["champion"]["diff_vs_baseline"]
    finally:
        restore()


def test_ledger_and_falsified_history_and_stagnation_steering():
    restore = _install(_eval_from_content(improves_train=False, holdout_ok=True))
    ft = FakeTeacher("explorer", 'system_prompt = "unhelpful prompt edit"')
    forge.teacher.build_panel = lambda verbose=True: [ft]
    try:
        lin = forge.train(
            rounds=2, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_LEDGER",
            stagnation_limit=1,
        )
        # Round 2 prompt must contain the falsified mutation history from round 1
        # AND stagnation steering toward an untouched surface (tool_desc.apply_patch).
        assert len(ft.prompts) == 2
        r2_prompt = ft.prompts[1]
        assert "Falsified / Rejected Mutations" in r2_prompt, r2_prompt
        assert "below-margin" in r2_prompt, r2_prompt
        assert "Stagnation steering" in r2_prompt, r2_prompt
        assert "tool_desc.apply_patch" in r2_prompt, r2_prompt
        # Ledger file exists and has 2 JSONL entries
        ledger_path = forge.RUNS_DIR / "ledger_T_LEDGER.jsonl"
        assert ledger_path.exists()
        rows = [json.loads(line) for line in ledger_path.read_text(encoding="utf-8").splitlines()]
        assert len(rows) == 2 and all(r["verdict"] == "rejected" for r in rows), rows
        assert lin["rounds"][1]["focus_surface"] == "tool_desc.apply_patch"
    finally:
        restore()


def test_token_cost_acceptance_and_bloat_rejection():
    def token_eval(genome_path, tasks, repeats, timeout):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        r = ev.EvalResult(genome=str(genome_path))
        for t in tasks:
            is_holdout = t.startswith("hold")
            if "LEAN" in content:
                # Same pass-rate on train (1/2), much fewer tokens (600 vs 1000)
                p = 1 if (is_holdout or t == "t_pass") else 0
                tok = 600.0
            elif "BLOATED" in content:
                # Higher pass-rate on train (2/2), but huge token growth (2500 vs 1000)
                p = 1
                tok = 2500.0
            else:
                # Baseline: passes 1/2 train tasks (`t_pass`), fails `t_fail`, 1000 tokens
                p = 1 if (is_holdout or t == "t_pass") else 0
                tok = 1000.0
            r.tasks[t] = ev.TaskResult(
                task=t, passes=p, runs=1, mean_s=1.0, mean_tokens=tok,
                failure_trajectory=("" if p else "failed task"),
            )
        return r

    # Part A: LEAN candidate accepted via token-saving gate at equal pass-rate
    restore = _install(token_eval)
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("lean_t", 'system_prompt = "LEAN concise prompt"')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_pass", "t_fail"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_LEAN",
            accept_margin=1, delta_tokens=200.0,
        )
        acc = lin["rounds"][0]["accept"]
        assert acc.get("teacher") == "lean_t" and acc.get("mode") == "token-saving", acc
    finally:
        restore()

    # Part B: BLOATED candidate rejected via max_token_growth despite +1 pass gain
    restore = _install(token_eval)
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("bloat_t", 'system_prompt = "BLOATED verbose prompt"')
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_pass", "t_fail"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_BLOAT",
            accept_margin=1, max_token_growth=500.0,
        )
        acc = lin["rounds"][0]["accept"]
        assert acc.get("status") == "token-bloat", acc
    finally:
        restore()


def test_prune_genome_reverts_redundant_surface():
    def prune_eval(genome_path, tasks, repeats, timeout):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        # Only `IMPROVED` in system_prompt is actually needed to pass `t_a`;
        # the edit to `tool_desc.read_file` ("redundant rf edit") is pure bloat.
        improved = "IMPROVED" in content
        r = ev.EvalResult(genome=str(genome_path))
        for t in tasks:
            p = 1 if (t.startswith("hold") or improved) else 0
            r.tasks[t] = ev.TaskResult(
                task=t, passes=p, runs=1, mean_s=1.0,
                failure_trajectory=("" if p else "fail"),
            )
        return r

    restore = _install(prune_eval)
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher(
            "bundle",
            'system_prompt = "IMPROVED prompt"\n[tool_desc]\nread_file = "redundant rf edit"',
        )
    ]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_a"], holdout_tasks=["hold_b"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_PRUNE",
            prune_at_end=True,
        )
        champ = lin["champion"]
        assert "tool_desc.read_file" in champ["pruned_surfaces"], champ
        assert "system_prompt" not in champ["pruned_surfaces"], champ
        assert "system_prompt" in champ["diff_vs_baseline"], champ
        assert "tool_desc.read_file" not in champ["diff_vs_baseline"], champ
    finally:
        restore()


def test_p2_expanded_genome_surfaces_roundtrip_and_validate():
    base = genome.Genome(
        system_prompt="base",
        tool_desc={"read_file": "rf"},
        orchestrator_prompts={"plan_sys": "plan default"},
        memory_config={"max_entries": 6, "max_chars": 2400},
    )
    cand, why = teacher.parse_candidate(
        '```toml\n[orchestrator_prompts]\nplan_sys = "concise 3-step plan"\n[memory_config]\nmax_entries = 4\n```',
        base,
    )
    assert cand is not None and why == "ok"
    assert genome.changed_surfaces(base, cand) == [
        "orchestrator_prompts.plan_sys",
        "memory_config.max_entries",
    ]
    assert genome.validate(cand, base) == []
    rt = genome.Genome.from_toml(cand.to_toml())
    assert rt == cand
    # Out-of-bounds memory_config is rejected by validate
    bad = cand.copy()
    bad.memory_config["max_entries"] = 99
    errs = genome.validate(bad, base)
    assert any("max_entries" in e for e in errs), errs


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
