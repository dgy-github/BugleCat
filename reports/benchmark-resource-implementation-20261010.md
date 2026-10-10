# Resource cohort implementation — 2026-10-10

GPT-6.1-Sol implementation; all commands in this report made **zero network / paid calls**. Root coordinates any subsequent live collection. Production policy remains `host-rules-v3.2`, disabled semantic enforcement; release packages are unchanged.

## Reason and constraint

The original first batch consumed six main-model reservations: three usable responses and three HTTP-200 responses with `finish_reason=length`, empty content, and all 4096 output tokens consumed by reasoning. Those failures remain consumed; they are not retry candidates. Three honest uncertain candidates produced zero eligible semantic claims; no Jev evaluation is justified by them.

The root froze `evals/verification/reports/benchmark_resource_adjustment_20261010.json` (SHA256 `df858bca359a7a4b6186a2a715beb41e135328ad590139d283ecb99bca6b49f2`). The new manifest is `evals/verification/reports/benchmark_collection_batch02_20261010_r2.json` (SHA256 `4fac8815ec6f7448f4fb432bd86db85490deb8c1368c0dbde877402d17c38f7a`). Its eight groups comprise two previously unattempted groups from the original manifest and six other new groups. No attempted group is retried.

## Implementation

Changed only the collector example, its support module, and `tests/test_verification_collect_binary.py`.

- A manifest requesting 8192 tokens must bind the explicit resource adjustment by raw file SHA. No resource file means exactly the original 4096 token cohort.
- The adjustment permits only `max_tokens` 4096 → 8192 and collector rebuild. Model, reasoning, timeout, zero retries, endpoint, source, catalog, protocol, system prompt, host policy, authorization, and ledger remain fixed.
- Old prepared JSONL is checked against its exact raw SHA, per-row seals, original executable SHA, settings hash, authorization, and unchanged system prompt. The stable identity is compared to every new prepared row.
- Existing SQLite `collection_settings_sha256` is never updated or deleted. Live requires its value to equal the declared original binding. It adds `collection_stable_settings_sha256` and `resource_cohort:<resource-file-SHA>` metadata; the latter binds the exact new executable identity once. A later binary change needs another explicit frozen adjustment rather than silent drift.
- The first use of a resource cohort is capped at eight rows. Same SQLite authorization, 400 global cap, 200 calibration cap and UNIQUE group constraint persist. Reservation precedes HTTP, failures consume slots, automatic retries remain zero.
- Prepared rows, started/submitted/finished events and candidate receipts contain `resource_cohort` with cohort id, adjustment SHA and original settings SHA. This allows separate reporting of the resource change.

The resource change does not guarantee sufficiently independent positive/negative cases or valid semantic claims. If this small batch still yields no eligible native scoped affirmative claims, expanding blindly to 400 is unwarranted. Calibration/holdout certification remains unachieved.

## Preserved original executable

`reports/collector-resource-diagnostic-20261010-original/` preserves the original actual main executable and source bytes before rebuild:

- `verification_collect-original.exe`: SHA256 `8a1612e43893deea875bc9a94eb8a7ad1e084f38b912b82753792a49e8627ce9`, 8,855,552 bytes, original UTC mtime `2026-10-10T10:18:02.7146788Z`.
- `verification_collect-original.rs`: SHA256 `0ad55c036256468e9a36e1edbb01cd3767fc9cd93ed302177525dc51e8ae64e1`.
- `support-original.rs`: SHA256 `656cc541eaac8b5aa8d57c47df226aa093563109613212953c6aa5a3b5bad371`.

Original prepared raw SHA remains `ad29af09076e27bbf21099a497ec0cc2d81e87bc6fbd9bbb991e831bdc901407` and original ledger settings SHA remains `bcf73bf3cc8c8d8d2681cb7a7dde3eed6f6ebf1acc6f7a289215d286e980c1dc`.

## Actual new main entry and preparation

- Executable: `rust/target/debug/examples/verification_collect.exe`, SHA256 `84f8fb1fa87a689d02a4dee55d775c82e0c2fc8d7322de78eb8ed1227460434c`, 8,935,936 bytes, UTC mtime `2026-10-10T10:27:26.8397702Z`.
- Runner source SHA256 `a7907ed7568a8190612963d6d99602f587880ff02cf4245df32dcb7da4ae326e`.
- Support source SHA256 `02584f55383f898836aaf925b8022cf540937f34309a828fb95192eeed850a2d`.
- Exact new executable and source copies are archived at `reports/collector-resource-diagnostic-20261010-8192/`.
- Offline prepared directory: `reports/benchmark-collection-batch02-20261010-resource-r1/`.
- New prepared JSONL SHA256 `3f2777e221204a40458499bc6059c4906294f5ece4ac1774855ada55ecf5cd83`, eight rows, zero paid attempts during preparation.
- Read-only real ledger snapshot after prepare: six consumed main reservations, authorization SHA `d1c5a4ccf695dbccc1777af5123441bbd19aebe39da2a4d3e848abf30f6af0f2`, only original authorization/settings metadata existed. Prepare did not mutate financial metadata or reserve slots. Later root live actions are outside this snapshot.

## Verification and commands

Passed:

- `cargo test -p ncx-core --example verification_collect --manifest-path rust/Cargo.toml`: 10 tests.
- `cargo clippy -p ncx-core --example verification_collect --manifest-path rust/Cargo.toml -- -D warnings`.
- Explicit `rustfmt --check --edition 2021` for the two changed Rust files.
- `cargo build -p ncx-core --example verification_collect --manifest-path rust/Cargo.toml`: rebuilt the actual main entry, not only test binaries.
- `python -m pytest -q tests/test_verification_collect_binary.py`: four tests of actual built entry; zero HTTP.
- `python -m ruff check tests/test_verification_collect_binary.py`.
- Actual `offline-resource-test` isolated diagnostic: original metadata unchanged, six old reservations preserved, repeated group rejected, nine-row initial cohort rejected, unapproved token/build change rejected, stable timeout change rejected, same cohort later build rejected; seventh fake reservation receives ordinal 7. It uses a separate diagnostic ledger and does not affect the real cap.

Prepare is already complete. Root may explicitly run:

```powershell
& 'rust/target/debug/examples/verification_collect.exe' live 'reports/benchmark-collection-batch02-20261010-resource-r1' --live
```

This command makes real paid main-model calls and is intentionally **not executed by this implementation agent**. It still makes no Jev calls.

Zero-fee ledger diagnostic:

```powershell
& 'rust/target/debug/examples/verification_collect.exe' offline-resource-test '<new isolated directory>'
```

Collector source and binary are frozen after preparation for root's batch. Existing original reports, source data, production policy/extractors, installer and packaged executables were not rewritten.
