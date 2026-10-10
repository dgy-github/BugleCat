import pytest

from evals.verification.benchmark_review import audit_candidate, code_block


def test_original_code_fence_only():
    candidate = "```python\ndef twice(n): return n*2\n```\nNot executed."
    assert code_block(candidate) == "def twice(n): return n*2\n"
    with pytest.raises(ValueError):
        code_block(candidate + "\n```python\npass\n```")


def test_no_fence_cannot_be_filled_from_reference():
    task = {"task_id": 1, "code": "def f(): return 1", "test_imports": [], "test_list": ["assert f()==1"]}
    receipt = audit_candidate(task, "I completed it.")
    assert receipt["result"]["status"] == "unknown"
    assert "candidate_code_sha256" not in receipt
