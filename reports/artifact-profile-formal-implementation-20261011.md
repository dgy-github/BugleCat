# High-profile formal Jev sidecar — frozen implementation

GPT-6.1-Sol, 2026-10-11. **0 HTTP / 0 paid calls / 0 real Jev reservations or starts** by this child. Original profile and five legacy sidecars are preserved; no package/production setting or parent formal Python owner was changed.

## Completed files

New actual entry `rust/crates/ncx-core/examples/verification_artifact_profile_formal.rs`, with independent `engine.rs`, `support.rs`, `formal.rs` under the same-name directory. New regression file `tests/test_verification_artifact_profile_formal_binary.py`. Frozen copies are under `reports/artifact-profile-formal-frozen-20261011/`.

The transport engine is shared through wrapper-defined `RUNNER_SOURCE`, `SUPPORT_SOURCE`, `FORMAL_SOURCE` and crate gate/support modules. A subsequent independent holdout wrapper can reuse the engine while supplying a genuine holdout gate; this entry remains **calibration-only**. It reads the frozen high-profile collector validator directly and reuses original authorization/Ledger/prompt tools. It never edits a row to ultra or changes its split to bypass a validator.

The typed profile triple `main_resource_profile_file`, `main_resource_profile_sha256` and `main_resource_profile_hash` must match the actual source cohort, manifest, prepared-artifact, dataset/registration, original main prepared row, original SQLite start/finish and original main receipt. It is retained in every new Jev start/submit/finish/receipt. The dataset/registration retains the same protocol and exact original Oracle label binding; 150 supported and 50 unsupported independent groups are required before a key, marker or request can be used. A registration quota probe rejects before any dataset or probability access.

Beyond the original source gate, Rust now joins the original main SQL finish to the original final-receipt hash, actual request/raw-response/candidate bytes, the original single response's model/usage, original prepared seals/profile/build/settings, exact public user prompt and unchanged SYSTEM, exact main start/submit/finish event sequence, session/source log hashes and native packet bytes. The original main's embedded source hashes must match the frozen high main wrapper/engine/validator, frozen ledger support and original provider request/response sources. Native policy must be `host-rules-v3.3`. The parent independently verifies original executable/source and raw transport joins through its intake/observer as well.

Jev retains the actual prepared single `acceptance_violation` NOUL body: 48KB input, 32KB response, 8-second deadline, cancellation polling, no retry/redirect, actual model `jev-1.13.0` and unsigned token usage. Raw 2xx bytes are create-new/fsynced before parse, including invalid model/usage/JSON responses; non2xx bodies are omitted. Original request versus actual submitted hash is guarded and recorded. Root's current-extension reservation in the shared cap500 ledger is mandatory; original cap400 ordinals remain bounded under their original authorization. A create-new/fsynced `.started` marker survives failure/cancellation/crash and prevents a repeated reservation from sending again. Missing key is not_run, with no HTTP or started marker; it does not restore the root's already reserved slot.

## CLI

```powershell
rust/target/debug/examples/verification_artifact_profile_formal.exe check-source <prepared-artifact.jsonl> <id>
rust/target/debug/examples/verification_artifact_profile_formal.exe check-registration <prepared-artifact.jsonl> <id> <formal-registration.json>
rust/target/debug/examples/verification_artifact_profile_formal.exe validate <prepared-artifact.jsonl> <id> <formal-registration.json> <new-validation.json>
rust/target/debug/examples/verification_artifact_profile_formal.exe live <prepared-artifact.jsonl> <id> <formal-registration.json> <reservation.json> <new-output-directory> --live [--cancel-file <path>]
rust/target/debug/examples/verification_artifact_profile_formal.exe offline-transport-test <new-directory> <scenario>
```

The original full registered prepared file is required; an unregistered one-row projection cannot replace it. `check-source` verifies provenance only and cannot issue HTTP or qualify labels. Current real high calibration lacks the 150/50 quota; there is no real positive full-registration or live path result to report.

## Build/source contract for parent observer

Parent's independent filename dispatch must verify these **10 source fields** against these exact paths (plus actual exe SHA):

