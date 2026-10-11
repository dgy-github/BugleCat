"""Real credential-stripped child execution, no model/service requests."""

import json
import subprocess

import pytest

from evals.verification.apps_v3 import oracle


def run(code, inputs, outputs, **kwargs):
    return oracle.execute(code, {"fn_name": "f", "inputs": inputs, "outputs": outputs}, **kwargs)


def test_persistent_namespace_and_all_pairs_after_runtime_failure():
    receipt = run("state=[]\ndef f(x):\n state.append(x)\n if x == 2: raise ValueError()\n return len(state)",
                  [[1], [2], [3]], [1, 2, 3])
    assert receipt["pair_count"] == 3
    result = receipt["result"]
    assert result["status"] == "failed"
    assert [item["passed"] for item in result["pair_results"]] == [True, False, True]
    assert result["pairs_executed"] == 3


def test_original_dictionary_and_return_quirks_in_real_worker():
    result = run("def f(x): return tuple(x.values())", [[{"1": 7}, 99]], [[[7]]])["result"]
    assert result["status"] == "passed"


def test_pinned_implicit_stdlib_names():
    result = run("def f(x: List[int]): return heappop(x) + Counter(x)[2] + int(sqrt(4))",
                 [[[1, 2, 2]]], [5])["result"]
    assert result["status"] == "passed"


@pytest.mark.parametrize("code,reason", [
    ("import os\ndef f(): return 1", "restricted_import"),
    ("def f(): return open('secret')", "restricted_name"),
    ("class Solution:\n def f(self): return 1", "unsupported_construct"),
    ("# class Solution\ndef f(): return 1", "official_solution_branch_outside_scope"),
    ("import math\nmath.pi=0\ndef f(): return 1", "oracle_dependency_mutation"),
    ("len=1\ndef f(): return 1", "oracle_builtin_shadowed"),
    ("def f(): return np.array([1])", "unsupported_implicit_dependency"),
    ("def f(): return format(1, 'd')", "unsupported_implicit_builtin"),
    ("def impl(): return 1\nf=impl", "unsupported_entry_point_form"),
    ("def f(): return iter([1])", "unsupported_comparison_value"),
])
def test_unsupported_scope_is_unknown(code, reason):
    result = run(code, [[]], [1])["result"]
    assert result == {"status": "unknown", "reason": reason} or (
        result["status"] == "unknown" and result["reason"] == reason)


def test_timeout_keeps_true_pair_count_and_no_synthetic_failures():
    receipt = run("def f():\n while True: pass", [[], []], [1, 1], timeout=1)
    assert receipt["pair_count"] == 2
    assert receipt["result"] == {"status": "unknown", "reason": "worker_timeout", "pairs_executed": None}


def test_syntax_error_is_actual_failure_without_fake_pair_results():
    receipt = run("def f(:", [[]], [1])
    assert receipt["result"]["status"] == "failed"
    assert receipt["result"]["error_kind"] == "SyntaxError"
    assert receipt["result"]["pair_results"] == []


@pytest.mark.parametrize("exception", ["ValueError", "TypeError", "KeyError"])
def test_artifact_initialization_exception_is_not_a_policy_refusal(exception):
    receipt = run(f"raise {exception}('initialization error')\ndef f(): return 1", [[]], [1])
    assert receipt["result"]["status"] == "failed"
    assert receipt["result"]["error_kind"] == exception
    assert receipt["result"]["failure_phase"] == "compile_or_lookup"


@pytest.mark.parametrize("result", [
    {"status": "passed"},
    {"status": "passed", "pairs_executed": 2, "pair_results": [{"index": 0, "passed": True}]},
    {"status": "failed", "pairs_executed": 1, "pair_results": [{"index": 0, "passed": True}]},
    {"status": "passed", "pairs_executed": 1, "pair_results": [{"index": 3, "passed": True}]},
    {"status": "passed", "pairs_executed": 1, "pair_results": [{"index": 0, "passed": 1}]},
    {"status": "unknown", "reason": "resource", "pairs_executed": 3},
    {"status": "failed", "failure_phase": "compile_or_lookup", "error_kind": "ValueError", "pairs_executed": False, "pair_results": []},
])
def test_malformed_worker_evidence_is_unknown(result):
    assert oracle.checked_result(result, 1) == {"status": "unknown", "reason": "invalid_worker_output"}


def test_empty_or_malformed_pairs_are_unknown():
    assert run("def f(): return 1", [], [])["result"]["status"] == "unknown"
    assert run("def f(): return 1", [[]], [])["result"]["status"] == "unknown"
    receipt = run("def f(): return 1", None, [1])
    assert receipt["pair_count"] is None
    assert receipt["result"]["status"] == "unknown"


def test_environment_excludes_credentials_and_source_mutation_fails(monkeypatch, tmp_path):
    # Test the actual boundary, without putting credentials in code/payload.
    graph = {"worker": tmp_path / "worker", "runtime": tmp_path / "runtime"}
    for path in graph.values():
        path.write_bytes(b"original")
    monkeypatch.setattr(oracle, "source_graph", lambda: graph)
    monkeypatch.setenv("DEEPSEEK_API_KEY", "test-secret")
    monkeypatch.setenv("NCX_JEV_API_KEY", "test-secret")

    def fake_child(argv, **kwargs):
        assert "DEEPSEEK_API_KEY" not in kwargs["env"]
        assert "NCX_JEV_API_KEY" not in kwargs["env"]
        assert argv[1:4] == ["-I", "-S", "-B"]
        graph["worker"].write_bytes(b"changed")
        return subprocess.CompletedProcess(argv, 0, json.dumps({"status": "passed"}), "")
    monkeypatch.setattr(oracle.subprocess, "run", fake_child)
    with pytest.raises(ValueError, match="changed during execution"):
        run("def f(): return 1", [[]], [1])
