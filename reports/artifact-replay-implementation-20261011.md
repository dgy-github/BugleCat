# Artifact acceptance replay implementation — 2026-10-11

GPT-6.1-Sol implemented a separate development-only Rust sidecar. This agent made **zero network, Jev or paid main-model calls**, did not reserve any real budget slot, and did not change original native packets, paid receipts, labels, production configuration or published executables. Any subsequent live smoke coordinated by root is a separate recorded action.

## Object and version contract

The sidecar sends exactly the original unique Python code block, public task/interfaces, frozen acceptance assertions and imports under the host hypothesis `host_artifact_hypothesis`. It does not invent a positive native completion statement or claim that the main model ran tests.

- Prepared schema: `buglecat.verification.prepared-artifact.v1`.
- Track policy: `artifact-acceptance-v1`; template `jev-artifact-acceptance-v1`.
- Request model: `jev-latest`; successful response must be actual `jev-1.13.0`.
- Exactly one question: `acceptance_violation`, type `noul`, instructions byte-identical to root's `artifact_acceptance.py` QUESTION.
- State keys and request/question shape are strict. Labels, reference implementation, oracle result and additional native questions cannot be sent in the body.
- Only split `development` is accepted. Formal calibration/holdout are explicitly rejected.

## Implementation and safety invariants

New files:

- `rust/crates/ncx-core/examples/verification_artifact.rs`: CLI, explicit live transport, durable attempt records and fake-transport main regression.
- `rust/crates/ncx-core/examples/verification_artifact/support.rs`: source/body seals, original-source joins, shared-reservation verification and strict single-answer parsing.
- `tests/test_verification_artifact_binary.py`: actual executable regressions including UTF-8 canonical hash compatibility.

The validator checks Python-compatible canonical prepared/request hashes, raw source/manifest/main-receipt/oracle/protocol/authorization file hashes, original candidate SHA and original single code fence. Task prompt/assertions/imports must equal the frozen source task; interfaces/group/task/id must equal the original collection manifest. Main receipt must be successful with equal recorded request/actual hashes and correct original native policy. Oracle seal, code/candidate/task bindings and reference admission must agree; Unknown oracle rows are refused. Oracle data stays outside the body.

Live takes one id and one existing Python `paid_budget.py` reservation. It verifies the immutable authorization SHA, actual ledger root and authorization metadata, group-derived reservation filename, group/request hash, unique global groups/indices and the 400 cap. The same-directory create-new/fsynced `<group-hash>.started` marker is never deleted: failure, cancellation and incomplete execution cannot resend that reservation. The marker uses `.started`, not `.json`, so Python's reservation `*.json` counter is unaffected. No independent budget or retry loop is created.

Missing key or invalid prepared input never constructs/posts a request. A missing key produces a not-run receipt with zero attempted calls and no started marker; the Python reservation remains consumed. User credentials are read only from the process environment at explicit live time, never printed or loaded from a repository file.

The original prepared body is passed directly to `ncx_provider::jev::HttpTransport`; the old three-question `jev::request/check_with` is never called. The guarded submission hash is computed from `serde_json::to_vec`, the same encoding used by reqwest's `.json` in the embedded provider implementation. Limits: 48,000 request bytes, 32,000 response bytes while reading, outer 8-second deadline, cancellation polling, zero retries/redirects. Successful transport raw bytes are create-new/fsynced **before** JSON/model/answer/usage validation. Non-2xx bodies are omitted. Responses require exactly one finite NOUL in [0,1], actual model match, and unsigned integer input/output token counts.

Started/submitted/finished JSONL and receipt record request/body/actual hash, original-source seals, reservation bytes hash, exact compiled runner/support/provider source hashes and actual executable hash. Submitted includes `planned_raw_response_file`; finished includes parsed response/model/usage, `response_sha256` and `raw_response_file`. Malformed/model-mismatched 2xx raw bytes remain auditable. Before-submission cancellation still consumes the marker but has no submitted record; interruption preserves its pending denominator. Live `paid_attempts` counts attempted budget use; `paid_calls` is null because the tool cannot infer billing. Offline tests explicitly report zero paid/network calls.

## Actual build and zero-fee verification

Passed:

