"""Independent source/Oracle fixtures; never real model or certification data."""
import json
import subprocess
from pathlib import Path

import pytest

from evals.verification import humaneval_source as source
from evals.verification.humaneval_oracle_worker import run


def row(identity=0):
    return {"task_id": f"HumanEval/{identity}", "entry_point": "double",
            "prompt": "from typing import List\ndef double(items: List[int]) -> List[int]:\n",
            "canonical_solution": "    return [item * 2 for item in items]\n",
            "test": "def check(candidate):\n    assert candidate([1, 2]) == [2, 4]\n    assert candidate([]) == []\n"}


def payload(task, code):
    return {"reference": task["code"], "imports": task["test_imports"], "tests": task["test_list"], "candidate": code}


def test_original_reference_and_complete_check_remain_exact():
    original = row()
    adapted = source.adapt(original)
    assert adapted["code"] == original["prompt"] + original["canonical_solution"]
    assert adapted["test_list"] == [original["test"], "check(double)"]
    assert adapted["task_id"] == 1_000_000 and adapted["source_task_id"] == "HumanEval/0"
    assert run(payload(adapted, adapted["code"]))["status"] == "passed"


def test_second_original_assertion_is_not_dropped():
    task = source.adapt(row())
    bad = "def double(items):\n    return [2, 4]\n"
    result = run(payload(task, bad))
    assert result["status"] == "failed" and result["error_kind"] == "AssertionError"
    assert result["reference"]["status"] == "passed"


@pytest.mark.parametrize("code", ["from os import system\ndef double(items):\n    return items", "from typing import get_type_hints\ndef double(items):\n    return items", "import typing\ndef double(items):\n    return typing.get_type_hints(double)", "def double(items):\n    return eval('1')", "def double(items):\n    return getattr(items, 'copy')()"])
def test_unsupported_input_is_unknown_not_negative(code):
    task = source.adapt(row())
    assert run(payload(task, code))["status"] == "unknown"


def test_reference_failure_kept_unknown_not_candidate_negative():
    task = source.adapt(row())
    task["code"] = "def double(items):\n    return items\n"
    result = run(payload(task, "def double(items):\n    return [2*x for x in items]\n"))
    assert result["status"] == "unknown" and result["reason"] == "frozen_reference_does_not_pass"


def test_syntax_failure_is_binary_only_after_reference_pass():
    task = source.adapt(row())
    result = run(payload(task, "def broken(:"))
    assert result["status"] == "failed" and result["error_kind"] == "SyntaxError"
    assert result["reference"]["status"] == "passed"


def test_no_source_split_or_label_fabricated_and_duplicates_grouped():
    tasks, index = source.inventory([row(0), row(1)])
    assert len(tasks) == 2 and index["source_group_count"] == 1
    assert all(case["reserved_split"] == "unassigned_new_source" and case["candidate_label"] == "unlabeled"
               and not case["reference_executed"] for case in index["tasks"])
    assert not index["collection_preregistered"] and not index["production_authorized"]


def test_mbpp_normalized_reference_match_is_excluded_not_new_sample():
    adapted = source.adapt(row())
    _, index = source.inventory([row()], [{"code": adapted["code"]}])
    assert index["tasks"][0]["source_admission"] == "excluded_duplicate"


def test_annotations_docstrings_do_not_create_cross_suite_duplicate_samples():
    mbpp = {"code": "def renamed(values):\n    return [value * 2 for value in values]\n"}
    _, index = source.inventory([row()], [mbpp])
    assert index["tasks"][0]["source_admission"] == "excluded_duplicate"
    # Presentation-only AST is never used to alter the frozen runnable source.
    assert source.adapt(row())["code"].startswith("from typing import List")


def test_duplicate_source_ids_and_missing_complete_check_rejected():
    with pytest.raises(ValueError):
        source.inventory([row(), row()])
    modified = row()
    modified["test"] = "assert True"
    with pytest.raises(ValueError):
        source.adapt(modified)


def test_actual_isolated_child_typing_and_complete_tests():
    task = source.adapt(row())
    receipt = source.audit(task, task["code"])
    assert receipt["result"]["status"] == "passed"
    assert receipt["paid_attempts"] == 0 and receipt["secrets_removed"]
    assert len(receipt["oracle_bindings"]) == 2
    assert receipt["original_test_sha256"] == task["original_test_sha256"]


def test_child_env_is_credential_free_and_timeout_retains_unknown(monkeypatch):
    monkeypatch.setenv("DEEPSEEK_API_KEY", "fixture-secret-not-real")
    monkeypatch.setenv("NCX_JEV_API_KEY", "fixture-secret-not-real")
    calls = []
    def fake(args, **kwargs):
        calls.append(kwargs)
        assert "DEEPSEEK_API_KEY" not in kwargs["env"] and "NCX_JEV_API_KEY" not in kwargs["env"]
        raise subprocess.TimeoutExpired(args, kwargs["timeout"])
    monkeypatch.setattr(source.subprocess, "run", fake)
    task = source.adapt(row())
    result = source.audit(task, task["code"], timeout=3)
    assert result["result"]["status"] == "unknown" and calls[0]["timeout"] == 3


def test_changed_or_foreign_official_manifest_rejected(tmp_path):
    file = tmp_path / "manifest.json"
    file.write_text(json.dumps({"commit": "0" * 40, "files": {}}))
    with pytest.raises(ValueError):
        source.official_rows(file)


def test_source_adapter_never_writes_old_mbpp_file(tmp_path):
    file = tmp_path / "old.json"
    file.write_text(json.dumps([{"code": source.adapt(row())["code"]}]))
    before = file.read_bytes()
    source.inventory([row()], json.loads(file.read_text()))
    assert file.read_bytes() == before
    assert Path(source.__file__).name == "humaneval_source.py"
