"""Offline guards for the six-category evaluator, not model calls."""
import subprocess
from pathlib import Path

import pytest

from evals.self_repair.cases import CASES, mutate
from evals.self_repair.suite_support import audit, export_workspace, fingerprint, safe_child

ROOT = Path(__file__).resolve().parents[1]


def test_redaction_preserves_json_structure():
    import json

    from evals.self_repair.suite_support import sanitize_value
    value = {"output": ["token=abc", "Bearer xyz", "password=abc"], "key": "fixture-value"}
    result = sanitize_value(value, ("fixture-value",))
    assert json.loads(json.dumps(result)) == result
    assert result["key"] == "[REDACTED]"
    assert "abc" not in str(result) and "xyz" not in str(result)


def test_metrics_separate_patch_pass_and_clean_completion():
    from evals.self_repair.suite_report import metrics
    attempt = dict(grader={"status": "passed"}, returncode=1, timed_out=True, elapsed_s=3)
    state = dict(cases=[dict(status="passed", attempts=[attempt]),
                        dict(status="invalid_seed", attempts=[])])
    result = metrics(state)
    assert result["total"] == 2 and result["first_pass"] == 1
    assert result["clean_first"] == 0 and result["invalid"] == 1
    state["cases"][1]["status"] = "infrastructure_error"
    assert metrics(state)["infrastructure"] == 1


def test_grader_does_not_accept_zero_tests_or_infrastructure_failure():
    from evals.self_repair.suite_grading import classify
    result = dict(stdout="test result: ok. 0 passed; 0 failed;", stderr="",
                  returncode=0, timed_out=False, elapsed_s=1)
    assert classify(result, True)["status"] == "grader_error"
    result["stdout"] = "test result: FAILED. 2 passed; 1 failed;"
    result["returncode"] = 101
    assert classify(result, True)["status"] == "test_failed"
    result["timed_out"] = True
    assert classify(result, True)["status"] == "grader_timeout"
    result.update(timed_out=False, stdout="", stderr="error[E0425]: cannot find value `x`")
    assert classify(result, True)["status"] == "compile_failed"
    result["stderr"] = "error: linker `link.exe` not found"
    assert classify(result, True)["status"] == "grader_error"
    result["stderr"] = "IndentationError: unexpected indent"
    assert classify(result, False)["status"] == "compile_failed"


def test_case_selection_is_exact_and_unique():
    from evals.self_repair.suite import select_cases
    assert len(select_cases("")) == 6
    assert [c.id for c in select_cases("s1_retry")] == ["s1_retry"]
    for bad in ("s1", "s1_retry,s1_retry", "unknown"):
        with pytest.raises(ValueError):
            select_cases(bad)


def test_frozen_test_identity_cannot_be_replaced_by_another_passing_test():
    from evals.self_repair.suite_grading import enforce_checks
    result = dict(status="passed", checks=["replacement"], passed=1, failed=0)
    assert enforce_checks(result, {"checks": ["original"], "passed": 1, "failed": 0})["status"] == "integrity_failed"


def test_grading_exception_preserves_completed_invocation(tmp_path, monkeypatch):
    from evals.self_repair import suite
    monkeypatch.setattr(suite, "owned_command", lambda *a, **kw: dict(
        returncode=0, timed_out=False, elapsed_s=2, stdout="", stderr=""))
    monkeypatch.setattr(suite, "audit", lambda *a: (_ for _ in ()).throw(OSError("unreadable")))
    attempts = suite.run_case(CASES[0], tmp_path, {}, "seed", Path("ncx"), tmp_path,
                              {}, "", tmp_path, {})
    assert attempts[0]["elapsed_s"] == 2
    assert attempts[0]["grader"]["status"] == "infrastructure_error"
    assert (tmp_path / "s1_retry_attempts.json").exists()


def test_frozen_toolchain_wrapper_restores_only_build_environment(tmp_path):
    import sys

    from evals.self_repair.toolchain import install_wrapper, minimal_environment
    environment = {"PATH": str(Path(sys.executable).parent), "PROGRAMFILES(X86)": "fixture-vs",
                   "NANOCODEX_API_KEY": "do-not-copy", "CUSTOM_TOKEN": "do-not-copy"}
    install_wrapper(tmp_path, [sys.executable, "-c",
                              "import os; assert os.environ['PROGRAMFILES(X86)']=='fixture-vs'; "
                              "assert 'NANOCODEX_API_KEY' not in os.environ"], environment)
    wrapper = tmp_path / "self_repair_check.py"
    assert "do-not-copy" not in wrapper.read_text(encoding="utf-8")
    subprocess.run([sys.executable, str(wrapper)], env=minimal_environment(environment), check=True)


