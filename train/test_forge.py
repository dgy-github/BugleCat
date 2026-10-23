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


def test_h2_build_contrast_blind_vs_nonblind():
    base = genome.Genome(system_prompt="base")
    p0 = forge.Probe(
        idx=0, kind="control", genome=base, path=Path("p0.toml"),
        surfaces=[], diff="(control)",
        trajectory=(
            "  #01 [tool] read_file (ok, 10ms): a.py\n"
            "  #02 [tool] bash (ok, 900ms): pytest\n"
            "  TIMEOUT after 60s — passed=0"
        ),
        passed=0, runs=2, mean_tokens=1500.0, mean_s=60.0,
    )
    p1 = forge.Probe(
        idx=1, kind="general", genome=base, path=Path("p1.toml"),
        surfaces=["system_prompt"], diff="system_prompt: 4 -> 22 chars",
        trajectory=(
            "  #01 [tool] read_file (ok, 10ms): a.py\n"
            "  #02 [tool] edit_file (ok, 15ms): a.py\n"
            "  #03 [tool] bash (ok, 120ms): pytest -k test_one\n"
            "  [final_output] all tests passed"
        ),
        passed=2, runs=2, mean_tokens=800.0, mean_s=12.0,
    )
    blind_txt = forge.build_contrast([p0, p1], blind=True)
    assert "passes=" not in blind_txt, blind_txt
    assert "[redacted]" in blind_txt, blind_txt
    assert "step 2 (control=bash vs probe=edit_file)" in blind_txt, blind_txt
    assert "<<<UNTRUSTED trajectory tail probe #1" in blind_txt, blind_txt

    open_txt = forge.build_contrast([p0, p1], blind=False)
    assert "passes=2/2" in open_txt and "passes=0/2" in open_txt, open_txt


def test_h2_lesson_library_lifecycle_and_prune_refute():
    import lesson_lib

    tmp = Path(tempfile.mkdtemp(prefix="forge_lessons_"))
    lib = lesson_lib.LessonLibrary(
        tmp / "lessons.jsonl", adopted_path=tmp / "adopted_harnesses.jsonl"
    )
    sig = lesson_lib.task_signature(
        "  #01 [tool] bash (ok, 500ms): pytest\n  TIMEOUT after 120s\n"
    )
    assert "timeout" in sig["symptoms"], sig

    # Anti-memorization: lesson containing forbidden task name is rejected
    leaked = lib.record_lesson(
        {
            "symptoms": ["timeout"],
            "surfaces": ["system_prompt"],
            "edit_pattern": "special rule for task_secret_42",
            "why": "fixes task_secret_42",
        },
        stamp="S1", round_num=1, task_sig=sig, surfaces=["system_prompt"],
        margin=1, token_saving=100.0, forbidden_terms=["task_secret_42"],
    )
    assert leaked is None

    # Valid lesson is persisted with support=1
    rec = lib.record_lesson(
        {
            "symptoms": ["timeout", "full_suite_pytest"],
            "surfaces": ["system_prompt", "tool_desc.read_file"],
            "edit_pattern": "Run targeted pytest -k instead of full test suite",
            "why": "Avoids 120s timeout on large test suites",
        },
        stamp="S1", round_num=1, task_sig=sig,
        surfaces=["system_prompt", "tool_desc.read_file"],
        margin=1, token_saving=300.0, forbidden_terms=["task_secret_42"],
    )
    assert rec is not None and rec["id"] == "L-0001"
    hits = lib.retrieve(sig, k=3)
    assert len(hits) == 1 and hits[0]["id"] == "L-0001", hits

    # Pruned surface increments refute
    refuted_ids = lib.refute_by_surface(["tool_desc.read_file"], stamp="S1")
    assert refuted_ids == ["L-0001"]

    # Refute until refute >= support + 2 -> status becomes 'retired'
    lib.update_outcomes(
        ["L-0001"], accepted=False, stamp="S1", round_num=2,
        task_sig_hash=sig["sig_hash"], margin=0, token_saving=0.0, reason="below-margin",
    )
    lib.update_outcomes(
        ["L-0001"], accepted=False, stamp="S1", round_num=3,
        task_sig_hash=sig["sig_hash"], margin=0, token_saving=0.0, reason="below-margin",
    )
    assert lib.retrieve(sig, k=3) == []
    retired = lib.retired_lessons()
    assert len(retired) == 1 and retired[0]["id"] == "L-0001", retired


