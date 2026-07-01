# Video Agent P1 Acceptance Status

Status date: 2026-07-01

This document tracks the P1 `S01`-`S12` acceptance evidence for the AI video-agent production framework. It is not a completion claim: live TOS and live Seedance/TOS end-to-end rough-cut validation still require real credentials and account balance.

## Commands

Run script commands from `D:\agent_prac\nanocodex` unless noted. Run the direct `cargo ...` commands from `D:\agent_prac\nanocodex\rust`.

One-command local acceptance gates, without live TOS/Seedance spending:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly
```

One-command full paid P1 acceptance, after TOS credentials and account budget are available:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_acceptance.ps1 -StartTemporalIfNeeded -RunPaidLive
```

`-LocalOnly` is intentionally not a completion claim: it skips live TOS readiness and paid Seedance/TOS proof. `-RunPaidLive` first runs all local/Temporal gates and live readiness, then runs both paid Seedance/TOS proofs.

Manual local gates from `D:\agent_prac\nanocodex\rust`:

```powershell
cargo test -p ncx-video-agent --quiet
$env:PROTOC='C:\Users\jingc\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\protoc-bin-vendored-win32-3.2.0\bin\protoc.exe'
cargo check -p ncx-video-agent --features temporal --bin p1_temporal_probe
cargo run -p ncx-video-agent --bin p1_dry_run --quiet -- D:\agent_prac\nanocodex\rust\target\p1_dry_run_out_current
powershell -NoProfile -ExecutionPolicy Bypass -File .\crates\ncx-video-agent\scripts\p1_verify_rough_cut_trace.ps1 -OutDir D:\agent_prac\nanocodex\rust\target\p1_dry_run_out_current -AllowLocalDryRun
powershell -NoProfile -ExecutionPolicy Bypass -File .\crates\ncx-video-agent\scripts\p1_paid_preflight_selftest.ps1
```

Non-paid live preflight before spending Seedance budget:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_live_readiness.ps1 -StartTemporalIfNeeded
```

This compiles the Temporal probe and runs `p1_smoke`, including a real TOS put/get/delete roundtrip when TOS credentials are present. It does not submit a Seedance job.

Temporal crash-recovery smoke, with a real dev server at `127.0.0.1:7233`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_temporal_crash_recovery_smoke.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_temporal_dry_run_recovery_smoke.ps1
```

Paid live Temporal recovery smoke, also requiring `P1_TEMPORAL_ALLOW_REAL_ARK=1`, ARK config, and real TOS credentials:

```powershell
$env:P1_TEMPORAL_ALLOW_REAL_ARK='1'
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_temporal_live_seedance_recovery_smoke.ps1
```

