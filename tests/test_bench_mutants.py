"""Bench mutation testing: graders must reject deliberately wrong references."""

import json
import pathlib

from evals.golden import runner

TASKS = pathlib.Path(__file__).resolve().parents[1] / "bench" / "tasks"


def test_apply_mutant_requires_unique_anchor():
    ref = {"a.py": "x = 1\r\ny = 1\r\n"}
    out = runner.apply_mutant(ref, {"file": "a.py", "old": "x = 1", "new": "x = 2"})
    assert out == {"a.py": "x = 2\ny = 1\n"}  # CRLF-normalised, single replacement
    assert ref["a.py"].startswith("x = 1")  # reference untouched
    assert runner.apply_mutant(ref, {"file": "a.py", "old": "= 1", "new": "= 2"}) is None  # ambiguous
    assert runner.apply_mutant(ref, {"file": "a.py", "old": "zzz", "new": ""}) is None  # absent
    assert runner.apply_mutant(ref, {"file": "b.py", "old": "x", "new": "y"}) is None  # wrong file
    assert runner.apply_mutant(ref, {"file": "a.py", "old": "", "new": "y"}) is None  # empty anchor


def test_every_task_has_mutants_with_rationale():
    for task in sorted(p for p in TASKS.iterdir() if p.is_dir()):
        path = task / "reference" / "mutants.json"
        assert path.exists(), task.name
        mutants = json.loads(path.read_text(encoding="utf-8"))["mutants"]
        assert mutants, task.name
        for m in mutants:
            assert m["id"] and m["why"], (task.name, m)


def test_same_length_mutant_is_not_masked_by_stale_bytecode(tmp_path):
    # Regression: .pyc is validated by mtime+size only, so a same-length edit
    # in the same second used to reuse the reference's bytecode and "survive".
    check = tmp_path / "check_src.py"
    check.write_text("import m\nassert m.v == 1\n", encoding="utf-8")
    ws = tmp_path / "ws"
    ws.mkdir()
    (ws / "m.py").write_text("v = 1\n", encoding="utf-8")
    assert runner._run_check_in(ws, check)[0] is True
    (ws / "m.py").write_text("v = 2\n", encoding="utf-8")  # same length
    assert runner._run_check_in(ws, check)[0] is False
