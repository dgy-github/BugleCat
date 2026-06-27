#!/usr/bin/env python3
"""Deterministic unit tests for TaskGen's self-validation gate (no model calls).

These pin the correctness property that matters: a task is admitted ONLY if its
reference passes the check (deterministically) AND its starting state fails it.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import taskgen as tg  # noqa: E402

# A well-formed task: create solution.py with double(x); seed is empty so the
# starting state fails (no module), reference provides a correct module.
GOOD = {
    "name": "double_it",
    "prompt": "Create solution.py with double(x) returning 2*x.",
    "check": "import solution\nassert solution.double(3)==6\nassert solution.double(0)==0\nprint('ok')\n",
    "seed": {},
    "reference": {"solution.py": "def double(x):\n    return 2*x\n"},
}


def test_good_task_is_admitted():
    ok, name = tg.validate(GOOD)
    assert ok, name
    assert name == "gen_double_it"


def test_reference_must_pass():
    bad = dict(GOOD, reference={"solution.py": "def double(x):\n    return x  # wrong\n"})
    ok, why = tg.validate(bad)
    assert not ok and "reference does not pass" in why, why


def test_seed_already_passing_is_rejected():
    # seed already contains a correct solution -> task is trivial (no work to do).
    trivial = dict(GOOD, seed={"solution.py": "def double(x):\n    return 2*x\n"})
    ok, why = tg.validate(trivial)
    assert not ok and "already passes" in why, why


def test_nondeterministic_check_is_rejected():
    # Deterministically exercise the determinism guard: the reference passes the
    # first run then fails the second (simulating a flaky check). Monkeypatch
    # _run_check so the test itself is not flaky.
    seq = iter([(True, ""), (False, "")])  # ref run 1 ok, ref run 2 not ok

    def fake_run(check_src, files, timeout=60):
        try:
            return next(seq)
        except StopIteration:
            return (False, "")
    orig = tg._run_check
    tg._run_check = fake_run
    try:
        ok, why = tg.validate(GOOD)
        assert not ok and "non-deterministic" in why, why
    finally:
        tg._run_check = orig


def test_missing_fields_rejected():
    ok, why = tg.validate({"name": "x", "prompt": "p"})  # no check/reference
    assert not ok, why


def test_parse_extracts_json_fence():
    resp = 'prose\n```json\n{"name":"a","prompt":"p","check":"c","reference":{"f":"x"}}\n```\nmore'
    d = tg._parse(resp)
    assert d and d["name"] == "a" and d["seed"] == {}


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"ok   {fn.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"FAIL {fn.__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    raise SystemExit(1 if failed else 0)
