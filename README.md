# BugleCat

English | [简体中文](README.zh-CN.md)

## Latest Update (2026-10-23)

> [!IMPORTANT]
> **Stage 3 Milestone (2026-10-23): Self-Evolving Harness Regularization (RRSI / Harness² Alignment, P0–P4) + Paired Compounding Replay**
> - **P0 — Noise-Calibrated Paired Evaluation (`evals/compounding_replay/analyze.py`) & `python-tabulate` Curriculum**:
>   * **Statistical Rigor**: Replaced single-run anecdotal comparisons with single-arm noise-floor calibration ($\delta_{\text{resolved}}$ via Jeffreys-smoothed Bernoulli variance, $\delta_{\text{cost}}$ via per-task sample variance sum) and paired A/B testing (`_mcnemar_exact` on discordant pairs + 4,000-iteration task-level paired bootstrap 95% CIs on `cost_per_resolved` ratio `B/A` and per-round cost difference `B-A`).
>   * **Second Real-Repo Benchmark (`astanin/python-tabulate`)**: Hardened `mine_tasks.py` (`--body` full-commit search, merge-commit diffing, and an `issue["created_at"] <= fixed_at` guard that filtered 7 pre-GitHub Bitbucket issue ID collisions) and verified 9/9 official gold fixes via `validate_gold.py`.
>   * **3-Round Paired Calibration (`tabulate`, 27 paired runs/arm)**: Arm B reduced `cost_per_resolved` by **21.0%** (`B/A = 0.7904`, paired bootstrap 95% CI `[0.6239, 0.9523]`, excluding `1.0`; mean round cost `1.313 CNY` vs `1.586 CNY`, paired 95% CI `[-0.5207, -0.0685]`) while passing the resolution-rate non-inferiority guard (`22/27` vs `21/27`, `+3.7pt >= -14.9pt`). Note: re-analyzing the earlier 2-round `sqlparse_split` v2 run under the same paired bootstrap yields `B/A = 0.7131`, 95% CI `[0.3693, 1.1586]` (crosses `1.0`, statistically inconclusive).
> - **P1 — `ncx-forge` Offline Evolution RRSI Regularization (`train/`)**:
>   * **Annealed Edit Budgets**: `changed_surfaces` & `changed_lines` enforce per-round surface count (`max(1, max_surfaces - round // 2)`) and line-delta caps (`--initial-max-surfaces` (alias: `--max-surfaces`), `--anneal-surfaces`, `--max-changed-lines`).
>   * **Rule-Based Anti-Memorization Critic (`critic_check`)**: Rejects candidate mutations whose newly added lines reference benchmark task IDs, `check.py`, `_check.py`, or `hidden test`.
>   * **Falsified Hypothesis Ledger & Stagnation Steering**: Logs every candidate to `train/runs/ledger_<stamp>.jsonl`, injects recent rejected mutations into the teacher prompt (`Falsified / Rejected Mutations`), and steers toward the least-explored surface after `--stagnation-limit` stagnant rounds.
>   * **Two-Sided Cost-Aware Gate & Post-Search Ablation Pruning (`--prune`)**: Accepts candidates either via pass-rate improvement (guarded by `--max-token-growth`) or equal-pass token savings (`>= --delta-tokens`), and ablate-prunes zero-benefit surface edits from the final champion.
> - **P2 — Expanded `NCX_GENOME` Evolvable Surfaces (`rust/` + `train/`)**:
>   * Added `[orchestrator_prompts]` (`classify_sys`, `plan_sys`, `decompose_sys`, `worker_sys`, `verify_sys`) and bounded `[memory_config]` (`max_entries` clamped to `1..=32`, `max_chars` clamped to `200..=16000`) to `ncx-core::genome` and `ncx --dump-genome`, with a 100% byte-identical no-op guarantee when `NCX_GENOME` is unset or `{}`.
> - **P3 — Harness² Two-Tier Online Memory Stratification, Negative Guardrail & Cost-Aware Attribution (`ncx-core`)**:
>   * **Module-Level Invariant Promotion (`promote_module_invariant_if_due`) & Regression Guardrail (`record_regression_guardrail`)**: When a module accumulates `>= 2` judge-confirmed (`Accepted`) episodic notes, `MemoryStore` distills a canonical `[Module Invariant: <module_path>]` (`+6` recall priority boost, stable header-hashed `note_id` across in-place updates) and trims older episodic notes for that module to `MAX_EPISODIC_PER_MODULE = 2`, synchronization-rebuilding the SQLite index (`rebuild_from_markdown`). When a rejected turn causes a `pass_to_pass` regression or rollback, it records a `[Regression Guardrail: <module_path>]` warning.
>   * **Tightened Recall Budget & Cost-Aware Feedback (`record_unhelpful_injection`)**: Default recall caps tightened from `8 entries / 4000 chars` to `6 entries / 2400 chars`. On expensive turns (`iterations > 16 || tool_calls > 24 || total_tokens > 80_000`), only recalled notes matching modified file basenames receive `accepted + 1`; unmatched free-rider notes record `injected + 1` only. Live Arm B' verification (`20261023-115758-B-tab-p3-r1`) confirmed `learnings_entries` stayed hard-capped at `6` from task 4 through task 9 (`1.413 CNY` total, `-10.9%` vs Arm A mean).
> - **P4 — Harness² `Propose–Probe–Compose` Across Runtime (`ncx-core`) & Offline Trainer (`train/forge.py`, `train/lesson_lib.py`)**:
>   * **Runtime Multi-Worker File Composition (`compose_with_paths` / `COMPOSE:i,j`) & Retry Synthesis (`build_compose_feedback`)**: When the verifier emits `BEST:i` alongside `COMPOSE:i,j`, `HarnessAgentRunner::compose_workers` promotes the primary worker's changes and composes non-conflicting file changes (disjoint paths or identical SHA-256 fingerprints) from secondary workers while merging their `SubrunTelemetry`. When a multi-worker round fails verification, `build_compose_feedback` synthesizes a structured peer probe digest for the next retry.
>   * **Offline `ncx-forge` PPC (`--h2-k`, `--h2-mode parallel|sequential`, `--h2-blind`) & Cross-Run Lesson Library (`train/lesson_lib.py`)**: Probes $K$ general/domain genome edits plus the incumbent control on the same failing task (`keep_trajectory=True`), builds a deterministic verifier-blind behavioral contrast (`build_contrast`), and synthesizes 1 composed genome + 1 meta-lesson draft (`build_h2_compose_prompt`, falling back to the best valid probe if compose violates edit budgets). `LessonLibrary` (`lessons.jsonl` + `adopted_harnesses.jsonl`) retrieves active lessons by symptom overlap $\times$ Beta posterior `(support+1)/(support+refute+2)`, updates `support`/`refute` after the outer `train + holdout` gate (and `--prune`), and auto-retires lessons at `refute >= support + 2`. `evals/compounding_replay/replay.py` adds `--genome PATH` to evaluate evolved genomes on real-repo replays ([`docs/harness2-forge-integration.zh-CN.md`](docs/harness2-forge-integration.zh-CN.md)).
>
> ---
>
> **Stage 3 Foundation (2026-10-19): Intelligence Compounding Closed-Loop, ACR & 60-Point Singularity Telemetry + Cross-Agent Harness Upgrades**
> - **Autonomous Completion Rate (ACR) & 60-Point Singularity Gate**: Records privacy-safe `TaskOutcome` telemetry across **4 core engineering task categories** (`code_fix_test`, `compile_repair`, `feature_impl`, `doc_refactor`; pure `chat` and subagent worker turns are excluded from the primary denominator). A category crosses the 60-point singularity when rolling 30-day `n >= 30`, `ACR-strict` 95% Wilson confidence lower bound `>= 60%`, and `rework_rate <= 15%`.
> - **Zero-Daemon Implicit & Explicit Followup Signals (`I1–I5` + `L`)**: Automatically resolves post-turn acceptance or rework via `I1` terminal failure/block, `I2` approval/question intervention, `I3` checkpoint `/restore`, `I4` multilingual (ZH/EN/JA + touched-file overlap) correction heuristics, `I5` **`LazyGitUndoDetector`** (`git merge-base --is-ancestor` + `git status --porcelain` across sessions for `git reset` / `git revert` / manual edits), and `L` explicit labels (`/outcome good|bad` in CLI/GUI + turn-indexed `♡`/`♧` assistant message buttons in GUI).
> - **Guarded Online Feedback Loop (`shadow` -> `on` with 14-Day Baseline Gate)**: `[feedback] memory / routing = "off" | "shadow" | "on"` (default `"shadow"`). Switching to `"on"` is guarded by a mandatory **14-day shadow baseline gate** (`>= 14` days of shadow telemetry and `>= 2` core categories with `n >= 30`, overridable via `--force`). When active, it drives bounded Beta-posterior memory re-ranking (`±20%`, scored once per resolved task), low-utility memory demotion (`injected >= 8 && p < 0.25`), and monotonic orchestrator route upgrades (`Simple -> Medium -> Complex`).
> - **Compound Recovery Chain, Worker Isolation & Frequency-Prioritized Promotion**: Captures multi-step `tool_failed -> diagnostic/read -> tool_succeeded` recovery chains (`CompoundRecoveryChain`) as `Candidate` notes, tracks cross-session recurrence (`occurrences`) sorted by hit frequency, isolates subagent worker candidates so only the winning `promoted` worker's path-remapped candidates enter the host store, and supports both high-confidence auto-promotion (`memory_auto_promote_enabled`) and human review (`ncx --memory-review` / GUI Review panel).
> - **Cross-Agent Harness Upgrades (Pi / OpenCode / Codex)**:
>   * **Pi-style In-Conversation Trace Tree (`_trace`)**: Every message in `session.jsonl` embeds a self-contained `MessageTrace` (`id` / `parent_id` tree pointers, `turn_id`, SHA-256 `content_hash`, `workspace_revision`, assistant `model` / `usage`, tool `duration_ms` / `exit_code`, and `CompactionTrace::first_kept_entry_id`), stripped before wire LLM requests.
>   * **OpenCode-style Post-Edit Auto-Diagnostics**: `apply_patch` and `str_replace_editor` automatically run incremental LSP diagnostics (`textDocument/didChange`), in-process JSON/TOML syntax checks, and Git conflict-marker checks, appending diagnostics directly to the tool output.
>   * **Codex-style Large Tool Output Offloading**: Tool outputs exceeding 16,000 chars are persisted in full to disk while returning an inline 8,000-char head + 8,000-char tail preview and a `[tool_output_offloaded: path=... offset=...]` handle for on-demand `read_file` inspection.
> - **CLI & Desktop Entry Points**:
>   * Singularity & ACR Report: `ncx --outcomes report --since 30d --by category,model,policy` (or `/outcome report` in CLI/GUI)
>   * Multi-Project Cold-Start Bootstrap: `ncx --outcomes bootstrap <repo_dir_1> <repo_dir_2> ...`
>   * Candidate Memory Review: `ncx --memory-review [list | confirm <id> | reject <id> | restore <id>]`
>   * Desktop Visual Singularity Dashboard: Open the right-hand **Memory** panel in the Tauri GUI to inspect live 30-day ACR, Wilson 95% lower-bound progress toward 60%, per-category sample status, and 14-day Shadow baseline readiness.
> - **Prebuilt `v0.1.2` Releases & Claim-Level Verification (`host-rules-v3.4`)**:
>   * [GitHub Release `v0.1.2`](https://github.com/dgy-github/BugleCat/releases/tag/v0.1.2) (`BugleCat_0.1.0_x64-setup.exe`, `buglecat-20261014-final.zip`, macOS `arm64`/`x64` DMGs) · [Delivery Receipt](reports/buglecat-delivery-20261014.json) · [175-Case Claim Drift Benchmark Report](reports/claim_drift_eval_report_v1.md)

## Collaboration & self-test evidence

Latest: [September 8 afternoon rerun report](docs/reports/self_repair_20260908_081319.md) — **6/6 on the first attempt**, all six patches independently replayed; 609.09 seconds of agent runtime. Same model and executor as the earlier run below; a small-sample result, not proof of a stable success rate.

Interested in coding-agent integration and customization, evaluation and self-repair research, or joint development? Contact **[2537676793@qq.com](mailto:2537676793@qq.com)** with your team, use case, proposed scope, and timeline. Scope and delivery arrangements are discussed individually.

In the **September 8, 2026** six-category source self-repair test using `deepseek-v4-pro`, BugleCat passed **5/6 tasks on the first attempt** and **6/6 with at most one feedback turn per task**. Independent checks and patch replay verified the results. This is a small, controlled fault-injection test with specified files and visible tests—not a general success-rate claim or production certification.

[Read the self-test report (Chinese)](docs/reports/self_repair_20260908.md) · [Evidence and reproduction instructions](evals/self_repair/README.md) · [Collaboration overview](nanocodex.html#collaboration)

## Capability Page

[Open the capability page](nanocodex.html) · [BugleCat repository](https://github.com/dgy-github/BugleCat)

[Design brief PDF](docs/ai-coding-agent-design-brief.pdf) · [Design brief HTML](docs/ai-coding-agent-design-brief.html)

[Six-category self-repair report (Chinese)](docs/reports/self_repair_20260908.md) · [Evaluation package](evals/self_repair/README.md)

📖 **[设计理念手册（中文）](docs/design-philosophy.zh-CN.md)** — why the tiered orchestrator, recursive decomposition, tool-less reasoning nodes, progressive disclosure, vision routing, and the benchmark methodology are built the way they are.

🛡️ **[Context Compression Safety Protocol（中文）](docs/context-compression-safety-protocol.zh-CN.md)** — why coding agents can drift after context compaction, and how snapshots, decision provenance, Git evidence, and read-only recovery contain the risk.

📈 **[智能复利回流与 60 分奇点方案（中文）](docs/intelligence-compounding-feedback-plan.zh-CN.md)** — how BugleCat closes the online loop from `Action → TaskOutcome → OutcomeFollowup → Learning` (ACR & 60-point singularity metrics, bounded Beta-posterior memory re-ranking, monotonic route upgrades, and compound recovery candidate promotion).

🧬 **[Harness² integration for forge (zh-CN)](docs/harness2-forge-integration.zh-CN.md)** — design draft for adding Harness² Propose–Probe–Compose recursion and a cross-task lessons library to `train/forge.py`, wired into the existing RRSI gates.

🧬 **[自进化 Harness 架构与实现方案（对照 Google RRSI & Harness²，中文）](docs/self-evolving-harness-rrsi-harness2.zh-CN.md)** — how BugleCat unifies L0 within-task `Propose–Probe–Compose` (Harness²), L1 cross-task module invariant stratification & negative regression guardrails (Harness²), and L2 offline `ncx-forge` regularized genome evolution (RRSI), backed by paired bootstrap replay verification.

[![BugleCat desktop screenshot: Tauri + Svelte desktop client](assets/buglecat-desktop-real.png)](nanocodex.html)

**BugleCat** is a local, extensible Codex-style coding agent. A
chat-completions model proposes structured tool calls, the agent runs sandboxed
file/shell/LSP/terminal tools, records the session and event snapshots, and loops until the task is done. It
runs against DeepSeek's hosted API or any OpenAI-compatible local model, and
ships with MCP integration, a plugin ecosystem, a skills system, a sandbox/approval state machine,
context compaction, token-cost accounting, a Tauri 2 + Svelte 5 desktop client, a scheduler, and
git-worktree A/B comparison.

The project evolved from the original `nanocodex` prototype. That legacy name
still appears in compatibility paths (`~/.nanocodex/`), environment variables (`NANOCODEX_*`), and Python
commands so existing installations keep working. The project has evolved through **three clear stages**: from fast product exploration in Python, to a strictly typed native Rust + Tauri runtime, and now to a self-evaluating **intelligence-compounding closed loop**.

## Project Phases

### Stage 1: Python Baseline

The Python implementation under `nanocodex/` is the original, legacy feature-complete
agent line. It was optimized for fast product exploration: prove the agent
loop, tool UX, approval model, and desktop workflows before locking the system
into a stricter runtime.

**Architecture**

- A Python package centered on a compact async agent loop: model call -> tool
  execution -> session update -> next model call.
- Tooling, provider, sandbox, MCP, skills, memory, scheduler, compaction, and
  GUI modules are easy to extend independently while the product surface is
  still changing.
- Runtime contracts are intentionally dynamic, which made it cheap to add
  features such as MCP marketplace entries, prompt enhancement, image input,
  session resume/fork, and A/B worktree comparison.
- The legacy prototype GUI uses Tkinter, keeping the first desktop version dependency
  light and simple to debug.

**Performance and delivery profile**

- Best for iteration speed: no compile step, quick experiments, and a large
  offline test suite around mocked providers.
- Offline unit tests validate behavior without API keys or network calls.
- Runtime delivery still depends on a Python install, package environment, and
  import-time startup cost; desktop distribution is therefore less clean than a
  native binary.
- Dynamic boundaries are productive during exploration, but become harder to
  reason about as sandboxing, tool execution, memory, MCP, and parallel agent
  flows grow.

### Stage 2: Rust Rewrite

The Rust implementation under `rust/` is the native release foundation. It keeps the
Python tree intact while rebuilding the core as small crates plus a Tauri
desktop shell. The rewrite keeps the proven Python feature map, but changes the
internal shape so the project can ship as a smaller, faster, more predictable
tool.

**Architecture**

- The workspace is split by responsibility: `ncx-config`, `ncx-provider`,
  `ncx-sandbox`, `ncx-tools`, `ncx-mcp`, `ncx-context`, `ncx-thread-store`,
  `ncx-protocol`, `ncx-app-server`, `ncx-core`, and `ncx-cli`.
- Core contracts are typed: provider responses, tool calls, sandbox decisions,
  session messages, memory entries, protocol requests, and orchestrator results cross crate
  boundaries explicitly.
- Tool execution is centralized behind `ToolContext` and `ToolRegistry`, so
  sandbox policy, approval policy, timeouts, search, and memory are attached at
  the boundary where actions actually happen.
- `AgentLoop` is assembled from replaceable model providers, named turn-context
  providers, a tool registry, tool middleware, and an injectable scheduler.
  The runtime still owns write barriers and routes every scheduled call through
  `ToolRegistry`, preserving middleware, approval, and sandbox enforcement.
- CLI, GUI, and orchestrated workers resolve budgets, context editing,
  concurrency, and model endpoints through the same `AgentRuntimeProfile`
  assembly path, preventing frontend-specific configuration drift.
- The orchestration layer adds task classification, main/fast model routing,
  isolated worker workspaces, verifier selection, and promotion of the winning
  worker back into the real workspace.
- Project memory is local to `.ncx/memory/LEARNINGS.md`; startup uses cheap
  heuristic deduplication, while `ncx --memory-merge` runs the more expensive
  LLM-backed consolidation only as an explicit maintenance command.
- The desktop line moves to Tauri v2 + Svelte 5, separating the native backend
  from the UI surface and building native Windows NSIS installers and macOS DMGs.

**Performance and delivery profile**

- The CLI builds to a native `ncx.exe`, so users do not need Python, virtual
  environments, or editable installs.
- CLI release builds use strip, LTO, size optimization, and the Windows MSVC target
  used by the desktop installer;
  the CLI package includes README, license, and config example files.
- Startup avoids Python interpreter/import overhead and is suitable for short
  one-shot commands as well as interactive REPL use.
- Typed ownership makes parallel worker isolation and result promotion easier
  to reason about without shared mutable state leaks.
- The workspace test suite covers provider and runtime contracts, sandbox and
  approval policy, tool scheduling and recovery, MCP replacement, persistent
  processes, LSP, raw PTY sessions, GUI assembly, claim verification, and orchestration.

**Platform control-plane upgrades**

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
  Windows child processes are contained in Job Objects.
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

### Why Rust For Stage 2

Rust was chosen because the second stage is about productizing the agent, not
only adding more features:

- **Architecture hardening:** the sandbox, approval engine, provider adapter,
  tool registry, memory store, and orchestrator now have explicit typed
  contracts instead of relying on Python's dynamic object boundaries.
- **Predictable action boundary:** file, shell, search, and memory operations
  all pass through one tool context, which keeps approval and sandbox checks
  close to execution.
- **Parallel orchestration:** isolated worker copies, verifier selection, and
  result promotion are safer when ownership is explicit and data movement is
  visible in the type system.
- **Runtime control plane:** task budgets, context editing, tool search, and
  semantic memory sit in the Rust runtime boundary rather than depending on
  model-side conventions alone.
- **Native release performance:** a small `ncx.exe` starts without interpreter
  setup, making one-shot CLI tasks feel immediate and making distribution much
  easier for Windows users.
- **Desktop packaging path:** Tauri provides a native shell with a web UI
  frontend, a better long-term packaging fit than growing the Tkinter prototype.

### Stage 3: Intelligence Compounding & 60-Point Singularity Closed-Loop (Current)

Stage 2 built a reliable execution harness, but every task still started statistically cold: `TurnResult` was discarded after cost accounting, recalled memories had no outcome attribution, orchestrator routing ignored historical success rates, and post-turn human corrections or Git rollbacks were invisible. **Stage 3** upgrades BugleCat into a closed-loop compounding system (`Action → TaskOutcome → OutcomeFollowup → Learning`), designed around the **60-Point Singularity** (the structural threshold where an AI agent transitions from "humans backstopping AI" to "AI owning the engineering outcome"):

1. **Four-Category Task Outcome & 60-Point Singularity Telemetry (`G1`)**
   - Every primary turn—both single-agent turns and multi-agent orchestrated runs (`{session}:orch-{turn}`)—emits a privacy-safe `TaskOutcome` event into SQLite (`context_store`), classified into **4 core engineering task categories** to prevent sample fragmentation:
     - `code_fix_test` (bugfixes, test repairs, assertions)
     - `compile_repair` (compiler, typecheck, and lint error repairs)
     - `feature_impl` (new feature implementation and wiring)
     - `doc_refactor` (documentation, configuration, and structural refactoring)
   - Pure Q&A turns (`chat`) and subagent worker/node turns (`OrchestratorWorker`, `OrchestratorNode`) are recorded for trace completeness but **excluded from the primary ACR denominator**. Completed turns without a resolved followup signal remain `pending` rather than inflating success rates.
   - **60-Point Singularity Criterion**: A core task category crosses the singularity when rolling 30-day resolved tasks `n >= 30`, `ACR-strict` 95% Wilson confidence lower bound `>= 60%`, and `rework_rate <= 15%`.

2. **Zero-Daemon Multi-Signal Followup Resolution (`I1–I5` & `L`)**
   - `I1` (Explicit block/failure): terminal `NeedsHuman`, `Failed`, or `Cancelled`.
   - `I2` (Explicit inquiry): sandbox approval prompts (`approvals_requested` / `approvals_denied`) and `ask_user_question` invocations (`ACR-strict` penalizes all `I2`; `ACR-approval-tolerant` allows approved escalations).
   - `I3` (Checkpoint rollback): `/restore <id>` in CLI or checkpoint restoration in GUI attributes a `Rollback` followup to the most recent primary task created after that checkpoint.
   - `I4` (Multilingual correction heuristics): next-turn `DirectHuman` prompts matching deterministic ZH/EN/JA correction patterns (or referencing the same touched files) record an inferred `Correction` followup.
   - `I5` (**Zero-daemon `LazyGitUndoDetector`**): at session start and before the next primary turn (including GUI multi-agent orchestrated turns), BugleCat inspects the previous turn's recorded `workspace_revision` (Git `HEAD` SHA) and `files_touched`. Using `git merge-base --is-ancestor` and `git status --porcelain`, it detects cross-session `git reset --hard`, `git revert`, or manual edits to agent-touched files without any background daemon.
   - `L` (Explicit human labels, highest priority): CLI/GUI `/outcome good|bad [note]`, A/B worktree adoption, and **turn-indexed `♡` / `♧` rating buttons** on every assistant message in the GUI accurately bind human verdicts to that specific turn's `TaskOutcome`.

3. **Guarded Online Feedback (`G2`: Bounded Memory Re-Ranking & Monotonic Route Upgrades)**
   - **14-Day Shadow Baseline Gate**: Both `[feedback] memory` and `[feedback] routing` default to `"shadow"` (which behaves identically to `"off"` while logging counterfactual diffs in `memory_rank_changed` and `shadow_recommended_route`). Switching either setting to `"on"` in CLI `/config` or GUI Settings is enforced by `check_shadow_baseline_gate`: requires `>= 14` days of shadow telemetry and `>= 2` core task categories with `n >= 30` resolved tasks (manual override available via `--force`).
   - **Attributable Memory Recall & Bounded Beta-Posterior Scoring**: `MemoryStore::recall_entries` attaches deterministic content-hash `note_id`s to `TaskOutcome.recalled_note_ids` (unioned across promoted orchestrator workers). When a task resolves, `memory_feedback` updates `injected / accepted / rejected` **once per task** (flipping cleanly if a later human `Label` overrides an inferred signal). Beta posterior `p = (accepted + 1) / (injected + 2)` adjusts lexical retrieval scores within a strict **`±20%` bound**, while notes with `injected >= 8` and `p < 0.25` in `"on"` mode transition to `NoteStatus::Demoted` (excluded from recall, restorable via `ncx --memory-review restore <id>`).
   - **Monotonic Orchestrator Route Upgrades**: `RoutingStats` aggregates 30-day ACR and cost medians by `(category, initial_complexity, route)`. In `"on"` mode, if a category's 95% Wilson lower bound on `Simple` (or `Medium`) drops below `40%` (`n >= 20`) and a higher pipeline tier improves the lower bound by `>= 10%`, the orchestrator upgrades the route (`Simple -> Medium -> Complex`), never downgrading automatically.

4. **Compound Recovery Chain, Isolated Worker Guard & Frequency-Prioritized Promotion**
   - **Multi-Step Recovery Extraction (`CompoundRecoveryChain`)**: Captures `failed_tool -> intermediate diagnostic/read -> recovery_tool` chains within a completed, verified turn as structured `Candidate` notes in SQLite (`memory_index.sqlite`).
   - **Worker Isolation & Path Remapping**: Subagent workers running inside temporary `.buglecat-subruns` directories never write candidates directly to the host store; `HarnessAgentRunner` remaps scratch workspace paths back to the real project root and persists candidates **only from the winning `promoted` worker**.
   - **Recurrence Counting (`occurrences`) & High-Confidence Auto-Promotion**: Repeatedly encountered candidates increment `occurrences` and sort by `occurrences DESC, created_at DESC` (`命中 N 次`). When `memory_auto_promote_enabled = true` (default), candidates from zero-intervention, verification-passed turns automatically promote to `.ncx/memory/LEARNINGS.md`; all other candidates remain isolated until confirmed via `ncx --memory-review confirm <id>` or the GUI Memory Review panel.

5. **Multi-Project Cold-Start Bootstrap & Desktop Visual Singularity Dashboard**
   - **Cross-Project Bootstrap (`ncx --outcomes bootstrap <paths...>`)**: Scans historical `.nanocodex/session.jsonl` transcripts across multiple local repositories, backfills `_trace` metadata, and cold-starts global `TaskOutcome` and `OutcomeFollowup` telemetry in `~/.nanocodex/global-outcomes/context.sqlite`.
   - **GUI ACR & 60-Point Singularity Dashboard**: The Tauri + Svelte 5 right-hand **Memory** panel renders a live visual dashboard displaying 30-day `ACR-strict`, Wilson 95% lower-bound progress bar toward the 60% singularity threshold, per-category sample progress (`n/30`) and singularity badges, and 14-day Shadow baseline readiness.

6. **Self-Evolving Harness Regularization (P0–P4: Paired Replay Calibration, RRSI Forge, Two-Tier Memory & Propose–Probe–Compose)**
   - **Noise-Calibrated Paired Evaluation (`evals/compounding_replay/analyze.py`)**: Calibrates single-arm noise floors ($\delta_{\text{resolved}}$, $\delta_{\text{cost}}$) and evaluates multi-round curricula (`sqlparse`, `python-tabulate`) with exact McNemar tests and task-level paired bootstrap 95% confidence intervals on `cost_per_resolved` (`B/A = 0.7904`, 95% CI `[0.6239, 0.9523]` across 3 paired rounds on `python-tabulate`).
   - **Offline Evolution Regularization (`train/forge.py`, `NCX_GENOME`)**: Extends `NCX_GENOME` to cover `system_prompt`, `tool_descriptions`, `orchestrator_prompts` (`classify_sys`, `plan_sys`, `decompose_sys`, `worker_sys`, `verify_sys`), and bounded `memory_config` (`max_entries`, `max_chars`), regularized by annealed surface/line edit budgets, a rule-based anti-memorization critic (`critic_check`), a falsified hypothesis ledger (`ledger_<stamp>.jsonl`), stagnation surface steering, a two-sided token-cost acceptance gate, and post-search ablation pruning (`--prune`).
   - **Harness² Two-Tier Online Memory & Negative Regression Guardrails (`MemoryStore::promote_module_invariant_if_due`, `record_regression_guardrail`)**: Separates transient per-task episodic notes (capped at `MAX_EPISODIC_PER_MODULE = 2` per module) from persistent module-level invariants (`[Module Invariant: <module_path>]`, `+6` recall priority boost, stable header-hashed `note_id`) and negative regression guardrails (`[Regression Guardrail: <module_path>]` on `pass_to_pass` regressions/rollbacks), paired with cost-aware feedback attribution (`record_unhelpful_injection`) on high-cost turns.
   - **Harness² Within-Task `Propose–Probe–Compose` (`orchestrator.rs`, `workspace_promotion.rs`)**: Supports multi-worker non-conflicting file composition (`compose_with_paths` / `COMPOSE:i,j`) when the verifier selects complementary worker outputs, and cross-worker probe digest synthesis (`build_compose_feedback`) when multi-worker attempts enter retry rounds.

## Table of Contents

- [Project Phases](#project-phases)
- [Highlights](#highlights)
- [Architecture](#architecture)
- [Tools](#tools)
- [Install](#install)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Custom Slash Commands](#custom-slash-commands)
- [Local Model / OpenAI-Compatible Endpoint](#local-model--openai-compatible-endpoint)
- [Sandbox & Approval](#sandbox--approval)
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
│   ├── ncx-tools/         # process executor (Windows Job Object), managed jobs, PTY, file & patch tools
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
memory_auto_promote_enabled = true # auto-promote high-confidence zero-intervention recovery candidates

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

## Sandbox & Approval

Two orthogonal axes gate every action, mirroring Codex:

**Sandbox mode** — what's physically allowed:

| Mode | Reads | Writes | Network |
| --- | --- | --- | --- |
| `read-only` | anywhere | none | off |
| `workspace-write` | anywhere | workspace + writable roots + temp | off unless enabled |
| `danger-full-access` | anywhere | anywhere | on |

**Approval policy** — what to do when an action exceeds the sandbox:
`untrusted`, `on-failure`, `on-request`, `never`. The approval engine resolves
each escalation to `ASK` / `AUTO_APPROVE` / `AUTO_DENY`.

On Windows enforcement is **policy-level**: path checks and writable-root gating
happen at the tool boundary. It is not kernel isolation.

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
  - **Candidate (`Candidate`)**: multi-step `tool_failed -> diagnostic/read -> tool_succeeded` recovery chains (`CompoundRecoveryChain`) are captured with a cross-session recurrence counter (`occurrences`, sorted by `occurrences DESC, created_at DESC`). Isolated subagent workers remap scratch paths and only promote candidates from the winning worker. High-confidence candidates from zero-intervention, verified turns auto-promote when `memory_auto_promote_enabled = true`; others await `ncx --memory-review confirm <id>` or GUI confirmation.
  - **Active (`Active`)**: recalled via hybrid lexical-semantic ranking, attributed in `TaskOutcome.recalled_note_ids`, and re-ranked within a bounded **`±20%`** window using Beta-posterior outcome scores (`p = (accepted + 1) / (injected + 2)`).
  - **Demoted (`Demoted`)**: notes with `injected >= 8` and `p < 0.25` in `[feedback] memory = "on"` mode are automatically demoted (excluded from recall while preserving the row) and can be restored via `ncx --memory-review restore <id>`.
- **AGENTS.md / CLAUDE.md** — project instructions layered from
  `~/.codex/AGENTS.md` and `~/.claude/CLAUDE.md`, then every `AGENTS.md`,
  `CLAUDE.md`, and `.claude/CLAUDE.md` from the repo root down to the workspace,
  so nested directories refine their parents. Total size is capped so a huge
  file can't blow the context. Rust CLI, orchestrator workers, and the Tauri GUI
  all inject this block at session startup.

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

The BugleCat desktop uses Tauri 2 + Svelte 5 on Windows (`rust/gui`):

- Streaming chat with reasoning/answer separation, Stop button, and **turn-indexed `♡` / `♧` rating buttons** on every assistant turn (binding human `Label` verdicts directly to that turn's `TaskOutcome`).
- **Visual ACR & 60-Point Singularity Dashboard** in the right-hand **Memory** panel: displays 30-day `ACR-strict`, Wilson 95% lower-bound progress toward the 60% singularity threshold, per-category sample counts (`n/30`) and singularity badges across the 4 core engineering categories, 14-day Shadow baseline readiness, and interactive Candidate/Demoted memory review (`Confirm` / `Reject` / `Restore` with recurrence counts `命中 N 次`).
- Project switcher, model switcher, `/outcome good|bad|report` slash commands, and a multi-section Settings page (including `memory_auto_promote_enabled` toggle).
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
- The sandbox is **policy-level on Windows** — it gates tool actions and writable
  roots, but is not kernel isolation.
- **MCP tools run outside the sandbox** as external subprocesses. Only enable
  servers you trust; the marketplace validates names but does not vet behavior.
- **Hooks run local commands** around tool execution. Treat hook configuration
  like code and review it before enabling it in a project.
- External content (file contents, command output, web/MCP results) is treated
  as untrusted data, not as instructions.

## License

MIT — see [LICENSE](LICENSE).
