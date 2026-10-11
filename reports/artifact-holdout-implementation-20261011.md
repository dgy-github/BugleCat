# Artifact holdout gate — implementation record

Execution model: GPT-6.1-Sol. Date: 2026-10-11. Workspace: `D:\github_dgy\BugleCat`.

## Completed scope

Added `evals/verification/artifact_holdout.py` and `tests/test_verification_artifact_holdout.py`. This is a zero-network source-registration, Oracle-label-registration and once-only original-observation evaluation gate for the separate artifact acceptance track. It sends no model request and reserves no paid slot.

The gate calls the existing owner's `artifact_formal.revalidate_calibration`. That shared verifier reconstructs the registration and each observation from original transport bytes, validates runner/source bindings, reuses the fixed numerical calibration implementation, and compares the complete sealed result. A state string, old result without observation raw-file binding, or canonical hash alone cannot open this gate. It additionally requires a qualified frozen-grid threshold, no prior holdout and `production_authorized=false`.

Source registration reconstructs the official catalog through the existing registry, copies public interfaces only, uses the original fixed split/seed and group order, and excludes **all** calibration allocation groups, including uncollected and Unknown cases. It also excludes all global started/reserved groups. Previous starts, reservations and metadata must still exist unchanged in the same global ledger. The remaining main and Jev budgets and source pool must each allow at least 200 holdout groups; neither labels nor successful observations are guaranteed by that check.

The new cohort seals raw/canonical calibration, registration and observation bindings, original source/catalog/protocol/authorization/Oracle bindings, a logical global ledger snapshot and the locked threshold. Its `collection_authorized=false` explicitly records that a new independently validated holdout transport entry is still required. Existing main and formal Jev collectors are calibration-only and are not enabled for holdout by this module.

Opening, Oracle-label registration and observation consumption each use `create_new`/flush/fsync markers under the same global main ledger, keyed by the calibration canonical seal. Copying the calibration or choosing another output filename cannot reopen it. Once observation consumption starts, a malformed/missing observation, failure or interruption leaves the marker occupied; there is no retry or threshold adjustment.

Label registration must include every allocated source case, retain Unknown separately, bind eligible supported/unsupported labels to the original Oracle result and original main receipt, and meet 150 supported / 50 unsupported before any holdout Jev registration. Evaluation independently reconstructs every original Jev observation via the shared observer and calls `artifact_gate.score` once at the locked threshold. Failed attempts remain in binary denominators; Unknown source groups remain separately reported. No observation becomes Verified Pass. The result always leaves `certified_threshold=null`, native certification false and production false; unavailable native A/B baselines are explicitly unavailable.

## CLI

All commands below are offline. Output files must be new.

```powershell
python -m evals.verification.artifact_holdout register-source --calibration <qualified-original-calibration.json> --cohort-id <new-id> --output <new-holdout-cohort.json>
python -m evals.verification.artifact_holdout validate --cohort <new-holdout-cohort.json>
python -m evals.verification.artifact_holdout register-labels --cohort <new-holdout-cohort.json> --dataset <original-holdout-dataset.json> --prepared <original-holdout-prepared.jsonl> --output <new-label-registration.json>
python -m evals.verification.artifact_holdout consume --registration <new-label-registration.json> --observations <original-holdout-observations.jsonl> --output <new-evaluation.json>
```

No real qualified calibration exists in this work, so **no real holdout was opened**, no real labels registered and no real probabilities consumed. The successful test paths explicitly replace the previously verified provenance boundary with code fixtures. They test the gate and irreversible markers; they are not source certification, independent model samples or a paid end-to-end result.

## Verification

Commands actually run:

```powershell
python -m pytest tests/test_verification_artifact_holdout.py tests/test_verification_artifact_gate.py tests/test_verification_artifact_formal_intake.py -q
python -m ruff check evals/verification/artifact_holdout.py tests/test_verification_artifact_holdout.py
python -m evals.verification.artifact_holdout --help
Get-FileHash rust/target/debug/examples/verification_artifact_collect.exe,rust/target/debug/examples/verification_artifact_formal.exe,rust/target/debug/examples/verification_artifact.exe,rust/target/debug/examples/verification_collect.exe,rust/target/debug/examples/verification_replay.exe
```

Results: **26 passed in 0.58s** (19 new gate tests plus 7 shared gate/intake regressions); Ruff passed; actual module CLI help returned exit 0. Direct cases cover raw-evidence absence, nonqualified/foreign threshold, fixture rejection, full allocation disjointness, prior-start/reservation exclusion, source order, input-copy/output-alias reopening, shared root and metadata/counter reset, label quota rejection before probability registration, Unknown preservation, once-only success/failure consumption and failed-attempt denominators.

New module SHA256: `b3721705d830ed138c5c363b718cad428c423b5d7859685a2904fe3985acc900`.

New test SHA256: `1c6258bacc3133a4efc21f1541b7f907551b1be822fdaa5f993e631375efc1a2`.

The five actual frozen example executables remain unchanged:

- Formal main collector: `b35f1f21d4c4aaaedd781bcf4e62fb2b32f6262976bf8016a3c03ddd533f9ef8`.
- Formal Jev: `a1d68d38582ff2a1025835cbbe42f35a319db93c2665b81ff934ca471c54e728`.
- Development artifact Jev: `82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`.
- Legacy native main collector: `84f8fb1fa87a689d02a4dee55d775c82e0c2fc8d7322de78eb8ed1227460434c`.
- Legacy native Jev replay: `a7a81112a2acdb51fca6428a6d7e5ec2a44e69ae3d3c2edf09aa3c4f1b755a7c`.

This child made **0 HTTP requests, 0 paid calls, 0 real reservations/starts and 0 actual holdout openings**. No package, production setting, old collector/sidecar source, historical receipt or root HANDOFF was modified.

## Current collection limitation and next boundary

Parent reports that the stopped original formal ultra/8192 collection started 23 new main groups, with 18 valid responses; the independent combined record is 14 supported / 5 unsupported / 5 Unknown, including an unstarted case. Global main starts are 35, leaving 465 of 500; Jev has one original development reservation, leaving 499. These parent-reported results are not produced by this child and are not a qualified calibration. Four resource-invalid raw responses have `finish_reason=length`, completion/reasoning tokens 8192 and empty content. Old started groups cannot be retried and old ultra data must not be folded into a new formal profile's denominator.

A separate high/16384/120 profile can reuse the original Python preregistration owner through a new wrapper that binds a frozen profile and replaces only the three settings in a new seal. It can preallocate at most 265 fresh calibration groups while reserving 200 main/Jev groups for holdout under the present budget. This provides at most 65 calibration groups beyond the minimum 200 binary labels and no guaranteed holdout slack. Failed/Unknown groups and an unfavorable natural positive/negative distribution can still prevent certification; budgets cannot promise 400 qualified observations.

The frozen main source and source validator both hardcode ultra/8192/60, and the frozen formal Jev validator reuses that validator. Therefore a new profile requires an independent main entry and a subsequent independent formal Jev entry, with a shared parameterized source/profile validator for the new entries. Existing provider request-builder bytes, original prompts and five frozen binaries must remain intact. No new profile or transport entry has been implemented or paid under this report.
