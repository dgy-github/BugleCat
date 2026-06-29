---
description: Extract + dedupe candidates from the latest snapshot and triage them into eval proposals. Loads the eval-triage skill.
---

Turn the latest production snapshot into deduped candidates and triaged proposals.

First, load the `eval-triage` skill (call the `skill` tool with `eval-triage`) and
follow its method exactly — it defines the dedupe key, clustering, root-cause labels,
risk grades, the promotable gate-preview, and the output schema.

Then:

1. `DATE=YYYYMMDD`. Read `data/prod_snapshots/<DATE>/` (the latest snapshot). If it is
   missing, run `/pull-snapshot` first.
2. **Extract candidates** from `{{SOURCE_TABLES}}`: failed / low-confidence / flagged
   rows. Keep raw request, intent, confidence, error_code, response summary, trace_id,
   timestamp, version.
3. **Dedupe** by the stable key (`{{DEDUPE_KEY}}`, normalized per the skill). Drop keys
   already present in earlier `eval/candidates/*.jsonl` or in `{{HARD_CASES_FILE}}`.
   Write the survivors to `eval/candidates/<DATE>.jsonl`.
4. **Triage** each cluster (cause, confidence, risk, promotable, action) per the skill
   schema. Write `eval/proposals/<DATE>.jsonl`.

Hard rules:
- Append-only; never overwrite a prior day's file.
- Do NOT touch `{{HARD_CASES_FILE}}`.
- When you cannot state an explicit expected result, set `expected:null`,
  `promotable:false`, `action:"report_only"` — never invent a correct answer.

Report: candidate count (after dedupe), proposal counts by action, and the top 3
clusters by size.
