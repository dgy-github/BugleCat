from evals.verification.benchmark_oracle import audit


def task():
    return {"task_id": 1, "code": "def twice(n): return n * 2", "test_imports": [],
            "test_list": ["assert twice(3) == 6", "assert twice(-1) == -2"]}


def test_real_child_assertions_pass_and_fail():
    good = audit(task(), "def twice(n): return n + n")
    bad = audit(task(), "def twice(n): return n * 3")
    assert good["result"]["status"] == "passed"
    assert bad["result"]["status"] == "failed"
    assert bad["result"]["error_kind"] == "AssertionError"
    assert good["independent_of_jev"] and good["secrets_removed"]


def test_unsafe_candidate_and_bad_reference_are_unknown():
    assert audit(task(), "import os\ndef twice(n): return n*2")["result"]["status"] == "unknown"
    changed = {**task(), "code": "def twice(n): return n"}
    assert audit(changed, "def twice(n): return n*2")["result"]["reason"] == "frozen_reference_does_not_pass"


def test_timeout_and_syntax_are_distinct():
    assert audit(task(), "while True: pass", timeout=1)["result"]["status"] == "unknown"
    assert audit(task(), "def !!!")["result"]["error_kind"] == "SyntaxError"


def test_dynamic_private_reflection_is_not_executed():
    unsafe = 'import operator\nf=operator.attrgetter("__globals__")\ndef twice(n): return n*2'
    assert audit(task(), unsafe)["result"]["status"] == "unknown"
    assert audit(task(), "def twice(n):\n _value=n+n\n return _value")["result"]["status"] == "passed"


def test_candidate_cannot_change_expected_assertion_semantics():
    bad = "set=lambda x: True\ndef twice(n): return 123"
    assert audit(task(), bad)["result"]["status"] == "unknown"
    mutated = "import math\nmath.sqrt=lambda n: 1\ndef twice(n): return n*2"
    assert audit(task(), mutated)["result"]["status"] == "unknown"
