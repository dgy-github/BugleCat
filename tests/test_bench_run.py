from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

_BENCH_RUN_PATH = Path(__file__).resolve().parents[1] / "bench" / "run.py"


@pytest.fixture()
def bench():
    """Load bench/run.py as an isolated module without invoking any model."""
    spec = importlib.util.spec_from_file_location("bench_run", _BENCH_RUN_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _evidence(
    *,
    passed: bool,
    elapsed_s: float,
    failure_kind: str | None = None,
    missing: list[str] | None = None,
    timed_out: bool = False,
    note: str = "",
    compile_ok: bool = True,
) -> dict:
    return {
        "task": "t1",
        "agent": "fake",
        "passed": passed,
        "elapsed_s": elapsed_s,
        "timed_out": timed_out,
        "grader_note": note,
        "failure_kind": failure_kind,
        "missing_expected_files": [] if missing is None else missing,
        "stdout_tail": [],
        "stderr_tail": [],
        "python_compile": [] if compile_ok else [{"file": "t1.py", "ok": False}],
    }


def test_failure_kind_compile_failed_takes_priority(bench):
    evidence = {
        "passed": False,
        "timed_out": False,
        "python_compile": [{"file": "t1.py", "ok": False}],
        "missing_expected_files": ["t1.py"],
        "stdout_tail": ["model returned empty response"],
        "stderr_tail": [],
    }
    assert bench._failure_kind(evidence) == "compile_failed"


def test_failure_kind_empty_response_with_missing_files(bench):
    evidence = {
        "passed": False,
        "timed_out": False,
        "python_compile": [],
        "missing_expected_files": ["t1.py"],
        "stdout_tail": ["model returned empty response"],
        "stderr_tail": [],
    }
    assert bench._failure_kind(evidence) == "empty_response"


def test_failure_kind_detects_chinese_empty_response(bench):
    evidence = {
        "passed": False,
        "timed_out": False,
        "python_compile": [],
        "missing_expected_files": ["t1.py"],
        "stdout_tail": ["空内容"],
        "stderr_tail": [],
    }
    assert bench._failure_kind(evidence) == "empty_response"


def test_failure_kind_successful_empty_response_mention_is_not_failure(bench):
    evidence = {
        "passed": True,
        "timed_out": False,
        "python_compile": [],
        "missing_expected_files": [],
        "stdout_tail": ["note: an empty response was retried internally"],
        "stderr_tail": [],
    }
    assert bench._failure_kind(evidence) is None


def test_failure_kind_missing_files_without_empty_response(bench):
    evidence = {
        "passed": False,
        "timed_out": False,
        "python_compile": [],
        "missing_expected_files": ["t1.py"],
        "stdout_tail": [],
        "stderr_tail": [],
    }
    assert bench._failure_kind(evidence) == "missing_expected_file"


def test_failure_kind_timeout_and_grader_failed(bench):
    timed_out = {
        "passed": False,
        "timed_out": True,
        "python_compile": [],
        "missing_expected_files": [],
        "stdout_tail": [],
        "stderr_tail": [],
    }
    assert bench._failure_kind(timed_out) == "timeout"
    grader_failed = dict(timed_out, timed_out=False)
    assert bench._failure_kind(grader_failed) == "grader_failed"


def test_recovery_retries_empty_response_then_succeeds(bench, monkeypatch, capsys):
    calls = [
        _evidence(passed=False, elapsed_s=1.0, failure_kind="empty_response",
                  missing=["t1.py"], note="empty response"),
        _evidence(passed=True, elapsed_s=2.0, failure_kind=None),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=1, empty_response_retries=2)
    row = results["t1"]
    assert row["passes"] == 1
    assert row["runs"] == 1
    assert [item["attempt"] for item in row["attempts"]] == [1, 1]
    assert [item["retry"] for item in row["attempts"]] == [0, 1]
    assert [item["empty_response_retries"] for item in row["attempts"]] == [1, 1]
    assert row["mean_s"] == 3.0
    assert row["timeouts"] == 0


def test_exhaustion_stops_after_bounded_retries(bench, monkeypatch):
    calls = [
        _evidence(passed=False, elapsed_s=1.0, failure_kind="empty_response",
                  missing=["t1.py"], timed_out=False),
        _evidence(passed=False, elapsed_s=2.0, failure_kind="empty_response",
                  missing=["t1.py"], timed_out=True),
        _evidence(passed=False, elapsed_s=3.0, failure_kind="empty_response",
                  missing=["t1.py"], timed_out=False),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=1, empty_response_retries=2)
    row = results["t1"]
    assert row["passes"] == 0
    assert row["runs"] == 1
    assert [item["retry"] for item in row["attempts"]] == [0, 1, 2]
    assert row["timeouts"] == 1
    assert row["mean_s"] == 6.0
    assert row["attempts"][-1]["empty_response_retries"] == 2


def test_accounting_keeps_logical_runs_and_sums_physical_durations(bench, monkeypatch):
    calls = [
        _evidence(passed=False, elapsed_s=1.0, failure_kind="empty_response",
                  missing=["t1.py"]),
        _evidence(passed=True, elapsed_s=2.0, failure_kind=None),
        _evidence(passed=False, elapsed_s=4.0, failure_kind=None, note="grader failed"),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=2, empty_response_retries=1)
    row = results["t1"]
    assert row["runs"] == 2
    assert row["passes"] == 1
    assert [(item["attempt"], item["retry"]) for item in row["attempts"]] == [
        (1, 0),
        (1, 1),
        (2, 0),
    ]
    assert row["mean_s"] == 3.5


def test_zero_retries_disables_retry(bench, monkeypatch):
    calls = [
        _evidence(passed=False, elapsed_s=1.0, failure_kind="empty_response",
                  missing=["t1.py"]),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=1, empty_response_retries=0)
    row = results["t1"]
    assert len(row["attempts"]) == 1
    assert row["attempts"][0]["retry"] == 0
    assert row["attempts"][0]["empty_response_retries"] == 0
    assert row["passes"] == 0


def test_recovery_retries_empty_response_with_existing_files(bench, monkeypatch):
    calls = [
        _evidence(passed=False, elapsed_s=1.0, failure_kind="empty_response", missing=[]),
        _evidence(passed=True, elapsed_s=2.0, failure_kind=None),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=1, empty_response_retries=2)
    row = results["t1"]
    assert row["passes"] == 1
    assert row["runs"] == 1
    assert [item["attempt"] for item in row["attempts"]] == [1, 1]
    assert [item["retry"] for item in row["attempts"]] == [0, 1]
    assert [item["empty_response_retries"] for item in row["attempts"]] == [1, 1]
    assert row["mean_s"] == 3.0


def test_exhaustion_retries_empty_response_with_existing_files(bench, monkeypatch):
    calls = [
        _evidence(passed=False, elapsed_s=2.0, failure_kind="empty_response", missing=[]),
        _evidence(passed=False, elapsed_s=2.0, failure_kind="empty_response", missing=[]),
        _evidence(passed=False, elapsed_s=2.0, failure_kind="empty_response", missing=[]),
    ]
    monkeypatch.setattr(bench, "tasks", lambda filt="": [Path("t1")])
    monkeypatch.setattr(bench, "_run_once_detailed", lambda *a, **k: calls.pop(0))

    results = bench.run_agent("fake", timeout=60, repeats=1, empty_response_retries=2)
    row = results["t1"]
    assert row["passes"] == 0
    assert row["runs"] == 1
    assert [item["attempt"] for item in row["attempts"]] == [1, 1, 1]
    assert [item["retry"] for item in row["attempts"]] == [0, 1, 2]
    assert row["timeouts"] == 0
    assert row["mean_s"] == 6.0
    assert row["attempts"][-1]["empty_response_retries"] == 2
