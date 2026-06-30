#!/usr/bin/env python3
"""Demo eval gate — stand-in for the project's real `{{EVAL_CMD}}`.

A real gate runs the system under test against every hard case and asserts the
output. Here (self-contained, no SUT) we enforce the structural contract every
promotable hard case must satisfy, so the gate is meaningful for the demo:

  - each case has a non-empty intent / input / expected
  - no two cases collapse to the same stable key (no silent duplicates)

Exit 0 = EVAL PASS, non-zero = EVAL FAIL. Swap this file for your real eval.
"""
from __future__ import annotations

import sys
from pathlib import Path

from run_pipeline import CONFIG, stable_key

ROOT = Path(__file__).resolve().parent


def main() -> int:
    import json

    cases = json.loads(CONFIG["HARD_CASES_FILE"].read_text(encoding="utf-8"))
    failures: list[str] = []
    seen: dict[str, int] = {}

    for i, c in enumerate(cases):
        for field in ("intent", "input", "expected"):
            if not str(c.get(field, "")).strip():
                failures.append(f"case#{i}: missing/empty {field}")
        k = stable_key(c)
        if k in seen:
            failures.append(f"case#{i}: duplicate of case#{seen[k]} (key {k})")
        else:
            seen[k] = i

    total = len(cases)
    if failures:
        print(f"EVAL FAIL - {len(failures)}/{total} problem(s):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print(f"EVAL PASS - {total}/{total} hard cases well-formed and unique.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
