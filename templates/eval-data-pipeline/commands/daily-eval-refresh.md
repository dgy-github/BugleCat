---
description: Daily cron entrypoint — pull prod snapshot, analyze candidates, write the health report. Read-only wrt the eval set.
---

Run the daily eval-data refresh, end to end. This is the scheduled entrypoint
(`ncx -p "/daily-eval-refresh"`). It must be safe to run unattended and idempotent.

Do these steps in order, stopping with a clear report if any step fails:

1. Determine today's date as `YYYYMMDD` (run `date +%Y%m%d`). Use it for all paths.
2. **Pull**: do what `/pull-snapshot` describes — fetch the nightly snapshot into
   `data/prod_snapshots/<DATE>/`. If the snapshot is missing or empty, stop and say so.
3. **Analyze**: do what `/analyze-candidates` describes — load the `eval-triage` skill,
   extract + dedupe candidates → `eval/candidates/<DATE>.jsonl`, triage →
   `eval/proposals/<DATE>.jsonl`.
4. **Report**: do what `/daily-report` describes — write
   `reports/schedule_health/<DATE>.md`.

Hard rules:
- Do NOT modify `{{HARD_CASES_FILE}}` here. Promotion is a separate, human-triggered
  step (`/promote-proposals`).
- Do NOT apply any production data fix here. Only report counts and recommendations.
- Append-only for candidates/proposals — never overwrite a prior day's file.

End with a 5-line summary: snapshot rows, new candidates, new proposals (promotable /
report-only / ignored), top error reason, and the single recommended next action.