fastText model fetch helper:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_fetch_fasttext_lid.ps1 -Format bin
# or smaller official quantized model for smoke:
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_fetch_fasttext_lid.ps1 -Format ftz
```

Real paid Seedance/TOS smoke:

```powershell
cargo run -p ncx-video-agent --bin p1_seedance_tos_smoke -- --submit-real-ark-job D:\agent_prac\nanocodex\rust\target\p1_seedance_tos_smoke_out
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_verify_rough_cut_trace.ps1 -OutDir D:\agent_prac\nanocodex\rust\target\p1_seedance_tos_smoke_out
```

This live smoke is intentionally opt-in because it spends ARK/Seedance budget. On success it must produce:

- a Seedance shot artifact uploaded to TOS,
- a local TOS roundtrip copy validated by media L0,
- a deterministic `rough_cut.mp4` assembled from the real returned shot,
- a project-level `rough_cut` artifact uploaded to TOS,
- `failed_shots.json`, `assembly_manifest.json`, and `trace.json`,
- passing `validation_records` for both the Seedance video and the rough cut.

## S01-S12 Evidence

| Step | Current evidence | Status |
| --- | --- | --- |
| S01 environment smoke | `p1_smoke` checks SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, real fastText model, ARK via `ncx-config`, TOS roundtrip, and VL via `ncx-config`. OpenCV is probed through `opencv_version` when available, otherwise Python `cv2` with a tiny `cvtColor` operation. TOS config parsing is covered for both `TOS_*` and AWS-compatible aliases (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL`, `S3_BUCKET`) plus region inference from Volcengine endpoint hostnames. With local Temporal + `LID_176_FTZ`, all pass except live TOS credentials. | Partial: live TOS missing |
| S02 SQLite schema | `db.rs` creates `projects`, `chapters`, `scenes`, `shots`, `artifacts`, `jobs`, `validation_records`, `golden_cases`, `gate_metrics`, `model_metrics`; tests verify WAL, JSON1, and `jobs.idempotency_key` uniqueness. Test database paths now include process id, nanosecond timestamp, and an in-process counter so repeated/parallel test runs cannot reuse stale SQLite files after Windows process-id reuse. | Passed locally |
| S03 Temporal workflow and crash recovery | `p1_temporal_probe` compiles with Temporal Rust SDK; `p1_temporal_crash_recovery_smoke.ps1` passed against `temporal server start-dev`, killing and restarting the worker before approval signal. `p1_temporal_dry_run_recovery_smoke.ps1` also passed with a local P1 dry-run rough-cut workflow after killing the worker during a Temporal timer. `P1LiveSeedanceWorkflow` now submits a real Seedance job in an Activity, polls through workflow timers, then persists TOS/rough_cut/trace in a final Activity; the paid recovery harness kills the worker after a real `running` poll marker. | Passed minimal S03 probe and local P1 dry-run workflow; live recovery proof pending credentials/paid run |
| S04 jobs idempotency | `jobs.rs` and `render.rs` tests verify duplicate `(shot, attempt, params)` does not re-submit to provider. `submit_job_once` now treats an existing idempotent job without `provider_job_id` as an ambiguous external-submit state and fails safely without calling the provider again; regression tests cover both a crash-like `reserved` row and a `submit_failed` row after an uncertain transport error. | Passed locally |
| S05 budget ledger | `jobs.rs` tests cover reserve, settle, hard-stop on exhausted budget, concurrent reserve, and failure budget release. `trace.json` now exports a project budget summary with `budget_total`, `budget_reserved`, `budget_spent`, `job_cost_total`, and `job_reserved_total`; the reusable verifier checks `budget_spent == job_cost_total` and `budget_reserved == job_reserved_total`, so the delivered trace proves the Runbook budget reconciliation invariant instead of leaving it only in unit tests. | Passed locally |
| S06 ARK tool node | `ark.rs` and `render.rs` cover submit/poll/download/upload plumbing with fake transports; `p1_seedance_tos_smoke` is the opt-in live proof and reuses `ncx-config` for ARK. `p1_temporal_probe live-start/live-result` adds the same paid path under Temporal timer polling. Both paid entry points use the library preflight helper to validate TOS configuration before submitting a paid Seedance job, with regression tests proving ARK resolution is not attempted when TOS is missing. The direct paid smoke and Temporal live submit Activity both resolve TOS/ARK before creating any output directory, SQLite DB, or workflow output state. `p1_paid_preflight_selftest.ps1` temporarily clears TOS env and verifies that the direct paid smoke fails before printing `submitted Seedance task` or creating output, while the Temporal paid harness fails before starting worker/workflow output; it also asserts all cleared TOS env vars and `P1_TEMPORAL_ALLOW_REAL_ARK` are restored before returning, so `-RunPaidLive` can safely run live readiness afterward. `tos.rs` also has fake-transport SigV4 put/get/delete coverage and alias/region tests for live TOS configuration shapes. | Partial: live Seedance/TOS and live Temporal recovery not yet run |
| S07 L0 + fastText gate | `l0.rs` covers structural L0 and language gate. `FastTextModelDetector` loads official fastText `.bin`/`.ftz`; `p1_smoke` verified `lid.176.ftz` detects Chinese as `zh`. | Passed with official `.ftz`; strict `.bin` still optional/local-download dependent |
| S08 validation contract | `validation.rs`, `node.rs`, and dry-run trace verify downstream inputs require passing `validation_records`. The reusable verifier now also checks exported validation record integrity: every validation must have required fields, `verdict` in `pass/repair/escalate`, confidence in `[0,1]`, non-null layers, an artifact_id that exists in the current trace scope, and at most one `pass` record per artifact/stage. | Passed locally |
| S09 structured Agent artifacts | `structured.rs` validates brief/chapters/shots/assets. `dry_run.rs` emits all four artifacts with pass records and hero/tier shot fields. `p1_verify_rough_cut_trace.ps1 -AllowLocalDryRun` now requires the four agent artifacts, their pass self-check records, storyboard continuity fields, valid `tier`, and at least one `is_hero` shot. | Passed locally |
| S10 text/voice separation | `text_separation.rs` adds `no text overlays`, writes SRT, and emits TTS requests; dry-run burns subtitle in FFmpeg and muxes a clearly marked local post-production audio placeholder for shots with TTS requests. The verifier now requires every generation job to carry the `no text overlays` constraint, local dry-run subtitle burn-in, TTS request evidence, and an actual muxed post-production audio file in `assembly_manifest.json`. This proves the separate audio-track assembly path without pretending to be real Chinese TTS. | Passed locally |
| S11 FFmpeg rough cut + partial delivery | `edit.rs` builds `rough_cut.mp4`, `failed_shots.json`, and `assembly_manifest.json`; dry-run rough cut is FFprobe-parseable. `p1_verify_rough_cut_trace.ps1` checks rough_cut duration, failed_shots, assembly manifest, trace, DB, structured agent artifacts in local mode, video artifact pass validations, and that the rough_cut artifact `content_hash` equals the local `rough_cut.mp4` SHA-256. The rough_cut artifact params must resolve to the current output directory's assembly manifest and failed-shots files and set `partial_delivery=true`, so the trace links the partial-delivery evidence instead of relying on stale files elsewhere. The verifier now accepts true partial delivery: a failed shot may lack a video artifact only when it is listed in `failed_shots.json`, has a terminal failed job with `failure_reason`, includes rerun context, and has an `assembly_manifest.json` row without an assembled clip. It also rejects duplicate failed-shot rows, unknown shot ids, delivered video artifacts that are still listed as failed, and `assembly_manifest.json` rows that do not exactly match the trace shot set. Delivered shots must have an existing assembled clip and source clip in the manifest, with subtitle/audio paths required when their burn-in or mux flags are true. `p1_temporal_dry_run_recovery_smoke.ps1` proved rough-cut generation after worker crash/restart and now runs the verifier. The live Seedance/TOS smoke and live Temporal workflow assemble and upload a real-shot rough cut when credentials are available. | Passed locally and via Temporal dry-run; live proof pending |
| S12 full trace | `trace.rs` exports jobs, artifacts, validations, project artifacts, project budget summary, and project-scoped shot traces through `export_project_shot_trace(project_id, shot_id)`; cross-project shot queries are rejected by regression test. Dry-run writes `trace.json` plus `trace_shot_01.json`/`trace_shot_02.json`, each carrying model/provider/params/cost/latency_ms/attempt/failure_reason for the shot jobs. Project-level artifacts now carry `artifacts.project_id`, and trace export filters `shot_id IS NULL` artifacts by the requested project so a shared SQLite DB cannot leak another project's rough cut into the trace. `p1_verify_rough_cut_trace.ps1` enforces exactly one project-level rough_cut artifact, rough_cut params with `assembly_manifest`, `failed_shots`, and `partial_delivery=true` pointing at the current output files, terminal generation job status (`provider_succeeded` or `settled`, or terminal failed status only for declared failed shots), concrete non-negative `latency_ms`, cost/attempt fields, per-shot trace files, validation-record reference/uniqueness integrity, budget reconciliation (`budget_spent == job_cost_total`, `budget_reserved == job_reserved_total`), rough_cut local SHA-256 content hash equality, manifest-to-trace shot-set equality, delivered clip file evidence, media L0 `probe.path`/`size_bytes` binding to real files, and pass validations for every delivered video artifact. Per-shot trace files must exactly match the corresponding project trace shot's `jobs`, `artifacts`, and `validations`, not just have the same counts. Local dry-run video artifacts must have `dry_run_l0` media L0 and a `local-size-N` content hash matching the probed file size. In live mode it also requires `tos://` URIs, ARK/Seedance jobs, `seedance_media_l0` validations whose `tos_roundtrip_path` and `media_l0.probe.path` both resolve to the current `seedance_tos_roundtrip.mp4`, and video artifact `content_hash` equality with that TOS roundtrip file. `p1_verify_rough_cut_trace_selftest.ps1` mutates a known-good dry-run trace to duplicate the rough_cut artifact, to leave a job in `provider_running`, to remove the rough_cut assembly manifest pointer, to point it at the wrong existing file, and to replace the rough_cut `content_hash` with a bogus SHA-256, verifying the strict checker rejects all five; it also rejects media L0 probe size mismatch, local-size content-hash mismatch, mismatched per-shot trace job/artifact content, validation records pointing at unknown artifacts, and duplicate pass validations for one artifact/stage. It synthesizes a valid partial-delivery trace with one failed shot, rejects unknown/duplicate/inconsistent `failed_shots.json` rows, rejects missing/unknown/duplicate manifest rows and delivered manifest rows without assembled clips, synthesizes a strict live-like trace, and verifies that a TOS roundtrip/video artifact hash mismatch is rejected. The self-test now matches raw PowerShell exception fields instead of formatted host text, so expected failure checks are not terminal-width dependent. A unit regression also simulates the strict live Seedance/TOS trace shape without network calls. The live Seedance/TOS smoke and live Temporal workflow write `trace.json` and `trace_shot_01.json` after rough-cut validation. | Passed locally and via Temporal dry-run; live proof pending |

