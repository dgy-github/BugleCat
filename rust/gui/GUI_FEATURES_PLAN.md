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
  (memory_list / memory_consolidate / memory_add). LLM-merge still TODO (needs
  the model via the bridge).
- **P3 ✅ DONE (`c1c0801`):** file/image upload — tauri-plugin-dialog picker;
  Command::Prompt { text, images }; images→vision multimodal, files→@mention.
- **P2 ✅ DONE (`df06b9e`):** session resume + fork. History panel Resume/⑂ Fork;
  bridge Command::{Resume, Fork} reseed via Session::fork; `loaded` event replays
  the transcript. (Drive needs a saved snapshot — send a turn first.)
- **Hermes trigger (pending integration):** forge's optimizer loop (M0b) AND M1
  (split/TaskGen/noise-aware accept) are ALREADY built on `feat/train`
  (forge.py train(), teacher.py panel incl. codex, genome.py, splits.py,
  taskgen.py) and run end-to-end. To wire the GUI "Hermes" button, feat/train +
  feat/gui must first land on rust-capability; then a Tauri command shells
  `python train/forge.py` and streams progress.
- **P4:** orchestrate toggle + plan/usage panels (bridge + shared Arc<Mutex>
  snapshot of the plan/usage the !Send agent thread holds). Plus Hermes LLM-merge.

## Notes
- Frontend is Svelte 5 (runes). Follow App.svelte's existing modal pattern
  (settings/checkpoints): a toolbar button opens an overlay; invoke() calls the
  command; results render in the modal.
- git commands shell out via std::process::Command in the current_dir workspace.
