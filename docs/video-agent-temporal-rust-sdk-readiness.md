# Video Agent P1 Temporal Rust SDK Readiness

Status date: 2026-07-01

## Sources Checked

- `cargo info temporalio-sdk` resolved official crate `temporalio-sdk = 0.5.0`.
- The crate metadata points to `https://github.com/temporalio/sdk-rust`, `https://temporal.io/`, and `https://docs.rs/temporalio-sdk/0.5.0`.
- Local crate README from Cargo registry states the Rust SDK is Public Preview and under active development.

## Required P1/S03 Semantics

- Durable workflow execution and replay after worker crash.
- Activities for side effects.
- Timers for long polling instead of blocking an Activity.
- Signals or updates for human gates.
- Determinism guardrails so workflow code does not use raw Tokio I/O, timers, spawn, random, or system time.

## Evidence

- Workflow and activity macros exist: `#[workflow]`, `#[workflow_methods]`, `#[activities]`, `#[activity]`.
- Worker registration exists through `WorkerOptions::register_workflow` and `register_activities`.
- Workflow timers exist via `ctx.timer(Duration)`.
- Workflow state waits exist via `ctx.wait_condition`.
- Signals, queries, and updates are documented through `#[signal]`, `#[query]`, `#[update]`, plus typed client calls.
- Deterministic workflow helpers exist: `temporalio_sdk::workflows::select!`, `join!`, and `join_all`.
- Runtime nondeterminism detection is enabled by default and flags raw Tokio timers, async I/O, `tokio::spawn`, and direct async channels inside workflows.

## Local Compile Proof

The feature-gated P1 Temporal probe compiles locally when `PROTOC` is provided:

```powershell
$env:PROTOC='C:\Users\jingc\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\protoc-bin-vendored-win32-3.2.0\bin\protoc.exe'
cargo check -p ncx-video-agent --features temporal --bin p1_temporal_probe
```

Result on 2026-06-30:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s)
```

Without `PROTOC`, `temporalio-protos` / `prost-wkt-types` build scripts fail before the probe can compile.

## Repeatable Crash-Recovery Harness

`rust/crates/ncx-video-agent/scripts/p1_temporal_crash_recovery_smoke.ps1` automates the S03 acceptance sequence against a real `temporal server start-dev`:

- check `127.0.0.1:7233` is reachable,
- start the P1 probe worker,
- start a unique workflow id,
- kill the first worker process,
- restart the worker,
- send the approval signal for the human gate,
- wait for workflow completion and require `shot_01:dry-temporal-job-shot_01:approved`.

Command:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_temporal_crash_recovery_smoke.ps1
```

The probe accepts these optional env overrides, used by the harness for repeatability:

- `P1_TEMPORAL_TASK_QUEUE`
- `P1_TEMPORAL_WORKFLOW_ID`
- `P1_TEMPORAL_SHOT_ID`

Local run on 2026-06-30 first failed because no Temporal dev server was reachable:

```text
Temporal dev server is not reachable at 127.0.0.1:7233. Start it with: temporal server start-dev
```

After installing the official Temporal CLI locally under `rust/target/tools/temporal-cli`, `temporal server start-dev --ip 127.0.0.1` was started and the repeatable harness passed. It was rerun on 2026-06-30 during the resumed implementation pass:

