# Artifact cohort: authenticated cap500 extension — 2026-10-11

## Change and scope

Following the user's additional authorization of up to100 main and100 Jev attempts, GPT-6.1-Sol adapted `evals/verification/artifact_cohort.py` to the parent-owned `budget_authorization.resolve` contract. This stage changed only that module, its directly owned tests, and this new report/output. **Network requests, paid calls, reservations and collection starts: all0.** No Rust source, executable, package, HANDOFF, old authorization, ledger metadata, old start, old reservation or historical cap400 report was rewritten.

The resolver binds the immutable original authorization and the new extension, rejects changed previous-file SHA256, changed ledger roots, reset counts, retries or unauthorized caps. Cohort registration now reads its current per-service cap, permitted original/current authorization hashes, original ledger metadata hash and both authorization-file bindings. Existing SQLite metadata and reservation authorization metadata must retain the **original** hash. Old and new starts/reservations may carry their own allowed hash and share the same cumulative ledgers and group exclusions. A record carrying an original cap400 hash cannot retroactively claim an ordinal above400; records under the extension may use the current cap500. Current logical snapshots include cap values and the complete authorization chain.

The full-campaign minimum remains400 fresh possible observations (200 per phase); per-phase150-supported/50-unsupported quotas and confidence/error gates are unchanged. Additional authorization changes available budget, not independent source counts, labels or guarantees of negative examples. Formal default mode remains limited to the audited, raw-SHA-pinned official MBPP source. Additional independent sources require a separate audited intake extension. No holdout cases/predictions are emitted and no threshold is certified.

## Actual zero-network refusal

```powershell
python -m evals.verification.artifact_cohort `
  --source evals/verification/data/mbpp/sanitized-mbpp-f82046ba5aabbbb427dbfd38a254d26bff08b533.json `
  --catalog evals/verification/reports/external_mbpp_catalog_20261010.json `
  --authorization evals/verification/reports/jev_budget_extension_20261011_100.json `
  --protocol evals/verification/reports/artifact_protocol_20261011_v1.json `
  --cohort-id artifact-formal-source-check-20261011-cap500-v1 `
  --output reports/artifact-cohort-rejection-20261011-cap500.json
```

The command exclusively created and fsynced a **new** refusal artifact; CLI refusal code2. The source AST emitted its existing invalid-escape SyntaxWarnings, without executing code.

- Fresh calibration194, holdout source groups207.
- Main cap500,12 consumed → **488 remaining**.
- Jev cap500,1 consumed → **499 remaining**.
- Sole blocker: `insufficient_fresh_calibration_source_groups`.
- Cases0; `full_campaign_source_gate=false`, `formal_registration=false`, `collection_authorized=false`, holdout not opened, certified threshold null.
- New raw artifact SHA256: `1af8478e546ac91614bdf9dcd65612acb6b69abc34dcd6fbe221587376e6915b`.
- New canonical seal: `c3173eeacab56046e02699bd0813d1de8d127e74f633783b7e124ce25a181c8c`.
- Original authorization raw SHA256: `d1c5a4ccf695dbccc1777af5123441bbd19aebe39da2a4d3e848abf30f6af0f2`.
- Extension raw SHA256: `11749a365bb3943fabccae8bc7eefdcdea685353508ae5427bba4e00ae3200e2`.

The previous cap400 refusal remains `bbe993c2f903c0a7d805d49ffe4d19fa6a1440ce7830e713df02501182c50cb4`; its report and numerical budget results were preserved as historical observations.

## Verification and implementation hashes

```text
python -m pytest -q tests/test_verification_artifact_cohort.py tests/test_verification_artifact_source_registry.py
21 passed in2.61s
python -m ruff check evals/verification/artifact_cohort.py tests/test_verification_artifact_cohort.py
All checks passed!
```

The original13 cohort regressions remain; six added cases verify mixed original/new authorization starts and reservations, cumulative counters without metadata reset, extension root/reset/previous hash/metadata tampering rejection, and prohibition on retroactive original-authorized ordinals above400. Fixtures are purely code regressions, not formal benchmark observations.

- Cohort source SHA256: `0bda17bfc2aed994fe8b16e1851fc0afbbb286625fc75eb7e476ff5b53936094`.
- Test source SHA256: `a5dae59906cdde0f02948cf5fbe1dee998b05ac9989eed0ce20495bbceb1e7d9`.

Three existing actual main binaries rehashed unchanged:

- artifact: `82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`.
- collector: `84f8fb1fa87a689d02a4dee55d775c82e0c2fc8d7322de78eb8ed1227460434c`.
- native replay: `a7a81112a2acdb51fca6428a6d7e5ec2a44e69ae3d3c2edf09aa3c4f1b755a7c`.

The frozen artifact executable still implements its original cap400 live contract. This Python preregistration adaptation does not claim to modify that executable or permit it to consume extension reservations. A future new separately archived collector/sidecar must authenticate the same extension and immutable cohort before HTTP; old paid binaries and source receipts remain preserved. Current source counts still reject a complete formal campaign; no spending follows this preregistration refusal.
