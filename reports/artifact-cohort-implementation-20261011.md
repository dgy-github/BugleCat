# Artifact collection preregistration — 2026-10-11

## Implemented scope

GPT-6.1-Sol implemented only `evals/verification/artifact_cohort.py` and `tests/test_verification_artifact_cohort.py`. This stage made **0 network requests, 0 paid calls, and 0 reservations**. It did not rebuild or change any Rust runner or sealed release, modify old development data, open holdout predictions, change production flags, or edit HANDOFF.

The new `buglecat.verification.artifact-cohort.v1` artifact is a **collection preregistration**, distinct from the existing `artifact-source-inventory.v1` read-only inventory. It binds source/catalog/protocol/authorization raw hashes; both actual Oracle source files; registry/cohort source files; and existing reservation files. SQLite starts are snapshotted logically using a read transaction, including WAL-visible records. A plain database-file hash cannot replace this logical snapshot.

Every already-started main group and every durable main/Jev reservation group is excluded, including failed, pending, interrupted and canceled attempts. A main reservation overlapping its own started group consumes one main slot. `.started` markers do not count as extra Jev reservations. Reservation filenames, unique indices/groups, authorization and request hashes are validated; an active reservation transaction blocks preregistration. Bound files and ledgers are read again before returning the artifact. Consumers must still atomically reserve and revalidate before a later paid send; this module is not a spending lock.

The protocol is checked against the unchanged single-NOUL artifact question/model/versions/grid and 150-supported/50-unsupported quotas for each phase, 95% confidence, 2% false-rejection upper limit, and 50% error-rejection lower limit. Necessary source/budget gate: at least 200 fresh groups per phase and 400 remaining main and Jev slots. This does not guarantee negative examples or successful responses. Only calibration cases can be emitted; no holdout case, label, prediction or evaluation is emitted. Exposure flags refer to future model candidates, not familiarity with public tasks.

Default formal mode accepts only the existing byte-pinned official MBPP source/commit. Arbitrary authored sources cannot obtain formal registration by changing their catalog metadata. Additional external sources require a separate audited intake extension. Explicit `--fixture` exercises code guards with synthetic inputs and always sets `formal_registration=false`, `collection_authorized=false`, `status=fixture_only` on a passing source gate. Fixtures never count as independent labeled benchmark samples.

## Interface

```powershell
python -m evals.verification.artifact_cohort `
  --source evals/verification/data/mbpp/sanitized-mbpp-f82046ba5aabbbb427dbfd38a254d26bff08b533.json `
  --catalog evals/verification/reports/external_mbpp_catalog_20261010.json `
  --authorization evals/verification/reports/jev_budget_authorization_20261010_400.json `
  --protocol evals/verification/reports/artifact_protocol_20261011_v1.json `
  --cohort-id artifact-formal-source-check-20261011-v1 `
  --output reports/artifact-cohort-rejection-20261011.json
```

`register(source, catalog_file, authorization, protocol, cohort_id, output, fixture=False)` exclusively creates and fsyncs a new file. `preregister(...)` returns the same sealed result without writing. A rejected campaign produces `status=rejected`, `cases=[]`, `full_campaign_source_gate=false`, `formal_registration=false`, `collection_authorized=false`. CLI returns 2 for this refusal, 0 for a valid formal registration or passing explicit code fixture. Outputs are never overwritten. There is no live/API/holdout switch.

Cases contain `id`, `group_id`, `task_id`, `split=calibration`, raw `source_sha256`, canonical `task_sha256`, `public_interface` and its canonical hash, category `artifact_acceptance`, `label=unlabeled`, source kind, candidate exposure and `main_model_candidate=not_collected`. They contain no reference body, acceptance tests, generated candidate, label result or probability. Binding rows use `role/file/sha256`; the outer `artifact_hash` is the existing Python canonical digest excluding that field.

A future formal collector must record and verify **before HTTP** the immutable cohort file path, raw file SHA256 (`artifact_cohort_sha256`), canonical seal (`artifact_cohort_hash`) and exact matching group/case in both its durable SQLite start and original transport receipt. Existing development starts have no such binding and cannot be promoted by changing files afterward. This future collector integration is not implemented in this stage.

## Actual zero-fee run

The command above created `reports/artifact-cohort-rejection-20261011.json` exclusively:

- Fresh calibration groups **194**, holdout source groups **207**.
- Remaining main attempts **388**, remaining Jev reservations **399** under the original unchanged cap400 authorization.
- Blockers: insufficient remaining main attempts, insufficient remaining Jev reservations, insufficient fresh calibration source groups.
- Cases **0**, registration false, holdout not opened, threshold null, paid calls **0**.
- Canonical artifact seal: `14f3fbdce728de9c225a895768f98088f58c086606d46bca2ab497a27434f4b1`.
- Raw receipt SHA256: `bbe993c2f903c0a7d805d49ffe4d19fa6a1440ce7830e713df02501182c50cb4`.

The official source AST parser emitted legacy invalid-escape SyntaxWarnings while indexing frozen source strings; it did not execute source code. The rejection JSON was still generated and sealed successfully. PowerShell tool reported nonzero exit; the direct binary CLI regression confirms the refusal return code is 2.

## Verification

```text
python -m pytest -q tests/test_verification_artifact_cohort.py
13 passed in 2.57s
python -m pytest -q tests/test_verification_artifact_cohort.py tests/test_verification_artifact_source_registry.py
15 passed in 2.03s
python -m ruff check evals/verification/artifact_cohort.py tests/test_verification_artifact_cohort.py
All checks passed!
```

Regression coverage includes source grouping/duplicates and fixed split, no holdout emission, exclusive output/no overwrite, pending-start and both-service reservation exclusion, shared slot overlap, synthetic-formal rejection, five weakened protocol gates, Oracle/catalog mutation, concurrent reservation mutation, filename mismatch, transaction lock, and actual CLI refusal with a sealed output. These are implementation fixtures, not evidence of classifier effectiveness.

Existing executable hashes rechecked unchanged:

- artifact sidecar: `82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`.
- original collector: `84f8fb1fa87a689d02a4dee55d775c82e0c2fc8d7322de78eb8ed1227460434c`.
- original native replay: `a7a81112a2acdb51fca6428a6d7e5ec2a44e69ae3d3c2edf09aa3c4f1b755a7c`.

## Subsequent authorization and remaining work

After the actual cap400 run, the parent notified this agent that the user authorized up to **100 additional main and 100 additional Jev attempts**, with the same durable ledgers and unchanged old metadata/start records. The parent owns the new immutable authorization chain resolver; this implementation is deliberately frozen at the observed cap400 contract pending that interface. The rejection above is a historical cap400 result and must not be relabeled as a cap500 result. The source shortage (194 calibration groups) remains independently material; more spending authorization cannot create fresh source groups or labels.

Remaining: integrate the authenticated extension-chain resolver without resetting counts; audit any additional independent source intake; preregister a sufficient fresh campaign; add a separately archived collector with pre-HTTP cohort binding; independently join Oracle/transport records and register labels before reading probabilities; qualify calibration and then evaluate one untouched holdout. No formal threshold, production enablement, guaranteed negative quota, or closed-loop task benefit is claimed.
