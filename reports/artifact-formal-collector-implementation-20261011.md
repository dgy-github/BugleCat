# Fresh artifact cohort and formal main collector — 2026-10-11

GPT-6.1-Sol completed the source preregistration and new single-response collector. This agent made **0 HTTP requests, 0 paid calls, 0 actual reservations/starts**. Preparation only created new files. Actual fake-transport tests use isolated temporary ledgers and are not formal observations. Root separately controls paid collection after its independent joins. No old paid runner or release was rebuilt.

## Frozen source and allocation

`artifact_cohort.py` supports the official expanded MBPP source only by exact `extended_mbpp.rebuild` reconstruction. It pins the official sanitized raw SHA, full raw SHA and fixed commit, preserves original groups/splits, and seals base/full/base-catalog files plus reconstruction/Oracle/registry/resolver sources. Arbitrary rebranded fixtures remain nonformal. The official full SHA is `ccf64ceae9c5403bf50a044cb6d505bfd2a2963ee58338ba268fd65beab92a9f`, derived source SHA `2875e6f093d50c4f10709666562b8f14018d932f31dc5dbda9ff81c02f4af9ce`.

Actual new cohort: `reports/artifact-formal-cohort-expanded-20261011-v1.json`, rawSHA `72bb061e677766332230cdaebc9898110d192298b03f66a5b531fb7badb5a0be`, canonical seal `9e8528f164e5df4f7515fa6afac69c48ebb8fe882204479bdfd10496dbad327f`. Fresh441calibration/454holdout source groups; budget488main/499Jev at registration; blockers empty. Before any new main response, a fixed source order allocates **288 calibration groups**, retaining at least200 main/Jev requests for a later qualified holdout. Labels/negative quota are unknown. Mainsettings are frozen in the cohort: deepseek-flash, ultra,8192tokens,60seconds,0retry,openai; original SYSTEM prompt hash is bound. No holdout cases or predictions were opened. This is collection preregistration, not a certified dataset or threshold. Historical cap400/cap500 refusal files/reports remain unchanged.

## New actual entry point

New source `rust/crates/ncx-core/examples/verification_artifact_collect.rs` and `verification_artifact_collect/support.rs`. The new binary reuses original read-only SYSTEM/user prompt and serialization/session utilities; original collector source/exe remain unchanged. It accepts only the pinned official derived source and immutable authenticated cap500 extension, shares the **original SQLite** metadata/table/counter/group uniqueness, verifies the cohort's original-start baseline to reject reset/deletion, and adds a distinct immutable cohort/build/settings key without replacing old metadata.

Each batch is1..8 new calibration cases from the next fixed unattempted source sequence. It records `artifact_cohort_file`, raw `artifact_cohort_sha256`, canonical `artifact_cohort_hash` consistently in prepared/start/submission/finish/receipt. The cohort, all bound files, exact original request bytes, build, model, endpoint hash, workspace identities and public task/interface are revalidated before submission. Durable SQLite reservation precedes HTTP; global main reservation files share the same short lock and consume slots too. Failed/incomplete groups cannot retry. A canceled-before-start case consumes no new slot; in-flight canceled/timeout requests keep their started slot. No automatic retry; any HTTP failure or repeated identical response failure stops remaining batch cases.

Original `prepared-main.v1` / `collected-main.v1` schemas, rawrequest/response, candidate.txt, session/sources/events, native packet/B decision are retained for existing independent receipt joins. Raw bounded2xx bytes are fsynced before content/model/usage validation. Actual model and unsigned token counters are required. Non2xx bodies are omitted. Response bound512KiB, request bound128KiB, timeout60s. Native policy is host-rules-v3.3. It is a **single model response without tools or AgentLoop**, not a completed coding loop. No reference/acceptance assertions/Oracle labels go to the main model and no positive success assertion is appended by the host.

```powershell
& rust/target/debug/examples/verification_artifact_collect.exe prepare <manifest.json> <newdir>
& rust/target/debug/examples/verification_artifact_collect.exe live <prepared-dir> --live [--cancel-file path]
& rust/target/debug/examples/verification_artifact_collect.exe offline-test <newdir> <scenario>
```

Offline scenarios: success, failure, malformed, model_mismatch, invalid_usage, timeout, cancel_before, cancel_during. Default prepare/offline never sends HTTP. Live without explicit flag fails before side effects.

## First eight and exact build

- Manifest `reports/artifact-formal-main-batch01-20261011.json` rawSHA `6534cf85a0862ea3dde9754d690981211369d800a75f727f1a17db0e04dfec7b`.
- Actual prepared directory `reports/artifact-formal-main-batch01-20261011-prepared`.
- Prepared JSONL SHA `d02cce01a798fc5deb2bbc018430b265943d5154c343ef2426f725402dd47123`.
- Actual exe SHA `b35f1f21d4c4aaaedd781bcf4e62fb2b32f6262976bf8016a3c03ddd533f9ef8`.
- Main source SHA `7a6b93416f850f7cfed966a57104b57fe1c55e71fc106df42200ef08ca2f97be`.
- Support SHA `d93dd4c7000d27e45b3cb877962a46935ed3c53d94db4806095b65c37d14d8f4`.
- Exact source/exe copies: `reports/artifact-formal-collector-frozen-20261011/`.

The actual main was explicitly rebuilt after final source edits; cargo test/check alone do not update it. Source/exe/cohort are frozen for root-coordinated collection; subsequent Jev formal runner work uses separate files and does not alter these inputs.

## Necessary verification

```text
cargo check -p ncx-core --example verification_artifact_collect --manifest-path rust/Cargo.toml
cargo build -p ncx-core --example verification_artifact_collect --manifest-path rust/Cargo.toml
cargo test -p ncx-core --example verification_artifact_collect --manifest-path rust/Cargo.toml
7passed:2 new ledger guards +5 reused legacy utility guards
cargo clippy -p ncx-core --example verification_artifact_collect --manifest-path rust/Cargo.toml -- -D warnings
passed
python -m pytest -q tests/test_verification_artifact_collect_binary.py tests/test_verification_artifact_cohort.py
35passed in9.39s (15 actual binary +20 cohort)
python -m ruff check tests/test_verification_artifact_collect_binary.py tests/test_verification_artifact_cohort.py evals/verification/artifact_cohort.py
All checks passed
```

Actual binary tests verify source/exe hashes; original raw/candidate/session/native policy; existing `collection_receipts.verify` success join;12preexisting starts preserved plus exactly1new failed/canceled/successful slot; malformed/model/usage failure raw preservation; prestart cancellation0newslots; timeout/no retry; immutable receipt/no overwrite; real8case preparation with no ledger changes; invalid holdout,missingcohort,fixture,weakenedsettings,9casebatch refusal before output; explicitliveflag. Expanded-source regression rejects custom official claims and changed original full bytes. Root separately reported independent15binarytests and8 reference-only admissions passed, still0new HTTP at that review point.

Old exe hashes rechecked unchanged: artifact `82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`; collector `84f8fb1fa87a689d02a4dee55d775c82e0c2fc8d7322de78eb8ed1227460434c`; native replay `a7a81112a2acdb51fca6428a6d7e5ec2a44e69ae3d3c2edf09aa3c4f1b755a7c`.

No formal Jev scores or threshold were produced. Future independent intake must verify cohort-bound original SQLite starts, main transport, source/interfaces, Oracle labels, label registration before probabilities and full calibration quotas. A separately built formal Jev entry remains next; the frozen development artifact executable is not upgraded by this work. Source/budget sufficiency is a necessary gate, not a guarantee of150supported/50unsupported outcomes or final certification.