- `runner_source_sha256`: `verification_artifact_profile_formal.rs`.
- `runner_support_source_sha256`: `verification_artifact_profile_formal/support.rs`.
- `formal_gate_source_sha256`: `verification_artifact_profile_formal/formal.rs`.
- `artifact_transport_engine_source_sha256`: `verification_artifact_profile_formal/engine.rs`.
- `collector_validator_source_sha256`: frozen `verification_artifact_profile_collect/support.rs`.
- `profile_collection_engine_source_sha256`: frozen `verification_artifact_profile_collect/engine.rs`.
- `profile_collector_source_sha256`: frozen `verification_artifact_profile_collect.rs`.
- `frozen_collector_support_sha256`: frozen `verification_artifact_collect/support.rs`.
- `legacy_collector_support_sha256`: frozen `verification_collect/support.rs`.
- `provider_transport_source_sha256`: frozen `ncx-provider/src/jev.rs`.

Paths above are relative to core examples except provider. `runner_exe_sha256` is the actual main executable; start/submitted/finished also carry top-level main source and exe hashes.

## Tests actually performed

```powershell
cargo check -p ncx-core --example verification_artifact_profile_formal
cargo build -p ncx-core --example verification_artifact_profile_formal
cargo test -p ncx-core --example verification_artifact_profile_formal
cargo clippy -p ncx-core --example verification_artifact_profile_formal -- -D warnings
rustfmt --edition 2021 --config skip_children=true --check crates/ncx-core/examples/verification_artifact_profile_formal.rs crates/ncx-core/examples/verification_artifact_profile_formal/engine.rs crates/ncx-core/examples/verification_artifact_profile_formal/formal.rs crates/ncx-core/examples/verification_artifact_profile_formal/support.rs
python -m pytest tests/test_verification_artifact_profile_formal_binary.py -q
python -m ruff check tests/test_verification_artifact_profile_formal_binary.py reports/artifact_profile_formal_source_gen_20261011.py
```

**18 actual-main Python tests passed in 1.70s**, **10 Rust tests passed**, check/build/Clippy/fmt/Ruff passed. The real source test iterated **all seven eligible original first high-batch artifacts** (six independently supported, one unsupported as parent reports) and verified their original source/SQL/raw-byte/profile joins with no registration/key/probability/HTTP. It did not create new candidates or assertions. Four independently resealed profile/policy mutations were rejected. Other tests cover nine fake bounded transport outcomes, original raw-response/usage/model/body/source/exe joins, profile field continuity, marker duplicates, old-development promotion rejection, unqualified 149/50 and 150/49 gates, explicit-live flag and the ten source-path mappings.

Persistent fake success receipt: `reports/artifact-profile-formal-binary-success-20261011/attempt/`, fake transport1, HTTP0, paid0, development/offline true. These fixtures are not formal observations. Real calibration and holdout thresholds remain unqualified/null; production remains false.

## Frozen hashes

- Actual main exe: `286f4a22365386773293e9a7cc73e9b4f46543754612f1ada5bc40c1d225c52a`, **7,989,248 bytes**, mtime UTC 2026-10-11 01:38:37.
- Main source: `ea8d8d5153001f605453f4659e7d85093a83e08ac4706aeb1f2fff0d058e0141`.
- Shared transport engine: `97f5ab31f2c8e2ac8ffa9e44864e4037bf24b93bf129b5b4ea2c3a472c391a2a`.
- Support: `820539d33f696ffb85d8efc46127406288368c90df22de154a12ad5f8d04cea6`.
- Formal gate: `06d212332f50aa970908dadfd296a13d03714d7b2e5aecd8dc1490385d3d5605`.
- Binary tests: `8d9a045aba009fcc818bbbdc7b5c0fdaa06c5cd82110bd7b6ce52dbe4908b7ed`.

Rehashed unchanged: profile main `ae6d0cf5...7148b39`, original formal main `b35f1f21...533f9ef8`, original formal Jev `a1d68d38...c54e728`, development artifact `82efa727...0a47b9e`, legacy main `84f8fb1f...7460434c`, native replay `a7a81112...1b755a7c`, provider request `9d1f1619...5b96ee22`. Their sources and the source-bound profile Python/cohort were not modified.

## Next independent holdout transport boundary

Root has authorized continued zero-HTTP implementation of independent holdout main/Jev entries. Before actual transport they must call the shared Python owner to reconstruct qualified calibration from original registration, observations and runner/raw-byte bindings, validate the original once-only holdout cohort/lock, enforce full calibration/prior-group exclusion and preserve the fixed threshold. Main requests must first bind the genuine holdout cohort/profile in the same cap500 SQLite ledger; Jev must additionally bind a once-only 150/50 independent holdout label registration and root reservation. Neither new entry may invoke a calibration-only validator using a fabricated split. The profile main engine and this formal entry remain frozen; new wrappers/support can reuse the shared Jev transport engine through explicit gate injection. No real holdout cohort is opened by this report, and the entries are subsequent work.
