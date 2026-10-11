# Main collection resource diagnostic — 2026-10-11

## Original ultra diagnostic snapshot

The root coordinator made 23 new main requests across three source-preregistered batches. Eighteen produced valid original code responses. Independent frozen assertions labeled fourteen supported and five unsupported; four started requests returned invalid empty content, and one prepared source group was never started. No Jev probability was read or requested for these labels.

The third batch stopped after two consecutive content failures. All four failures have original `finish_reason=length`, exactly8192 completion tokens and8192 reasoning tokens, with zero final content characters. The original receipts and raw bytes are retained in the shared ledger. No failed group is retried. The diagnostic contains only response metadata, no candidate or reasoning text, and is independently derived from the original responses in `reports/artifact-formal-main-first3-failure-diagnostic-20261011.json`.

The [official DeepSeek thinking documentation](https://api-docs.deepseek.com/guides/thinking_mode/) maps `ultra` to `max`, and `high` to `high`. The original frozen provider request builder already applies this mapping for these two levels. The observed resource failure concerns the8192-token total generation allowance consumed before final content; it is not a Jev classification result.

A distinct proposed profile uses the same source, system/user prompt and acceptance assertions, with `high`,16384 output tokens,120 seconds and zero retries. It requires a new source preregistration and compiled entry; the existing source validators intentionally reject this changed profile. New profile candidates will exclude all35 already-started groups and will not pool earlier ultra answers into their formal denominator. Old cohort, attempts, budgets and executable provenance remain intact. This adjustment is still under zero-network implementation and has not made new requests.

Current actual accounting: main35 started/34 finished/465 requests remaining; Jev1 development attempt/499 remaining. The old pause left one unfinished main request with unknown usage. Caps remain500 per service; no monetary balance was queried. At most265 new calibration requests can be allocated while reserving200 for holdout, with no guarantee of sufficient natural positive/negative labels. Statistical gates remain150 supported/50 unsupported per stage; no threshold is certified, no actual holdout has opened and production remains off.

## Subsequent frozen high profile observation

The independent `deepseek-flash-high-16384-120-v1` profile is now collecting actual requests under its own source preregistration. The earlier implementation and accounting paragraph above is a historical snapshot; current counts are in the top section of HANDOFF and policy_status.

Three subsequent original high-profile content failures (`mbpp-main-374`, `mbpp-main-15`, `mbpp-main-408`) also ended with `finish_reason=length`: all 16384 completion tokens were reasoning tokens, with zero final content bytes. Each raw response SHA matches its original receipt. This confirms that the new output allowance was applied; it does not guarantee every request produces final code. One separate high-profile transport failure has no raw response. These attempts remain in the shared budget and as Unknown source outcomes, without retry or a binary acceptance label. No source, prompt, Oracle, profile parameter or statistical gate was changed after seeing these outcomes.

## Reference admission failure before batch 29

The original 29th batch stopped before any new paid start. Seven references passed; task 180 returned `unknown/frozen_reference_does_not_pass`, with an `AssertionError` after the first assertion. Its second public assertion uses exact floating equality: the original formula returns `6069.3979333005145` on this machine, while the frozen literal is `6069.397933300514`. The difference is one ULP, `9.094947017729282e-13`. The source, formula, runtime, worker and assertions were preserved.

Independent review of the frozen owners distinguishes reference admission from candidate classification. The preregistration records `source_admission_executed=false`; the compiled collector requires the original unpaid prefix, source hashes and fixed settings. Reference success is required for a binary Oracle label, not for an original main response. The ignored Root admission helper added the stronger all-reference-pass check. The frozen worker returns Unknown when the reference fails, and the original intake excludes Unknown from Jev prepared requests and from the 150/50 binary quotas.

Root therefore requires a separate, exact source-only review before collecting this known reference issue in its original queue position. It must bind the original failed admission, task/group, source/cohort/worker hashes and full reference receipt. It cannot mark the reference passed, fix the assertion, skip the group, invent a paid start or classify the issue as a model negative. Any unexpected source failure still stops before fees. The actual model response and its unchanged Oracle outcome must be retained, and no failed HTTP is retried.