## Latest Smoke Snapshot

On the latest 2026-06-30 non-paid live readiness run, a temporary `temporal server start-dev --ip 127.0.0.1` was started from `rust\target\tools\temporal-cli\extract\temporal.exe`, and `p1_smoke` reached:

```text
PASS  SQLite WAL + JSON1
PASS  Temporal port
PASS  FFmpeg
PASS  OpenCV
PASS  fastText lid
PASS  ARK_API_KEY
FAIL  TOS roundtrip
PASS  VL_API_KEY
PASS  VL_BASE_URL
PASS  VL_MODEL
PASS  ncx-config
```

The OpenCV check used Python `cv2 4.13.0` and performed a tiny BGR-to-gray conversion. The fastText check used the resolved `FASTTEXT_LID_MODEL` and detected Chinese as `zh`. The only failing smoke check is now the live TOS roundtrip because TOS credentials are not present in the environment.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was exercised again on 2026-06-30. It compiled `p1_temporal_probe`, started a temporary Temporal dev server, ran `p1_smoke`, and stopped the server. The preflight failed only at the TOS roundtrip check because TOS credentials were missing; no Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after the rough_cut SHA-256 trace hardening. The external-state result is unchanged: Temporal, FFmpeg, OpenCV, fastText, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` loading passed; live TOS roundtrip failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after the latest LocalOnly acceptance pass. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` all passed. The only failure remained live TOS roundtrip: `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after adding project-scoped shot trace export and job latency recording. It again started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after adding explicit `trace_shot_01.json` checks to the Temporal recovery harnesses. The result was unchanged: SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after adding trace budget reconciliation. The result was unchanged: SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after the partial-delivery verifier hardening. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after hardening failed-shot manifest consistency. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after hardening assembly manifest equality. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after hardening media L0 proof binding. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after hardening per-shot trace equality. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after hardening validation-record integrity. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed; live TOS roundtrip still failed with `TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was exercised again on 2026-06-30. It passed formatting check, 55 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. This is a local/non-paid gate only and is not a P1 completion claim.

