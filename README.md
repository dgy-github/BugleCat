# BugleCat

English | [简体中文](README.zh-CN.md)

**BugleCat** is a local, extensible Codex-style coding agent. A
chat-completions model proposes structured tool calls; the agent runs
file/shell/LSP/terminal tools behind a sandbox and approval policy, records the
session and event snapshots, and loops until the task is done. It runs against
DeepSeek's hosted API or any OpenAI-compatible local model, and ships with MCP
integration, a plugin ecosystem, a skills system, context compaction,
token-cost accounting, a scheduler, git-worktree A/B comparison, and a Tauri 2 +
Svelte 5 desktop client.

[![BugleCat desktop screenshot: Tauri + Svelte desktop client](assets/buglecat-desktop-real.png)](nanocodex.html)

[Capability page](nanocodex.html) · [Changelog](CHANGELOG.md) · [Releases](https://github.com/dgy-github/BugleCat/releases/latest) · [Design brief (PDF)](docs/ai-coding-agent-design-brief.pdf) / [HTML](docs/ai-coding-agent-design-brief.html)

Design documents (Chinese):
[design philosophy](docs/design-philosophy.zh-CN.md) ·
[context-compression safety protocol](docs/context-compression-safety-protocol.zh-CN.md) ·
[intelligence compounding & 60-point singularity](docs/intelligence-compounding-feedback-plan.zh-CN.md) ·
[self-evolving harness (RRSI & Harness²)](docs/self-evolving-harness-rrsi-harness2.zh-CN.md) ·
[Harness² in forge](docs/harness2-forge-integration.zh-CN.md)

## Status at a Glance

- **Current line:** the Rust runtime under `rust/` (`ncx.exe` CLI + Tauri
  desktop app), now in Stage 3 (intelligence compounding). The Python
  `nanocodex/` package is kept as the legacy reference implementation; the old
  name survives in `~/.nanocodex/`, `NANOCODEX_*` variables, and Python commands
  for compatibility.
- **Latest release:** [`v0.1.2`](https://github.com/dgy-github/BugleCat/releases/tag/v0.1.2)
  — Windows installer and portable bundle, macOS DMGs.
- **Latest changes (2026-10-23):** post-review fixes to shell approval, process
  containment, Propose–Probe–Compose, forge acceptance, and memory guardrails —
  see the [Changelog](CHANGELOG.md).
- **Defaults worth knowing:** `sandbox_mode = "workspace-write"`,
  `approval_policy = "on-request"`. Online compounding feedback
  (`[feedback] memory` / `routing`) defaults to `"shadow"`: it observes and
  logs counterfactuals and review candidates, but writes no learnings and
  changes no routes. Switching to `"on"` is gated by a 14-day baseline.
- **Known limits:** the sandbox is policy-level, not OS-level filesystem or
  network isolation; MCP servers and hooks run outside it; the compounding
  replay cost result is preliminary (9 tasks).

## Table of Contents

- [Install](#install)
- [Quick Start](#quick-start)
- [Highlights](#highlights)
- [Architecture](#architecture)
- [Project Phases](#project-phases)
- [Tools](#tools)
- [Configuration](#configuration)
- [Sandbox & Approval](#sandbox--approval)
- [Intelligence Compounding Loop](#intelligence-compounding-loop)
- [Self-Evolving Harness](#self-evolving-harness)
- [Evaluation & Evidence](#evaluation--evidence)
- [Custom Slash Commands](#custom-slash-commands)
- [Local Model / OpenAI-Compatible Endpoint](#local-model--openai-compatible-endpoint)
- [MCP](#mcp)
- [Plugin Ecosystem](#plugin-ecosystem)
- [Skills](#skills)
- [Memory & AGENTS.md](#memory--agentsmd)
- [Sessions, Resume & History](#sessions-resume--history)
- [Context Compaction](#context-compaction)
- [Token Usage & Cost](#token-usage--cost)
- [Scheduler](#scheduler)
- [A/B Worktree Comparison](#ab-worktree-comparison)
- [BugleCat Desktop GUI](#buglecat-desktop-gui)
- [Tests](#tests)
- [Release Packaging](#release-packaging)
- [Security Notes](#security-notes)
- [Collaboration](#collaboration)
- [License](#license)

## Install

### Option 1: Prebuilt Releases (Recommended)

Download directly from [GitHub Releases (`v0.1.2`)](https://github.com/dgy-github/BugleCat/releases/latest):

- **Windows NSIS Installer**: `BugleCat_0.1.0_x64-setup.exe`
- **Windows Full Portable Bundle (includes `BugleCat.exe`, `ncx.exe`, and offline verification tools)**: `buglecat-20261014-final.zip`
- **macOS Disk Images**: `BugleCat-arm64.dmg` (Apple Silicon) / `BugleCat-x64.dmg` (Intel)

### Option 2: Build from Source

Current Rust release line:

```powershell
cd path\to\BugleCat
rustup toolchain install stable-x86_64-pc-windows-msvc
cargo +stable-x86_64-pc-windows-msvc build --manifest-path rust\Cargo.toml -p ncx-cli --release

cd rust\gui
npm ci
npm run tauri:build
```

This requires the Rust toolchain and Node.js 18 or newer. The original Python
implementation remains available for reference and prototyping:

```powershell
python -m pip install -e ".[dev]"
```

The Python line requires Python 3.11 or newer.

## Quick Start

Rust CLI, current release line:

```powershell
cd rust
cargo run -p ncx-cli -- "summarize this repository"
cargo run -p ncx-cli
cargo run -p ncx-cli -- --resume
cargo run -p ncx-cli -- --history
cargo run -p ncx-cli -- --memory-merge
cargo run -p ncx-cli -- --outcomes report --since 30d --by category,model,policy
cargo run -p ncx-cli -- --outcomes bootstrap D:\repo1 D:\repo2  # backfill historical sessions into global ACR store
cargo run -p ncx-cli -- --memory-review                         # list | confirm <id> | reject <id> | restore <id>

# Tauri desktop app
cd gui
npm ci
npm.cmd run tauri -- dev --target x86_64-pc-windows-msvc
```

Inside the Rust REPL, `/config` shows the resolved config file path, current
model/sandbox/approval values, and writable keys. `/config key=value` persists a
setting to `~/.nanocodex/config.toml` (switching `feedback.memory=on` or `feedback.routing=on` enforces the 14-day Shadow baseline gate unless `--force` is passed); restart the REPL for provider, model,
sandbox, or budget changes to affect the active session. `/usage` (or `/cost`)
shows raw token usage for the last turn and current REPL session; `/outcome good|bad [note]` records an explicit human label for the latest primary task, and `/outcome report` prints the workspace ACR & 60-point singularity report.

Legacy Python CLI (the command name remains `nanocodex` for compatibility):

```powershell
# one-shot task
nanocodex "add a --json flag to the CLI"

# interactive, in the current directory
nanocodex --cd .

# with MCP servers enabled
nanocodex --mcp

# the GUI
nanocodex-gui --cd .
```

For the legacy Python GUI, Windows users can also double-click `nanocodex-gui.cmd` after installation, or
generate a Start-menu shortcut with `scripts/make-shortcut.ps1`.

## Highlights

- **Codex-style agent loop** — streaming token output, multi-round tool calls,
  cancellation, and per-turn usage accounting.
- **DeepSeek + any OpenAI-compatible backend** — point `base_url` at the hosted
  API or a local server (vLLM, llama-server, LM Studio, …).
- **Sandbox & approval state machine** — three sandbox modes and four approval
  policies gate every file/shell/network action.
- **MCP integration + marketplace** — load servers from `mcp.toml`, or install
  from a built-in / remote catalog; tools retain the names supplied by their
  servers, and duplicate names are rejected.
  `/mcp reload` prepares and atomically swaps the active external tool set in
  the same process.
- **Plugin ecosystem** — install Codex-compatible resource plugins that bundle
  Skills, MCP servers, Apps, Hooks, and UI slots from local, Git, NPM, or
  community marketplaces, with compatibility checks and conflict blocking.
- **Pluggable Agent runtime** — model provider, turn context, tool registry,
  scheduler, middleware, and permission profile are explicit runtime seams;
  CLI and GUI share the same assembly contract.
- **Harness tools** — real LSP queries, bounded background jobs, persistent raw
  PTY sessions, structured workspace inspection, web tools, and session lookup.
- **Skills system** — user skills plus three built-in coding skills; only
  name + description are injected, bodies load on demand.
- **Custom slash commands** — prompt-backed project/user commands in
  `.nanocodex/commands`, with `.claude/commands` compatibility.
- **Persistent memory + AGENTS.md / CLAUDE.md** — durable notes plus layered
  project instructions injected each turn.
- **Cross-agent long-term memory plugin** — the bundled LLM Wiki example pairs
  an always-applied Skill with a local MCP service to recall user preferences
  and project snapshots across BugleCat and other Agent clients.
- **Browsable session history** — JSONL logs, full-transcript snapshots, resume,
  and fork.
- **Context compaction** — zero-cost deterministic digest or opt-in model
  summarizer, keyed to a token budget.
- **Cache-aware cost accounting** — real per-call usage priced against
  DeepSeek's hit/miss rates.
- **Adaptive reasoning effort** — the `auto` tier picks `max`/`high`/`low` from
  the request (multilingual keyword tables: EN / 中文 / 日本語).
- **Scheduler** — recurring/one-shot saved prompts with consecutive-failure
  auto-disable.
- **A/B worktree comparison** — run one prompt under two configs in isolated git
  worktrees, compare diff/cost/latency, adopt one side.
- **Prompt enhancement, image input, Chinese-first responses**, and a **Tauri 2 +
  Svelte 5** desktop client.

## Architecture

```text
rust/
├── crates/
│   ├── ncx-config/        # layered config and runtime limits
│   ├── ncx-provider/      # replaceable OpenAI-compatible / Anthropic model boundary
│   ├── ncx-sandbox/       # policy and 4-state approval decisions
│   ├── ncx-tools/         # process executor (Job Object / process-group containment), managed jobs, PTY, file & patch tools
│   ├── ncx-mcp/           # MCP transport, protocol validation, and atomic tool reload
│   ├── ncx-context/       # associative memory and context retrieval
│   ├── ncx-thread-store/  # durable session and event storage
│   ├── ncx-protocol/      # strongly-typed GUI / App Server protocol contract
│   ├── ncx-app-server/    # desktop and host service bridge
│   ├── ncx-core/          # AgentLoop, ToolRegistry, middleware, claim verification, orchestrator
│   └── ncx-cli/           # one-shot and interactive CLI front end (ncx.exe)
└── gui/
    ├── src/               # Svelte 5 desktop UI
    └── src-tauri/         # Tauri v2 native commands and shared runtime assembly

nanocodex/                 # Stage 1 Python reference and compatibility implementation
```

### Runtime Control Plane

- **Task budget:** every model call receives a runtime budget note with current
  model-call, tool-call, and context limits; the loop stops cleanly when model
  or tool budgets are exhausted, backfills unanswered tool calls, and reports
  unresolved verification status accurately.
- **Context editing:** the full local session remains intact, but the provider
  sees a send-time edited view that compresses old tool results and drops older
  prefixes once the context budget is exceeded.
- **Tool search:** tools are registered into a catalog. Small registries expose
  all tools; larger registries expose core tools plus `tool_search`, and search
  hits are made visible in the next schema view. The default view admits up to
  16 schemas so a mixed task can expose LSP, background, and terminal tools in
  the same model turn.
- **Harness tool families:** the Rust runtime includes a real `rust-analyzer`
  LSP provider, bounded background processes with incremental output, and
  persistent ConPTY/PTY shells with raw stdin, cursor reads, resize, and close.
  Windows child processes start suspended inside Job Objects; Unix commands
  run in their own process group.
- **Tool recovery:** read-only tool failures are classified, transient failures
  may be retried once, and argument-compatible fallbacks are selected without
  bypassing middleware, approval, or sandbox enforcement.
- **Structured workspace inspection:** `list_directory` and `path_info` use
  native filesystem APIs, while `git_status` and `git_diff` expose fixed
  read-only Git operations. Agents do not need to compose `ls`, `dir`, `find`,
  or platform-specific shell probes just to inspect a workspace.
- **Semantic memory:** project memory retrieval now uses a hybrid lexical
  semantic ranker: keywords, tags, phrase matches, Jaccard similarity, recency,
  and a small domain synonym map for agent/runtime terms.
- **Deterministic hooks:** `[[hooks]]` can run project commands before or after
  matching tools and at turn lifecycle points. A failing `pre_tool` or
  `user_prompt` hook blocks the action; `post_tool` and `stop` output is
  appended for audit, formatting, and quality-gate workflows.
- **Checkpoint / restore:** the Rust CLI and Tauri GUI create file checkpoints
  before model turns. CLI exposes `/checkpoint`, `/checkpoints`, and
  `/restore <id>`; the GUI exposes a checkpoint panel for manual save, list, and
  restore.
- **Pi-style in-conversation trace tree (`_trace`):** every message in `Session::messages` and `session.jsonl` embeds a structured `MessageTrace` (`id` / `parent_id` tree pointers, `turn_id`, `content_hash`, `workspace_revision`, assistant `model` / `finish_reason` / token `usage`, tool `duration_ms` / `status` / `exit_code`, and `CompactionTrace::first_kept_entry_id`), automatically stripped from wire requests before calling the LLM.
- **OpenCode-style post-edit automatic diagnostics:** after `apply_patch` or `str_replace_editor` mutates workspace files, the harness automatically runs incremental LSP diagnostics (synchronizing `textDocument/didChange`), in-process JSON/TOML syntax validation, and Git merge-conflict marker checks, appending any errors or warnings directly to the edit result.
- **Codex-style large tool output offloading:** tool outputs exceeding 16,000 characters are automatically written in full to an external artifact file while returning an inline head + tail preview and a `[tool_output_offloaded: path=... offset=...]` handle for on-demand inspection via `read_file`.

## Project Phases

BugleCat has gone through three stages: fast product exploration in Python, a
strictly typed native Rust + Tauri runtime, and now a self-evaluating
intelligence-compounding loop.

### Stage 1: Python Baseline

The `nanocodex/` package is the original, feature-complete agent line, built
for iteration speed: a compact async loop (model call → tool execution →
session update), dynamic module boundaries that made features cheap to add
(MCP marketplace, prompt enhancement, image input, session resume/fork, A/B
worktrees), a Tkinter prototype GUI, and a large offline test suite. Its costs —
a Python install, import-time startup, and dynamic boundaries that grow hard to
reason about once sandboxing, memory, MCP, and parallel agents interact —
motivated the rewrite.

### Stage 2: Rust Rewrite

`rust/` rebuilds the core as small crates plus a Tauri shell while keeping the
proven feature map. Core contracts (provider responses, tool calls, sandbox
decisions, session messages, memory entries, protocol requests, orchestrator
results) are typed across crate boundaries; every file, shell, search, and
memory action goes through one `ToolContext` / `ToolRegistry`, so approval and
sandbox checks sit next to execution. `AgentLoop` is assembled from replaceable
providers, turn-context providers, middleware, and an injectable scheduler, and
CLI, GUI, and orchestrated workers share one `AgentRuntimeProfile` assembly
path. The orchestrator adds task classification, main/fast model routing,
isolated worker workspaces, verification, and promotion of the winning worker.

Why Rust: explicit ownership makes parallel worker isolation and result
promotion safer; a small native `ncx.exe` starts instantly with no interpreter
(release builds use strip, LTO, and size optimization); and Tauri gives a
better long-term desktop packaging path than the Tkinter prototype.

### Stage 3: Intelligence Compounding (Current)

Stage 2 built a reliable harness, but every task still started cold: outcomes
were discarded after cost accounting, recalled memories had no outcome
attribution, routing ignored history, and human corrections were invisible.
Stage 3 closes the loop `Action → TaskOutcome → OutcomeFollowup → Learning` —
see [Intelligence Compounding Loop](#intelligence-compounding-loop) and
[Self-Evolving Harness](#self-evolving-harness).

## Tools

The Rust registry exposes a task-relevant subset each turn. Core tools stay
visible; named matches and `tool_search` hints add specialized tools without
sending the entire catalog on every model call.

| Tool | Purpose |
| --- | --- |
| `shell` | Run a shell command, gated by the sandbox/approval policy. |
| `apply_patch` | Apply a Codex-style patch to create/edit/delete files. |
| `update_plan` | Maintain a visible step plan for multi-step tasks. |
| `read_file` | Read a file (or a line range) from the workspace. |
| `str_replace_editor` | Perform exact, approval-gated text edits. |
| `grep`, `grep_literal`, `glob` | Search source and paths without shell parsing. |
| `list_directory`, `path_info`, `git_status`, `git_diff` | Structured workspace inspection. |
| `web_search`, `web_fetch` | Search and fetch through configured network policy. |
| `lsp` | Query symbols, definitions, references, hover, and diagnostics. |
| `background_start/poll/stop/list` | Manage bounded background processes with incremental output. |
| `terminal_open/write/read/exec/resize/close/list` | Manage persistent raw PTY sessions. |
| `ask_user_question` | Pause for a GUI/CLI answer when the frontend supplies a handler. |
| `session_search`, `session_trace`, `session_event_read/search/trace` | Query saved session snapshots and redacted events. |
| `skill` | Load a discovered skill body on demand. |
| `remember` | Append a durable note to user memory. |
| `<tool>` (MCP) | A tool exposed by a connected MCP server, using its server-supplied name. |

## Configuration

Settings resolve in priority order:

```text
CLI flags > environment > ~/.nanocodex/config.toml > ~/.deepseek/config.toml > ~/.codex/config.toml > defaults
```

The real API key should stay outside the repository:

```powershell
$env:DEEPSEEK_API_KEY = "sk-..."
$env:NANOCODEX_API_KEY = "sk-..."
```

Or create `~/.nanocodex/config.toml`:

```toml
api_key = "sk-..."
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"

sandbox_mode = "workspace-write"   # read-only | workspace-write | danger-full-access
approval_policy = "on-request"     # untrusted | on-failure | on-request | never
reasoning_effort = "auto"          # auto | low | high | max | off

# Online compounding feedback modes (off | shadow | on, default shadow)
# Switching to "on" requires >= 14 days of shadow data and >= 2 core categories with n >= 30
[feedback]
memory = "shadow"
routing = "shadow"

# Optional
# context_token_budget = 512000
# context_window = 1048576
# max_iterations = 60
# max_tool_calls = 120
# max_parallel_tool_calls = 8  # valid range: 1..=128; read-only tools only
# context_edit_enabled = true
# context_edit_max_chars = 120000
# context_edit_keep_recent_messages = 30
# context_edit_max_tool_result_chars = 4000
# available_models = ["deepseek-chat", "deepseek-reasoner", "deepseek-v4-pro"]

# [[hooks]]
# event = "pre_tool"          # pre_tool | post_tool | user_prompt | stop
# matcher = "shell|apply_patch"
# command = "echo checking %NCX_HOOK_TOOL%"
# timeout_s = 10
```

Runtime control-plane settings can also be set with environment variables:
`NANOCODEX_MAX_ITERATIONS`, `NANOCODEX_MAX_TOOL_CALLS`,
`NANOCODEX_CONTEXT_EDIT_ENABLED`, `NANOCODEX_CONTEXT_EDIT_MAX_CHARS`,
`NANOCODEX_CONTEXT_EDIT_KEEP_RECENT`, and
`NANOCODEX_CONTEXT_EDIT_TOOL_RESULT_CHARS`. The Rust CLI also accepts
`--max-iterations`, `--max-tool-calls`, `--context-edit-max-chars`,
`--context-edit-keep-recent`, `--context-edit-tool-result-chars`, and
`--disable-context-edit`.

A full example lives in `config.example.toml`.

Hooks receive `NCX_HOOK_EVENT`, `NCX_HOOK_TOOL`, `NCX_HOOK_ARGS`,
`NCX_HOOK_RESULT`, and `NCX_HOOK_WORKSPACE` in their environment. Use
`pre_tool` for deterministic guards such as blocking risky shell commands,
`post_tool` for audit and formatting, `user_prompt` to block or annotate a
prompt before the model sees it, and `stop` for end-of-turn quality gates or
notifications. Claude-style event names such as `UserPromptSubmit`, `Stop`,
`PreToolUse`, and `PostToolUse` are accepted and normalized. Hooks run as local
subprocesses, so configure only commands you trust.

## Sandbox & Approval

Two orthogonal axes gate every action, mirroring Codex:

**Sandbox mode** — what's physically allowed:

| Mode | Reads | Writes | Network |
| --- | --- | --- | --- |
| `read-only` | anywhere | none | off |
| `workspace-write` | anywhere (except secret files such as `.env`, private keys, cloud credentials) | workspace + writable roots (+ system temp only when opted in) | off unless enabled |
| `danger-full-access` | anywhere | anywhere | on |

**Approval policy** — what to do when an action exceeds the sandbox:
`untrusted`, `on-failure`, `on-request`, `never`. The approval engine resolves
each escalation to `ASK` / `AUTO_APPROVE` / `AUTO_DENY`.

Enforcement is **policy-level on every platform**: file tools check paths and
writable roots at the tool boundary, and a shell command is classified from its
command line before it runs (read-only allowlist, destructive patterns, network
commands and inline network code, paths outside the workspace, secret files
including globbed or escaped names, `git config` / hook hijacks). It is **not**
kernel isolation: a program the command starts — a Python script, a build, a
test suite — is not restricted once it runs. What *is* enforced by the OS is
process containment: on Windows each command starts suspended inside a
kill-on-close Job Object, and on Unix in its own process group, so a timeout
kills the whole tree. Under `untrusted`, interpreters and test runners always
ask first.

## Intelligence Compounding Loop

Designed around the **60-point singularity**: the point at which an agent moves
from "humans backstopping AI" to "AI owning the engineering outcome". Details:
[intelligence compounding plan (Chinese)](docs/intelligence-compounding-feedback-plan.zh-CN.md).

- **Task outcomes & ACR.** Every primary turn — single-agent or orchestrated
  (`{session}:orch-{turn}`) — emits a privacy-safe `TaskOutcome` into SQLite,
  classified into four categories: `code_fix_test`, `compile_repair`,
  `feature_impl`, `doc_refactor`. Chat turns and orchestrator worker/node turns
  are recorded but excluded from the Autonomous Completion Rate (ACR)
  denominator, and turns without a resolved followup stay `pending`. A category
  crosses the singularity when, over 30 days, `n >= 30`, the `ACR-strict` 95%
  Wilson lower bound is `>= 60%`, and `rework_rate <= 15%`.
- **Followup signals without a daemon (`I1–I5`, `L`).** `I1` terminal
  failure/block; `I2` approval prompts and `ask_user_question` (`ACR-strict`
  penalizes all of them, `ACR-approval-tolerant` allows approved escalations);
  `I3` checkpoint `/restore`; `I4` multilingual (ZH/EN/JA) correction phrases or
  edits to the same files; `I5` the `LazyGitUndoDetector`, which uses
  `git merge-base --is-ancestor` and `git status --porcelain` to spot
  `git reset`, `git revert`, or manual edits across sessions; and `L` explicit
  labels (`/outcome good|bad [note]`, A/B adoption, and the GUI's per-turn
  `♡` / `♧` buttons), which take priority.
- **Guarded online feedback.** `[feedback] memory` and `routing` default to
  `"shadow"`, which behaves like `"off"` while logging what would have changed
  (`memory_rank_changed`, `shadow_recommended_route`). Switching to `"on"` —
  via CLI `/config` or GUI Settings — requires `>= 14` days of shadow data and
  `>= 2` categories with `n >= 30` (`--force` overrides). When on: recalled
  notes are scored once per resolved task with a Beta posterior
  `p = (accepted + 1) / (injected + 2)` that moves retrieval scores within
  `±20%`; notes with `injected >= 8` and `p < 0.25` are demoted (restorable);
  and routing upgrades monotonically (`Simple → Medium → Complex`) when a
  category's lower bound drops below `40%` (`n >= 20`) and a higher tier
  improves it by `>= 10%`.
- **Recovery-chain learning.** Multi-step `tool_failed → diagnostic/read →
  tool_succeeded` chains (`CompoundRecoveryChain`) become `Candidate` notes
  with a cross-session `occurrences` counter. Orchestrator workers never write
  candidates directly: only the promoted worker's path-remapped candidates
  reach the host store. With `[feedback] memory = "on"`, recovery chains and
  task notes from completed, verified turns are promoted automatically; in
  the default `"shadow"` mode they are only recorded as candidates and wait
  for `ncx --memory-review confirm <id>` or the GUI review panel.
- **Bootstrap & dashboard.** `ncx --outcomes bootstrap <repo...>` backfills
  historical sessions into `~/.nanocodex/global-outcomes/context.sqlite`;
  `ncx --outcomes report --since 30d --by category,model,policy` (or
  `/outcome report`) prints the report; the GUI **Memory** panel shows 30-day
  `ACR-strict`, Wilson progress toward 60%, per-category `n/30`, and shadow
  baseline readiness.

## Self-Evolving Harness

Three loops, modeled on Google's RRSI and Harness² work. Details:
[self-evolving harness (Chinese)](docs/self-evolving-harness-rrsi-harness2.zh-CN.md).

- **L0 — within a task: Propose–Probe–Compose (`ncx-core` orchestrator).**
  Several isolated workers attempt the task; the verifier picks `BEST:<n>` and,
  when other workers made complementary changes to *different* files, adds
  `COMPOSE:<best>,<n>,...`. A composition is staged on a throwaway copy of the
  workspace and re-verified there by a tool-using verifier
  (`COMPOSE_VERIFY_SYS`); only a `PASS` promotes it, otherwise only the best
  worker is promoted and the result says why. Secondary workers whose edits
  clash with an earlier one are dropped whole, and rounds that exhaust their
  retries never compose. A failing round feeds the verifier's verdict plus a
  digest of every worker's findings into the next round
  (`build_compose_feedback`).
- **L1 — across tasks: two-tier memory and guardrails (`[feedback] memory = "on"`
  only).** Once a module has `>= 2` judge-confirmed notes, its newest verified
  snippets are consolidated into one `[Module Invariant: <module>]` entry
  (`+6` recall boost, stable `note_id`) and older per-task notes for it are
  trimmed to `MAX_EPISODIC_PER_MODULE = 2`. A `[Regression Guardrail: <module>]`
  warning is written only for an explicitly reported regression
  (`p2p_regressed > 0`, or a negative label that mentions a regression without
  negating it) and is cleared by a later verified success on that module. On
  expensive turns only recalled notes matching the edited files get credit.
  Recall defaults to 6 entries / 2,400 chars.
- **L2 — offline: `ncx-forge` genome search (`train/`).** `NCX_GENOME` exposes
  `system_prompt`, tool descriptions, orchestrator prompts (`classify_sys`,
  `plan_sys`, `decompose_sys`, `worker_sys`, `verify_sys`,
  `compose_verify_sys`), and bounded `memory_config`; an empty genome is a
  byte-identical no-op. The search is regularized by annealed edit budgets
  (`--initial-max-surfaces`, `--max-changed-lines`), an anti-memorization
  critic, a falsified-hypothesis ledger, stagnation steering, and post-search
  ablation pruning (`--prune`). A candidate is accepted only if its train gain
  clears the sampling-noise floor from `analyze.pass_noise_floor`
  (`--noise-z`, default one-sided 95%; defaults `--repeats 3`,
  `--accept-margin 2`) or it saves tokens at an equal pass rate, and holdout
  does not regress; the accepted champion's holdout is then re-evaluated with
  fresh runs. Harness² mode (`--h2-k`) probes K genome edits on a failing task
  and composes one from a verifier-blind behavioral contrast; the lesson
  library credits only lessons the compose step reports in `applied_lessons`.
  See [Harness² in forge (Chinese)](docs/harness2-forge-integration.zh-CN.md).

## Evaluation & Evidence

All of these are small, controlled measurements, not general success-rate
claims or production certification.

- **Source self-repair (2026-09-08, `deepseek-v4-pro`).** Six fault-injection
  categories with specified files and visible tests: 5/6 on the first attempt
  and 6/6 with at most one feedback turn; an afternoon rerun passed 6/6 on the
  first attempt (609 s of agent runtime). Results were independently checked
  and the patches replayed.
  [Report (Chinese)](docs/reports/self_repair_20260908.md) ·
  [rerun report](docs/reports/self_repair_20260908_081319.md) ·
  [evidence & reproduction](evals/self_repair/README.md)
- **Claim drift benchmark.** 175 synthetic bilingual cases across 11 policy
  categories for the host-level claim verifier (`host-rules-v3.4`) — a policy
  regression suite, not an end-to-end evaluation.
  [Report](reports/claim_drift_eval_report_v1.md) ·
  [delivery receipt](reports/buglecat-delivery-20261014.json)
- **Compounding replay (`python-tabulate`, 9 tasks × 3 paired rounds,
  `deepseek-chat`).** With single-tier memory (Arm B) vs no memory (Arm A),
  cost per resolved task fell from 0.2265 to 0.1790 CNY (`B/A = 0.7904`,
  task-level paired bootstrap 95% CI `[0.6239, 0.9523]`) with resolution
  `22/27` vs `21/27`. Caveats: the bootstrap resamples only 9 tasks, the
  non-inferiority margin (`14.9pt`) is wide, the earlier 2-round `sqlparse`
  run was inconclusive (`B/A = 0.7131`, CI `[0.3693, 1.1586]`), and the single
  two-tier-memory run (Arm B') capped learnings at 6 entries but saved less
  (`-10.9%`) than Arm B. Tools: `evals/compounding_replay/analyze.py`
  (noise floors, exact McNemar, paired bootstrap) and `validate_gold.py`
  (9/9 official fixes verified).

## Custom Slash Commands

Rust REPL can turn Markdown prompt templates into slash commands. Put project
commands in `.nanocodex/commands/<name>.md`; for Claude Code compatibility,
`.claude/commands/<name>.md` is also read. User commands live in
`~/.nanocodex/commands/<name>.md`, with `~/.claude/commands/<name>.md` as a
compatibility fallback.

```markdown
---
description: Review one file
---
Review `$ARGUMENTS[0]` for bugs, regressions, and missing tests.
```

In the REPL:

```text
/review rust/crates/ncx-core/src/session.rs
/project:review rust/crates/ncx-core/src/session.rs
/user:review rust/crates/ncx-core/src/session.rs
```

`/name` resolves project commands before user commands. Templates support
`$ARGUMENTS` for the raw argument string plus `$0`..`$9` and
`$ARGUMENTS[0]`..`$ARGUMENTS[9]` for simple positional arguments. If a command
template has no placeholders, the raw arguments are appended under an
`Arguments:` block. These commands expand to a normal user prompt; they do not
run local shell code by themselves.

## Local Model / OpenAI-Compatible Endpoint

BugleCat talks plain `/v1/chat/completions`, so any OpenAI-compatible server
works — vLLM, llama-server, LM Studio, Ollama's OpenAI shim, etc. Point
`base_url` at the server's `/v1` root. Most local servers ignore the API key,
but a non-empty placeholder is still required because the OpenAI SDK expects
one.

```toml
api_key = "local-dev-key"
base_url = "http://127.0.0.1:8005/v1"
model = "Qwen3.6-27B-Q4_K_M"
```

Quick connectivity check:

```powershell
curl http://127.0.0.1:8005/v1/models
```

Streaming has a bounded "response-header" timeout (default 45s, override with
`NANOCODEX_STREAM_OPEN_TIMEOUT_S`) so a stalled local server fails fast with a
clear hint instead of hanging the UI.

## MCP

MCP servers are opt-in and run **outside** the sandbox (they launch external
subprocesses). Configure them in `~/.nanocodex/mcp.toml`:

```toml
[mcp_servers.fetch]
command = "uvx"
args = ["mcp-server-fetch"]

[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:\\projects"]
```

Then start with MCP enabled:

```powershell
nanocodex --mcp
```

The Rust `ncx` REPL can apply MCP config changes without restarting:

```text
/mcp reload
```

Reload prepares every configured server before replacing the active MCP tool
set. A failed server is skipped while the successfully prepared servers replace
the set; if every configured server fails, or a tool-name conflict is found,
the previous set stays active. An empty config removes all active MCP tools.

Each connected server has one background stdio reader. Responses are routed by
JSON-RPC request ID, so independent read calls can complete out of order without
being matched to the wrong tool call; EOF, write errors, and request timeouts
wake and clean up their pending requests. Calls for a server share a bounded
gate: at most four explicitly read-only calls are in flight, while a
side-effecting or unknown call takes the full gate and is exclusive. The gate is
per server, so a slow server does not serialize unrelated servers. Approval is
still checked before a side-effecting call acquires the gate.

The initialize handshake validates an explicitly returned MCP protocol version
before tools are registered. A server that omits the field (older compatible
implementations) remains accepted; an explicit incompatible or malformed value
fails only that server's startup.

Each server's tools retain the name supplied by that server. Tool-name conflicts
are rejected during the atomic reload, so choose distinct MCP tool names. A
**marketplace** adds one-click install from a built-in curated catalog or a
remote catalog (`NANOCODEX_MARKETPLACE_URL`); every entry funnels through the
same name-validation and dup-check as a hand-added server, and remote catalogs
are treated as untrusted data. See `mcp.example.toml` for more.

## Plugin Ecosystem

BugleCat supports two deliberately separated plugin forms:

- **Codex-compatible resource plugins** use `.codex-plugin/plugin.json` and can
  bundle Skills, MCP servers, Apps, Hooks, and interface contributions.
- **External process plugins** use `plugin.toml` and communicate across a
  process boundary. BugleCat does not load third-party DLL, SO, or DYLIB files
  into the application process.

A resource plugin can use this layout:

```text
my-plugin/
├── .codex-plugin/plugin.json
├── skills/<name>/SKILL.md
├── .mcp.json
├── .app.json
└── hooks/hooks.json
```

Plugins can be installed globally in `~/.ncx/codex-plugins` or per workspace in
`.ncx/codex-plugins`. A workspace plugin with the same ID overrides the global
copy. Marketplace discovery understands OpenAI Codex, Claude, and Cursor style
directories, plus DSH Community sources such as dshfind, DeepSeek 1024 Store,
and standard HTTPS catalogs. Install sources may be local paths, Git
repositories, or NPM packages; each candidate is staged and validated before
installation.

Optional resources are isolated during runtime discovery. A malformed or
unreadable plugin catalog, Hooks document, Apps document, or individual App is
reported and skipped without hiding valid resources from another plugin. The
explicit catalog-management command remains strict, so a maintainer can still
inspect and fix invalid manifests instead of having errors silently rewritten.

The settings UI previews requested capabilities and compatibility before
installing. If an enabled plugin already owns an overlapping capability, the
new installation is blocked instead of silently replacing it. Interface
plugins can contribute to the supported UI slots `settings.plugins.tab`,
`sidebar.footer.action`, and `shell.overlay`, while Harness diagnostics show the
capabilities mounted at runtime.

MCP and external attachment parsers remain disabled by default because they can
start subprocesses or send content to another service. Enable only plugins and
marketplace sources you trust. See
[`local-plugins/llmwiki-memory`](local-plugins/llmwiki-memory) for a working
plugin that ships a Skill and an MCP server together.

## Skills

Skills are reusable instruction documents, one folder each:

```text
~/.nanocodex/skills/<skill-name>/SKILL.md
```

Only each skill's **name and description** are injected into the system prompt;
the full body is read on demand, so a large library doesn't eat the context
window. The model can also create/read/delete user skills in-chat via the
`manage_skills` tool.

Minimal skill:

```markdown
---
name: code-review
description: Review code changes and focus on bugs, regressions, and missing tests.
---

# Code Review

Look for behavior regressions first, then missing tests, then maintainability.
```

The package ships three **read-only built-in skills** under
`nanocodex/builtin_skills/`:

- **code-review** — two-pass review (correctness, then cleanup), ranked by impact.
- **debug** — reproduce → localize → fix → verify; resist patching the first
  plausible line.
- **write-tests** — test observable behavior, one behavior per test, prefer pure
  functions over mocks.

A user skill of the same name shadows the built-in.

## Memory & AGENTS.md

Three complementary layers of persistent context:

- **User memory** (`~/.nanocodex/memory.md`) — durable personal facts and
  preferences. Written by the `remember` tool, by typing `# something` in the
  GUI composer (quick-capture), or by hand. Wrapped in a `<user_memory>` block.
- **Project learnings & compounding feedback** (`.ncx/memory/LEARNINGS.md` + `memory_index.sqlite`) —
  project-scoped operational notes governed by an online feedback lifecycle:
  - **Candidate (`Candidate`)**: multi-step `tool_failed -> diagnostic/read -> tool_succeeded` recovery chains (`CompoundRecoveryChain`) are captured with a cross-session recurrence counter (`occurrences`, sorted by `occurrences DESC, created_at DESC`). Isolated subagent workers remap scratch paths and only promote candidates from the winning worker. With `[feedback] memory = "on"`, candidates from completed, verified turns auto-promote; otherwise they await `ncx --memory-review confirm <id>` or GUI confirmation.
  - **Active (`Active`)**: recalled via hybrid lexical-semantic ranking, attributed in `TaskOutcome.recalled_note_ids`, and re-ranked within a bounded **`±20%`** window using Beta-posterior outcome scores (`p = (accepted + 1) / (injected + 2)`).
  - **Demoted (`Demoted`)**: notes with `injected >= 8` and `p < 0.25` in `[feedback] memory = "on"` mode are automatically demoted (excluded from recall while preserving the row) and can be restored via `ncx --memory-review restore <id>`.
- **AGENTS.md / CLAUDE.md** — project instructions layered from
  `~/.codex/AGENTS.md` and `~/.claude/CLAUDE.md`, then every `AGENTS.md`,
  `CLAUDE.md`, and `.claude/CLAUDE.md` from the repo root down to the workspace,
  so nested directories refine their parents. Total size is capped so a huge
  file can't blow the context. Rust CLI, orchestrator workers, and the Tauri GUI
  all inject this block at session startup.

Startup deduplicates project learnings with a cheap heuristic; `ncx --memory-merge`
runs the costlier LLM-backed consolidation only as an explicit maintenance command.

Memory is "who/what" (preferences, facts, verified project learnings); skills are "how to do X";
AGENTS.md / CLAUDE.md are project-scoped guidance.

### LLM Wiki cross-agent long-term memory

[`local-plugins/llmwiki-memory`](local-plugins/llmwiki-memory) is an optional
local plugin that adds a more structured memory lifecycle. It combines an
`always_apply` Skill with one local MCP tool, using a configured local LLM Wiki
directory (for example `<llmwiki-root>`) as the actual memory store, while its
protocol/template repository remains separate from live data.

After the plugin and its MCP server are enabled:

1. A new Agent session automatically recalls the user profile before handling
   the first business request.
2. Entering an existing project recalls only its short L1 project snapshot;
   historical sections load on demand instead of filling the context window.
3. While the MCP service is running, saved BugleCat sessions are collected
   incrementally into the L0 corpus, normally within about one minute. No new
   conversation or manual import is required.
4. Confirmed project decisions, progress, pending work, and verification
   evidence can be recorded at task handoff and recalled by later Agents.

The layers have different trust rules: L0 is collected conversation material;
L1 is a concise project memory with source references; L2 is the stable user
profile. New conversation text never changes L2 directly. Profile candidates
must meet the evidence threshold and remain pending until the user explicitly
approves them. Tokens, cookies, passwords, secrets, database credentials, and
sensitive business content must not be stored.

Long-term memory is a recovery aid, **not a source of code truth**. Repository
requirements, contracts, current files, Git diff, tests, and database state
must be checked again before an Agent modifies a project. If the MCP service is
unavailable, the current task continues with a visible warning rather than
blocking or silently writing the Wiki files directly.

## Sessions, Resume & History

- Every conversation is appended to a **JSONL session log** (base64 image data
  is redacted from the log to keep it small).
- A **global index** (`~/.nanocodex/sessions.jsonl`) holds one summary line per
  conversation, newest-first, for the GUI's history list.
- A **per-session snapshot** (`~/.nanocodex/snapshots/<id>.json`) freezes the
  full transcript so the detail view replays the real conversation, not a digest.
- The Rust CLI supports `--resume` to reload the workspace
  `.nanocodex/session.jsonl` before starting and `--history` to list recent
  global session summaries. The Tauri backend records the same snapshots after
  each GUI turn.
- Rust CLI and Tauri GUI save a workspace file checkpoint before each model
  turn. Use `/checkpoints` to list recent checkpoints, `/checkpoint <label>` to
  create one manually, and `/restore <id>` to restore files; the GUI has the
  same save/list/restore flow in its checkpoint panel. Restore first creates a
  safety checkpoint of the current state.
- The original Python GUI can **fork** a saved snapshot to branch a past
  conversation without mutating the source session.

## Context Compaction

Long conversations are folded to stay within a token budget while preserving the
system message and a recent tail (the tail always starts at a `user` message, so
no tool-call/result pair is split). Two strategies share one interface:

- **deterministic** (default, zero API cost) — the folded middle becomes a
  factual, rule-based digest.
- **summarizer** (opt-in, costs tokens) — a model call turns the middle into prose.

The trigger estimate uses a Chinese-leaning chars/token ratio so zh-heavy chats
don't compact too late.

In the Rust CLI, `/compact` materializes the active context-edit policy into the
live session and rewrites the workspace session log, so future turns and
`--resume` continue from the compacted history.

## Token Usage & Cost

The provider returns real `usage` per call, including DeepSeek's
cache-hit/miss split. In the Rust REPL, `/usage` and `/cost` show the last turn
and session total model calls, tool calls, prompt tokens, completion tokens, and
cache hit/miss tokens. The Rust command intentionally reports raw usage only;
`pricing.py` turns usage into a USD cost for the Python line:

- **Cache-aware** — a cache-hit input token is ~120× cheaper than a miss; each is
  billed at its own rate. When the split is absent, the whole prompt is billed at
  the miss rate so cost is never understated.
- **Honest about staleness** — prices are a hardcoded snapshot carrying a source
  and as-of date; an unknown model returns "cost unknown" rather than a wrong
  number.

## Scheduler

Save a prompt to run automatically — once at a future time or on an interval:

```powershell
nanocodex schedule add "run the tests" --at 2026-06-08T09:00:00
nanocodex schedule add "summarize new issues" --every 3600
nanocodex schedule list
nanocodex schedule run        # keep this running for tasks to fire
```

A task that fails repeatedly **auto-disables** after 5 consecutive failures
(success resets the counter; re-enabling clears it), so a broken task can't loop
forever. The agent can also manage tasks in-chat via `manage_schedule`.

## A/B Worktree Comparison

Run the **same prompt under two configurations** and compare the results without
risking your working tree. Each side runs in its own isolated **git worktree**,
so real `shell`/`apply_patch` edits never collide:

1. Pick two configs (model / reasoning effort / sandbox / approval).
2. BugleCat creates two worktrees from clean `HEAD` and runs the prompt in each,
   serially, with auto-approve scoped to the worktree.
3. You get a side-by-side comparison: diff, token cost, latency, iterations,
   stop reason.
4. **Adopt** one side (its diff is applied to the real workspace) or discard
   both; the worktrees are always cleaned up.

Requires a clean git workspace (no uncommitted changes); the entry is disabled
otherwise.

## BugleCat Desktop GUI

The BugleCat desktop app uses Tauri 2 + Svelte 5 (`rust/gui`) and ships for Windows and macOS:

- Streaming chat with reasoning/answer separation, Stop button, and **turn-indexed `♡` / `♧` rating buttons** on every assistant turn (binding human `Label` verdicts directly to that turn's `TaskOutcome`).
- **Visual ACR & 60-Point Singularity Dashboard** in the right-hand **Memory** panel: displays 30-day `ACR-strict`, Wilson 95% lower-bound progress toward the 60% singularity threshold, per-category sample counts (`n/30`) and singularity badges across the 4 core engineering categories, 14-day Shadow baseline readiness, and interactive Candidate/Demoted memory review (`Confirm` / `Reject` / `Restore` with recurrence counts `命中 N 次`).
- Project switcher, model switcher, `/outcome good|bad|report` slash commands, and a multi-section Settings page (including the `[feedback]` memory/routing modes).
- Browsable session history (click to replay a full transcript).
- File panel, prompt enhancement, image attachment, `#` quick-capture to memory,
  MCP controls, checkpoints, and permission modes.
- A movable sidebar, approval dialog, and real `ask_user_question` modal with
  choice, free-text, and cancel flows.
- Failed tool results are removed from the visible transcript while remaining
  available to the model and session log for recovery and diagnosis.

CLI and GUI derive provider, permission, budgets, context editing, concurrency,
and LSP attachment from the same `AgentRuntimeProfile` assembly path.

## Tests

Rust + Tauri release line (zero real API key / offline deterministic):

```powershell
# Rust formatting check (respects SHA-256 frozen verification example files)
python scripts/check_rustfmt.py

# Rust workspace & Tauri host static checks and unit tests
cargo clippy --manifest-path rust\Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path rust\gui\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path rust\Cargo.toml --workspace --all-features
cargo test --manifest-path rust\gui\src-tauri\Cargo.toml --lib

# Golden evaluation gates (24 unit + 11 cargo suites)
cargo run --manifest-path rust\Cargo.toml -p ncx-cli -- eval golden evals\golden --tier unit
cargo run --manifest-path rust\Cargo.toml -p ncx-cli -- eval golden evals\golden --tier cargo

# Frontend protocol drift, TypeScript typecheck, and Vite production build
npm.cmd --prefix rust\gui run test:protocol
npm.cmd --prefix rust\gui run protocol:check
npm.cmd --prefix rust\gui run typecheck
npm.cmd --prefix rust\gui run build
```

Python compatibility & engineering scripts:

```powershell
python -m pytest -q
python -m ruff check .
```

The normal Rust and Python suites are deterministic and offline. Explicit live
acceptance runs cover `rust-analyzer`, external network providers, MCP hot
reload, and real model-driven combinations of LSP, background, and PTY tools.

### Claim-Level Contradiction & Agent Drift Verification (Host Physics)

BugleCat includes a deterministic, host-level physical defense line (`ncx-core::verification`, rule version `host-rules-v3.4`) that blocks model statement drift and tool-execution hallucinations:

```powershell
# Run the 175-case bilingual claim-level contradiction and drift benchmark
cargo run --manifest-path rust/Cargo.toml --example eval_claim_drift -- evals/verification/claim_drift_dataset_v1.json reports/claim_drift_eval_report_v1.json

# Regenerate formatted Markdown benchmark report
python scripts/format_claim_drift_report.py

# Audit frozen dataset SHA-256 integrity, protocol version alignment, and independent evaluation readiness
python scripts/audit_claim_source_pool.py

# Independently audit session verification logs (.verification.jsonl) with CI strict gating
python scripts/replay_verification.py path/to/.verification.jsonl --strict
```

The 175 synthetic cases cover 11 policy categories with preconstructed claims and evidence. The v2 metrics count `NeedEvidence` and `Contradiction` symmetrically as interventions, and exclude `Unknown` from detection. These are policy regression results, not an end-to-end or independent production certification. See [the report](reports/claim_drift_eval_report_v1.md).

### Key Reliability Boundaries Covered by Regression Suites

- **Permission & MCP fail-closed isolation**: MCP read-only classification trusts only explicit `readOnlyHint=true` + `destructiveHint=false` annotations (plus a repo-owned read-action allowlist for multiplexed tools like `llmwiki`). Missing or conflicting annotations remain approval-gated; broken MCP servers or plugin catalogs only skip the offending entry without blocking valid servers.
- **Protocol & state-machine consistency**: `ncx-protocol::ClientRequest` is the single source of truth for all 70 GUI IPC methods (`scripts/check-protocol-version.mjs` enforces zero drift before Vite builds). Durable Goal state transitions, Harness Profile locking, and Thread workspace snapshots are serialized under atomic host gates so stale background workers cannot cross-contaminate projects.
- **Provider & UI defensive bounds**: Non-2xx HTTP responses from OpenAI-compatible and Anthropic endpoints expose only `HTTP <status>` without leaking raw remote bodies or credentials into logs/context. Workspace Changes file previews are capped at 1,000 lines / 192 KiB with single-scroll-owner layout protection.
- **Isolated test fixtures**: Rust and Python test temporary directories use a monotonic per-session ID (timestamp + PID + atomic sequence) so parallel CLI and IDE test runs never collide.

Run the local Tauri development app with:

```powershell
cd rust\gui
npm.cmd ci
npm.cmd run tauri -- dev --target x86_64-pc-windows-msvc
```

The frontend listens on `http://localhost:5179/`. If that port is already
owned by BugleCat, reuse the existing instance and let Vite hot-reload instead
of starting a second development process.

When changing `ncx-protocol::ClientRequest` or `PROTOCOL_VERSION`, regenerate
the checked-in GUI contract before running the build:

```powershell
npm.cmd --prefix rust\gui run protocol:generate
npm.cmd --prefix rust\gui run protocol:check
```

## Release Packaging

Recommended Windows release entry point:

```powershell
.\scripts\build-rust-release.ps1
```

The script runs the Rust workspace tests, builds the Windows MSVC release binary,
creates `releases\BugleCat-<version>-x86_64-pc-windows-msvc.zip`, builds the
Tauri NSIS installer, then writes `releases\SHA256SUMS.txt` and
`releases\release-manifest.json`. Use `-SkipTauri` for a CLI-only package or
`-SkipTests` only after the same target has already passed in CI/local release
validation.

Manual Windows MSVC CLI release:

```powershell
cd rust
cargo build --release --workspace --target x86_64-pc-windows-msvc
```

Manual Tauri desktop installer:

```powershell
cd rust\gui
npm.cmd ci
npm.cmd run tauri:installer
```

The desktop build now targets the Windows NSIS installer explicitly. The
installer is emitted under
`rust\gui\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\`.
The GUI Settings dialog also exposes the resolved `~/.nanocodex/config.toml`
path and buttons to open the config file or its directory.

The [`macOS Release`](.github/workflows/macos-release.yml) workflow builds both
Intel (`x86_64-apple-darwin`) and Apple Silicon (`aarch64-apple-darwin`) macOS
packages and attaches them to the same GitHub Release. It prefers DMGs; when a
runner cannot create disk images, it falls back to a zipped `.app`. It runs automatically for `v*`
tags and can also be started manually with **Actions → macOS Release → Run
workflow**, specifying the release tag. The workflow currently produces
unsigned packages; configure Apple Developer signing and notarization secrets
in GitHub Actions before distributing them broadly outside the App Store.

The Tauri crate deliberately keeps `crate-type = ["lib"]`; changing it to
`cdylib` or `staticlib` previously overflowed the Windows GNU linker's export
table, so validate the release target before changing it.

## Security Notes

- **Never commit real API keys.** `.env`, `*.key`, `*.pem`, token files, and
  local handoff files are git-ignored; `config.toml` / `mcp.toml` live in
  `~/.nanocodex/`, outside the repo.
- The sandbox is **policy-level** on every platform — it gates tool actions,
  writable roots, and shell command lines, but is not kernel isolation: programs
  a command launches run with your permissions. Use `untrusted` or `on-request`
  approval in repositories you do not trust.
- **MCP tools run outside the sandbox** as external subprocesses. Only enable
  servers you trust; the marketplace validates names but does not vet behavior.
- **Hooks run local commands** around tool execution. Treat hook configuration
  like code and review it before enabling it in a project.
- External content (file contents, command output, web/MCP results) is treated
  as untrusted data, not as instructions.

## Collaboration

Interested in coding-agent integration and customization, evaluation and
self-repair research, or joint development? Contact
**[2537676793@qq.com](mailto:2537676793@qq.com)** with your team, use case,
proposed scope, and timeline. Scope and delivery arrangements are discussed
individually. See also the [collaboration overview](nanocodex.html#collaboration).

## License

MIT — see [LICENSE](LICENSE).
