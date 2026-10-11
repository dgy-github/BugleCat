# Separate high/16384/120 main profile — implementation record

Execution model: GPT-6.1-Sol. Date: 2026-10-11. Child activity: **0 HTTP / 0 paid calls / 0 real reservations / 0 real cohort registrations**.

## New files and ownership

- `evals/verification/artifact_profile_cohort.py`: profile validation and new-cohort preregistration wrapper. It calls the original `artifact_cohort.preregister` for the unchanged source reconstruction, global budget/snapshot, exclusion rules and fixed source ordering. Only the coordinator actually registers a real cohort.
- `rust/crates/ncx-core/examples/verification_artifact_profile_collect.rs`: independent actual main entry.
- `rust/crates/ncx-core/examples/verification_artifact_profile_collect/engine.rs`: shared main collection engine extracted once into new files from the frozen collector. Subsequent entries can reuse this engine instead of copying another collector.
- `rust/crates/ncx-core/examples/verification_artifact_profile_collect/support.rs`: shared source/profile validator. It reads actual high profile bindings; it does not rewrite inputs to ultra or invoke the old ultra validator with fabricated settings. It reuses the frozen public authorization-chain/Ledger/prompt/file/seal tools.
- `tests/test_verification_artifact_profile_binary.py`: actual-main offline and tamper regression tests.
- `reports/artifact-main-resource-profile-high-20261011-v1.json`: profile template only, not a registered cohort.

The profile schema is `buglecat.verification.main-resource-profile.v1`, profile ID `deepseek-flash-high-16384-120-v1`. Exactly frozen settings: model `deepseek-flash`, reasoning `high`, max tokens 16384, timeout 120 seconds, retries 0, protocol `openai`.

The template seals original cohort raw/canonical hashes and the root's diagnostic raw hash. Validation reads each of the four original failure receipts and corresponding raw response bytes, verifies their source cohort and response SHA, and verifies `length`, empty content and 8192 completion/reasoning tokens. The original model inputs, SYSTEM/user template and provider request builder are unchanged. A wrong setting, changed diagnosis, wrong prior cohort, wrong raw bytes or wrong profile seal fails before configuration or transport.

The new cohort wrapper replaces only resource settings and adds explicitly sealed profile/provenance fields and bindings in a **new** output. It preserves source order, original split, shared cap500 authorization chain and all starts/reservations. Under the coordinator-reported baseline of 35 main starts, main465/Jev499 remaining, the original owner preallocates at most 265 fresh calibration groups and reserves 200 for holdout. Old 23 ultra candidates remain outside this profile's formal denominator; old attempted groups cannot be retried. The wrapper binds its own Python source, the new wrapper/engine/validator sources and the four original diagnostic receipts/raw responses.

## Request/receipt contract

The three added fields are identical in cohort, manifest, prepared-main, actual SQLite `started_json`, attempt-started, request-submitted, attempt-finished and final collected-main receipt:

```text
main_resource_profile_file
main_resource_profile_sha256   # original raw file bytes
main_resource_profile_hash     # canonical artifact seal
```

Root's artifact intake propagates these fields separately to dataset and prepared-artifact evidence. The new validator supports calibration only. Holdout main requests still need a separate locked-gate transport entry. The previously frozen formal Jev entry still supports only the original ultra source validator; a new profile Jev entry must reuse this new shared validator before any high-profile Jev call.

Prepare/live load the configured provider/model/key with an ephemeral `Overrides.reasoning_effort=high`, without modifying the user's global config. The original builder emits actual `reasoning_effort=high`, `thinking.type=enabled`, `max_tokens=16384`. Before HTTP, the engine revalidates profile/cohort/source bindings, exact request bytes/hash, prepared seals, actual build and endpoint, global original-ledger baseline and the next fixed case prefix. A batch is 1–8 groups, one main request per group, no retry. Authorization remains the shared original ledger and immutable extension; a distinct profile/build bind is added without replacing the old metadata.

Start/submit/finish/receipt source attestation also adds:

```text
collection_engine_source_sha256
frozen_collector_support_sha256
```

These must be independently checked by the coordinator's formal intake. The actual response is bounded, raw 2xx bytes are fsynced before parse, empty content/model/usage failures remain original receipts, non2xx bodies are omitted, timeout/cancellation keep started slots, and repeated provider/resource failures stop the rest of a batch. Actual single-response scope is retained: no AgentLoop, tools or independent assertions are executed by the main runner.

## Actual CLI

```powershell
python -m evals.verification.artifact_profile_cohort validate-profile --profile reports/artifact-main-resource-profile-high-20261011-v1.json
python -m evals.verification.artifact_profile_cohort register --source <source> --catalog <catalog> --authorization <current-extension> --protocol <artifact-protocol> --profile <profile> --cohort-id <new-id> --output <new-cohort.json>
rust/target/debug/examples/verification_artifact_profile_collect.exe validate-profile <profile.json>
rust/target/debug/examples/verification_artifact_profile_collect.exe offline-body-test <profile.json>
rust/target/debug/examples/verification_artifact_profile_collect.exe prepare <manifest.json> <new-directory>
rust/target/debug/examples/verification_artifact_profile_collect.exe live <prepared-directory> --live [--cancel-file <path>]
rust/target/debug/examples/verification_artifact_profile_collect.exe offline-test <new-directory> <scenario>
```