The direct paid smoke was also probed without TOS credentials after moving TOS config resolution ahead of ARK submission. It failed immediately with `TOS access key missing...` and did not print `submitted Seedance task`, which guards the manual paid path from spending Seedance budget before object storage is available. The Temporal live submit Activity has the same pre-submit TOS config guard through the shared helper.

The direct paid smoke preflight was rerun with explicit `--submit-real-ark-job` after the latest readiness check, still without TOS credentials. It failed before Seedance submission with `TOS error: TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID` and did not print `submitted Seedance task`.

The paid Temporal live recovery harness was also probed with `P1_TEMPORAL_ALLOW_REAL_ARK=1` but without TOS credentials. It failed immediately at script preflight with `Missing TOS access key env: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, or AWS_ACCESS_KEY_ID`, before starting a worker or workflow.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after aligning the Temporal live submit Activity with the direct paid smoke preflight. It passed formatting, 55 unit tests, binary checks, the deterministic dry-run, the rough-cut trace verifier, the trace verifier negative self-test, the paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun again after hardening the negative self-test's PowerShell error capture. It passed the same local/non-paid gate set. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-526fbf5db16548b1a16b06a4856ba18d`; dry-run workflow `video-agent-p1-dry-run-ccaa1025de5a4c3999d144061c11655c`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-1461558af487401eab1ba773dbb63e6c`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after adding project-scoped shot trace export, per-shot trace files, and concrete job latency recording. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-2f56f819e83d48a5ad9507fa2daffad7`; dry-run workflow `video-agent-p1-dry-run-59fa47bed9134b9f8b1436c91ce67e9f`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e6f36595a4984ebdac699d4482220732`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after making the Temporal recovery harnesses explicitly require `shot_trace=` and `trace_shot_01.json`. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-dd33c5c120fe4b338944f26f0a59418e`; dry-run workflow `video-agent-p1-dry-run-61edf4edea364ac1882ba6e76c244ba0`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e11d554589f4427ea6fd88c64ae55048`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after adding trace budget reconciliation. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-13b259850e8243ed9db7c95e2f341ade`; dry-run workflow `video-agent-p1-dry-run-1edc2a5683374773b5a325fd5a46ba8f`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6db4913a896f442691cb1db047137de1`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after the partial-delivery verifier hardening. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test included a synthetic partial delivery accepted by the verifier with `video_artifacts=1` and `failed_shots_count=1`. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-43fab5dec19a4fdfbf124b0352e8c48a`; dry-run workflow `video-agent-p1-dry-run-707400f7ba8f47d3a7ab18660f37539e`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9e1cad2a28ff40a985d83bdeadde93cc`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after hardening failed-shot manifest consistency. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test still accepts a synthetic partial delivery with `video_artifacts=1` and `failed_shots_count=1`, and now also rejects unknown failed-shot ids, duplicate failed-shot rows, and delivered shots incorrectly listed as failed. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-4abfdacc8284489c8cfa5345364f84c8`; dry-run workflow `video-agent-p1-dry-run-3f4c2cf87f2f40a3bcfe4d773b094f07`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-40a11760e5444a69a79d8d6138cf9c07`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after hardening assembly manifest equality. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test still accepts a synthetic partial delivery and now also rejects manifest rows that are missing, unknown, duplicated, or missing an assembled clip for delivered shots. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-0d2797b2738c41a8aeb4d90f77de1c7d`; dry-run workflow `video-agent-p1-dry-run-24f858893a1f4ebe9d676f0015224029`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-237c4dc0a36c4afeac210630099e0dec`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after hardening media L0 proof binding. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test now rejects media L0 probe size mismatches and local dry-run video artifact `local-size-N` mismatches, while preserving the synthetic partial-delivery and strict live-like checks. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-c108058ea2534227b3f12031e4ecb06b`; dry-run workflow `video-agent-p1-dry-run-6667c97344da487492c9f4eb03d5c27e`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-228d283746ad421b94aa61cabfc225db`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after hardening per-shot trace equality. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test now rejects `trace_shot_01.json` job/artifact content mismatches instead of accepting files with only matching counts. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-71905f45e0f54d46a131346e541fd844`; dry-run workflow `video-agent-p1-dry-run-cb5f534e8dfc4762ae1bd2b5e23bcdb2`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-738261162290410ea8a4093128e9d6e4`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun after hardening validation-record integrity. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. The self-test now rejects validation records pointing at unknown artifacts and duplicate pass validations for the same artifact/stage. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-866b561265e14903924b5aa8e625404e`; dry-run workflow `video-agent-p1-dry-run-060fb1fa1bce4ef4a071601769063cff`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-f5ccdb3f5ec24bdebdd8162183407770`.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` was rerun again as a final local refresh. It passed formatting check, 56 unit tests, binary checks, deterministic dry-run, rough-cut trace verifier, trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery. Latest workflow evidence from that run: crash workflow `video-agent-p1-probe-bd666a28cdde4977a36ee54a28cf8de1`; dry-run workflow `video-agent-p1-dry-run-ec6e38c458474675a84a4840bf78b79b`; dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-acd1d734f0da4f9692f809ad85b3e5b7`.

