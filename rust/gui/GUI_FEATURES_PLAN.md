# GUI feature-parity plan (feat/gui)

Bring the Tauri GUI closer to the CLI/core. Branch `feat/gui` off rust-capability.
Built in an isolated worktree (the shared checkout is being thrashed by parallel
sessions). Each GUI build: `npm run build` (frontend) then
`cargo build --release --features tauri/custom-protocol` (the feature is
required or the app loads the dev URL — see the blank-page gotcha).

## Difficulty split

**No bridge** (plain Tauri commands in lib.rs — fully self-contained):
- git branches: list / current / create+switch / switch
- git diff (working tree)
- list sessions (SessionIndex::default().entries())

**Bridge surgery** (agent thread owns agent/session — needs new Command variants):
- resume session  → rebuild agent seeded from SessionIndex snapshot
- fork session    → Session::fork into a new log/id from a snapshot
- file/image upload → Prompt carries attachments; images reuse the vision
  multimodal content path, files reuse @mention expansion
- orchestrate mode → a different run path (Orchestrator vs AgentLoop.run_turn)
- live plan / usage → state lives on the !Send thread; expose via a shared
  Arc<Mutex<Snapshot>> the thread updates each turn, read by a Tauri command

## Phases

- **P1 ✅ DONE (`56301f8`):** git branches + git diff + session history LIST.
- **Hermes ✅ DONE (`5b9b618`):** project-memory self-evolution panel
  (memory_list / memory_consolidate / memory_add). Deterministic near-duplicate
  consolidation and model-backed consolidation are shipped. The LLM merger is
  a shared ncx-core consumer with an injected Harness Provider. GUI runs it as
  a versioned App Server background operation with status polling, cooperative
  HTTP cancellation, byte-baseline conflict detection and zero partial writes.
- **P3 ✅ DONE (`c1c0801`):** file/image upload — tauri-plugin-dialog picker;
  Command::Prompt { text, images }; images→vision multimodal, files→@mention.
- **P2 ✅ DONE (`df06b9e`):** session resume + fork. History panel Resume/⑂ Fork;
  bridge Command::{Resume, Fork} reseed via Session::fork; `loaded` event replays
  the transcript. (Drive needs a saved snapshot — send a turn first.)
- **Hermes trigger ✅ DONE:** forge's optimizer loop (M0b) AND M1
  (split/TaskGen/noise-aware accept) are already present in this worktree and
  run end-to-end. The old cross-branch merge prerequisite is obsolete. Forge
  subprocesses now use an owned process-tree runner, so timeout kills ncx,
  teacher and grader descendants; host-provided output directories isolate
  genomes/lineage under a workspace instead of mutating `train/`. Remaining
  GUI integration is complete: the Forge overlay bundles a pinned/hash-checked
  embeddable Python, release ncx sidecar and minimal scripts/tasks; App Server
  owns a typed background job; the panel exposes only bounded
  `rounds/repeats/timeout/budget/teacher/accept-margin`, requires an explicit
  cost confirmation, supports cancellation and renders a whitelisted lineage
  summary. Windows jobs use a kill-on-close Job Object so detached descendants
  cannot escape cancellation. `--no-gate`, arbitrary task/path arguments and
  raw stdout/stderr remain intentionally unavailable.
- **P4 ✅ DONE (protocol-first):** real Agent / Orchestrator runtime mode. The
  old “toggle + shared snapshot” note is insufficient and must not be shipped
  as a prompt-template shortcut. Required gates:
  1. Move the live `AgentRunner` out of the CLI-only ownership boundary so CLI
     and GUI use one implementation. Worker-copy failure is fail-closed; no
     parallel worker may fall back to the real workspace.
  2. Extend the core orchestration contract with typed node progress,
     cooperative cancellation and aggregated usage/model evidence. A cancelled
     classify/plan/worker/verify node must stop scheduling later nodes.
  3. Add an explicit execution mode to the App Server turn-start boundary and
     persisted turn projection. Thread ID, Turn ID, visible transcript,
     approvals/questions, artifacts, cancellation and final status remain owned
     by the existing protocol lifecycle.
  4. Project orchestration events into the existing GUI trajectory instead of
     creating a second frontend Agent state machine. Switching mode affects the
     next turn in the current conversation and never clears its transcript.
  5. Prove ordinary and orchestrated turns with Core tests, App Server contract
     tests and a real WebView E2E covering mode switch, progress, cancellation,
     transcript preservation and history reload.

  Completed with protocol v3 `ExecutionMode`, the shared `HarnessAgentRunner`,
  typed stage/activity events, cooperative cancellation, usage/model evidence,
  fail-closed Worker isolation, three-way result promotion, GUI mode switching,
  trajectory projection and focused Core/App Server/GUI/WebView tests. Worker
  images remain explicitly unsupported; native multimodal turns stay on Agent.

## Notes
- **P4 ✅ DONE (session profiles):** Harness Profile is now durable per Thread.
  Empty conversations can select full/coding/readonly/minimal/headless, the
  first Turn locks the selection, Resume restores it, Fork inherits it, and
  Agent/Orchestrator runtimes receive the explicit composition without a
  process-global environment race. The selector rebuilds an empty active Agent
  immediately; backend validation rejects unknown profiles before persistence.
- Frontend is Svelte 5 (runes). Follow App.svelte's existing modal pattern
  (settings/checkpoints): a toolbar button opens an overlay; invoke() calls the
  command; results render in the modal.
- git commands shell out via std::process::Command in the current_dir workspace.