def test_grader_invalidates_only_mutable_workspace_debug_artifacts(tmp_path, monkeypatch):
    from evals.self_repair import suite_grading
    calls = []

    def command(args, *rest):
        calls.append(args)
        return dict(returncode=0, timed_out=False, elapsed_s=1, stdout="", stderr="")

    monkeypatch.setattr(suite_grading, "owned_command", command)
    suite_grading.invalidate_workspace_artifacts(tmp_path, tmp_path / "cache", {})
    assert calls[0][1:4] == ["clean", "--profile", "dev"]
    assert "--release" not in calls[0] and "--workspace" not in calls[0]
    assert [calls[0][i + 1] for i, arg in enumerate(calls[0]) if arg == "-p"] == [
        "ncx-cli", "ncx-core", "ncx-provider", "ncx-tools"]


def test_report_requires_complete_matching_independent_evidence():
    import copy

    from evals.self_repair.suite_report import validate_recheck
    state = dict(run_id="run", cases=[dict(id="s1", attempts=[dict(attempt=1, grader={"status": "passed"})])])
    checked = dict(run_id="run", checks=[dict(id="s1", integrity_issues=[],
                  healthy={"status": "passed"}, seed={"status": "test_failed"},
                  final={"status": "passed"}, replayed_attempts=[dict(attempt=1, agrees=True,
                      original_status="passed", validation={"status": "passed"}, integrity_issues=[])])])
    validate_recheck(state, checked)
    for bad in ({}, dict(run_id="run", checks=[])):
        with pytest.raises(ValueError):
            validate_recheck(state, bad)
    bad = copy.deepcopy(checked)
    bad["checks"][0]["seed"]["status"] = "passed"
    with pytest.raises(ValueError):
        validate_recheck(state, bad)
    bad = copy.deepcopy(checked)
    bad["checks"][0]["replayed_attempts"][0]["agrees"] = False
    with pytest.raises(ValueError):
        validate_recheck(state, bad)


def test_six_real_targets_have_detectable_mutations():
    assert len(CASES) == 6
    assert len({c.id for c in CASES}) == 6
    for case in CASES:
        original = (ROOT / case.target).read_text(encoding="utf-8")
        broken = mutate(case, original)
        assert original != broken
        with pytest.raises(ValueError):
            mutate(case, broken)


def test_destination_cannot_escape_or_reuse_existing_directory(tmp_path):
    with pytest.raises(ValueError):
        safe_child(tmp_path, "../escape")
    with pytest.raises(ValueError):
        safe_child(tmp_path, str(ROOT))
    (tmp_path / "existing").mkdir()
    with pytest.raises(FileExistsError):
        safe_child(tmp_path, "existing")


def test_snapshot_excludes_answer_history_and_grader(tmp_path):
    source = tmp_path / "source"
    source.mkdir()
    subprocess.run(["git", "init", "-q", str(source)], check=True)
    (source / "a.py").write_text("x=1\n", encoding="utf-8")
    (source / "evals/self_repair").mkdir(parents=True)
    (source / "evals/self_repair/answer.patch").write_text("private", encoding="utf-8")
    (source / "HANDOFF.md").write_text("host context", encoding="utf-8")
    destination = tmp_path / "candidate"
    export_workspace(source, destination)
    assert (destination / "a.py").exists()
    assert not (destination / ".git").exists()
    assert not (destination / "evals/self_repair").exists()
    assert not (destination / "HANDOFF.md").exists()


def test_audit_rejects_checker_changes_and_protected_inline_tests(tmp_path):
    target = tmp_path / "module.rs"
    target.write_text("fn f() {}\n#[cfg(test)]\nmod tests {}\n", encoding="utf-8")
    checker = tmp_path / "test.py"
    checker.write_text("assert True\n", encoding="utf-8")
    baseline = fingerprint(tmp_path)
    target.write_text("fn f() {return;}\n#[cfg(test)]\nmod tests {}\n", encoding="utf-8")
    assert audit(tmp_path, baseline, "module.rs") == []
    target.write_text("fn f() {}\n#[cfg(test)]\nmod tests { /* weakened */ }\n", encoding="utf-8")
    assert audit(tmp_path, baseline, "module.rs")
    checker.write_text("assert False\n", encoding="utf-8")
    assert any("test.py" in issue for issue in audit(tmp_path, baseline, "module.rs"))
