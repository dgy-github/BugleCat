"""Benchmark exit-code gate: 0/N must never look like a pass."""

import json
import pathlib

from evals import harness

BASELINE = pathlib.Path(__file__).resolve().parents[1] / "bench" / "reports" / "trace_20261006_141227.jsonl"


def test_gate_fails_closed_on_empty_run():
    gate = harness.benchmark_gate({"runs": 0, "task_success_rate": None}, 0.0)
    assert gate["passed"] is False


def test_gate_fails_below_threshold_and_passes_at_threshold():
    assert harness.benchmark_gate({"runs": 20, "task_success_rate": 0.0}, 0.8)["passed"] is False
    assert harness.benchmark_gate({"runs": 20, "task_success_rate": 0.75}, 0.8)["passed"] is False
    assert harness.benchmark_gate({"runs": 20, "task_success_rate": 0.8}, 0.8)["passed"] is True


def test_gate_fails_closed_when_metric_missing():
    gate = harness.benchmark_gate({"runs": 20, "task_success_rate": 1.0}, 0.45, "clean_pass_rate")
    assert gate["passed"] is False and "clean_pass_rate" in gate["reason"]


def test_timed_out_passes_are_not_clean():
    rows = [
        {"task": "a", "runs": 3, "passes": 3, "clean_passes": 1, "timeouts": 2},
        {"task": "b", "runs": 3, "passes": 2, "clean_passes": 2, "timeouts": 0},
    ]
    s = harness.summarize_benchmark("x", rows)
    assert s["task_success_rate"] == round(5 / 6, 4)
    assert s["clean_pass_rate"] == round(3 / 6, 4)
    assert s["timeouts"] == 2
    # Saturated pass rate must not satisfy a clean-rate gate on its own.
    assert harness.benchmark_gate(s, 0.6, "task_success_rate")["passed"] is True
    assert harness.benchmark_gate(s, 0.6, "clean_pass_rate")["passed"] is False


def _fake(rate_pass, rate_clean):
    return {"benchmark": {"agent": "x", "tasks": [], "task_success_rate": rate_pass,
                          "clean_pass_rate": rate_clean, "runs": 60, "passes": 60, "clean_passes": 31}}


def test_cli_defaults_to_clean_metric(monkeypatch, tmp_path):
    # 60/60 passed but only 31 clean: plain pass rate would hide the timeouts.
    monkeypatch.setattr(harness, "_benchmark", lambda args: _fake(1.0, 0.4))
    out = tmp_path / "r.json"
    assert harness.main(["benchmark", "--min-pass-rate", "0.45", "--output", str(out)]) == 1
    gate = json.loads(out.read_text(encoding="utf-8"))["benchmark"]["gate"]
    assert gate["metric"] == "clean_pass_rate" and gate["passed"] is False
    assert harness.main(["benchmark", "--min-pass-rate", "0.45", "--gate-metric", "pass",
                         "--output", str(out)]) == 0


def test_cli_passes_when_clean_rate_meets_threshold(monkeypatch, tmp_path):
    monkeypatch.setattr(harness, "_benchmark", lambda args: _fake(1.0, 0.5167))
    assert harness.main(["benchmark", "--min-pass-rate", "0.45", "--output", str(tmp_path / "r.json")]) == 0


def test_golden_agent_cases_declare_clean_threshold():
    from evals.golden import runner

    for case in runner.load_manifest()["cases"]:
        if case["kind"] == "agent":
            args = case["args"]
            assert "--min-pass-rate" in args, case["id"]
            assert args[args.index("--gate-metric") + 1] == "clean", case["id"]


def test_recorded_baseline_clean_rate_clears_threshold():
    """The 2026-10-06 baseline (31/60 clean) is what 0.45 was derived from."""
    if not BASELINE.exists():
        return
    rows = [json.loads(line) for line in BASELINE.read_text(encoding="utf-8").splitlines() if line.strip()]
    clean = sum(1 for r in rows if r["passed"] and not r["timed_out"])
    assert (len(rows), clean) == (60, 31)
    assert clean / len(rows) >= 0.45
