"""Cargo-tier cases must not pass vacuously when a filter matches zero tests."""

from evals.golden import runner

_OK_ZERO = "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 372 filtered out\n"
_OK_SOME = (
    "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 367 filtered out\n"
    "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
)


def test_cargo_tests_run_counts_across_binaries():
    assert runner.cargo_tests_run(_OK_ZERO) == 0
    assert runner.cargo_tests_run(_OK_SOME) == 5
    assert runner.cargo_tests_run("test result: FAILED. 3 passed; 2 failed; 0 ignored") == 5


def test_zero_matched_cargo_case_fails(monkeypatch):
    monkeypatch.setattr(runner, "_cargo_available", lambda: True)
    monkeypatch.setattr(runner, "_run", lambda *a, **k: (0, _OK_ZERO))
    case = {"id": "X", "capability": "tools", "tier": "cargo", "kind": "cargo", "args": ["test"]}
    out = runner.execute_case(case, {"cargo"})
    assert out["status"] == "failed"
    assert "zero tests" in out["reason"]


def test_cargo_case_with_tests_passes(monkeypatch):
    monkeypatch.setattr(runner, "_cargo_available", lambda: True)
    monkeypatch.setattr(runner, "_run", lambda *a, **k: (0, _OK_SOME))
    case = {"id": "X", "capability": "tools", "tier": "cargo", "kind": "cargo", "args": ["test"]}
    assert runner.execute_case(case, {"cargo"})["status"] == "passed"


def test_rust_mainline_modules_are_covered():
    args = [" ".join(c.get("args", [])) for c in runner.load_manifest()["cases"] if c["kind"] == "cargo"]
    for module in ("context_store::", "context_retrieval::", "memory_index::",
                   "semantic_compaction::", "tool_recovery::", "agent_state::", "instruction_state::"):
        assert any(module in a for a in args), module
