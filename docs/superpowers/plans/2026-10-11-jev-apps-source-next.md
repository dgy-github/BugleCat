# APPS source preflight — 2026-10-11

## Superseding implementation status

The full pinned train has now been acquired once and verified: 107101272 bytes, full SHA256 `45e82ef22ed8e7c0c04d881a21b923e9dd233157896b0b8d5b3493e887499cae`. All 5000 original rows were inventoried; 3062 call-based, 1742 STDIN, 196 unavailable IO. New `apps_v3/` owners implement the explicitly restricted top-level-function comparison/execution contract, with 53 passing local regressions and independent review. All old frozen owners and executables remain unchanged.

Root's zero-model screening executed the first original reference exactly once for each of 2004 structurally eligible tasks: 1972 passed, 19 failed, 13 Unknown. The complete original source/receipt join was rechecked without re-execution. These are reference results, not model labels or independently admitted groups. Historical source exclusion, duplicate grouping, new protocol/template/resource freeze and paid intake remain required. No new model/Jev/reservation/split was created; current main510/750 and phase Jev222/750 are unchanged. Test download remains deferred.

See [implemented Oracle and remaining gates](2026-10-11-jev-apps-oracle.md). The following sections preserve the earlier static-preflight evidence and proposal; their acquisition/execution counts apply to that historical preflight only.

## Actual status

This is source inventory and static contract review, not collection preregistration or source admission. New model calls, Jev calls, reservations, remote-code execution, reference execution, candidate execution and dependency installations: **0**. No existing source/Oracle/EXE/cohort/split was changed. All new material lives beside this report.

Official repository commit: `362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5`. Official README links the `codeparrot/apps` mirror, pinned separately to `21e74ddf8de1a21436da12e3e653065c5213e9d1`. Mirror metadata is a second provenance chain; it is not asserted byte-equivalent to the original archive.

Only small repo source, mirror metadata/builder/card, and an exact HTTP 206 range of 65,536 bytes of pinned train JSONL were read. The partial range contains five complete rows (IDs 0–4), all STDIN (`fn_name` absent), and an incomplete trailing row that was not interpreted. These five rows prove format only, not random population coverage or function-subset availability. Full LFS object hashes are metadata, **not verified downloaded object hashes**. No shard or original archive was downloaded in full.

## Immutable official evidence

- Repo README: <https://github.com/hendrycks/apps/blob/362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5/README.md>.
- Evaluator: <https://github.com/hendrycks/apps/blob/362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5/eval/testing_util.py>.
- Evaluation driver: <https://github.com/hendrycks/apps/blob/362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5/eval/test_one_solution.py>.
- MIT repo license: <https://github.com/hendrycks/apps/blob/362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5/LICENSE>.
- Mirror card: <https://huggingface.co/datasets/codeparrot/apps/blob/21e74ddf8de1a21436da12e3e653065c5213e9d1/README.md>.
- Mirror builder: <https://huggingface.co/datasets/codeparrot/apps/blob/21e74ddf8de1a21436da12e3e653065c5213e9d1/apps.py>.

`repo-download.json`, `mirror-download.json`, `file-manifest.json` retain request URL, revision, raw SHA256, sizes and server metadata. `format-probe.json` records the bounded range and only structural row fields; code/text stays in the private raw range file.

## Licensing / data entry

The repo LICENSE is MIT. The mirror card declares MIT for the dataset; the downloaded builder's own source header is Apache-2.0. Preserve both applicable notices if adapting their respective code/data. The dataset is curated from external problem sites, so this preflight does not independently prove each upstream site's redistribution terms.

The repo's original archive is approximately 1.3GB. Pinned mirror tree lists `train.jsonl` at 107,101,272 bytes (LFS SHA256 `45e82ef22ed8e7c0c04d881a21b923e9dd233157896b0b8d5b3493e887499cae`) and `test.jsonl` at 1,292,436,853 bytes (LFS SHA256 `5b003a65ac40feb47dd5eaec267a767a6fc435bdcfa68ff715fe869f948e760c`). These are inputs for a future bounded acquisition design, not artifacts acquired here.

The mirror card describes 5,000 train + 5,000 test problems, but explicitly says few have `fn_name`/starter code, 195 train rows lack test cases, and 1,235 test rows lack reference solutions. Consequently **10,000 is not an admissible function-group count**. Actual sufficient fresh calibration/holdout counts remain unmeasured. The card also warns test coverage can mark incorrect algorithms as correct; future certification must mean frozen acceptance pairs only.

## Exact evaluator semantics (static, pinned)

