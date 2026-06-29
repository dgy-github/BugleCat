---
name: eval-triage
description: Method for turning raw production candidates into a clean, deduped, risk-graded set of eval-case proposals. Use when analyzing prod snapshots, clustering failures, attributing root cause, or deciding what is promotable to the hard-case set.
---

# Eval triage method

You are distilling raw production behaviour into eval-case **proposals**. You never
edit the committed eval set here — you only produce candidates/proposals and a
verdict for each. Be conservative: a noisy baseline is worse than a small one.

## 1. Stable dedupe key

Compute a stable key per candidate from `{{DEDUPE_KEY}}` (typically
`intent + normalized_input + error_code`). Normalize before hashing:

- lowercase, collapse whitespace, strip volatile tokens (ids, timestamps, uuids,
  absolute paths, request-specific numbers that don't change the class of failure).
- Two candidates with the same key are the SAME case — keep one representative
  (the clearest minimal example), record `occurrences` and `first/last_seen`.

Only emit candidates whose key is not already present in today's `candidates/` or in
`{{HARD_CASES_FILE}}`.

## 2. Cluster

Group candidates by failure signature (error_code → intent → message shape). For each
cluster record: size, representative example, intents affected, time span. Big sudden
clusters = likely a regression; long-tail singletons = likely edge cases or noise.

## 3. Attribute root cause (label, don't guess)

Tag each cluster with one primary cause and mark confidence:

- `model_capability` — the model genuinely can't do it (prompt won't fix).
- `harness/prompt` — fixable by skeleton/prompt/tooling change.
- `data_quality` — bad/inconsistent production data (e.g. `misplaced_duration`).
- `contract/spec` — expected output/contract is ambiguous or changed.
- `flaky` — non-deterministic; depends on same-day dynamic data or ordering.
- `infra` — timeout/quota/network, not the agent's logic.

Use `consistent with` / `suggests`, not `proves`, unless you replayed it.

## 4. Risk grade

- `critical` — silent wrong output on a common intent (regression).
- `high` — wrong output, narrower intent.
- `medium` — degraded but recoverable / cosmetic.
- `low` — noise, already-handled, or non-actionable.

## 5. Promotable? (gate preview — record, do not act)

Mark `promotable: true` only if it plausibly meets ALL gate criteria:

1. reproducible, 2. explicit expected result, 3. has a frozen fixture (no same-day
dynamic data), 4. not already covered. Otherwise set `promotable: false` and a reason
(`flaky`, `dup`, `stale`, `needs_human`).

Cases tagged `model_capability` are usually NOT promotable as harness regressions —
flag them separately as capability gaps.

## 6. Output schema (one JSON object per line → `eval/proposals/YYYYMMDD.jsonl`)

```json
{
  "key": "<stable hash>",
  "cluster": "<signature>",
  "intent": "<intent>",
  "example": { "input": "...", "observed": "...", "trace_id": "..." },
  "expected": "<explicit assertion or null if unknown>",
  "cause": "harness/prompt",
  "confidence": "high|medium|low",
  "risk": "critical|high|medium|low",
  "occurrences": 12,
  "first_seen": "...", "last_seen": "...",
  "promotable": true,
  "action": "promote | report_only | ignore_dup | mark_flaky | mark_stale",
  "notes": "..."
}
```

Keep `expected` honest: if you cannot state the correct result precisely, set it to
`null`, `promotable:false`, `action:"report_only"` — a human decides.