def test_h2_ppc_parallel_and_sequential_with_lessons():
    def h2_eval(genome_path, tasks, repeats, timeout, model=None, keep_trajectory=False):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        composed_ok = "COMPOSED_RULE" in content
        probe_ok = "PROBE_RULE" in content
        r = ev.EvalResult(genome=str(genome_path))
        for t in tasks:
            if t.startswith("hold"):
                p = 1
            elif composed_ok or probe_ok:
                p = 1
            else:
                p = 0
            tok = 700.0 if (composed_ok or probe_ok) else 1200.0
            traj = (
                "  #01 [tool] read_file (ok, 10ms): mod.py\n"
                "  #02 [tool] edit_file (ok, 12ms): mod.py\n"
                if (composed_ok or probe_ok)
                else "  #01 [tool] bash (ok, 900ms): pytest\n  TIMEOUT after 60s\n"
            )
            r.tasks[t] = ev.TaskResult(
                task=t, passes=p, runs=1, mean_s=1.0, mean_tokens=tok,
                failure_trajectory=("" if p else traj),
                trajectory=(traj if (keep_trajectory or not p) else ""),
            )
        return r

    class H2Teacher:
        name = "h2_mock"

        def __init__(self):
            self.calls = []

        def propose(self, prompt: str) -> str:
            self.calls.append(prompt)
            if "Compose phase" in prompt:
                return (
                    '```toml\nsystem_prompt = "COMPOSED_RULE concise verification"\n```\n'
                    '```json\n'
                    '{"symptoms": ["timeout"], "surfaces": ["system_prompt"], '
                    '"edit_pattern": "Use targeted verification before exit", '
                    '"why": "Probe #1 avoided timeout and used 500 fewer tokens"}\n'
                    '```'
                )
            return '```toml\nsystem_prompt = "PROBE_RULE targeted check"\n```'

    restore = _install(h2_eval)
    t_backend = H2Teacher()
    forge.teacher.build_panel = lambda verbose=True: [t_backend]
    lessons_file = forge.RUNS_DIR / "lessons.jsonl"
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_alpha"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_H2_PAR",
            h2_k=2, h2_mode="parallel", h2_tasks=1, h2_blind=True,
            lessons_path=lessons_file, enable_lessons=True,
        )
        rnd0 = lin["rounds"][0]
        assert rnd0.get("accept", {}).get("mode") == "pass-gain", rnd0
        assert len(rnd0.get("h2", [])) == 1, rnd0
        assert len(rnd0["h2"][0]["probes"]) == 3, rnd0["h2"][0]  # 1 control + 2 probes
        assert lessons_file.is_file(), "lessons.jsonl should be written on ACCEPT"
        lessons_data = [
            json.loads(line)
            for line in lessons_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        assert len(lessons_data) == 1 and lessons_data[0]["support"] == 1, lessons_data

        # Run a second sequential round that retrieves the lesson from round 1
        lin_seq = forge.train(
            rounds=1, train_tasks=["t_alpha"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_H2_SEQ",
            h2_k=2, h2_mode="sequential", h2_tasks=1, h2_blind=True,
            lessons_path=lessons_file, enable_lessons=True,
        )
        rnd_seq = lin_seq["rounds"][0]
        assert "L-0001" in rnd_seq["h2"][0]["lessons_used"], rnd_seq["h2"][0]
    finally:
        restore()


# ---------------------------------------------------------------------------
# Follow-up fixes: blind fallback, budget exit, threaded probes, lesson library
# persistence/concurrency, stagnation accounting.
# ---------------------------------------------------------------------------

_TRAJ_A = "  #01 [tool] read_file (ok, 10ms): mod.py\n  #02 [tool] edit_file (ok, 12ms): mod.py\n"
_TRAJ_B = "  #01 [tool] bash (ok, 900ms): pytest\n  TIMEOUT after 60s\n"


def _probe(idx, traj, passed=0, tokens=1000.0, text="x"):
    return forge.Probe(
        idx=idx, kind="control" if idx == 0 else "general",
        genome=genome.Genome(system_prompt=text), path=Path(f"p{idx}.toml"),
        trajectory=traj, passed=passed, runs=1, mean_tokens=tokens,
    )


def test_h2_blind_fallback_ignores_grader_scores():
    ctrl = _probe(0, _TRAJ_B)
    same_as_ctrl_but_passed = _probe(1, _TRAJ_B, passed=1, text="p1")
    behaviorally_new = _probe(2, _TRAJ_A, passed=0, text="p2")
    probes = [ctrl, same_as_ctrl_but_passed, behaviorally_new]
    # Blind: pick by behavior (no timeout, largest divergence) -> probe #2.
    assert forge._select_fallback_probe(probes, blind=True).idx == 2
    # Flipping pass counts must not change the blind choice.
    same_as_ctrl_but_passed.passed, behaviorally_new.passed = 0, 1
    assert forge._select_fallback_probe(probes, blind=True).idx == 2
    same_as_ctrl_but_passed.passed, behaviorally_new.passed = 1, 0
    # Non-blind ablation uses the probe score.
    assert forge._select_fallback_probe(probes, blind=False).idx == 1
    # Deterministic tie-break on identical behavior: lowest idx.
    twins = [ctrl, _probe(1, _TRAJ_A), _probe(2, _TRAJ_A)]
    assert forge._select_fallback_probe(twins, blind=True).idx == 1


def _h2_fake_eval(sleep_s=0.0, tracker=None):
    import threading
    import time as _t

    lock = threading.Lock()

    def h2_eval(genome_path, tasks, repeats, timeout, model=None, keep_trajectory=False):
        if tracker is not None:
            with lock:
                tracker["active"] += 1
                tracker["peak"] = max(tracker["peak"], tracker["active"])
        try:
            if sleep_s:
                _t.sleep(sleep_s)
            content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
            ok = "PROBE_RULE" in content or "COMPOSED_RULE" in content
            r = ev.EvalResult(genome=str(genome_path))
            for t in tasks:
                p = 1 if (t.startswith("hold") or ok) else 0
                traj = _TRAJ_A if ok else _TRAJ_B
                # Encode the genome file name so threaded results can be checked.
                traj = traj + f"  genome={Path(genome_path).name}\n"
                r.tasks[t] = ev.TaskResult(
                    task=t, passes=p, runs=1, mean_s=1.0,
                    mean_tokens=700.0 if ok else 1200.0,
                    failure_trajectory=("" if p else traj),
                    trajectory=(traj if (keep_trajectory or not p) else ""),
                )
            return r
        finally:
            if tracker is not None:
                with lock:
                    tracker["active"] -= 1

    return h2_eval


class _ProbeOnlyTeacher:
    """Proposes a valid probe; the compose reply contains no TOML (rejected)."""

    name = "h2_probe_only"

    def __init__(self, n_variants=1):
        self.calls = []
        self.n = 0
        self.n_variants = n_variants

    def propose(self, prompt: str) -> str:
        self.calls.append(prompt)
        if "Compose phase" in prompt:
            return "I cannot decide on a composition."
        self.n += 1
        return f'```toml\nsystem_prompt = "PROBE_RULE variant {self.n % self.n_variants}"\n```'


def test_h2_compose_rejected_falls_back_to_probe_and_still_gated():
    restore = _install(_h2_fake_eval())
    tb = _ProbeOnlyTeacher()
    forge.teacher.build_panel = lambda verbose=True: [tb]
    try:
        lin = forge.train(
            rounds=1, train_tasks=["t_alpha"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_H2_FB",
            h2_k=2, h2_mode="parallel", h2_tasks=1, h2_blind=True, enable_lessons=False,
        )
        rnd0 = lin["rounds"][0]
        assert rnd0["h2"][0]["composed"] is True, rnd0
        assert rnd0.get("accept", {}).get("mode") == "pass-gain", rnd0
        ledger = (forge.RUNS_DIR / "ledger_T_H2_FB.jsonl").read_text(encoding="utf-8")
        rows = [json.loads(x) for x in ledger.splitlines() if x.strip()]
        assert any(r.get("phase") == "h2-compose" and r.get("verdict") == "rejected"
                   for r in rows), rows
    finally:
        restore()


def test_h2_budget_exhaustion_exits_cleanly():
    restore = _install(_h2_fake_eval())
    tb = _ProbeOnlyTeacher()
    forge.teacher.build_panel = lambda verbose=True: [tb]
    try:
        # Budget already exhausted after gen0 evaluation -> no rounds, no teacher calls.
        lin = forge.train(
            rounds=3, train_tasks=["t_alpha"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=-1, teachers="panel", stamp="T_H2_BUD",
            h2_k=2, enable_lessons=False,
        )
        assert lin["rounds"] == [], lin["rounds"]
        assert tb.calls == []

        # Budget expiring inside a PPC round: only the control probe runs, nothing composed.
        res = forge._h2_round(
            1, "t_alpha", _TRAJ_B, BASELINE.copy(), BASELINE.copy(), [tb],
            k=3, mode="parallel", max_surfaces=None, focus_surface=None,
            max_changed_lines=None, max_token_growth=None, forbidden_terms=[],
            rejected_history=[], touched_counts={}, lesson_store=None, timeout=10,
            probe_repeats=1, workers=2, blind=True, stamp="T_H2_BUD2",
            elapsed_fn=lambda: 10.0, budget_s=1.0,
        )
        assert len(res.probes) == 1 and res.composed is None, res
        assert tb.calls == []
    finally:
        restore()


def test_h2_parallel_workers_run_concurrently_and_map_results():
    tracker = {"active": 0, "peak": 0}
    restore = _install(_h2_fake_eval(sleep_s=0.2, tracker=tracker))
    tb = _ProbeOnlyTeacher(n_variants=3)
    forge.GENOMES_DIR.mkdir(parents=True, exist_ok=True)
    forge.RUNS_DIR.mkdir(parents=True, exist_ok=True)
    try:
        res = forge._h2_round(
            1, "t_alpha", _TRAJ_B, BASELINE.copy(), BASELINE.copy(), [tb],
            k=3, mode="parallel", max_surfaces=None, focus_surface=None,
            max_changed_lines=None, max_token_growth=None, forbidden_terms=[],
            rejected_history=[], touched_counts={}, lesson_store=None, timeout=10,
            probe_repeats=1, workers=3, blind=True, stamp="T_H2_THR",
            elapsed_fn=lambda: 0.0, budget_s=999,
        )
        assert len(res.probes) == 4, res.probes
        assert tracker["peak"] >= 2, tracker
        for p in res.probes:
            assert f"genome={p.path.name}" in p.trajectory, (p.idx, p.trajectory)
        assert all(p.passed == 1 for p in res.probes[1:])
        assert res.probes[0].passed == 0
    finally:
        restore()


def test_h2_accepted_round_resets_stagnation_even_without_variation():
    # Trajectories/tokens are identical for every genome -> no behavioral variation,
    # but the probe genome still clears the train gate on t_alpha.
    def flat_eval(genome_path, tasks, repeats, timeout, model=None, keep_trajectory=False):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        ok = "PROBE_RULE" in content
        r = ev.EvalResult(genome=str(genome_path))
        for t in tasks:
            p = 1 if t.startswith("hold") or (ok and t == "t_alpha") else 0
            r.tasks[t] = ev.TaskResult(
                task=t, passes=p, runs=1, mean_s=1.0, mean_tokens=1000.0,
                failure_trajectory=("" if p else _TRAJ_B),
                trajectory=_TRAJ_B,
            )
        return r

    restore = _install(flat_eval)
    tb = _ProbeOnlyTeacher()
    forge.teacher.build_panel = lambda verbose=True: [tb]
    try:
        lin = forge.train(
            rounds=2, train_tasks=["t_alpha", "t_gamma"], holdout_tasks=["hold_beta"],
            repeats=1, timeout=10, budget_s=999, teachers="panel", stamp="T_H2_STAG",
            h2_k=1, h2_tasks=1, stagnation_limit=1, enable_lessons=False,
        )
        r1, r2 = lin["rounds"][0], lin["rounds"][1]
        assert r1.get("accept", {}).get("mode") == "pass-gain", r1
        assert r1.get("h2_behavioral_variation") is False, r1
        assert r2["focus_surface"] is None, r2
    finally:
        restore()


def test_lesson_library_bootstrap_from_ledgers():
    import lesson_lib

    tmp = Path(tempfile.mkdtemp(prefix="forge_boot_"))
    runs = tmp / "runs"
    runs.mkdir()
    rows = [
        {"round": 1, "verdict": "accepted", "target_surfaces": ["system_prompt"],
         "diff": "system_prompt: 4 -> 30 chars", "reason": "pass-gain"},
        {"round": 2, "verdict": "rejected", "target_surfaces": ["tool_desc.bash"],
         "diff": "x", "reason": "below-margin"},
        {"round": 3, "verdict": "accepted", "target_surfaces": [], "diff": "y"},
    ]
    (runs / "ledger_S9.jsonl").write_text(
        "\n".join(json.dumps(r) for r in rows) + "\nnot json\n", encoding="utf-8"
    )
    lib = lesson_lib.LessonLibrary(tmp / "lessons.jsonl")
    assert lib.bootstrap_from_ledgers(runs) == 1
    recs = lib.load_all()
    assert len(recs) == 1 and recs[0]["stamp"] == "S9", recs
    assert recs[0]["trigger"]["surfaces"] == ["system_prompt"]
    assert lib.bootstrap_from_ledgers(tmp / "missing") == 0
    assert not (tmp / "lessons.jsonl.lock").exists()


def test_lesson_library_ids_are_collision_free_and_concurrent_writes_kept():
    import threading
    import lesson_lib

    tmp = Path(tempfile.mkdtemp(prefix="forge_lconc_"))
    path = tmp / "lessons.jsonl"
    # Pre-existing file with a gap / corrupt line: len(records)+1 would collide.
    path.write_text(
        json.dumps({"id": "L-0005", "edit_pattern": "old", "trigger": {}, "support": 1,
                    "refute": 0, "status": "active"}) + "\n{corrupt\n",
        encoding="utf-8",
    )
    errors = []

    def worker(i):
        try:
            lib = lesson_lib.LessonLibrary(path)
            lib.record_lesson(
                {"symptoms": [f"sym{i}"], "edit_pattern": f"pattern number {i}", "why": "w"},
                stamp="C", round_num=i,
            )
        except Exception as e:  # pragma: no cover - reported via assert
            errors.append(e)

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(12)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert not errors, errors
    recs = lesson_lib.LessonLibrary(path).load_all()
    ids = [r["id"] for r in recs]
    assert len(recs) == 13, ids
    assert len(set(ids)) == len(ids), ids
    assert "L-0006" in ids and "L-0002" not in ids, ids
    assert not (tmp / "lessons.jsonl.lock").exists()
    assert not list(tmp.glob("*.tmp"))


def test_lesson_merge_requires_symptom_overlap_and_caps_symptoms():
    import lesson_lib

    tmp = Path(tempfile.mkdtemp(prefix="forge_lmerge_"))
    lib = lesson_lib.LessonLibrary(tmp / "lessons.jsonl")
    pat = "Run targeted pytest -k before exit"
    a = lib.record_lesson({"symptoms": ["timeout"], "edit_pattern": pat}, stamp="M")
    b = lib.record_lesson({"symptoms": ["import_error"], "edit_pattern": pat}, stamp="M")
    assert a["id"] != b["id"], (a, b)
    c = lib.record_lesson(
        {"symptoms": ["timeout"] + [f"s{i}" for i in range(20)], "edit_pattern": pat},
        stamp="M",
    )
    assert c["id"] == a["id"] and c["support"] == 2, c
    assert len(c["trigger"]["symptoms"]) <= lesson_lib.MAX_TRIGGER_SYMPTOMS, c



def _counted_eval(champ_rates: dict[str, int], cand_rates: dict[str, int], runs: int,
                  hold_passes: list[int] | None = None, calls: list | None = None):
    """evaluate() stand-in with `runs` repeats per task. The baseline passes
    `champ_rates[t]` times on train task t, the IMPROVED genome `cand_rates[t]`.
    Holdout: the baseline passes every run; the IMPROVED genome's successive
    holdout evaluations return `hold_passes` in order (default: all pass)."""
    hold_iter = iter(hold_passes or [])

    def fake_eval(genome_path, tasks, repeats, timeout):
        content = Path(genome_path).read_text(encoding="utf-8") if genome_path else ""
        improved = "IMPROVED" in content
        is_holdout = all(t.startswith("hold") for t in tasks)
        if calls is not None:
            calls.append(("cand" if improved else "champ", "hold" if is_holdout else "train"))
        r = ev.EvalResult(genome=str(genome_path))
        hold_total = next(hold_iter, runs * len(tasks)) if (improved and is_holdout) else None
        for t in tasks:
            if is_holdout:
                p = runs if hold_total is None else min(runs, hold_total)
                if hold_total is not None:
                    hold_total -= p
            else:
                p = (cand_rates if improved else champ_rates)[t]
            r.tasks[t] = ev.TaskResult(
                task=t, passes=p, runs=runs, mean_s=1.0,
                failure_trajectory=("" if p == runs else "agent did the wrong thing"),
            )
        return r

    return fake_eval


def _improver_panel():
    forge.teacher.build_panel = lambda verbose=True: [
        FakeTeacher("fake", 'system_prompt = "IMPROVED prompt"')
    ]


def test_pass_noise_floor_matches_pooled_binomial_formula():
    analyze = forge.replay_analyze
    assert analyze.jeffreys(0, 3) == 0.5 / 4
    assert analyze.jeffreys(3, 3) == 3.5 / 4
    # One task, 3 runs each, 0 vs 3 passes: pooled p = jeffreys(3, 6) = 0.5,
    # Var = 6 * 0.25 = 1.5.
    floor = analyze.pass_noise_floor([(0, 3, 3, 3)], z=2.0)
    assert abs(floor - 2.0 * 1.5 ** 0.5) < 1e-9, floor
    # Variance adds across tasks and grows with repeats.
    four = analyze.pass_noise_floor([(0, 3, 3, 3)] * 4, z=2.0)
    assert abs(four - 2.0 * 6.0 ** 0.5) < 1e-9, four
    assert analyze.pass_noise_floor([(1, 1, 1, 1)], z=1.0) < analyze.pass_noise_floor(
        [(5, 10, 5, 10)], z=1.0)
    # forge's helper reads the per-task counts and z <= 0 disables it.
    champ = ev.EvalResult(genome="c")
    cand = ev.EvalResult(genome="d")
    for t in ("a", "b"):
        champ.tasks[t] = ev.TaskResult(task=t, passes=0, runs=3, mean_s=1.0)
        cand.tasks[t] = ev.TaskResult(task=t, passes=3, runs=3, mean_s=1.0)
    expected = analyze.pass_noise_floor([(0, 3, 3, 3), (0, 3, 3, 3)], z=1.5)
    assert forge.train_noise_floor(champ, cand, 1.5) == expected
    assert forge.train_noise_floor(champ, cand, 0.0) == 0.0


def test_noise_gate_rejects_a_gain_inside_sampling_noise():
    # 4 tasks x 3 runs: champion 1/3 each, candidate +2 passes in total. That
    # clears accept_margin=1 but not the ~3.9-pass one-sided noise floor.
    champ = {f"t{i}": 1 for i in range(4)}
    cand = dict(champ, t0=2, t1=2)
    restore = _install(_counted_eval(champ, cand, runs=3))
    _improver_panel()
    try:
        lin = forge.train(
            rounds=1, train_tasks=list(champ), holdout_tasks=["hold_a"],
            repeats=3, timeout=10, budget_s=999, teachers="panel", stamp="T_NOISE_REJ",
            accept_margin=1, noise_z=forge.NOISE_Z_DEFAULT,
        )
        rnd = lin["rounds"][0]
        assert "accept" not in rnd or "mode" not in rnd["accept"], rnd
        assert rnd["required_margin"] > 2, rnd
        assert "system_prompt" not in lin["champion"]["diff_vs_baseline"]
    finally:
        restore()

    # The same +2 gain is accepted when the noise floor is turned off.
    restore = _install(_counted_eval(champ, cand, runs=3))
    _improver_panel()
    try:
        lin = forge.train(
            rounds=1, train_tasks=list(champ), holdout_tasks=["hold_a"],
            repeats=3, timeout=10, budget_s=999, teachers="panel", stamp="T_NOISE_OFF",
            accept_margin=1, noise_z=0.0,
        )
        assert lin["rounds"][0]["accept"]["mode"] == "pass-gain", lin["rounds"][0]
        assert lin["rounds"][0]["required_margin"] == 1
    finally:
        restore()


def test_noise_gate_accepts_a_gain_well_above_noise():
    # 0/3 -> 3/3 on 4 tasks: +12 passes against a ~4.0-pass floor.
    champ = {f"t{i}": 0 for i in range(4)}
    cand = {f"t{i}": 3 for i in range(4)}
    restore = _install(_counted_eval(champ, cand, runs=3))
    _improver_panel()
    try:
        lin = forge.train(
            rounds=1, train_tasks=list(champ), holdout_tasks=["hold_a"],
            repeats=3, timeout=10, budget_s=999, teachers="panel", stamp="T_NOISE_ACC",
            accept_margin=2, noise_z=forge.NOISE_Z_DEFAULT,
        )
        rnd = lin["rounds"][0]
        assert rnd["accept"]["mode"] == "pass-gain", rnd
        assert 4 < rnd["noise_floor"] < 5 and rnd["required_margin"] == 5, rnd
        assert rnd["accept"]["required_margin"] == 5
    finally:
        restore()


def test_accepted_champion_holdout_is_reevaluated_once_and_becomes_the_bar():
    # Holdout of the accepted genome: 3/3 at the gate, then a fresh 1/3.
    champ = {"t0": 0, "t1": 0}
    cand = {"t0": 3, "t1": 3}
    calls: list = []
    restore = _install(_counted_eval(champ, cand, runs=3, hold_passes=[3, 1], calls=calls))
    _improver_panel()
    try:
        lin = forge.train(
            rounds=2, train_tasks=list(champ), holdout_tasks=["hold_a"],
            repeats=3, timeout=10, budget_s=999, teachers="panel", stamp="T_HOLD_RE",
            accept_margin=1, noise_z=0.0,
        )
        acc = lin["rounds"][0]["accept"]
        assert acc["holdout"] == 3 and acc["holdout_reeval"] == 1, acc
        # Gate eval + one fresh re-eval; round 2 reuses the fresh score instead
        # of evaluating the new champion's holdout a third time.
        assert calls.count(("cand", "hold")) == 2, calls
        assert lin["champion"]["holdout"] == 1, lin["champion"]
        ledger = (forge.RUNS_DIR / "ledger_T_HOLD_RE.jsonl").read_text(encoding="utf-8")
        accepted = [json.loads(line) for line in ledger.splitlines()
                    if json.loads(line).get("verdict") == "accepted"]
        assert accepted[0]["holdout_reeval_passes"] == 1, accepted
    finally:
        restore()


def test_cli_defaults_repeat_three_times_and_enable_the_noise_gate():
    captured: dict = {}
    fake_bin = Path(tempfile.mkdtemp(prefix="forge_cli_")) / "ncx.exe"
    fake_bin.write_text("", encoding="utf-8")
    orig = (forge.ev.bench.NCX, forge.splits.load_splits, forge.train, sys.argv)
    forge.ev.bench.NCX = fake_bin
    forge.splits.load_splits = lambda: {"train": ["t"], "val": ["h"], "test": ["x"]}

    def fake_train(rounds, train_tasks, holdout_tasks, repeats, timeout, *args, **kwargs):
        captured.update(repeats=repeats, **kwargs)
        return {}

    forge.train = fake_train
    sys.argv = ["forge.py", "--train", "--no-gate"]
    try:
        assert forge.main() == 0
    finally:
        forge.ev.bench.NCX, forge.splits.load_splits, forge.train, sys.argv = orig
    assert captured["repeats"] == 3, captured
    assert captured["accept_margin"] == 2, captured
    assert captured["noise_z"] == forge.NOISE_Z_DEFAULT > 0, captured

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
