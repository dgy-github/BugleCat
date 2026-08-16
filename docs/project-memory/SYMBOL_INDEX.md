# 项目符号记忆索引

> 由 `python scripts/generate_project_memory.py` 自动生成，请勿手工编辑。
> 新功能开发前搜索本文件；模块或公共符号变化后重新生成。

## Python

| 路径 | 行 | 类型 | 名称 | 摘要 |
| --- | ---: | --- | --- | --- |
| `nanocodex/__init__.py` | 1 | module | `__init__` | nanocodex: a minimal Codex-style coding agent on a DeepSeek backend. Architecture (independent rewrite, inspired by nanobot's agent loop): config -> resolve model / sandbox / appro |
| `nanocodex/__main__.py` | 1 | module | `__main__` |  |
| `nanocodex/agent/__init__.py` | 1 | module | `__init__` | Agent layer: prompt, session, and the turn loop. |
| `nanocodex/agent/ab_compare.py` | 1 | module | `ab_compare` | A/B configuration comparison: run one task under two configs, compare. The user picks two configurations (each a set of ``_build_loop`` overrides — model / reasoning_effort / sandb |
| `nanocodex/agent/ab_compare.py` | 39 | class | `ABConfig` | One side of the comparison: a label + a set of _build_loop overrides. |
| `nanocodex/agent/ab_compare.py` | 47 | class | `ABResult` | One side's outcome after running the task in its worktree. |
| `nanocodex/agent/ab_compare.py` | 62 | function | `build_result` | Assemble an ABResult from a finished TurnResult (pure). Pulls the model out of the config's overrides to price the run's usage. A missing/unknown model price yields cost=None (show |
| `nanocodex/agent/ab_compare.py` | 93 | function | `_fmt_cost` |  |
| `nanocodex/agent/ab_compare.py` | 101 | function | `_diff_stat` | Tiny +added/-removed summary of a unified diff (pure). |
| `nanocodex/agent/ab_compare.py` | 116 | function | `summarize_result` | One-side human-readable summary (pure). |
| `nanocodex/agent/ab_compare.py` | 131 | function | `format_ab_comparison` | Side-by-side comparison text for the result dialog (pure). |
| `nanocodex/agent/ab_compare.py` | 142 | function | `worktree_name` | Deterministic, filesystem-safe worktree dir name (pure, testable). *token* is an injected unique id (the GUI passes a timestamp/uuid) so the name is stable for a given (label, toke |
| `nanocodex/agent/ab_compare.py` | 156 | class | `ABGitError` | Raised when the workspace can't host an A/B run (not git / dirty / etc). |
| `nanocodex/agent/ab_compare.py` | 160 | function | `_git` | Run a git command, returning stdout. Raises ABGitError on failure. |
| `nanocodex/agent/ab_compare.py` | 176 | function | `ensure_clean_git_workspace` | Verify *workspace* is a git repo with no uncommitted changes. Returns the current HEAD commit hash (the shared A/B base). Raises ABGitError with a user-facing message otherwise. |
| `nanocodex/agent/ab_compare.py` | 193 | function | `create_worktree` | Add a detached git worktree at *tmp_root/name* on *base_commit*. |
| `nanocodex/agent/ab_compare.py` | 200 | function | `collect_worktree_diff` | Return a unified diff of all changes (staged+unstaged) in *worktree*. Stages everything first (so new/untracked files appear in the diff) but never commits — the diff is the artifa |
| `nanocodex/agent/ab_compare.py` | 210 | function | `adopt_diff` | Apply a collected diff onto the real *workspace* (no commit). Leaves the changes in the working tree for the user to review/commit, the same as if the agent had edited the files di |
| `nanocodex/agent/ab_compare.py` | 230 | function | `cleanup_worktree` | Remove a worktree (force), best-effort — never raises. |
| `nanocodex/agent/agents_md.py` | 1 | module | `agents_md` | AGENTS.md project-instruction loading, Codex-style. Codex layers project instructions from several sources, outermost first: 1. A global file at ``~/.codex/AGENTS.md`` (user-wide d |
| `nanocodex/agent/agents_md.py` | 24 | class | `AgentsDoc` | One discovered AGENTS.md: where it came from and its text. |
| `nanocodex/agent/agents_md.py` | 32 | class | `AgentsInstructions` |  |
| `nanocodex/agent/agents_md.py` | 36 | function | `is_empty` |  |
| `nanocodex/agent/agents_md.py` | 39 | function | `render` | Concatenate all docs into a single block for the system prompt. |
| `nanocodex/agent/agents_md.py` | 49 | function | `_git_root` | Walk up from *start* looking for a .git directory. |
| `nanocodex/agent/agents_md.py` | 57 | function | `_read` |  |
| `nanocodex/agent/agents_md.py` | 67 | function | `discover_agents` | Collect AGENTS.md docs: global, then git-root .. workspace (outermost first). |
| `nanocodex/agent/auto_reasoning.py` | 1 | module | `auto_reasoning` | Adaptive reasoning-effort selection for the ``auto`` tier. Ported from DeepSeek-TUI's ``auto_reasoning.rs`` (#663), adapted to nanocodex. nanocodex's config defaults ``reasoning_ef |
| `nanocodex/agent/auto_reasoning.py` | 73 | function | `select_auto_effort` | Pick a concrete reasoning tier for the next request (pure). Returns one of ``"max"`` / ``"high"`` / ``"low"``. Mirrors the rule order of DeepSeek-TUI's ``select``: sub-agent first, |
| `nanocodex/agent/auto_reasoning.py` | 90 | function | `_content_to_text` | Flatten a message ``content`` (str or content-block list) to plain text. |
| `nanocodex/agent/auto_reasoning.py` | 103 | function | `last_user_text` | Return the text of the most recent ``user`` message, or "" if none. Tolerates both plain-string content and the content-block list shape used when images are attached. |
| `nanocodex/agent/auto_reasoning.py` | 115 | function | `resolve_effort` | Map a configured effort to a concrete tier when it's ``"auto"``. * ``configured`` is None/empty → return it unchanged (provider default). * ``configured`` is an explicit tier (``hi |
| `nanocodex/agent/compaction.py` | 1 | module | `compaction` | Context compaction: keep the prompt within a token budget. Long conversations grow without bound; once the message list exceeds the model's context window the backend truncates or  |
| `nanocodex/agent/compaction.py` | 42 | class | `CompactionConfig` | When and how to compact. |
| `nanocodex/agent/compaction.py` | 53 | function | `enabled` |  |
| `nanocodex/agent/compaction.py` | 57 | function | `estimate_tokens` | Cheap, deterministic token estimate (no tokenizer, no network). |
| `nanocodex/agent/compaction.py` | 75 | function | `_first_user_index` |  |
| `nanocodex/agent/compaction.py` | 82 | function | `_drop_orphan_tools` | Remove tool messages whose tool_call_id has no preceding assistant call. |
| `nanocodex/agent/compaction.py` | 99 | function | `_digest` | Deterministic, zero-cost summary of the folded middle. |
| `nanocodex/agent/compaction.py` | 119 | async function | `compact` | Return a compacted copy of *messages* if over budget, else the original. Structure of the result: [system?, (summary user msg), <recent tail>]. The tail always begins at a user mes |
| `nanocodex/agent/enhance_prompt.py` | 1 | module | `enhance_prompt` | Input prompt enhancement: rewrite a user's raw input into a clearer prompt. The GUI's ✨ button takes whatever the user typed and, before sending it as a turn, asks the model to rew |
| `nanocodex/agent/enhance_prompt.py` | 44 | function | `should_enhance` | True when *text* is worth sending to the rewrite model. Skips empty/whitespace, slash-ish meta lines, and over-long inputs (a big paste is already explicit; rewriting it just burns |
| `nanocodex/agent/enhance_prompt.py` | 58 | function | `build_enhance_messages` | Build the chat messages for one rewrite call (pure). A fixed system instruction plus the raw user text as the only user turn. No tools, no history — enhancement is a stateless tran |
| `nanocodex/agent/enhance_prompt.py` | 70 | function | `clean_enhanced` | Tidy the model's rewrite, falling back to *original* when it's unusable. Strips surrounding whitespace and a single layer of wrapping quotes/fences the model sometimes adds despite |
| `nanocodex/agent/enhance_prompt.py` | 85 | function | `_strip_code_fence` | Remove a single ``` fenced block wrapper if the whole text is fenced. |
| `nanocodex/agent/enhance_prompt.py` | 96 | function | `_strip_wrapping_quotes` | Remove one layer of matching surrounding quotes, if the whole is wrapped. |
| `nanocodex/agent/fact_merge.py` | 1 | module | `fact_merge` | Merge facts from concurrent research workers, surfacing conflicts. The review's concurrency finding: two research workers can reach opposite conclusions about the same thing, and i |
| `nanocodex/agent/fact_merge.py` | 34 | function | `_normalize` |  |
| `nanocodex/agent/fact_merge.py` | 38 | function | `_subject_value` |  |
| `nanocodex/agent/fact_merge.py` | 46 | class | `MergeOutcome` |  |
| `nanocodex/agent/fact_merge.py` | 52 | function | `merge_facts` | Dedup *facts* and flag subject-level contradictions as disputed. |
| `nanocodex/agent/images.py` | 1 | module | `images` | Image input: build OpenAI multimodal content blocks from local files. Honesty note on model support ------------------------------ Attaching an image only helps if the *model* can  |
| `nanocodex/agent/images.py` | 35 | class | `ImageError` | Raised when an image cannot be read or is unsupported. |
| `nanocodex/agent/images.py` | 39 | function | `detect_mime` | Detect image MIME from magic bytes, falling back to the extension. |
| `nanocodex/agent/images.py` | 50 | function | `encode_image_block` | Return a single OpenAI ``image_url`` content block for *path*. |
| `nanocodex/agent/images.py` | 74 | function | `build_user_content` | Build a user message's content. With no images, returns the plain text string (cheapest, most compatible). With images, returns a multimodal block list: the text block first, then  |
| `nanocodex/agent/loop.py` | 1 | module | `loop` | The agent turn loop: call model, run tools, feed results, repeat until done. |
| `nanocodex/agent/loop.py` | 25 | class | `LoopHooks` |  |
| `nanocodex/agent/loop.py` | 36 | function | `wants_streaming` |  |
| `nanocodex/agent/loop.py` | 41 | class | `TurnResult` |  |
| `nanocodex/agent/loop.py` | 51 | class | `AgentLoop` | Drive one user turn to completion. |
| `nanocodex/agent/loop.py` | 54 | function | `__init__` |  |
| `nanocodex/agent/loop.py` | 78 | function | `_active_provider` | The provider for THIS turn: the vision backend when routing, else main. Set per-turn in run_turn (`_use_vision_this_turn`) so only image-bearing turns hit the VL model; text/coding |
| `nanocodex/agent/loop.py` | 88 | function | `_streaming_active` | True when the caller wants streaming and the active provider can do it. |
| `nanocodex/agent/loop.py` | 94 | async function | `_prepared_messages` | Session messages, compacted to the token budget when configured. |
| `nanocodex/agent/loop.py` | 101 | async function | `_call_model` | Call the provider, streaming deltas through hooks when enabled. |
| `nanocodex/agent/loop.py` | 126 | function | `_is_parallel_safe` | True when this call's tool is read-only (safe to run concurrently). |
| `nanocodex/agent/loop.py` | 131 | async function | `_execute_cancellable` | Run one tool call, but abandon (and let it be killed) on Stop. Tool execution can block for a long time — a hung shell command, a launched process that never returns. Cooperative c |
| `nanocodex/agent/loop.py` | 157 | async function | `run_turn` |  |
| `nanocodex/agent/loop.py` | 179 | function | `_cancelled` |  |
| `nanocodex/agent/loop.py` | 230 | function | `_cancelled_result` |  |
| `nanocodex/agent/loop.py` | 243 | async function | `_run_one` |  |
| `nanocodex/agent/loop.py` | 299 | function | `_has_image_block` | True when the user content carries at least one image_url block. A plain string is text-only. A multimodal content list (built by images.build_user_content) carries image_url block |
| `nanocodex/agent/loop.py` | 314 | function | `_dump_args` |  |
| `nanocodex/agent/memory_store.py` | 1 | module | `memory_store` | User memory: a persistent personal note file the model sees every turn. Ported from DeepSeek-TUI's ``memory.rs`` MVP, adapted to nanocodex's idiom (plain file the user owns, pure f |
| `nanocodex/agent/memory_store.py` | 42 | function | `_now_stamp` | Local timestamp for a captured bullet (date + minute is enough). |
| `nanocodex/agent/memory_store.py` | 47 | function | `load` | Read the memory file, or None when it's absent or blank after trimming. |
| `nanocodex/agent/memory_store.py` | 57 | function | `as_system_block` | Wrap memory text in a ``<user_memory>`` block for the system prompt (pure). Returns "" for empty content. Oversized content is truncated to ``_MAX_MEMORY_CHARS`` with a marker so t |
| `nanocodex/agent/memory_store.py` | 82 | function | `render_for_prompt` | Convenience: load + wrap in one call. "" when there's no memory. |
| `nanocodex/agent/memory_store.py` | 91 | function | `format_bullet` | Render one timestamped markdown bullet (pure). Collapses inner newlines. |
| `nanocodex/agent/memory_store.py` | 98 | class | `MemoryStore` | Append-and-read wrapper over the user memory file. |
| `nanocodex/agent/memory_store.py` | 101 | function | `__init__` |  |
| `nanocodex/agent/memory_store.py` | 104 | function | `load` |  |
| `nanocodex/agent/memory_store.py` | 107 | function | `render_for_prompt` |  |
| `nanocodex/agent/memory_store.py` | 110 | function | `append` | Append a timestamped bullet. Returns the bullet line written. Raises ValueError on an empty note. Creates the file (and parent dir) on first write. A heading is added once so a fre |
| `nanocodex/agent/mentions.py` | 1 | module | `mentions` | Expand ``@path`` file mentions in a user prompt into inline file context. Codex-style: typing ``@src/foo.py`` in a message pulls that file's contents into the turn so the model see |
| `nanocodex/agent/mentions.py` | 27 | function | `find_mentions` | Return the @-mention path tokens in order (trailing punctuation trimmed). |
| `nanocodex/agent/mentions.py` | 37 | function | `expand_file_mentions` | Append inline file context for each @mention that resolves to a readable file. The original text is preserved (the @mention stays inline so the model can correlate it); one fenced  |
| `nanocodex/agent/orch_prompts.py` | 1 | module | `orch_prompts` | System prompts for the orchestrator's roles. Kept as module constants (not files) so the orchestrator has no runtime file IO and the prompts are versioned with the code that depend |
| `nanocodex/agent/orch_prompts.py` | 74 | function | `node_brief` | The per-node user message handed to a worker (role system prompt is separate). |
| `nanocodex/agent/orchestrator.py` | 1 | module | `orchestrator` | OrchestratorLoop: plan -> execute -> verify -> replan over a task graph. This sits ABOVE the existing AgentLoop. AgentLoop stays the worker runtime; the orchestrator owns planning, |
| `nanocodex/agent/orchestrator.py` | 44 | class | `OrchestratorResult` |  |
| `nanocodex/agent/orchestrator.py` | 51 | class | `OrchestratorLoop` |  |
| `nanocodex/agent/orchestrator.py` | 52 | function | `__init__` |  |
| `nanocodex/agent/orchestrator.py` | 75 | function | `_cancelled` |  |
| `nanocodex/agent/orchestrator.py` | 79 | async function | `_plan` | Run the planner, validate its graph, retrying on invalid output. Returns True if a valid plan is in place. The validation is pure code — the model never gets to push an illegal gra |
| `nanocodex/agent/orchestrator.py` | 111 | async function | `run` |  |
| `nanocodex/agent/orchestrator.py` | 128 | async function | `_execute` |  |
| `nanocodex/agent/orchestrator.py` | 160 | async function | `_run_node` |  |
| `nanocodex/agent/orchestrator.py` | 181 | function | `_reconcile_facts` | Merge facts after a concurrent research batch; route conflicts out. Conflicting facts are marked disputed (so workers' briefs exclude them) and a needs_clarification node is added  |
| `nanocodex/agent/orchestrator.py` | 206 | async function | `_run_worker` |  |
| `nanocodex/agent/orchestrator.py` | 220 | async function | `_gate_and_verify` | The anti-"fake done" gate, then independent verification. If the worker exited WITHOUT calling request_verification, its node is still `running` here (request_verification flips it |
| `nanocodex/agent/orchestrator.py` | 257 | async function | `_run_verify_node` |  |
| `nanocodex/agent/orchestrator.py` | 273 | function | `_apply_failure` | Retry the node (per-node budget) or mark it failed. Corrective actions from the verifier are threaded into the node's inputs so the retry worker sees what to fix — a lightweight re |
| `nanocodex/agent/orchestrator.py` | 289 | function | `_terminal_result` |  |
| `nanocodex/agent/orchestrator.py` | 303 | function | `_finish` |  |
| `nanocodex/agent/orchestrator.py` | 309 | function | `_parse_plan` | Parse a planner's JSON into TaskNodes. Returns (nodes, error_message). |
| `nanocodex/agent/pricing.py` | 1 | module | `pricing` | Token cost estimation from DeepSeek's published per-token prices. The provider returns a real ``usage`` dict per model call (prompt/completion tokens, plus DeepSeek's cache-hit/mis |
| `nanocodex/agent/pricing.py` | 40 | class | `ModelPrice` | USD per 1,000,000 tokens for one model. |
| `nanocodex/agent/pricing.py` | 95 | function | `estimate_seedance_cost_cny` | Estimate CNY cost for *total_seconds* of 720p video BEFORE rendering. Multiplies measured tokens/second by the package rate. This is an ESTIMATE for budgeting (e.g. "8 shots, 46s - |
| `nanocodex/agent/pricing.py` | 116 | function | `is_seedance` | True if *model* is a Seedance video model (priced in CNY, not USD). |
| `nanocodex/agent/pricing.py` | 121 | function | `seedance_cost_cny` | CNY cost of one Seedance task from its response ``usage`` dict. Bills ``usage.total_tokens`` at the package rate (37 CNY/1M without video input, 22 CNY/1M with). Returns ``None`` w |
| `nanocodex/agent/pricing.py` | 144 | function | `unsupported_reason` | Return why *model* is intentionally not priced, or None if not listed. Same matching as :func:`price_for` -- exact name, then longest known prefix -- so a dated/suffixed variant (` |
| `nanocodex/agent/pricing.py` | 162 | function | `price_for` | Look up the price for *model*, or None if we don't have it. Matches the exact name first, then a longest known-prefix (so a dated or suffixed variant like ``deepseek-v4-pro-0606``  |
| `nanocodex/agent/pricing.py` | 180 | function | `cost_usd` | USD cost of one model call from its usage dict. Returns ``None`` when the model price is unknown or usage is empty, so the caller can show "cost unknown" instead of a misleading `` |
| `nanocodex/agent/pricing.py` | 212 | function | `add_usage` | Accumulate one usage dict into a running total (pure; returns a new dict). Sums prompt/completion and the cache hit/miss fields so a whole turn (or a whole session) can be priced a |
| `nanocodex/agent/pricing.py` | 232 | function | `_as_int` |  |
| `nanocodex/agent/prompt.py` | 1 | module | `prompt` | The Codex-style system prompt. |
| `nanocodex/agent/prompt.py` | 50 | function | `build_system_prompt` | Compose the system prompt with sandbox context and AGENTS.md guidance. *memory* is an already-rendered ``<user_memory>`` block (see memory_store.render_for_prompt). When present it |
| `nanocodex/agent/roles.py` | 1 | module | `roles` | Role-based tool isolation — enforced in code, not in the prompt. The review's finding: "planner doesn't edit / verifier can't edit" is worthless if it lives only in a prompt the mo |
| `nanocodex/agent/roles.py` | 50 | class | `Role` | A worker role: the capability tags it may use and whether it's read-only. |
| `nanocodex/agent/roles.py` | 57 | function | `grants` |  |
| `nanocodex/agent/roles.py` | 106 | function | `_read_only_ctx` | A clone of *ctx* whose sandbox forbids writes and network. The shell a read-only role receives runs through this policy/executor, so it physically cannot write — independent of wha |
| `nanocodex/agent/roles.py` | 116 | function | `build_role_registry` | Build a ToolRegistry exposing only the tools *role_name* is allowed. Raises KeyError for an unknown role (callers must use a defined role). |
| `nanocodex/agent/schedule.py` | 1 | module | `schedule` | Scheduled tasks: run a saved prompt automatically at a future time. A scheduled task is just "a prompt + when to run it", persisted as plain JSON the user controls (mirrors the res |
| `nanocodex/agent/schedule.py` | 38 | function | `_now` |  |
| `nanocodex/agent/schedule.py` | 42 | function | `_parse_iso` |  |
| `nanocodex/agent/schedule.py` | 53 | class | `ScheduledTask` | One saved task. ``next_run`` is the ISO time it should fire next. |
| `nanocodex/agent/schedule.py` | 74 | function | `to_dict` |  |
| `nanocodex/agent/schedule.py` | 78 | function | `compute_next_run` | Return the next ISO firing time strictly after *after*, or None. ``once`` has no next run after it fires (returns None). ``interval`` advances by whole periods until it is past *af |
| `nanocodex/agent/schedule.py` | 104 | function | `_initial_next_run` | First firing time when a task is created. |
| `nanocodex/agent/schedule.py` | 121 | class | `ScheduleStore` | Load/save scheduled tasks as plain JSON, and compute what's due. |
| `nanocodex/agent/schedule.py` | 124 | function | `__init__` |  |
| `nanocodex/agent/schedule.py` | 129 | function | `_load` |  |
| `nanocodex/agent/schedule.py` | 152 | function | `_save` |  |
| `nanocodex/agent/schedule.py` | 162 | function | `add` | Create and persist a task. Raises ValueError on invalid input. |
| `nanocodex/agent/schedule.py` | 188 | function | `remove` |  |
| `nanocodex/agent/schedule.py` | 196 | function | `set_enabled` |  |
| `nanocodex/agent/schedule.py` | 209 | function | `get` |  |
| `nanocodex/agent/schedule.py` | 215 | function | `due` | Enabled tasks whose next_run is at or before *now*. |
| `nanocodex/agent/schedule.py` | 227 | function | `mark_ran` | Record a firing and roll the task forward (or disable a spent 'once'). *ok* reports whether the run succeeded. A run of failures (timeouts / errors) with no success in between is c |
| `nanocodex/agent/schedule.py` | 262 | function | `seconds_until_next` | Seconds until the soonest enabled task fires, or None if none pending. |
| `nanocodex/agent/schedule_runner.py` | 1 | module | `schedule_runner` | Scheduler runtime: poll the ScheduleStore and run due tasks. Kept deliberately thin and INJECTABLE so it tests offline: * ``run_due_once`` fires every task due at a given ``now`` t |
| `nanocodex/agent/schedule_runner.py` | 32 | async function | `run_due_once` | Run every task due at *now*; return the ids that fired. Each task is marked ran (rolled forward / disabled) AFTER its turn, whether or not the turn raised — a task that errors shou |
| `nanocodex/agent/schedule_runner.py` | 65 | async function | `run_forever` | Sleep until the next task is due, run it, repeat until stopped. ``poll_interval`` caps the sleep so tasks added while we wait are picked up within that window. ``stop_check`` lets  |
| `nanocodex/agent/session.py` | 1 | module | `session` | Session: the message history for one conversation, persisted as JSONL. |
| `nanocodex/agent/session.py` | 11 | function | `_redact_for_log` | Replace base64 image data with a placeholder before logging. Keeps the JSONL log small and avoids persisting huge data URLs (which would also bloat any later resume). The in-memory |
| `nanocodex/agent/session.py` | 39 | class | `Session` | Holds the running message list and appends each turn to a JSONL log. |
| `nanocodex/agent/session.py` | 42 | function | `__init__` |  |
| `nanocodex/agent/session.py` | 53 | function | `resume` | Rebuild a session from a prior JSONL log, if one exists. The system prompt is taken fresh (sandbox/AGENTS.md may have changed) rather than from the log. The restored tail is saniti |
| `nanocodex/agent/session.py` | 72 | function | `fork` | Start a NEW conversation seeded from another's frozen messages. Used to "continue from" a past conversation without mutating it: the caller passes a snapshot's message list; we dro |
| `nanocodex/agent/session.py` | 97 | function | `_read_log` |  |
| `nanocodex/agent/session.py` | 119 | function | `_backfill_tool_results` | Insert synthetic results for any tool_call left unanswered. Mirrors the contract the backend enforces: an assistant message with tool_calls must be followed by a tool message per c |
| `nanocodex/agent/session.py` | 146 | function | `add_user` |  |
| `nanocodex/agent/session.py` | 149 | function | `add_assistant` |  |
| `nanocodex/agent/session.py` | 162 | function | `add_tool_result` |  |
| `nanocodex/agent/session.py` | 170 | function | `backfill_unanswered_tool_calls` | Append synthetic tool results for any tool_call left unanswered. The backend contract: an assistant message carrying ``tool_calls`` must be followed by a ``tool`` message for *ever |
| `nanocodex/agent/session.py` | 203 | function | `_append` |  |
| `nanocodex/agent/session.py` | 213 | function | `for_model` | The message list to send to the provider (a shallow copy). |
| `nanocodex/agent/session_index.py` | 1 | module | `session_index` | Session snapshots: a browsable, replayable history of past conversations. The GUI shows a left-hand list of past conversations; clicking one replays its FULL transcript. Two layers |
| `nanocodex/agent/session_index.py` | 41 | function | `new_session_id` | Mint a fresh conversation id (the GUI calls this when it builds a loop). |
| `nanocodex/agent/session_index.py` | 46 | function | `_now_iso` |  |
| `nanocodex/agent/session_index.py` | 50 | function | `_first_text` | Flatten a message ``content`` (str or block list) to plain text. |
| `nanocodex/agent/session_index.py` | 63 | function | `_clip` |  |
| `nanocodex/agent/session_index.py` | 71 | class | `SessionSummary` | A browsable digest of one conversation. Pure data — built by :func:`summarize`, persisted by :class:`SessionIndex`. ``session_id`` is the stable key (one entry per conversation; re |
| `nanocodex/agent/session_index.py` | 95 | function | `to_dict` |  |
| `nanocodex/agent/session_index.py` | 99 | function | `summarize` | Build a DETERMINISTIC summary of *messages* for one conversation. Zero-cost: title = first user message (clipped), snippet = last assistant message (clipped), plus counts and the m |
| `nanocodex/agent/session_index.py` | 158 | function | `_redact_messages` | Strip base64 image data from a message list before freezing a snapshot. Mirrors session.py's log redaction: a full transcript may carry huge ``data:`` image URLs that would bloat t |
| `nanocodex/agent/session_index.py` | 189 | class | `SessionIndex` | Load/save the global conversation directory as JSONL, keyed by session_id. Each conversation gets one row; :meth:`record` UPSERTs by ``session_id`` so a turn updates its own row, w |
| `nanocodex/agent/session_index.py` | 202 | function | `__init__` |  |
| `nanocodex/agent/session_index.py` | 208 | function | `_load` |  |
| `nanocodex/agent/session_index.py` | 249 | function | `_save` | Rewrite the whole index file from the folded map (dedup on disk too). |
| `nanocodex/agent/session_index.py` | 262 | function | `_sorted` | Newest activity first; blank timestamps sort last. |
| `nanocodex/agent/session_index.py` | 272 | function | `snapshot_path` |  |
| `nanocodex/agent/session_index.py` | 275 | function | `save_snapshot` | Freeze the full message list for one conversation. Best-effort. Returns True if the snapshot was written. Images are redacted to keep the file small; the snapshot is rewritten in f |
| `nanocodex/agent/session_index.py` | 295 | function | `load_snapshot` | Read back a frozen transcript, or None if there is no snapshot. |
| `nanocodex/agent/session_index.py` | 310 | function | `record` | UPSERT one conversation's summary (by session_id) and persist. |
| `nanocodex/agent/session_index.py` | 315 | function | `record_turn` | Freeze the transcript + upsert the row — the GUI's one call per turn. ``created_at`` is preserved from the conversation's first turn so the history keeps a stable "started at" even |
| `nanocodex/agent/session_index.py` | 341 | function | `entries` | All conversation summaries, newest activity first (directory list). |
| `nanocodex/agent/session_index.py` | 345 | function | `get` |  |
| `nanocodex/agent/skills_store.py` | 1 | module | `skills_store` | Skills: installable, reusable capability docs (SKILL.md), Codex/Claude-style. A "skill" is a folder under ``~/.nanocodex/skills/<name>/`` containing a ``SKILL.md`` file. The file s |
| `nanocodex/agent/skills_store.py` | 58 | class | `Skill` | One discovered skill: its name, model-visible description, and body. |
| `nanocodex/agent/skills_store.py` | 66 | function | `header_line` | The single line shown per skill in the system prompt. |
| `nanocodex/agent/skills_store.py` | 72 | function | `_split_frontmatter` | Parse a leading ``---`` frontmatter block into {key: value} + the body. Deliberately tiny (no YAML dep, matching nanocodex's no-heavy-deps rule): only flat ``key: value`` lines are |
| `nanocodex/agent/skills_store.py` | 97 | function | `parse_skill` | Parse SKILL.md text into a Skill (pure). Returns None if unusable. A skill is usable when it has a name (from frontmatter, else the folder name) and a non-empty description. The de |
| `nanocodex/agent/skills_store.py` | 114 | function | `is_valid_skill_name` | True if *name* is a safe single path segment (no separators/traversal). |
| `nanocodex/agent/skills_store.py` | 121 | class | `SkillsCollection` | All discovered skills + any parse warnings, for the prompt and the tool. |
| `nanocodex/agent/skills_store.py` | 128 | function | `is_empty` |  |
| `nanocodex/agent/skills_store.py` | 131 | function | `render_for_prompt` | The compact block injected into the system prompt (names + one-liners). |
| `nanocodex/agent/skills_store.py` | 138 | function | `_scan_one_dir` | Scan a single skills root, appending parsed skills/warnings into *collection*. A skill whose name was already added (e.g. by an earlier, higher-priority root) is skipped, so a user |
| `nanocodex/agent/skills_store.py` | 170 | function | `discover_skills` | Discover skills, parsing each ``<name>/SKILL.md`` (pure-ish I/O). With no argument, scans the user skills dir *and* the package's built-in skills, with user skills shadowing same-n |
| `nanocodex/agent/skills_store.py` | 192 | class | `SkillsStore` | Filesystem CRUD over the skills directory (install / list / remove / show). |
| `nanocodex/agent/skills_store.py` | 195 | function | `__init__` |  |
| `nanocodex/agent/skills_store.py` | 198 | function | `list` |  |
| `nanocodex/agent/skills_store.py` | 201 | function | `get` |  |
| `nanocodex/agent/skills_store.py` | 211 | function | `install` | Create ``<dir>/<name>/SKILL.md`` from name + description + body. Raises ValueError on an invalid name, an empty description, or an existing skill when overwrite is False. |
| `nanocodex/agent/skills_store.py` | 239 | function | `remove` | Delete a skill's folder. Returns False if it wasn't there. |
| `nanocodex/agent/skills_store.py` | 259 | function | `_render_skill_md` | Compose a SKILL.md with frontmatter + body (pure). |
| `nanocodex/agent/slash.py` | 1 | module | `slash` | Parse REPL slash commands into ``(command, argument)``. Pure string logic so the dispatcher is unit-tested without a console: this module only recognizes and splits a slash line; t |
| `nanocodex/agent/slash.py` | 32 | function | `parse_duration` | Parse a human duration into whole seconds, or None if it isn't one. Accepts ``30s`` / ``5m`` / ``1h`` and a bare number (seconds). Returns None for anything else (so the caller can |
| `nanocodex/agent/slash.py` | 47 | function | `split_loop_arg` | Split a /loop argument into ``(interval_seconds, prompt)``. A leading token that parses as a duration becomes the interval and the rest is the prompt (``5m run the tests``); otherw |
| `nanocodex/agent/slash.py` | 64 | function | `parse_slash` | Return ``(command, arg)`` for a slash line, or ``(None, "")`` if not one. A command is a line whose first non-space character is ``/``. The command token is lower-cased; the remain |
| `nanocodex/agent/state.py` | 1 | module | `state` | Explicit, serializable agent state for the Fable-style orchestrator. The single-agent loop keeps everything implicit in chat history. The orchestrator needs the opposite: a structu |
| `nanocodex/agent/state.py` | 48 | class | `TaskNode` | One unit of work in the task graph. |
| `nanocodex/agent/state.py` | 65 | function | `to_dict` |  |
| `nanocodex/agent/state.py` | 81 | function | `from_dict` |  |
| `nanocodex/agent/state.py` | 98 | class | `AgentCheckpoint` | A durable snapshot written at the end of a node's work. The checkpoint — not state.json — is the source of truth on recovery: it is written first and atomically, so if the process  |
| `nanocodex/agent/state.py` | 115 | function | `to_dict` |  |
| `nanocodex/agent/state.py` | 128 | function | `from_dict` |  |
| `nanocodex/agent/state.py` | 148 | class | `VerifyResult` | A verifier's verdict on one node's acceptance criteria. |
| `nanocodex/agent/state.py` | 162 | function | `passed` |  |
| `nanocodex/agent/state.py` | 165 | function | `to_dict` |  |
| `nanocodex/agent/state.py` | 176 | function | `from_dict` |  |
| `nanocodex/agent/state.py` | 192 | class | `Fact` | A confirmed repo fact, with provenance so conflicts can be reconciled. |
| `nanocodex/agent/state.py` | 201 | function | `to_dict` |  |
| `nanocodex/agent/state.py` | 205 | function | `from_dict` |  |
| `nanocodex/agent/state.py` | 214 | class | `AgentState` | The whole durable state of one orchestrated goal. |
| `nanocodex/agent/state.py` | 238 | function | `node` |  |
| `nanocodex/agent/state.py` | 244 | function | `completed_node_ids` |  |
| `nanocodex/agent/state.py` | 247 | function | `failed_node_ids` |  |
| `nanocodex/agent/state.py` | 250 | function | `latest_checkpoint_for` |  |
| `nanocodex/agent/state.py` | 254 | function | `latest_verify_for` |  |
| `nanocodex/agent/state.py` | 258 | function | `next_seq` |  |
| `nanocodex/agent/state.py` | 263 | function | `to_dict` |  |
| `nanocodex/agent/state.py` | 281 | function | `from_dict` |  |
| `nanocodex/agent/store.py` | 1 | module | `store` | Durable, crash-tolerant persistence for AgentState. The review's high-severity persistence finding: state.json and the checkpoint files are written by separate steps, so a crash be |
| `nanocodex/agent/store.py` | 31 | class | `AgentStateStore` | Reads/writes :class:`AgentState` under a workspace's ``.nanocodex`` dir. |
| `nanocodex/agent/store.py` | 34 | function | `__init__` |  |
| `nanocodex/agent/store.py` | 39 | function | `_ensure_dirs` |  |
| `nanocodex/agent/store.py` | 43 | function | `_atomic_write_json` | Write JSON via temp file + atomic rename so readers never see a tear. The temp file sits in the same directory as the target so ``os.replace`` is a same-filesystem rename (atomic). |
| `nanocodex/agent/store.py` | 60 | function | `write_checkpoint` | Persist one checkpoint atomically. Call BEFORE saving state. |
| `nanocodex/agent/store.py` | 67 | function | `load_checkpoints` | All checkpoints on disk, oldest first (by created_at then id). |
| `nanocodex/agent/store.py` | 85 | function | `save` | Persist state.json atomically (checkpoints must already be on disk). |
| `nanocodex/agent/store.py` | 90 | function | `exists` |  |
| `nanocodex/agent/store.py` | 93 | function | `load` | Load state.json, reconciled against the checkpoint directory. Reconciliation handles the crash window between "checkpoint written" and "state.json written": any checkpoint whose no |
| `nanocodex/agent/store.py` | 120 | function | `new_checkpoint` | Mint a checkpoint with a deterministic id and an ISO timestamp. Persistence order is the caller's responsibility: write the checkpoint, append it to ``state.checkpoints``, then ``s |
| `nanocodex/agent/task_graph.py` | 1 | module | `task_graph` | Task-graph validation and scheduling — pure code, never the model. The review's #1 and #2 high-severity findings: a model-produced task graph WILL sometimes contain cycles, danglin |
| `nanocodex/agent/task_graph.py` | 16 | class | `GraphError` | Raised when a task graph is structurally invalid (cycle, dangling, dup). |
| `nanocodex/agent/task_graph.py` | 20 | function | `validate_graph` | Raise :class:`GraphError` if the graph is not a legal DAG. Checks, in order: duplicate ids, unknown kinds, dangling dependencies (depends_on an id that doesn't exist), self-depende |
| `nanocodex/agent/task_graph.py` | 52 | function | `_find_cycle` | Return one cycle as an id path, or [] if the graph is acyclic (DFS colors). |
| `nanocodex/agent/task_graph.py` | 59 | function | `dfs` |  |
| `nanocodex/agent/task_graph.py` | 83 | function | `topo_order` | Kahn topological order of node ids. Assumes the graph already validated. Order is dependency-first: a node appears after every node it depends on. Raises :class:`GraphError` if a c |
| `nanocodex/agent/task_graph.py` | 117 | function | `ready_nodes` | Nodes whose every dependency is `done` and that are themselves runnable. A node is runnable when its status is pending/ready and all deps are done. Returned in plan order so the sc |
| `nanocodex/agent/task_graph.py` | 133 | function | `propagate_skips` | Mark as `skipped` any non-terminal node with a failed/skipped ancestor. The review's missing-state finding: when a node fails, its descendants should become `skipped` (cannot run,  |
| `nanocodex/agent/trace.py` | 1 | module | `trace` | Lightweight, dimension-filtered tracing gated on the NCX_TRACE env var. Mirrors the Rust port's convention (NCX_TRACE non-empty = on) but adds *dimensions* so a live run can opt in |
| `nanocodex/agent/trace.py` | 22 | function | `trace_enabled` | True if NCX_TRACE is on globally or names *dimension* in its allow-list. |
| `nanocodex/agent/trace.py` | 32 | function | `trace` | Emit a ``[trace:<dimension>] message`` line to stderr when enabled. |
| `nanocodex/agent/verifier.py` | 1 | module | `verifier` | Independent verifier: an isolated review pass over a node's acceptance. The review's highest-severity finding was that "independent verifier" usually degrades into a rubber stamp b |
| `nanocodex/agent/verifier.py` | 69 | class | `Verifier` | Runs an isolated, read-only verification pass and returns a verdict. |
| `nanocodex/agent/verifier.py` | 72 | function | `__init__` |  |
| `nanocodex/agent/verifier.py` | 95 | function | `_build_brief` | The single user message: criteria + where to look. No worker history. |
| `nanocodex/agent/verifier.py` | 139 | async function | `verify` |  |
| `nanocodex/agent/verifier.py` | 200 | function | `parse_verdict` | Parse a verifier's final text into a VerifyResult, failing conservatively. Tolerates code fences and leading/trailing prose. Any failure to find a well-formed verdict object yields |
| `nanocodex/agent/verifier.py` | 220 | function | `_extract_json_object` |  |
| `nanocodex/cli.py` | 1 | module | `cli` | nanocodex CLI: an interactive Codex-style coding REPL on a DeepSeek backend. Usage: nanocodex # REPL in the current directory nanocodex --sandbox read-only # override sandbox mode  |
| `nanocodex/cli.py` | 51 | class | `_SubcommandFirstGroup` | Route a leading subcommand name to the subcommand, not the ``task`` arg. The root callback has an optional positional ``task`` (so ``nanocodex "fix the bug"`` works as a one-shot). |
| `nanocodex/cli.py` | 63 | function | `parse_args` |  |
| `nanocodex/cli.py` | 89 | function | `_build_console_approver` | An Approver whose callback prompts the console with y/n. |
| `nanocodex/cli.py` | 92 | async function | `_ask` |  |
| `nanocodex/cli.py` | 118 | function | `_make_hooks` |  |
| `nanocodex/cli.py` | 123 | function | `_emit` |  |
| `nanocodex/cli.py` | 132 | async function | `on_reasoning_delta` |  |
| `nanocodex/cli.py` | 140 | async function | `on_content_delta` |  |
| `nanocodex/cli.py` | 147 | async function | `on_stream_end` |  |
| `nanocodex/cli.py` | 152 | async function | `on_assistant_text` |  |
| `nanocodex/cli.py` | 157 | async function | `on_tool_start` |  |
| `nanocodex/cli.py` | 161 | async function | `on_tool_result` |  |
| `nanocodex/cli.py` | 176 | function | `_summarize_call` |  |
| `nanocodex/cli.py` | 196 | function | `_build_loop` | Build an AgentLoop. *log_path* controls where the session transcript is persisted: - ``_UNSET`` (default): the usual ``workspace/.nanocodex/session.jsonl``. - ``None``: do NOT pers |
| `nanocodex/cli.py` | 279 | function | `_print_banner` |  |
| `nanocodex/cli.py` | 291 | function | `_build_orchestrator` | Build an OrchestratorLoop sharing _build_loop's config/provider/ctx setup. Returns (orchestrator, cfg). The orchestrator builds its own role-scoped registries per worker, so we han |
| `nanocodex/cli.py` | 340 | function | `_print_orchestrator_result` | Render the final task graph + verdicts after an orchestrated run. |
| `nanocodex/cli.py` | 366 | function | `orchestrate_cmd` | Run the multi-agent orchestrator: plan -> execute -> verify -> replan. |
| `nanocodex/cli.py` | 411 | function | `main` |  |
| `nanocodex/cli.py` | 497 | async function | `_orchestrate` | Connect MCP (if requested), run the task/REPL, then tear MCP down. |
| `nanocodex/cli.py` | 521 | async function | `_connect_mcp` | Discover + connect MCP servers, registering their tools onto the loop. |
| `nanocodex/cli.py` | 542 | async function | `_run_once` |  |
| `nanocodex/cli.py` | 553 | async function | `_repl` |  |
| `nanocodex/cli.py` | 588 | async function | `_dispatch_slash` | Handle a REPL slash command. Returns True only for /exit (quit the REPL). Read-only commands (/help /status /diff /plan) just print; the mutating ones (/model /approvals /compact / |
| `nanocodex/cli.py` | 687 | async function | `_run_loop_command` | `/loop [interval] <prompt>`: re-run a prompt on an interval until Ctrl+C. Ad-hoc, in-session, no persistence — complements the (cron-like, unattended) scheduler. The interval accep |
| `nanocodex/cli.py` | 726 | function | `_print_plan` |  |
| `nanocodex/cli.py` | 738 | function | `_schedule_store` |  |
| `nanocodex/cli.py` | 744 | function | `schedule_add` | Add a scheduled task. |
| `nanocodex/cli.py` | 786 | function | `schedule_list` | List all scheduled tasks. |
| `nanocodex/cli.py` | 802 | function | `schedule_remove` | Remove a scheduled task. |
| `nanocodex/cli.py` | 809 | function | `schedule_enable` | Enable a scheduled task. |
| `nanocodex/cli.py` | 816 | function | `schedule_disable` | Disable a scheduled task. |
| `nanocodex/cli.py` | 823 | function | `schedule_run` | Run the scheduler: wait for tasks to come due and execute them. This is a long-running foreground process (Ctrl+C to stop). Each due task runs as one agent turn in the given worksp |
| `nanocodex/cli.py` | 849 | async function | `run_task` |  |
| `nanocodex/cli.py` | 868 | function | `_on_event` |  |
| `nanocodex/cli.py` | 877 | function | `datetime_now` |  |
| `nanocodex/cli.py` | 882 | function | `_auto_deny_approver` | Approver for unattended runs: never grants escalation (no human present). |
| `nanocodex/cli.py` | 884 | async function | `_deny` |  |
| `nanocodex/cli.py` | 889 | function | `_desktop_only_approver` | Unattended approver that grants ONLY desktop (MCP) actions, nothing else. Security model for allow_desktop tasks. The trick is the policy choice: * Under ``never`` the MCP gate aut |
| `nanocodex/cli.py` | 906 | async function | `_allow_mcp_only` |  |
| `nanocodex/cli.py` | 911 | function | `_auto_approve_approver` | Approver that grants every escalation without asking. Used for A/B comparison runs: each side runs inside its OWN throwaway git worktree, so file writes are already isolated from t |
| `nanocodex/cli.py` | 922 | async function | `_allow_all` |  |
| `nanocodex/cli.py` | 937 | function | `storyboard_run` | Plan a storyboard from a story file + image directory, exporting JSON (and optionally video). |
| `nanocodex/cli.py` | 1053 | function | `_progress` |  |
| `nanocodex/config.py` | 1 | module | `config` | Configuration loading for nanocodex. Resolution order (highest priority wins): explicit overrides (CLI) > environment > ~/.nanocodex/config.toml > ~/.deepseek/config.toml > ~/.code |
| `nanocodex/config.py` | 41 | class | `ConfigError` | Raised when configuration is missing or invalid. |
| `nanocodex/config.py` | 46 | class | `Config` | Resolved runtime configuration. |
| `nanocodex/config.py` | 97 | function | `validate` |  |
| `nanocodex/config.py` | 114 | function | `redacted` | Config snapshot safe to display: the API key is masked. |
| `nanocodex/config.py` | 148 | function | `_load_toml` |  |
| `nanocodex/config.py` | 158 | function | `_deepseek_values` | Extract the fields we care about from a ~/.deepseek/config.toml dump. |
| `nanocodex/config.py` | 186 | function | `_nanocodex_values` | Extract settings from nanocodex's own ~/.nanocodex/config.toml. This is a flat file nanocodex fully owns (written by the GUI Settings dialog), so the keys match Config field names  |
| `nanocodex/config.py` | 206 | function | `_codex_values` | Extract Codex-style settings from ~/.codex/config.toml. |
| `nanocodex/config.py` | 233 | function | `_profile_values` | Pull the known profile-able keys out of one [profiles.<name>] table. |
| `nanocodex/config.py` | 238 | function | `list_profiles` | Names of [profiles.<name>] tables defined in ~/.nanocodex/config.toml. |
| `nanocodex/config.py` | 244 | function | `load_config` | Resolve a :class:`Config` from files, environment, and explicit overrides. *overrides* (typically from CLI flags) win over everything else. ``None`` values inside *overrides* are i |
| `nanocodex/config.py` | 344 | function | `_model_list` | Build the model-switcher list from config/env, with the active model first. Accepts a comma-separated string (from env) or a list. When nothing is configured, falls back to a built |
| `nanocodex/config.py` | 370 | function | `_as_int` | Coerce a config value (possibly a string from env) to int. |
| `nanocodex/config.py` | 396 | function | `_esc_toml` | Escape a string for a TOML basic (double-quoted) string. |
| `nanocodex/config.py` | 408 | function | `dump_nanocodex_toml` | Serialize known settings into ~/.nanocodex/config.toml text (pure). Only keys in :data:`_WRITABLE_KEYS` are emitted, in a fixed order, so the output round-trips through :func:`toml |
| `nanocodex/config.py` | 429 | function | `write_nanocodex_config` | Merge *updates* into ~/.nanocodex/config.toml and write it back. Existing values in the file are preserved (this is a merge, not a replace), so setting just the API key won't wipe  |
| `nanocodex/gui.py` | 1 | module | `gui` | Tkinter desktop GUI for nanocodex (Windows-friendly entry point). Threading model (the crux) -------------------------- Tkinter must run on the main thread and is synchronous; the  |
| `nanocodex/gui.py` | 39 | function | `_load_state` | Read the whole gui_state.json dict (best-effort; {} on any problem). |
| `nanocodex/gui.py` | 49 | function | `_save_state` | MERGE *updates* into gui_state.json (best-effort; never raises). Critical: read-modify-write so persisting one key (e.g. the scheduler toggle) never clobbers another (e.g. last_wor |
| `nanocodex/gui.py` | 66 | function | `_load_last_workspace` | Return the last-opened project dir, if it was saved and still exists. |
| `nanocodex/gui.py` | 75 | function | `_save_last_workspace` | Persist the active project dir (best-effort; never raises). |
| `nanocodex/gui.py` | 80 | function | `_load_scheduler_enabled` | Whether the managed scheduler should auto-start (default True). The user chose "GUI launches it automatically", so absence of the key means ON. Only an explicit stored ``false`` ke |
| `nanocodex/gui.py` | 90 | function | `_save_scheduler_enabled` | Persist the scheduler toggle (best-effort; never raises). |
| `nanocodex/gui.py` | 142 | function | `_palette_for` | Return the palette dict for *theme*, falling back to light if unknown. |
| `nanocodex/gui.py` | 147 | function | `_load_theme` | The persisted UI theme (default 'light'); only a known value is honored. |
| `nanocodex/gui.py` | 153 | function | `_save_theme` | Persist the UI theme choice (best-effort; never raises). |
| `nanocodex/gui.py` | 159 | class | `_UiEvent` | A message from the worker thread to the Tk main thread. |
| `nanocodex/gui.py` | 164 | function | `__init__` |  |
| `nanocodex/gui.py` | 169 | class | `_ApprovalRequestUI` | Carries an approval prompt across threads with a blocking handshake. |
| `nanocodex/gui.py` | 174 | function | `__init__` |  |
| `nanocodex/gui.py` | 184 | class | `NanocodexGUI` |  |
| `nanocodex/gui.py` | 185 | function | `__init__` |  |
| `nanocodex/gui.py` | 297 | function | `_build_widgets` |  |
| `nanocodex/gui.py` | 314 | function | `flat_btn` |  |
| `nanocodex/gui.py` | 326 | function | `add_tooltip` | Lightweight hover tooltip so each action button is self-explaining. A borderless Toplevel shown on <Enter> near the cursor and destroyed on <Leave>; best-effort (any Tk error is sw |
| `nanocodex/gui.py` | 335 | function | `show` |  |
| `nanocodex/gui.py` | 353 | function | `hide` |  |
| `nanocodex/gui.py` | 628 | function | `_on_toggle_auto` | Mirror the Tk checkbox into the plain bool the worker thread reads. Auto-approve ON -> full auto (no prompts for in-sandbox writes). Auto-approve OFF -> confirm each write step (th |
| `nanocodex/gui.py` | 647 | function | `_sync_step_approval` | Push the toggle state onto the live ToolContext (worker reads it). |
| `nanocodex/gui.py` | 657 | function | `_on_toggle_file_panel` | Top-bar 'Files' switch: show/hide the right-side diff dock. |
| `nanocodex/gui.py` | 664 | function | `_show_file_panel` | Pack the dock (and its border) on the right; render any cached edit. |
| `nanocodex/gui.py` | 675 | function | `_hide_file_panel` | Collapse the dock; the transcript reclaims the freed width. |
| `nanocodex/gui.py` | 683 | function | `_on_file_edit` | Worker reported an apply_patch; cache it, render only if panel is open. Manual mode: a hidden panel just stores the latest edit so flipping the switch on shows it — an edit never f |
| `nanocodex/gui.py` | 693 | function | `_render_file_edit` | Draw the classified diff rows into the read-only file view. |
| `nanocodex/gui.py` | 740 | function | `_update_context_usage` | Status bar, Claude-Code style: state \| model \| used / window (%). |
| `nanocodex/gui.py` | 774 | function | `_show_context_details` | Popup breaking down context usage, styled after Claude Code: a header (used / window, %), a colored progress bar, then categories sorted by size with a color swatch, token count, a |
| `nanocodex/gui.py` | 892 | function | `_init_loop` |  |
| `nanocodex/gui.py` | 895 | function | `gui_approver_factory` |  |
| `nanocodex/gui.py` | 959 | function | `_autoconnect_mcp` | Start a long-lived MCP thread (once) and connect servers on it. GUI users never run `--mcp`; this gives them MCP tools automatically from ~/.nanocodex/mcp.toml. The connection live |
| `nanocodex/gui.py` | 985 | function | `_mcp_thread_main` | Owns a persistent event loop the MCP connection stays bound to. |
| `nanocodex/gui.py` | 1015 | function | `_register_bridged_tool` | Register an MCP tool whose execute() is dispatched to the MCP loop. The agent runs each turn on its own loop; an MCP tool's coroutine is bound to the MCP loop, so we hop threads vi |
| `nanocodex/gui.py` | 1026 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1035 | function | `_autostart_scheduler` | Start the in-GUI schedule runner once, on a dedicated thread. GUI users never run `nanocodex schedule run`; this hosts it for them so a due task fires with no manual step. Honors t |
| `nanocodex/gui.py` | 1064 | function | `_scheduler_thread_main` | Own an event loop and poll the ScheduleStore until told to stop. |
| `nanocodex/gui.py` | 1089 | async function | `_scheduler_run_task` | Run ONE due task unattended (on the scheduler loop). Concurrency: the GUI conversation and a scheduled task drive the SAME mouse/keyboard, so they must never overlap. The desktop l |
| `nanocodex/gui.py` | 1111 | async function | `_run` |  |
| `nanocodex/gui.py` | 1173 | async function | `_attach_scheduler_mcp_tools` | Give the scheduled task's loop the MCP desktop tools, bridged. Rebuilds the tools against the TASK loop's ctx (desktop-only approver), not the GUI's, so the approval gate is the un |
| `nanocodex/gui.py` | 1194 | function | `_bridge` |  |
| `nanocodex/gui.py` | 1195 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1203 | function | `_reattach_mcp_tools` | Re-register the live MCP tools onto the current (rebuilt) loop. ``_autoconnect_mcp`` connects once per session on a long-lived MCP thread; it returns early on every later ``_init_l |
| `nanocodex/gui.py` | 1239 | function | `_bridge` |  |
| `nanocodex/gui.py` | 1240 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1253 | function | `_scheduler_log` | Append a line to ~/.nanocodex/scheduler.log (best-effort, UTF-8). Unattended runs deliberately do NOT touch the transcript; this file is their only record. |
| `nanocodex/gui.py` | 1266 | function | `_on_toggle_scheduler` | Flip the managed scheduler on/off and persist the choice. |
| `nanocodex/gui.py` | 1287 | function | `_on_open_project` | Pick a folder, rebuild the loop in it, and reset the transcript. |
| `nanocodex/gui.py` | 1305 | function | `_on_new_session` | Start a clean conversation in the current workspace. |
| `nanocodex/gui.py` | 1312 | function | `_start_new_session` | Mint a fresh session_id, clear the transcript, and rebuild the loop. |
| `nanocodex/gui.py` | 1335 | function | `_on_pick_model` | Open a menu of available models; switching rebuilds the loop. |
| `nanocodex/gui.py` | 1359 | function | `_switch_model` |  |
| `nanocodex/gui.py` | 1369 | function | `_refresh_plugin_list` | Redraw the server rows in the plugin manager from mcp.toml. |
| `nanocodex/gui.py` | 1405 | function | `_remove` |  |
| `nanocodex/gui.py` | 1410 | function | `_toggle` |  |
| `nanocodex/gui.py` | 1429 | function | `_open_settings` | Codex-style settings window: a left nav list switches right sections. Folds the old standalone Settings dialog AND the MCP plugin manager into one window with four sections (Genera |
| `nanocodex/gui.py` | 1487 | function | `_settings_show_section` | Repaint the content frame with *name*'s section; re-highlight nav. |
| `nanocodex/gui.py` | 1516 | function | `_settings_section_header` | Shared title + subtitle for a settings section (tuple pads on pack). |
| `nanocodex/gui.py` | 1528 | function | `_settings_section_general` | Read-only workspace + model overview (model is changed in Config). |
| `nanocodex/gui.py` | 1581 | function | `_on_pick_theme` | Switch the UI theme: persist it, rebuild every widget, restore the transcript text, and reopen Settings on this section. Rebuilding is how the ~300 ``P[...]`` lookups re-color at o |
| `nanocodex/gui.py` | 1640 | function | `_settings_section_config` | Editable config: API key / base URL / model / sandbox / approval / reasoning, persisted to ~/.nanocodex/config.toml and applied via rebuild. |
| `nanocodex/gui.py` | 1670 | function | `_label` |  |
| `nanocodex/gui.py` | 1675 | function | `_entry` |  |
| `nanocodex/gui.py` | 1685 | function | `_option` |  |
| `nanocodex/gui.py` | 1751 | function | `_do_save` |  |
| `nanocodex/gui.py` | 1800 | function | `_settings_section_mcp` | MCP server CRUD, folded in from the old plugin manager. Reuses _refresh_plugin_list (it renders into self._plugin_list_frame). Edits persist immediately but only connect on the NEX |
| `nanocodex/gui.py` | 1831 | function | `_row` |  |
| `nanocodex/gui.py` | 1849 | function | `_do_add` |  |
| `nanocodex/gui.py` | 1880 | function | `_settings_section_marketplace` | Browse + one-click install MCP servers from a built-in catalog and (optionally) a remote URL. Both sources install through the SAME McpStore the "MCP servers" section uses, so an i |
| `nanocodex/gui.py` | 1939 | function | `_installed_server_names` | Names already in mcp.toml (so the marketplace can mark them installed). |
| `nanocodex/gui.py` | 1947 | function | `_render_catalog_row` | Draw one catalog entry row with name/source/description + Install. |
| `nanocodex/gui.py` | 1967 | function | `_install` |  |
| `nanocodex/gui.py` | 1976 | function | `_refresh_marketplace_local` | Redraw the built-in catalog rows. |
| `nanocodex/gui.py` | 1988 | function | `_refresh_marketplace_remote` | Redraw the remote catalog rows from a fetched entry list. |
| `nanocodex/gui.py` | 2006 | function | `_on_marketplace_refresh` | Fetch the remote catalog in a background thread (never blocks the UI). |
| `nanocodex/gui.py` | 2024 | function | `_run_marketplace_fetch` | Worker: fetch+parse remote catalog, hand the result back to the main thread via root.after (Tk-safe). Errors are reported, never crash. |
| `nanocodex/gui.py` | 2035 | function | `_marketplace_fetch_done` | Main-thread callback after a remote fetch finishes. |
| `nanocodex/gui.py` | 2055 | function | `_install_marketplace_entry` | Install a catalog entry. If it needs a path or env values, prompt for them in a small modal first; otherwise install immediately. |
| `nanocodex/gui.py` | 2063 | function | `_prompt_marketplace_install` | Modal collecting the machine-specific path and/or env values an entry needs before install. Uses Entry widgets (env values masked). |
| `nanocodex/gui.py` | 2107 | function | `_submit` |  |
| `nanocodex/gui.py` | 2130 | function | `_do_marketplace_install` | Funnel an install through marketplace.install_entry → McpStore. Returns (ok, message). On success refreshes both the marketplace rows and the MCP servers list (if that section's fr |
| `nanocodex/gui.py` | 2155 | function | `_settings_section_schedule` | Manual CRUD over scheduled tasks (the SAME ScheduleStore the model's manage_schedule tool and the CLI use). Lets the user add/enable/disable/ remove tasks visually instead of only  |
| `nanocodex/gui.py` | 2190 | function | `_label` |  |
| `nanocodex/gui.py` | 2195 | function | `_entry` |  |
| `nanocodex/gui.py` | 2240 | function | `_do_add` |  |
| `nanocodex/gui.py` | 2277 | function | `_refresh_schedule_mgr` | Redraw the task rows in the Scheduled-tasks settings section. Each row shows the task's prompt + recurrence summary with Enable/Disable and Remove buttons, mutating the shared Sche |
| `nanocodex/gui.py` | 2325 | function | `_remove` |  |
| `nanocodex/gui.py` | 2331 | function | `_toggle` |  |
| `nanocodex/gui.py` | 2349 | function | `_settings_section_desktop` | Read-only mirror of desktop-control state (toggles live in the top bar). nanocodex's desktop control runs through MCP (windows-computer-use-mcp) under approval gating. The live swi |
| `nanocodex/gui.py` | 2366 | function | `_state_row` |  |
| `nanocodex/gui.py` | 2391 | async function | `_approve_via_ui` | Approver callback (runs on the worker loop). Blocks on the UI. Short-circuits without a dialog when the global auto-approve toggle is on, when the user previously chose "allow all  |
| `nanocodex/gui.py` | 2418 | function | `_show_approval_dialog` |  |
| `nanocodex/gui.py` | 2438 | function | `_decide` |  |
| `nanocodex/gui.py` | 2445 | function | `dlg_btn` |  |
| `nanocodex/gui.py` | 2508 | function | `_on_continue` | Resume an unfinished turn (hit step-limit / paused mid-plan) with one click, instead of making the user type 'continue'. |
| `nanocodex/gui.py` | 2522 | function | `_on_send` |  |
| `nanocodex/gui.py` | 2565 | function | `_autogrow_entry` | Grow/shrink the composer to fit its content, within [min, max] rows. Counts the displayed lines and clamps the Text height to that range; past the max the box scrolls internally. B |
| `nanocodex/gui.py` | 2588 | function | `_quick_capture_memory` | Append `note` to user memory (the `# ...` composer shortcut). Best-effort and synchronous: writing one bullet to a local file is instant, so no worker thread. Never raises into the |
| `nanocodex/gui.py` | 2607 | function | `_on_attach` | 📎 button: pick local files to attach to the NEXT message. Images become OpenAI multimodal blocks (only seen by a vision-capable model); other (text-like) files are read and inlined |
| `nanocodex/gui.py` | 2634 | function | `_refresh_attach_label` | Show the pending attachment count on the 📎 button (cosmetic). |
| `nanocodex/gui.py` | 2645 | function | `_consume_attachments` | Fold pending attachments into the message content, then clear them. Returns a plain string when there are no images (text-only / no files), or an OpenAI multimodal block list when  |
| `nanocodex/gui.py` | 2687 | function | `_on_enhance` | ✨ button: rewrite the composer text into a clearer prompt. Reads the current input, kicks off a background rewrite (the model call must not block the UI), and on completion shows a |
| `nanocodex/gui.py` | 2716 | function | `_run_enhance_thread` | Daemon thread: one provider.chat to rewrite *text*; result to the queue. Mirrors _run_turn_thread's "own asyncio loop, post results via the UI queue" shape, but it's a single state |
| `nanocodex/gui.py` | 2737 | function | `_refresh_enhance_label` | Reflect the in-flight state on the ✨ button (cosmetic, never crashes). |
| `nanocodex/gui.py` | 2747 | function | `_show_enhance_dialog` | Preview the rewrite; let the user use it, keep the original, or cancel. A rewrite NEVER silently replaces the user's words — they pick here. 'Use rewrite' replaces the composer tex |
| `nanocodex/gui.py` | 2777 | function | `_use` |  |
| `nanocodex/gui.py` | 2784 | function | `dlg_btn` |  |
| `nanocodex/gui.py` | 2826 | function | `_flat_btn` | Class-level twin of the _build_widgets-local flat_btn closure, so dialogs built outside _build_widgets (storyboard panel) get the same flat, palette-colored button without re-defin |
| `nanocodex/gui.py` | 2842 | function | `_handle_storyboard_command` | `/storyboard` composer command. `render` -> render the previewed state; anything else -> open the panel (prefilling+auto-previewing the story text when given). |
| `nanocodex/gui.py` | 2852 | function | `_open_storyboard_panel` | Open (or focus) the dedicated storyboard panel. Single-instance, like Settings: a story box + image picker + aspect ratio on top, a read-only two-level (chapters / shots) preview i |
| `nanocodex/gui.py` | 3062 | function | `_on_close` |  |
| `nanocodex/gui.py` | 3073 | function | `_sb_memory_path` | Where the panel persists its inputs (story + images + ratio). |
| `nanocodex/gui.py` | 3078 | function | `_sb_save_memory` | Persist the panel's story + picked images + ratio (best-effort). Lets reopening the panel (or relaunching the app) restore what you had instead of starting blank. Any failure is sw |
| `nanocodex/gui.py` | 3114 | function | `_sb_load_memory` | Load the panel's last-saved inputs; {} when none/unreadable. |
| `nanocodex/gui.py` | 3127 | function | `_sb_render_thumbs` | Show small thumbnails of the picked reference images under the picker. Uses Pillow to decode any format (PNG/JPEG/webp/…) and downscale to a ~64px-tall thumbnail. PhotoImage refs a |
| `nanocodex/gui.py` | 3175 | function | `_sb_pick_images` | Pick reference images for the storyboard (optional). |
| `nanocodex/gui.py` | 3191 | function | `_sb_set_status` |  |
| `nanocodex/gui.py` | 3203 | function | `_sb_build_obj` | Build the schema project dict from the panel's story + images. |
| `nanocodex/gui.py` | 3224 | function | `_sb_build_deps` | Wire pipeline deps from layered config (planner/chapters always; vision only with a VL backend + images; seedance only when rendering). Raises with a clear message when a needed ke |
| `nanocodex/gui.py` | 3261 | function | `_sb_run_preview` | [生成预览]: plan chapters + shots on a worker thread (never renders). |
| `nanocodex/gui.py` | 3279 | function | `_sb_preview_thread` | Daemon thread: run_planning over its own asyncio loop; result to queue. |
| `nanocodex/gui.py` | 3289 | function | `_sb_run_render` | [出片]: render the previewed state after an explicit confirm (COSTS $$). |
| `nanocodex/gui.py` | 3322 | function | `_sb_render_thread` | Daemon thread: render the planned state via Seedance, download the finished clips locally, then export. Results to the UI queue. Each render gets its OWN archived directory under s |
| `nanocodex/gui.py` | 3340 | function | `_prog` |  |
| `nanocodex/gui.py` | 3354 | function | `_sb_run_meta` | Build the index row for one 出片 run (id/title/time/counts/cost/dir). |
| `nanocodex/gui.py` | 3373 | function | `_sb_download_clips` | Download each successful shot's signed URL to ``out_dir/<shot_id>.mp4``. Signed Seedance URLs expire (~24h), so a local copy makes 播放 reliable. Best-effort: a download failure leav |
| `nanocodex/gui.py` | 3395 | function | `_sb_rerender_one` | Re-render a single (usually failed) shot after a per-shot confirm. |
| `nanocodex/gui.py` | 3421 | function | `_sb_rerender_thread` | Daemon thread: re-render ONE shot in place, download, export, refresh. A 重试 belongs to the SAME run as the original render (it's补 the failed shot, not a new history entry): reuse s |
| `nanocodex/gui.py` | 3440 | function | `_prog` |  |
| `nanocodex/gui.py` | 3451 | function | `_sb_show_preview` | Render the chapters + shots into the read-only preview, set cost. |
| `nanocodex/gui.py` | 3535 | function | `_sb_show_render_done` | Show per-shot results + actual cost after a render; fold cost in. Keeps the rendered state so a failed shot can be re-generated on its own, then (re)builds the per-shot result list |
| `nanocodex/gui.py` | 3574 | function | `_sb_render_results` | (Re)build the per-shot result rows: status + ▶播放 / ↻重试. A succeeded shot shows ✓ + its title + ▶播放 (opens the local mp4, or the signed URL as fallback). A failed shot shows ✗ + the |
| `nanocodex/gui.py` | 3654 | function | `_sb_play_clip` | Open a rendered clip: prefer the local mp4, fall back to the URL. Uses the OS default handler (os.startfile on Windows) so the system video player opens it. A signed URL is the fal |
| `nanocodex/gui.py` | 3682 | function | `_sb_run_merge` | Pre-check continuity, THEN (after the user OKs) stitch into full.mp4. Merging hard-cuts the clips in shot order, so a storyboard with missing transitions plays as a jumpy story. Be |
| `nanocodex/gui.py` | 3708 | function | `_sb_check_thread` | Daemon thread: run the continuity check over its own asyncio loop. Mirrors _sb_preview_thread. ``available`` is the same filter the rest of the panel uses (a shot has a clip iff it |
| `nanocodex/gui.py` | 3729 | function | `_sb_open_merge_progress` | Show a small modal progress dialog (reused for both check & merge). The bar runs in indeterminate (marquee) mode — neither the DeepSeek check nor ffmpeg's concat give a clean perce |
| `nanocodex/gui.py` | 3774 | function | `_sb_close_merge_progress` | Tear down the merge progress dialog (idempotent: done OR error). |
| `nanocodex/gui.py` | 3795 | function | `_sb_show_continuity_report` | Show the pre-merge continuity report; let the user 补镜 / 合并 / 取消. Built like _sb_open_merge_progress (palette Toplevel) with a scrollable list of gaps. Each gap carries a 「补这镜」butto |
| `nanocodex/gui.py` | 3859 | function | `_gap_key` |  |
| `nanocodex/gui.py` | 3884 | function | `_line` |  |
| `nanocodex/gui.py` | 3914 | function | `_cancel` |  |
| `nanocodex/gui.py` | 3924 | function | `_proceed` |  |
| `nanocodex/gui.py` | 3952 | function | `_sb_fill_one` | Adopt one gap suggestion as a real shot and render it (real spend). Confirms cost, splices the suggestion into ``state`` via insert_fill_shot (so it lands between the two shots it  |
| `nanocodex/gui.py` | 3999 | function | `_sb_fill_thread` | Daemon thread: render ONE fill shot, download, export, refresh index. The fill shot belongs to the SAME run as the originals (it補 a gap, not a new history entry): reuse run_dir and |
| `nanocodex/gui.py` | 4013 | function | `_prog` |  |
| `nanocodex/gui.py` | 4026 | function | `_sb_proceed_merge` | Run the actual ffmpeg concat after the continuity report was OK'd. This is the back-half of the old _sb_run_merge (split out so the check gates it). _sb_busy is already set (held s |
| `nanocodex/gui.py` | 4039 | function | `_sb_merge_thread` | Daemon thread: run concat_clips, report the merged path (or error). |
| `nanocodex/gui.py` | 4049 | function | `_sb_show_merge_done` | Report the merged full video; surface it as a persistent 整片 row. Closes the progress dialog, re-enables 合并, and re-renders the result rows so the 整片 row appears (it's rendered when |
| `nanocodex/gui.py` | 4075 | function | `_sb_show_history` | Popup listing past 出片 runs from storyboard_out/runs/index.json. Each row: time · title · ok/总镜 · ¥cost, with a 载入 button that reopens that run in the panel (replay clips / retry fa |
| `nanocodex/gui.py` | 4139 | function | `_sb_load_run` | Reload a past run's exported state into the panel for replay/retry/merge. Rebuilds the PipelineState from the run dir's JSON (via load_run_state), points the panel at that dir so 播 |
| `nanocodex/gui.py` | 4200 | function | `_sb_open_dir` | Open a run's archived directory in the OS file manager. |
| `nanocodex/gui.py` | 4218 | function | `_on_ab_compare` | Open the A/B setup dialog: two configs + one prompt, run isolated. Disabled while busy (an A/B run rebuilds loops and drives files, same as a turn). Requires a clean git workspace  |
| `nanocodex/gui.py` | 4241 | function | `_show_ab_setup_dialog` | Two columns of config controls + a shared prompt box + Run button. |
| `nanocodex/gui.py` | 4287 | function | `_make_column` |  |
| `nanocodex/gui.py` | 4296 | function | `_opt` |  |
| `nanocodex/gui.py` | 4332 | function | `_overrides_from` |  |
| `nanocodex/gui.py` | 4340 | function | `_run` |  |
| `nanocodex/gui.py` | 4350 | function | `ab_btn` |  |
| `nanocodex/gui.py` | 4364 | function | `_start_ab_run` | Kick off the A/B worker thread (mirrors _start_turn's setup). |
| `nanocodex/gui.py` | 4382 | function | `_run_ab_thread` | Daemon thread: run both sides serially in isolated worktrees. Mirrors _run_turn_thread (own asyncio loop, desktop lock, results via the UI queue). Worktrees are NOT cleaned here —  |
| `nanocodex/gui.py` | 4415 | function | `_run_side` |  |
| `nanocodex/gui.py` | 4450 | function | `_show_ab_result_dialog` | Show both sides' summary + diff; adopt one or discard both. Adopting applies the chosen side's diff onto the real workspace; then BOTH worktrees are cleaned up. Discarding cleans b |
| `nanocodex/gui.py` | 4477 | function | `_cleanup_both` |  |
| `nanocodex/gui.py` | 4483 | function | `_adopt` |  |
| `nanocodex/gui.py` | 4497 | function | `_discard` |  |
| `nanocodex/gui.py` | 4505 | function | `rb` |  |
| `nanocodex/gui.py` | 4545 | function | `_start_turn` | Echo the prompt and kick off a worker turn for it (idle path). Shared by _on_send (when not busy) and the queue drain at turn end, so the 'echo + clear cancel + busy + spawn worker |
| `nanocodex/gui.py` | 4572 | function | `_drain_queue` | At turn end, start the next queued input if any (main thread). Stop only cancels the CURRENT turn (the user's choice); the queue keeps going, so a cancelled turn still hands off to |
| `nanocodex/gui.py` | 4586 | function | `_refresh_send_label` | Update the Send button text to reflect the queue backlog. |
| `nanocodex/gui.py` | 4596 | function | `_request_stop` | Ask the running turn to stop at its next cancellation point. |
| `nanocodex/gui.py` | 4604 | function | `_run_turn_thread` | Runs on a daemon thread; owns its own asyncio loop for this turn. |
| `nanocodex/gui.py` | 4638 | function | `_handle_loop_command` | `/loop [interval] <prompt>`: repeat a prompt on an interval until Stop. Ad-hoc, in-session, no persistence — complements the (cron-like) scheduler. The interval accepts 30s / 5m /  |
| `nanocodex/gui.py` | 4664 | function | `_run_loop_thread` | Daemon thread: re-run `prompt` every `interval_s`s until Stop. Mirrors _run_turn_thread per iteration (own asyncio loop, desktop lock, results via the UI queue), then waits the int |
| `nanocodex/gui.py` | 4718 | function | `_make_gui_hooks` |  |
| `nanocodex/gui.py` | 4725 | async function | `on_reasoning` |  |
| `nanocodex/gui.py` | 4728 | async function | `on_content` |  |
| `nanocodex/gui.py` | 4731 | async function | `on_stream_end` |  |
| `nanocodex/gui.py` | 4734 | async function | `on_tool_start` |  |
| `nanocodex/gui.py` | 4747 | async function | `on_tool_result` |  |
| `nanocodex/gui.py` | 4767 | function | `_poll_queue` |  |
| `nanocodex/gui.py` | 4776 | function | `_handle_event` |  |
| `nanocodex/gui.py` | 4881 | function | `_record_session_index` | Upsert this workspace's summary into the global session directory. Runs on the main thread at turn end (the session message list is stable then). Best-effort: a directory-index fai |
| `nanocodex/gui.py` | 4907 | function | `_refresh_session_list` | Repopulate the sidebar from the global index, newest activity first. Runs on the main thread (turn end / startup). Best-effort: a listing failure must never disturb the conversatio |
| `nanocodex/gui.py` | 4947 | function | `_refresh_schedule_panel` | Repaint the Scheduled panel from the store + the live running flag. Runs on the main thread (slow timer / toggle / startup). Everything but "running now" comes from ~/.nanocodex/sc |
| `nanocodex/gui.py` | 4987 | function | `_start_schedule_panel_refresh` | Arm the slow Scheduled-panel repaint loop (once). Separate from the 40ms _poll_queue: the panel only needs to track the running dot + next/last times, so a ~3s cadence is plenty an |
| `nanocodex/gui.py` | 5000 | function | `_tick` |  |
| `nanocodex/gui.py` | 5007 | function | `_on_session_select` | Replay the selected conversation: a summary header + the FULL frozen transcript (when a snapshot exists). Read-only: this surfaces the stored digest plus the complete message histo |
| `nanocodex/gui.py` | 5105 | function | `_render_transcript` | Render a frozen message list into the replay Text widget (read-only). Skips the system prompt (scaffolding, not conversation); shows each user/assistant message and a compact one-l |
| `nanocodex/gui.py` | 5136 | function | `_continue_session` | Fork the selected past conversation into a NEW one and continue it. Non-destructive: the original session's snapshot/log are untouched. We load its frozen transcript, mint a FRESH  |
| `nanocodex/gui.py` | 5191 | function | `_echo_seed_transcript` | Replay inherited messages into the MAIN panel (not the replay popup). Mirrors _render_transcript's role mapping but writes to self._append so the continued thread looks like the li |
| `nanocodex/gui.py` | 5219 | function | `_render_plan` |  |
| `nanocodex/gui.py` | 5226 | function | `_record_turn_cost` | Price this turn's usage and fold it into the running session total. Uses the REAL usage the provider reported (summed across the turn's model calls in loop.run_turn), priced via pr |
| `nanocodex/gui.py` | 5248 | function | `_announce_turn_end` | Say WHY the turn ended, so a mid-task stop is never a silent mystery. result is a TurnResult or None (None = an exception already reported). |
| `nanocodex/gui.py` | 5293 | function | `_append` |  |
| `nanocodex/gui.py` | 5299 | function | `_set_busy` |  |
| `nanocodex/gui.py` | 5309 | function | `run` |  |
| `nanocodex/gui.py` | 5320 | function | `_summarize` |  |
| `nanocodex/gui.py` | 5343 | function | `_summarize_desktop` | Human-readable description of one desktop action, for the live view. |
| `nanocodex/gui.py` | 5378 | function | `_line_gutter` | Right-aligned line-number gutter; blanks when the number is absent. |
| `nanocodex/gui.py` | 5385 | function | `_classify_patch_file` | Turn one parsed FileAction into a render-ready dict of classified rows. Pure: consumes nanocodex.tools.patch data only, touches no Tk and no disk. Caps total rows at _FILE_PANEL_MA |
| `nanocodex/gui.py` | 5396 | function | `add_row` |  |
| `nanocodex/gui.py` | 5444 | function | `_build_file_edit_payload` | Parse a V4A patch into a Tk-free payload for the file panel. Returns None on a parse error or a no-op patch (every file has zero rows), so a malformed or empty patch never blanks a |
| `nanocodex/gui.py` | 5464 | function | `_is_mcp_command` | An approval request whose 'command' is an MCP tool name (mcp__<srv>__<tool>). MCP desktop tools post their tool NAME as the approval command (see McpTool._gate_decision), so this d |
| `nanocodex/gui.py` | 5474 | function | `_approval_short_circuit` | Pure decision: may this approval request skip the dialog and auto-approve? Mirrors Codex's "approve for session" semantics. Returns True when: * global auto-approve is on (everythi |
| `nanocodex/gui.py` | 5497 | function | `_scheduler_run_plan` | Pure decision for how the managed scheduler runs one task. Returns ``(approver_kind, attach_mcp_tools)`` where ``approver_kind`` is ``"desktop_only"`` or ``"auto_deny"``. The whole |
| `nanocodex/gui.py` | 5529 | function | `_scheduler_turn_timeout` | Resolve the scheduled-turn timeout (env override, else the default). |
| `nanocodex/gui.py` | 5541 | async function | `_run_scheduled_turn` | Run ONE unattended scheduled turn under *lock*, bounded by a timeout. Tk-free and fully injectable so it unit-tests offline: * ``lock`` — a ``threading.Lock``-like (``acquire(block |
| `nanocodex/gui.py` | 5574 | function | `cancel_check` |  |
| `nanocodex/gui.py` | 5586 | async function | `_soft_deadline` |  |
| `nanocodex/gui.py` | 5620 | function | `_now_iso` | Current local time as an ISO second-precision string (for log lines). |
| `nanocodex/gui.py` | 5626 | function | `_format_scheduler_log_entry` | Format one ~/.nanocodex/scheduler.log line (pure; timestamp injected). Unattended runs never touch the transcript (user's decision), so this file is the only record. Kept Tk-free a |
| `nanocodex/gui.py` | 5646 | function | `_hhmm` | Pull HH:MM out of an ISO timestamp for compact display; tolerate junk. |
| `nanocodex/gui.py` | 5653 | function | `_format_schedule_panel_line` | Format one scheduled task into a 1-2 line sidebar panel block (pure). Tk-free + clock-free so it unit-tests deterministically. Layout: <glyph> <label> [desktop] <state/next/last/×r |
| `nanocodex/gui.py` | 5695 | function | `_settings_sections` | Ordered nav entries for the Settings window (Codex-style sections). Pure (no Tk) so the navigation order can be unit-tested. The strings double as both the nav-button labels and th |
| `nanocodex/gui.py` | 5712 | function | `_collect_schedule_add` | Coerce raw Scheduled-tasks form fields into ScheduleStore.add() kwargs. Pure (no Tk) so it unit-tests cleanly, and it mirrors exactly what the conversational manage_schedule tool d |
| `nanocodex/gui.py` | 5728 | function | `_int` |  |
| `nanocodex/gui.py` | 5745 | function | `_format_schedule_recurrence` | One-line recurrence summary for a task row (pure, unit-testable). once -> "once" interval -> "every Ns" (or a friendlier "every Nm"/"every Nh" for round minute/hour periods, so a 3 |
| `nanocodex/gui.py` | 5770 | function | `_collect_settings_updates` | Build the updates dict for write_nanocodex_config from raw field values. Pure (no Tk) so it unit-tests cleanly. Rules: * A blank new API key / VL key is OMITTED — an empty submit m |
| `nanocodex/gui.py` | 5812 | function | `_send_button_label` | Text for the Send button given how many inputs are QUEUED behind the running turn (Codex-style: you can type the next task while one runs). Pure so it unit-tests without Tk: * 0 qu |
| `nanocodex/gui.py` | 5828 | function | `_fmt_tok` | Format a token count like Claude Code: 666, 12.3k, 1.0M. |
| `nanocodex/gui.py` | 5837 | function | `_fmt_usd` | Format a USD cost. Sub-cent turns are common (a cache-hit prompt costs fractions of a cent), so show 4 decimals under $1 and 2 above — a flat ``$0.00`` would hide every cheap turn. |
| `nanocodex/gui.py` | 5848 | function | `_fmt_cny` | Format a CNY cost. Seedance clips cost a few yuan each, so 2 decimals is plenty; sub-cent rounding isn't a concern as it is for USD turns. |
| `nanocodex/gui.py` | 5856 | function | `_build_status` | Pure status-bar text builder (no Tk) so it can be unit-tested. Always shows state; shows the error if the loop failed to build (so the bar is never mysteriously blank); otherwise s |
| `nanocodex/gui.py` | 5892 | function | `launch` |  |
| `nanocodex/gui.py` | 5896 | function | `main_cli` | Console entry point for ``nanocodex-gui``. Thin argparse front end (Typer isn't needed here): supports the same workspace / sandbox / approval / model / resume knobs as the CLI, th |
| `nanocodex/provider/__init__.py` | 1 | module | `__init__` | LLM provider layer. |
| `nanocodex/provider/base.py` | 1 | module | `base` | Provider protocol and shared response types. |
| `nanocodex/provider/base.py` | 10 | class | `ToolCall` | A single tool invocation requested by the model. |
| `nanocodex/provider/base.py` | 19 | class | `ModelResponse` | Normalized result of one model call. |
| `nanocodex/provider/base.py` | 29 | function | `has_tool_calls` |  |
| `nanocodex/provider/base.py` | 33 | class | `Provider` | Minimal async chat interface every backend must implement. |
| `nanocodex/provider/base.py` | 38 | async function | `chat` |  |
| `nanocodex/provider/deepseek.py` | 1 | module | `deepseek` | DeepSeek provider (OpenAI-compatible chat-completions with tool calling). |
| `nanocodex/provider/deepseek.py` | 34 | function | `_stream_open_timeout_s` | Bounded override for the streaming response-header wait (seconds). |
| `nanocodex/provider/deepseek.py` | 46 | class | `ProviderError` | Raised when the backend call fails irrecoverably. |
| `nanocodex/provider/deepseek.py` | 50 | function | `_extract_usage` | Normalize an SDK usage object into a plain int dict. Captures prompt/completion tokens plus DeepSeek's cache-accounting fields. DeepSeek returns ``prompt_cache_hit_tokens`` / ``pro |
| `nanocodex/provider/deepseek.py` | 65 | function | `_get` |  |
| `nanocodex/provider/deepseek.py` | 90 | class | `DeepSeekProvider` | Talk to DeepSeek (or any OpenAI-compatible endpoint) over the SDK. |
| `nanocodex/provider/deepseek.py` | 96 | function | `__init__` |  |
| `nanocodex/provider/deepseek.py` | 117 | function | `_build_kwargs` |  |
| `nanocodex/provider/deepseek.py` | 139 | async function | `chat` |  |
| `nanocodex/provider/deepseek.py` | 182 | async function | `chat_stream` | Stream a completion, invoking delta callbacks, and return the aggregate. Mirrors :meth:`chat`'s return shape so the loop can treat both identically once the stream finishes. |
| `nanocodex/provider/deepseek.py` | 289 | function | `_extract_reasoning` | Read DeepSeek/OpenAI-compatible reasoning fields from SDK objects. |
| `nanocodex/provider/deepseek.py` | 294 | function | `_is_deepseek_model` | True for DeepSeek's own models (their thinking-mode API shape applies). |
| `nanocodex/provider/deepseek.py` | 299 | function | `_apply_reasoning_effort` | Translate a reasoning-effort tier into the right request fields per backend. DeepSeek's thinking-mode API (the default backend) only understands enabled/disabled plus ``reasoning_e |
| `nanocodex/provider/deepseek.py` | 344 | function | `_sanitize_reasoning_replay` | Ensure DeepSeek thinking-mode tool-call history replays reasoning_content. DeepSeek V4/reasoner rejects a later request when an assistant history item carries tool_calls but lacks  |
| `nanocodex/provider/deepseek.py` | 368 | function | `_should_replay_reasoning_content` |  |
| `nanocodex/provider/deepseek.py` | 374 | function | `_requires_reasoning_content` |  |
| `nanocodex/sandbox/__init__.py` | 1 | module | `__init__` | Codex-style sandbox: policy + approval + executor. |
| `nanocodex/sandbox/approval.py` | 1 | module | `approval` | Codex-style approval state machine. Mirrors Codex's four approval policies: untrusted auto-run only known-safe (read-only-ish) commands; ask for the rest on-failure run sandboxed f |
| `nanocodex/sandbox/approval.py` | 30 | class | `Decision` |  |
| `nanocodex/sandbox/approval.py` | 41 | function | `step_decision` | Layer per-step confirmation on top of the sandbox-escalation decision. The base decision comes from :meth:`Approver.classify` (escalation/policy). When the user has turned OFF auto |
| `nanocodex/sandbox/approval.py` | 84 | class | `ApprovalRequest` | Context handed to the approval callback for a human decision. |
| `nanocodex/sandbox/approval.py` | 102 | function | `_first_token` |  |
| `nanocodex/sandbox/approval.py` | 110 | function | `_is_trusted` |  |
| `nanocodex/sandbox/approval.py` | 129 | class | `Approver` | Decide whether a shell command may run, and prompt when required. |
| `nanocodex/sandbox/approval.py` | 132 | function | `__init__` |  |
| `nanocodex/sandbox/approval.py` | 136 | function | `classify` | Pure decision: can this run automatically, must we ask, or auto-deny? *needs_escalation* is True when the command wants something the sandbox forbids (e.g. writing outside the work |
| `nanocodex/sandbox/approval.py` | 157 | async function | `request` | Ask the human (via the injected callback). Returns approval. |
| `nanocodex/sandbox/executor.py` | 1 | module | `executor` | Sandboxed command execution. Honesty note on platform fidelity --------------------------------- Real Codex isolates commands with OS kernel facilities: Seatbelt on macOS and Landl |
| `nanocodex/sandbox/executor.py` | 70 | class | `_IO_COUNTERS` |  |
| `nanocodex/sandbox/executor.py` | 75 | class | `_JOBOBJECT_BASIC_LIMIT_INFORMATION` |  |
| `nanocodex/sandbox/executor.py` | 88 | class | `_JOBOBJECT_EXTENDED_LIMIT_INFORMATION` |  |
| `nanocodex/sandbox/executor.py` | 98 | class | `_WindowsJob` | A Win32 Job Object that kills its whole process tree when closed. Raises OSError if any Job API call fails so the caller can degrade to an un-contained run rather than failing the  |
| `nanocodex/sandbox/executor.py` | 105 | function | `__init__` |  |
| `nanocodex/sandbox/executor.py` | 133 | function | `assign` | Put process *pid* (and thus its future children) into the job. |
| `nanocodex/sandbox/executor.py` | 143 | function | `terminate` | Kill every process in the job at once. |
| `nanocodex/sandbox/executor.py` | 148 | function | `close` | Release handles. Closing the last job handle (kill-on-close) also reaps any still-running tree member, so orphans can't survive. |
| `nanocodex/sandbox/executor.py` | 159 | class | `ExecResult` | Outcome of a single sandboxed command. |
| `nanocodex/sandbox/executor.py` | 170 | function | `ok` |  |
| `nanocodex/sandbox/executor.py` | 173 | function | `render` |  |
| `nanocodex/sandbox/executor.py` | 195 | class | `PolicyExecutor` | Run commands under policy-level enforcement (see module docstring). |
| `nanocodex/sandbox/executor.py` | 198 | function | `__init__` |  |
| `nanocodex/sandbox/executor.py` | 201 | function | `preflight` | Static check before running. Returns (allowed, reason_if_denied). Conservative: only blocks what we can clearly attribute to a write outside the writable roots. Ambiguous commands  |
| `nanocodex/sandbox/executor.py` | 212 | async function | `run` |  |
| `nanocodex/sandbox/executor.py` | 256 | function | `_cleanup` | Hook: release per-process resources after a run. Base does nothing. |
| `nanocodex/sandbox/executor.py` | 259 | async function | `_spawn` |  |
| `nanocodex/sandbox/executor.py` | 277 | async function | `_kill` |  |
| `nanocodex/sandbox/executor.py` | 286 | function | `_build_env` |  |
| `nanocodex/sandbox/executor.py` | 312 | class | `WindowsJobExecutor` | Windows backend: run each command inside a Job Object. Adds real OS-level PROCESS/RESOURCE containment on top of PolicyExecutor — the command's whole descendant tree lives in a Win |
| `nanocodex/sandbox/executor.py` | 326 | async function | `_spawn` |  |
| `nanocodex/sandbox/executor.py` | 337 | async function | `_kill` |  |
| `nanocodex/sandbox/executor.py` | 343 | function | `_cleanup` |  |
| `nanocodex/sandbox/executor.py` | 349 | function | `make_executor` | Pick the best available executor for this platform. Windows gets :class:`WindowsJobExecutor` (Job Object process/resource containment); other platforms get :class:`PolicyExecutor`  |
| `nanocodex/sandbox/policy.py` | 1 | module | `policy` | Codex-style sandbox policy. Mirrors Codex's three sandbox modes: read-only read anywhere; no writes; no network workspace-write read anywhere; write to workspace + writable roots + |
| `nanocodex/sandbox/policy.py` | 28 | class | `SandboxPolicy` | Resolved filesystem/network permissions for a sandbox mode. |
| `nanocodex/sandbox/policy.py` | 40 | function | `__post_init__` |  |
| `nanocodex/sandbox/policy.py` | 45 | function | `from_config` |  |
| `nanocodex/sandbox/policy.py` | 56 | function | `writes_allowed` |  |
| `nanocodex/sandbox/policy.py` | 59 | function | `_writable_dirs` |  |
| `nanocodex/sandbox/policy.py` | 65 | function | `can_read` |  |
| `nanocodex/sandbox/policy.py` | 70 | function | `can_write` |  |
| `nanocodex/sandbox/policy.py` | 87 | function | `describe` |  |
| `nanocodex/storyboard/__init__.py` | 1 | module | `__init__` | Storyboard video pipeline: story text + images -> shots -> Seedance video. A self-contained sub-package that turns a story and a handful of reference images into a storyboard (a li |
| `nanocodex/storyboard/clients.py` | 1 | module | `clients` | Model adapters for the storyboard pipeline (injectable, offline-testable). Three clients, one per external capability the pipeline needs: * :class:`VisionAnalyzer` — wraps an OpenA |
| `nanocodex/storyboard/clients.py` | 42 | function | `_load_prompt` |  |
| `nanocodex/storyboard/clients.py` | 49 | class | `ChatProvider` | The subset of provider/deepseek.py:DeepSeekProvider we rely on. |
| `nanocodex/storyboard/clients.py` | 54 | async function | `chat` |  |
| `nanocodex/storyboard/clients.py` | 65 | function | `_extract_json` | Pull the first JSON object/array out of a model reply. Models often wrap JSON in prose or ```json fences. Be lenient: strip fences, then grab the outermost {...} or [...]. Raises V |
| `nanocodex/storyboard/clients.py` | 95 | class | `VisionAnalyzer` | Analyze one image into an AssetAnalysis via a vision-capable provider. |
| `nanocodex/storyboard/clients.py` | 98 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 102 | async function | `analyze` |  |
| `nanocodex/storyboard/clients.py` | 124 | function | `_chapters_for_prompt` | Render chapters as a compact numbered outline for the shot-planner prompt. Returns "(none)" when there are no chapters, so the prompt's fallback branch (plan straight from the full |
| `nanocodex/storyboard/clients.py` | 149 | class | `ChapterPlanner` | Split a story into chapters (the story-detail layer above shots). |
| `nanocodex/storyboard/clients.py` | 152 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 156 | async function | `plan` |  |
| `nanocodex/storyboard/clients.py` | 191 | class | `TextPlanner` | Turn story text into a list of Shot objects via the main provider. |
| `nanocodex/storyboard/clients.py` | 194 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 198 | async function | `plan` |  |
| `nanocodex/storyboard/clients.py` | 250 | function | `_shots_for_prompt` | Render shots as a compact ordered outline for the continuity prompt. One block per shot (shot_id · title, then its 中文画面 prompt_zh — or the English prompt as a fallback — and any di |
| `nanocodex/storyboard/clients.py` | 272 | function | `_available_for_prompt` | Describe which shots actually have a rendered clip (vs. real gaps). ``available_ids is None`` means the caller is checking at planning time with no render yet — return "(all)" so t |
| `nanocodex/storyboard/clients.py` | 293 | class | `ContinuityChecker` | Flag missing story beats between consecutive shots (pre-merge review). Unlike the planners, an EMPTY result is a valid good outcome: clean shots return ``ok=True`` with no gaps and |
| `nanocodex/storyboard/clients.py` | 302 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 306 | async function | `check` |  |
| `nanocodex/storyboard/clients.py` | 365 | function | `_urllib_transport` | Default transport: stdlib urllib (no extra deps), used in production. |
| `nanocodex/storyboard/clients.py` | 382 | class | `SeedanceError` | Raised when a Seedance task fails to submit or render. |
| `nanocodex/storyboard/clients.py` | 387 | class | `SeedanceResult` | Outcome of a finished Seedance task. Carries the signed ``video_url`` plus the raw ``usage`` dict from the task response. The live API returns ``usage.total_tokens`` on success (ve |
| `nanocodex/storyboard/clients.py` | 400 | class | `SeedanceClient` | Submit a video task to ARK and poll until it renders. The ARK video API is asynchronous: ``submit`` returns a task id, then you ``poll`` that id until status is ``succeeded`` (then |
| `nanocodex/storyboard/clients.py` | 411 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 422 | function | `_headers` |  |
| `nanocodex/storyboard/clients.py` | 428 | function | `submit` | POST a generation task; return its task id. |
| `nanocodex/storyboard/clients.py` | 441 | function | `poll_once` | GET task status once. Return (status, video_url_or_empty, usage). ``usage`` is the raw usage dict from the response (``{}`` if absent). On success it carries ``total_tokens``, whic |
| `nanocodex/storyboard/clients.py` | 464 | function | `generate` | Submit then poll until the video is ready; return a SeedanceResult. The result carries the signed video URL plus the response ``usage`` dict (with ``total_tokens`` for billing). Ra |
| `nanocodex/storyboard/models.py` | 1 | module | `models` | Data models + JSON-Schema validation for the storyboard pipeline. House style mirrors agent/schedule.py: plain dataclasses for the typed shape, pure functions over data, no I/O. Th |
| `nanocodex/storyboard/models.py` | 21 | class | `StoryboardError` | Raised when a project fails schema validation or a stage cannot proceed. |
| `nanocodex/storyboard/models.py` | 26 | function | `_load_schema` |  |
| `nanocodex/storyboard/models.py` | 31 | function | `validate_project` | Validate a raw project dict against the draft-07 schema. Raises :class:`StoryboardError` with a path-qualified message on the first violation. ``jsonschema`` is an optional-but-dec |
| `nanocodex/storyboard/models.py` | 57 | class | `Project` |  |
| `nanocodex/storyboard/models.py` | 74 | class | `ImageInput` |  |
| `nanocodex/storyboard/models.py` | 82 | class | `Character` |  |
| `nanocodex/storyboard/models.py` | 92 | class | `Chapter` | A story chapter — the "story-detail" layer that sits ABOVE shots. A long story is first split into a handful of chapters (3-8), each carrying its plot summary, setting, cast and ke |
| `nanocodex/storyboard/models.py` | 111 | class | `AssetAnalysis` |  |
| `nanocodex/storyboard/models.py` | 120 | class | `Shot` |  |
| `nanocodex/storyboard/models.py` | 142 | class | `ContinuityGap` | A missing story beat between two consecutive shots. The continuity checker (clients.py:ContinuityChecker) flags places where the storyboard jumps — a small transition/beat is missi |
| `nanocodex/storyboard/models.py` | 166 | class | `ContinuityReport` | Result of a pre-merge continuity check over a storyboard. ``ok`` True with empty ``gaps`` means the shots flow cleanly; otherwise ``gaps`` lists the missing beats with 补镜 suggestio |
| `nanocodex/storyboard/models.py` | 181 | class | `SeedancePayload` |  |
| `nanocodex/storyboard/models.py` | 187 | function | `project_from_dict` | Build the typed Project + image inputs from a validated dict. Call :func:`validate_project` first; this assumes the shape is already schema-valid and only pulls the fields the pipe |
| `nanocodex/storyboard/models.py` | 216 | function | `as_jsonable` | Recursively convert dataclasses to plain dicts for JSON export. |
| `nanocodex/storyboard/pipeline.py` | 1 | module | `pipeline` | Storyboard pipeline: story text + images -> shots -> Seedance payloads -> video. Seven stages, run in order by :func:`run_pipeline`. Each stage is a small pure-ish function ``(stat |
| `nanocodex/storyboard/pipeline.py` | 49 | class | `PipelineDeps` | Injected capabilities. Any may be None when its stage is not exercised. Tests pass fakes; production wires real clients (clients.py). Keeping them optional lets the offline tests r |
| `nanocodex/storyboard/pipeline.py` | 64 | class | `PipelineState` | The running project as it accretes through the stages. |
| `nanocodex/storyboard/pipeline.py` | 80 | function | `_payload_has_video_input` | True if a Seedance payload's content includes a VIDEO reference block. Seedance charges a cheaper rate when the INPUT contains video (22 vs 37 CNY/1M). This pipeline currently send |
| `nanocodex/storyboard/pipeline.py` | 103 | function | `ingest` | Validate the raw project dict and build the initial state. |
| `nanocodex/storyboard/pipeline.py` | 114 | async function | `analyze_assets` | Run the vision analyzer over every input image. |
| `nanocodex/storyboard/pipeline.py` | 128 | async function | `plan_chapters` | Split the story into chapters (3-8) BEFORE it is broken into shots. Skipped when no chapter planner is injected (offline tests / callers that don't want the chapter layer), in whic |
| `nanocodex/storyboard/pipeline.py` | 147 | async function | `plan_storyboard` | Turn the story text into shots via the text planner. When chapters were planned, they are passed through so shots are sliced chapter by chapter (continuity preserved); otherwise th |
| `nanocodex/storyboard/pipeline.py` | 183 | function | `scan_multi_action_shots` | Flag shots whose text suggests MULTIPLE ordered actions in one shot. Returns ``{shot_id: [matched markers]}`` for every shot whose ``camera`` / ``prompt`` / ``prompt_zh`` contains  |
| `nanocodex/storyboard/pipeline.py` | 214 | async function | `check_continuity` | Review the planned shots for missing story beats before a merge. Standalone — NOT part of :func:`run_planning` (it never spends and never blocks rendering): the GUI calls it just b |
| `nanocodex/storyboard/pipeline.py` | 238 | function | `_classify` | Decide whether an image is a character or a background. Prefer the user-declared ``kind`` from the input; otherwise infer from the VL ``usable_for`` / ``scene_tags`` tags. Defaults |
| `nanocodex/storyboard/pipeline.py` | 253 | function | `map_assets` | Attach background/character image ids to each shot (rule-based MVP). The MVP rule: split images into character vs background buckets (by declared kind, else VL tags), then give eve |
| `nanocodex/storyboard/pipeline.py` | 283 | function | `_build_shot_payload` | Assemble ONE Seedance payload for a single shot. Shared by :func:`build_payloads` (the whole storyboard) and :func:`insert_fill_shot` (one補镜 added after the fact), so a fill-in sho |
| `nanocodex/storyboard/pipeline.py` | 307 | function | `_ref_url` | Turn a reference-image source into something ARK accepts. ARK's ``image_url`` takes a fetchable URL or a base64 data URI — NOT a local disk path (that returns HTTP 400 InvalidParam |
| `nanocodex/storyboard/pipeline.py` | 372 | function | `build_payloads` | Assemble one Seedance payload per shot. Mirrors the ARK content-shape verified live: a text block (prompt) plus optional reference_image blocks (first character + first background) |
| `nanocodex/storyboard/pipeline.py` | 392 | function | `_unique_fill_id` | Pick a fresh shot_id for a 补镜 wedged after ``after_id``. ``shot_03`` → ``shot_03b`` (then ``shot_03c`` …) so the id sorts/reads right between ``after_id`` and the next shot. Falls  |
| `nanocodex/storyboard/pipeline.py` | 410 | function | `insert_fill_shot` | Adopt one continuity-gap suggestion as a REAL shot, inserted in order. Turns a :class:`ContinuityGap`'s 补镜 suggestion into a :class:`Shot`, gives it a fresh id wedged right after ` |
| `nanocodex/storyboard/pipeline.py` | 465 | function | `_set_first_frame` | Make ``frame_uri`` the shot's ARK ``first_frame`` reference image. Removes any existing image_url blocks (the subject ``reference_image`` plus any earlier first_frame) before addin |
| `nanocodex/storyboard/pipeline.py` | 489 | function | `_default_frame_extractor` | Extract a clip's LAST frame as a base64 JPEG data URI (None on failure). ffmpeg reads the (signed) video URL directly — no local download needed — seeks 1s before the end (``-sseof |
| `nanocodex/storyboard/pipeline.py` | 525 | function | `_render_chained` | Render shots IN ORDER, threading each shot's last frame into the next. For 画面前后衔接: shot N renders, its last frame is extracted and injected as shot N+1's ``first_frame`` (baked int |
| `nanocodex/storyboard/pipeline.py` | 558 | function | `render_one` | Render (or RE-render) a single shot by id, updating *state* in place. Returns True on success (``video_urls[shot_id]`` is a real URL), False on failure (``video_urls[shot_id]`` hol |
| `nanocodex/storyboard/pipeline.py` | 574 | function | `_cb` |  |
| `nanocodex/storyboard/pipeline.py` | 601 | function | `render` | Render each shot's payload to a video via Seedance (OPT-IN). Only called when the caller explicitly enables rendering. Each clip is real spend, so failures on one shot are recorded |
| `nanocodex/storyboard/pipeline.py` | 658 | function | `export` | Write asset_analysis / storyboard / seedance_payloads / video urls to json. Returns the paths written. Video URLs are signed + expire (~24h) — noted in the urls file so a stale lin |
| `nanocodex/storyboard/pipeline.py` | 725 | function | `_slug_title` | Turn a story title into a filesystem-safe slug of at most ``max_len`` chars. Illegal chars → "_", runs of whitespace → "_", trimmed of leading/trailing separators. Empty/blank titl |
| `nanocodex/storyboard/pipeline.py` | 738 | function | `make_run_dir` | Create and return a unique run directory ``<base>/runs/<ts>_<slug>/``. ``ts`` is ``YYYYMMDD-HHMM`` (local time) so runs sort chronologically; the slug is the cleaned story title (< |
| `nanocodex/storyboard/pipeline.py` | 765 | function | `write_run_index` | Append a run summary to ``<base>/runs/index.json`` (append-only history). The index is a list of run-meta dicts (newest appended last). A run is keyed by ``run_id``: writing the sa |
| `nanocodex/storyboard/pipeline.py` | 794 | function | `read_run_index` | Read ``<base>/runs/index.json`` into a list; [] when missing/unreadable. |
| `nanocodex/storyboard/pipeline.py` | 816 | function | `_dataclass_from_dict` | Build a dataclass instance from a dict, keeping only known fields. Unknown keys are dropped and missing keys fall back to the dataclass's own defaults, so JSON written by an older  |
| `nanocodex/storyboard/pipeline.py` | 831 | function | `load_run_state` | Reconstruct a :class:`PipelineState` from a run's exported JSON files. Reads chapters/asset_analysis/storyboard/seedance_payloads/video_urls/ video_cost from ``run_dir`` (each miss |
| `nanocodex/storyboard/pipeline.py` | 845 | function | `_read_json` |  |
| `nanocodex/storyboard/pipeline.py` | 854 | function | `_rows` |  |
| `nanocodex/storyboard/pipeline.py` | 912 | function | `_default_runner` | Default Runner: run argv, capture combined output (no shell). On Windows each ffmpeg/ffprobe child would otherwise flash its own black console window (a concat probes every clip th |
| `nanocodex/storyboard/pipeline.py` | 933 | function | `_ffprobe_params` | Probe (width, height, avg_frame_rate) via ffprobe; None on any failure. |
| `nanocodex/storyboard/pipeline.py` | 952 | function | `concat_clips` | Stitch a run's shot clips into one ``<run_dir>/<dest_name>``, in order. Picks ``<run_dir>/<shot_id>.mp4`` for each id in ``shot_ids`` THAT EXISTS (missing shots — e.g. ones that fa |
| `nanocodex/storyboard/pipeline.py` | 1019 | async function | `run_planning` | Run the PLANNING half only: ingest → analyze → chapters → shots → map → payloads. NEVER renders (never spends money). This is the "preview" path. Returns the planned state (chapter |
| `nanocodex/storyboard/pipeline.py` | 1039 | function | `render_state` | Render an ALREADY-PLANNED state (the "make video" path). Call this on a state returned by :func:`run_planning` once the user has reviewed the preview and chosen to spend. Runs the  |
| `nanocodex/storyboard/pipeline.py` | 1060 | async function | `run_pipeline` | Run all stages in order. Returns (final_state, exported_paths). ``render_video`` defaults False — Seedance billing is opt-in. ``out_dir`` None skips the export write (used by tests |
| `nanocodex/tools/__init__.py` | 1 | module | `__init__` | Tool registry: build the Codex tool set and dispatch calls. |
| `nanocodex/tools/__init__.py` | 32 | class | `ToolRegistry` |  |
| `nanocodex/tools/__init__.py` | 33 | function | `__init__` |  |
| `nanocodex/tools/__init__.py` | 42 | function | `register` |  |
| `nanocodex/tools/__init__.py` | 45 | function | `get` |  |
| `nanocodex/tools/__init__.py` | 49 | function | `names` |  |
| `nanocodex/tools/__init__.py` | 52 | function | `schemas` |  |
| `nanocodex/tools/__init__.py` | 55 | async function | `execute` |  |
| `nanocodex/tools/apply_patch.py` | 1 | module | `apply_patch` | apply_patch tool: the model's primary way to edit files (Codex V4A format). |
| `nanocodex/tools/apply_patch.py` | 22 | class | `ApplyPatchTool` |  |
| `nanocodex/tools/apply_patch.py` | 28 | function | `name` |  |
| `nanocodex/tools/apply_patch.py` | 32 | function | `description` |  |
| `nanocodex/tools/apply_patch.py` | 44 | function | `parameters` |  |
| `nanocodex/tools/apply_patch.py` | 59 | async function | `execute` |  |
| `nanocodex/tools/apply_patch.py` | 116 | function | `can_write` |  |
| `nanocodex/tools/apply_patch.py` | 133 | function | `_escaping_paths` | Resolved targets the patch would touch that fall outside the sandbox. |
| `nanocodex/tools/apply_patch.py` | 151 | function | `_rel` |  |
| `nanocodex/tools/base.py` | 1 | module | `base` | Tool base classes and the execution context shared across tools. |
| `nanocodex/tools/base.py` | 16 | class | `ToolContext` | Everything a tool needs to do its job, injected at registration. |
| `nanocodex/tools/base.py` | 47 | class | `Tool` | An agent capability exposed to the model as an OpenAI function tool. |
| `nanocodex/tools/base.py` | 65 | function | `__init__` |  |
| `nanocodex/tools/base.py` | 70 | function | `name` |  |
| `nanocodex/tools/base.py` | 74 | function | `description` |  |
| `nanocodex/tools/base.py` | 78 | function | `parameters` | JSON Schema for the arguments object. |
| `nanocodex/tools/base.py` | 82 | async function | `execute` | Run the tool and return a string result for the model. |
| `nanocodex/tools/base.py` | 85 | function | `to_schema` |  |
| `nanocodex/tools/marketplace.py` | 1 | module | `marketplace` | MCP marketplace: a browsable catalog of MCP servers with one-click install. Two sources, both feeding the SAME ``McpStore`` the manual "MCP servers" settings section uses: * **Buil |
| `nanocodex/tools/marketplace.py` | 59 | class | `CatalogEntry` | One installable MCP server in the marketplace. ``env_keys`` lists the NAMES of environment variables the server needs the user to provide (e.g. an API token) — never the values. Th |
| `nanocodex/tools/marketplace.py` | 139 | function | `marketplace_url` | The configured remote catalog URL, or None when the env var is unset/blank. |
| `nanocodex/tools/marketplace.py` | 145 | function | `_entry_from_dict` | Build a validated :class:`CatalogEntry` from one remote JSON object. Returns None (caller drops it) when the entry is unusable: not an object, bad/missing name, or empty command. U |
| `nanocodex/tools/marketplace.py` | 173 | function | `parse_remote_catalog` | Parse a remote catalog JSON document into validated entries (pure). Accepts either a top-level list of entries, or an object with an ``entries``/``servers`` list. Malformed JSON yi |
| `nanocodex/tools/marketplace.py` | 211 | function | `_default_opener` | Fetch *url* and return the raw body bytes (network required; lazy import). |
| `nanocodex/tools/marketplace.py` | 219 | function | `fetch_remote_catalog` | Fetch and parse the remote catalog at *url*. *opener* is injectable so tests run fully offline. Network/parse errors propagate as exceptions to the caller (the GUI catches and show |
| `nanocodex/tools/marketplace.py` | 236 | function | `install_entry` | Install *entry* into mcp.toml via :class:`McpStore`. *env_values* supplies values for the env vars the entry declares; only keys in ``entry.env_keys`` are kept (extras ignored). *p |
| `nanocodex/tools/mcp.py` | 1 | module | `mcp` | MCP (Model Context Protocol) connector. Connects to configured MCP servers over stdio, discovers their tools, and wraps each remote tool as a nanocodex Tool named ``mcp__<server>__ |
| `nanocodex/tools/mcp.py` | 45 | class | `McpServerConfig` |  |
| `nanocodex/tools/mcp.py` | 56 | function | `extract_text` | Pull text out of an MCP CallToolResult; note non-text/error blocks. |
| `nanocodex/tools/mcp.py` | 85 | function | `extract_structured` | Pull an MCP tool's ``structuredContent`` (the machine-readable second layer), tolerating both attribute- and dict-shaped results. Many servers return a short human ``text`` block P |
| `nanocodex/tools/mcp.py` | 102 | function | `format_result` | Render an MCP result for the model: the text layer, plus a compact JSON dump of any structuredContent so structured data (window lists, handles, geometry) actually reaches the mode |
| `nanocodex/tools/mcp.py` | 138 | function | `is_readonly_mcp_tool` | True if the remote tool name looks read-only (non-mutating). |
| `nanocodex/tools/mcp.py` | 144 | class | `McpTool` | Wraps one remote MCP tool, delegating execution to its session. |
| `nanocodex/tools/mcp.py` | 147 | function | `__init__` |  |
| `nanocodex/tools/mcp.py` | 165 | function | `name` |  |
| `nanocodex/tools/mcp.py` | 169 | function | `description` |  |
| `nanocodex/tools/mcp.py` | 173 | function | `parameters` |  |
| `nanocodex/tools/mcp.py` | 179 | async function | `execute` |  |
| `nanocodex/tools/mcp.py` | 193 | async function | `_gate_decision` | Approval gate for MCP tools, mirroring ShellTool. MCP tools run OUTSIDE the sandbox (desktop/WeChat automation, etc.), so a write-class tool is treated as an escalated action: unde |
| `nanocodex/tools/mcp.py` | 236 | async function | `build_tools_for_session` | List an initialized session's tools and wrap each as an McpTool. *session* must expose ``async list_tools()`` (returning an object with a ``.tools`` list of descriptors with ``.nam |
| `nanocodex/tools/mcp.py` | 273 | function | `parse_mcp_servers` | Parse ``[mcp_servers.<name>]`` tables from a loaded TOML dict. |
| `nanocodex/tools/mcp.py` | 302 | function | `discover_mcp_servers` | Read MCP server definitions from nanocodex's own ~/.nanocodex/mcp.toml (best-effort). Deliberately isolated from ~/.codex/config.toml. |
| `nanocodex/tools/mcp.py` | 316 | class | `McpManager` | Owns the lifecycle of one or more live MCP stdio connections. Real subprocess I/O — only verifiable against a live server. Tools whose server fails to start are skipped; the error  |
| `nanocodex/tools/mcp.py` | 323 | function | `__init__` |  |
| `nanocodex/tools/mcp.py` | 337 | function | `_open_errlog` | Open a UTF-8 log for server stderr. Critical on Windows GUI launches: stdio_client defaults errlog to the parent's sys.stderr, which is None/invalid under pythonw (no console) and  |
| `nanocodex/tools/mcp.py` | 354 | async function | `connect` |  |
| `nanocodex/tools/mcp.py` | 378 | async function | `build_tools_with_ctx` | Re-wrap every connected server's tools against a DIFFERENT ctx. The tools returned by :meth:`connect` are bound to ``self.ctx`` (the GUI's interactive context, whose approver promp |
| `nanocodex/tools/mcp.py` | 395 | async function | `aclose` |  |
| `nanocodex/tools/mcp_store.py` | 1 | module | `mcp_store` | MCP plugin store: CRUD over nanocodex's own ``~/.nanocodex/mcp.toml``. The GUI plugin manager needs to add, edit, enable/disable, and remove MCP server definitions. ``tomllib`` (st |
| `nanocodex/tools/mcp_store.py` | 43 | function | `is_valid_server_name` | True if *name* is a safe bare TOML key (no quoting/escaping needed). |
| `nanocodex/tools/mcp_store.py` | 49 | function | `_esc` | Escape a string for a TOML basic (double-quoted) string. |
| `nanocodex/tools/mcp_store.py` | 63 | function | `_dump_one` | Render one server as a ``[mcp_servers.<name>]`` TOML block (pure). |
| `nanocodex/tools/mcp_store.py` | 80 | function | `dump_mcp_toml` | Serialize servers into mcp.toml text (pure; round-trips via tomllib). |
| `nanocodex/tools/mcp_store.py` | 92 | class | `McpStore` | Add / edit / enable / disable / remove MCP servers in mcp.toml. Changes are persisted immediately but only take effect on the next launch (the live connection is not hot-reloaded). |
| `nanocodex/tools/mcp_store.py` | 99 | function | `__init__` |  |
| `nanocodex/tools/mcp_store.py` | 102 | function | `list` | All configured servers (including disabled), as the connector sees them. |
| `nanocodex/tools/mcp_store.py` | 106 | function | `get` |  |
| `nanocodex/tools/mcp_store.py` | 112 | function | `_save` |  |
| `nanocodex/tools/mcp_store.py` | 116 | function | `add` | Create a server. Raises ValueError on bad name / dup / empty command. |
| `nanocodex/tools/mcp_store.py` | 144 | function | `remove` |  |
| `nanocodex/tools/mcp_store.py` | 152 | function | `set_enabled` |  |
| `nanocodex/tools/patch.py` | 1 | module | `patch` | apply_patch: Codex's V4A patch format. The format, as emitted by Codex-family models:: *** Begin Patch *** Add File: path/to/new.py +line one +line two *** Update File: path/to/exi |
| `nanocodex/tools/patch.py` | 37 | class | `PatchError` | Raised when a patch cannot be parsed or applied. |
| `nanocodex/tools/patch.py` | 41 | class | `ActionType` |  |
| `nanocodex/tools/patch.py` | 48 | class | `Chunk` | A contiguous change inside an Update hunk. |
| `nanocodex/tools/patch.py` | 59 | class | `FileAction` |  |
| `nanocodex/tools/patch.py` | 77 | function | `parse_patch` | Parse a V4A patch envelope into structured file actions. |
| `nanocodex/tools/patch.py` | 136 | function | `_parse_update_body` | Parse the hunk body of an Update File section. |
| `nanocodex/tools/patch.py` | 144 | function | `flush` |  |
| `nanocodex/tools/patch.py` | 190 | function | `_match_at` | Find *needle* in *haystack* at or after *start*; return index or -1. Three-level fallback: exact, rstrip, then full-strip equality. |
| `nanocodex/tools/patch.py` | 206 | function | `_apply_update` |  |
| `nanocodex/tools/patch.py` | 247 | class | `ApplyOutcome` | What changed, for reporting back to the model and CLI. |
| `nanocodex/tools/patch.py` | 255 | function | `summary` |  |
| `nanocodex/tools/patch.py` | 268 | function | `apply_patch` | Parse and apply a V4A patch under *root*. *can_write(path) -> bool* gates every file the patch would create, modify, move, or delete. The patch is staged fully in memory first; if  |
| `nanocodex/tools/patch.py` | 283 | function | `resolve` |  |
| `nanocodex/tools/read_file.py` | 1 | module | `read_file` | read_file: line-numbered reads, helpful for weaker models that don't grep well. |
| `nanocodex/tools/read_file.py` | 15 | class | `ReadFileTool` |  |
| `nanocodex/tools/read_file.py` | 21 | function | `name` |  |
| `nanocodex/tools/read_file.py` | 25 | function | `description` |  |
| `nanocodex/tools/read_file.py` | 33 | function | `parameters` |  |
| `nanocodex/tools/read_file.py` | 44 | function | `_resolve` |  |
| `nanocodex/tools/read_file.py` | 50 | async function | `execute` |  |
| `nanocodex/tools/record_fact.py` | 1 | module | `record_fact` | record_fact: a research worker writes a confirmed repo fact into state. Facts carry their source node so the orchestrator's fact_merge step can detect when two concurrent research  |
| `nanocodex/tools/record_fact.py` | 15 | class | `RecordFactTool` |  |
| `nanocodex/tools/record_fact.py` | 19 | function | `name` |  |
| `nanocodex/tools/record_fact.py` | 23 | function | `description` |  |
| `nanocodex/tools/record_fact.py` | 32 | function | `parameters` |  |
| `nanocodex/tools/record_fact.py` | 41 | async function | `execute` |  |
| `nanocodex/tools/remember_tool.py` | 1 | module | `remember_tool` | remember: let the model append a durable note to the user's memory file. A "memory" is a persistent, user-level fact or preference the user wants kept across every session and proj |
| `nanocodex/tools/remember_tool.py` | 21 | class | `RememberTool` |  |
| `nanocodex/tools/remember_tool.py` | 23 | function | `name` |  |
| `nanocodex/tools/remember_tool.py` | 27 | function | `description` |  |
| `nanocodex/tools/remember_tool.py` | 42 | function | `parameters` |  |
| `nanocodex/tools/remember_tool.py` | 58 | async function | `execute` |  |
| `nanocodex/tools/request_verification.py` | 1 | module | `request_verification` | request_verification: a worker hands its node off for independent review. This is the worker's ONLY legitimate path to "done": it cannot mark its own node done. Calling this flips  |
| `nanocodex/tools/request_verification.py` | 18 | class | `RequestVerificationTool` |  |
| `nanocodex/tools/request_verification.py` | 22 | function | `name` |  |
| `nanocodex/tools/request_verification.py` | 26 | function | `description` |  |
| `nanocodex/tools/request_verification.py` | 36 | function | `parameters` |  |
| `nanocodex/tools/request_verification.py` | 52 | async function | `execute` |  |
| `nanocodex/tools/schedule_tool.py` | 1 | module | `schedule_tool` | manage_schedule: let the agent create/list/cancel scheduled tasks in-chat. So the model can act on "every day at 9, run the tests" without the user needing to know the CLI. It wrap |
| `nanocodex/tools/schedule_tool.py` | 20 | class | `ManageScheduleTool` |  |
| `nanocodex/tools/schedule_tool.py` | 22 | function | `name` |  |
| `nanocodex/tools/schedule_tool.py` | 26 | function | `description` |  |
| `nanocodex/tools/schedule_tool.py` | 51 | function | `parameters` |  |
| `nanocodex/tools/schedule_tool.py` | 108 | async function | `execute` |  |
| `nanocodex/tools/schedule_tool.py` | 134 | function | `_add` |  |
| `nanocodex/tools/schedule_tool.py` | 170 | function | `_render_list` |  |
| `nanocodex/tools/shell.py` | 1 | module | `shell` | shell: run commands under the sandbox + approval state machine. This is the Codex `shell` / `local_shell` tool. The flow mirrors Codex: 1. Decide via the approval policy whether th |
| `nanocodex/tools/shell.py` | 22 | class | `ShellTool` |  |
| `nanocodex/tools/shell.py` | 30 | function | `name` |  |
| `nanocodex/tools/shell.py` | 34 | function | `description` |  |
| `nanocodex/tools/shell.py` | 43 | function | `parameters` |  |
| `nanocodex/tools/shell.py` | 72 | function | `_resolve_workdir` |  |
| `nanocodex/tools/shell.py` | 80 | function | `_needs_escalation` | Heuristic: does this command want something the sandbox forbids? |
| `nanocodex/tools/shell.py` | 93 | async function | `execute` |  |
| `nanocodex/tools/shell.py` | 174 | function | `_looks_read_only` | Best-effort: True only if EVERY chained segment is a known read-only command. Conservative on purpose — when in doubt it returns False so the action goes through the approval/escal |
| `nanocodex/tools/skills_tool.py` | 1 | module | `skills_tool` | manage_skills: let the agent install/list/remove/show reusable SKILL.md guides. A "skill" is a saved how-to guide (a SKILL.md under ~/.nanocodex/skills/<name>/) that the model can  |
| `nanocodex/tools/skills_tool.py` | 20 | class | `ManageSkillsTool` |  |
| `nanocodex/tools/skills_tool.py` | 22 | function | `name` |  |
| `nanocodex/tools/skills_tool.py` | 26 | function | `description` |  |
| `nanocodex/tools/skills_tool.py` | 43 | function | `parameters` |  |
| `nanocodex/tools/skills_tool.py` | 86 | async function | `execute` |  |
| `nanocodex/tools/skills_tool.py` | 108 | function | `_install` |  |
| `nanocodex/tools/skills_tool.py` | 131 | function | `_show` |  |
| `nanocodex/tools/skills_tool.py` | 145 | function | `_render_list` |  |
| `nanocodex/tools/storyboard_tool.py` | 1 | module | `storyboard_tool` | storyboard: turn story text + reference images into a storyboard (and video). Wraps the storyboard sub-package (nanocodex/storyboard) as an agent tool. Given a story and a few imag |
| `nanocodex/tools/storyboard_tool.py` | 28 | class | `StoryboardTool` |  |
| `nanocodex/tools/storyboard_tool.py` | 30 | function | `name` |  |
| `nanocodex/tools/storyboard_tool.py` | 34 | function | `description` |  |
| `nanocodex/tools/storyboard_tool.py` | 48 | function | `parameters` |  |
| `nanocodex/tools/storyboard_tool.py` | 77 | async function | `execute` |  |
| `nanocodex/tools/update_plan.py` | 1 | module | `update_plan` | update_plan: Codex's planning tool. The model maintains a short checklist of steps with statuses. nanocodex stores the latest plan on the shared :class:`ToolContext` so the CLI can |
| `nanocodex/tools/update_plan.py` | 17 | class | `UpdatePlanTool` |  |
| `nanocodex/tools/update_plan.py` | 21 | function | `name` |  |
| `nanocodex/tools/update_plan.py` | 25 | function | `description` |  |
| `nanocodex/tools/update_plan.py` | 35 | function | `parameters` |  |
| `nanocodex/tools/update_plan.py` | 62 | async function | `execute` |  |
| `nanocodex/tools/update_plan.py` | 91 | function | `render_plan` |  |
| `nanocodex/tools/web_search.py` | 1 | module | `web_search` | web_search: DuckDuckGo search, gated by the sandbox network policy. Network access mirrors Codex's sandbox semantics: * ``danger-full-access`` -> network on -> search runs without  |
| `nanocodex/tools/web_search.py` | 25 | function | `_default_search` | Run a real DuckDuckGo search via ddgs (lazy import; network required). |
| `nanocodex/tools/web_search.py` | 33 | class | `WebSearchTool` |  |
| `nanocodex/tools/web_search.py` | 36 | function | `__init__` |  |
| `nanocodex/tools/web_search.py` | 41 | function | `name` |  |
| `nanocodex/tools/web_search.py` | 45 | function | `description` |  |
| `nanocodex/tools/web_search.py` | 54 | function | `parameters` |  |
| `nanocodex/tools/web_search.py` | 69 | async function | `execute` |  |
| `nanocodex/tools/write_checkpoint.py` | 1 | module | `write_checkpoint` | write_checkpoint: a worker records a durable snapshot of what it just did. The checkpoint is the evidence index the verifier reads from and the source of truth recovery reconciles  |
| `nanocodex/tools/write_checkpoint.py` | 17 | class | `WriteCheckpointTool` |  |
| `nanocodex/tools/write_checkpoint.py` | 21 | function | `name` |  |
| `nanocodex/tools/write_checkpoint.py` | 25 | function | `description` |  |
| `nanocodex/tools/write_checkpoint.py` | 35 | function | `parameters` |  |
| `nanocodex/tools/write_checkpoint.py` | 56 | async function | `execute` |  |
## Rust

| 路径 | 行 | 类型 | 名称 | 摘要 |
| --- | ---: | --- | --- | --- |
| `rust/crates/ncx-cli/src/args.rs` | 1 | module | `args` |  |
| `rust/crates/ncx-cli/src/args.rs` | 5 | const | `USAGE` |  |
| `rust/crates/ncx-cli/src/args.rs` | 54 | struct | `Args` |  |
| `rust/crates/ncx-cli/src/args.rs` | 88 | fn | `parse_args` |  |
| `rust/crates/ncx-cli/src/args.rs` | 149 | fn | `take_value` |  |
| `rust/crates/ncx-cli/src/args.rs` | 156 | fn | `take_i64` |  |
| `rust/crates/ncx-cli/src/args.rs` | 166 | fn | `args` |  |
| `rust/crates/ncx-cli/src/args.rs` | 172 | fn | `empty_is_repl_mode` |  |
| `rust/crates/ncx-cli/src/args.rs` | 179 | fn | `help_and_version_flags` |  |
| `rust/crates/ncx-cli/src/args.rs` | 186 | fn | `options_with_values` |  |
| `rust/crates/ncx-cli/src/args.rs` | 211 | fn | `positional_becomes_prompt` |  |
| `rust/crates/ncx-cli/src/args.rs` | 217 | fn | `flags_then_prompt` |  |
| `rust/crates/ncx-cli/src/args.rs` | 224 | fn | `double_dash_forces_positional` |  |
| `rust/crates/ncx-cli/src/args.rs` | 231 | fn | `missing_value_errors` |  |
| `rust/crates/ncx-cli/src/args.rs` | 236 | fn | `numeric_flags_validate_integer_values` |  |
| `rust/crates/ncx-cli/src/args.rs` | 244 | fn | `resume_and_history_flags` |  |
| `rust/crates/ncx-cli/src/args.rs` | 252 | fn | `unknown_flag_errors` |  |
| `rust/crates/ncx-cli/src/args.rs` | 257 | fn | `dump_genome_flag` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1 | module | `main` |  |
| `rust/crates/ncx-cli/src/main.rs` | 37 | const | `SYSTEM_PROMPT` |  |
| `rust/crates/ncx-cli/src/main.rs` | 43 | const | `PLAN_MODE_NOTE` |  |
| `rust/crates/ncx-cli/src/main.rs` | 47 | fn | `main` |  |
| `rust/crates/ncx-cli/src/main.rs` | 76 | fn | `run` |  |
| `rust/crates/ncx-cli/src/main.rs` | 277 | fn | `run_orchestrated` |  |
| `rust/crates/ncx-cli/src/main.rs` | 309 | fn | `dump_genome_toml` |  |
| `rust/crates/ncx-cli/src/main.rs` | 330 | fn | `toml_escape` |  |
| `rust/crates/ncx-cli/src/main.rs` | 347 | fn | `repl` |  |
| `rust/crates/ncx-cli/src/main.rs` | 393 | fn | `run_one_turn` |  |
| `rust/crates/ncx-cli/src/main.rs` | 421 | fn | `split_inline_images` |  |
| `rust/crates/ncx-cli/src/main.rs` | 436 | enum | `SlashOutcome` |  |
| `rust/crates/ncx-cli/src/main.rs` | 446 | fn | `dispatch_slash` |  |
| `rust/crates/ncx-cli/src/main.rs` | 523 | fn | `reload_mcp_tools` |  |
| `rust/crates/ncx-cli/src/main.rs` | 551 | fn | `prepare_configured_mcp_tools` |  |
| `rust/crates/ncx-cli/src/main.rs` | 565 | fn | `render_skills` |  |
| `rust/crates/ncx-cli/src/main.rs` | 582 | fn | `render_help` |  |
| `rust/crates/ncx-cli/src/main.rs` | 590 | fn | `render_help_for_workspace` |  |
| `rust/crates/ncx-cli/src/main.rs` | 608 | fn | `render_status` |  |
| `rust/crates/ncx-cli/src/main.rs` | 642 | fn | `export_session_text` |  |
| `rust/crates/ncx-cli/src/main.rs` | 688 | fn | `export_target_path` |  |
| `rust/crates/ncx-cli/src/main.rs` | 710 | fn | `render_session_markdown` |  |
| `rust/crates/ncx-cli/src/main.rs` | 781 | fn | `push_block` |  |
| `rust/crates/ncx-cli/src/main.rs` | 791 | fn | `code_fence` |  |
| `rust/crates/ncx-cli/src/main.rs` | 807 | fn | `push_fenced` |  |
| `rust/crates/ncx-cli/src/main.rs` | 819 | fn | `count_roles` |  |
| `rust/crates/ncx-cli/src/main.rs` | 836 | fn | `content_to_markdown` |  |
| `rust/crates/ncx-cli/src/main.rs` | 859 | fn | `scope_suffix` |  |
| `rust/crates/ncx-cli/src/main.rs` | 868 | fn | `review_prompt` |  |
| `rust/crates/ncx-cli/src/main.rs` | 879 | fn | `security_review_prompt` |  |
| `rust/crates/ncx-cli/src/main.rs` | 890 | fn | `verify_prompt` |  |
| `rust/crates/ncx-cli/src/main.rs` | 908 | fn | `doc_backend_hint` |  |
| `rust/crates/ncx-cli/src/main.rs` | 918 | fn | `doc_prompt` |  |
| `rust/crates/ncx-cli/src/main.rs` | 935 | fn | `config_text` |  |
| `rust/crates/ncx-cli/src/main.rs` | 940 | fn | `config_text_at` |  |
| `rust/crates/ncx-cli/src/main.rs` | 975 | fn | `render_config_overview` |  |
| `rust/crates/ncx-cli/src/main.rs` | 989 | fn | `parse_config_assignment` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1009 | struct | `UsageTracker` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1017 | struct | `TurnUsage` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1025 | fn | `record` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1036 | fn | `render` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1058 | fn | `add_usage` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1064 | fn | `format_usage_block` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1092 | fn | `usage_value` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1096 | struct | `SessionRecorder` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1105 | fn | `new` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1113 | fn | `session_id` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1117 | fn | `record` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1124 | fn | `session_log_path` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1132 | fn | `emit_usage_line` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1140 | fn | `render_history` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1164 | fn | `compact_session_text` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1176 | fn | `checkpoint_before_turn` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1189 | fn | `create_checkpoint_text` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1201 | fn | `restore_checkpoint_text` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1220 | fn | `format_checkpoint_saved` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1231 | fn | `render_checkpoints` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1253 | fn | `clipped_label` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1265 | fn | `runtime_profile_for_args` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1280 | fn | `build_image_user_input` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1295 | fn | `image_mime` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1311 | fn | `base64_encode` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1312 | const | `T` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1340 | fn | `help_lists_all_commands` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1348 | fn | `base64_matches_known_vectors` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1360 | fn | `image_input_builds_multimodal_content` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1384 | fn | `inline_images_split_from_prompt` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1397 | fn | `vision_provider_only_built_when_vl_model_set` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1407 | fn | `cli_and_gui_use_equivalent_runtime_profiles_for_same_config` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1426 | fn | `help_lists_custom_project_commands` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1439 | fn | `parse_config_assignment_accepts_trimmed_key_value` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1450 | fn | `usage_tracker_renders_last_and_total_usage` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1490 | fn | `config_text_writes_known_key_to_path` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1502 | fn | `config_text_rejects_unknown_key` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1513 | fn | `status_masks_api_key` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1524 | fn | `history_renders_saved_sessions` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1547 | fn | `checkpoints_render_saved_entries` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1563 | fn | `export_renders_user_assistant_tool_markdown` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1594 | fn | `export_flattens_multimodal_and_hides_image_data` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1611 | fn | `export_writes_markdown_file_to_explicit_path` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1631 | fn | `export_refuses_to_overwrite_existing_explicit_file` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1652 | fn | `export_refuses_directory_arg_with_clear_message` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1669 | fn | `export_default_path_uses_session_id_under_exports` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1681 | fn | `export_uses_longer_fence_when_content_has_backticks` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1698 | fn | `review_verify_prompts_reference_diff_and_scope` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1713 | fn | `doc_prompts_name_format_file_and_backend` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 1 | module | `runner` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 27 | struct | `LiveRunner` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 37 | fn | `new` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 47 | fn | `model_for` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 67 | fn | `run_in` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 103 | fn | `scratch_dir` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 112 | fn | `run` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 116 | fn | `reason` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 123 | fn | `run_worker` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 145 | fn | `promote_worker` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 160 | fn | `compose_system_prompt` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 170 | struct | `LiveSummarizer` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 176 | fn | `new` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 179 | fn | `fast_model` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 190 | fn | `merge` |  |
| `rust/crates/ncx-config/src/config.rs` | 1 | module | `config` |  |
| `rust/crates/ncx-config/src/config.rs` | 6 | const | `DEFAULT_BASE_URL` |  |
| `rust/crates/ncx-config/src/config.rs` | 8 | const | `DEFAULT_MODEL` |  |
| `rust/crates/ncx-config/src/config.rs` | 9 | const | `DEFAULT_MODELS` |  |
| `rust/crates/ncx-config/src/config.rs` | 10 | const | `VALID_SANDBOX_MODES` |  |
| `rust/crates/ncx-config/src/config.rs` | 12 | const | `VALID_APPROVAL_POLICIES` |  |
| `rust/crates/ncx-config/src/config.rs` | 13 | const | `VALID_HOOK_EVENTS` |  |
| `rust/crates/ncx-config/src/config.rs` | 16 | const | `VALID_PERMISSION_MODES` |  |
| `rust/crates/ncx-config/src/config.rs` | 21 | fn | `permission_mode_to_knobs` |  |
| `rust/crates/ncx-config/src/config.rs` | 32 | fn | `derive_permission_mode` |  |
| `rust/crates/ncx-config/src/config.rs` | 42 | struct | `McpServerConfig` |  |
| `rust/crates/ncx-config/src/config.rs` | 52 | struct | `HookConfig` |  |
| `rust/crates/ncx-config/src/config.rs` | 65 | struct | `Config` |  |
| `rust/crates/ncx-config/src/config.rs` | 114 | fn | `default` |  |
| `rust/crates/ncx-config/src/config.rs` | 155 | fn | `validate` |  |
| `rust/crates/ncx-config/src/config.rs` | 209 | fn | `redacted` |  |
| `rust/crates/ncx-config/src/config.rs` | 265 | struct | `ConfigError` |  |
| `rust/crates/ncx-config/src/config.rs` | 268 | fn | `fmt` |  |
| `rust/crates/ncx-config/src/config.rs` | 280 | fn | `permission_mode_maps_to_knobs` |  |
| `rust/crates/ncx-config/src/config.rs` | 305 | fn | `derive_permission_mode_migrates_legacy_sandbox` |  |
| `rust/crates/ncx-config/src/config.rs` | 312 | fn | `default_permission_mode_is_valid` |  |
| `rust/crates/ncx-config/src/config.rs` | 325 | fn | `parallel_tool_limit_must_be_bounded` |  |
| `rust/crates/ncx-config/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1 | module | `loader` |  |
| `rust/crates/ncx-config/src/loader.rs` | 17 | type | `Table` |  |
| `rust/crates/ncx-config/src/loader.rs` | 21 | fn | `home_dir` |  |
| `rust/crates/ncx-config/src/loader.rs` | 31 | struct | `ConfigPaths` |  |
| `rust/crates/ncx-config/src/loader.rs` | 38 | fn | `default` |  |
| `rust/crates/ncx-config/src/loader.rs` | 52 | struct | `Overrides` |  |
| `rust/crates/ncx-config/src/loader.rs` | 80 | fn | `load_toml` |  |
| `rust/crates/ncx-config/src/loader.rs` | 94 | fn | `str_val` |  |
| `rust/crates/ncx-config/src/loader.rs` | 103 | fn | `to_string_val` |  |
| `rust/crates/ncx-config/src/loader.rs` | 115 | fn | `deepseek_values` |  |
| `rust/crates/ncx-config/src/loader.rs` | 149 | fn | `nanocodex_values` |  |
| `rust/crates/ncx-config/src/loader.rs` | 187 | fn | `codex_values` |  |
| `rust/crates/ncx-config/src/loader.rs` | 205 | const | `PROFILE_KEYS` |  |
| `rust/crates/ncx-config/src/loader.rs` | 227 | fn | `profile_values` |  |
| `rust/crates/ncx-config/src/loader.rs` | 239 | fn | `as_int` |  |
| `rust/crates/ncx-config/src/loader.rs` | 243 | fn | `as_float` |  |
| `rust/crates/ncx-config/src/loader.rs` | 248 | fn | `as_bool` |  |
| `rust/crates/ncx-config/src/loader.rs` | 256 | fn | `selected_scalar` |  |
| `rust/crates/ncx-config/src/loader.rs` | 260 | fn | `parse_hooks` |  |
| `rust/crates/ncx-config/src/loader.rs` | 287 | fn | `normalize_hook_event` |  |
| `rust/crates/ncx-config/src/loader.rs` | 299 | fn | `model_list` |  |
| `rust/crates/ncx-config/src/loader.rs` | 331 | fn | `list_profiles_at` |  |
| `rust/crates/ncx-config/src/loader.rs` | 342 | fn | `list_profiles` |  |
| `rust/crates/ncx-config/src/loader.rs` | 358 | fn | `load_mcp_servers_at` |  |
| `rust/crates/ncx-config/src/loader.rs` | 416 | fn | `load_mcp_servers` |  |
| `rust/crates/ncx-config/src/loader.rs` | 421 | fn | `load_config` |  |
| `rust/crates/ncx-config/src/loader.rs` | 427 | fn | `load_config_with_paths` |  |
| `rust/crates/ncx-config/src/loader.rs` | 436 | fn | `load_config_impl` |  |
| `rust/crates/ncx-config/src/loader.rs` | 677 | fn | `empty_env` |  |
| `rust/crates/ncx-config/src/loader.rs` | 681 | fn | `env1` |  |
| `rust/crates/ncx-config/src/loader.rs` | 687 | fn | `write` |  |
| `rust/crates/ncx-config/src/loader.rs` | 694 | fn | `no_paths` |  |
| `rust/crates/ncx-config/src/loader.rs` | 704 | fn | `config_redacts_api_key` |  |
| `rust/crates/ncx-config/src/loader.rs` | 717 | fn | `validate_rejects_bad_sandbox_mode` |  |
| `rust/crates/ncx-config/src/loader.rs` | 728 | fn | `validate_rejects_missing_key` |  |
| `rust/crates/ncx-config/src/loader.rs` | 735 | fn | `compaction_defaults_on_with_1m_window` |  |
| `rust/crates/ncx-config/src/loader.rs` | 743 | fn | `load_reads_deepseek_file` |  |
| `rust/crates/ncx-config/src/loader.rs` | 778 | fn | `overrides_win_over_file` |  |
| `rust/crates/ncx-config/src/loader.rs` | 803 | fn | `deepseek_nested_provider_key` |  |
| `rust/crates/ncx-config/src/loader.rs` | 829 | fn | `max_iterations_default_and_override` |  |
| `rust/crates/ncx-config/src/loader.rs` | 859 | fn | `max_iterations_from_env` |  |
| `rust/crates/ncx-config/src/loader.rs` | 875 | fn | `runtime_budget_and_context_edit_fields_load_from_file_env_and_overrides` |  |
| `rust/crates/ncx-config/src/loader.rs` | 933 | fn | `hooks_load_from_nanocodex_file` |  |
| `rust/crates/ncx-config/src/loader.rs` | 975 | fn | `hook_event_aliases_are_normalized` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1014 | fn | `hook_missing_command_fails_validation` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1049 | fn | `nanocodex_file_wins_over_deepseek` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1078 | fn | `env_wins_over_nanocodex_file` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1101 | fn | `max_retries_default_and_env` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1146 | fn | `profile_overrides_base_but_below_env` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1199 | fn | `profile_name_from_env_and_unknown_raises` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1239 | fn | `list_profiles_returns_sorted_names` |  |
| `rust/crates/ncx-config/src/writer.rs` | 1 | module | `writer` |  |
| `rust/crates/ncx-config/src/writer.rs` | 11 | const | `WRITABLE_KEYS` |  |
| `rust/crates/ncx-config/src/writer.rs` | 36 | fn | `esc_toml` |  |
| `rust/crates/ncx-config/src/writer.rs` | 50 | fn | `dump_nanocodex_toml` |  |
| `rust/crates/ncx-config/src/writer.rs` | 77 | fn | `write_nanocodex_config` |  |
| `rust/crates/ncx-config/src/writer.rs` | 120 | fn | `map` |  |
| `rust/crates/ncx-config/src/writer.rs` | 126 | fn | `dump_round_trips_quoted_value` |  |
| `rust/crates/ncx-config/src/writer.rs` | 144 | fn | `dump_skips_empty_and_unknown` |  |
| `rust/crates/ncx-config/src/writer.rs` | 157 | fn | `write_creates_and_merges` |  |
| `rust/crates/ncx-config/src/writer.rs` | 185 | fn | `write_ignores_unknown_keys` |  |
| `rust/crates/ncx-config/src/writer.rs` | 200 | fn | `write_persists_runtime_control_keys` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 1 | module | `live_agent_soak` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 18 | const | `CONCURRENCY` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 20 | const | `DEFAULT_DURATION_SECS` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 21 | const | `PROGRESS_INTERVAL_SECS` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 22 | const | `MAX_OUTPUT_TOKENS` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 23 | const | `CONSECUTIVE_ERROR_LIMIT` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 24 | const | `ERROR_RATE_MIN_SAMPLES` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 25 | const | `ERROR_RATE_LIMIT` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 26 | const | `DEFAULT_TOKEN_BUDGET` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 29 | struct | `CappedProvider` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 35 | fn | `model` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 38 | fn | `chat` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 64 | struct | `Metrics` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 82 | fn | `record_start` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 87 | fn | `record_result` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 110 | fn | `total_tokens` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 114 | fn | `apply_circuit_breakers` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 142 | fn | `main` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 188 | fn | `run_worker` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 209 | fn | `run_request` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 221 | fn | `report_progress` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 242 | fn | `print_summary` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 279 | fn | `percentile` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 287 | struct | `Config` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 298 | fn | `from_process` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 327 | fn | `duration_arg` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 355 | fn | `percentile_uses_nearest_rank` |  |
| `rust/crates/ncx-core/examples/live_agent_soak.rs` | 362 | fn | `duration_defaults_and_parses_override` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 1 | module | `live_agent_tool_soak` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 19 | const | `CONCURRENCY` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 21 | const | `DEFAULT_DURATION_SECS` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 22 | const | `PROGRESS_INTERVAL_SECS` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 23 | const | `MAX_OUTPUT_TOKENS` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 24 | const | `DEFAULT_TOKEN_BUDGET` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 25 | const | `CONSECUTIVE_ERROR_LIMIT` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 26 | const | `CONTRACT_FAILURE_MIN_SAMPLES` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 27 | const | `CONTRACT_FAILURE_RATE_LIMIT` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 28 | const | `MIN_WORKLOAD_SUCCESS_RATE` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 31 | struct | `CappedProvider` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 35 | fn | `model` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 38 | fn | `chat` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 64 | struct | `ProbeState` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 72 | struct | `ReadProbe` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 77 | fn | `name` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 80 | fn | `description` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 84 | fn | `parameters` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 92 | fn | `read_only` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 96 | fn | `execute` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 112 | struct | `SerialProbe` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 117 | fn | `name` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 120 | fn | `description` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 124 | fn | `parameters` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 132 | fn | `execute` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 141 | struct | `RequestOutcome` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 148 | fn | `contract_ok` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 159 | struct | `Metrics` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 183 | fn | `record_start` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 188 | fn | `record_result` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 239 | fn | `record_provider_error` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 247 | fn | `total_tokens` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 251 | fn | `workload_success_rate` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 258 | fn | `apply_circuit_breakers` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 276 | fn | `main` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 322 | fn | `run_worker` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 342 | fn | `run_request` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 371 | fn | `report_progress` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 388 | fn | `print_summary` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 429 | fn | `percentile` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 436 | struct | `Config` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 447 | fn | `from_process` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 473 | fn | `duration_arg` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 501 | fn | `contract_requires_parallel_reads_and_middle_barrier` |  |
| `rust/crates/ncx-core/examples/live_agent_tool_soak.rs` | 529 | fn | `duration_defaults_to_thirty_minutes` |  |
| `rust/crates/ncx-core/examples/live_lsp.rs` | 1 | module | `live_lsp` |  |
| `rust/crates/ncx-core/examples/live_lsp.rs` | 6 | fn | `main` |  |
| `rust/crates/ncx-core/examples/live_web_tools.rs` | 1 | module | `live_web_tools` |  |
| `rust/crates/ncx-core/examples/live_web_tools.rs` | 13 | fn | `main` |  |
| `rust/crates/ncx-core/examples/live_web_tools.rs` | 29 | fn | `report` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 1 | module | `agent_loop` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 20 | const | `DEFAULT_MAX_PARALLEL_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 28 | struct | `TurnResult` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 37 | struct | `TaskBudget` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 45 | fn | `default` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 56 | enum | `LoopEvent` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 70 | type | `EventSink` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 71 | fn | `emit` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 79 | struct | `AgentLoop` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 96 | fn | `new` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 113 | fn | `with_max_iterations` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 119 | fn | `with_task_budget` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 135 | fn | `with_max_parallel_tool_calls` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 142 | fn | `with_tool_scheduler` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 148 | fn | `replace_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 153 | fn | `provider_model` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 158 | fn | `runtime_profile` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 174 | fn | `register_context_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 182 | fn | `unregister_context_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 185 | fn | `with_context_edit` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 193 | fn | `with_vision_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 200 | fn | `set_event_sink` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 203 | fn | `active_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 215 | fn | `call_model` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 237 | fn | `run_turn` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 251 | fn | `apply_stop_hook` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 282 | fn | `dump_args` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 15 | struct | `ScriptedProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 21 | fn | `new` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 30 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 33 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 46 | fn | `tmpdir` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 53 | fn | `build` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 61 | fn | `build_with_hooks` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 73 | fn | `tc` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 81 | fn | `assistant_toolcall` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 92 | fn | `returns_final_text_without_tools` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 106 | fn | `executes_apply_patch_then_finishes` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 129 | fn | `emits_events_for_tool_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 157 | fn | `persists_reasoning_on_tool_call_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 184 | fn | `runs_update_plan_and_records_state` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 213 | fn | `stops_at_max_iterations` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 230 | struct | `CapturingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 236 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 239 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 247 | struct | `CountingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 251 | struct | `StaticContextProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 256 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 259 | fn | `provide` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 266 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 269 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 279 | fn | `memory_recall_is_sent_as_query_scoped_system_note` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 332 | fn | `registered_context_provider_is_query_scoped_and_reversible` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 361 | fn | `primary_provider_can_be_replaced_without_rebuilding_runtime_state` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 383 | fn | `task_budget_is_visible_to_model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 408 | fn | `user_prompt_hook_can_block_model_call` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 432 | fn | `user_prompt_hook_output_is_sent_as_system_note` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 456 | fn | `stop_hook_output_is_appended_to_final_text` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 482 | fn | `tool_budget_stops_and_backfills_unanswered_calls` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 506 | fn | `answered` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 528 | fn | `cancel_mid_tool_loop_backfills_tool_results` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 555 | fn | `image_turn_routes_to_vision_provider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 577 | fn | `read_only_calls_run_concurrently` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 578 | struct | `SlowReadTool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 581 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 584 | fn | `description` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 587 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 590 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 593 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 631 | struct | `RecordingScheduler` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 639 | fn | `execute_one` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 648 | fn | `execute_read_only_batch` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 666 | fn | `custom_scheduler_receives_read_batches_and_serial_barriers` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 700 | fn | `write_between_reads_stays_serial_and_ordered` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 730 | fn | `stop_interrupts_a_hanging_tool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 731 | struct | `HangingTool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 734 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 737 | fn | `description` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 740 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 743 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 775 | fn | `stop_interrupts_a_hanging_model_request` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 776 | struct | `HangingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 779 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 782 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 1 | module | `tool_dispatch` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 6 | enum | `DispatchStop` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 11 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 50 | fn | `starts_parallel_run` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 56 | fn | `execute_read_batch` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 89 | fn | `execute_serial` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 105 | fn | `record_tool_start` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 116 | fn | `record_tool_result` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 132 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 1 | module | `trace` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 6 | fn | `enabled` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 12 | fn | `model_response` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 36 | fn | `tool_result` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 46 | fn | `truncate` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 1 | module | `turn` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 13 | const | `MEMORY_RECALL_MAX_ENTRIES` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 15 | const | `MEMORY_RECALL_MAX_CHARS` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 18 | struct | `TurnState` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 24 | fn | `finish` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 34 | struct | `PromptContext` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 39 | fn | `run` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 89 | fn | `request_model_cancellable` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 111 | fn | `cancelled_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 117 | fn | `prepare_prompt` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 159 | fn | `request_model` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 175 | fn | `finish_response` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 203 | fn | `persist_tool_calls` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 220 | fn | `stop_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 242 | fn | `tool_budget_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 257 | fn | `model_budget_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 270 | fn | `budget_note` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 281 | fn | `has_image_block` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 289 | fn | `user_query_text` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 304 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 308 | fn | `add_usage` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 314 | fn | `memory_recall_notes` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 1 | module | `checkpoint` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 15 | const | `MAX_FILES` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 17 | const | `MAX_TOTAL_BYTES` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 18 | const | `MAX_FILE_BYTES` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 19 | static | `CHECKPOINT_SEQ` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 23 | struct | `CheckpointMeta` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 33 | struct | `RestoreReport` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 39 | struct | `CheckpointStore` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 46 | fn | `new` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 51 | fn | `create` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 105 | fn | `list` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 117 | fn | `get` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 122 | fn | `restore` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 173 | fn | `list_workspace_files` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 179 | fn | `walk` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 203 | fn | `should_exclude` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 234 | fn | `write_meta` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 248 | fn | `read_meta` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 265 | fn | `string_field` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 273 | fn | `string_array` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 287 | fn | `rel_to_key` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 297 | fn | `key_to_path` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 311 | fn | `safe_checkpoint_id` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 323 | fn | `inside_workspace` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 343 | fn | `new_checkpoint_id` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 348 | fn | `now_stamp` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 359 | fn | `tmp_ws` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 367 | fn | `create_and_restore_reverts_modified_and_new_files` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 391 | fn | `list_returns_newest_first` |  |
| `rust/crates/ncx-core/src/checkpoint.rs` | 406 | fn | `checkpoint_paths_reject_traversal` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 1 | module | `custom_commands` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 6 | struct | `CustomCommandSummary` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 13 | struct | `CustomCommandQuery` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 17 | fn | `custom_command_prompt` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 41 | fn | `parse_custom_command_query` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 65 | fn | `resolve_custom_command` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 86 | fn | `list_custom_commands` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 123 | struct | `CustomCommandRoot` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 128 | fn | `custom_command_roots` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 152 | fn | `home_dir` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 158 | fn | `valid_custom_command_name` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 165 | fn | `strip_frontmatter` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 182 | fn | `expand_custom_command_template` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 201 | fn | `split_custom_args` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 230 | fn | `parses_scoped_custom_command_queries` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 239 | fn | `expands_custom_command_arguments` |  |
| `rust/crates/ncx-core/src/custom_commands.rs` | 254 | fn | `custom_command_prompt_strips_frontmatter_and_prefers_project` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 1 | module | `editor_tool` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 11 | struct | `StrReplaceEditorTool` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 15 | fn | `name` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 18 | fn | `description` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 24 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 38 | fn | `execute` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 55 | fn | `create` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 66 | fn | `replace` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 88 | fn | `insert` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 115 | fn | `read_utf8` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 125 | fn | `resolve` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 135 | fn | `add_patch` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 144 | fn | `update_patch` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 158 | fn | `apply` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 168 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 180 | fn | `replaces_unique_text_through_patch_boundary` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 194 | fn | `refuses_ambiguous_replacement_without_writing` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 209 | fn | `creates_and_inserts_text_through_patch_boundary` |  |
| `rust/crates/ncx-core/src/editor_tool.rs` | 235 | fn | `plan_mode_refuses_editor_mutations` |  |
| `rust/crates/ncx-core/src/genome.rs` | 1 | module | `genome` |  |
| `rust/crates/ncx-core/src/genome.rs` | 30 | struct | `Genome` |  |
| `rust/crates/ncx-core/src/genome.rs` | 43 | fn | `from_env` |  |
| `rust/crates/ncx-core/src/genome.rs` | 51 | fn | `load` |  |
| `rust/crates/ncx-core/src/genome.rs` | 58 | fn | `parse` |  |
| `rust/crates/ncx-core/src/genome.rs` | 83 | fn | `is_empty` |  |
| `rust/crates/ncx-core/src/genome.rs` | 88 | fn | `base_system_prompt` |  |
| `rust/crates/ncx-core/src/genome.rs` | 94 | fn | `describe` |  |
| `rust/crates/ncx-core/src/genome.rs` | 107 | fn | `empty_genome_is_a_noop` |  |
| `rust/crates/ncx-core/src/genome.rs` | 115 | fn | `parses_system_prompt_and_tool_desc` |  |
| `rust/crates/ncx-core/src/genome.rs` | 136 | fn | `blank_overrides_are_rejected` |  |
| `rust/crates/ncx-core/src/genome.rs` | 145 | fn | `malformed_toml_is_an_error` |  |
| `rust/crates/ncx-core/src/genome.rs` | 150 | fn | `multiline_triple_quoted_prompt` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 1 | module | `hooks` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 15 | enum | `HookEvent` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 23 | fn | `as_str` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 34 | struct | `HookOutcome` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 38 | fn | `run_matching_hooks` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 67 | fn | `matches_tool` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 79 | fn | `run_one_hook` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 104 | fn | `render_hook_result` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 143 | fn | `matcher_supports_exact_wildcard_and_lists` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 1 | module | `isolate` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 15 | const | `SKIP_DIRS` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 27 | fn | `copy_tree` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 59 | fn | `tmp` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 67 | fn | `copies_files_and_skips_ignored` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 87 | fn | `isolated_copy_is_independent` |  |
| `rust/crates/ncx-core/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 1 | module | `lsp_tool` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 9 | struct | `LspRequest` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 18 | trait | `LspProvider` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 19 | fn | `request` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 21 | struct | `LspTool` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 26 | fn | `name` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 29 | fn | `description` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 33 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 51 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 55 | fn | `execute` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 73 | fn | `parse_request` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 100 | fn | `string_arg` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 109 | fn | `integer_arg` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 119 | fn | `validate_required_fields` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 137 | fn | `error_response` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 151 | struct | `MockProvider` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 158 | fn | `request` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 163 | fn | `context` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 172 | fn | `reports_unavailable_without_a_provider` |  |
| `rust/crates/ncx-core/src/lsp_tool.rs` | 186 | fn | `delegates_valid_requests_to_the_provider` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 1 | module | `mcp_tool` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 23 | struct | `McpTool` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 31 | fn | `new` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 42 | fn | `is_read_only_name` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 54 | fn | `name` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 57 | fn | `description` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 61 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 65 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 69 | fn | `execute` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 124 | fn | `prepare_mcp_server_tools` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 141 | fn | `register_mcp_server` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 163 | fn | `read_only_heuristic` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 180 | fn | `write_mock_server` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 213 | fn | `register_and_execute_echo` |  |
| `rust/crates/ncx-core/src/memory.rs` | 1 | module | `memory` |  |
| `rust/crates/ncx-core/src/memory.rs` | 26 | trait | `Summarizer` |  |
| `rust/crates/ncx-core/src/memory.rs` | 27 | fn | `merge` |  |
| `rust/crates/ncx-core/src/memory.rs` | 31 | const | `MAX_ENTRIES` |  |
| `rust/crates/ncx-core/src/memory.rs` | 32 | const | `RECALL_HEADER` |  |
| `rust/crates/ncx-core/src/memory.rs` | 36 | struct | `MemoryEntry` |  |
| `rust/crates/ncx-core/src/memory.rs` | 45 | struct | `MemoryStore` |  |
| `rust/crates/ncx-core/src/memory.rs` | 51 | fn | `new` |  |
| `rust/crates/ncx-core/src/memory.rs` | 60 | fn | `remember` |  |
| `rust/crates/ncx-core/src/memory.rs` | 86 | fn | `entries` |  |
| `rust/crates/ncx-core/src/memory.rs` | 96 | fn | `recall` |  |
| `rust/crates/ncx-core/src/memory.rs` | 141 | fn | `consolidate` |  |
| `rust/crates/ncx-core/src/memory.rs` | 179 | fn | `summarize_consolidate` |  |
| `rust/crates/ncx-core/src/memory.rs` | 254 | fn | `write_all` |  |
| `rust/crates/ncx-core/src/memory.rs` | 273 | fn | `keywords` |  |
| `rust/crates/ncx-core/src/memory.rs` | 282 | fn | `expanded_keywords` |  |
| `rust/crates/ncx-core/src/memory.rs` | 294 | fn | `semantic_aliases` |  |
| `rust/crates/ncx-core/src/memory.rs` | 311 | fn | `semantic_score` |  |
| `rust/crates/ncx-core/src/memory.rs` | 342 | fn | `normalize` |  |
| `rust/crates/ncx-core/src/memory.rs` | 351 | fn | `word_set` |  |
| `rust/crates/ncx-core/src/memory.rs` | 358 | fn | `phrases` |  |
| `rust/crates/ncx-core/src/memory.rs` | 372 | fn | `jaccard` |  |
| `rust/crates/ncx-core/src/memory.rs` | 384 | fn | `parse_entries` |  |
| `rust/crates/ncx-core/src/memory.rs` | 441 | fn | `store` |  |
| `rust/crates/ncx-core/src/memory.rs` | 449 | fn | `remember_then_round_trips` |  |
| `rust/crates/ncx-core/src/memory.rs` | 466 | fn | `dedup_skips_identical` |  |
| `rust/crates/ncx-core/src/memory.rs` | 475 | fn | `empty_is_not_stored` |  |
| `rust/crates/ncx-core/src/memory.rs` | 482 | fn | `cap_drops_oldest` |  |
| `rust/crates/ncx-core/src/memory.rs` | 494 | fn | `recall_scores_by_keyword_overlap` |  |
| `rust/crates/ncx-core/src/memory.rs` | 518 | fn | `recall_uses_semantic_aliases_and_tags` |  |
| `rust/crates/ncx-core/src/memory.rs` | 534 | fn | `recall_empty_store_is_blank` |  |
| `rust/crates/ncx-core/src/memory.rs` | 540 | fn | `consolidate_merges_near_duplicates` |  |
| `rust/crates/ncx-core/src/memory.rs` | 557 | struct | `FixedMerger` |  |
| `rust/crates/ncx-core/src/memory.rs` | 561 | fn | `merge` |  |
| `rust/crates/ncx-core/src/memory.rs` | 571 | fn | `summarize_merges_cluster_into_one` |  |
| `rust/crates/ncx-core/src/memory.rs` | 603 | fn | `summarize_falls_back_to_newest_when_merge_none` |  |
| `rust/crates/ncx-core/src/memory.rs` | 621 | fn | `consolidate_is_idempotent` |  |
| `rust/crates/ncx-core/src/memory.rs` | 633 | fn | `recall_respects_entry_cap` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 1 | module | `mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 9 | const | `TRIM_TRAILING` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 11 | const | `MAX_FILE_BYTES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 12 | const | `MAX_FILES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 13 | const | `MAX_TOTAL_BYTES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 19 | fn | `find_mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 50 | fn | `expand_file_mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 104 | fn | `tmpdir` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 113 | fn | `find_mentions_basic_and_trailing_punct` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 122 | fn | `expand_inlines_existing_file` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 132 | fn | `nonexistent_mention_is_left_alone` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 139 | fn | `dedup_and_multiple` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 149 | fn | `binary_file_skipped` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 157 | fn | `large_file_truncated` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 1 | module | `model_provider` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 12 | trait | `Provider` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 13 | fn | `model` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 16 | fn | `chat` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 24 | fn | `chat_streaming` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 43 | fn | `model` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 46 | fn | `chat` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 59 | fn | `chat_streaming` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 85 | fn | `provider_error` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 1 | module | `orchestrator` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 35 | enum | `Tier` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 44 | enum | `Complexity` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 54 | trait | `AgentRunner` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 55 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 62 | fn | `reason` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 70 | fn | `run_worker` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 77 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 81 | const | `CLASSIFY_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 85 | const | `PLAN_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 88 | const | `DECOMPOSE_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 93 | const | `WORKER_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 96 | const | `VERIFY_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 103 | struct | `OrchestratorConfig` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 120 | fn | `default` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 133 | struct | `OrchestratorOutcome` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 147 | struct | `Orchestrator` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 153 | fn | `new` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 158 | fn | `handle` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 165 | fn | `handle_at` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 199 | fn | `classify` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 207 | fn | `pipeline` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 221 | fn | `run_attempts` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 282 | fn | `decompose_and_recurse` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 362 | fn | `orch_trace` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 367 | fn | `parse_complexity` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 383 | fn | `verdict_passed` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 391 | fn | `parse_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 418 | fn | `strip_list_marker` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 436 | fn | `build_worker_task` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 450 | fn | `build_decompose_task` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 454 | fn | `build_verify_task` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 464 | fn | `synthesize` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 479 | fn | `synthesize_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 490 | fn | `parse_best_worker` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 516 | struct | `MockRunner` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 530 | fn | `new` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 541 | fn | `with_complexities` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 546 | fn | `with_decomposition` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 550 | fn | `stage` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 569 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 590 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 594 | fn | `count` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 603 | fn | `simple_runs_single_fast` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 616 | fn | `medium_runs_plan_2workers_then_flash_verify` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 632 | fn | `high_atomic_falls_back_to_best_of_n_on_main` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 649 | fn | `high_decomposes_into_recursive_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 671 | fn | `subtask_count_is_capped` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 694 | fn | `recursion_is_depth_capped` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 713 | fn | `decomposition_off_when_max_depth_zero` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 736 | fn | `closed_loop_retries_on_fail_then_passes` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 758 | fn | `verifier_selects_best_worker_and_promotes_it` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 782 | fn | `missing_best_defaults_to_worker_zero` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 791 | fn | `retries_are_capped` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 810 | fn | `parse_subtasks_extracts_prefixed_lines` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 818 | fn | `parse_subtasks_falls_back_to_lists` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 1 | module | `process_tools` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 11 | const | `MAX_BACKGROUND_TASKS` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 13 | const | `MAX_BACKGROUND_TIMEOUT_S` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 14 | struct | `ProcessManager` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 19 | struct | `BackgroundTask` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 28 | fn | `default` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 35 | struct | `BackgroundStartTool` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 37 | struct | `BackgroundPollTool` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 38 | struct | `BackgroundStopTool` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 39 | struct | `BackgroundListTool` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 43 | fn | `name` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 46 | fn | `description` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 50 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 64 | fn | `execute` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 119 | fn | `name` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 122 | fn | `description` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 126 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 130 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 134 | fn | `execute` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 160 | fn | `name` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 163 | fn | `description` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 167 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 171 | fn | `execute` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 190 | fn | `name` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 193 | fn | `description` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 197 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 201 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 205 | fn | `execute` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 216 | fn | `task_id_schema` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 229 | fn | `task_id` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 235 | fn | `render_snapshot` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 252 | fn | `error` |  |
| `rust/crates/ncx-core/src/process_tools.rs` | 267 | fn | `background_command_can_be_started_and_polled` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 1 | module | `project_instructions` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 8 | const | `HEADER` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 11 | const | `TRUNCATED` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 20 | fn | `load_project_instructions` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 31 | fn | `load_workspace_instructions` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 34 | fn | `load_project_instructions_with_home` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 59 | fn | `instruction_paths` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 89 | fn | `repo_root` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 100 | fn | `cap_block` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 113 | fn | `home_dir` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 123 | fn | `tmp` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 133 | fn | `loads_home_and_layered_workspace_files` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 167 | fn | `empty_when_no_files` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 173 | fn | `caps_large_instruction_block` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 1 | module | `prompt` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 8 | struct | `PromptSection` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 17 | struct | `PromptAssembler` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 25 | fn | `new` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 34 | fn | `upsert` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 54 | fn | `remove` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 61 | fn | `build` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 81 | fn | `orders_named_sections_and_skips_empty_content` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 93 | fn | `replacement_and_removal_are_deterministic` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 1 | module | `runtime_profile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 13 | const | `DEFAULT_MAX_MODEL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 15 | const | `DEFAULT_MAX_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 16 | const | `DEFAULT_MAX_PARALLEL_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 17 | const | `DEFAULT_CONTEXT_MAX_CHARS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 18 | const | `DEFAULT_CONTEXT_KEEP_RECENT` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 19 | const | `DEFAULT_CONTEXT_TOOL_RESULT_CHARS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 23 | struct | `AgentRuntimeProfile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 32 | struct | `RuntimePermissionProfile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 42 | fn | `from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 47 | fn | `from_permission_mode` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 61 | fn | `from_legacy_permissions` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 73 | fn | `with_permissions` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 101 | fn | `apply` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 109 | fn | `sandbox_policy` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 115 | fn | `apply_tool_context` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 127 | fn | `model_provider_from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 138 | fn | `vision_provider_from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 160 | fn | `positive_usize` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 167 | fn | `nonnegative_usize` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 178 | fn | `assemble_frontend` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 196 | fn | `cli_and_gui_runtime_assembly_is_equivalent_for_same_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 252 | fn | `invalid_numeric_values_use_runtime_defaults` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 282 | fn | `explicit_legacy_permissions_remain_available_for_cli_flags` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 1 | module | `rust_analyzer` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 17 | const | `REQUEST_TIMEOUT` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 21 | struct | `RustAnalyzerProvider` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 27 | fn | `new` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 37 | fn | `request` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 56 | struct | `LspSession` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 66 | fn | `start` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 110 | fn | `request` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 118 | fn | `open_document` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 149 | fn | `call` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 164 | fn | `notify` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 169 | fn | `write_message` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 186 | fn | `read_response` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 199 | fn | `read_message` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 228 | fn | `terminate` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 235 | fn | `drop` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 239 | fn | `request_parts` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 281 | fn | `file_uri` |  |
| `rust/crates/ncx-core/src/rust_analyzer.rs` | 307 | fn | `rejects_source_paths_outside_workspace` |  |
| `rust/crates/ncx-core/src/search.rs` | 1 | module | `search` |  |
| `rust/crates/ncx-core/src/search.rs` | 19 | const | `IGNORE_DIRS` |  |
| `rust/crates/ncx-core/src/search.rs` | 29 | const | `MAX_FILES` |  |
| `rust/crates/ncx-core/src/search.rs` | 30 | const | `MAX_FILE_BYTES` |  |
| `rust/crates/ncx-core/src/search.rs` | 31 | const | `DEFAULT_MAX_RESULTS` |  |
| `rust/crates/ncx-core/src/search.rs` | 35 | fn | `walk_files` |  |
| `rust/crates/ncx-core/src/search.rs` | 66 | fn | `rel_slash` |  |
| `rust/crates/ncx-core/src/search.rs` | 75 | fn | `glob_to_regex` |  |
| `rust/crates/ncx-core/src/search.rs` | 108 | fn | `grep` |  |
| `rust/crates/ncx-core/src/search.rs` | 167 | fn | `grep_literal` |  |
| `rust/crates/ncx-core/src/search.rs` | 177 | fn | `glob` |  |
| `rust/crates/ncx-core/src/search.rs` | 200 | struct | `GrepTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 204 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 207 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 213 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 224 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 227 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 244 | struct | `GrepLiteralTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 248 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 251 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 256 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 268 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 272 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 288 | struct | `GlobTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 292 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 295 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 300 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 310 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 313 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 326 | struct | `WebSearchTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 330 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 333 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 339 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 348 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 351 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 389 | struct | `WebFetchTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 393 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 396 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 401 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 410 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 413 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 434 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/search.rs` | 456 | fn | `glob_to_regex_matches_expected` |  |
| `rust/crates/ncx-core/src/search.rs` | 466 | fn | `grep_finds_matches_and_skips_ignored` |  |
| `rust/crates/ncx-core/src/search.rs` | 476 | fn | `grep_path_glob_filters` |  |
| `rust/crates/ncx-core/src/search.rs` | 485 | fn | `grep_no_match_reports_count` |  |
| `rust/crates/ncx-core/src/search.rs` | 492 | fn | `grep_invalid_regex_errors` |  |
| `rust/crates/ncx-core/src/search.rs` | 498 | fn | `grep_literal_accepts_regex_metacharacters` |  |
| `rust/crates/ncx-core/src/search.rs` | 506 | fn | `glob_lists_rs_files_skipping_ignored` |  |
| `rust/crates/ncx-core/src/search.rs` | 515 | fn | `web_tools_blocked_in_read_only` |  |
| `rust/crates/ncx-core/src/session.rs` | 1 | module | `session` |  |
| `rust/crates/ncx-core/src/session.rs` | 17 | struct | `ContextEditPolicy` |  |
| `rust/crates/ncx-core/src/session.rs` | 25 | fn | `default` |  |
| `rust/crates/ncx-core/src/session.rs` | 36 | struct | `ContextEditStats` |  |
| `rust/crates/ncx-core/src/session.rs` | 44 | struct | `ContextMessages` |  |
| `rust/crates/ncx-core/src/session.rs` | 50 | struct | `Session` |  |
| `rust/crates/ncx-core/src/session.rs` | 58 | fn | `new` |  |
| `rust/crates/ncx-core/src/session.rs` | 61 | fn | `with_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 75 | fn | `resume` |  |
| `rust/crates/ncx-core/src/session.rs` | 87 | fn | `fork` |  |
| `rust/crates/ncx-core/src/session.rs` | 103 | fn | `full_messages` |  |
| `rust/crates/ncx-core/src/session.rs` | 113 | fn | `add_user` |  |
| `rust/crates/ncx-core/src/session.rs` | 116 | fn | `add_user_text` |  |
| `rust/crates/ncx-core/src/session.rs` | 122 | fn | `add_assistant` |  |
| `rust/crates/ncx-core/src/session.rs` | 143 | fn | `add_tool_result` |  |
| `rust/crates/ncx-core/src/session.rs` | 153 | fn | `for_model` |  |
| `rust/crates/ncx-core/src/session.rs` | 167 | fn | `for_model_edited` |  |
| `rust/crates/ncx-core/src/session.rs` | 192 | fn | `compact` |  |
| `rust/crates/ncx-core/src/session.rs` | 202 | fn | `edited_body` |  |
| `rust/crates/ncx-core/src/session.rs` | 249 | fn | `answered_ids` |  |
| `rust/crates/ncx-core/src/session.rs` | 263 | fn | `backfill_unanswered_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 293 | fn | `append` |  |
| `rust/crates/ncx-core/src/session.rs` | 298 | fn | `append_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 320 | fn | `rewrite_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 350 | fn | `role` |  |
| `rust/crates/ncx-core/src/session.rs` | 354 | fn | `read_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 372 | fn | `sanitize_restored_messages` |  |
| `rust/crates/ncx-core/src/session.rs` | 411 | fn | `redact_image_data` |  |
| `rust/crates/ncx-core/src/session.rs` | 433 | fn | `now_stamp` |  |
| `rust/crates/ncx-core/src/session.rs` | 440 | fn | `json_chars` |  |
| `rust/crates/ncx-core/src/session.rs` | 446 | fn | `total_chars` |  |
| `rust/crates/ncx-core/src/session.rs` | 459 | fn | `estimate_tokens` |  |
| `rust/crates/ncx-core/src/session.rs` | 460 | const | `CHARS_PER_TOKEN` |  |
| `rust/crates/ncx-core/src/session.rs` | 490 | fn | `compress_tool_result` |  |
| `rust/crates/ncx-core/src/session.rs` | 518 | fn | `estimate_tokens_counts_text_and_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 547 | fn | `for_model_prepends_system` |  |
| `rust/crates/ncx-core/src/session.rs` | 558 | fn | `assistant_records_reasoning_only_when_present` |  |
| `rust/crates/ncx-core/src/session.rs` | 567 | fn | `backfill_answers_dangling_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 589 | fn | `context_edit_compresses_old_tool_results_without_mutating_session` |  |
| `rust/crates/ncx-core/src/session.rs` | 621 | fn | `context_edit_drops_old_prefix_when_over_budget` |  |
| `rust/crates/ncx-core/src/session.rs` | 642 | fn | `compact_materializes_context_edit_and_rewrites_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 667 | fn | `compact_noops_when_under_budget` |  |
| `rust/crates/ncx-core/src/session.rs` | 685 | fn | `logs_messages_as_jsonl_and_resumes_body` |  |
| `rust/crates/ncx-core/src/session.rs` | 704 | fn | `resume_backfills_dangling_tool_call` |  |
| `rust/crates/ncx-core/src/session.rs` | 729 | fn | `log_redacts_inline_image_data` |  |
| `rust/crates/ncx-core/src/session.rs` | 744 | fn | `fork_uses_seed_without_touching_source_log` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 1 | module | `session_index` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 16 | const | `TITLE_MAX` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 18 | const | `SNIPPET_MAX` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 19 | static | `SESSION_SEQ` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 23 | struct | `SessionSummary` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 40 | fn | `to_value` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 57 | fn | `from_value` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 107 | struct | `SessionIndex` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 115 | fn | `default` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 121 | fn | `new` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 134 | fn | `entries` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 143 | fn | `get` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 147 | fn | `record` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 152 | fn | `record_turn` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 182 | fn | `set_archived` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 192 | fn | `snapshot_path` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 197 | fn | `save_snapshot` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 211 | fn | `load_snapshot` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 217 | fn | `load` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 232 | fn | `save` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 250 | fn | `new_session_id` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 259 | fn | `default_index_path` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 267 | fn | `summarize` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 342 | fn | `first_text` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 356 | fn | `clip` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 365 | fn | `redact_messages` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 372 | fn | `string_field` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 380 | fn | `usize_field` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 388 | fn | `safe_file_stem` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 401 | fn | `now_stamp` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 412 | fn | `parse_ts_ms` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 446 | fn | `parse_ts_ms_orders_legacy_iso_before_ms_epoch` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 458 | fn | `tmp_path` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 462 | fn | `msgs` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 476 | fn | `summarize_pulls_title_snippet_counts_and_tools` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 497 | fn | `index_upserts_and_sorts_newest_first` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 536 | fn | `persists_and_loads_legacy_rows` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 554 | fn | `snapshot_round_trip_redacts_image_data` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 577 | fn | `session_ids_are_unique` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 1 | module | `session_query_tools` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 10 | const | `MAX_RESULTS` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 12 | fn | `session_query_tools` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 25 | struct | `SessionQueryTool` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 32 | fn | `new` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 35 | fn | `index` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 46 | fn | `name` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 49 | fn | `description` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 63 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 99 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 103 | fn | `execute` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 114 | fn | `search_sessions` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 130 | fn | `session_trace` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 140 | fn | `read_events` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 156 | fn | `search_events` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 177 | fn | `event_trace` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 192 | fn | `summary_json` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 199 | fn | `summary_text` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 209 | fn | `session_id` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 213 | fn | `missing_session_id` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 216 | fn | `limit` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 230 | fn | `searches_and_reads_redacted_snapshots` |  |
| `rust/crates/ncx-core/src/skills.rs` | 1 | module | `skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 28 | const | `INDEX_HEADER` |  |
| `rust/crates/ncx-core/src/skills.rs` | 35 | struct | `Skill` |  |
| `rust/crates/ncx-core/src/skills.rs` | 52 | fn | `load_body` |  |
| `rust/crates/ncx-core/src/skills.rs` | 61 | fn | `is_builtin` |  |
| `rust/crates/ncx-core/src/skills.rs` | 69 | fn | `builtin_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 71 | const | `BUILTINS` |  |
| `rust/crates/ncx-core/src/skills.rs` | 97 | fn | `discover_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 100 | fn | `discover_skills_with_home` |  |
| `rust/crates/ncx-core/src/skills.rs` | 123 | fn | `scan_root` |  |
| `rust/crates/ncx-core/src/skills.rs` | 160 | fn | `skills_index_block` |  |
| `rust/crates/ncx-core/src/skills.rs` | 177 | fn | `parse_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 191 | fn | `frontmatter_lines` |  |
| `rust/crates/ncx-core/src/skills.rs` | 210 | fn | `strip_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 225 | fn | `find_closing_fence` |  |
| `rust/crates/ncx-core/src/skills.rs` | 235 | fn | `unquote` |  |
| `rust/crates/ncx-core/src/skills.rs` | 246 | fn | `home_dir` |  |
| `rust/crates/ncx-core/src/skills.rs` | 256 | fn | `tmp` |  |
| `rust/crates/ncx-core/src/skills.rs` | 263 | fn | `write_skill` |  |
| `rust/crates/ncx-core/src/skills.rs` | 271 | fn | `fs_only` |  |
| `rust/crates/ncx-core/src/skills.rs` | 276 | fn | `discovers_and_parses_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 291 | fn | `name_falls_back_to_dir` |  |
| `rust/crates/ncx-core/src/skills.rs` | 300 | fn | `builtins_are_always_present_and_loadable` |  |
| `rust/crates/ncx-core/src/skills.rs` | 312 | fn | `filesystem_skill_shadows_builtin` |  |
| `rust/crates/ncx-core/src/skills.rs` | 326 | fn | `workspace_shadows_home_same_name` |  |
| `rust/crates/ncx-core/src/skills.rs` | 345 | fn | `index_block_lists_name_and_description` |  |
| `rust/crates/ncx-core/src/skills.rs` | 359 | fn | `empty_when_no_filesystem_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 367 | fn | `malformed_frontmatter_skipped_or_dir_named` |  |
| `rust/crates/ncx-core/src/slash.rs` | 1 | module | `slash` |  |
| `rust/crates/ncx-core/src/slash.rs` | 8 | const | `DEFAULT_LOOP_INTERVAL_S` |  |
| `rust/crates/ncx-core/src/slash.rs` | 11 | const | `SLASH_HELP` |  |
| `rust/crates/ncx-core/src/slash.rs` | 104 | fn | `is_known` |  |
| `rust/crates/ncx-core/src/slash.rs` | 112 | fn | `parse_duration` |  |
| `rust/crates/ncx-core/src/slash.rs` | 151 | fn | `split_loop_arg` |  |
| `rust/crates/ncx-core/src/slash.rs` | 167 | fn | `parse_slash` |  |
| `rust/crates/ncx-core/src/slash.rs` | 188 | fn | `ps` |  |
| `rust/crates/ncx-core/src/slash.rs` | 194 | fn | `plain_text_is_not_a_command` |  |
| `rust/crates/ncx-core/src/slash.rs` | 200 | fn | `bare_command` |  |
| `rust/crates/ncx-core/src/slash.rs` | 206 | fn | `command_with_argument` |  |
| `rust/crates/ncx-core/src/slash.rs` | 218 | fn | `quit_normalizes_to_exit` |  |
| `rust/crates/ncx-core/src/slash.rs` | 223 | fn | `case_insensitive_command` |  |
| `rust/crates/ncx-core/src/slash.rs` | 228 | fn | `help_table_covers_core_commands` |  |
| `rust/crates/ncx-core/src/slash.rs` | 263 | fn | `parse_duration_units` |  |
| `rust/crates/ncx-core/src/slash.rs` | 272 | fn | `parse_duration_rejects_non_durations` |  |
| `rust/crates/ncx-core/src/slash.rs` | 279 | fn | `split_loop_arg_with_leading_interval` |  |
| `rust/crates/ncx-core/src/slash.rs` | 291 | fn | `split_loop_arg_without_interval_uses_default` |  |
| `rust/crates/ncx-core/src/slash.rs` | 303 | fn | `split_loop_arg_empty` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 1 | module | `terminal_tools` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 13 | const | `MAX_TERMINALS` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 15 | struct | `TerminalManager` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 20 | struct | `TerminalSession` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 28 | fn | `default` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 35 | struct | `TerminalOpenTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 37 | struct | `TerminalExecTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 38 | struct | `TerminalWriteTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 39 | struct | `TerminalReadTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 40 | struct | `TerminalResizeTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 41 | struct | `TerminalCloseTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 42 | struct | `TerminalListTool` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 46 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 49 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 53 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 66 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 114 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 117 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 121 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 137 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 177 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 180 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 184 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 188 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 192 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 220 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 223 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 227 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 240 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 288 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 291 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 295 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 308 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 332 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 335 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 339 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 343 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 362 | fn | `name` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 365 | fn | `description` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 369 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 373 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 377 | fn | `execute` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 388 | fn | `resolve_workdir` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 420 | fn | `shell_compatible_path` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 430 | fn | `shell_compatible_path` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 435 | fn | `platform_newline` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 440 | fn | `platform_newline` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 443 | fn | `render_snapshot` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 459 | fn | `terminal_cursor_schema` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 471 | fn | `terminal_id_schema` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 480 | fn | `terminal_id` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 486 | fn | `bounded_u16` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 493 | fn | `error` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 508 | fn | `raw_terminal_accepts_stdin_and_returns_incremental_output` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 548 | fn | `strips_windows_extended_path_prefix_for_cmd` |  |
| `rust/crates/ncx-core/src/terminal_tools.rs` | 556 | fn | `raw_terminal_is_denied_in_read_only_mode` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 1 | module | `tool_middleware` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 13 | enum | `ToolMiddlewareDecision` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 26 | trait | `ToolMiddleware` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 28 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 31 | fn | `before_execute` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 41 | fn | `after_execute` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 63 | struct | `RecordingTool` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 70 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 73 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 77 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 81 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 87 | struct | `RecordingMiddleware` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 96 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 99 | fn | `before_execute` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 115 | fn | `after_execute` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 129 | fn | `registry` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 137 | fn | `layer` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 151 | fn | `middleware_enters_in_order_and_leaves_in_reverse` |  |
| `rust/crates/ncx-core/src/tool_middleware.rs` | 177 | fn | `blocking_short_circuits_and_registration_is_reversible` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 1 | module | `tool_recovery` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 9 | enum | `ToolCapability` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 25 | fn | `fmt` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 46 | enum | `ToolFailureClass` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 59 | fn | `fmt` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 65 | fn | `retryable` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 71 | fn | `classify_tool_result` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 123 | fn | `infer_capabilities` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 178 | fn | `fallback_call` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 216 | fn | `classifies_failures_without_marking_success_text` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 233 | fn | `fallback_routes_only_known_compatible_calls` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 240 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 250 | fn | `registry_falls_back_from_invalid_regex_to_literal_search` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 264 | fn | `registry_treats_directory_read_as_directory_listing` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 276 | struct | `FlakyReadTool` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 280 | struct | `FailingWriteTool` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 287 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 290 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 294 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 298 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 307 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 310 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 314 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 318 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 322 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 335 | fn | `registry_retries_transient_read_once` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 353 | fn | `registry_never_retries_mutating_tools` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 1 | module | `tool_scheduler` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 17 | trait | `ToolScheduler` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 19 | fn | `execute_one` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 27 | fn | `execute_read_only_batch` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 38 | struct | `BoundedToolScheduler` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 42 | fn | `execute_one` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 50 | fn | `execute_read_only_batch` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 61 | fn | `execute_cancellable` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 81 | fn | `execute_bounded_read_only_batch` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 124 | struct | `DelayedReadTool` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 132 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 135 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 139 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 143 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 147 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_scheduler.rs` | 160 | fn | `read_only_pool_is_bounded_and_preserves_model_order` |  |
| `rust/crates/ncx-core/src/tools.rs` | 1 | module | `tools` |  |
| `rust/crates/ncx-core/src/tools.rs` | 32 | const | `DEFAULT_VISIBLE_TOOL_LIMIT` |  |
| `rust/crates/ncx-core/src/tools.rs` | 34 | const | `ALWAYS_VISIBLE_TOOLS` |  |
| `rust/crates/ncx-core/src/tools.rs` | 46 | struct | `ToolCatalogEntry` |  |
| `rust/crates/ncx-core/src/tools.rs` | 56 | enum | `ApprovalDecision` |  |
| `rust/crates/ncx-core/src/tools.rs` | 64 | fn | `approved` |  |
| `rust/crates/ncx-core/src/tools.rs` | 73 | struct | `SessionGrants` |  |
| `rust/crates/ncx-core/src/tools.rs` | 87 | trait | `ApprovalHandler` |  |
| `rust/crates/ncx-core/src/tools.rs` | 88 | fn | `request` |  |
| `rust/crates/ncx-core/src/tools.rs` | 93 | struct | `ToolContext` |  |
| `rust/crates/ncx-core/src/tools.rs` | 141 | fn | `new` |  |
| `rust/crates/ncx-core/src/tools.rs` | 168 | fn | `with_search` |  |
| `rust/crates/ncx-core/src/tools.rs` | 175 | fn | `with_memory` |  |
| `rust/crates/ncx-core/src/tools.rs` | 181 | fn | `with_approver` |  |
| `rust/crates/ncx-core/src/tools.rs` | 187 | fn | `with_user_question_handler` |  |
| `rust/crates/ncx-core/src/tools.rs` | 193 | fn | `with_lsp_provider` |  |
| `rust/crates/ncx-core/src/tools.rs` | 199 | fn | `with_approval_policy` |  |
| `rust/crates/ncx-core/src/tools.rs` | 205 | fn | `with_require_edit_approval` |  |
| `rust/crates/ncx-core/src/tools.rs` | 211 | fn | `with_plan_mode` |  |
| `rust/crates/ncx-core/src/tools.rs` | 218 | fn | `with_session_grants` |  |
| `rust/crates/ncx-core/src/tools.rs` | 224 | fn | `with_timeout` |  |
| `rust/crates/ncx-core/src/tools.rs` | 230 | fn | `with_hooks` |  |
| `rust/crates/ncx-core/src/tools.rs` | 236 | fn | `with_skills` |  |
| `rust/crates/ncx-core/src/tools.rs` | 242 | fn | `with_genome` |  |
| `rust/crates/ncx-core/src/tools.rs` | 250 | trait | `Tool` |  |
| `rust/crates/ncx-core/src/tools.rs` | 251 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools.rs` | 252 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools.rs` | 253 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools.rs` | 257 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 260 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools.rs` | 262 | fn | `to_schema` |  |
| `rust/crates/ncx-core/src/tools.rs` | 277 | struct | `ToolRegistry` |  |
| `rust/crates/ncx-core/src/tools.rs` | 287 | fn | `new` |  |
| `rust/crates/ncx-core/src/tools.rs` | 342 | fn | `empty` |  |
| `rust/crates/ncx-core/src/tools.rs` | 354 | fn | `register_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 370 | fn | `unregister_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 378 | fn | `register` |  |
| `rust/crates/ncx-core/src/tools.rs` | 405 | fn | `replace_tools` |  |
| `rust/crates/ncx-core/src/tools.rs` | 434 | fn | `rebuild_tool_indexes` |  |
| `rust/crates/ncx-core/src/tools.rs` | 462 | fn | `schema_for` |  |
| `rust/crates/ncx-core/src/tools.rs` | 473 | fn | `get` |  |
| `rust/crates/ncx-core/src/tools.rs` | 477 | fn | `is_read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 483 | fn | `schemas` |  |
| `rust/crates/ncx-core/src/tools.rs` | 490 | fn | `schemas_for_query` |  |
| `rust/crates/ncx-core/src/tools.rs` | 493 | fn | `schemas_limited_for_query` |  |
| `rust/crates/ncx-core/src/tools.rs` | 542 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools.rs` | 547 | fn | `execute_with_recovery` |  |
| `rust/crates/ncx-core/src/tools.rs` | 581 | fn | `execute_attempt` |  |
| `rust/crates/ncx-core/src/tools.rs` | 595 | fn | `enter_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 613 | fn | `leave_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 631 | fn | `execute_with_hooks` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 1 | module | `builtins` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 2 | struct | `UpdatePlanTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 12 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 30 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 43 | struct | `ShellTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 48 | fn | `needs_escalation` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 63 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 66 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 72 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 84 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 110 | fn | `resolve_shell_workdir` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 119 | fn | `authorize_shell` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 147 | fn | `request_shell_approval` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 174 | fn | `run_shell` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 193 | fn | `approve_failed_retry` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 215 | struct | `RememberTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 219 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 222 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 228 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 238 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 271 | struct | `SkillTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 275 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 278 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 284 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 293 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 296 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 1 | module | `catalog` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 2 | struct | `ToolSearchTool` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 12 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 22 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 25 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 67 | fn | `tool_words` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 81 | fn | `catalog_score` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 1 | module | `file` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 2 | struct | `ReadFileTool` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 13 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 24 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 27 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 63 | struct | `ApplyPatchTool` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 67 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 70 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 96 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 105 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 142 | fn | `escaping_targets` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 161 | fn | `approve_patch` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 190 | fn | `patch_approval_details` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 5 | fn | `tmp_ws` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 12 | struct | `Answer` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 16 | fn | `request` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 24 | struct | `AlwaysAnswer` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 28 | fn | `request` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 34 | const | `ESCAPING` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 37 | fn | `denied_escaping_patch_is_blocked` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 51 | fn | `no_approver_escaping_patch_errors_out_of_sandbox` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 66 | fn | `in_workspace_patch_needs_no_approval` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 78 | fn | `plan_mode_refuses_edits` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 92 | fn | `require_edit_approval_prompts_in_workspace` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 120 | fn | `always_allow_edits_skips_later_prompts` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 148 | fn | `shell_read_only_command_auto_runs` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 160 | fn | `shell_escalating_command_denied_without_approval` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 172 | fn | `shell_escalating_command_runs_when_approved` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 184 | struct | `NamedTool` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 188 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 191 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 194 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 197 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 203 | fn | `tool_subset_replacement_is_atomic_and_rebuilds_catalog` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 242 | fn | `tool_search_returns_matches_and_hints_schema_exposure` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 270 | fn | `mixed_harness_task_exposes_lsp_background_and_terminal_tools` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 293 | fn | `schema_desc` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 306 | fn | `empty_genome_leaves_schema_and_catalog_byte_identical` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 323 | fn | `genome_override_reaches_schema_and_catalog` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 353 | fn | `skill_tool_loads_body_and_reports_unknown` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 383 | fn | `skill_tool_registered_only_when_skills_present` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 401 | fn | `pre_tool_hook_can_block_execution` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 420 | fn | `post_tool_hook_output_is_returned` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 1 | module | `turn_context` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 11 | struct | `TurnContextRequest` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 21 | trait | `TurnContextProvider` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 23 | fn | `name` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 26 | fn | `provide` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 28 | struct | `ContextEntry` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 36 | struct | `TurnContextRegistry` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 43 | fn | `register` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 65 | fn | `unregister` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 72 | fn | `collect` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 94 | struct | `Provider` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 103 | fn | `name` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 106 | fn | `provide` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 112 | fn | `provider` |  |
| `rust/crates/ncx-core/src/turn_context.rs` | 122 | fn | `providers_are_ordered_unique_and_reversible` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 1 | module | `user_question` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 9 | const | `MAX_QUESTION_CHARS` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 11 | const | `MAX_OPTIONS` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 12 | const | `MAX_OPTION_CHARS` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 16 | struct | `UserQuestionRequest` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 24 | trait | `UserQuestionHandler` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 25 | fn | `request` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 27 | struct | `AskUserQuestionTool` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 33 | fn | `new` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 40 | fn | `name` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 43 | fn | `description` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 47 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 69 | fn | `execute` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 84 | fn | `parse_request` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 122 | fn | `validate_request` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 159 | struct | `AnsweringHandler` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 167 | fn | `request` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 172 | fn | `context` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 181 | fn | `returns_the_frontend_answer` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 202 | fn | `rejects_a_choice_only_question_without_options` |  |
| `rust/crates/ncx-core/src/user_question.rs` | 223 | fn | `registry_exposes_question_tool_only_with_a_handler` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 1 | module | `workspace_tools` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 15 | const | `DEFAULT_LIST_LIMIT` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 17 | const | `MAX_LIST_LIMIT` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 18 | const | `DEFAULT_LIST_DEPTH` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 19 | const | `MAX_LIST_DEPTH` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 22 | struct | `ListDirectoryTool` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 26 | fn | `name` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 29 | fn | `description` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 35 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 63 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 67 | fn | `execute` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 101 | struct | `PathInfoTool` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 105 | fn | `name` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 108 | fn | `description` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 114 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 127 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 131 | fn | `execute` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 155 | struct | `GitStatusTool` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 159 | fn | `name` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 162 | fn | `description` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 167 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 179 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 183 | fn | `execute` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 195 | struct | `GitDiffTool` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 199 | fn | `name` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 202 | fn | `description` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 208 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 224 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 228 | fn | `execute` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 240 | fn | `bounded_usize` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 247 | fn | `resolve_path` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 257 | fn | `display_path` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 271 | fn | `collect_entries` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 312 | fn | `entry_kind` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 324 | fn | `path_metadata` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 341 | fn | `git_status_command` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 349 | fn | `git_diff_command` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 358 | fn | `run_git` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 375 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 386 | fn | `context` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 392 | fn | `list_directory_is_sorted_bounded_and_cross_platform` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 413 | fn | `list_directory_depth_controls_recursion` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 429 | fn | `path_info_reports_existing_and_missing_paths` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 448 | fn | `git_commands_only_vary_fixed_read_only_options` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 460 | fn | `registry_keeps_workspace_tools_visible_and_git_tools_discoverable` |  |
| `rust/crates/ncx-core/src/workspace_tools.rs` | 478 | fn | `schema_names` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 18 | const | `DEFAULT_API_ADDR` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 20 | const | `DEFAULT_ADMIN_ADDR` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 21 | const | `DEFAULT_IMAGE_MODEL` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 24 | struct | `GatewayConfig` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 31 | fn | `from_env` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 52 | struct | `GatewayError` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 58 | fn | `bad_request` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 64 | fn | `unauthorized` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 71 | fn | `config` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 78 | fn | `io` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 88 | fn | `fmt` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 96 | fn | `into_response` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 111 | struct | `ProviderToken` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 120 | struct | `ApiKey` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 129 | struct | `GatewayState` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 137 | fn | `default` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 154 | fn | `verify_api_key` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 169 | fn | `verify_admin_password` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 176 | fn | `add_provider_token` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 213 | fn | `generate_api_key` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 229 | fn | `pick_provider_token` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 239 | fn | `safe_snapshot` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 255 | struct | `SafeProviderToken` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 264 | fn | `from` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 276 | struct | `SafeApiKey` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 285 | fn | `from` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 297 | struct | `SafeState` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 305 | struct | `AppState` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 310 | fn | `load` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 319 | struct | `StateStore` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 325 | fn | `load` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 337 | fn | `save` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 354 | struct | `ModelInfo` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 359 | fn | `built_in_models` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 391 | struct | `SetupRequest` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 396 | struct | `AddTokenRequest` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 403 | struct | `GenerateKeyRequest` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 409 | struct | `ImagesRequest` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 418 | struct | `ChatCompletionRequest` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 425 | struct | `ChatMessage` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 429 | fn | `api_router` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 439 | fn | `admin_router` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 450 | fn | `health` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 458 | fn | `models` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 465 | fn | `images_generations` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 509 | fn | `chat_completions` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 547 | fn | `admin_index` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 551 | fn | `admin_status` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 556 | fn | `admin_setup` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 574 | fn | `admin_add_token` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 585 | fn | `admin_generate_key` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 602 | fn | `require_admin` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 609 | fn | `authorize_api_key` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 620 | fn | `pick_token_label` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 630 | fn | `bearer_token` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 640 | fn | `extract_prompt` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 665 | fn | `format_mock_prompt` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 672 | fn | `redact_secret` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 686 | fn | `hash_secret` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 692 | fn | `now_unix` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 699 | fn | `remove_state_file` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 709 | const | `ADMIN_HTML` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 765 | const | `res` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 766 | const | `data` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 772 | const | `data` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 779 | const | `data` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 790 | const | `data` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 799 | const | `res` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 812 | fn | `redacts_secrets_without_leaking_middle` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 818 | fn | `default_state_has_local_dev_key` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 825 | fn | `token_pool_round_robins` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 839 | fn | `extracts_openai_multimodal_text` |  |
| `rust/crates/ncx-dreamina-gateway/src/lib.rs` | 851 | fn | `model_list_contains_requested_jimeng_image_3` |  |
| `rust/crates/ncx-dreamina-gateway/src/main.rs` | 1 | module | `main` |  |
| `rust/crates/ncx-dreamina-gateway/src/main.rs` | 4 | fn | `main` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 20 | const | `PROTOCOL` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 22 | const | `REQ_TIMEOUT` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 26 | struct | `McpToolDef` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 34 | struct | `McpClient` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 45 | fn | `connect` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 72 | fn | `initialize` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 85 | fn | `write_msg` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 95 | fn | `notify` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 103 | fn | `request` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 146 | fn | `list_tools` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 177 | fn | `call_tool` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 186 | fn | `drop` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 194 | fn | `format_content` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 236 | fn | `format_content_joins_text_blocks` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 242 | fn | `format_content_includes_structured` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 252 | fn | `format_content_empty_error` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 260 | fn | `write_mock_server` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 289 | fn | `python` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 296 | fn | `connects_lists_and_calls_against_mock_server` |  |
| `rust/crates/ncx-provider/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 1 | module | `provider` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 20 | const | `DEFAULT_STREAM_OPEN_TIMEOUT_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 22 | const | `STREAM_OPEN_TIMEOUT_MIN_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 23 | const | `STREAM_OPEN_TIMEOUT_MAX_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 26 | fn | `stream_open_timeout_s` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 35 | fn | `backoff_sleep` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 41 | fn | `stream_open_timeout_from` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 53 | struct | `DeepSeekProvider` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 63 | const | `SUPPORTS_STREAMING` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 64 | fn | `new` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 68 | fn | `with_opts` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 89 | fn | `body` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 109 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 135 | fn | `post` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 167 | fn | `chat_stream` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 254 | fn | `consume_sse` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 308 | struct | `StreamAgg` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 318 | struct | `ToolFrag` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 325 | fn | `ingest` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 380 | fn | `finish` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 411 | struct | `HttpErr` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 418 | fn | `from_reqwest` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 436 | fn | `stream_open_timeout_defaults_when_unset` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 442 | fn | `stream_open_timeout_honors_env_within_bounds` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 447 | fn | `stream_open_timeout_clamps_and_tolerates_garbage` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 454 | fn | `provider_default_max_retries_is_three` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 460 | fn | `provider_sets_max_retries` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 472 | fn | `endpoint_appends_chat_completions_without_double_slash` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 478 | fn | `stream_agg_aggregates_content_and_tool_calls` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 518 | fn | `stream_agg_synthesizes_id_when_missing` |  |
| `rust/crates/ncx-provider/src/request.rs` | 1 | module | `request` |  |
| `rust/crates/ncx-provider/src/request.rs` | 11 | const | `REASONING_PLACEHOLDER` |  |
| `rust/crates/ncx-provider/src/request.rs` | 13 | const | `DISABLED_REASONING_EFFORTS` |  |
| `rust/crates/ncx-provider/src/request.rs` | 16 | fn | `is_deepseek_model` |  |
| `rust/crates/ncx-provider/src/request.rs` | 23 | fn | `build_body` |  |
| `rust/crates/ncx-provider/src/request.rs` | 59 | fn | `apply_reasoning_effort` |  |
| `rust/crates/ncx-provider/src/request.rs` | 109 | fn | `sanitize_reasoning_replay` |  |
| `rust/crates/ncx-provider/src/request.rs` | 142 | fn | `should_replay_reasoning_content` |  |
| `rust/crates/ncx-provider/src/request.rs` | 151 | fn | `requires_reasoning_content` |  |
| `rust/crates/ncx-provider/src/request.rs` | 162 | fn | `to_request_json` |  |
| `rust/crates/ncx-provider/src/request.rs` | 177 | fn | `user_msg` |  |
| `rust/crates/ncx-provider/src/request.rs` | 181 | fn | `assistant_toolcall` |  |
| `rust/crates/ncx-provider/src/request.rs` | 195 | fn | `replays_reasoning_placeholder_for_deepseek_tool_history` |  |
| `rust/crates/ncx-provider/src/request.rs` | 212 | fn | `preserves_existing_reasoning_content` |  |
| `rust/crates/ncx-provider/src/request.rs` | 230 | fn | `does_not_replay_reasoning_when_effort_disabled` |  |
| `rust/crates/ncx-provider/src/request.rs` | 245 | fn | `maps_reasoning_effort_to_deepseek_beta_body` |  |
| `rust/crates/ncx-provider/src/request.rs` | 261 | fn | `deepseek_collapses_low_medium_to_high` |  |
| `rust/crates/ncx-provider/src/request.rs` | 278 | fn | `generic_model_passes_real_tier_through_top_level` |  |
| `rust/crates/ncx-provider/src/request.rs` | 306 | fn | `generic_model_off_omits_reasoning_field` |  |
| `rust/crates/ncx-provider/src/request.rs` | 320 | fn | `auto_and_none_set_no_reasoning_fields` |  |
| `rust/crates/ncx-provider/src/request.rs` | 331 | fn | `tools_add_tool_choice_auto` |  |
| `rust/crates/ncx-provider/src/request.rs` | 346 | fn | `to_request_json_flattens_extra_body` |  |
| `rust/crates/ncx-provider/src/response.rs` | 1 | module | `response` |  |
| `rust/crates/ncx-provider/src/response.rs` | 13 | fn | `extract_reasoning` |  |
| `rust/crates/ncx-provider/src/response.rs` | 27 | fn | `extract_usage` |  |
| `rust/crates/ncx-provider/src/response.rs` | 54 | fn | `parse_completion` |  |
| `rust/crates/ncx-provider/src/response.rs` | 114 | fn | `parse_args` |  |
| `rust/crates/ncx-provider/src/response.rs` | 129 | fn | `extract_reasoning_accepts_reasoning_alias` |  |
| `rust/crates/ncx-provider/src/response.rs` | 137 | fn | `extract_reasoning_prefers_reasoning_content` |  |
| `rust/crates/ncx-provider/src/response.rs` | 143 | fn | `extract_usage_captures_basic_tokens` |  |
| `rust/crates/ncx-provider/src/response.rs` | 152 | fn | `extract_usage_records_cache_split_when_present` |  |
| `rust/crates/ncx-provider/src/response.rs` | 163 | fn | `extract_usage_none_is_empty` |  |
| `rust/crates/ncx-provider/src/response.rs` | 169 | fn | `parse_completion_basic_content` |  |
| `rust/crates/ncx-provider/src/response.rs` | 182 | fn | `parse_completion_tool_calls` |  |
| `rust/crates/ncx-provider/src/response.rs` | 204 | fn | `parse_completion_bad_args_collapse_to_empty_object` |  |
| `rust/crates/ncx-provider/src/response.rs` | 219 | fn | `parse_completion_defaults_finish_reason_to_stop` |  |
| `rust/crates/ncx-provider/src/types.rs` | 1 | module | `types` |  |
| `rust/crates/ncx-provider/src/types.rs` | 9 | struct | `ToolCall` |  |
| `rust/crates/ncx-provider/src/types.rs` | 18 | struct | `ModelResponse` |  |
| `rust/crates/ncx-provider/src/types.rs` | 28 | fn | `default` |  |
| `rust/crates/ncx-provider/src/types.rs` | 40 | fn | `has_tool_calls` |  |
| `rust/crates/ncx-provider/src/types.rs` | 47 | struct | `ProviderError` |  |
| `rust/crates/ncx-provider/src/types.rs` | 50 | fn | `fmt` |  |
| `rust/crates/ncx-provider/src/web.rs` | 1 | module | `web` |  |
| `rust/crates/ncx-provider/src/web.rs` | 11 | const | `ENDPOINT` |  |
| `rust/crates/ncx-provider/src/web.rs` | 13 | const | `TAVILY_ENDPOINT` |  |
| `rust/crates/ncx-provider/src/web.rs` | 14 | const | `WIKIPEDIA_ENDPOINT` |  |
| `rust/crates/ncx-provider/src/web.rs` | 15 | const | `BING_ENDPOINT` |  |
| `rust/crates/ncx-provider/src/web.rs` | 20 | fn | `tavily_search` |  |
| `rust/crates/ncx-provider/src/web.rs` | 51 | fn | `format_tavily` |  |
| `rust/crates/ncx-provider/src/web.rs` | 84 | fn | `ddg_instant_answer` |  |
| `rust/crates/ncx-provider/src/web.rs` | 113 | fn | `wikipedia_search` |  |
| `rust/crates/ncx-provider/src/web.rs` | 140 | fn | `format_wikipedia` |  |
| `rust/crates/ncx-provider/src/web.rs` | 175 | fn | `bing_rss_search` |  |
| `rust/crates/ncx-provider/src/web.rs` | 196 | fn | `format_bing_rss` |  |
| `rust/crates/ncx-provider/src/web.rs` | 229 | fn | `format_answer` |  |
| `rust/crates/ncx-provider/src/web.rs` | 293 | const | `MAX_FETCH_BYTES` |  |
| `rust/crates/ncx-provider/src/web.rs` | 295 | const | `MAX_TEXT_CHARS` |  |
| `rust/crates/ncx-provider/src/web.rs` | 299 | fn | `fetch_url` |  |
| `rust/crates/ncx-provider/src/web.rs` | 347 | fn | `html_to_text` |  |
| `rust/crates/ncx-provider/src/web.rs` | 408 | fn | `find_ci` |  |
| `rust/crates/ncx-provider/src/web.rs` | 428 | fn | `html_to_text_strips_tags_and_scripts` |  |
| `rust/crates/ncx-provider/src/web.rs` | 438 | fn | `formats_abstract_and_related` |  |
| `rust/crates/ncx-provider/src/web.rs` | 452 | fn | `empty_response_explains_limits` |  |
| `rust/crates/ncx-provider/src/web.rs` | 459 | fn | `tavily_formats_answer_and_results` |  |
| `rust/crates/ncx-provider/src/web.rs` | 473 | fn | `wikipedia_formats_parallel_result_arrays` |  |
| `rust/crates/ncx-provider/src/web.rs` | 486 | fn | `bing_formats_rss_items` |  |
| `rust/crates/ncx-provider/src/web.rs` | 494 | fn | `tavily_without_key_errors` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 1 | module | `approval` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 17 | const | `UNTRUSTED` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 19 | const | `ON_FAILURE` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 20 | const | `ON_REQUEST` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 21 | const | `NEVER` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 25 | enum | `Decision` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 36 | struct | `ApprovalRequest` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 47 | const | `WRITE_TOOLS` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 54 | fn | `step_decision` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 66 | const | `TRUSTED_COMMANDS` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 73 | const | `GIT_WRITE_SUBCMDS` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 92 | fn | `dangerous_patterns` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 93 | static | `PATS` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 109 | fn | `first_token` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 119 | fn | `is_trusted` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 146 | struct | `Approver` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 151 | fn | `new` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 161 | fn | `classify` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 198 | fn | `never_policy_auto_denies_escalation` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 205 | fn | `on_request_asks_only_on_escalation` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 212 | fn | `on_failure_runs_first` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 218 | fn | `untrusted_auto_approves_safe_commands` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 226 | fn | `untrusted_asks_for_unknown_or_write_commands` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 234 | fn | `untrusted_blocks_dangerous_even_if_leading_token_trusted` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 241 | fn | `dangerous_pattern_blocks_trusted_leading_token` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 248 | fn | `step_decision_upgrades_writes_when_confirming_each_step` |  |
| `rust/crates/ncx-sandbox/src/approval.rs` | 271 | fn | `git_exe_with_path_prefix_is_normalized` |  |
| `rust/crates/ncx-sandbox/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 1 | module | `policy` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 17 | const | `READ_ONLY` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 19 | const | `WORKSPACE_WRITE` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 20 | const | `DANGER_FULL_ACCESS` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 24 | struct | `SandboxPolicy` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 35 | fn | `new` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 44 | fn | `with_writable_roots` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 53 | fn | `with_allow_temp_write` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 58 | fn | `with_network_access` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 65 | fn | `writes_allowed` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 68 | fn | `writable_dirs` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 80 | fn | `can_read` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 85 | fn | `can_write` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 100 | fn | `describe` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 123 | fn | `make_absolute` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 133 | fn | `normalize` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 154 | fn | `base` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 159 | fn | `read_only_forbids_writes` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 167 | fn | `workspace_write_allows_inside_only` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 177 | fn | `workspace_write_denies_system_temp_by_default` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 191 | fn | `workspace_write_honors_extra_writable_roots` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 199 | fn | `danger_full_access_allows_everything` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 207 | fn | `relative_path_resolves_against_workspace` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 1 | module | `detect` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 9 | const | `READ_ONLY_PREFIXES` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 36 | const | `WRITE_OR_SUBSHELL` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 39 | fn | `split_chain` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 53 | fn | `looks_read_only` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 83 | fn | `plain_read_only_commands_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 99 | fn | `plain_writes_do_not_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 111 | fn | `command_chain_with_write_does_not_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 124 | fn | `all_segments_read_only_passes` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 131 | fn | `redirection_does_not_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 143 | fn | `command_substitution_does_not_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 150 | fn | `arbitrary_code_runners_not_assumed_read_only` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 158 | fn | `prefix_lookalike_does_not_pass` |  |
| `rust/crates/ncx-tools/src/detect.rs` | 164 | fn | `empty_is_not_read_only` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 1 | module | `executor` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 19 | const | `MAX_OUTPUT` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 24 | struct | `ExecResult` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 34 | fn | `ok` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 39 | fn | `render` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 70 | struct | `PolicyExecutor` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 76 | fn | `default` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 85 | fn | `new` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 90 | fn | `run` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 97 | fn | `run_with_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 170 | fn | `command_with_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 188 | const | `CREATE_NO_WINDOW` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 195 | fn | `base_command` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 213 | fn | `which_bash` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 223 | fn | `build_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 268 | struct | `Job` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 282 | fn | `contain` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 320 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 328 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 349 | fn | `ok_requires_zero_exit` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 364 | fn | `render_includes_exit_code` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 376 | fn | `render_includes_stderr_and_timeout` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 390 | fn | `render_sandbox_denied` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 400 | fn | `render_truncates_huge_output` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 411 | fn | `run_echo_returns_stdout` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 424 | fn | `run_nonzero_exit_is_captured` |  |
| `rust/crates/ncx-tools/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 1 | module | `managed` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 14 | const | `MAX_BUFFERED_BYTES` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 18 | struct | `ProcessOutputChunk` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 25 | struct | `ProcessSnapshot` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 31 | struct | `ManagedProcess` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 43 | struct | `OutputBuffer` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 50 | fn | `spawn_managed` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 92 | fn | `poll` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 120 | fn | `write_stdin` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 135 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 147 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 154 | fn | `spawn_reader` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 179 | fn | `push_chunk` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 196 | fn | `managed_process_returns_incremental_output_and_exit` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 1 | module | `patch` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 24 | struct | `PatchError` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 27 | fn | `fmt` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 34 | enum | `ActionType` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 42 | struct | `Chunk` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 50 | struct | `FileAction` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 57 | const | `BEGIN` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 59 | const | `END` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 60 | const | `ADD` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 61 | const | `UPDATE` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 62 | const | `DELETE` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 63 | const | `MOVE` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 64 | const | `HUNK_AT` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 65 | fn | `err` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 71 | fn | `parse_patch` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 165 | fn | `parse_update_body` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 241 | fn | `match_at` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 262 | fn | `apply_update` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 318 | struct | `ApplyOutcome` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 326 | fn | `summary` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 347 | fn | `apply_patch` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 444 | fn | `tmpdir` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 451 | fn | `allow_all` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 457 | fn | `parse_requires_begin_and_end` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 463 | fn | `parse_add_file` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 473 | fn | `parse_rejects_add_line_without_plus` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 479 | fn | `parse_update_with_locator_and_change` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 490 | fn | `add_file_writes_to_disk` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 504 | fn | `add_file_rejects_existing` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 517 | fn | `update_replaces_matched_lines` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 532 | fn | `update_uses_locator_to_disambiguate` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 545 | fn | `update_with_whitespace_fallback` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 557 | fn | `update_failure_to_locate_is_atomic` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 571 | fn | `delete_file` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 585 | fn | `move_file_writes_dest_removes_source` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 602 | fn | `unwritable_path_blocks_whole_patch` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 614 | fn | `summary_orders_a_r_m_d` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 1 | module | `pty` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 12 | const | `MAX_BUFFERED_BYTES` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 16 | struct | `PtyOutputChunk` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 22 | struct | `PtySnapshot` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 30 | struct | `PtyProcess` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 41 | struct | `OutputBuffer` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 48 | fn | `spawn_pty` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 93 | fn | `write` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 106 | fn | `resize` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 117 | fn | `poll` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 141 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 153 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 157 | fn | `shell_command` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 171 | fn | `spawn_reader` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 205 | fn | `push_chunk` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 224 | fn | `raw_pty_accepts_stdin_and_returns_output` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 1 | module | `read_file` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 3 | const | `MAX_CHARS` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 5 | const | `DEFAULT_LIMIT` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 10 | fn | `render` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 51 | fn | `empty_file` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 56 | fn | `numbers_lines_from_one` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 63 | fn | `offset_and_limit_window` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 71 | fn | `offset_beyond_end_errors` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 77 | fn | `crlf_normalized` |  |
| `rust/crates/ncx-tools/src/read_file.rs` | 83 | fn | `zero_offset_treated_as_one` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 1 | module | `ark` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 6 | const | `ARK_BASE_URL` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 8 | trait | `ArkTransport` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 10 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 18 | struct | `ReqwestArkTransport` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 24 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 33 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 61 | struct | `ArkTaskStatus` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 66 | struct | `ArkClient` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 74 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 77 | fn | `with_base_url` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 95 | fn | `submit` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 118 | fn | `poll_once` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 155 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 169 | fn | `preview` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 171 | const | `MAX` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 186 | struct | `FakeTransport` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 192 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 210 | fn | `ark_submit_posts_task_and_reads_id` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 227 | fn | `ark_poll_reads_succeeded_video_url_and_usage` |  |
| `rust/crates/ncx-video-agent/src/ark.rs` | 255 | fn | `ark_rejects_empty_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_dry_run.rs` | 1 | module | `p1_dry_run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_dry_run.rs` | 4 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_paid_config_check.rs` | 1 | module | `p1_paid_config_check` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_paid_config_check.rs` | 6 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_paid_config_check.rs` | 16 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 1 | module | `p1_seedance_tos_smoke` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 16 | const | `MODEL` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 18 | const | `PROJECT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 19 | const | `SHOT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 20 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 38 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 297 | fn | `clear_stale_output_evidence` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 324 | fn | `remove_file_if_exists` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 335 | fn | `is_direct_smoke_sqlite_sidecar` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 349 | fn | `seed_db` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 365 | fn | `poll_until_succeeded` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 399 | fn | `elapsed_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 403 | fn | `total_tokens` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 418 | fn | `direct_smoke_sqlite_sidecar_matcher_is_narrow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 431 | fn | `clear_stale_output_evidence_removes_only_direct_smoke_files` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 1 | module | `p1_smoke` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 11 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 76 | struct | `Check` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 82 | fn | `check_sqlite` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 121 | fn | `check_temporal_port` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 154 | fn | `check_ffmpeg` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 181 | fn | `check_opencv` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 253 | fn | `check_fasttext` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 300 | fn | `check_fasttext_cli` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 348 | fn | `check_tos_roundtrip` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 415 | fn | `chrono_free_timestamp` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 422 | fn | `check_resolved_setting` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 442 | fn | `check_config_load` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_smoke.rs` | 457 | fn | `first_env_value` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1 | module | `p1_temporal_probe` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 27 | const | `DEFAULT_TASK_QUEUE` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 29 | const | `DEFAULT_WORKFLOW_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 30 | const | `DEFAULT_DRY_RUN_WORKFLOW_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 31 | const | `DEFAULT_LIVE_WORKFLOW_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 32 | const | `DEFAULT_SHOT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 33 | const | `LIVE_PROJECT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 34 | const | `LIVE_CHAPTER_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 35 | const | `LIVE_SCENE_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 36 | const | `LIVE_SHOT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 37 | const | `LIVE_MODEL` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 38 | const | `LIVE_DURATION_S` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 39 | const | `LIVE_MAX_POLLS` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 40 | const | `LIVE_POLL_INTERVAL_S` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 41 | struct | `P1ProbeActivities` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 47 | fn | `submit_video_job` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 55 | fn | `poll_video_job` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 64 | fn | `prepare_p1_dry_run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 79 | fn | `run_p1_dry_run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 99 | fn | `submit_live_seedance_job` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 107 | fn | `poll_live_seedance_job` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 115 | fn | `persist_live_seedance_outputs` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 122 | fn | `activity_opts` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 126 | fn | `dry_run_activity_opts` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 130 | fn | `live_submit_activity_opts` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 134 | fn | `live_poll_activity_opts` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 138 | fn | `live_persist_activity_opts` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 142 | fn | `activity_error` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 146 | fn | `workflow_error` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 153 | struct | `P1ProbeWorkflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 160 | struct | `P1DryRunWorkflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 164 | struct | `P1LiveSeedanceWorkflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 169 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 194 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 243 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 272 | fn | `approve` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 277 | fn | `is_approved` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 282 | fn | `gate_state` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 294 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 312 | fn | `temporal_client` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 319 | fn | `run_worker` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 339 | fn | `start_dry_run_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 358 | fn | `wait_dry_run_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 369 | fn | `start_live_seedance_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 389 | fn | `wait_live_seedance_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 400 | fn | `start_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 419 | fn | `signal_approval` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 434 | fn | `query_gate_state` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 449 | fn | `wait_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 460 | fn | `task_queue` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 464 | fn | `workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 468 | fn | `dry_run_workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 475 | fn | `live_workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 479 | fn | `dry_run_out_dir` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 492 | fn | `live_out_dir` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 505 | fn | `shot_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 509 | fn | `env_or_default` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 517 | fn | `print_help` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 562 | fn | `submit_live_seedance_job_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 607 | fn | `poll_live_seedance_job_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 719 | fn | `persist_live_seedance_outputs_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 926 | fn | `require_live_opt_in` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 941 | fn | `resolve_ark_api_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 952 | fn | `seed_live_db` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 991 | fn | `live_seedance_payload` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1004 | fn | `live_db_path` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1008 | fn | `write_live_marker` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1016 | fn | `parse_state` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1020 | fn | `required_string` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1030 | fn | `state_kind` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1041 | fn | `state_reason` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1052 | fn | `artifact_exists` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1064 | fn | `artifact_tos_uri` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1076 | fn | `validation_exists` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1088 | fn | `record_validation_once` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1096 | fn | `live_video_tos_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1103 | fn | `live_rough_tos_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1110 | fn | `live_video_artifact_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1114 | fn | `live_rough_artifact_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1121 | fn | `live_video_validation_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1128 | fn | `live_rough_validation_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1135 | fn | `sanitize_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1153 | fn | `now_unix_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1160 | fn | `elapsed_since_unix_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1164 | fn | `total_tokens` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 1 | module | `p1_video_to_prompt` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 23 | const | `DEFAULT_FRAMES` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 25 | const | `GATE_VERSION` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 26 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 164 | struct | `Parsed` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 173 | fn | `parse_args` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_video_to_prompt.rs` | 218 | fn | `persist` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 1 | module | `db` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 8 | struct | `Database` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 14 | fn | `open` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 18 | fn | `connection` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 22 | fn | `connection_mut` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 26 | fn | `create_project` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 35 | fn | `create_chapter` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 44 | fn | `create_scene` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 56 | fn | `create_shot` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 84 | fn | `create_artifact` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 101 | fn | `create_project_artifact` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 119 | fn | `open_db` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 129 | fn | `require_json1` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 137 | fn | `init_schema` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 286 | fn | `ensure_artifact_project_id` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 302 | fn | `ensure_artifact_owner_triggers` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 329 | fn | `ensure_artifact_trace_field_triggers` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 354 | fn | `ensure_job_trace_field_triggers` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 379 | fn | `ensure_job_contract_triggers` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 434 | fn | `ensure_validation_record_contract_triggers` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 474 | fn | `schema_creates_tables_wal_and_json1` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 511 | fn | `schema_rejects_invalid_json_columns` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 583 | fn | `schema_rejects_invalid_p1_contract_values` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 713 | fn | `schema_rejects_artifacts_without_exactly_one_owner` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 771 | fn | `schema_rejects_empty_trace_identity_fields` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 853 | fn | `schema_rejects_invalid_job_trace_contract_values` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 986 | fn | `schema_rejects_duplicate_pass_for_same_artifact_stage` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 1063 | fn | `duplicate_idempotency_key_is_rejected` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 1117 | fn | `schema_supports_excellence_fields_from_p1` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 1 | module | `dry_run` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 25 | struct | `LocalDryRunOutput` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 33 | fn | `run_local_p1_dry_run` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 257 | fn | `elapsed_ms` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 261 | fn | `seed_project` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 268 | fn | `seed_agent_artifacts` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 410 | fn | `write_structured_agent_pass` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 436 | fn | `remove_previous_sqlite` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 456 | fn | `make_color_clip` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 475 | fn | `make_local_tts_placeholder_audio` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 496 | fn | `local_file_hash_marker` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 501 | fn | `sha256_file_hash_marker` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 529 | fn | `local_p1_dry_run_produces_rough_cut_and_trace` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 604 | fn | `local_p1_dry_run_can_be_repeated_in_same_output_dir` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 1 | module | `edit` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 9 | struct | `RenderedShot` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 18 | struct | `FailedShot` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 25 | struct | `RoughCutResult` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 31 | struct | `AssemblyClip` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 40 | fn | `build_rough_cut` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 130 | fn | `write_failed_shots` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 147 | fn | `write_assembly_manifest` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 158 | fn | `assemble_shot_clip` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 259 | fn | `concat_clips` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 298 | fn | `ffmpeg_concat_path` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 304 | fn | `subtitle_filter` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 308 | fn | `ffmpeg_filter_path` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 315 | fn | `usable_file` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 319 | fn | `missing_optional_media_notes` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 334 | fn | `sanitize_filename` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 364 | fn | `partial_delivery_writes_failed_shots_even_without_any_clips` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 395 | fn | `ffmpeg_builds_rough_cut_and_keeps_failed_context` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 467 | fn | `make_test_clip` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 487 | fn | `make_test_audio` |  |
| `rust/crates/ncx-video-agent/src/edit.rs` | 508 | fn | `ffprobe_audio_stream_count` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1 | module | `jobs` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 8 | struct | `JobRecord` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 28 | struct | `JobSubmitOutcome` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 32 | fn | `idempotency_key` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 46 | fn | `submit_job_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 119 | fn | `settle_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 160 | fn | `mark_job_status` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 197 | fn | `record_job_latency_ms` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 219 | fn | `fail_job_and_release_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 259 | fn | `reserve_and_insert_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 313 | fn | `release_failed_reservation` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 346 | fn | `ensure_shot_belongs_to_project` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 368 | fn | `ensure_job_belongs_to_project` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 391 | fn | `validate_finite_nonnegative` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 400 | fn | `validate_nonempty_reason` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 409 | fn | `load_job_by_key` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 423 | fn | `load_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 437 | fn | `row_to_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 464 | fn | `canonical_json` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 506 | fn | `seeded_db` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 527 | fn | `idempotency_key_canonicalizes_json_object_order` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 541 | fn | `submit_job_is_idempotent_and_reserves_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 597 | fn | `submit_job_rejects_invalid_reserve_cost_before_provider_call` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 637 | fn | `submit_job_rejects_empty_provider_or_model_before_provider_call` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 695 | fn | `submit_job_releases_budget_when_provider_returns_empty_job_id` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 755 | fn | `submit_job_rejects_cross_project_shot_without_mutating_ledger` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 809 | fn | `submit_job_refuses_to_resubmit_ambiguous_existing_job_without_provider_id` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 852 | fn | `submit_job_refuses_to_retry_submit_failed_job_without_provider_id` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 907 | fn | `settle_budget_reconciles_project_and_job_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 940 | fn | `settle_budget_rejects_invalid_cost_or_tokens_without_mutating_ledger` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 991 | fn | `settle_and_fail_reject_cross_project_job_without_mutating_ledgers` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1066 | fn | `mark_job_status_only_allows_provider_poll_statuses` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1121 | fn | `status_and_latency_updates_reject_missing_jobs` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1136 | fn | `concurrent_reservations_never_exceed_project_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1189 | fn | `failed_provider_job_releases_reserved_budget_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 1240 | fn | `failed_provider_job_requires_failure_reason` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 1 | module | `keyframes` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 20 | const | `DEFAULT_MAX_WIDTH` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 23 | const | `OCR_MAX_WIDTH` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 25 | const | `DEFAULT_SCENE_THRESHOLD` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 27 | const | `MAX_FRAMES` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 35 | fn | `extract_keyframes` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 42 | fn | `extract_keyframes_scaled` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 81 | fn | `scene_keyframes` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 116 | fn | `uniform_keyframes` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 160 | fn | `read_jpegs` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 181 | fn | `dedup_exact` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 195 | fn | `downsample` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 205 | struct | `TempDir` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 210 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 227 | fn | `drop` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 235 | fn | `ffmpeg_available` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 240 | fn | `make_test_clip` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 261 | fn | `extract_keyframes_returns_bounded_nonempty_jpegs` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 285 | fn | `extract_keyframes_rejects_missing_file` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 292 | fn | `dedup_exact_collapses_identical_frames` |  |
| `rust/crates/ncx-video-agent/src/keyframes.rs` | 299 | fn | `downsample_picks_evenly_and_caps` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 1 | module | `l0` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 11 | trait | `LanguageDetector` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 13 | fn | `detect_language` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 17 | struct | `FastTextCliDetector` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 23 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 29 | fn | `with_binary` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 36 | fn | `model_path` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 43 | fn | `detect_language` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 89 | struct | `FastTextModelDetector` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 95 | fn | `load` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 105 | fn | `model_path` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 112 | fn | `detect_language` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 139 | struct | `HeuristicLanguageDetector` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 142 | fn | `detect_language` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 154 | enum | `L0Verdict` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 160 | struct | `L0Report` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 166 | fn | `pass` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 172 | fn | `repair` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 180 | fn | `validate_scene_l0` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 338 | struct | `ShotForL0` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 345 | fn | `is_boundary_reference` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 349 | fn | `check_continuity_closure` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 379 | fn | `db_continuity_in` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 383 | fn | `db_continuity_out` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 387 | fn | `plan_continuity_in` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 391 | fn | `plan_continuity_out` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 395 | fn | `candidate_text_fields` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 403 | fn | `is_cjk` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 410 | fn | `parse_fasttext_label` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 423 | fn | `seeded_scene` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 434 | fn | `l0_rejects_unclosed_references` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 459 | fn | `l0_rejects_self_referential_continuity` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 484 | fn | `l0_rejects_nonreciprocal_continuity_closure` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 520 | fn | `l0_repairs_english_text_when_chinese_required` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 545 | fn | `l0_passes_valid_chinese_scene_and_duration_budget` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 591 | fn | `parses_fasttext_language_labels` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 78 | enum | `VideoAgentError` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 112 | type | `Result` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 120 | static | `NEXT_ID` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 122 | fn | `temp_db_path` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 1 | module | `media` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 9 | struct | `MediaProbe` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 21 | struct | `MediaL0Report` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 28 | fn | `layers_json` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 36 | fn | `validate_video_file_l0` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 127 | fn | `media_probe_from_ffprobe` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 177 | fn | `repair` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 185 | fn | `media_probe_json` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 198 | fn | `parse_json_f64` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 204 | fn | `parse_rate` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 220 | fn | `media_l0_passes_parseable_video_with_expected_duration` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 244 | fn | `media_l0_rejects_missing_audio_when_required` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 265 | fn | `media_l0_reports_missing_file_as_repair` |  |
| `rust/crates/ncx-video-agent/src/media.rs` | 272 | fn | `make_test_clip` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 1 | module | `node` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 7 | enum | `NodeKind` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 13 | enum | `AgentReasoningMode` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 20 | struct | `NodeSpec` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 29 | enum | `P1AgentNode` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 35 | fn | `p1_agent_node_spec` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 54 | fn | `agent_spec` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 70 | struct | `ContextPacket` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 77 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 91 | fn | `assert_context_packet_admissible` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 107 | fn | `validate_node_spec` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 133 | fn | `assert_no_reasoning_leak` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 142 | fn | `find_forbidden_reasoning_key` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 172 | fn | `is_forbidden_context_key` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 188 | fn | `render_path` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 205 | fn | `db_with_artifact` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 228 | fn | `context_packet_rejects_reasoning_or_conversation_history` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 242 | fn | `admissible_context_requires_upstream_pass_record` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 280 | fn | `judgment_and_planning_nodes_have_no_tools_and_agents_pin_mode` |  |
| `rust/crates/ncx-video-agent/src/node.rs` | 307 | fn | `p1_agent_node_specs_pin_modes_and_have_no_tools` |  |
| `rust/crates/ncx-video-agent/src/preflight.rs` | 1 | module | `preflight` |  |
| `rust/crates/ncx-video-agent/src/preflight.rs` | 2 | fn | `resolve_paid_seedance_prereqs` |  |
| `rust/crates/ncx-video-agent/src/preflight.rs` | 24 | fn | `paid_seedance_prereqs_do_not_resolve_ark_when_tos_is_missing` |  |
| `rust/crates/ncx-video-agent/src/preflight.rs` | 40 | fn | `paid_seedance_prereqs_resolve_tos_before_ark` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 1 | const | `SEEDANCE_PRICING_AS_OF` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 1 | module | `pricing` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 2 | const | `SEEDANCE_CNY_PER_M_NO_VIDEO_INPUT` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 3 | const | `SEEDANCE_CNY_PER_M_WITH_VIDEO_INPUT` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 4 | const | `SEEDANCE_TOKENS_PER_SECOND_720P` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 5 | const | `PER_TOKENS` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 7 | fn | `estimate_seedance_cost_cny` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 19 | fn | `seedance_cost_cny` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 40 | fn | `seedance_cost_uses_total_tokens_and_video_input_rate` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 50 | fn | `seedance_cost_is_none_without_positive_tokens` |  |
| `rust/crates/ncx-video-agent/src/pricing.rs` | 56 | fn | `seedance_estimate_matches_measured_tokens_per_second` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 1 | module | `render` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 12 | struct | `SeedanceSubmitInput` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 22 | struct | `SeedanceArtifactInput` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 33 | struct | `SeedanceArtifactOutput` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 41 | enum | `SeedancePollOutcome` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 54 | trait | `VideoDownloader` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 56 | fn | `download` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 58 | struct | `ReqwestVideoDownloader` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 64 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 73 | fn | `download` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 94 | fn | `submit_seedance_job_once` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 112 | fn | `poll_seedance_job_once` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 156 | fn | `persist_seedance_video_artifact` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 197 | fn | `artifact_params_json` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 218 | fn | `hex_sha256` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 234 | struct | `ScriptedArk` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 240 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 253 | struct | `FakeDownloader` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 256 | fn | `download` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 263 | struct | `FakeTos` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 268 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 277 | fn | `seeded_db` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 298 | fn | `seedance_submit_uses_jobs_idempotency_layer` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 328 | fn | `seedance_poll_once_updates_running_then_succeeded_job_status` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 402 | fn | `seedance_poll_failure_releases_budget` |  |
| `rust/crates/ncx-video-agent/src/render.rs` | 445 | fn | `seedance_artifact_downloads_uploads_and_records_traceable_artifact` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 1 | module | `reverse_prompt` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 23 | const | `SYSTEM_PROMPT` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 49 | const | `REQUIRED_FIELDS` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 62 | const | `VL_REQUIRED_FIELDS` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 77 | fn | `encode_frame` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 83 | fn | `build_vl_messages` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 119 | struct | `VlEndpoint` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 127 | fn | `from_config` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 144 | fn | `chat_url` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 156 | fn | `request_reverse_prompt` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 195 | fn | `send_once` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 240 | fn | `extract_json_object` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 245 | fn | `truncate` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 255 | fn | `validate_vl_payload` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 276 | const | `OCR_SYSTEM_PROMPT` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 288 | fn | `request_subtitle_ocr` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 346 | fn | `build_vl_messages_has_system_and_all_image_parts` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 364 | fn | `extract_json_object_strips_fences_and_prose` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 372 | fn | `extract_json_object_none_when_absent` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 377 | fn | `from_config_errors_when_unset` |  |
| `rust/crates/ncx-video-agent/src/reverse_prompt.rs` | 390 | fn | `encode_frame_roundtrips` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 1 | module | `runtime_config` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 4 | struct | `ResolvedSetting` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 10 | struct | `P1ExternalConfig` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 19 | fn | `load` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 60 | fn | `resolve_setting` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 90 | fn | `env_lookup` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 100 | fn | `config_value_wins_before_env_fallback` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 114 | fn | `env_fallback_supports_runbook_key_aliases` |  |
| `rust/crates/ncx-video-agent/src/runtime_config.rs` | 125 | fn | `blank_values_are_missing` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 1 | module | `structured` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 14 | enum | `AgentArtifactKind` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 23 | fn | `stage` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 32 | fn | `artifact_kind` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 45 | struct | `StructuredValidationReport` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 53 | fn | `pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 61 | fn | `repair` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 71 | fn | `validate_brief_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 114 | fn | `validate_chapters_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 165 | fn | `validate_shots_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 296 | fn | `validate_assets_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 351 | fn | `validate_system_prompt_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 375 | fn | `record_structured_validation_if_pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 404 | fn | `record_structured_agent_validation_if_pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 448 | fn | `json_content_hash` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 461 | fn | `chapter_budgets_from_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 475 | fn | `shot_ids_from_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 488 | fn | `insert_project_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 510 | fn | `finish` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 522 | fn | `required_string` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 526 | fn | `is_boundary_reference` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 530 | fn | `node_kind_name` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 537 | fn | `reasoning_mode_name` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 545 | fn | `positive_or_null` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 553 | fn | `canonical_json` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 596 | fn | `structured_chain_validates_brief_chapters_shots_and_assets` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 620 | fn | `system_prompt_validator_requires_all_six_dimensions_and_prompt` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 654 | fn | `shots_validator_rejects_duration_reference_and_missing_routing_fields` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 676 | fn | `shots_validator_requires_reciprocal_continuity_closure` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 691 | fn | `shots_validator_rejects_self_referential_continuity` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 705 | fn | `invalid_artifact_does_not_get_a_pass_record` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 724 | fn | `agent_validation_records_context_packet_contract_evidence` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 774 | fn | `invalid_agent_artifact_does_not_get_a_pass_record` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 1 | module | `text_separation` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 6 | struct | `OverlayText` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 13 | struct | `ShotTextSpec` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 22 | struct | `TtsRequest` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 29 | struct | `SeparatedShot` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 36 | fn | `separate_text_and_voice` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 84 | fn | `write_srt` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 102 | fn | `enforce_no_text_overlays` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 113 | fn | `normalized_overlays` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 137 | fn | `srt_timestamp` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 148 | fn | `sanitize_filename` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 175 | fn | `text_and_voice_are_split_from_generation_prompt` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 204 | fn | `no_text_shot_has_no_srt_but_still_blocks_text_overlays` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 224 | fn | `invalid_overlay_time_is_rejected` |  |
| `rust/crates/ncx-video-agent/src/text_separation.rs` | 244 | fn | `srt_timestamp_rounds_milliseconds` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 1 | module | `tos` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 10 | const | `SERVICE` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 12 | const | `ALGORITHM` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 13 | const | `AMZ_DATE_FORMAT` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 15 | const | `DATE_FORMAT` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 18 | struct | `TosConfig` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 27 | struct | `TosObjectRef` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 36 | struct | `TosRequest` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 44 | struct | `TosResponse` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 49 | trait | `TosTransport` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 51 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 53 | struct | `ReqwestTosTransport` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 59 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 68 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 101 | struct | `TosClient` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 109 | fn | `from_env` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 112 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 141 | fn | `from_lookup` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 188 | fn | `new` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 195 | fn | `put_object` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 219 | fn | `get_object` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 231 | fn | `delete_object` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 243 | fn | `send_signed` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 260 | fn | `sign_request` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 322 | fn | `signing_key` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 329 | fn | `hmac_sha256` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 335 | fn | `hmac_sha256_hex` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 339 | fn | `hex_sha256` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 344 | fn | `hex_bytes` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 348 | fn | `canonical_uri` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 363 | fn | `percent_encode_path_segment` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 375 | fn | `normalize_endpoint` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 389 | fn | `endpoint_origin` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 403 | fn | `endpoint_authority` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 411 | fn | `endpoint_path` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 422 | fn | `parse_region_from_endpoint` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 434 | fn | `trim_header` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 438 | fn | `first_setting` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 448 | fn | `env_lookup` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 452 | fn | `missing_env` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 456 | fn | `is_success` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 460 | fn | `preview_bytes` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 462 | const | `MAX` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 476 | struct | `FakeTransport` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 482 | fn | `send` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 524 | fn | `fixed_now` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 528 | fn | `test_client` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 546 | fn | `tos_put_get_delete_roundtrip_uses_sigv4_headers` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 574 | fn | `tos_config_normalizes_endpoint_and_parses_region` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 585 | fn | `tos_config_lookup_supports_aws_aliases_and_region_inference` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 604 | fn | `tos_config_lookup_prefers_tos_aliases_and_explicit_region` |  |
| `rust/crates/ncx-video-agent/src/tos.rs` | 628 | fn | `tos_rejects_empty_key` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 1 | module | `trace` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 7 | struct | `ShotTrace` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 13 | fn | `export_project_trace` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 46 | fn | `export_project_budget` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 81 | fn | `export_project_shot_trace` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 115 | fn | `export_project_artifacts` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 141 | fn | `export_shot_trace` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 231 | fn | `export_artifact_validations_for_row` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 262 | fn | `query_json_rows` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 277 | fn | `query_json_rows_without_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 308 | fn | `trace_exports_jobs_artifacts_and_validation_by_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 418 | fn | `trace_exports_only_project_owned_artifacts` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 456 | fn | `project_shot_trace_rejects_cross_project_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 479 | fn | `trace_exports_live_seedance_tos_shape_for_strict_verifier` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1 | module | `transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 14 | const | `DEFAULT_ASR_MODEL` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 16 | const | `FILETRANS_ASR_MODEL` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 17 | const | `DEFAULT_TRANSCRIPT_PLACEHOLDER` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 18 | const | `MAX_DATA_URI_AUDIO_BYTES` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 19 | const | `MAX_INLINE_AUDIO_DURATION_SECS` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 20 | const | `TRANSCRIPTION_SEGMENT_DURATION_SECS` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 21 | const | `TRANSCRIPTION_AUDIO_SAMPLE_RATE` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 22 | const | `TRANSCRIPTION_AUDIO_BITRATE` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 23 | const | `FILETRANS_POLL_INTERVAL_SECS` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 24 | const | `FILETRANS_MIN_WAIT_SECS` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 25 | const | `FILETRANS_MAX_WAIT_SECS` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 28 | struct | `TranscriptionSegment` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 40 | struct | `TranscriptionArtifact` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 48 | struct | `PreparedTranscriptionAudio` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 54 | struct | `UploadPolicy` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 65 | struct | `AsrEndpoint` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 76 | fn | `from_config` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 91 | fn | `generation_url` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 98 | fn | `filetrans_submit_url` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 105 | fn | `filetrans_task_url` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 109 | fn | `upload_policy_url` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 116 | fn | `service_root` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 127 | fn | `transcribe_video_audio` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 141 | fn | `request_transcription_artifact` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 153 | fn | `request_transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 157 | fn | `request_transcription_prepared` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 180 | fn | `request_transcription_inner` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 270 | fn | `fallback_to_segmented_transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 287 | fn | `request_transcription_segmented` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 307 | fn | `request_transcription_filetrans` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 327 | fn | `audio_requires_filetrans` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 331 | fn | `prepare_audio_for_transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 383 | fn | `transcode_audio_for_transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 415 | fn | `request_upload_policy` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 454 | fn | `upload_audio_for_filetrans` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 508 | fn | `submit_filetrans_task` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 563 | fn | `poll_filetrans_task` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 635 | fn | `extract_transcription_url` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 660 | fn | `download_filetrans_result` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 682 | fn | `parse_filetrans_transcription_result` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 758 | fn | `collect_filetrans_transcripts` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 780 | fn | `simple_transcription_artifact` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 789 | fn | `normalize_transcript_text` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 798 | fn | `audio_duration_exceeds_inline_limit` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 804 | fn | `audio_size_exceeds_inline_limit` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 810 | fn | `probe_audio_duration_seconds` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 843 | fn | `split_audio_for_transcription` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 910 | fn | `transcription_error_suggests_filetrans` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 917 | fn | `audio_mime_from_path` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 932 | fn | `sanitized_upload_file_name` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 959 | fn | `json_string` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 969 | fn | `json_string_any` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 981 | fn | `json_number_as_f64` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 990 | fn | `require` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 996 | fn | `extract_audio_wav` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1054 | fn | `extract_text_content` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1073 | fn | `truncate` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1083 | fn | `temp_audio_path` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1094 | fn | `temp_audio_dir` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1110 | fn | `extract_text_content_supports_plain_string` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1118 | fn | `extract_text_content_supports_openai_style_parts` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1130 | fn | `asr_endpoint_reuses_vl_settings` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1155 | fn | `asr_endpoint_errors_without_vl_settings` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1167 | fn | `audio_mime_detection_supports_mp3` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1173 | fn | `transcription_error_detects_audio_too_long` |  |
| `rust/crates/ncx-video-agent/src/transcription.rs` | 1186 | fn | `parse_filetrans_transcription_result_extracts_sentences` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 1 | module | `validation` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 7 | struct | `ValidationInput` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 18 | fn | `record_validation` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 41 | fn | `validate_input` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 75 | fn | `require_nonempty` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 84 | fn | `require_unit_interval` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 93 | fn | `assert_artifacts_passed` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 119 | fn | `db_with_artifact` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 142 | fn | `downstream_contract_rejects_missing_and_non_pass_records` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 191 | fn | `record_validation_rejects_invalid_contract_inputs_before_insert` |  |
## Web

| 路径 | 行 | 类型 | 名称 | 摘要 |
| --- | ---: | --- | --- | --- |
| `rust/gui/src/App.svelte` | 1 | module | `App` |  |
| `rust/gui/src/App.svelte` | 6 | symbol | `IMAGE_EXTS` |  |
| `rust/gui/src/App.svelte` | 8 | symbol | `isImage` |  |
| `rust/gui/src/App.svelte` | 9 | symbol | `baseName` |  |
| `rust/gui/src/App.svelte` | 12 | symbol | `UiEvent` |  |
| `rust/gui/src/App.svelte` | 23 | symbol | `Approval` |  |
| `rust/gui/src/App.svelte` | 25 | symbol | `approval` |  |
| `rust/gui/src/App.svelte` | 26 | symbol | `UserQuestion` |  |
| `rust/gui/src/App.svelte` | 27 | symbol | `userQuestion` |  |
| `rust/gui/src/App.svelte` | 28 | symbol | `questionAnswer` |  |
| `rust/gui/src/App.svelte` | 29 | symbol | `Settings` |  |
| `rust/gui/src/App.svelte` | 50 | symbol | `ConfigLocation` |  |
| `rust/gui/src/App.svelte` | 54 | symbol | `settings` |  |
| `rust/gui/src/App.svelte` | 55 | symbol | `configLocation` |  |
| `rust/gui/src/App.svelte` | 56 | symbol | `apiKeyInput` |  |
| `rust/gui/src/App.svelte` | 57 | symbol | `saving` |  |
| `rust/gui/src/App.svelte` | 58 | symbol | `Checkpoint` |  |
| `rust/gui/src/App.svelte` | 67 | symbol | `RestoreReport` |  |
| `rust/gui/src/App.svelte` | 73 | symbol | `checkpointOpen` |  |
| `rust/gui/src/App.svelte` | 74 | symbol | `checkpoints` |  |
| `rust/gui/src/App.svelte` | 75 | symbol | `checkpointLabel` |  |
| `rust/gui/src/App.svelte` | 76 | symbol | `checkpointBusy` |  |
| `rust/gui/src/App.svelte` | 77 | symbol | `Msg` |  |
| `rust/gui/src/App.svelte` | 81 | symbol | `messages` |  |
| `rust/gui/src/App.svelte` | 83 | symbol | `input` |  |
| `rust/gui/src/App.svelte` | 84 | symbol | `attached` |  |
| `rust/gui/src/App.svelte` | 85 | symbol | `queued` |  |
| `rust/gui/src/App.svelte` | 86 | symbol | `busy` |  |
| `rust/gui/src/App.svelte` | 87 | symbol | `stopping` |  |
| `rust/gui/src/App.svelte` | 89 | symbol | `DirEntry` |  |
| `rust/gui/src/App.svelte` | 90 | symbol | `filesOpen` |  |
| `rust/gui/src/App.svelte` | 91 | symbol | `filesPath` |  |
| `rust/gui/src/App.svelte` | 92 | symbol | `filesEntries` |  |
| `rust/gui/src/App.svelte` | 93 | symbol | `header` |  |
| `rust/gui/src/App.svelte` | 94 | symbol | `workspace` |  |
| `rust/gui/src/App.svelte` | 95 | symbol | `needsWorkspace` |  |
| `rust/gui/src/App.svelte` | 97 | symbol | `wsName` |  |
| `rust/gui/src/App.svelte` | 100 | symbol | `sessionTitle` |  |
| `rust/gui/src/App.svelte` | 101 | symbol | `sidebarOpen` |  |
| `rust/gui/src/App.svelte` | 102 | symbol | `SIDEBAR_DEFAULT_WIDTH` |  |
| `rust/gui/src/App.svelte` | 103 | symbol | `SIDEBAR_MIN_WIDTH` |  |
| `rust/gui/src/App.svelte` | 104 | symbol | `SIDEBAR_MAX_WIDTH` |  |
| `rust/gui/src/App.svelte` | 105 | symbol | `sidebarWidth` |  |
| `rust/gui/src/App.svelte` | 106 | symbol | `sidebarResizing` |  |
| `rust/gui/src/App.svelte` | 107 | symbol | `sandboxMode` |  |
| `rust/gui/src/App.svelte` | 108 | symbol | `tokIn` |  |
| `rust/gui/src/App.svelte` | 109 | symbol | `tokOut` |  |
| `rust/gui/src/App.svelte` | 111 | symbol | `priceIn` |  |
| `rust/gui/src/App.svelte` | 112 | symbol | `priceOut` |  |
| `rust/gui/src/App.svelte` | 113 | symbol | `cost` |  |
| `rust/gui/src/App.svelte` | 114 | symbol | `streamingIdx` |  |
| `rust/gui/src/App.svelte` | 115 | symbol | `fmtTok` |  |
| `rust/gui/src/App.svelte` | 116 | symbol | `fmtCost` |  |
| `rust/gui/src/App.svelte` | 120 | symbol | `COLLAPSE_LINES` |  |
| `rust/gui/src/App.svelte` | 121 | symbol | `COLLAPSE_CHARS` |  |
| `rust/gui/src/App.svelte` | 122 | symbol | `isLong` |  |
| `rust/gui/src/App.svelte` | 124 | symbol | `lineCount` |  |
| `rust/gui/src/App.svelte` | 126 | symbol | `collapsedHint` |  |
| `rust/gui/src/App.svelte` | 127 | symbol | `lines` |  |
| `rust/gui/src/App.svelte` | 132 | symbol | `toolOutcome` |  |
| `rust/gui/src/App.svelte` | 133 | symbol | `exit` |  |
| `rust/gui/src/App.svelte` | 134 | symbol | `trimmed` |  |
| `rust/gui/src/App.svelte` | 135 | symbol | `body` |  |
| `rust/gui/src/App.svelte` | 148 | symbol | `toolStatusLabel` |  |
| `rust/gui/src/App.svelte` | 149 | symbol | `oc` |  |
| `rust/gui/src/App.svelte` | 153 | symbol | `diffLineClass` |  |
| `rust/gui/src/App.svelte` | 161 | symbol | `esc` |  |
| `rust/gui/src/App.svelte` | 164 | symbol | `inlineMd` |  |
| `rust/gui/src/App.svelte` | 175 | symbol | `renderMarkdown` |  |
| `rust/gui/src/App.svelte` | 176 | symbol | `lines` |  |
| `rust/gui/src/App.svelte` | 177 | symbol | `out` |  |
| `rust/gui/src/App.svelte` | 178 | symbol | `i` |  |
| `rust/gui/src/App.svelte` | 179 | symbol | `ul` |  |
| `rust/gui/src/App.svelte` | 180 | symbol | `closeLists` |  |
| `rust/gui/src/App.svelte` | 184 | symbol | `rowCells` |  |
| `rust/gui/src/App.svelte` | 187 | symbol | `line` |  |
| `rust/gui/src/App.svelte` | 188 | symbol | `fence` |  |
| `rust/gui/src/App.svelte` | 191 | symbol | `buf` |  |
| `rust/gui/src/App.svelte` | 200 | symbol | `headers` |  |
| `rust/gui/src/App.svelte` | 202 | symbol | `rows` |  |
| `rust/gui/src/App.svelte` | 204 | symbol | `t` |  |
| `rust/gui/src/App.svelte` | 211 | symbol | `h` |  |
| `rust/gui/src/App.svelte` | 221 | symbol | `um` |  |
| `rust/gui/src/App.svelte` | 223 | symbol | `om` |  |
| `rust/gui/src/App.svelte` | 236 | symbol | `Commit` |  |
| `rust/gui/src/App.svelte` | 237 | symbol | `branchCommits` |  |
| `rust/gui/src/App.svelte` | 238 | symbol | `toggleBranchDetail` |  |
| `rust/gui/src/App.svelte` | 250 | symbol | `checkpointFiles` |  |
| `rust/gui/src/App.svelte` | 251 | symbol | `toggleCheckpointDetail` |  |
| `rust/gui/src/App.svelte` | 263 | symbol | `toggleTool` |  |
| `rust/gui/src/App.svelte` | 266 | symbol | `rightPanel` |  |
| `rust/gui/src/App.svelte` | 267 | symbol | `PANEL_TITLES` |  |
| `rust/gui/src/App.svelte` | 270 | symbol | `currentSessionId` |  |
| `rust/gui/src/App.svelte` | 272 | symbol | `currentModel` |  |
| `rust/gui/src/App.svelte` | 273 | symbol | `models` |  |
| `rust/gui/src/App.svelte` | 274 | symbol | `modelMenuOpen` |  |
| `rust/gui/src/App.svelte` | 275 | symbol | `selectModel` |  |
| `rust/gui/src/App.svelte` | 278 | symbol | `prev` |  |
| `rust/gui/src/App.svelte` | 288 | symbol | `permissionMode` |  |
| `rust/gui/src/App.svelte` | 289 | symbol | `modeMenuOpen` |  |
| `rust/gui/src/App.svelte` | 290 | symbol | `PERMISSION_MODES` |  |
| `rust/gui/src/App.svelte` | 296 | symbol | `modeLabel` |  |
| `rust/gui/src/App.svelte` | 297 | symbol | `modeIcon` |  |
| `rust/gui/src/App.svelte` | 298 | symbol | `selectMode` |  |
| `rust/gui/src/App.svelte` | 301 | symbol | `prev` |  |
| `rust/gui/src/App.svelte` | 310 | symbol | `scroller` |  |
| `rust/gui/src/App.svelte` | 311 | symbol | `clampSidebarWidth` |  |
| `rust/gui/src/App.svelte` | 313 | symbol | `viewportMax` |  |
| `rust/gui/src/App.svelte` | 318 | symbol | `setSidebarWidth` |  |
| `rust/gui/src/App.svelte` | 325 | symbol | `stopSidebarResize` |  |
| `rust/gui/src/App.svelte` | 333 | symbol | `resizeSidebar` |  |
| `rust/gui/src/App.svelte` | 338 | symbol | `sidebarResizeStartX` |  |
| `rust/gui/src/App.svelte` | 340 | symbol | `sidebarResizeStartWidth` |  |
| `rust/gui/src/App.svelte` | 341 | symbol | `beginSidebarResize` |  |
| `rust/gui/src/App.svelte` | 351 | symbol | `handleSidebarResizeKey` |  |
| `rust/gui/src/App.svelte` | 364 | symbol | `scrollDown` |  |
| `rust/gui/src/App.svelte` | 371 | symbol | `savedWidth` |  |
| `rust/gui/src/App.svelte` | 376 | symbol | `s` |  |
| `rust/gui/src/App.svelte` | 389 | symbol | `p` |  |
| `rust/gui/src/App.svelte` | 409 | symbol | `m` |  |
| `rust/gui/src/App.svelte` | 415 | symbol | `m` |  |
| `rust/gui/src/App.svelte` | 430 | symbol | `m` |  |
| `rust/gui/src/App.svelte` | 450 | symbol | `pendingIndex` |  |
| `rust/gui/src/App.svelte` | 457 | symbol | `collapsed` |  |
| `rust/gui/src/App.svelte` | 458 | symbol | `pending` |  |
| `rust/gui/src/App.svelte` | 470 | symbol | `u` |  |
| `rust/gui/src/App.svelte` | 508 | symbol | `attachFiles` |  |
| `rust/gui/src/App.svelte` | 511 | symbol | `picked` |  |
| `rust/gui/src/App.svelte` | 513 | symbol | `paths` |  |
| `rust/gui/src/App.svelte` | 519 | symbol | `removeAttachment` |  |
| `rust/gui/src/App.svelte` | 525 | symbol | `handlePaste` |  |
| `rust/gui/src/App.svelte` | 526 | symbol | `items` |  |
| `rust/gui/src/App.svelte` | 531 | symbol | `file` |  |
| `rust/gui/src/App.svelte` | 534 | symbol | `buf` |  |
| `rust/gui/src/App.svelte` | 535 | symbol | `ext` |  |
| `rust/gui/src/App.svelte` | 536 | symbol | `path` |  |
| `rust/gui/src/App.svelte` | 546 | symbol | `filePreview` |  |
| `rust/gui/src/App.svelte` | 547 | symbol | `loadDir` |  |
| `rust/gui/src/App.svelte` | 556 | symbol | `openFiles` |  |
| `rust/gui/src/App.svelte` | 561 | symbol | `filesUp` |  |
| `rust/gui/src/App.svelte` | 564 | symbol | `parent` |  |
| `rust/gui/src/App.svelte` | 567 | symbol | `pickFile` |  |
| `rust/gui/src/App.svelte` | 573 | symbol | `content` |  |
| `rust/gui/src/App.svelte` | 579 | symbol | `insertMention` |  |
| `rust/gui/src/App.svelte` | 582 | symbol | `chooseWorkspace` |  |
| `rust/gui/src/App.svelte` | 585 | symbol | `dir` |  |
| `rust/gui/src/App.svelte` | 587 | symbol | `set` |  |
| `rust/gui/src/App.svelte` | 605 | symbol | `dispatch` |  |
| `rust/gui/src/App.svelte` | 619 | symbol | `stopGeneration` |  |
| `rust/gui/src/App.svelte` | 635 | symbol | `dequeue` |  |
| `rust/gui/src/App.svelte` | 637 | symbol | `next` |  |
| `rust/gui/src/App.svelte` | 641 | symbol | `send` |  |
| `rust/gui/src/App.svelte` | 642 | symbol | `text` |  |
| `rust/gui/src/App.svelte` | 649 | symbol | `images` |  |
| `rust/gui/src/App.svelte` | 650 | symbol | `files` |  |
| `rust/gui/src/App.svelte` | 651 | symbol | `mentions` |  |
| `rust/gui/src/App.svelte` | 652 | symbol | `fullText` |  |
| `rust/gui/src/App.svelte` | 653 | symbol | `shown` |  |
| `rust/gui/src/App.svelte` | 655 | symbol | `imgs` |  |
| `rust/gui/src/App.svelte` | 668 | symbol | `onKey` |  |
| `rust/gui/src/App.svelte` | 683 | symbol | `decide` |  |
| `rust/gui/src/App.svelte` | 686 | symbol | `id` |  |
| `rust/gui/src/App.svelte` | 694 | symbol | `answerUserQuestion` |  |
| `rust/gui/src/App.svelte` | 697 | symbol | `id` |  |
| `rust/gui/src/App.svelte` | 706 | symbol | `openSettings` |  |
| `rust/gui/src/App.svelte` | 720 | symbol | `openConfigFile` |  |
| `rust/gui/src/App.svelte` | 729 | symbol | `openConfigDir` |  |
| `rust/gui/src/App.svelte` | 738 | symbol | `saveSettings` |  |
| `rust/gui/src/App.svelte` | 742 | symbol | `updates` |  |
| `rust/gui/src/App.svelte` | 767 | symbol | `loadCheckpoints` |  |
| `rust/gui/src/App.svelte` | 771 | symbol | `openCheckpoints` |  |
| `rust/gui/src/App.svelte` | 783 | symbol | `saveCheckpoint` |  |
| `rust/gui/src/App.svelte` | 787 | symbol | `cp` |  |
| `rust/gui/src/App.svelte` | 796 | symbol | `restoreCheckpoint` |  |
| `rust/gui/src/App.svelte` | 802 | symbol | `report` |  |
| `rust/gui/src/App.svelte` | 815 | symbol | `BranchInfo` |  |
| `rust/gui/src/App.svelte` | 816 | symbol | `SessionRow` |  |
| `rust/gui/src/App.svelte` | 827 | symbol | `branchOpen` |  |
| `rust/gui/src/App.svelte` | 828 | symbol | `branches` |  |
| `rust/gui/src/App.svelte` | 829 | symbol | `newBranch` |  |
| `rust/gui/src/App.svelte` | 830 | symbol | `branchBusy` |  |
| `rust/gui/src/App.svelte` | 831 | symbol | `FileChange` |  |
| `rust/gui/src/App.svelte` | 832 | symbol | `diffOpen` |  |
| `rust/gui/src/App.svelte` | 833 | symbol | `diffFiles` |  |
| `rust/gui/src/App.svelte` | 834 | symbol | `diffOpenFiles` |  |
| `rust/gui/src/App.svelte` | 835 | symbol | `historyOpen` |  |
| `rust/gui/src/App.svelte` | 836 | symbol | `sessions` |  |
| `rust/gui/src/App.svelte` | 838 | symbol | `showArchived` |  |
| `rust/gui/src/App.svelte` | 841 | symbol | `orderedSessions` |  |
| `rust/gui/src/App.svelte` | 842 | symbol | `visible` |  |
| `rust/gui/src/App.svelte` | 851 | symbol | `archivedCount` |  |
| `rust/gui/src/App.svelte` | 854 | symbol | `fmtWhen` |  |
| `rust/gui/src/App.svelte` | 856 | symbol | `t` |  |
| `rust/gui/src/App.svelte` | 858 | symbol | `diff` |  |
| `rust/gui/src/App.svelte` | 862 | symbol | `d` |  |
| `rust/gui/src/App.svelte` | 863 | symbol | `p` |  |
| `rust/gui/src/App.svelte` | 866 | symbol | `archiveSession` |  |
| `rust/gui/src/App.svelte` | 869 | symbol | `s` |  |
| `rust/gui/src/App.svelte` | 876 | symbol | `loadBranches` |  |
| `rust/gui/src/App.svelte` | 880 | symbol | `openBranches` |  |
| `rust/gui/src/App.svelte` | 891 | symbol | `createBranch` |  |
| `rust/gui/src/App.svelte` | 904 | symbol | `switchBranch` |  |
| `rust/gui/src/App.svelte` | 916 | symbol | `openDiff` |  |
| `rust/gui/src/App.svelte` | 927 | symbol | `toggleFile` |  |
| `rust/gui/src/App.svelte` | 934 | symbol | `d` |  |
| `rust/gui/src/App.svelte` | 940 | symbol | `reloadPanel` |  |
| `rust/gui/src/App.svelte` | 951 | symbol | `refreshSessions` |  |
| `rust/gui/src/App.svelte` | 958 | symbol | `toggleSidebar` |  |
| `rust/gui/src/App.svelte` | 961 | symbol | `newSession` |  |
| `rust/gui/src/App.svelte` | 971 | symbol | `resumeSession` |  |
| `rust/gui/src/App.svelte` | 982 | symbol | `forkSession` |  |
| `rust/gui/src/App.svelte` | 993 | symbol | `openSessionLog` |  |
| `rust/gui/src/App.svelte` | 1000 | symbol | `openSessionSnapshot` |  |
| `rust/gui/src/App.svelte` | 1009 | symbol | `MemoryNote` |  |
| `rust/gui/src/App.svelte` | 1010 | symbol | `hermesOpen` |  |
| `rust/gui/src/App.svelte` | 1011 | symbol | `notes` |  |
| `rust/gui/src/App.svelte` | 1012 | symbol | `hermesBusy` |  |
| `rust/gui/src/App.svelte` | 1013 | symbol | `newNote` |  |
| `rust/gui/src/App.svelte` | 1014 | symbol | `newNoteTags` |  |
| `rust/gui/src/App.svelte` | 1015 | symbol | `loadNotes` |  |
| `rust/gui/src/App.svelte` | 1019 | symbol | `openHermes` |  |
| `rust/gui/src/App.svelte` | 1030 | symbol | `consolidateMemory` |  |
| `rust/gui/src/App.svelte` | 1033 | symbol | `removed` |  |
| `rust/gui/src/App.svelte` | 1041 | symbol | `addNote` |  |
| `rust/gui/src/App.svelte` | 1045 | symbol | `tags` |  |
| `rust/gui/src/App.svelte` | 1046 | symbol | `saved` |  |
| `rust/gui/src/App.svelte` | 1056 | symbol | `openMemoryFile` |  |
| `rust/gui/src/App.svelte` | 1063 | symbol | `fmtTs` |  |
| `rust/gui/src/App.svelte` | 1072 | symbol | `slashIdx` |  |
| `rust/gui/src/App.svelte` | 1073 | symbol | `showSlash` |  |
| `rust/gui/src/App.svelte` | 1074 | symbol | `slashFilter` |  |
| `rust/gui/src/App.svelte` | 1075 | symbol | `forkCurrent` |  |
| `rust/gui/src/App.svelte` | 1079 | symbol | `cmdUsage` |  |
| `rust/gui/src/App.svelte` | 1080 | symbol | `c` |  |
| `rust/gui/src/App.svelte` | 1083 | symbol | `cmdMcp` |  |
| `rust/gui/src/App.svelte` | 1085 | symbol | `rows` |  |
| `rust/gui/src/App.svelte` | 1096 | symbol | `cmdFeedback` |  |
| `rust/gui/src/App.svelte` | 1101 | symbol | `cmdUltrareview` |  |
| `rust/gui/src/App.svelte` | 1104 | symbol | `cmdBtw` |  |
| `rust/gui/src/App.svelte` | 1107 | symbol | `cmdSoon` |  |
| `rust/gui/src/App.svelte` | 1110 | symbol | `cmdRename` |  |
| `rust/gui/src/App.svelte` | 1115 | symbol | `customCommands` |  |
| `rust/gui/src/App.svelte` | 1116 | symbol | `slashArg` |  |
| `rust/gui/src/App.svelte` | 1117 | symbol | `loadCustomCommands` |  |
| `rust/gui/src/App.svelte` | 1124 | symbol | `runCustom` |  |
| `rust/gui/src/App.svelte` | 1126 | symbol | `expanded` |  |
| `rust/gui/src/App.svelte` | 1132 | symbol | `SlashCmd` |  |
| `rust/gui/src/App.svelte` | 1133 | symbol | `SLASH_COMMANDS` |  |
| `rust/gui/src/App.svelte` | 1152 | symbol | `slashHead` |  |
| `rust/gui/src/App.svelte` | 1153 | symbol | `customSlash` |  |
| `rust/gui/src/App.svelte` | 1161 | symbol | `slashMatches` |  |
| `rust/gui/src/App.svelte` | 1173 | symbol | `runSlash` |  |
| `rust/gui/src/main.ts` | 1 | module | `main` |  |
| `rust/gui/src/main.ts` | 4 | symbol | `app` |  |