```text
temporal version 1.7.2 (Server 1.31.1, UI 2.49.1)
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

The latest local acceptance refresh produced:

```text
PASS P1 Temporal crash-recovery smoke
workflow_id: video-agent-p1-probe-bd666a28cdde4977a36ee54a28cf8de1
task_queue: video-agent-p1-probe
```

This proves the P1/S03 minimum for the Rust SDK in this environment: Activity side effects, workflow timer polling, worker crash/restart recovery, and signal-driven human gate resume.

## Temporal P1 Dry-Run Recovery Harness

`rust/crates/ncx-video-agent/scripts/p1_temporal_dry_run_recovery_smoke.ps1` extends the Temporal proof from a minimal signal workflow to a P1 dry-run project workflow:

- start the shared P1 probe worker,
- start `P1DryRunWorkflow` with a unique workflow id and output directory,
- wait for a prepare Activity marker on disk,
- kill the first worker while the workflow is waiting on a Temporal timer,
- restart the worker,
- wait for workflow result,
- require `rough_cut.mp4` and `trace.json` in the output directory.
- run `p1_verify_rough_cut_trace.ps1 -AllowLocalDryRun` against the output directory.

Local run on 2026-06-30 passed and was refreshed during the resumed implementation pass:

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

The latest local acceptance refresh produced:

```text
PASS P1 Temporal dry-run recovery smoke
workflow_id: video-agent-p1-dry-run-ec6e38c458474675a84a4840bf78b79b
task_queue: video-agent-p1-probe
rough_cut: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-acd1d734f0da4f9692f809ad85b3e5b7\rough_cut.mp4
trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-acd1d734f0da4f9692f809ad85b3e5b7\trace.json
shot_trace: C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-acd1d734f0da4f9692f809ad85b3e5b7\trace_shot_01.json
```

The refreshed dry-run `rough_cut.mp4` is FFprobe-parseable with duration `0.820996`, includes a local post-production audio stream, and the exported trace has `artifact_rough_cut` with `rough_cut_media_l0=pass` plus a SHA-256 `content_hash` matching the local rough cut bytes.

The same output also passed the reusable rough-cut trace verifier inside the recovery harness, which checks the output database, media duration, failed_shots/assembly manifest files, local structured agent artifact validations, text-separation evidence, project rough_cut validation, shot video artifact validations, project trace shape, budget reconciliation, per-shot trace files, and S12 job fields including `model`, `params`, `cost`, concrete `latency_ms`, `attempt`, and `failure_reason`.

This proves a local P1 rough-cut workflow can survive worker crash/restart through Temporal. It still does not prove the paid Seedance/TOS workflow path; that remains gated on live TOS credentials and explicit `--submit-real-ark-job`.

## Temporal Live Seedance/TOS Workflow

`p1_temporal_probe` also registers `P1LiveSeedanceWorkflow`, which keeps the P1/S06 long-job boundary inside Temporal:

- `submit_live_seedance_job` Activity validates TOS/ARK prerequisites before creating output state, then creates the SQLite project scaffold, reserves budget through `jobs`, submits a real Seedance job once, and writes a submit marker for recovery evidence.
- The workflow polls with `ctx.timer(Duration::from_secs(6))` between `poll_live_seedance_job` Activity calls. The Activity only performs one provider poll per invocation.
- A successful poll settles budget when ARK usage reports `total_tokens`.
- `persist_live_seedance_outputs` downloads the Seedance result, uploads it to TOS, performs a TOS roundtrip media L0 check, records `validation_records`, assembles `rough_cut.mp4`, uploads the rough cut to TOS, records project-level validation, and exports both `trace.json` and project-scoped `trace_shot_01.json`.
- The paid path is double-gated: use `live-start` explicitly and set `P1_TEMPORAL_ALLOW_REAL_ARK=1`.

Manual commands:

```powershell
$env:P1_TEMPORAL_ALLOW_REAL_ARK='1'
$env:PROTOC='C:\Users\jingc\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\protoc-bin-vendored-win32-3.2.0\bin\protoc.exe'
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- worker
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- live-start
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- live-result
```

Paid crash-recovery harness:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly
$env:P1_TEMPORAL_ALLOW_REAL_ARK='0'
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_live_readiness.ps1 -StartTemporalIfNeeded
$env:P1_TEMPORAL_ALLOW_REAL_ARK='1'
powershell -NoProfile -ExecutionPolicy Bypass -File .\rust\crates\ncx-video-agent\scripts\p1_temporal_live_seedance_recovery_smoke.ps1
```

The `p1_acceptance.ps1 -LocalOnly` command is the repeatable non-paid Temporal/local gate runner. Full paid completion evidence is `p1_acceptance.ps1 -StartTemporalIfNeeded -RunPaidLive`, which runs local gates, live readiness, the direct paid Seedance/TOS smoke, and the paid Temporal live recovery smoke.

`p1_acceptance.ps1 -StartTemporalIfNeeded -LocalOnly` passed on 2026-06-30, including 55 unit tests, the trace verifier negative self-test, paid preflight safety self-test, Temporal crash recovery, and Temporal dry-run recovery with the reusable rough-cut trace verifier. It intentionally skips live TOS readiness and paid Seedance/TOS proof.

The latest `-LocalOnly` rerun after aligning Temporal live submit preflight passed with crash workflow `video-agent-p1-probe-54d2868f342f440792583c4cc2aaa57d` and dry-run workflow `video-agent-p1-dry-run-589aec1075854a50a196df2acf81ee4b`.

The latest `-LocalOnly` rerun after hardening the negative self-test PowerShell error capture passed with crash workflow `video-agent-p1-probe-526fbf5db16548b1a16b06a4856ba18d`, dry-run workflow `video-agent-p1-dry-run-ccaa1025de5a4c3999d144061c11655c`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-1461558af487401eab1ba773dbb63e6c`.

The latest `-LocalOnly` rerun after adding project-scoped shot trace export and concrete job latency recording passed with 56 unit tests, crash workflow `video-agent-p1-probe-2f56f819e83d48a5ad9507fa2daffad7`, dry-run workflow `video-agent-p1-dry-run-59fa47bed9134b9f8b1436c91ce67e9f`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e6f36595a4984ebdac699d4482220732`.

The latest `-LocalOnly` rerun after explicit Temporal shot-trace harness checks passed with 56 unit tests, crash workflow `video-agent-p1-probe-dd33c5c120fe4b338944f26f0a59418e`, dry-run workflow `video-agent-p1-dry-run-61edf4edea364ac1882ba6e76c244ba0`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-e11d554589f4427ea6fd88c64ae55048`.

The latest `-LocalOnly` rerun after trace budget reconciliation passed with 56 unit tests, crash workflow `video-agent-p1-probe-13b259850e8243ed9db7c95e2f341ade`, dry-run workflow `video-agent-p1-dry-run-1edc2a5683374773b5a325fd5a46ba8f`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-6db4913a896f442691cb1db047137de1`.

