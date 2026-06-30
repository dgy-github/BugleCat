---
description: Run the project's full eval as the promotion gate; report pass/fail with the real output.
---

Run the project's full evaluation — the gate that protects the baseline. Observe the
real result; do not assume.

1. Run: `{{EVAL_CMD}}`  (optionally scope to `$ARGUMENTS` if the harness supports it).
2. Report the actual command output: total / passed / failed, and the failing case
   names. Show the real stderr/stdout on failure — do not summarize away the error.
3. Verdict: `EVAL PASS` (exit 0, no regressions) or `EVAL FAIL` (with the blocking
   cases). On FAIL, nothing may be promoted.

This command only reports; it does not change the eval set or any data.
