# Eval-data continuous-update pipeline (portable nanocodex driver)

A project-agnostic scaffold for the loop:

```
pull-snapshot → analyze-candidates → (proposals) → run-eval (gate) → daily-report
                                          │
                                          └─ promote-proposals (human-gated) → repo patch
```

**Responsibility split (do not blur it):**

| Layer | Does | Does NOT |
|---|---|---|
| **Production** | emit audit/candidate rows; nightly snapshot export; light dedupe | run the agent; mutate the eval set; make "smart" decisions |
| **Local coding agent (nanocodex)** | pull snapshot, analyze, cluster, attribute, **propose**, write reports | auto-merge proposals; auto-apply medium-confidence data fixes |
| **CI / human** | run full eval, merge promoted hard cases, approve data migrations | — |

The agent maintains the eval set as a living **data asset**: every day it distills
real production behaviour into hard cases, squeezes out duplicate noise, and keeps
the genuine regressions — instead of humans topping up cases by gut feel.

## Install (into the TARGET project, e.g. the 排产 repo)

These are templates. Copy them into the project nanocodex runs against:

```
cp -r templates/eval-data-pipeline/commands/*   <project>/.nanocodex/commands/
cp -r templates/eval-data-pipeline/skills/*     <project>/.nanocodex/skills/
cp    templates/eval-data-pipeline/reports/schedule_health.template.md \
                                                <project>/reports/schedule_health.template.md
```

Then fill the `{{PLACEHOLDERS}}` (see below) inside the copied command files. nanocodex
discovers `/<name>` from `<project>/.nanocodex/commands/<name>.md` and loads the
`eval-triage` skill on demand.

## Placeholders to fill (once, per project)

| Placeholder | Meaning |
|---|---|
| `{{SNAPSHOT_EXPORT}}` | command the agent runs to fetch the nightly prod snapshot (SQL dump / S3 pull / HTTP) |
| `{{EVAL_CMD}}` | command that runs the project's **full eval** (the gate), exits non-zero on fail |
| `{{HARD_CASES_FILE}}` | the committed eval set, e.g. `eval/online_hard_cases.json` |
| `{{DEDUPE_KEY}}` | fields that form the stable candidate key, e.g. `intent + normalized_input + error_code` |
| `{{SOURCE_TABLES}}` | source rows, e.g. `audit_log`, `eval_candidate_case`, `quality_task` |

## Directory conventions (created under the target repo)

```
data/prod_snapshots/YYYYMMDD/        # raw pulled snapshot (gitignored)
eval/candidates/YYYYMMDD.jsonl       # extracted, deduped candidates
eval/proposals/YYYYMMDD.jsonl        # triaged proposals (NOT yet in the eval set)
reports/schedule_health/YYYYMMDD.md  # daily health report
{{HARD_CASES_FILE}}                  # the eval set — only changed via promote-proposals + commit
```

Add to the target repo's `.gitignore`: `data/prod_snapshots/`.

## Daily schedule (cron / Windows Task Scheduler)

Drive the agent headless — one shot, then exit. Do **not** keep it resident.

```
ncx -p "/daily-eval-refresh" --workspace <project>
```

`daily-eval-refresh` chains pull → analyze → daily-report. It is **read-only** with
respect to `{{HARD_CASES_FILE}}`: it only writes snapshots, candidates, proposals,
and the report. Promotion is a separate, human-triggered step.

## The promotion gate (why nothing auto-pollutes the baseline)

A proposal becomes a hard case only when ALL hold, and only via a reviewed commit:

1. **Reproducible** — replays to the same outcome.
2. **Expected result is explicit** — a fixed assertion, not "looks wrong".
3. **Main eval passes** (`{{EVAL_CMD}}`) before and after adding it.
4. **No same-day dynamic data** — has a frozen fixture.

Production data fixes follow a separate channel: high-confidence (e.g. the 14.8k
`misplaced_duration`) → batch `migration`; medium and up → report only, human applies.

See `commands/` for each step and `skills/eval-triage/SKILL.md` for the analysis method.