The latest `-LocalOnly` rerun after partial-delivery verifier hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-43fab5dec19a4fdfbf124b0352e8c48a`, dry-run workflow `video-agent-p1-dry-run-707400f7ba8f47d3a7ab18660f37539e`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-9e1cad2a28ff40a985d83bdeadde93cc`.

The latest `-LocalOnly` rerun after failed-shot manifest consistency hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-4abfdacc8284489c8cfa5345364f84c8`, dry-run workflow `video-agent-p1-dry-run-3f4c2cf87f2f40a3bcfe4d773b094f07`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-40a11760e5444a69a79d8d6138cf9c07`.

The latest `-LocalOnly` rerun after assembly manifest equality hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-0d2797b2738c41a8aeb4d90f77de1c7d`, dry-run workflow `video-agent-p1-dry-run-24f858893a1f4ebe9d676f0015224029`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-237c4dc0a36c4afeac210630099e0dec`.

The latest `-LocalOnly` rerun after media L0 proof binding hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-c108058ea2534227b3f12031e4ecb06b`, dry-run workflow `video-agent-p1-dry-run-6667c97344da487492c9f4eb03d5c27e`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-228d283746ad421b94aa61cabfc225db`.

The latest `-LocalOnly` rerun after per-shot trace equality hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-71905f45e0f54d46a131346e541fd844`, dry-run workflow `video-agent-p1-dry-run-cb5f534e8dfc4762ae1bd2b5e23bcdb2`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-738261162290410ea8a4093128e9d6e4`.

The latest `-LocalOnly` rerun after validation-record integrity hardening passed with 56 unit tests, crash workflow `video-agent-p1-probe-866b561265e14903924b5aa8e625404e`, dry-run workflow `video-agent-p1-dry-run-060fb1fa1bce4ef4a071601769063cff`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-f5ccdb3f5ec24bdebdd8162183407770`.

The latest `-LocalOnly` refresh passed with 56 unit tests, crash workflow `video-agent-p1-probe-bd666a28cdde4977a36ee54a28cf8de1`, dry-run workflow `video-agent-p1-dry-run-ec6e38c458474675a84a4840bf78b79b`, and dry-run output `C:\Users\jingc\AppData\Local\Temp\ncx-video-agent-temporal-dry-run-acd1d734f0da4f9692f809ad85b3e5b7`.

Run the readiness preflight first; it does not submit Seedance, but it does compile the Temporal probe and require `p1_smoke` to pass, including live TOS roundtrip when credentials are present.

The harness starts the worker, starts the live workflow, waits for a real `temporal_live_poll_marker.json` with `kind=running`, kills the worker, restarts it, and waits for `rough_cut.mp4`, `trace.json`, and `trace_shot_01.json`.
It then runs `p1_verify_rough_cut_trace.ps1` in strict live mode, requiring `tos://` artifact URIs, ARK/Seedance jobs, project/shot trace files, concrete S12 job fields, budget reconciliation, and pass validations.

This live Temporal path has been compiled locally but not executed end to end because TOS credentials are missing and the run is paid.

## Decision

Proceed with a minimal S03 proof using `temporalio-sdk = 0.5.0`, but keep the risk explicit:

- The SDK is Public Preview, so API churn is expected.
- The first S03 implementation has passed as a small crash-recovery smoke workflow before ARK/TOS workflow integration.
- Keep ARK/TOS long-job integration on Temporal timers and activities; do not replace Temporal with a local queue.
- The live Seedance/TOS Temporal workflow is implemented and compile-checked; P1 still requires a paid live recovery run before completion can be claimed.

## Local Probe Added

`ncx-video-agent` now has a `temporal` feature and `p1_temporal_probe` binary. It intentionally does not run by default. It compiles a minimal workflow with:

- one submit Activity,
- timer-based polling Activity loop,
- `wait_condition` human gate,
- typed approval signal,
- typed query.

Manual commands once a Temporal dev server is available:

```powershell
$env:PROTOC='C:\Users\jingc\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\protoc-bin-vendored-win32-3.2.0\bin\protoc.exe'
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- worker
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- start
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- signal
cargo run -p ncx-video-agent --features temporal --bin p1_temporal_probe -- result
```

Crash-recovery acceptance is proved for both the minimal S03 probe and the local P1 dry-run rough-cut workflow as of 2026-06-30. Full P1 still needs the Seedance/TOS path run with live credentials.

Manual S03 crash-recovery smoke:

1. Start Temporal dev server: `temporal server start-dev`.
2. Terminal A: run the `worker` command above.
3. Terminal B: run the `start` command above and confirm a `run_id` is printed.
4. Stop Terminal A's worker process.
5. Terminal A: run the `worker` command again.
6. Terminal B: run the `signal` command above.
7. Terminal B: run the `result` command above and require `shot_01:dry-temporal-job-shot_01:approved`.