`p1_live_readiness.ps1 -StartTemporalIfNeeded` was rerun after the final local refresh. It started a temporary Temporal dev server, compile-checked the Temporal probe, and ran `p1_smoke`. SQLite WAL/JSON1, Temporal port, FFmpeg, OpenCV, fastText lid, ARK via `ncx-config`, VL via `ncx-config`, and `ncx-config` passed. The only failure remained live TOS roundtrip: `TOS error: TOS access key missing one of: TOS_ACCESS_KEY_ID, TOS_ACCESS_KEY, AWS_ACCESS_KEY_ID`. No Seedance job was submitted.

Remaining live blockers:

- TOS credentials/env: `TOS_ACCESS_KEY_ID` or `TOS_ACCESS_KEY` or `AWS_ACCESS_KEY_ID`; `TOS_SECRET_ACCESS_KEY` or `TOS_SECRET_KEY` or `AWS_SECRET_ACCESS_KEY`; `TOS_ENDPOINT` or `AWS_ENDPOINT_URL`; `TOS_BUCKET` or `S3_BUCKET`.
- Real paid Seedance/TOS smoke must run with `--submit-real-ark-job`.
- Real paid Temporal Seedance/TOS recovery smoke must run with `P1_TEMPORAL_ALLOW_REAL_ARK=1` and the live recovery script.
- P1 is not complete until live TOS roundtrip and live Seedance -> TOS -> media L0 -> validation contract evidence are captured.