1. `testing_util.py:140`: `input_output.fn_name == None` selects STDIN; otherwise call-based. No conversion of STDIN to functions is justified.
2. `:153–170`: a broad implicit import prelude is prepended. Literal substring `class Solution` selects construction of one `Solution()` instance; otherwise a module callable is used. This is not an AST-based class detector. Instance/module state persists across all cases.
3. `:224–247`: callable is obtained by `getattr`; numeric-key dict coercion is attempted on the first input and expected output. Some successful input coercions replace the argument list with one dict and may discard other arguments. Expected values are mutated in place. These quirks must be represented explicitly or conservatively excluded, not silently corrected.
4. `:259–273`: invoke `method(*inputs)` in original order. Top-level tuple result becomes list. Accept direct equality, or equality against expected first element when expected is a nonempty list; nested tuple-list conversion has another expected-first-element fallback. This is broader than a simple generated `assert actual == expected`.
5. `:37–38`: Unix SIGALRM, 4 seconds per compile/case. `test_one_solution.py:63–93` adds an outer child deadline and substitutes **21** failed entries when no result arrives. That synthetic denominator must not be copied into honest receipts. Local worker inability/timeout must remain Unknown with original true pair count.
6. Float `numpy.allclose` and whitespace/set/round-to-3 handling appear in the **STDIN** branch (`:396+`), not the call-based return-comparison branch. Do not apply that tolerance to function outputs.
7. `:520`: reliability guard itself says it is not a security sandbox. RuntimeModule, numpy, global SIGALRM and process mutation make importing/running this evaluator unchanged unsuitable for the current Windows restricted worker. Nothing from this remote source was imported or run.

## Existing interface reuse and hard differences

Reusable design: canonical/raw source seals; per-task original public prompt/interface; whole-group de-duplication before new split; credential-stripped isolated child process; actual Python binary/version hash; worker and comparison source SHA; reference-before-candidate admission; durable same shared budget + unique group start; raw main/Jev request-response receipts; all-started dataset join; Unknown/unstarted distinction; independent labels before probabilities; registered grid, 150/50 minimum, one-use qualified holdout.

Not reusable as an APPS Oracle: `benchmark_oracle_worker.py:30` prohibits ClassDef, `:20` restricts getattr and I/O; its module allowlist lacks numpy/pyext/typing/random. `acceptance()` executes Python assert strings, not APPS paired inputs/output alternatives and persistent callable state. Existing `artifact_v2_formal_cohort.py:52,85` only admits mbpp/humaneval. Existing `artifact_v2_formal_intake.py:226` correctly reconstructs the original source-specific dataset; it should not be broadened after paid evidence. Reuse architectural primitives through **new owners**, not mutation of the frozen V2 source graph.

## Concrete next implementation boundary (proposal only)

1. New `apps_source_v3.py`: ingest a fully verified pinned dataset acquisition, preserve raw original question/starter/solutions/inputs/outputs/fn_name, dataset split and problem ID. Reject malformed/empty pairs or unequal lengths. Census explicit free-function vs Solution vs STDIN; no candidate generated during census. Deduplicate normalized references/prompts and original URLs across APPS and all historical pools; retain original bytes, full excluded-group evidence, and source-exposure flags. Never take difficulty as a correctness label.
2. New `apps_oracle_v3_worker.py` + wrapper: initially an explicitly limited **free-function call-based subset**, with no silent class/STDIN rewrite. Compare exact frozen official call-based behavior (including all documented alternatives), execute all original pairs in order with one persistent module. Unsupported imports/constructs/comparison ambiguity or resource exhaustion → Unknown. Implement host callable lookup outside candidate-accessible builtins. Broader class support requires a separately reviewed restricted AST/runtime policy. Do not load the old remote evaluator in a credential-bearing parent.
3. New receipt: `source_kind=apps_call_based`, original task/IO/reference/candidate hashes, comparison contract/version/hash, fn_name/call mode, pair count, runtime/worker/import-policy hashes, reference result and per-pair outcome, actual time/resource scope. Missing reference or a reference failing the frozen pairs → source-only Unknown, never an artificial model negative. Freeze a deterministic reference policy before any model candidate; do not search references after seeing a candidate result.
4. New `artifact_v3_cohort.py`/intake and template: freeze source-only admitted groups, old whole allocations and started/reserved exclusions, independent group split/order, interface/system/user bytes, runtime, all transitive owners, budget before candidate generation. Main receives task and interface only; reference/paired tests remain hidden. Jev can receive frozen pairs plus candidate according to the new acceptance template, but no observed Oracle results/labels. Describe ordered calls, persistent namespace, dict-key/return coercions precisely. Scope is frozen APPS pairs, not whole task/agent success.
5. Before new formal collection, prove at least 200 source-admitted distinct groups per split plus realistic Unknown slack, independent candidate labels and the unchanged statistical gates. Future model attempts needed are **at least 400 main + up to 400 eligible Jev**, not assured by 400 attempts. Current fresh admitted calibration remains 0 in the old source pool; all 269 outside admitted sources are reserved holdout and are not available to refill calibration. This preflight does not change those allocations or grant budget. Recompute actual remaining budget when the current campaign ends.

## Required zero-model regressions before adoption

- Positional unpack of scalar/list/dict args; numeric/nonnumeric dict keys; multiple arguments retained/excluded according to fixed official quirk policy.
- Direct vs expected[0] comparisons; tuple/list/nested tuples; empty outputs; exact call-based float equality (no inherited STDIN allclose).
- Ordered stateful calls, module vs Solution detection, missing callable, compile/runtime failure, safe AST/import constraints and dependency mutation.
- Full true test count on timeout/child failure; credential stripping; worker/runtime/hash tampering; no remote evaluator execution in parent.
- Missing/no reference/no tests → admission Unknown; every original pair preserved; source group leakage and historical allocations excluded; rejection before fees.
- Dataset joins all actual starts/failed/Unknown, distinguishes never-started; original immutable request/response/candidate/source receipts; whole independent label preregistration before Jev.

These are proposed tests, **not passed semantic conformance tests**. Only local static evidence/hash checks were run in this preflight.
