---
description: Write the daily schedule-health report from today's candidates/proposals.
---

Write `reports/schedule_health/<DATE>.md` (`DATE=YYYYMMDD`) from today's
`eval/candidates/<DATE>.jsonl` and `eval/proposals/<DATE>.jsonl`. Use
`reports/schedule_health.template.md` as the skeleton.

Fill it with real numbers (count from the files — do not estimate):

- new failures, new candidates (after dedupe)
- proposals: pending / promotable / report-only / ignored
- top error reasons (by cluster size)
- top intent regressions (clusters that spiked vs prior days)
- data quality: new issues / fixed / remaining (e.g. `misplaced_duration` counts,
  split by high-confidence vs medium)
- **recommended actions**: the few highest-value next steps, each tagged
  `auto-safe` (high-confidence, batch-appliable) or `needs-human`.

Keep it short and operational — a morning glance, not an essay. Do not modify the
eval set or apply any fix; this is reporting only. End by printing the report path.