## Temporal Dry-Run Recovery Snapshot

`p1_temporal_crash_recovery_smoke.ps1` and `p1_temporal_dry_run_recovery_smoke.ps1` were rerun on 2026-06-30 against a temporary local Temporal dev server:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-cdeb39307c9146febbbcc05c3493e8b1
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-feb65f54b8174c83b94eae8442ef00d0
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding the trace verifier negative self-test produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-3fc92c2429974332ac59bb278795aed5
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding the OpenCV smoke probe produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-1eb0af3db36b4ca9aae11ef196c52836
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding TOS alias/region tests produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-175745a1117a4d5fa199f3a28d4ff92e
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after requiring terminal job status in trace verification produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-765972dd41fb4da19bfaad0ea87d082c
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding ambiguous submit-state safe failure produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-12f90606c54b4a8ebc0389673e0db597
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after requiring rough_cut trace params to link partial-delivery files produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-a0064f0129624191967c8e4b78458aa3
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after requiring rough_cut params to resolve to the current output files produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-0a61544547834a99b4f6bcdd120a8d9b
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after requiring rough_cut artifact content_hash to match the local file SHA-256 produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-de1a3c1e292e4f179f831ebf0bc84c25
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after requiring live TOS roundtrip/video artifact hash binding produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-b2d4027fdba9412abeaeb6e554d51649
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after hardening temporary SQLite test path uniqueness produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-6bcfa400c5ef4920a55bc79f35531b1a
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding the paid preflight safety self-test produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-30b5e0f115ff41278ed01fcb74687072
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after moving direct paid preflight before output creation produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-ceb6d8e2d9a545e8b6c375493b3f98a7
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding paid preflight environment-restore assertions produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-70ce174fa2304e73b9a8ed1af6311b52
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after aligning Temporal live submit preflight before output state creation produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-54d2868f342f440792583c4cc2aaa57d
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after hardening the negative self-test PowerShell error capture produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-526fbf5db16548b1a16b06a4856ba18d
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after adding project-scoped shot traces and concrete job latency evidence produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-2f56f819e83d48a5ad9507fa2daffad7
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after explicit Temporal shot-trace harness checks produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-dd33c5c120fe4b338944f26f0a59418e
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after trace budget reconciliation produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-13b259850e8243ed9db7c95e2f341ade
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after partial-delivery verifier hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-43fab5dec19a4fdfbf124b0352e8c48a
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after failed-shot manifest consistency hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-4abfdacc8284489c8cfa5345364f84c8
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after assembly manifest equality hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-0d2797b2738c41a8aeb4d90f77de1c7d
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after media L0 proof binding hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-c108058ea2534227b3f12031e4ecb06b
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after per-shot trace equality hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-71905f45e0f54d46a131346e541fd844
task_queue: video-agent-p1-probe
```

The latest local acceptance rerun after validation-record integrity hardening produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-866b561265e14903924b5aa8e625404e
task_queue: video-agent-p1-probe
```

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-6d4e1714a91f4f1d8075474c140fcf0a
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-8d3643fba5d94534b22cc67c42688154\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-8d3643fba5d94534b22cc67c42688154\trace.json
```

The latest local acceptance rerun produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-739076cdcc3940e1ad0f0681d0ad20e7
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b3f2f4f2d23d444aa898fe8bf66b72fd\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b3f2f4f2d23d444aa898fe8bf66b72fd\trace.json
```

