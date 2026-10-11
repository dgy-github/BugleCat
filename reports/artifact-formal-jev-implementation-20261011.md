# Independently registered artifact Jev entry — 2026-10-11

GPT-6.1-Sol implemented a **new** `verification_artifact_formal` entry, without modifying the frozen development sidecar, formal main collector or old native replay/collector executables. This agent made **0 HTTP requests, 0 paid calls and 0 actual global reservations**. The actual binary was explicitly built and exercised through isolated fake transport. Root owns all paid collection and eventual predictions. No production switch or threshold was enabled.

## Gate and provenance

New files: `rust/crates/ncx-core/examples/verification_artifact_formal.rs`, `verification_artifact_formal/support.rs`, `verification_artifact_formal/formal.rs`, `tests/test_verification_artifact_formal_binary.py`.

The entry accepts only fresh **calibration** artifacts from the byte-pinned derived official source, immutable cap500 extension and formally preregistered cohort. It reuses the frozen collector validator read-only; validates all cohort/source/Oracle/authorization bindings; verifies the actual original SQLite main start carries the same group, original main request and cohort hashes; joins the original candidate/code/manifest/oracle to the exact single acceptance-violation question. Old viewed development responses cannot be promoted by changing split, exposure flags and seals. Holdout is rejected by this entry.

Before transport or a started marker it validates the sealed formal label registration: dataset/prepared raw SHA and canonical hashes, original cohort, exact selected set, unique groups, Oracle receipt/label consistency, unchanged protocol and **at least150 supported and50 unsupported**. The registered prepared file must be the actual file passed to the entry, not a one-row projection. Every registered row is revalidated; no probability file is read to choose cases. Unknown/failed main results cannot become binary labels. An unqualified registration is rejected before accessing its dataset/prepared bindings or any key. `check-registration` exposes this zero-network preflight.

Live remains explicit and one prepared group per invocation. It requires the existing root-generated Jev reservation under the same global ledger; original metadata hash remains unchanged, old and extension reservation hashes are accepted globally, selected reservation must use current extension hash. Historical cap400 ordinals cannot exceed400; current cap500 ordinals cannot exceed500. Unique group/index and expected filename checks remain; create_new/fsync `.started` permanently consumes the reservation even on cancellation/crash. Output/events/raw response must be inside the authenticated Jev ledger, so independent root transport joins can audit the original files. Missing key yields not_run with no started marker; it does not undo the root's prior reservation.

All original start/submitted/finished receipts include exact `request`, raw/canonical request hash, compiled build metadata, source/code/candidate/Oracle hashes, cohort file/rawSHA/canonicalhash, formal registration rawSHA/canonicalhash and reservation bytes/hash. Marker equals the original start. Actual submitted hash is independently recorded. Original2xx bytes are fsynced before parse/model/NOUL/usage validation, including malformed/wrong-model responses; non2xx bodies are omitted. Existing8second/48KB request/32KB response limits, cancellation, no retry, exact actual `jev-1.13.0`, one finite[0,1]NOUL answer and unsigned usage counters apply. No native claim or Pass is synthesized. Probability classification/threshold selection remains root's separate fixed statistical owner.

Build metadata embeds main, support, provider transport, formal gate and the reused frozen collector/legacy validator hashes. The parent was told to verify the three additional helper fields in its independent observation join. The final receipt is create_new/fsynced and its raw hash appears in the finish event. Formal registration and all source joins are repeated immediately before claiming a live request.

## CLI and limitations

```powershell
& rust/target/debug/examples/verification_artifact_formal.exe check-registration <prepared.jsonl> <id> <registration.json>
& rust/target/debug/examples/verification_artifact_formal.exe validate <prepared.jsonl> <id> <registration.json> <new-validation.json>
& rust/target/debug/examples/verification_artifact_formal.exe live <prepared.jsonl> <id> <registration.json> <reservation.json> <newdir> --live [--cancel-file path]
& rust/target/debug/examples/verification_artifact_formal.exe offline-transport-test <newdir> <scenario>
```

Scenarios: success,malformed,model_mismatch,invalid_usage,extra_answers,http500,cancel_before,cancel_during,timeout. Fake fixtures are development code checks, **never registered formal observations**. There is no holdout/retuning/production path. Prepared aggregate input is bounded64MB to support a full registered set; each actual outgoing request remains48KB.

At delivery there is **no real150/50 qualifying registration**. The parent reported first8 fresh main responses independently yielded5supported/2unsupported/1unknown, with its second batch underway. This implementation therefore does not claim a real successful formal request, full positive registration integration, calibrated threshold, certified classifier or completed holdout. It does not use the fake response as evidence of predictive quality.

## Verification and exact actual build

```text
cargo check/build/test -p ncx-core --example verification_artifact_formal --manifest-path rust/Cargo.toml
actual main explicitly built;10 Rusttests passed (1quota,2response/reservation,7read-only reused guards)
cargo clippy ... -- -D warnings
passed
rustfmt --check --edition2021 --config skip_children=true <three new Rustfiles>
passed
python -m pytest -q tests/test_verification_artifact_formal_binary.py
12passed in2.10s
python -m ruff check tests/test_verification_artifact_formal_binary.py
passed
```

Actual main tests cover9bounded response/HTTP/cancel/timeout cases, exact request/raw2xx hash and usage preservation, no non2xx secret body, immutable marker/duplicate-attempt rejection, consistent registration/cohort fields, actual embedded source/exe/helper hashes,150/49 and149/50 and8/0 preflight refusal, legacy development relabel/reseal rejection, and explicit live flag. Final review added the original-authorization ordinal400 bound; actual main was rebuilt and12binary tests/strictClippy rerun after that change. No stale test executable is presented as the main entry.

- Final actual exe SHA: `a1d68d38582ff2a1025835cbbe42f35a319db93c2665b81ff934ca471c54e728`;7,758,336bytes.
- Main source SHA: `870d6c89e4f700678c69d623835200e96a4ee8e723397bb2fbd24862d6a2ac61`.
- Support SHA: `83e65212c405a41d95366c2efcce5e4d8a3dbc1d4eb72607e2df61265f53ad94`.
- Formal gate SHA: `3f0c882782fc464353970c76b822d3511e964811fcd0cb9f8b53096f4a708916`.
- Frozen copies: `reports/artifact-formal-jev-frozen-20261011/`.
- Actual main offline receipt example: `reports/artifact-formal-jev-binary-success-20261011/attempt/`.

The earlier provisional `d8b418...` formal exe hash was superseded **before any formal paid attempt** by the necessary ordinal bound and was explicitly corrected to the parent. This is a new tool outside the sealed v3.3 package. The frozen main collector remains `b35f1f21d4c4aaaedd781bcf4e62fb2b32f6262976bf8016a3c03ddd533f9ef8`; old development artifact/native collector/native replay remain their previously reported hashes. Future holdout requires a separate qualified calibration gate, untouched source preregistration, immutable locked threshold and new explicit entry; it is not implemented here.