Manifest retains the original collection schema and complete `main_collection_settings`, including `max_retries=0` and `provider_protocol=openai`, the three profile fields, original source/catalog/protocol/authorization fields, original cohort fields and 1–8 exact frozen cohort cases.

Offline scenarios: success, failure, malformed, model_mismatch, invalid_usage, timeout, cancel_before, cancel_during. They explicitly use development fixtures in an isolated ledger with 35 artificial old slots; they are not real cohort candidates or global paid starts.

## Verification actually performed

```powershell
cargo check -p ncx-core --example verification_artifact_profile_collect
cargo build -p ncx-core --example verification_artifact_profile_collect
cargo test -p ncx-core --example verification_artifact_profile_collect
cargo clippy -p ncx-core --example verification_artifact_profile_collect -- -D warnings
rustfmt --edition 2021 --config skip_children=true --check crates/ncx-core/examples/verification_artifact_profile_collect.rs crates/ncx-core/examples/verification_artifact_profile_collect/engine.rs crates/ncx-core/examples/verification_artifact_profile_collect/support.rs
python -m pytest tests/test_verification_artifact_profile_binary.py -q
python -m ruff check evals/verification/artifact_profile_cohort.py tests/test_verification_artifact_profile_binary.py reports/artifact_profile_source_gen_20261011.py
```

Final results: **18 actual-binary Python tests passed in 3.27s**; **7 reused Rust guard tests passed**; check/build/Clippy/fmt/Ruff passed. The initial extraction loaded the same private legacy module twice and Clippy rejected it; the new validator was corrected to reuse frozen public tools once and provide its own small bound-file reader. The final build and actual main tests were rerun afterward. No frozen source was edited to fix this.

Actual binary tests verify original profile diagnosis, high/16384 actual request body without credentials/config writes, seven resealed profile tamper variants, seven bounded fake-response cases, prestart/inflight cancellation, exact SQLite/profile event/receipt joins, original bytes/model/usage/session/native v3.3 packet join, duplicate output refusal, explicit-live-flag gate and source/exe/engine/provider attestation. The Python preregistration wrapper positive path explicitly mocks its upstream owner; **no real cohort was registered by the child**. A coordinator's actual registered-source prepare and subsequent independent receipt join are still required before deciding paid collection.

Persisted actual-main offline success evidence: `reports/artifact-main-profile-binary-success-20261011/`, fake transport1, externalHTTP0, paid0. Source/exe copies: `reports/artifact-main-profile-frozen-20261011/`.

## Frozen hashes

- Actual exe: `ae6d0cf59375fae0e31008eb9d384ce60a2364f4c28f30ed3d762cf0c7148b39`, **9,074,688 bytes**, mtime UTC 2026-10-11 01:30:34.
- Main source: `af9f8478e9ac25bd430807f71365cce0079980aadf74f23ca45829a61fac9cb0`.
- Shared engine: `4beb85d3f5ee204599a5767d937b77a041e14b934e58efd6a78ed9a415357797`.
- Shared validator: `9d2fa2a6cd50e5b7a052375713beb842a82eb4dde9243962ae5dc1e64bdf643d`.
- Python profile owner: `da81bf7f8195a4a2c44e6a9b5e3a187ad5a052fd7e3409d2bf9a23872d85ffcc`.
- Binary test: `39295f85b410dbb0c5edfc198847ce8447d8f9e8febe5787d1c3e81f12ef9968`.
- Profile raw: `e0044b9de9e61ca55358a5081eba27a85c352f17ef3d74afa8ba028bc9064427`.
- Profile canonical: `87e2dea1ef9f4e24c2936616361db2efbc0e1bf4a1b89aa2f8f8268fee52762d`.
- Original diagnostic raw: `054dc456393a12f2e668500d8bda5405af490278bdc37aa441861a3e2cffcfb4`.

Rehashed unchanged actual old executables: formal main `b35f1f21...533f9ef8`; formal Jev `a1d68d38...c54e728`; development artifact `82efa727...0a47b9e`; legacy main `84f8fb1f...7460434c`; native replay `a7a81112...1b755a7c`. Provider/request.rs remains `9d1f16194e3c8e48f3c922a43f4ec5a11d98a09ad32fbce7995bcd885b96ee22`. Sealed v3.3 release is untouched and does not contain this new sidecar.

## Remaining limits

No new main response or Jev observation has been collected by this child. A larger output budget and lower reasoning profile cannot guarantee 150 supported / 50 unsupported independent groups, eliminate failures, or ensure 200 separately qualifying holdout groups. Old viewed ultra responses cannot be renamed as high-profile formal samples. The production switch remains off and the certified threshold remains null. New-profile formal Jev and actual locked holdout transport entries remain subsequent independent work.