The latest local acceptance rerun after adding the trace verifier negative self-test produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-bb4c0bd5dc594475be89a964fa3f25d3
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-29403d47f2af47b7a82d13f1f28ba4af\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-29403d47f2af47b7a82d13f1f28ba4af\trace.json
```

The latest local acceptance rerun after adding the OpenCV smoke probe produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-5e6b8f486d054b579e2e6c50e2f63cc4
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-305f36b933fd4848b4f71ac66da021c4\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-305f36b933fd4848b4f71ac66da021c4\trace.json
```

The latest local acceptance rerun after adding TOS alias/region tests produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-1e3a47e9fb37462a9678b21b528f6df8
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-2c98622a3dee498ea9d0bdbd820720c5\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-2c98622a3dee498ea9d0bdbd820720c5\trace.json
```

The latest local acceptance rerun after requiring terminal job status in trace verification produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-97c0c8c014004da2b27258ad15ac5587
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b705c07e0465405aaf9f3fbd1d37c82c\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b705c07e0465405aaf9f3fbd1d37c82c\trace.json
```

The latest local acceptance rerun after adding ambiguous submit-state safe failure produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-d4f6868e26694cdbbbc1fd6274714469
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9f8f2f72fafc456ca106333a614276d2\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9f8f2f72fafc456ca106333a614276d2\trace.json
```

The latest local acceptance rerun after requiring rough_cut trace params to link partial-delivery files produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-04538b8af134407e9c368e0aac4cf5d3
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6be74160626146c7b3cff87d68a6b81c\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6be74160626146c7b3cff87d68a6b81c\trace.json
```

The latest local acceptance rerun after requiring rough_cut params to resolve to the current output files produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-8b970dd8ddd741d2b904d58f0bdd76db
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-79da53381f8f419d92c912a20b5a4a82\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-79da53381f8f419d92c912a20b5a4a82\trace.json
```

The latest local acceptance rerun after requiring rough_cut artifact content_hash to match the local file SHA-256 produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-ab23e540cf3e47cd988a167534c5c6a9
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-15c064d8570b49d2b05a6ffde77193e0\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-15c064d8570b49d2b05a6ffde77193e0\trace.json
```

The latest local acceptance rerun after requiring live TOS roundtrip/video artifact hash binding produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-368de880f3e9491a9678eb75b1b1cfd0
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-82ca82137b6a49f8b814c656a1cbc4fb\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-82ca82137b6a49f8b814c656a1cbc4fb\trace.json
```

The latest local acceptance rerun after hardening temporary SQLite test path uniqueness produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-131b6f005b80422d921bcd973458d7a5
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6017a0b56c2c419c833b12f053085445\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6017a0b56c2c419c833b12f053085445\trace.json
```

The latest local acceptance rerun after adding the paid preflight safety self-test produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-af383600e4ce4c539dc7bcad6858fd43
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-2e65575eb6a64290b1f7b64f83d960d5\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-2e65575eb6a64290b1f7b64f83d960d5\trace.json
```

The latest local acceptance rerun after moving direct paid preflight before output creation produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-ce63f6f0513b44a1811071dc9f1a2d76
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-98b4d52e81d648b19d15f3b045294561\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-98b4d52e81d648b19d15f3b045294561\trace.json
```

The latest local acceptance rerun after adding paid preflight environment-restore assertions produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-9d4673d82e6d409c95656a3266a4c02a
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b93b9e3e297e4e78ba7f8037b00a4203\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b93b9e3e297e4e78ba7f8037b00a4203\trace.json
```

The latest local acceptance rerun after aligning Temporal live submit preflight before output state creation produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-589aec1075854a50a196df2acf81ee4b
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-5462f77186f446edb35fc0c416cdde2d\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-5462f77186f446edb35fc0c416cdde2d\trace.json
```

The latest local acceptance rerun after hardening the negative self-test PowerShell error capture produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-ccaa1025de5a4c3999d144061c11655c
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-1461558af487401eab1ba773dbb63e6c\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-1461558af487401eab1ba773dbb63e6c\trace.json
```

The latest local acceptance rerun after adding project-scoped shot traces and concrete job latency evidence produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-59fa47bed9134b9f8b1436c91ce67e9f
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e6f36595a4984ebdac699d4482220732\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e6f36595a4984ebdac699d4482220732\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e6f36595a4984ebdac699d4482220732\trace_shot_01.json
```

