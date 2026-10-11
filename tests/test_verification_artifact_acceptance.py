import pytest

from evals.verification.artifact_acceptance import request
from evals.verification.benchmark_review import audit_candidate


def task():
    return {"task_id": 1, "prompt": "Return twice n", "code": "def f(n): return n * 2 # REFERENCE_SECRET",
            "test_list": ["assert f(2) == 4", "assert f(-2) == -4"], "test_imports": []}


def test_code_hypothesis_is_separate_from_model_claim_and_hides_reference_label():
    body = request(task(), ["def f(n):"], "def f(n): return n + n")
    assert set(body["questions"]) == {"acceptance_violation"}
    assert body["state"]["statement_origin"] == "host_artifact_hypothesis"
    assert body["state"]["scope"] == "frozen_acceptance_assertions_only"
    assert "REFERENCE_SECRET" not in str(body)
    assert not {"label", "oracle", "result", "claim"} & body["state"].keys()


@pytest.mark.parametrize("code,status", [("def f(n): return n * 2", "passed"),
                                        ("def f(n): return n * 3", "failed"),
                                        ("def f(n): raise ValueError()", "failed")])
def test_independent_acceptance_not_natural_language_certification(code, status):
    result = audit_candidate(task(), f"```python\n{code}\n```\nI have not executed tests.")
    assert result["result"]["status"] == status
    assert result["independent_of_jev"] is True and result["paid_attempts"] == 0


def test_ambiguous_code_does_not_become_negative_hypothesis():
    result = audit_candidate(task(), "```python\npass\n```\n```python\npass\n```")
    assert result["result"]["status"] == "unknown"
