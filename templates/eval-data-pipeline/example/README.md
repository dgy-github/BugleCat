# Runnable minimal closed loop (placeholders filled)

A self-contained, deterministic, LLM-free instantiation of the pipeline — proves the
loop closes and is the reference you adapt to a real project. Stdlib only.

```
python run_pipeline.py --date 20260629 --self-check    # pull -> analyze -> report
python eval.py                                          # the promotion gate
```

## What it demonstrates (on the synthetic snapshot)

`fixtures/prod_snapshot/audit_log.jsonl` = 13 synthetic prod rows. One run produces:

| cluster | occ | cause | action | why |
|---|---|---|---|---|
| OVERLAP_CONSTRAINT | 4 | contract/spec | **promote** | reproducible, explicit `expected` → eval-worthy |
| MISPLACED_DURATION | 2 | data_quality | report_only | → production data-fix channel, not an eval case |
| TIMEOUT | 2 | flaky | mark_flaky | depends on same-day data → needs a fixture first |
| WRONG_RESULT | 1 | model_capability | report_only | capability gap, not a harness regression |

- 4 OVERLAP rows with different job ids / timestamps **dedupe to one** (stable key
  over normalized fields).
- 3 `ok` rows are filtered; the `NEG_SETUP` row is **dropped** because its key is
  already in `eval/online_hard_cases.json` (no re-proposing covered cases).
- The daily loop **never touches** `online_hard_cases.json` (asserted by `--self-check`).

Outputs (gitignored): `data/prod_snapshots/<DATE>/`, `eval/candidates/<DATE>.jsonl`,
`eval/proposals/<DATE>.jsonl`, `reports/schedule_health/<DATE>.md`.

## Wiring to a real project

Edit the `CONFIG` block in `run_pipeline.py` and replace the fixture + `eval.py`:

| CONFIG key | replace with |
|---|---|
| `SNAPSHOT_SOURCE` | your nightly prod export output (`{{SNAPSHOT_EXPORT}}`) |
| `SOURCE_TABLES` | your audit/candidate tables |
| `HARD_CASES_FILE` | your committed eval set |
| `DEDUPE_KEY` | the fields that define "same failure" |
| `eval.py` | your real `{{EVAL_CMD}}` (run the SUT over the hard cases) |

The conventions stay: append-only candidates/proposals, read-only daily loop, the
4-criteria promotion gate, data fixes on a separate channel. The agent-driven `.md`
commands in `../commands/` layer judgement (smart clustering / attribution) on top of
these same mechanical steps.