The latest local acceptance rerun after explicit Temporal shot-trace harness checks produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-61edf4edea364ac1882ba6e76c244ba0
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e11d554589f4427ea6fd88c64ae55048\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e11d554589f4427ea6fd88c64ae55048\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e11d554589f4427ea6fd88c64ae55048\trace_shot_01.json
```

The latest local acceptance rerun after trace budget reconciliation produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-1edc2a5683374773b5a325fd5a46ba8f
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6db4913a896f442691cb1db047137de1\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6db4913a896f442691cb1db047137de1\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6db4913a896f442691cb1db047137de1\trace_shot_01.json
```

The latest local acceptance rerun after partial-delivery verifier hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-707400f7ba8f47d3a7ab18660f37539e
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9e1cad2a28ff40a985d83bdeadde93cc\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9e1cad2a28ff40a985d83bdeadde93cc\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9e1cad2a28ff40a985d83bdeadde93cc\trace_shot_01.json
```

The latest local acceptance rerun after failed-shot manifest consistency hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-3f4c2cf87f2f40a3bcfe4d773b094f07
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-40a11760e5444a69a79d8d6138cf9c07\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-40a11760e5444a69a79d8d6138cf9c07\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-40a11760e5444a69a79d8d6138cf9c07\trace_shot_01.json
```

The latest local acceptance rerun after assembly manifest equality hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-24f858893a1f4ebe9d676f0015224029
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-237c4dc0a36c4afeac210630099e0dec\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-237c4dc0a36c4afeac210630099e0dec\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-237c4dc0a36c4afeac210630099e0dec\trace_shot_01.json
```

The latest local acceptance rerun after media L0 proof binding hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-6667c97344da487492c9f4eb03d5c27e
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-228d283746ad421b94aa61cabfc225db\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-228d283746ad421b94aa61cabfc225db\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-228d283746ad421b94aa61cabfc225db\trace_shot_01.json
```

The latest local acceptance rerun after per-shot trace equality hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-cb5f534e8dfc4762ae1bd2b5e23bcdb2
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-738261162290410ea8a4093128e9d6e4\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-738261162290410ea8a4093128e9d6e4\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-738261162290410ea8a4093128e9d6e4\trace_shot_01.json
```

The latest local acceptance rerun after validation-record integrity hardening produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-060fb1fa1bce4ef4a071601769063cff
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-f5ccdb3f5ec24bdebdd8162183407770\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-f5ccdb3f5ec24bdebdd8162183407770\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-f5ccdb3f5ec24bdebdd8162183407770\trace_shot_01.json
```

The refreshed dry-run `rough_cut.mp4` is FFprobe-parseable with duration `0.820996`, includes a post-production audio stream in the local dry-run path, and its `trace.json` contains project artifact `artifact_rough_cut` with `rough_cut_media_l0=pass` and a SHA-256 `content_hash` matching the local rough cut bytes. The output now also includes per-shot trace files such as `trace_shot_01.json`, and the reusable verifier checks each job's `model`, `params`, `cost`, concrete `latency_ms`, `attempt`, and `failure_reason` fields.

The refreshed Temporal dry-run output also passed the reusable verifier inside the recovery harness:

```text
PASS P1 rough-cut trace verification
out_dir: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-8d3643fba5d94534b22cc67c42688154
project_id: project_p1_dry_run
rough_cut_duration_s: 0.820996
shots: 2
jobs: 2
video_artifacts: 2
failed_shots_count: 0
```

Latest verifier output:

```text
PASS P1 rough-cut trace verification
out_dir: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-b3f2f4f2d23d444aa898fe8bf66b72fd
project_id: project_p1_dry_run
rough_cut_duration_s: 0.820996
shots: 2
jobs: 2
video_artifacts: 2
failed_shots_count: 0
```

Latest verifier output after adding the negative self-test:

```text
PASS P1 rough-cut trace verification
out_dir: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-29403d47f2af47b7a82d13f1f28ba4af
project_id: project_p1_dry_run
rough_cut_duration_s: 0.820996
shots: 2
jobs: 2
video_artifacts: 2
failed_shots_count: 0
```
