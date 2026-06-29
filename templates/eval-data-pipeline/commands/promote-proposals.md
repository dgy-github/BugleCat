---
description: Human-triggered — promote approved proposals into the eval set behind the full gate, as a reviewable repo change. Never auto-merges.
---

Promote approved proposals into `{{HARD_CASES_FILE}}` — only behind the full gate, and
only as a change a human reviews. This is NOT run by the daily cron.

`$ARGUMENTS` = the approved proposal `key`s (space-separated), or `all-promotable` to
take every proposal whose `promotable:true` in the latest `eval/proposals/*.jsonl`.

For each selected proposal, re-verify the gate (refuse the ones that fail):

1. **Reproducible** — replay it; the observed result must match.
2. **Explicit expected** — `expected` is a concrete assertion (not null).
3. **Frozen fixture** — no same-day dynamic data; capture a fixture if needed.
4. Not already in `{{HARD_CASES_FILE}}`.

Then:

5. Add the surviving cases to `{{HARD_CASES_FILE}}` (use `apply_patch`).
6. Run the gate: `{{EVAL_CMD}}`. It must pass WITH the new cases. If it fails or
   regresses, revert the additions and report which case broke it.
7. Do NOT commit/push automatically. Leave the change in the working tree and print:
   the cases added, the eval result, and a suggested commit message. A human reviews
   and commits.

Production data fixes are out of scope here — those go through a separate reviewed
migration, never bundled with an eval-set change.

Report: promoted / rejected (with reason) per key, and the final eval verdict.
