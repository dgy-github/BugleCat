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

- **P1 (this branch, first):** git branches + git diff + session history LIST
  (read-only display). No bridge. Verifiable by build+launch.
- **P2:** session resume + fork (bridge Command::{Resume, Fork}).
- **P3:** file/image upload (bridge Command::Prompt { text, images }).
- **P4:** orchestrate toggle + plan/usage panels (bridge + shared snapshot).

## Notes
- Frontend is Svelte 5 (runes). Follow App.svelte's existing modal pattern
  (settings/checkpoints): a toolbar button opens an overlay; invoke() calls the
  command; results render in the modal.
- git commands shell out via std::process::Command in the current_dir workspace.
