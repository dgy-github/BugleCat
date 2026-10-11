# Execution negation repair — 2026-10-10

Implemented by GPT-6.1-Sol. This development stage made zero HTTP, Jev, or paid main-model calls. Original v3.2 paid packets, requests, responses, sessions, ledgers and reports were preserved.

## Reproduction and repair

Before changing behavior, the new `honest_unexecuted_tests_are_negative_with_exact_spans` regression failed against v3.2: `No tests were executed` produced polarity `positive`, expected `negative`. Actual collected candidates use this honest wording and were falsely treated as test-success claims requiring execution evidence.

`verification/extraction.rs` now recognizes the narrow subject-bound form `no [unit|integration] test[s] [were|was] run|executed`, using English word boundaries, as an execution denial. Existing explicit `not run`, `not executed`, `did not run` and Chinese nonexecution phrases remain recognized. The helper is shared by `verification/policy.rs` so extraction and interpretation agree:

- A truthful denial without execution evidence remains Unknown (`honest_unexecuted_limitation`). No semantic score certifies it.
- An explicit denial referencing authoritative actual execution contradicts that execution whether the run succeeded or failed (`denied_observed_execution`). A failed run is still a run; it does not support “no tests were executed”.
- Normal positive test statements remain positive. Existing segment delimiters preserve separate positive clauses in mixed reports; byte spans, references, quoted/code/conditional context remain intact.

The repair does not filter whole responses containing `not` or `no`, does not rewrite the general extractor, and does not fabricate positive semantic claims. Source policy is explicitly **host-rules-v3.3**; template remains `jev-single-claim-v2`. Production defaults remain off, enforce categories empty, experimental threshold absent.

## Changed owners and validation

- `rust/crates/ncx-core/src/verification/extraction.rs`: explicit nonexecution helper and extraction polarity.
- `rust/crates/ncx-core/src/verification/policy.rs`: shared denial interpretation.
- `rust/crates/ncx-core/src/verification/types.rs`: policy v3.2 → v3.3.
- `rust/crates/ncx-core/src/verification/tests.rs`: three meaningful regressions covering authentic honest text, run/executed variants, normal positive/mixed clauses, UTF-8 spans, call references, quote/code/conditional context and contradiction with actual successful/failed execution. Test results also assert the v3.3 result version.

Passed commands:

```powershell
cargo test -p ncx-core --lib verification:: --manifest-path rust/Cargo.toml
# 31 passed, 1 explicitly paid opt-in smoke ignored
cargo test -p ncx-core --lib --manifest-path rust/Cargo.toml
# 415 passed, 1 ignored
cargo clippy -p ncx-core --lib --manifest-path rust/Cargo.toml -- -D warnings
rustfmt --check --edition 2021 rust/crates/ncx-core/src/verification/extraction.rs rust/crates/ncx-core/src/verification/policy.rs rust/crates/ncx-core/src/verification/types.rs rust/crates/ncx-core/src/verification/tests.rs
```

No ignored paid test ran. No collector or replay main executable was rebuilt. The frozen v3.2 collection protocol cannot silently continue under rebuilt v3.3 code: its policy-version binding would reject the mismatch. Any new collection policy must be separately frozen. Old v3.2 receipt interpretation remains explicitly historical rather than relabeled in place.

## Limits and package follow-up

This is a bounded phrase repair, not complete English negation/temporal reasoning. Existing clause segmentation and conservative uncertainty remain; arbitrary prose and unsupported scope can remain Unknown. Three official benchmark assertions still do not certify full algorithm correctness. Independent affirmative positive/negative semantic datasets, 150/50 calibration minima, qualified threshold, one-time holdout and real task-loop benefit remain unachieved; no gate was lowered.

Root subsequently authorized a separate v3.3 CLI/GUI/NSIS package and minimum lifecycle revalidation. `scripts/stage-jev-release.py` now derives the explicit policy version from the frozen `types.rs` owner when checking real runtime logs and writing the new release manifest, instead of assuming v3.2. Existing release directories and manifests are not overwritten. New package hashes and actual runtime evidence will be recorded separately in `reports/release-v33-20261010.md` after completion; package success is not effect certification.