```powershell
cargo check -p ncx-core --example verification_artifact --manifest-path rust/Cargo.toml
cargo test -p ncx-core --example verification_artifact --manifest-path rust/Cargo.toml
# 2 tests: malformed probability/model/usage/extra answers; reservation binding/cap/marker
cargo clippy -p ncx-core --example verification_artifact --manifest-path rust/Cargo.toml -- -D warnings
rustfmt --check --edition 2021 rust/crates/ncx-core/examples/verification_artifact.rs rust/crates/ncx-core/examples/verification_artifact/support.rs
cargo build -p ncx-core --example verification_artifact --manifest-path rust/Cargo.toml
python -m pytest -q tests/test_verification_artifact_binary.py
# 12 passed
python -m ruff check tests/test_verification_artifact_binary.py
```

Actual main tests cover successful raw-byte/source/exe/hash/usage joins, malformed JSON, wrong model, invalid usage, extra answers, omitted HTTP500 body, cancellation before/during request, timeout, duplicate started marker, explicit live flag, no credentials needed, formal mode refusal and label contamination refusal. A first test run exposed the Windows test reader's default legacy encoding; reading UTF-8 explicitly fixed the test harness, then all 12 passed, including Chinese fixture byte/hash equality. No network was involved in either run.

The actual main executable is `rust/target/debug/examples/verification_artifact.exe`:

- Executable SHA256 `82efa727af1265ba03f3a14c44cc9ef7d179589d0c2e72822609a94e40a47b9e`.
- Runner compiled source SHA256 `dc4d190f83210e4c17c0962f14532c486f9cf93b2bb917b58f02d4aaac9ba9bb`.
- Support compiled source SHA256 `8cd1e70036f0c4aa7b9774cd70a2dafbc235749756814cfb72ecda841a4385ed`.
- Provider transport compiled source SHA256 `d8bccee31b4fca24d91e3593767ed162fbaadf0120349b68e358d8866c5c8871`.
- Exact executable and embedded source bytes archived create-new in `reports/artifact-binary-diagnostic-20261011/frozen-build/`.

All **seven** real r2 development rows validated using the actual entry: batch01 760/68/401 and batch02 3/104/796/103. Their validation outputs are `reports/artifact-binary-diagnostic-20261011/mbpp-main-<id>.validation.json`. No key or reservation was read. The separate main3 validation is `reports/artifact-validation-main3-20261011.json`.

Actual success fake transport evidence is at `reports/artifact-binary-diagnostic-20261011/actual-main-success/attempt/`: `request.json`, `raw-response.json`, `events.jsonl`, `receipt.json`. This is an isolated synthetic transport fixture, never formal data or a paid API receipt. Its repeated-start check blocked a second fake request.

## CLI

```powershell
# Zero network, no key lookup, no reservation/marker creation
& 'rust/target/debug/examples/verification_artifact.exe' validate 'reports/artifact-prepared-batch02-20261011-r2.jsonl' 'mbpp-main-104' '<new-validation.json>'

# Zero network; scenario: success/malformed/model_mismatch/invalid_usage/extra_answers/http500/cancel_before/cancel_during/timeout
& 'rust/target/debug/examples/verification_artifact.exe' offline-transport-test '<new-isolated-directory>' 'success'

# Real paid endpoint: root alone coordinates the following explicit invocation.
# reservation.json must already exist from the shared paid_budget.py ledger.
& 'rust/target/debug/examples/verification_artifact.exe' live '<prepared-jsonl>' '<id>' '<reservation.json>' '<new-output-directory>' --live
# Optional: --cancel-file '<path>'
```

## Remaining limits

The seven viewed answers and successful oracle labels remain development data, not prediction-blind formal samples. A classification score never becomes Pass or full task verification. Formal source intake, positive/negative quotas, unchanged 150/50 calibration gates, threshold certification, once-only holdout and actual task-loop decision benefit remain unachieved. Existing framework A/B artifact classifications are unavailable, not imagined “all pass”. Independent content hashes are audit bindings, not provider signatures or proof against an administrator rewriting the trusted local ledger.

The new example was added **after** the v3.3 package's sealed source graph. It is not included in that package/manifest and no package or old collector/replay main executable was rebuilt for this tool. Native policy remains v3.3, default off, production categories empty and threshold null. Root owns policy/data/docs/budget follow-up; independent trace paths and historical receipts were not edited.
