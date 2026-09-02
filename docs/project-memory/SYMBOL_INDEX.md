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
| `nanocodex/cli.py` | 52 | class | `_SubcommandFirstGroup` | Route a leading subcommand name to the subcommand, not the ``task`` arg. The root callback has an optional positional ``task`` (so ``nanocodex "fix the bug"`` works as a one-shot). |
| `nanocodex/cli.py` | 64 | function | `parse_args` |  |
| `nanocodex/cli.py` | 90 | function | `_build_console_approver` | An Approver whose callback prompts the console with y/n. |
| `nanocodex/cli.py` | 93 | async function | `_ask` |  |
| `nanocodex/cli.py` | 119 | function | `_make_hooks` |  |
| `nanocodex/cli.py` | 124 | function | `_emit` |  |
| `nanocodex/cli.py` | 133 | async function | `on_reasoning_delta` |  |
| `nanocodex/cli.py` | 141 | async function | `on_content_delta` |  |
| `nanocodex/cli.py` | 148 | async function | `on_stream_end` |  |
| `nanocodex/cli.py` | 153 | async function | `on_assistant_text` |  |
| `nanocodex/cli.py` | 158 | async function | `on_tool_start` |  |
| `nanocodex/cli.py` | 162 | async function | `on_tool_result` |  |
| `nanocodex/cli.py` | 177 | function | `_summarize_call` |  |
| `nanocodex/cli.py` | 197 | function | `_build_loop` | Build an AgentLoop. *log_path* controls where the session transcript is persisted: - ``_UNSET`` (default): the usual ``workspace/.nanocodex/session.jsonl``. - ``None``: do NOT pers |
| `nanocodex/cli.py` | 280 | function | `_print_banner` |  |
| `nanocodex/cli.py` | 297 | function | `_build_orchestrator` | Build an OrchestratorLoop sharing _build_loop's config/provider/ctx setup. Returns (orchestrator, cfg). The orchestrator builds its own role-scoped registries per worker, so we han |
| `nanocodex/cli.py` | 346 | function | `_print_orchestrator_result` | Render the final task graph + verdicts after an orchestrated run. |
| `nanocodex/cli.py` | 372 | function | `orchestrate_cmd` | Run the multi-agent orchestrator: plan -> execute -> verify -> replan. |
| `nanocodex/cli.py` | 417 | function | `main` |  |
| `nanocodex/cli.py` | 503 | async function | `_orchestrate` | Connect MCP (if requested), run the task/REPL, then tear MCP down. |
| `nanocodex/cli.py` | 527 | async function | `_connect_mcp` | Discover + connect MCP servers, registering their tools onto the loop. |
| `nanocodex/cli.py` | 548 | async function | `_run_once` |  |
| `nanocodex/cli.py` | 559 | async function | `_repl` |  |
| `nanocodex/cli.py` | 594 | async function | `_dispatch_slash` | Handle a REPL slash command. Returns True only for /exit (quit the REPL). Read-only commands (/help /status /diff /plan) just print; the mutating ones (/model /approvals /compact / |
| `nanocodex/cli.py` | 698 | async function | `_run_loop_command` | `/loop [interval] <prompt>`: re-run a prompt on an interval until Ctrl+C. Ad-hoc, in-session, no persistence — complements the (cron-like, unattended) scheduler. The interval accep |
| `nanocodex/cli.py` | 737 | function | `_print_plan` |  |
| `nanocodex/cli.py` | 749 | function | `_schedule_store` |  |
| `nanocodex/cli.py` | 755 | function | `schedule_add` | Add a scheduled task. |
| `nanocodex/cli.py` | 797 | function | `schedule_list` | List all scheduled tasks. |
| `nanocodex/cli.py` | 813 | function | `schedule_remove` | Remove a scheduled task. |
| `nanocodex/cli.py` | 820 | function | `schedule_enable` | Enable a scheduled task. |
| `nanocodex/cli.py` | 827 | function | `schedule_disable` | Disable a scheduled task. |
| `nanocodex/cli.py` | 834 | function | `schedule_run` | Run the scheduler: wait for tasks to come due and execute them. This is a long-running foreground process (Ctrl+C to stop). Each due task runs as one agent turn in the given worksp |
| `nanocodex/cli.py` | 860 | async function | `run_task` |  |
| `nanocodex/cli.py` | 866 | function | `factory` |  |
| `nanocodex/cli.py` | 869 | function | `factory` |  |
| `nanocodex/cli.py` | 881 | function | `_on_event` |  |
| `nanocodex/cli.py` | 890 | function | `datetime_now` |  |
| `nanocodex/cli.py` | 895 | function | `_auto_deny_approver` | Approver for unattended runs: never grants escalation (no human present). |
| `nanocodex/cli.py` | 897 | async function | `_deny` |  |
| `nanocodex/cli.py` | 902 | function | `_desktop_only_approver` | Unattended approver that grants ONLY desktop (MCP) actions, nothing else. Security model for allow_desktop tasks. The trick is the policy choice: * Under ``never`` the MCP gate aut |
| `nanocodex/cli.py` | 919 | async function | `_allow_mcp_only` |  |
| `nanocodex/cli.py` | 924 | function | `_auto_approve_approver` | Approver that grants every escalation without asking. Used for A/B comparison runs: each side runs inside its OWN throwaway git worktree, so file writes are already isolated from t |
| `nanocodex/cli.py` | 935 | async function | `_allow_all` |  |
| `nanocodex/cli.py` | 950 | function | `storyboard_run` | Plan a storyboard from a story file + image directory, exporting JSON (and optionally video). |
| `nanocodex/cli.py` | 1070 | function | `_progress` |  |
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
| `nanocodex/gui.py` | 1016 | function | `_register_bridged_tool` | Register an MCP tool whose execute() is dispatched to the MCP loop. The agent runs each turn on its own loop; an MCP tool's coroutine is bound to the MCP loop, so we hop threads vi |
| `nanocodex/gui.py` | 1027 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1036 | function | `_autostart_scheduler` | Start the in-GUI schedule runner once, on a dedicated thread. GUI users never run `nanocodex schedule run`; this hosts it for them so a due task fires with no manual step. Honors t |
| `nanocodex/gui.py` | 1065 | function | `_scheduler_thread_main` | Own an event loop and poll the ScheduleStore until told to stop. |
| `nanocodex/gui.py` | 1091 | async function | `_scheduler_run_task` | Run ONE due task unattended (on the scheduler loop). Concurrency: the GUI conversation and a scheduled task drive the SAME mouse/keyboard, so they must never overlap. The desktop l |
| `nanocodex/gui.py` | 1115 | async function | `_run` |  |
| `nanocodex/gui.py` | 1177 | async function | `_attach_scheduler_mcp_tools` | Give the scheduled task's loop the MCP desktop tools, bridged. Rebuilds the tools against the TASK loop's ctx (desktop-only approver), not the GUI's, so the approval gate is the un |
| `nanocodex/gui.py` | 1198 | function | `_bridge` |  |
| `nanocodex/gui.py` | 1199 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1207 | function | `_reattach_mcp_tools` | Re-register the live MCP tools onto the current (rebuilt) loop. ``_autoconnect_mcp`` connects once per session on a long-lived MCP thread; it returns early on every later ``_init_l |
| `nanocodex/gui.py` | 1243 | function | `_bridge` |  |
| `nanocodex/gui.py` | 1244 | async function | `bridged_execute` |  |
| `nanocodex/gui.py` | 1257 | function | `_scheduler_log` | Append a line to ~/.nanocodex/scheduler.log (best-effort, UTF-8). Unattended runs deliberately do NOT touch the transcript; this file is their only record. |
| `nanocodex/gui.py` | 1270 | function | `_on_toggle_scheduler` | Flip the managed scheduler on/off and persist the choice. |
| `nanocodex/gui.py` | 1291 | function | `_on_open_project` | Pick a folder, rebuild the loop in it, and reset the transcript. |
| `nanocodex/gui.py` | 1309 | function | `_on_new_session` | Start a clean conversation in the current workspace. |
| `nanocodex/gui.py` | 1316 | function | `_start_new_session` | Mint a fresh session_id, clear the transcript, and rebuild the loop. |
| `nanocodex/gui.py` | 1339 | function | `_on_pick_model` | Open a menu of available models; switching rebuilds the loop. |
| `nanocodex/gui.py` | 1363 | function | `_switch_model` |  |
| `nanocodex/gui.py` | 1373 | function | `_refresh_plugin_list` | Redraw the server rows in the plugin manager from mcp.toml. |
| `nanocodex/gui.py` | 1409 | function | `_remove` |  |
| `nanocodex/gui.py` | 1414 | function | `_toggle` |  |
| `nanocodex/gui.py` | 1433 | function | `_open_settings` | Codex-style settings window: a left nav list switches right sections. Folds the old standalone Settings dialog AND the MCP plugin manager into one window with four sections (Genera |
| `nanocodex/gui.py` | 1491 | function | `_settings_show_section` | Repaint the content frame with *name*'s section; re-highlight nav. |
| `nanocodex/gui.py` | 1520 | function | `_settings_section_header` | Shared title + subtitle for a settings section (tuple pads on pack). |
| `nanocodex/gui.py` | 1532 | function | `_settings_section_general` | Read-only workspace + model overview (model is changed in Config). |
| `nanocodex/gui.py` | 1585 | function | `_on_pick_theme` | Switch the UI theme: persist it, rebuild every widget, restore the transcript text, and reopen Settings on this section. Rebuilding is how the ~300 ``palette[...]`` lookups re-colo |
| `nanocodex/gui.py` | 1644 | function | `_settings_section_config` | Editable config: API key / base URL / model / sandbox / approval / reasoning, persisted to ~/.nanocodex/config.toml and applied via rebuild. |
| `nanocodex/gui.py` | 1674 | function | `_label` |  |
| `nanocodex/gui.py` | 1679 | function | `_entry` |  |
| `nanocodex/gui.py` | 1689 | function | `_option` |  |
| `nanocodex/gui.py` | 1776 | function | `_do_save` |  |
| `nanocodex/gui.py` | 1825 | function | `_settings_section_mcp` | MCP server CRUD, folded in from the old plugin manager. Reuses _refresh_plugin_list (it renders into self._plugin_list_frame). Edits persist immediately but only connect on the NEX |
| `nanocodex/gui.py` | 1856 | function | `_row` |  |
| `nanocodex/gui.py` | 1874 | function | `_do_add` |  |
| `nanocodex/gui.py` | 1905 | function | `_settings_section_marketplace` | Browse + one-click install MCP servers from a built-in catalog and (optionally) a remote URL. Both sources install through the SAME McpStore the "MCP servers" section uses, so an i |
| `nanocodex/gui.py` | 1964 | function | `_installed_server_names` | Names already in mcp.toml (so the marketplace can mark them installed). |
| `nanocodex/gui.py` | 1972 | function | `_render_catalog_row` | Draw one catalog entry row with name/source/description + Install. |
| `nanocodex/gui.py` | 1992 | function | `_install` |  |
| `nanocodex/gui.py` | 2001 | function | `_refresh_marketplace_local` | Redraw the built-in catalog rows. |
| `nanocodex/gui.py` | 2013 | function | `_refresh_marketplace_remote` | Redraw the remote catalog rows from a fetched entry list. |
| `nanocodex/gui.py` | 2031 | function | `_on_marketplace_refresh` | Fetch the remote catalog in a background thread (never blocks the UI). |
| `nanocodex/gui.py` | 2049 | function | `_run_marketplace_fetch` | Worker: fetch+parse remote catalog, hand the result back to the main thread via root.after (Tk-safe). Errors are reported, never crash. |
| `nanocodex/gui.py` | 2060 | function | `_marketplace_fetch_done` | Main-thread callback after a remote fetch finishes. |
| `nanocodex/gui.py` | 2080 | function | `_install_marketplace_entry` | Install a catalog entry. If it needs a path or env values, prompt for them in a small modal first; otherwise install immediately. |
| `nanocodex/gui.py` | 2088 | function | `_prompt_marketplace_install` | Modal collecting the machine-specific path and/or env values an entry needs before install. Uses Entry widgets (env values masked). |
| `nanocodex/gui.py` | 2132 | function | `_submit` |  |
| `nanocodex/gui.py` | 2155 | function | `_do_marketplace_install` | Funnel an install through marketplace.install_entry → McpStore. Returns (ok, message). On success refreshes both the marketplace rows and the MCP servers list (if that section's fr |
| `nanocodex/gui.py` | 2180 | function | `_settings_section_schedule` | Manual CRUD over scheduled tasks (the SAME ScheduleStore the model's manage_schedule tool and the CLI use). Lets the user add/enable/disable/ remove tasks visually instead of only  |
| `nanocodex/gui.py` | 2215 | function | `_label` |  |
| `nanocodex/gui.py` | 2220 | function | `_entry` |  |
| `nanocodex/gui.py` | 2270 | function | `_do_add` |  |
| `nanocodex/gui.py` | 2307 | function | `_refresh_schedule_mgr` | Redraw the task rows in the Scheduled-tasks settings section. Each row shows the task's prompt + recurrence summary with Enable/Disable and Remove buttons, mutating the shared Sche |
| `nanocodex/gui.py` | 2355 | function | `_remove` |  |
| `nanocodex/gui.py` | 2361 | function | `_toggle` |  |
| `nanocodex/gui.py` | 2379 | function | `_settings_section_desktop` | Read-only mirror of desktop-control state (toggles live in the top bar). nanocodex's desktop control runs through MCP (windows-computer-use-mcp) under approval gating. The live swi |
| `nanocodex/gui.py` | 2396 | function | `_state_row` |  |
| `nanocodex/gui.py` | 2421 | async function | `_approve_via_ui` | Approver callback (runs on the worker loop). Blocks on the UI. Short-circuits without a dialog when the global auto-approve toggle is on, when the user previously chose "allow all  |
| `nanocodex/gui.py` | 2448 | function | `_show_approval_dialog` |  |
| `nanocodex/gui.py` | 2468 | function | `_decide` |  |
| `nanocodex/gui.py` | 2475 | function | `dlg_btn` |  |
| `nanocodex/gui.py` | 2538 | function | `_on_continue` | Resume an unfinished turn (hit step-limit / paused mid-plan) with one click, instead of making the user type 'continue'. |
| `nanocodex/gui.py` | 2552 | function | `_on_send` |  |
| `nanocodex/gui.py` | 2595 | function | `_autogrow_entry` | Grow/shrink the composer to fit its content, within [min, max] rows. Counts the displayed lines and clamps the Text height to that range; past the max the box scrolls internally. B |
| `nanocodex/gui.py` | 2618 | function | `_quick_capture_memory` | Append `note` to user memory (the `# ...` composer shortcut). Best-effort and synchronous: writing one bullet to a local file is instant, so no worker thread. Never raises into the |
| `nanocodex/gui.py` | 2637 | function | `_on_attach` | 📎 button: pick local files to attach to the NEXT message. Images become OpenAI multimodal blocks (only seen by a vision-capable model); other (text-like) files are read and inlined |
| `nanocodex/gui.py` | 2664 | function | `_refresh_attach_label` | Show the pending attachment count on the 📎 button (cosmetic). |
| `nanocodex/gui.py` | 2675 | function | `_consume_attachments` | Fold pending attachments into the message content, then clear them. Returns a plain string when there are no images (text-only / no files), or an OpenAI multimodal block list when  |
| `nanocodex/gui.py` | 2717 | function | `_on_enhance` | ✨ button: rewrite the composer text into a clearer prompt. Reads the current input, kicks off a background rewrite (the model call must not block the UI), and on completion shows a |
| `nanocodex/gui.py` | 2746 | function | `_run_enhance_thread` | Daemon thread: one provider.chat to rewrite *text*; result to the queue. Mirrors _run_turn_thread's "own asyncio loop, post results via the UI queue" shape, but it's a single state |
| `nanocodex/gui.py` | 2767 | function | `_refresh_enhance_label` | Reflect the in-flight state on the ✨ button (cosmetic, never crashes). |
| `nanocodex/gui.py` | 2777 | function | `_show_enhance_dialog` | Preview the rewrite; let the user use it, keep the original, or cancel. A rewrite NEVER silently replaces the user's words — they pick here. 'Use rewrite' replaces the composer tex |
| `nanocodex/gui.py` | 2807 | function | `_use` |  |
| `nanocodex/gui.py` | 2814 | function | `dlg_btn` |  |
| `nanocodex/gui.py` | 2856 | function | `_flat_btn` | Class-level twin of the _build_widgets-local flat_btn closure, so dialogs built outside _build_widgets (storyboard panel) get the same flat, palette-colored button without re-defin |
| `nanocodex/gui.py` | 2872 | function | `_handle_storyboard_command` | `/storyboard` composer command. `render` -> render the previewed state; anything else -> open the panel (prefilling+auto-previewing the story text when given). |
| `nanocodex/gui.py` | 2882 | function | `_open_storyboard_panel` | Open (or focus) the dedicated storyboard panel. Single-instance, like Settings: a story box + image picker + aspect ratio on top, a read-only two-level (chapters / shots) preview i |
| `nanocodex/gui.py` | 3092 | function | `_on_close` |  |
| `nanocodex/gui.py` | 3103 | function | `_sb_memory_path` | Where the panel persists its inputs (story + images + ratio). |
| `nanocodex/gui.py` | 3108 | function | `_sb_save_memory` | Persist the panel's story + picked images + ratio (best-effort). Lets reopening the panel (or relaunching the app) restore what you had instead of starting blank. Any failure is sw |
| `nanocodex/gui.py` | 3144 | function | `_sb_load_memory` | Load the panel's last-saved inputs; {} when none/unreadable. |
| `nanocodex/gui.py` | 3157 | function | `_sb_render_thumbs` | Show small thumbnails of the picked reference images under the picker. Uses Pillow to decode any format (PNG/JPEG/webp/…) and downscale to a ~64px-tall thumbnail. PhotoImage refs a |
| `nanocodex/gui.py` | 3205 | function | `_sb_pick_images` | Pick reference images for the storyboard (optional). |
| `nanocodex/gui.py` | 3221 | function | `_sb_set_status` |  |
| `nanocodex/gui.py` | 3233 | function | `_sb_build_obj` | Build the schema project dict from the panel's story + images. |
| `nanocodex/gui.py` | 3254 | function | `_sb_build_deps` | Wire pipeline deps from layered config (planner/chapters always; vision only with a VL backend + images; seedance only when rendering). Raises with a clear message when a needed ke |
| `nanocodex/gui.py` | 3293 | function | `_sb_run_preview` | [生成预览]: plan chapters + shots on a worker thread (never renders). |
| `nanocodex/gui.py` | 3311 | function | `_sb_preview_thread` | Daemon thread: run_planning over its own asyncio loop; result to queue. |
| `nanocodex/gui.py` | 3321 | function | `_sb_run_render` | [出片]: render the previewed state after an explicit confirm (COSTS $$). |
| `nanocodex/gui.py` | 3355 | function | `_sb_render_thread` | Daemon thread: render the planned state via Seedance, download the finished clips locally, then export. Results to the UI queue. Each render gets its OWN archived directory under s |
| `nanocodex/gui.py` | 3376 | function | `_prog` |  |
| `nanocodex/gui.py` | 3390 | function | `_sb_run_meta` | Build the index row for one 出片 run (id/title/time/counts/cost/dir). |
| `nanocodex/gui.py` | 3409 | function | `_sb_download_clips` | Download each successful shot's signed URL to ``out_dir/<shot_id>.mp4``. Signed Seedance URLs expire (~24h), so a local copy makes 播放 reliable. Best-effort: a download failure leav |
| `nanocodex/gui.py` | 3431 | function | `_sb_rerender_one` | Re-render a single (usually failed) shot after a per-shot confirm. |
| `nanocodex/gui.py` | 3458 | function | `_sb_rerender_thread` | Daemon thread: re-render ONE shot in place, download, export, refresh. A 重试 belongs to the SAME run as the original render (it's补 the failed shot, not a new history entry): reuse s |
| `nanocodex/gui.py` | 3480 | function | `_prog` |  |
| `nanocodex/gui.py` | 3491 | function | `_sb_show_preview` | Render the chapters + shots into the read-only preview, set cost. |
| `nanocodex/gui.py` | 3575 | function | `_sb_show_render_done` | Show per-shot results + actual cost after a render; fold cost in. Keeps the rendered state so a failed shot can be re-generated on its own, then (re)builds the per-shot result list |
| `nanocodex/gui.py` | 3614 | function | `_sb_render_results` | (Re)build the per-shot result rows: status + ▶播放 / ↻重试. A succeeded shot shows ✓ + its title + ▶播放 (opens the local mp4, or the signed URL as fallback). A failed shot shows ✗ + the |
| `nanocodex/gui.py` | 3694 | function | `_sb_play_clip` | Open a rendered clip: prefer the local mp4, fall back to the URL. Uses the OS default handler (os.startfile on Windows) so the system video player opens it. A signed URL is the fal |
| `nanocodex/gui.py` | 3722 | function | `_sb_run_merge` | Pre-check continuity, THEN (after the user OKs) stitch into full.mp4. Merging hard-cuts the clips in shot order, so a storyboard with missing transitions plays as a jumpy story. Be |
| `nanocodex/gui.py` | 3748 | function | `_sb_check_thread` | Daemon thread: run the continuity check over its own asyncio loop. Mirrors _sb_preview_thread. ``available`` is the same filter the rest of the panel uses (a shot has a clip iff it |
| `nanocodex/gui.py` | 3769 | function | `_sb_open_merge_progress` | Show a small modal progress dialog (reused for both check & merge). The bar runs in indeterminate (marquee) mode — neither the DeepSeek check nor ffmpeg's concat give a clean perce |
| `nanocodex/gui.py` | 3814 | function | `_sb_close_merge_progress` | Tear down the merge progress dialog (idempotent: done OR error). |
| `nanocodex/gui.py` | 3835 | function | `_sb_show_continuity_report` | Show the pre-merge continuity report; let the user 补镜 / 合并 / 取消. Built like _sb_open_merge_progress (palette Toplevel) with a scrollable list of gaps. Each gap carries a 「补这镜」butto |
| `nanocodex/gui.py` | 3899 | function | `_gap_key` |  |
| `nanocodex/gui.py` | 3924 | function | `_line` |  |
| `nanocodex/gui.py` | 3954 | function | `_cancel` |  |
| `nanocodex/gui.py` | 3964 | function | `_proceed` |  |
| `nanocodex/gui.py` | 3992 | function | `_sb_fill_one` | Adopt one gap suggestion as a real shot and render it (real spend). Confirms cost, splices the suggestion into ``state`` via insert_fill_shot (so it lands between the two shots it  |
| `nanocodex/gui.py` | 4040 | function | `_sb_fill_thread` | Daemon thread: render ONE fill shot, download, export, refresh index. The fill shot belongs to the SAME run as the originals (it補 a gap, not a new history entry): reuse run_dir and |
| `nanocodex/gui.py` | 4056 | function | `_prog` |  |
| `nanocodex/gui.py` | 4069 | function | `_sb_proceed_merge` | Run the actual ffmpeg concat after the continuity report was OK'd. This is the back-half of the old _sb_run_merge (split out so the check gates it). _sb_busy is already set (held s |
| `nanocodex/gui.py` | 4082 | function | `_sb_merge_thread` | Daemon thread: run concat_clips, report the merged path (or error). |
| `nanocodex/gui.py` | 4092 | function | `_sb_show_merge_done` | Report the merged full video; surface it as a persistent 整片 row. Closes the progress dialog, re-enables 合并, and re-renders the result rows so the 整片 row appears (it's rendered when |
| `nanocodex/gui.py` | 4118 | function | `_sb_show_history` | Popup listing past 出片 runs from storyboard_out/runs/index.json. Each row: time · title · ok/总镜 · ¥cost, with a 载入 button that reopens that run in the panel (replay clips / retry fa |
| `nanocodex/gui.py` | 4182 | function | `_sb_load_run` | Reload a past run's exported state into the panel for replay/retry/merge. Rebuilds the PipelineState from the run dir's JSON (via load_run_state), points the panel at that dir so 播 |
| `nanocodex/gui.py` | 4244 | function | `_sb_open_dir` | Open a run's archived directory in the OS file manager. |
| `nanocodex/gui.py` | 4262 | function | `_on_ab_compare` | Open the A/B setup dialog: two configs + one prompt, run isolated. Disabled while busy (an A/B run rebuilds loops and drives files, same as a turn). Requires a clean git workspace  |
| `nanocodex/gui.py` | 4285 | function | `_show_ab_setup_dialog` | Two columns of config controls + a shared prompt box + Run button. |
| `nanocodex/gui.py` | 4331 | function | `_make_column` |  |
| `nanocodex/gui.py` | 4340 | function | `_opt` |  |
| `nanocodex/gui.py` | 4376 | function | `_overrides_from` |  |
| `nanocodex/gui.py` | 4384 | function | `_run` |  |
| `nanocodex/gui.py` | 4394 | function | `ab_btn` |  |
| `nanocodex/gui.py` | 4408 | function | `_start_ab_run` | Kick off the A/B worker thread (mirrors _start_turn's setup). |
| `nanocodex/gui.py` | 4426 | function | `_run_ab_thread` | Daemon thread: run both sides serially in isolated worktrees. Mirrors _run_turn_thread (own asyncio loop, desktop lock, results via the UI queue). Worktrees are NOT cleaned here —  |
| `nanocodex/gui.py` | 4465 | function | `_run_side` |  |
| `nanocodex/gui.py` | 4500 | function | `_show_ab_result_dialog` | Show both sides' summary + diff; adopt one or discard both. Adopting applies the chosen side's diff onto the real workspace; then BOTH worktrees are cleaned up. Discarding cleans b |
| `nanocodex/gui.py` | 4530 | function | `_cleanup_both` |  |
| `nanocodex/gui.py` | 4536 | function | `_adopt` |  |
| `nanocodex/gui.py` | 4550 | function | `_discard` |  |
| `nanocodex/gui.py` | 4558 | function | `rb` |  |
| `nanocodex/gui.py` | 4598 | function | `_start_turn` | Echo the prompt and kick off a worker turn for it (idle path). Shared by _on_send (when not busy) and the queue drain at turn end, so the 'echo + clear cancel + busy + spawn worker |
| `nanocodex/gui.py` | 4625 | function | `_drain_queue` | At turn end, start the next queued input if any (main thread). Stop only cancels the CURRENT turn (the user's choice); the queue keeps going, so a cancelled turn still hands off to |
| `nanocodex/gui.py` | 4639 | function | `_refresh_send_label` | Update the Send button text to reflect the queue backlog. |
| `nanocodex/gui.py` | 4649 | function | `_request_stop` | Ask the running turn to stop at its next cancellation point. |
| `nanocodex/gui.py` | 4657 | function | `_run_turn_thread` | Runs on a daemon thread; owns its own asyncio loop for this turn. |
| `nanocodex/gui.py` | 4691 | function | `_handle_loop_command` | `/loop [interval] <prompt>`: repeat a prompt on an interval until Stop. Ad-hoc, in-session, no persistence — complements the (cron-like) scheduler. The interval accepts 30s / 5m /  |
| `nanocodex/gui.py` | 4717 | function | `_run_loop_thread` | Daemon thread: re-run `prompt` every `interval_s`s until Stop. Mirrors _run_turn_thread per iteration (own asyncio loop, desktop lock, results via the UI queue), then waits the int |
| `nanocodex/gui.py` | 4771 | function | `_make_gui_hooks` |  |
| `nanocodex/gui.py` | 4778 | async function | `on_reasoning` |  |
| `nanocodex/gui.py` | 4781 | async function | `on_content` |  |
| `nanocodex/gui.py` | 4784 | async function | `on_stream_end` |  |
| `nanocodex/gui.py` | 4787 | async function | `on_tool_start` |  |
| `nanocodex/gui.py` | 4800 | async function | `on_tool_result` |  |
| `nanocodex/gui.py` | 4820 | function | `_poll_queue` |  |
| `nanocodex/gui.py` | 4829 | function | `_handle_event` |  |
| `nanocodex/gui.py` | 4934 | function | `_record_session_index` | Upsert this workspace's summary into the global session directory. Runs on the main thread at turn end (the session message list is stable then). Best-effort: a directory-index fai |
| `nanocodex/gui.py` | 4960 | function | `_refresh_session_list` | Repopulate the sidebar from the global index, newest activity first. Runs on the main thread (turn end / startup). Best-effort: a listing failure must never disturb the conversatio |
| `nanocodex/gui.py` | 5000 | function | `_refresh_schedule_panel` | Repaint the Scheduled panel from the store + the live running flag. Runs on the main thread (slow timer / toggle / startup). Everything but "running now" comes from ~/.nanocodex/sc |
| `nanocodex/gui.py` | 5040 | function | `_start_schedule_panel_refresh` | Arm the slow Scheduled-panel repaint loop (once). Separate from the 40ms _poll_queue: the panel only needs to track the running dot + next/last times, so a ~3s cadence is plenty an |
| `nanocodex/gui.py` | 5053 | function | `_tick` |  |
| `nanocodex/gui.py` | 5060 | function | `_on_session_select` | Replay the selected conversation: a summary header + the FULL frozen transcript (when a snapshot exists). Read-only: this surfaces the stored digest plus the complete message histo |
| `nanocodex/gui.py` | 5158 | function | `_render_transcript` | Render a frozen message list into the replay Text widget (read-only). Skips the system prompt (scaffolding, not conversation); shows each user/assistant message and a compact one-l |
| `nanocodex/gui.py` | 5189 | function | `_continue_session` | Fork the selected past conversation into a NEW one and continue it. Non-destructive: the original session's snapshot/log are untouched. We load its frozen transcript, mint a FRESH  |
| `nanocodex/gui.py` | 5244 | function | `_echo_seed_transcript` | Replay inherited messages into the MAIN panel (not the replay popup). Mirrors _render_transcript's role mapping but writes to self._append so the continued thread looks like the li |
| `nanocodex/gui.py` | 5272 | function | `_render_plan` |  |
| `nanocodex/gui.py` | 5279 | function | `_record_turn_cost` | Price this turn's usage and fold it into the running session total. Uses the REAL usage the provider reported (summed across the turn's model calls in loop.run_turn), priced via pr |
| `nanocodex/gui.py` | 5301 | function | `_announce_turn_end` | Say WHY the turn ended, so a mid-task stop is never a silent mystery. result is a TurnResult or None (None = an exception already reported). |
| `nanocodex/gui.py` | 5346 | function | `_append` |  |
| `nanocodex/gui.py` | 5352 | function | `_set_busy` |  |
| `nanocodex/gui.py` | 5362 | function | `run` |  |
| `nanocodex/gui.py` | 5373 | function | `_summarize` |  |
| `nanocodex/gui.py` | 5396 | function | `_summarize_desktop` | Human-readable description of one desktop action, for the live view. |
| `nanocodex/gui.py` | 5431 | function | `_line_gutter` | Right-aligned line-number gutter; blanks when the number is absent. |
| `nanocodex/gui.py` | 5438 | function | `_classify_patch_file` | Turn one parsed FileAction into a render-ready dict of classified rows. Pure: consumes nanocodex.tools.patch data only, touches no Tk and no disk. Caps total rows at _FILE_PANEL_MA |
| `nanocodex/gui.py` | 5449 | function | `add_row` |  |
| `nanocodex/gui.py` | 5497 | function | `_build_file_edit_payload` | Parse a V4A patch into a Tk-free payload for the file panel. Returns None on a parse error or a no-op patch (every file has zero rows), so a malformed or empty patch never blanks a |
| `nanocodex/gui.py` | 5517 | function | `_is_mcp_command` | An approval request whose 'command' is an MCP tool name (mcp__<srv>__<tool>). MCP desktop tools post their tool NAME as the approval command (see McpTool._gate_decision), so this d |
| `nanocodex/gui.py` | 5527 | function | `_approval_short_circuit` | Pure decision: may this approval request skip the dialog and auto-approve? Mirrors Codex's "approve for session" semantics. Returns True when: * global auto-approve is on (everythi |
| `nanocodex/gui.py` | 5550 | function | `_scheduler_run_plan` | Pure decision for how the managed scheduler runs one task. Returns ``(approver_kind, attach_mcp_tools)`` where ``approver_kind`` is ``"desktop_only"`` or ``"auto_deny"``. The whole |
| `nanocodex/gui.py` | 5582 | function | `_scheduler_turn_timeout` | Resolve the scheduled-turn timeout (env override, else the default). |
| `nanocodex/gui.py` | 5594 | async function | `_run_scheduled_turn` | Run ONE unattended scheduled turn under *lock*, bounded by a timeout. Tk-free and fully injectable so it unit-tests offline: * ``lock`` — a ``threading.Lock``-like (``acquire(block |
| `nanocodex/gui.py` | 5627 | function | `cancel_check` |  |
| `nanocodex/gui.py` | 5639 | async function | `_soft_deadline` |  |
| `nanocodex/gui.py` | 5673 | function | `_now_iso` | Current local time as an ISO second-precision string (for log lines). |
| `nanocodex/gui.py` | 5679 | function | `_format_scheduler_log_entry` | Format one ~/.nanocodex/scheduler.log line (pure; timestamp injected). Unattended runs never touch the transcript (user's decision), so this file is the only record. Kept Tk-free a |
| `nanocodex/gui.py` | 5699 | function | `_hhmm` | Pull HH:MM out of an ISO timestamp for compact display; tolerate junk. |
| `nanocodex/gui.py` | 5706 | function | `_format_schedule_panel_line` | Format one scheduled task into a 1-2 line sidebar panel block (pure). Tk-free + clock-free so it unit-tests deterministically. Layout: <glyph> <label> [desktop] <state/next/last/×r |
| `nanocodex/gui.py` | 5748 | function | `_settings_sections` | Ordered nav entries for the Settings window (Codex-style sections). Pure (no Tk) so the navigation order can be unit-tested. The strings double as both the nav-button labels and th |
| `nanocodex/gui.py` | 5765 | function | `_collect_schedule_add` | Coerce raw Scheduled-tasks form fields into ScheduleStore.add() kwargs. Pure (no Tk) so it unit-tests cleanly, and it mirrors exactly what the conversational manage_schedule tool d |
| `nanocodex/gui.py` | 5781 | function | `_int` |  |
| `nanocodex/gui.py` | 5798 | function | `_format_schedule_recurrence` | One-line recurrence summary for a task row (pure, unit-testable). once -> "once" interval -> "every Ns" (or a friendlier "every Nm"/"every Nh" for round minute/hour periods, so a 3 |
| `nanocodex/gui.py` | 5823 | function | `_collect_settings_updates` | Build the updates dict for write_nanocodex_config from raw field values. Pure (no Tk) so it unit-tests cleanly. Rules: * A blank new API key / VL key is OMITTED — an empty submit m |
| `nanocodex/gui.py` | 5865 | function | `_send_button_label` | Text for the Send button given how many inputs are QUEUED behind the running turn (Codex-style: you can type the next task while one runs). Pure so it unit-tests without Tk: * 0 qu |
| `nanocodex/gui.py` | 5881 | function | `_fmt_tok` | Format a token count like Claude Code: 666, 12.3k, 1.0M. |
| `nanocodex/gui.py` | 5890 | function | `_fmt_usd` | Format a USD cost. Sub-cent turns are common (a cache-hit prompt costs fractions of a cent), so show 4 decimals under $1 and 2 above — a flat ``$0.00`` would hide every cheap turn. |
| `nanocodex/gui.py` | 5901 | function | `_fmt_cny` | Format a CNY cost. Seedance clips cost a few yuan each, so 2 decimals is plenty; sub-cent rounding isn't a concern as it is for USD turns. |
| `nanocodex/gui.py` | 5909 | function | `_build_status` | Pure status-bar text builder (no Tk) so it can be unit-tested. Always shows state; shows the error if the loop failed to build (so the bar is never mysteriously blank); otherwise s |
| `nanocodex/gui.py` | 5945 | function | `launch` |  |
| `nanocodex/gui.py` | 5949 | function | `main_cli` | Console entry point for ``nanocodex-gui``. Thin argparse front end (Typer isn't needed here): supports the same workspace / sandbox / approval / model / resume knobs as the CLI, th |
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
| `nanocodex/storyboard/clients.py` | 41 | function | `_load_prompt` |  |
| `nanocodex/storyboard/clients.py` | 48 | class | `ChatProvider` | The subset of provider/deepseek.py:DeepSeekProvider we rely on. |
| `nanocodex/storyboard/clients.py` | 53 | async function | `chat` |  |
| `nanocodex/storyboard/clients.py` | 64 | function | `_extract_json` | Pull the first JSON object/array out of a model reply. Models often wrap JSON in prose or ```json fences. Be lenient: strip fences, then grab the outermost {...} or [...]. Raises V |
| `nanocodex/storyboard/clients.py` | 94 | class | `VisionAnalyzer` | Analyze one image into an AssetAnalysis via a vision-capable provider. |
| `nanocodex/storyboard/clients.py` | 97 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 101 | async function | `analyze` |  |
| `nanocodex/storyboard/clients.py` | 123 | function | `_chapters_for_prompt` | Render chapters as a compact numbered outline for the shot-planner prompt. Returns "(none)" when there are no chapters, so the prompt's fallback branch (plan straight from the full |
| `nanocodex/storyboard/clients.py` | 148 | class | `ChapterPlanner` | Split a story into chapters (the story-detail layer above shots). |
| `nanocodex/storyboard/clients.py` | 151 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 155 | async function | `plan` |  |
| `nanocodex/storyboard/clients.py` | 190 | class | `TextPlanner` | Turn story text into a list of Shot objects via the main provider. |
| `nanocodex/storyboard/clients.py` | 193 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 197 | async function | `plan` |  |
| `nanocodex/storyboard/clients.py` | 249 | function | `_shots_for_prompt` | Render shots as a compact ordered outline for the continuity prompt. One block per shot (shot_id · title, then its 中文画面 prompt_zh — or the English prompt as a fallback — and any di |
| `nanocodex/storyboard/clients.py` | 271 | function | `_available_for_prompt` | Describe which shots actually have a rendered clip (vs. real gaps). ``available_ids is None`` means the caller is checking at planning time with no render yet — return "(all)" so t |
| `nanocodex/storyboard/clients.py` | 292 | class | `ContinuityChecker` | Flag missing story beats between consecutive shots (pre-merge review). Unlike the planners, an EMPTY result is a valid good outcome: clean shots return ``ok=True`` with no gaps and |
| `nanocodex/storyboard/clients.py` | 301 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 305 | async function | `check` |  |
| `nanocodex/storyboard/clients.py` | 364 | function | `_urllib_transport` | Default transport: stdlib urllib (no extra deps), used in production. |
| `nanocodex/storyboard/clients.py` | 381 | class | `SeedanceError` | Raised when a Seedance task fails to submit or render. |
| `nanocodex/storyboard/clients.py` | 386 | class | `SeedanceResult` | Outcome of a finished Seedance task. Carries the signed ``video_url`` plus the raw ``usage`` dict from the task response. The live API returns ``usage.total_tokens`` on success (ve |
| `nanocodex/storyboard/clients.py` | 399 | class | `SeedanceClient` | Submit a video task to ARK and poll until it renders. The ARK video API is asynchronous: ``submit`` returns a task id, then you ``poll`` that id until status is ``succeeded`` (then |
| `nanocodex/storyboard/clients.py` | 410 | function | `__init__` |  |
| `nanocodex/storyboard/clients.py` | 421 | function | `_headers` |  |
| `nanocodex/storyboard/clients.py` | 427 | function | `submit` | POST a generation task; return its task id. |
| `nanocodex/storyboard/clients.py` | 440 | function | `poll_once` | GET task status once. Return (status, video_url_or_empty, usage). ``usage`` is the raw usage dict from the response (``{}`` if absent). On success it carries ``total_tokens``, whic |
| `nanocodex/storyboard/clients.py` | 463 | function | `generate` | Submit then poll until the video is ready; return a SeedanceResult. The result carries the signed video URL plus the response ``usage`` dict (with ``total_tokens`` for billing). Ra |
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
| `nanocodex/storyboard/pipeline.py` | 50 | class | `PipelineDeps` | Injected capabilities. Any may be None when its stage is not exercised. Tests pass fakes; production wires real clients (clients.py). Keeping them optional lets the offline tests r |
| `nanocodex/storyboard/pipeline.py` | 65 | class | `PipelineState` | The running project as it accretes through the stages. |
| `nanocodex/storyboard/pipeline.py` | 81 | function | `_payload_has_video_input` | True if a Seedance payload's content includes a VIDEO reference block. Seedance charges a cheaper rate when the INPUT contains video (22 vs 37 CNY/1M). This pipeline currently send |
| `nanocodex/storyboard/pipeline.py` | 104 | function | `ingest` | Validate the raw project dict and build the initial state. |
| `nanocodex/storyboard/pipeline.py` | 115 | async function | `analyze_assets` | Run the vision analyzer over every input image. |
| `nanocodex/storyboard/pipeline.py` | 129 | async function | `plan_chapters` | Split the story into chapters (3-8) BEFORE it is broken into shots. Skipped when no chapter planner is injected (offline tests / callers that don't want the chapter layer), in whic |
| `nanocodex/storyboard/pipeline.py` | 148 | async function | `plan_storyboard` | Turn the story text into shots via the text planner. When chapters were planned, they are passed through so shots are sliced chapter by chapter (continuity preserved); otherwise th |
| `nanocodex/storyboard/pipeline.py` | 184 | function | `scan_multi_action_shots` | Flag shots whose text suggests MULTIPLE ordered actions in one shot. Returns ``{shot_id: [matched markers]}`` for every shot whose ``camera`` / ``prompt`` / ``prompt_zh`` contains  |
| `nanocodex/storyboard/pipeline.py` | 215 | async function | `check_continuity` | Review the planned shots for missing story beats before a merge. Standalone — NOT part of :func:`run_planning` (it never spends and never blocks rendering): the GUI calls it just b |
| `nanocodex/storyboard/pipeline.py` | 239 | function | `_classify` | Decide whether an image is a character or a background. Prefer the user-declared ``kind`` from the input; otherwise infer from the VL ``usable_for`` / ``scene_tags`` tags. Defaults |
| `nanocodex/storyboard/pipeline.py` | 254 | function | `map_assets` | Attach background/character image ids to each shot (rule-based MVP). The MVP rule: split images into character vs background buckets (by declared kind, else VL tags), then give eve |
| `nanocodex/storyboard/pipeline.py` | 284 | function | `_build_shot_payload` | Assemble ONE Seedance payload for a single shot. Shared by :func:`build_payloads` (the whole storyboard) and :func:`insert_fill_shot` (one補镜 added after the fact), so a fill-in sho |
| `nanocodex/storyboard/pipeline.py` | 308 | function | `_ref_url` | Turn a reference-image source into something ARK accepts. ARK's ``image_url`` takes a fetchable URL or a base64 data URI — NOT a local disk path (that returns HTTP 400 InvalidParam |
| `nanocodex/storyboard/pipeline.py` | 373 | function | `build_payloads` | Assemble one Seedance payload per shot. Mirrors the ARK content-shape verified live: a text block (prompt) plus optional reference_image blocks (first character + first background) |
| `nanocodex/storyboard/pipeline.py` | 393 | function | `_unique_fill_id` | Pick a fresh shot_id for a 补镜 wedged after ``after_id``. ``shot_03`` → ``shot_03b`` (then ``shot_03c`` …) so the id sorts/reads right between ``after_id`` and the next shot. Falls  |
| `nanocodex/storyboard/pipeline.py` | 411 | function | `insert_fill_shot` | Adopt one continuity-gap suggestion as a REAL shot, inserted in order. Turns a :class:`ContinuityGap`'s 补镜 suggestion into a :class:`Shot`, gives it a fresh id wedged right after ` |
| `nanocodex/storyboard/pipeline.py` | 466 | function | `_set_first_frame` | Make ``frame_uri`` the shot's ARK ``first_frame`` reference image. Removes any existing image_url blocks (the subject ``reference_image`` plus any earlier first_frame) before addin |
| `nanocodex/storyboard/pipeline.py` | 490 | function | `_default_frame_extractor` | Extract a clip's LAST frame as a base64 JPEG data URI (None on failure). ffmpeg reads the (signed) video URL directly — no local download needed — seeks 1s before the end (``-sseof |
| `nanocodex/storyboard/pipeline.py` | 526 | function | `_render_chained` | Render shots IN ORDER, threading each shot's last frame into the next. For 画面前后衔接: shot N renders, its last frame is extracted and injected as shot N+1's ``first_frame`` (baked int |
| `nanocodex/storyboard/pipeline.py` | 559 | function | `render_one` | Render (or RE-render) a single shot by id, updating *state* in place. Returns True on success (``video_urls[shot_id]`` is a real URL), False on failure (``video_urls[shot_id]`` hol |
| `nanocodex/storyboard/pipeline.py` | 575 | function | `_cb` |  |
| `nanocodex/storyboard/pipeline.py` | 602 | function | `render` | Render each shot's payload to a video via Seedance (OPT-IN). Only called when the caller explicitly enables rendering. Each clip is real spend, so failures on one shot are recorded |
| `nanocodex/storyboard/pipeline.py` | 659 | function | `export` | Write asset_analysis / storyboard / seedance_payloads / video urls to json. Returns the paths written. Video URLs are signed + expire (~24h) — noted in the urls file so a stale lin |
| `nanocodex/storyboard/pipeline.py` | 726 | function | `_slug_title` | Turn a story title into a filesystem-safe slug of at most ``max_len`` chars. Illegal chars → "_", runs of whitespace → "_", trimmed of leading/trailing separators. Empty/blank titl |
| `nanocodex/storyboard/pipeline.py` | 739 | function | `make_run_dir` | Create and return a unique run directory ``<base>/runs/<ts>_<slug>/``. ``ts`` is ``YYYYMMDD-HHMM`` (local time) so runs sort chronologically; the slug is the cleaned story title (< |
| `nanocodex/storyboard/pipeline.py` | 766 | function | `write_run_index` | Append a run summary to ``<base>/runs/index.json`` (append-only history). The index is a list of run-meta dicts (newest appended last). A run is keyed by ``run_id``: writing the sa |
| `nanocodex/storyboard/pipeline.py` | 795 | function | `read_run_index` | Read ``<base>/runs/index.json`` into a list; [] when missing/unreadable. |
| `nanocodex/storyboard/pipeline.py` | 817 | function | `_dataclass_from_dict` | Build a dataclass instance from a dict, keeping only known fields. Unknown keys are dropped and missing keys fall back to the dataclass's own defaults, so JSON written by an older  |
| `nanocodex/storyboard/pipeline.py` | 832 | function | `load_run_state` | Reconstruct a :class:`PipelineState` from a run's exported JSON files. Reads chapters/asset_analysis/storyboard/seedance_payloads/video_urls/ video_cost from ``run_dir`` (each miss |
| `nanocodex/storyboard/pipeline.py` | 846 | function | `_read_json` |  |
| `nanocodex/storyboard/pipeline.py` | 855 | function | `_rows` |  |
| `nanocodex/storyboard/pipeline.py` | 913 | function | `_default_runner` | Default Runner: run argv, capture combined output (no shell). On Windows each ffmpeg/ffprobe child would otherwise flash its own black console window (a concat probes every clip th |
| `nanocodex/storyboard/pipeline.py` | 934 | function | `_ffprobe_params` | Probe (width, height, avg_frame_rate) via ffprobe; None on any failure. |
| `nanocodex/storyboard/pipeline.py` | 953 | function | `concat_clips` | Stitch a run's shot clips into one ``<run_dir>/<dest_name>``, in order. Picks ``<run_dir>/<shot_id>.mp4`` for each id in ``shot_ids`` THAT EXISTS (missing shots — e.g. ones that fa |
| `nanocodex/storyboard/pipeline.py` | 1020 | async function | `run_planning` | Run the PLANNING half only: ingest → analyze → chapters → shots → map → payloads. NEVER renders (never spends money). This is the "preview" path. Returns the planned state (chapter |
| `nanocodex/storyboard/pipeline.py` | 1040 | function | `render_state` | Render an ALREADY-PLANNED state (the "make video" path). Call this on a state returned by :func:`run_planning` once the user has reviewed the preview and chosen to spend. Runs the  |
| `nanocodex/storyboard/pipeline.py` | 1061 | async function | `run_pipeline` | Run all stages in order. Returns (final_state, exported_paths). ``render_video`` defaults False — Seedance billing is opt-in. ``out_dir`` None skips the export write (used by tests |
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
| `nanocodex/tools/storyboard_tool.py` | 27 | class | `StoryboardTool` |  |
| `nanocodex/tools/storyboard_tool.py` | 29 | function | `name` |  |
| `nanocodex/tools/storyboard_tool.py` | 33 | function | `description` |  |
| `nanocodex/tools/storyboard_tool.py` | 47 | function | `parameters` |  |
| `nanocodex/tools/storyboard_tool.py` | 76 | async function | `execute` |  |
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
| `rust/crates/ncx-app-server/src/adapter.rs` | 1 | module | `adapter` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 6 | trait | `AppServerAdapter` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 10 | fn | `validate_harness_profile` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 17 | fn | `create_thread` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 19 | fn | `activate_thread` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 20 | fn | `fork_thread` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 25 | fn | `submit_turn` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 32 | fn | `interrupt_latest` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 36 | fn | `continue_goal` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 43 | fn | `runtime_status` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 44 | fn | `refresh_ready` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 45 | fn | `set_workspace` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 46 | fn | `approve` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 52 | fn | `answer` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 58 | fn | `read_settings` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 59 | fn | `update_settings` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 63 | fn | `set_model` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 66 | fn | `set_permission_mode` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 71 | fn | `read_model_catalog` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 72 | fn | `apply_model_preset` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 77 | fn | `list_custom_providers` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 80 | fn | `save_custom_provider` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 91 | fn | `delete_custom_provider` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 94 | fn | `discover_custom_provider_models` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 97 | fn | `activate_custom_provider` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 100 | fn | `probe_custom_provider_chat` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 107 | fn | `harness_diagnostics` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 108 | fn | `list_external_plugins` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 109 | fn | `install_external_plugin` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 114 | fn | `set_external_plugin_enabled` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 115 | fn | `list_memory` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 116 | fn | `add_memory` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 122 | fn | `consolidate_memory` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 123 | fn | `start_memory_merge` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 126 | fn | `memory_merge_status` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 133 | fn | `cancel_memory_merge` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 140 | fn | `forge_runtime_status` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 149 | fn | `start_forge_job` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 161 | fn | `forge_job_status` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 168 | fn | `cancel_forge_job` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 175 | fn | `list_codex_plugins` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 176 | fn | `install_codex_plugin` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 181 | fn | `set_codex_plugin_enabled` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 182 | fn | `uninstall_codex_plugin` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 183 | fn | `list_marketplaces` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 184 | fn | `install_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 190 | fn | `search_dsh_marketplace` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 198 | fn | `preview_dsh_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/adapter.rs` | 204 | fn | `install_dsh_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 1 | module | `goal_driver` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 9 | enum | `GoalRoundDriveOutcome` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 23 | struct | `GoalRoundDriver` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 28 | fn | `new` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 31 | fn | `reserve_next` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 110 | fn | `cancel_reserved` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 130 | fn | `fail_reserved` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 161 | fn | `read` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 173 | fn | `finish_turn` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 191 | fn | `goal_ref` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 198 | fn | `expect_goal` |  |
| `rust/crates/ncx-app-server/src/goal_driver.rs` | 207 | fn | `render_prompt` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 1 | module | `goal_driver_tests` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 3 | fn | `armed_goal` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 41 | fn | `read_goal` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 55 | fn | `checkpoint_failure_disarms_without_admitting_a_round` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 82 | fn | `goal_change_during_checkpoint_prevents_stale_reservation` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 105 | fn | `ordinary_turn_claimed_during_checkpoint_wins_over_goal_round` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 126 | fn | `cancelled_reserved_round_pauses_and_disarms_goal` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 151 | fn | `exhausted_round_limit_blocks_without_reserving_another_turn` |  |
| `rust/crates/ncx-app-server/src/goal_driver_tests.rs` | 180 | fn | `queue_failure_finishes_turn_and_blocks_goal_without_raw_error` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 1 | module | `goal_operations` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 8 | fn | `dispatch` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 135 | fn | `start_round` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 194 | fn | `create` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 241 | enum | `ActivationUpdate` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 245 | fn | `mutate` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 282 | fn | `clear` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 300 | fn | `changed` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 340 | fn | `advance` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 345 | fn | `validate_definition` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 362 | fn | `validate_block_reason` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 369 | fn | `require_not_complete` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 376 | fn | `require_phase` |  |
| `rust/crates/ncx-app-server/src/goal_operations.rs` | 387 | fn | `invalid` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 19 | struct | `AppServer` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 43 | fn | `pending_activation_thread_ids` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 97 | fn | `new` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 107 | fn | `dispatch` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 164 | fn | `dispatch_thread_creation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 231 | fn | `update_thread_metadata` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 254 | fn | `set_harness_profile_if_idle` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 268 | fn | `read_thread` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 277 | fn | `outcome` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 290 | fn | `dispatch_with_runtime` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 617 | fn | `ack` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 621 | fn | `create_thread_for_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 657 | fn | `fork_thread_for_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 693 | fn | `compensate_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 717 | fn | `mark_runtime_activation_if_persisted` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 726 | fn | `begin_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 739 | fn | `finish_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 745 | fn | `reject_pending_activation` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 765 | fn | `response` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 769 | fn | `event` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 789 | fn | `lock_goal_transition` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 794 | fn | `lock_goal_activations` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 802 | fn | `goal_view_with_activations` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 836 | fn | `disarm_goal` |  |
| `rust/crates/ncx-app-server/src/lib.rs` | 845 | fn | `disarm_goal_if_matches` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 1 | module | `outcome` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 7 | struct | `DispatchOutcome` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 13 | enum | `AppServerError` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 22 | fn | `fmt` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 31 | fn | `ensure_import_is_idle` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 48 | fn | `from` |  |
| `rust/crates/ncx-app-server/src/outcome.rs` | 54 | fn | `from` |  |
| `rust/crates/ncx-app-server/src/profile_race_tests.rs` | 1 | module | `profile_race_tests` |  |
| `rust/crates/ncx-app-server/src/profile_race_tests.rs` | 4 | fn | `harness_profile_change_loses_to_a_first_turn_that_starts_during_validation` |  |
| `rust/crates/ncx-app-server/src/runtime_memory_tests.rs` | 1 | module | `runtime_memory_tests` |  |
| `rust/crates/ncx-app-server/src/runtime_memory_tests.rs` | 4 | fn | `memory_service_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_memory_tests.rs` | 73 | fn | `job_status_and_cancellation_keep_workspace_and_generation_at_the_adapter_boundary` |  |
| `rust/crates/ncx-app-server/src/runtime_operations.rs` | 1 | module | `runtime_operations` |  |
| `rust/crates/ncx-app-server/src/runtime_operations.rs` | 7 | fn | `requires_runtime_adapter` |  |
| `rust/crates/ncx-app-server/src/runtime_operations.rs` | 49 | fn | `set_harness_profile` |  |
| `rust/crates/ncx-app-server/src/runtime_operations.rs` | 68 | fn | `dispatch` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 1 | module | `runtime_tests` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 4 | fn | `runtime_goal_resume_arms_and_schedules_the_exact_thread` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 56 | fn | `runtime_goal_resume_failure_revokes_process_authority` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 105 | fn | `runtime_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 171 | fn | `failed_runtime_create_is_compensated_and_same_id_can_retry` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 210 | fn | `pending_runtime_create_rejects_a_concurrent_activation_before_compensation` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 252 | fn | `assert_cross_process_runtime_handoff_keeps_provisioned_thread` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 306 | fn | `cross_process_runtime_handoffs_keep_a_thread_when_the_provisioning_host_fails` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 346 | fn | `failed_runtime_fork_removes_target_thread_context_and_goal` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 401 | fn | `pending_runtime_fork_rejects_activation_of_the_new_target` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 456 | fn | `harness_profile_uses_the_last_serialized_selection_before_the_first_turn` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 526 | fn | `invalid_harness_profile_does_not_create_a_thread` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 547 | fn | `interaction_and_desktop_runtime_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 605 | fn | `settings_and_model_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 687 | fn | `permission_mode_requires_a_durable_thread_target` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 704 | fn | `custom_provider_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 781 | fn | `codex_plugin_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 828 | fn | `marketplace_requests_are_routed_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/runtime_tests.rs` | 897 | fn | `harness_diagnostics_and_external_plugins_use_the_same_protocol_boundary` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 6 | static | `TEST_SEQUENCE` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 8 | fn | `server` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 27 | struct | `ProfileValidationGate` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 34 | struct | `ProfileValidationGateState` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 40 | fn | `wait_until_entered` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 46 | fn | `release` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 52 | fn | `block_validation` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 64 | struct | `RecordingRuntime` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 77 | fn | `validate_harness_profile` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 92 | fn | `create_thread` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 103 | fn | `activate_thread` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 111 | fn | `fork_thread` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 125 | fn | `submit_turn` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 139 | fn | `interrupt_latest` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 147 | fn | `continue_goal` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 163 | fn | `runtime_status` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 167 | fn | `refresh_ready` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 172 | fn | `set_workspace` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 177 | fn | `approve` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 190 | fn | `answer` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 204 | fn | `read_settings` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 208 | fn | `update_settings` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 219 | fn | `set_model` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 224 | fn | `set_permission_mode` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 232 | fn | `read_model_catalog` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 236 | fn | `apply_model_preset` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 248 | fn | `list_custom_providers` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 252 | fn | `save_custom_provider` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 269 | fn | `delete_custom_provider` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 277 | fn | `discover_custom_provider_models` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 285 | fn | `activate_custom_provider` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 293 | fn | `probe_custom_provider_chat` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 305 | fn | `harness_diagnostics` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 309 | fn | `list_external_plugins` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 313 | fn | `install_external_plugin` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 325 | fn | `set_external_plugin_enabled` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 333 | fn | `list_memory` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 341 | fn | `add_memory` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 354 | fn | `consolidate_memory` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 362 | fn | `start_memory_merge` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 370 | fn | `memory_merge_status` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 387 | fn | `cancel_memory_merge` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 399 | fn | `forge_job_status` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 416 | fn | `cancel_forge_job` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 428 | fn | `list_codex_plugins` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 432 | fn | `install_codex_plugin` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 444 | fn | `set_codex_plugin_enabled` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 452 | fn | `uninstall_codex_plugin` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 460 | fn | `list_marketplaces` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 464 | fn | `install_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 476 | fn | `search_dsh_marketplace` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 489 | fn | `preview_dsh_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/tests.rs` | 500 | fn | `install_dsh_marketplace_plugin` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 1 | module | `thread_operations` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 8 | fn | `dispatch_metadata` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 37 | fn | `rename` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 51 | fn | `fork` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 73 | fn | `dispatch_model_context` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 106 | fn | `dispatch_turn` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 135 | fn | `start_turn` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 166 | fn | `finish_turn` |  |
| `rust/crates/ncx-app-server/src/thread_operations.rs` | 192 | fn | `dispatch_item` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 1 | module | `thread_tests` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 6 | fn | `create_and_start_turn_emit_owned_v3_events_and_mode` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 47 | fn | `second_concurrent_turn_is_rejected` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 77 | fn | `rename_and_fork_are_owned_by_the_app_server` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 113 | fn | `model_context_is_replaced_without_rewriting_visible_turns` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 144 | fn | `visible_thread_never_returns_tool_logs_or_intermediate_assistant_text` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 209 | fn | `goal_lifecycle_is_revisioned_and_emits_durable_snapshots` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 289 | fn | `goal_transition_lock_serializes_pause_against_an_in_flight_request` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 363 | fn | `stale_or_invalid_goal_transition_performs_no_mutation` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 418 | fn | `completed_goal_can_be_replaced_but_active_goal_cannot` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 472 | fn | `goal_activation_is_process_local_and_fork_never_inherits_it` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 543 | fn | `goal_activation_token_cannot_authorize_a_replacement_from_another_server` |  |
| `rust/crates/ncx-app-server/src/thread_tests.rs` | 664 | fn | `goal_round_requires_armed_exact_identity_and_claims_the_turn_atomically` |  |
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
| `rust/crates/ncx-cli/src/cli_app.rs` | 1 | module | `cli_app` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 2 | fn | `run` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 48 | fn | `load_cli_config` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 71 | fn | `early_exit` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 107 | fn | `validate_cli_config` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 119 | fn | `build_registry` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 176 | fn | `attach_mcp` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 224 | fn | `open_agent` |  |
| `rust/crates/ncx-cli/src/cli_app.rs` | 253 | fn | `run_prompt` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 1 | module | `command_support` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 2 | fn | `render_skills` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 19 | fn | `render_help` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 27 | fn | `render_help_for_workspace` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 45 | fn | `render_status` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 79 | fn | `export_session_text` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 125 | fn | `export_target_path` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 147 | fn | `render_session_markdown` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 218 | fn | `push_block` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 228 | fn | `code_fence` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 244 | fn | `push_fenced` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 256 | fn | `count_roles` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 273 | fn | `content_to_markdown` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 296 | fn | `scope_suffix` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 305 | fn | `review_prompt` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 316 | fn | `security_review_prompt` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 327 | fn | `verify_prompt` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 345 | fn | `doc_backend_hint` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 355 | fn | `doc_prompt` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 372 | fn | `config_text` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 377 | fn | `config_text_at` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 412 | fn | `render_config_overview` |  |
| `rust/crates/ncx-cli/src/command_support.rs` | 426 | fn | `parse_config_assignment` |  |
| `rust/crates/ncx-cli/src/main.rs` | 1 | module | `main` |  |
| `rust/crates/ncx-cli/src/main.rs` | 51 | const | `SYSTEM_PROMPT` |  |
| `rust/crates/ncx-cli/src/main.rs` | 58 | const | `PLAN_MODE_NOTE` |  |
| `rust/crates/ncx-cli/src/main.rs` | 62 | fn | `main` |  |
| `rust/crates/ncx-cli/src/main.rs` | 94 | fn | `run_orchestrated` |  |
| `rust/crates/ncx-cli/src/main.rs` | 153 | fn | `dump_genome_toml` |  |
| `rust/crates/ncx-cli/src/main.rs` | 174 | fn | `toml_escape` |  |
| `rust/crates/ncx-cli/src/main.rs` | 191 | fn | `repl` |  |
| `rust/crates/ncx-cli/src/main.rs` | 237 | fn | `run_one_turn` |  |
| `rust/crates/ncx-cli/src/main.rs` | 278 | fn | `split_inline_images` |  |
| `rust/crates/ncx-cli/src/main.rs` | 293 | enum | `SlashOutcome` |  |
| `rust/crates/ncx-cli/src/main.rs` | 303 | fn | `dispatch_slash` |  |
| `rust/crates/ncx-cli/src/main.rs` | 382 | fn | `reload_mcp_tools` |  |
| `rust/crates/ncx-cli/src/main.rs` | 423 | struct | `PreparedMcpTools` |  |
| `rust/crates/ncx-cli/src/main.rs` | 429 | fn | `prepare_configured_mcp_tools` |  |
| `rust/crates/ncx-cli/src/main.rs` | 440 | fn | `prepare_configured_mcp_tools_with` |  |
| `rust/crates/ncx-cli/src/main.rs` | 467 | fn | `report_mcp_server_failures` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 1 | module | `main_tests` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 9 | struct | `TestMcpTool` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 14 | fn | `name` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 17 | fn | `description` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 21 | fn | `parameters` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 25 | fn | `execute` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 32 | fn | `configured_mcp_preparation_skips_failed_servers_and_keeps_valid_tools` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 68 | fn | `help_lists_all_commands` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 76 | fn | `base64_matches_known_vectors` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 88 | fn | `image_input_builds_multimodal_content` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 112 | fn | `inline_images_split_from_prompt` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 125 | fn | `attachment_vision_provider_requires_model_and_explicit_plugin_enablement` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 138 | fn | `cli_and_gui_use_equivalent_runtime_profiles_for_same_config` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 157 | fn | `help_lists_custom_project_commands` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 170 | fn | `parse_config_assignment_accepts_trimmed_key_value` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 181 | fn | `usage_tracker_renders_last_and_total_usage` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 221 | fn | `config_text_writes_known_key_to_path` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 233 | fn | `config_text_rejects_unknown_key` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 244 | fn | `status_masks_api_key` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 255 | fn | `history_renders_saved_sessions` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 272 | fn | `cli_recorder_uses_protocol_store_for_turn_ownership_and_resume` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 332 | fn | `checkpoints_render_saved_entries` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 348 | fn | `export_renders_user_assistant_tool_markdown` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 379 | fn | `export_flattens_multimodal_and_hides_image_data` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 396 | fn | `export_writes_markdown_file_to_explicit_path` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 416 | fn | `export_refuses_to_overwrite_existing_explicit_file` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 437 | fn | `export_refuses_directory_arg_with_clear_message` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 454 | fn | `export_default_path_uses_session_id_under_exports` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 466 | fn | `export_uses_longer_fence_when_content_has_backticks` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 483 | fn | `review_verify_prompts_reference_diff_and_scope` |  |
| `rust/crates/ncx-cli/src/main_tests.rs` | 498 | fn | `doc_prompts_name_format_file_and_backend` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 1 | module | `runner` |  |
| `rust/crates/ncx-cli/src/runner.rs` | 5 | fn | `memory_summarizer` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 1 | module | `runtime_support` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 3 | fn | `compact_session_text` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 20 | fn | `checkpoint_before_turn` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 33 | fn | `create_checkpoint_text` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 45 | fn | `restore_checkpoint_text` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 64 | fn | `format_checkpoint_saved` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 75 | fn | `render_checkpoints` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 97 | fn | `clipped_label` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 109 | fn | `runtime_profile_for_args` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 124 | fn | `build_image_user_input` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 140 | fn | `validate_attachments` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 179 | fn | `image_mime` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 195 | fn | `base64_encode` |  |
| `rust/crates/ncx-cli/src/runtime_support.rs` | 196 | const | `T` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 1 | module | `session_recorder` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 2 | struct | `SessionRecorder` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 11 | fn | `open` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 14 | fn | `open_at` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 73 | fn | `model_context` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 79 | fn | `log_path` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 89 | fn | `start_turn` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 93 | fn | `start_turn_with_mode` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 136 | fn | `session_id` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 140 | fn | `finish_turn` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 192 | fn | `replace_model_context` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 204 | fn | `finish_external_turn` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 248 | fn | `safe_thread_file_stem` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 260 | fn | `now_epoch_millis` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 273 | fn | `emit_usage_line` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 281 | fn | `protocol_history` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 299 | fn | `render_history` |  |
| `rust/crates/ncx-cli/src/session_recorder.rs` | 318 | fn | `read_protocol_messages` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 1 | module | `usage` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 4 | struct | `UsageTracker` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 12 | struct | `TurnUsage` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 20 | fn | `record` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 31 | fn | `render` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 53 | fn | `add_usage` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 59 | fn | `format_usage_block` |  |
| `rust/crates/ncx-cli/src/usage.rs` | 87 | fn | `usage_value` |  |
| `rust/crates/ncx-config/src/config.rs` | 1 | module | `config` |  |
| `rust/crates/ncx-config/src/config.rs` | 6 | const | `DEFAULT_BASE_URL` |  |
| `rust/crates/ncx-config/src/config.rs` | 8 | const | `DEFAULT_MODEL` |  |
| `rust/crates/ncx-config/src/config.rs` | 9 | const | `DEFAULT_MODELS` |  |
| `rust/crates/ncx-config/src/config.rs` | 10 | const | `VALID_PRICE_CURRENCIES` |  |
| `rust/crates/ncx-config/src/config.rs` | 11 | const | `VALID_SANDBOX_MODES` |  |
| `rust/crates/ncx-config/src/config.rs` | 13 | const | `VALID_APPROVAL_POLICIES` |  |
| `rust/crates/ncx-config/src/config.rs` | 14 | const | `VALID_HOOK_EVENTS` |  |
| `rust/crates/ncx-config/src/config.rs` | 17 | const | `VALID_PERMISSION_MODES` |  |
| `rust/crates/ncx-config/src/config.rs` | 22 | fn | `permission_mode_to_knobs` |  |
| `rust/crates/ncx-config/src/config.rs` | 33 | fn | `derive_permission_mode` |  |
| `rust/crates/ncx-config/src/config.rs` | 43 | struct | `McpServerConfig` |  |
| `rust/crates/ncx-config/src/config.rs` | 53 | struct | `HookConfig` |  |
| `rust/crates/ncx-config/src/config.rs` | 66 | struct | `Config` |  |
| `rust/crates/ncx-config/src/config.rs` | 137 | fn | `default` |  |
| `rust/crates/ncx-config/src/config.rs` | 191 | fn | `validate` |  |
| `rust/crates/ncx-config/src/config.rs` | 252 | fn | `redacted` |  |
| `rust/crates/ncx-config/src/config.rs` | 312 | fn | `validate_orchestrator_budget` |  |
| `rust/crates/ncx-config/src/config.rs` | 349 | fn | `insert_redacted_runtime_budget` |  |
| `rust/crates/ncx-config/src/config.rs` | 376 | struct | `ConfigError` |  |
| `rust/crates/ncx-config/src/config.rs` | 379 | fn | `fmt` |  |
| `rust/crates/ncx-config/src/config.rs` | 391 | fn | `permission_mode_maps_to_knobs` |  |
| `rust/crates/ncx-config/src/config.rs` | 416 | fn | `derive_permission_mode_migrates_legacy_sandbox` |  |
| `rust/crates/ncx-config/src/config.rs` | 423 | fn | `default_permission_mode_is_valid` |  |
| `rust/crates/ncx-config/src/config.rs` | 436 | fn | `parallel_tool_limit_must_be_bounded` |  |
| `rust/crates/ncx-config/src/config.rs` | 449 | fn | `orchestrator_budget_defaults_and_bounds_are_validated` |  |
| `rust/crates/ncx-config/src/config.rs` | 461 | type | `ConfigMutator` |  |
| `rust/crates/ncx-config/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-config/src/loader.rs` | 1 | module | `loader` |  |
| `rust/crates/ncx-config/src/loader.rs` | 18 | type | `Table` |  |
| `rust/crates/ncx-config/src/loader.rs` | 22 | fn | `home_dir` |  |
| `rust/crates/ncx-config/src/loader.rs` | 32 | struct | `ConfigPaths` |  |
| `rust/crates/ncx-config/src/loader.rs` | 39 | fn | `default` |  |
| `rust/crates/ncx-config/src/loader.rs` | 53 | struct | `Overrides` |  |
| `rust/crates/ncx-config/src/loader.rs` | 95 | fn | `as_int` |  |
| `rust/crates/ncx-config/src/loader.rs` | 98 | fn | `as_float` |  |
| `rust/crates/ncx-config/src/loader.rs` | 103 | fn | `as_bool` |  |
| `rust/crates/ncx-config/src/loader.rs` | 111 | fn | `selected_scalar` |  |
| `rust/crates/ncx-config/src/loader.rs` | 115 | fn | `parse_hooks` |  |
| `rust/crates/ncx-config/src/loader.rs` | 142 | fn | `normalize_hook_event` |  |
| `rust/crates/ncx-config/src/loader.rs` | 154 | fn | `model_list` |  |
| `rust/crates/ncx-config/src/loader.rs` | 186 | fn | `list_profiles_at` |  |
| `rust/crates/ncx-config/src/loader.rs` | 197 | fn | `list_profiles` |  |
| `rust/crates/ncx-config/src/loader.rs` | 213 | fn | `load_mcp_servers_at` |  |
| `rust/crates/ncx-config/src/loader.rs` | 271 | fn | `load_mcp_servers` |  |
| `rust/crates/ncx-config/src/loader.rs` | 276 | fn | `load_config` |  |
| `rust/crates/ncx-config/src/loader.rs` | 282 | fn | `load_config_with_paths` |  |
| `rust/crates/ncx-config/src/loader.rs` | 291 | fn | `load_config_impl` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 1 | fn | `load_toml` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 1 | module | `sources` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 14 | fn | `str_val` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 23 | fn | `to_string_val` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 35 | fn | `deepseek_values` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 69 | fn | `nanocodex_values` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 121 | fn | `codex_values` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 139 | const | `PROFILE_KEYS` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 168 | fn | `profile_values` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 180 | struct | `ProviderRoute` |  |
| `rust/crates/ncx-config/src/loader/sources.rs` | 193 | fn | `apply_active_provider_route` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 4 | fn | `empty_env` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 8 | fn | `env1` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 14 | fn | `write` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 21 | fn | `no_paths` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 31 | fn | `active_provider_route_replaces_all_stale_flat_connection_fields` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 63 | fn | `missing_active_provider_fails_instead_of_falling_back_to_stale_route` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 77 | fn | `incomplete_active_provider_fails_without_exposing_credential` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 95 | fn | `legacy_price_config_defaults_to_cny_and_explicit_usd_round_trips` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 111 | fn | `config_redacts_api_key` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 124 | fn | `validate_rejects_bad_sandbox_mode` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 135 | fn | `validate_rejects_missing_key` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 142 | fn | `compaction_defaults_on_with_1m_window` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 150 | fn | `load_reads_deepseek_file` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 184 | fn | `overrides_win_over_file` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 208 | fn | `deepseek_nested_provider_key` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 233 | fn | `max_iterations_default_and_override` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 262 | fn | `orchestrator_budget_defaults_and_file_values` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 287 | fn | `max_iterations_from_env` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 302 | fn | `provider_protocol_can_be_isolated_by_the_host_environment` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 317 | fn | `runtime_budget_and_context_edit_fields_load_from_file_env_and_overrides` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 374 | fn | `hooks_load_from_nanocodex_file` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 415 | fn | `hook_event_aliases_are_normalized` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 453 | fn | `hook_missing_command_fails_validation` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 487 | fn | `nanocodex_file_wins_over_deepseek` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 515 | fn | `env_wins_over_nanocodex_file` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 537 | fn | `max_retries_default_and_env` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 581 | fn | `profile_overrides_base_but_below_env` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 633 | fn | `profile_name_from_env_and_unknown_raises` |  |
| `rust/crates/ncx-config/src/loader/tests.rs` | 672 | fn | `list_profiles_returns_sorted_names` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 1 | module | `provider_directory` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 17 | struct | `ProviderRoute` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 31 | struct | `ProviderRouteInput` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 41 | struct | `ProviderRouteView` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 54 | struct | `ProviderDirectory` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 60 | fn | `from_paths` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 66 | fn | `at` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 73 | fn | `path` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 77 | fn | `load` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 85 | fn | `get` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 92 | fn | `views` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 110 | fn | `upsert` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 157 | fn | `delete` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 169 | fn | `activate` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 177 | fn | `upsert_and_activate` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 223 | fn | `activate_with_updates` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 234 | fn | `commit_activation` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 301 | fn | `select_model` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 318 | fn | `reconcile_models` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 351 | fn | `clear_active_flags` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 362 | fn | `save` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 378 | fn | `into_view` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 405 | fn | `validate_input` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 419 | fn | `dedupe_models` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 428 | fn | `valid_model_id` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 441 | fn | `directory` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 446 | fn | `input` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 459 | fn | `upsert_masks_secret_dedupes_models_and_preserves_key_on_edit` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 472 | fn | `activation_writes_one_complete_route_and_rolls_selection_forward` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 488 | fn | `active_route_cannot_be_deleted_and_unknown_model_cannot_be_selected` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 496 | fn | `preset_upsert_and_activation_commit_one_named_route_with_pricing` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 526 | fn | `failed_preset_snapshot_restores_the_previous_directory` |  |
| `rust/crates/ncx-config/src/provider_directory.rs` | 550 | fn | `curated_model_reconciliation_preserves_token_selection_and_other_routes` |  |
| `rust/crates/ncx-config/src/test_support.rs` | 1 | module | `test_support` |  |
| `rust/crates/ncx-config/src/test_support.rs` | 12 | static | `TEST_TEMP_SEQUENCE` |  |
| `rust/crates/ncx-config/src/test_support.rs` | 14 | fn | `unique_temp_dir` |  |
| `rust/crates/ncx-config/src/writer.rs` | 1 | module | `writer` |  |
| `rust/crates/ncx-config/src/writer.rs` | 11 | const | `WRITABLE_KEYS` |  |
| `rust/crates/ncx-config/src/writer.rs` | 50 | fn | `esc_toml` |  |
| `rust/crates/ncx-config/src/writer.rs` | 64 | fn | `dump_nanocodex_toml` |  |
| `rust/crates/ncx-config/src/writer.rs` | 91 | fn | `write_nanocodex_config` |  |
| `rust/crates/ncx-config/src/writer.rs` | 135 | fn | `map` |  |
| `rust/crates/ncx-config/src/writer.rs` | 141 | fn | `dump_round_trips_quoted_value` |  |
| `rust/crates/ncx-config/src/writer.rs` | 159 | fn | `writer_persists_currency_and_available_models` |  |
| `rust/crates/ncx-config/src/writer.rs` | 181 | fn | `dump_skips_empty_and_unknown` |  |
| `rust/crates/ncx-config/src/writer.rs` | 194 | fn | `write_creates_and_merges` |  |
| `rust/crates/ncx-config/src/writer.rs` | 220 | fn | `write_ignores_unknown_keys` |  |
| `rust/crates/ncx-config/src/writer.rs` | 233 | fn | `write_persists_runtime_control_keys` |  |
| `rust/crates/ncx-context/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-context/src/lib.rs` | 2 | trait | `ContextFragment` |  |
| `rust/crates/ncx-context/src/lib.rs` | 4 | fn | `source` |  |
| `rust/crates/ncx-context/src/lib.rs` | 5 | fn | `render` |  |
| `rust/crates/ncx-context/src/lib.rs` | 6 | fn | `max_chars` |  |
| `rust/crates/ncx-context/src/lib.rs` | 7 | fn | `bounded_render` |  |
| `rust/crates/ncx-context/src/lib.rs` | 14 | struct | `ContextSection` |  |
| `rust/crates/ncx-context/src/lib.rs` | 23 | struct | `ContextAssembler` |  |
| `rust/crates/ncx-context/src/lib.rs` | 30 | fn | `new` |  |
| `rust/crates/ncx-context/src/lib.rs` | 37 | fn | `upsert` |  |
| `rust/crates/ncx-context/src/lib.rs` | 56 | fn | `upsert_fragment` |  |
| `rust/crates/ncx-context/src/lib.rs` | 60 | fn | `remove` |  |
| `rust/crates/ncx-context/src/lib.rs` | 66 | fn | `build` |  |
| `rust/crates/ncx-context/src/lib.rs` | 82 | struct | `TextContextFragment` |  |
| `rust/crates/ncx-context/src/lib.rs` | 89 | fn | `new` |  |
| `rust/crates/ncx-context/src/lib.rs` | 99 | fn | `source` |  |
| `rust/crates/ncx-context/src/lib.rs` | 102 | fn | `render` |  |
| `rust/crates/ncx-context/src/lib.rs` | 106 | fn | `max_chars` |  |
| `rust/crates/ncx-context/src/lib.rs` | 113 | struct | `ContextEntry` |  |
| `rust/crates/ncx-context/src/lib.rs` | 119 | struct | `ContextService` |  |
| `rust/crates/ncx-context/src/lib.rs` | 124 | fn | `new` |  |
| `rust/crates/ncx-context/src/lib.rs` | 127 | fn | `entries` |  |
| `rust/crates/ncx-context/src/lib.rs` | 131 | fn | `assemble` |  |
| `rust/crates/ncx-context/src/lib.rs` | 142 | struct | `ContextEditPolicy` |  |
| `rust/crates/ncx-context/src/lib.rs` | 150 | fn | `default` |  |
| `rust/crates/ncx-context/src/lib.rs` | 161 | struct | `ContextEditStats` |  |
| `rust/crates/ncx-context/src/lib.rs` | 171 | struct | `Fragment` |  |
| `rust/crates/ncx-context/src/lib.rs` | 175 | fn | `source` |  |
| `rust/crates/ncx-context/src/lib.rs` | 178 | fn | `render` |  |
| `rust/crates/ncx-context/src/lib.rs` | 181 | fn | `max_chars` |  |
| `rust/crates/ncx-context/src/lib.rs` | 187 | fn | `every_fragment_has_a_hard_output_bound` |  |
| `rust/crates/ncx-context/src/lib.rs` | 192 | fn | `assembler_orders_replaces_and_bounds_fragments` |  |
| `rust/crates/ncx-context/src/lib.rs` | 205 | fn | `context_service_is_an_executable_fragment_provider` |  |
| `rust/crates/ncx-core/examples/dashscope_media_smoke.rs` | 1 | module | `dashscope_media_smoke` |  |
| `rust/crates/ncx-core/examples/dashscope_media_smoke.rs` | 4 | fn | `main` |  |
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
| `rust/crates/ncx-core/src/agent_loop.rs` | 21 | const | `DEFAULT_MAX_PARALLEL_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 30 | struct | `TurnResult` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 39 | struct | `TaskBudget` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 47 | fn | `default` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 58 | enum | `LoopEvent` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 81 | type | `EventSink` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 82 | fn | `emit` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 90 | struct | `AgentLoop` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 115 | fn | `from_runtime_services` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 123 | fn | `new` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 156 | fn | `with_max_iterations` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 162 | fn | `with_task_budget` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 178 | fn | `with_max_parallel_tool_calls` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 185 | fn | `with_tool_scheduler` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 191 | fn | `replace_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 196 | fn | `provider_model` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 201 | fn | `llm_capabilities` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 204 | fn | `policy_service` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 208 | fn | `has_interaction_service` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 212 | fn | `context_service` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 216 | fn | `estimated_cost` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 228 | fn | `runtime_profile` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 253 | fn | `register_context_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 261 | fn | `unregister_context_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 264 | fn | `with_context_edit` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 272 | fn | `with_vision_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 279 | fn | `set_event_sink` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 282 | fn | `active_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 310 | fn | `suggest_title` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 317 | fn | `suggest_title_with_provider` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 340 | fn | `call_model` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 369 | fn | `run_turn` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 390 | fn | `run_goal_round` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 409 | fn | `run_turn_with_authority` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 438 | fn | `apply_stop_hook` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 476 | fn | `bounded_title_source` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 490 | fn | `sanitize_generated_title` |  |
| `rust/crates/ncx-core/src/agent_loop.rs` | 511 | fn | `dump_args` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 1 | module | `deliverable` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 10 | struct | `Fingerprint` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 16 | struct | `DeliverableRequirement` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 21 | fn | `detect` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 48 | fn | `completed_path` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 58 | fn | `pdf_fingerprints` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 79 | fn | `has_pdf_signature` |  |
| `rust/crates/ncx-core/src/agent_loop/deliverable.rs` | 104 | fn | `incomplete_pdf_container_is_not_a_valid_deliverable` |  |
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
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 93 | struct | `CapturingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 99 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 102 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 110 | struct | `CountingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 114 | struct | `LongFailureProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 120 | struct | `LongPdfDeliveryProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 128 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 131 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 161 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 164 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 191 | struct | `StaticContextProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 196 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 199 | fn | `provide` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 206 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 209 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 219 | fn | `answered` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 238 | struct | `RecordingScheduler` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 246 | fn | `execute_one` |  |
| `rust/crates/ncx-core/src/agent_loop/tests.rs` | 255 | fn | `execute_read_only_batch` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 1 | module | `basic_tests` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 4 | fn | `returns_final_text_without_tools` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 18 | fn | `fresh_human_turn_clears_compaction_read_only_recovery` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 34 | fn | `automatic_goal_round_cannot_clear_compaction_read_only_recovery` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 58 | fn | `suggests_a_short_task_oriented_session_title` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 76 | fn | `rejects_an_invalid_generated_session_title` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 88 | fn | `executes_apply_patch_then_finishes` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 111 | fn | `pdf_creation_request_cannot_finish_until_a_new_valid_pdf_exists` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 140 | fn | `pdf_read_request_does_not_require_creating_a_new_pdf` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 158 | fn | `emits_events_for_tool_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 186 | fn | `persists_reasoning_on_tool_call_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 221 | fn | `long_history_is_automatically_compacted_before_the_next_model_call` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 268 | fn | `compact_hooks_wrap_persisted_compaction_and_feed_runtime_notes` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 319 | fn | `runs_update_plan_and_records_state` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 363 | fn | `unfinished_plan_from_previous_turn_does_not_block_a_new_request` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 402 | fn | `retries_an_empty_response_before_completing` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 420 | fn | `retries_a_transport_error_before_completing_the_same_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 449 | fn | `retries_a_stream_decode_error_before_completing_the_same_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 478 | fn | `repeated_transport_errors_end_with_a_chinese_recoverable_message` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 508 | fn | `stops_with_an_error_after_three_empty_responses` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 528 | fn | `update_plan_cannot_drop_an_unfinished_step` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/basic_tests.rs` | 560 | fn | `stops_at_max_iterations` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 1 | module | `runtime_tests` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 4 | fn | `model_receives_the_windows_shell_contract` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 21 | fn | `repeated_tool_failures_force_a_final_answer_without_more_tools` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 41 | fn | `long_chain_convergence_keeps_tools_until_pdf_is_delivered` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 61 | fn | `memory_recall_is_sent_as_query_scoped_system_note` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 114 | fn | `registered_context_provider_is_query_scoped_and_reversible` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 143 | fn | `primary_provider_can_be_replaced_without_rebuilding_runtime_state` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 165 | fn | `task_budget_is_visible_to_model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 190 | fn | `user_prompt_hook_can_block_model_call` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 214 | fn | `user_prompt_hook_output_is_sent_as_system_note` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 238 | fn | `stop_hook_output_is_appended_to_final_text` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 264 | fn | `tool_budget_stops_and_backfills_unanswered_calls` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 290 | fn | `cancel_mid_tool_loop_backfills_tool_results` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 317 | fn | `image_turn_routes_to_vision_provider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 339 | fn | `read_only_calls_run_concurrently` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 342 | struct | `SlowReadTool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 349 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 352 | fn | `description` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 355 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 358 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 361 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 407 | fn | `custom_scheduler_receives_read_batches_and_serial_barriers` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 441 | fn | `write_between_reads_stays_serial_and_ordered` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 471 | fn | `stop_interrupts_a_hanging_tool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 472 | struct | `HangingTool` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 475 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 478 | fn | `description` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 481 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 484 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 516 | fn | `stop_interrupts_a_hanging_model_request` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 517 | struct | `HangingProvider` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 520 | fn | `model` |  |
| `rust/crates/ncx-core/src/agent_loop/tests/runtime_tests.rs` | 523 | fn | `chat` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 1 | module | `tool_dispatch` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 7 | enum | `DispatchStop` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 12 | struct | `DispatchOutput` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 18 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 62 | fn | `starts_parallel_run` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 82 | struct | `MultiplexedTool` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 87 | fn | `name` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 90 | fn | `description` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 94 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 98 | fn | `call_is_read_only` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 102 | fn | `execute` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 109 | fn | `dynamic_read_only_calls_form_parallel_batches_but_writes_do_not` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 137 | fn | `execute_read_batch` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 174 | fn | `execute_serial` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 192 | fn | `record_tool_start` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 203 | fn | `record_tool_result` |  |
| `rust/crates/ncx-core/src/agent_loop/tool_dispatch.rs` | 219 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 1 | module | `trace` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 6 | fn | `enabled` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 12 | fn | `model_response` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 36 | fn | `tool_result` |  |
| `rust/crates/ncx-core/src/agent_loop/trace.rs` | 46 | fn | `truncate` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 1 | module | `turn` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 14 | const | `MEMORY_RECALL_MAX_ENTRIES` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 16 | const | `MEMORY_RECALL_MAX_CHARS` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 17 | const | `MAX_CONSECUTIVE_EMPTY_RESPONSES` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 18 | const | `MAX_CONSECUTIVE_TRANSPORT_ERRORS` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 19 | const | `SOFT_CONVERGENCE_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 20 | const | `HARD_CONVERGENCE_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 21 | const | `HARD_CONVERGENCE_FAILURES` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 24 | struct | `TurnState` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 34 | fn | `finish` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 44 | struct | `PromptContext` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 50 | fn | `run` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 164 | fn | `request_model_cancellable` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 186 | fn | `cancelled_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 193 | fn | `prepare_prompt` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 240 | fn | `request_model` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 322 | fn | `finish_response` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 412 | fn | `is_retryable_transport_error` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 419 | fn | `has_unfinished_plan` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 431 | fn | `retire_active_plan` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 436 | fn | `persist_tool_calls` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 453 | fn | `stop_turn` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 476 | fn | `tool_budget_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 491 | fn | `model_budget_result` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 504 | fn | `budget_note` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 515 | fn | `has_image_block` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 523 | fn | `user_query_text` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 538 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 542 | fn | `add_usage` |  |
| `rust/crates/ncx-core/src/agent_loop/turn.rs` | 548 | fn | `memory_recall_notes` |  |
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
| `rust/crates/ncx-core/src/goal_tools.rs` | 1 | module | `goal_tools` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 5 | const | `DEFAULT_MAX_GOAL_ROUNDS` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 7 | const | `BLOCKED_AFTER_CONSECUTIVE_ROUNDS` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 10 | enum | `GoalAuthoritySource` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 20 | struct | `GoalTurnAuthority` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 27 | trait | `GoalToolService` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 28 | fn | `get` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 29 | fn | `create` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 30 | fn | `edit` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 36 | fn | `pause` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 37 | fn | `resume` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 38 | fn | `complete` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 39 | fn | `block` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 41 | fn | `goal_tools` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 49 | struct | `GetGoalTool` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 51 | struct | `CreateGoalTool` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 52 | struct | `UpdateGoalTool` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 53 | fn | `service` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 59 | fn | `authority` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 75 | fn | `require_direct_human` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 84 | fn | `exact_ref` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 103 | fn | `render` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 115 | fn | `name` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 118 | fn | `description` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 121 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 124 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 127 | fn | `execute` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 137 | fn | `name` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 140 | fn | `description` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 143 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 153 | fn | `execute` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 182 | fn | `name` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 185 | fn | `description` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 188 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 202 | fn | `execute` |  |
| `rust/crates/ncx-core/src/goal_tools.rs` | 206 | fn | `update` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 1 | module | `goal_tools_tests` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 7 | struct | `RecordingGoalService` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 14 | fn | `new` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 33 | fn | `record` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 44 | fn | `get` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 48 | fn | `create` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 51 | fn | `edit` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 54 | fn | `pause` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 57 | fn | `resume` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 60 | fn | `complete` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 63 | fn | `block` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 67 | fn | `context` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 83 | fn | `take_tool` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 92 | fn | `missing_or_stale_authority_is_rejected_before_service_access` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 111 | fn | `goal_round_cannot_use_direct_human_mutations` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 137 | fn | `exact_goal_round_can_complete_but_stale_identity_cannot` |  |
| `rust/crates/ncx-core/src/goal_tools_tests.rs` | 176 | fn | `model_reported_block_requires_three_admitted_rounds` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 1 | module | `hooks` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 15 | enum | `HookEvent` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 25 | fn | `as_str` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 38 | struct | `HookOutcome` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 42 | fn | `run_matching_hooks` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 71 | fn | `matches_tool` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 83 | fn | `run_one_hook` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 108 | fn | `render_hook_result` |  |
| `rust/crates/ncx-core/src/hooks.rs` | 151 | fn | `matcher_supports_exact_wildcard_and_lists` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 1 | module | `isolate` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 15 | const | `SKIP_DIRS` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 24 | fn | `is_skipped_dir` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 31 | fn | `copy_tree` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 63 | fn | `tmp` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 71 | fn | `copies_files_and_skips_ignored` |  |
| `rust/crates/ncx-core/src/isolate.rs` | 91 | fn | `isolated_copy_is_independent` |  |
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
| `rust/crates/ncx-core/src/mcp_tool.rs` | 43 | fn | `annotation_declares_read_only` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 47 | fn | `mcp_call_is_read_only` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 79 | fn | `approval_denied_message` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 88 | fn | `name` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 91 | fn | `description` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 95 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 99 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 103 | fn | `call_is_read_only` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 112 | fn | `execute` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 163 | fn | `prepare_mcp_server_tools` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 180 | fn | `register_mcp_server` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 202 | fn | `annotations_only_admit_explicit_non_destructive_reads` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 233 | fn | `llmwiki_read_actions_bypass_approval_but_mutations_do_not` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 293 | fn | `never_policy_denies_read_named_tools_without_safe_annotations` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 326 | fn | `never_policy_denies_side_effecting_llmwiki_actions_without_a_live_server` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 347 | fn | `write_mock_server` |  |
| `rust/crates/ncx-core/src/mcp_tool.rs` | 385 | fn | `register_and_execute_echo` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 1 | module | `media_tools` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 9 | struct | `MediaPrice` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 16 | struct | `MediaGenerationService` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 25 | fn | `cost` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 32 | struct | `GenerateImageTool` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 35 | fn | `new` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 42 | fn | `name` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 45 | fn | `description` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 48 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 51 | fn | `execute` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 55 | struct | `GenerateVideoTool` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 58 | fn | `new` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 65 | fn | `name` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 68 | fn | `description` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 71 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 74 | fn | `execute` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 78 | fn | `execute_generation` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 122 | struct | `FakeProvider` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 125 | fn | `generate` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 141 | fn | `service` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 161 | fn | `context` |  |
| `rust/crates/ncx-core/src/media_tools.rs` | 169 | fn | `results_include_explicit_cost_units` |  |
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
| `rust/crates/ncx-core/src/memory.rs` | 173 | fn | `write_all` |  |
| `rust/crates/ncx-core/src/memory.rs` | 192 | fn | `keywords` |  |
| `rust/crates/ncx-core/src/memory.rs` | 201 | fn | `expanded_keywords` |  |
| `rust/crates/ncx-core/src/memory.rs` | 213 | fn | `semantic_aliases` |  |
| `rust/crates/ncx-core/src/memory.rs` | 230 | fn | `semantic_score` |  |
| `rust/crates/ncx-core/src/memory.rs` | 261 | fn | `normalize` |  |
| `rust/crates/ncx-core/src/memory.rs` | 270 | fn | `word_set` |  |
| `rust/crates/ncx-core/src/memory.rs` | 277 | fn | `phrases` |  |
| `rust/crates/ncx-core/src/memory.rs` | 291 | fn | `jaccard` |  |
| `rust/crates/ncx-core/src/memory.rs` | 306 | fn | `parse_entries` |  |
| `rust/crates/ncx-core/src/memory.rs` | 363 | fn | `store` |  |
| `rust/crates/ncx-core/src/memory.rs` | 371 | fn | `remember_then_round_trips` |  |
| `rust/crates/ncx-core/src/memory.rs` | 388 | fn | `dedup_skips_identical` |  |
| `rust/crates/ncx-core/src/memory.rs` | 397 | fn | `empty_is_not_stored` |  |
| `rust/crates/ncx-core/src/memory.rs` | 404 | fn | `cap_drops_oldest` |  |
| `rust/crates/ncx-core/src/memory.rs` | 416 | fn | `recall_scores_by_keyword_overlap` |  |
| `rust/crates/ncx-core/src/memory.rs` | 440 | fn | `recall_uses_semantic_aliases_and_tags` |  |
| `rust/crates/ncx-core/src/memory.rs` | 456 | fn | `recall_empty_store_is_blank` |  |
| `rust/crates/ncx-core/src/memory.rs` | 462 | fn | `consolidate_merges_near_duplicates` |  |
| `rust/crates/ncx-core/src/memory.rs` | 479 | struct | `FixedMerger` |  |
| `rust/crates/ncx-core/src/memory.rs` | 483 | fn | `merge` |  |
| `rust/crates/ncx-core/src/memory.rs` | 493 | fn | `summarize_merges_cluster_into_one` |  |
| `rust/crates/ncx-core/src/memory.rs` | 525 | fn | `summarize_falls_back_to_newest_when_merge_none` |  |
| `rust/crates/ncx-core/src/memory.rs` | 543 | fn | `consolidate_is_idempotent` |  |
| `rust/crates/ncx-core/src/memory.rs` | 555 | fn | `recall_respects_entry_cap` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 1 | module | `memory_merge` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 9 | struct | `MemoryMergeDraft` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 17 | fn | `prepare_summarize_consolidate` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 28 | fn | `prepare_summarize_consolidate_cancellable` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 50 | fn | `commit_summarize_consolidate` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 66 | fn | `summarize_consolidate` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 77 | fn | `merge_entries` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 116 | fn | `merge_cluster` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 149 | struct | `FixedMerger` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 154 | fn | `merge` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 158 | fn | `store` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 166 | fn | `draft_does_not_write_until_commit` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 183 | fn | `concurrent_change_rejects_the_whole_draft` |  |
| `rust/crates/ncx-core/src/memory_merge.rs` | 203 | fn | `cancellation_discards_the_prepared_result` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 1 | module | `memory_summarizer` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 9 | struct | `ProviderMemorySummarizer` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 15 | fn | `new` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 23 | fn | `failure_count` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 30 | fn | `merge` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 61 | struct | `CapturingProvider` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 69 | fn | `model` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 72 | fn | `chat` |  |
| `rust/crates/ncx-core/src/memory_summarizer.rs` | 85 | fn | `uses_injected_model_and_rejects_errors` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 1 | module | `mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 9 | const | `TRIM_TRAILING` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 11 | const | `MAX_FILE_BYTES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 12 | const | `MAX_FILES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 13 | const | `MAX_TOTAL_BYTES` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 19 | fn | `find_mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 70 | fn | `expand_file_mentions` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 124 | fn | `tmpdir` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 133 | fn | `find_mentions_basic_and_trailing_punct` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 142 | fn | `quoted_mention_supports_attachment_paths_with_spaces` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 150 | fn | `expand_inlines_existing_file` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 160 | fn | `nonexistent_mention_is_left_alone` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 167 | fn | `dedup_and_multiple` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 177 | fn | `binary_file_skipped` |  |
| `rust/crates/ncx-core/src/mentions.rs` | 185 | fn | `large_file_truncated` |  |
| `rust/crates/ncx-core/src/model_provider.rs` | 1 | module | `model_provider` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 1 | module | `orchestrator` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 43 | struct | `Orchestrator` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 51 | fn | `new` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 59 | fn | `with_control` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 66 | fn | `handle` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 73 | fn | `emit_stage` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 83 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 88 | fn | `cancelled_outcome` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 109 | fn | `record` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 128 | fn | `run_node` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 134 | fn | `reason_node` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 144 | fn | `handle_at` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 192 | fn | `classify` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 205 | fn | `pipeline` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 223 | fn | `run_attempts` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 326 | fn | `decompose_and_recurse` |  |
| `rust/crates/ncx-core/src/orchestrator.rs` | 401 | fn | `plan_and_decompose` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 1 | module | `control_tests` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 4 | struct | `RecordingControl` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 12 | fn | `emit` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 18 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 23 | struct | `MediumRunner` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 30 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 43 | fn | `emits_typed_progress_in_graph_order` |  |
| `rust/crates/ncx-core/src/orchestrator/control_tests.rs` | 76 | fn | `cancellation_before_workers_stops_later_nodes_and_promotion` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 1 | module | `support` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 2 | const | `CLASSIFY_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 4 | const | `PLAN_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 5 | const | `DECOMPOSE_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 6 | const | `WORKER_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 7 | const | `VERIFY_SYS` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 8 | fn | `orch_trace` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 14 | fn | `parse_complexity` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 25 | fn | `verdict_passed` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 29 | fn | `parse_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 48 | fn | `strip_list_marker` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 66 | fn | `build_worker_task` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 86 | fn | `build_decompose_task` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 90 | fn | `build_verify_task` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 100 | fn | `synthesize` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 113 | fn | `synthesize_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator/support.rs` | 122 | fn | `parse_best_worker` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 5 | struct | `MockRunner` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 19 | fn | `new` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 30 | fn | `with_complexities` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 35 | fn | `with_decomposition` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 39 | fn | `stage` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 58 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 79 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 84 | fn | `count` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 91 | struct | `TelemetryRunner` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 96 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 103 | fn | `run_result` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 113 | fn | `reason_result` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 120 | fn | `aggregates_usage_and_model_evidence_across_nodes` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 132 | fn | `simple_runs_single_fast` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 145 | fn | `medium_runs_plan_2workers_then_flash_verify` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 165 | fn | `high_atomic_falls_back_to_best_of_n_on_main` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 182 | fn | `high_decomposes_into_recursive_subtasks` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 204 | fn | `subtask_count_is_capped` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 227 | fn | `recursion_is_depth_capped` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 246 | fn | `decomposition_off_when_max_depth_zero` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 269 | fn | `closed_loop_retries_on_fail_then_passes` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 291 | fn | `verifier_selects_best_worker_and_promotes_it` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 313 | struct | `FailingPromotionRunner` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 318 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 321 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 328 | fn | `promotion_failure_is_reported_and_never_claimed_as_verified` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 346 | fn | `runtime_config_controls_orchestrator_resource_budget` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 364 | fn | `missing_best_defaults_to_worker_zero` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 373 | fn | `retries_are_capped` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 392 | fn | `parse_subtasks_extracts_prefixed_lines` |  |
| `rust/crates/ncx-core/src/orchestrator/tests.rs` | 399 | fn | `parse_subtasks_falls_back_to_lists` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 1 | module | `types` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 5 | enum | `Tier` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 11 | enum | `Complexity` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 18 | enum | `OrchestratorStage` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 28 | struct | `OrchestratorEvent` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 36 | trait | `OrchestratorControl` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 37 | fn | `emit` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 38 | fn | `is_cancelled` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 45 | struct | `AgentCallResult` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 55 | struct | `OrchestratorTelemetry` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 64 | trait | `AgentRunner` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 65 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 66 | fn | `run_result` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 73 | fn | `reason` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 77 | fn | `reason_result` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 84 | fn | `run_worker` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 88 | fn | `run_worker_result` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 101 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 108 | struct | `OrchestratorConfig` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 117 | fn | `default` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 129 | fn | `from_runtime_config` |  |
| `rust/crates/ncx-core/src/orchestrator/types.rs` | 141 | struct | `OrchestratorOutcome` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 1 | module | `orchestrator_runner` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 22 | type | `CancelCheck` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 24 | type | `LoopObserver` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 25 | static | `SCRATCH_COUNTER` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 28 | enum | `HarnessRunnerEvent` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 39 | struct | `HarnessAgentRunner` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 49 | struct | `WorkerWorkspace` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 56 | fn | `new` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 69 | fn | `with_bindings` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 74 | fn | `with_cancel` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 79 | fn | `with_observer` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 84 | fn | `with_harness_profile` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 89 | fn | `config` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 93 | fn | `model_for` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 101 | fn | `run_in` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 178 | fn | `prepare_worker_workspace` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 211 | fn | `run` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 214 | fn | `run_result` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 219 | fn | `reason` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 223 | fn | `reason_result` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 228 | fn | `run_worker` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 234 | fn | `run_worker_result` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 257 | fn | `promote_worker` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 275 | fn | `drop` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 281 | fn | `remap_workspace_paths` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 293 | fn | `replace_path_variant` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 305 | fn | `worker_activity` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 326 | fn | `isolation_failure_is_fail_closed` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 340 | fn | `runner_promotion_applies_deletions_and_cleans_all_scratch` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 367 | fn | `worker_paths_are_remapped_without_touching_unrelated_text` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 380 | fn | `worker_path_remapping_is_case_insensitive_on_windows` |  |
| `rust/crates/ncx-core/src/orchestrator_runner.rs` | 390 | fn | `worker_activity_never_contains_tool_arguments_or_results` |  |
| `rust/crates/ncx-core/src/plugins.rs` | 1 | module | `plugins` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 1 | module | `api` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 10 | trait | `HarnessPlugin` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 12 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 13 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 15 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 18 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 20 | type | `Disposer` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 24 | struct | `PluginRuntimeState` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 30 | fn | `drop` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 36 | struct | `PluginHost` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 42 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 45 | fn | `tool` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 49 | fn | `context` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 53 | fn | `middleware` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 59 | fn | `provide` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 73 | fn | `service` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 84 | fn | `effect` |  |
| `rust/crates/ncx-core/src/plugins/api.rs` | 87 | fn | `has_service` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 1 | module | `builtin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 16 | struct | `ProviderDirectoryPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 18 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 21 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 28 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 37 | struct | `ProviderCatalogPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 39 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 42 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 49 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 52 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 62 | struct | `ProviderChatProbePlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 64 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 67 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 74 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 77 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 86 | struct | `CoreToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 89 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 92 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 96 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 99 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 114 | struct | `SearchToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 117 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 120 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 124 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 137 | struct | `WorkspaceToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 140 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 143 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 151 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 162 | struct | `ProcessToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 165 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 168 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 172 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 191 | struct | `SessionToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 194 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 197 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 201 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 228 | struct | `MemoryPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 231 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 234 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 237 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 247 | struct | `CompactionPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 250 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 253 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 256 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 263 | struct | `McpPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 266 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 269 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 272 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 290 | struct | `AttachmentPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 293 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 296 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 303 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 323 | struct | `MediaPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 326 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 329 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 332 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 354 | struct | `ExternalHostPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 356 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 359 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 366 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 370 | struct | `CostTelemetryPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 373 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 376 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 383 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 408 | struct | `LlmProviderPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 411 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 414 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 417 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 432 | struct | `InteractionPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 434 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 437 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 444 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 453 | struct | `PolicyPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 455 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 458 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 461 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 473 | struct | `ContextPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 475 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 478 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 481 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 486 | struct | `BuiltinToolsPlugin` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 489 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 492 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 500 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 519 | fn | `empty_registry` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 525 | fn | `names` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 535 | fn | `capability_plugins_install_independent_tool_sets` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 552 | fn | `compatibility_bundle_matches_default_registry` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 560 | struct | `PolicyProbe` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 565 | fn | `name` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 568 | fn | `description` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 571 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 574 | fn | `execute` |  |
| `rust/crates/ncx-core/src/plugins/builtin.rs` | 580 | fn | `tool_execution_consumes_replaceable_policy_service` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 1 | module | `composition` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 12 | struct | `PluginEntry` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 20 | fn | `enabled_by_default` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 24 | fn | `empty_config` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 31 | struct | `BundleSpec` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 39 | struct | `ProfileSpec` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 46 | struct | `OverlayEntry` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 55 | struct | `OverlaySpec` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 61 | struct | `HarnessComposition` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 67 | fn | `load` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 94 | fn | `compose` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 143 | fn | `enabled_plugins` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 150 | fn | `enabled_entries` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 154 | fn | `apply_overlay_files` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 162 | fn | `read_toml` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 169 | fn | `validate_entry` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 182 | fn | `validate_file_id` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 193 | fn | `apply_overlay` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 227 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 250 | fn | `profile_stacks_bundles_and_overlay_replaces_by_entry_id` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 263 | fn | `unknown_overlay_entry_fails_loud` |  |
| `rust/crates/ncx-core/src/plugins/composition.rs` | 273 | fn | `profile_and_bundle_ids_cannot_escape_the_config_root` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 1 | module | `external` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 21 | struct | `ExternalPluginManifest` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 34 | struct | `ExternalPluginRecord` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 42 | fn | `launch` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 63 | fn | `handshake` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 68 | fn | `tools` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 81 | struct | `ExternalPluginCatalog` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 86 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 89 | fn | `discover` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 116 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 135 | fn | `upgrade` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 175 | fn | `set_enabled` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 193 | fn | `parse_manifest` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 199 | fn | `validate_manifest` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 233 | fn | `validate_id` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 244 | fn | `version_tuple` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 254 | fn | `copy_dir` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 275 | fn | `temp` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 278 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 288 | fn | `install_disable_enable_and_upgrade_are_atomic` |  |
| `rust/crates/ncx-core/src/plugins/external.rs` | 307 | fn | `native_library_and_path_escape_are_rejected` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 1 | module | `protocol` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 11 | enum | `ExternalProtocolRequest` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 26 | enum | `ExternalProtocolResponse` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 46 | struct | `ExternalToolDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 55 | struct | `ExternalPluginHandshake` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 63 | struct | `ExternalPluginRegistration` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 67 | struct | `ExternalProcessTool` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 74 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 81 | fn | `name` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 84 | fn | `description` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 88 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 92 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 96 | fn | `execute` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 107 | fn | `handshake` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 140 | fn | `call_tool` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 186 | fn | `validate_handshake` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 238 | fn | `exchange` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 293 | fn | `temp_root` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 296 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 300 | fn | `fixture_at` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 329 | fn | `protocol_fixture_worker` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 370 | fn | `handshake_registers_and_executes_a_real_isolated_tool` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 388 | fn | `configured_runtime_discovers_and_registers_external_tools` |  |
| `rust/crates/ncx-core/src/plugins/external/protocol.rs` | 417 | fn | `handshake_rejects_tools_outside_the_plugin_namespace` |  |
| `rust/crates/ncx-core/src/plugins/manifest.rs` | 1 | module | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/manifest.rs` | 4 | enum | `PluginCapability` |  |
| `rust/crates/ncx-core/src/plugins/manifest.rs` | 24 | struct | `PluginManifest` |  |
| `rust/crates/ncx-core/src/plugins/manifest.rs` | 34 | const | `fn` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 1 | module | `openai_compat` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 15 | const | `MANIFEST` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 20 | struct | `CodexPluginManifest` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 42 | enum | `ResourcePaths` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 50 | fn | `values` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 60 | struct | `CodexPluginRecord` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 67 | struct | `CodexAppResource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 74 | fn | `skill_paths` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 93 | fn | `mcp_path` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 100 | fn | `apps_path` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 114 | fn | `hooks_path` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 124 | struct | `CodexPluginCatalog` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 127 | fn | `discover_enabled_codex_plugins_with_home` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 148 | fn | `local_home` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 156 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 159 | fn | `discover` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 163 | fn | `discover_best_effort` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 167 | fn | `discover_inner` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 200 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 204 | fn | `install_or_upgrade` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 225 | fn | `replace` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 258 | fn | `uninstall` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 267 | fn | `set_enabled` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 283 | fn | `recover_interrupted_updates` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 333 | fn | `discover_codex_mcp_servers` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 337 | fn | `discover_codex_mcp_servers_with_home` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 404 | fn | `load_mcp_server_resource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 421 | fn | `resolve_mcp_process_value` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 436 | fn | `resolve_mcp_args` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 455 | fn | `resolve_mcp_env` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 477 | fn | `discover_codex_apps` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 480 | fn | `discover_codex_apps_with_home` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 532 | fn | `discover_codex_hooks` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 536 | fn | `discover_codex_hooks_with_home` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 597 | fn | `map_hook_event` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 609 | fn | `resolve_json_resource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 628 | fn | `resolve_hook_resources` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 667 | fn | `load_record` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 679 | fn | `validate_manifest` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 715 | fn | `validate_json_resource_paths` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 730 | fn | `validate_resource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 751 | fn | `validate_segment` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat.rs` | 762 | fn | `copy_resource_tree` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 1 | module | `marketplace` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 6 | const | `MARKETPLACE_PATHS` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 15 | struct | `Marketplace` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 22 | struct | `MarketplacePlugin` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 29 | enum | `MarketplaceSource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 52 | fn | `deserialize` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 58 | enum | `RawSource` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 65 | enum | `RawSourceObject` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 135 | fn | `discover_marketplaces` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/marketplace.rs` | 150 | fn | `resolve_local_marketplace_plugin` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 2 | fn | `temp` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 5 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 18 | fn | `codex_plugin_installs_discovers_toggles_and_uninstalls` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 34 | fn | `global_plugins_are_available_and_workspace_plugins_shadow_same_name` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 52 | fn | `codex_plugin_upgrade_replaces_resources_and_preserves_disabled_state` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 70 | fn | `catalog_recovers_interrupted_upgrade_backup_and_hides_staging` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 94 | fn | `plugin_and_marketplace_paths_cannot_escape_their_roots` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 106 | fn | `local_marketplace_sources_resolve_from_repository_root_for_all_layouts` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 132 | fn | `official_marketplace_source_shapes_are_accepted` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 175 | fn | `conventional_codex_mcp_and_hook_resources_feed_existing_runtime_types` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 239 | fn | `official_path_based_mcp_hooks_and_interface_resources_are_supported` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 298 | fn | `installed_plugin_resolves_relative_mcp_script_from_target_root` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 330 | fn | `damaged_mcp_server_is_skipped_without_hiding_valid_servers` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 349 | fn | `malformed_mcp_args_and_env_are_skipped_without_hiding_valid_servers` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 377 | fn | `damaged_mcp_plugin_resource_is_skipped_without_hiding_valid_servers` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 404 | fn | `damaged_plugin_manifest_is_skipped_without_hiding_valid_mcp_servers` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 427 | fn | `bare_mcp_argument_keeps_process_cwd_semantics_even_when_file_exists_in_plugin` |  |
| `rust/crates/ncx-core/src/plugins/openai_compat/tests.rs` | 445 | fn | `codex_apps_are_parsed_as_hosted_connector_resources` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 1 | module | `registry` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 9 | struct | `PluginInstallReport` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 15 | struct | `PluginRegistry` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 18 | struct | `RegisteredPlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 25 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 28 | fn | `register` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 32 | fn | `register_configured` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 54 | fn | `ids` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 58 | fn | `manifests` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 62 | fn | `install_into` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 126 | struct | `EmptyPlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 130 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 133 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 136 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 142 | fn | `ids_are_stable_unique_and_ordered` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 150 | struct | `ServicePlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 156 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 159 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 162 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 169 | struct | `ConsumerPlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 173 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 176 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 179 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 182 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 192 | struct | `MissingDependencyPlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 196 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 199 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 206 | fn | `inject` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 209 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 213 | struct | `ConfigPlugin` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 217 | fn | `id` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 220 | fn | `manifest` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 227 | fn | `install` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 238 | fn | `dependencies_activate_by_service_and_effects_dispose_with_runtime` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 262 | fn | `unresolved_service_dependency_fails_loud` |  |
| `rust/crates/ncx-core/src/plugins/registry.rs` | 277 | fn | `file_composition_config_reaches_plugin_installation` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 1 | module | `runtime` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 18 | struct | `HarnessRuntimeBuilder` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 27 | fn | `default` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 33 | fn | `empty` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 41 | fn | `from_composition` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 63 | fn | `from_files` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 72 | fn | `builtin` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 76 | fn | `configured` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 84 | fn | `configured_for_profile` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 133 | fn | `register` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 138 | fn | `plugin_ids` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 142 | fn | `build` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 146 | fn | `build_with_report` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 173 | fn | `media_flag` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 180 | fn | `filter_skills_for_media` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 229 | fn | `builtin_composition` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 250 | fn | `builtin_plugin` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 275 | fn | `embedded_bundle` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 289 | fn | `parse_embedded` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 303 | fn | `context` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 309 | fn | `skill` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 323 | fn | `default_runtime_reports_architectural_plugin_order` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 354 | fn | `empty_runtime_has_no_model_facing_tools` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 360 | fn | `file_driven_profiles_select_components_without_changing_default_order` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 405 | fn | `full_minimal_and_headless_are_real_isolated_compositions` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 468 | fn | `profiles_filter_media_skills_before_tool_and_context_installation` |  |
| `rust/crates/ncx-core/src/plugins/runtime.rs` | 517 | fn | `external_profile_bundle_and_overlay_drive_runtime_selection` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 1 | module | `services` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 21 | struct | `LlmServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 30 | struct | `ProviderDirectoryService` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 36 | fn | `default` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 42 | fn | `from_paths` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 48 | fn | `list` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 54 | fn | `get` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 58 | fn | `save` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 62 | fn | `delete` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 69 | fn | `activate` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 73 | fn | `save_and_activate_preset` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 89 | fn | `select_model` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 93 | fn | `reconcile_models` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 97 | fn | `clear_active_flags` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 101 | fn | `diagnostics` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 117 | struct | `ProviderDirectoryDiagnostics` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 129 | struct | `ProviderCatalogService` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 136 | struct | `ProviderChatProbeService` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 141 | fn | `default` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 147 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 150 | fn | `probe_route` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 169 | fn | `default` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 175 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 178 | fn | `discover_route` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 197 | fn | `validate_route_model` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 208 | fn | `discover_config` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 222 | fn | `discover_public` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 226 | fn | `discover` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 239 | trait | `LlmProviderFactory` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 240 | fn | `primary` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 241 | fn | `vision` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 245 | struct | `LlmProviderFactoryHandle` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 248 | struct | `InteractionService` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 255 | struct | `ContextServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 263 | fn | `assemble` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 269 | struct | `MemoryServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 275 | struct | `CompactionServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 280 | struct | `McpServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 287 | struct | `AttachmentServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 293 | struct | `MediaServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 300 | struct | `CostTelemetryServiceDescriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 308 | fn | `estimate` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 313 | struct | `CostTelemetryService` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 323 | fn | `new` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 332 | fn | `record` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 345 | fn | `record_media_cost` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 350 | fn | `snapshot` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 366 | struct | `CostTelemetrySnapshot` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 376 | struct | `HarnessDiagnostics` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 391 | fn | `context_descriptor` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 405 | struct | `FailingCatalogClient` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 408 | fn | `discover` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 415 | struct | `FixedCatalogClient` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 418 | fn | `discover` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 432 | fn | `telemetry_accumulates_usage_and_estimates_cost` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 454 | fn | `provider_directory_service_owns_route_activation_and_safe_diagnostics` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 487 | fn | `catalog_failure_cannot_mutate_the_active_provider_route` |  |
| `rust/crates/ncx-core/src/plugins/services.rs` | 521 | fn | `route_validation_requires_the_selected_model_without_mutating_files` |  |
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
| `rust/crates/ncx-core/src/project_instructions.rs` | 132 | fn | `loads_home_and_layered_workspace_files` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 166 | fn | `empty_when_no_files` |  |
| `rust/crates/ncx-core/src/project_instructions.rs` | 172 | fn | `caps_large_instruction_block` |  |
| `rust/crates/ncx-core/src/prompt.rs` | 1 | module | `prompt` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 1 | module | `runtime_assembly` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 20 | const | `INSTRUCTIONS_ORDER` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 22 | const | `SKILLS_ORDER` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 23 | const | `PLAN_ORDER` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 26 | struct | `RuntimeContextSources` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 36 | fn | `new` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 46 | fn | `with_memory` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 51 | fn | `with_hooks` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 56 | fn | `with_genome` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 66 | struct | `RuntimeHostBindings` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 74 | struct | `ConfiguredHarnessRuntime` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 82 | fn | `new` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 90 | fn | `from_config` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 96 | fn | `profile` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 100 | fn | `with_harness_profile` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 108 | fn | `primary_provider` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 113 | fn | `build_tools` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 138 | fn | `build_toolless` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 167 | fn | `build_context` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 206 | fn | `context_entries` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 237 | struct | `TestGoalService` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 240 | fn | `get` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 243 | fn | `create` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 246 | fn | `edit` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 249 | fn | `pause` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 252 | fn | `resume` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 255 | fn | `complete` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 258 | fn | `block` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 264 | fn | `configured_runtime_owns_policy_provider_and_context_fragments` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 297 | fn | `tool_and_toolless_paths_share_the_same_runtime_contracts` |  |
| `rust/crates/ncx-core/src/runtime_assembly.rs` | 325 | fn | `goal_tools_remain_executable_in_every_session_profile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 1 | module | `runtime_profile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 15 | const | `DEFAULT_MAX_MODEL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 17 | const | `DEFAULT_MAX_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 18 | const | `DEFAULT_MAX_PARALLEL_TOOL_CALLS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 19 | const | `DEFAULT_CONTEXT_MAX_CHARS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 20 | const | `DEFAULT_CONTEXT_KEEP_RECENT` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 21 | const | `DEFAULT_CONTEXT_TOOL_RESULT_CHARS` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 25 | struct | `AgentRuntimeProfile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 34 | struct | `RuntimePermissionProfile` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 44 | fn | `from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 49 | fn | `from_permission_mode` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 63 | fn | `from_legacy_permissions` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 75 | fn | `with_permissions` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 103 | fn | `apply` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 111 | fn | `sandbox_policy` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 117 | fn | `apply_tool_context` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 129 | fn | `model_provider_from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 149 | fn | `vision_provider_from_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 174 | fn | `model_supports_native_vision` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 183 | struct | `ConfiguredLlmProviderFactory` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 189 | fn | `new` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 198 | fn | `primary` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 201 | fn | `vision` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 206 | fn | `install_llm_provider_factory` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 235 | fn | `install_media_provider` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 273 | fn | `media_price` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 288 | fn | `positive_usize` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 295 | fn | `nonnegative_usize` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 306 | fn | `assemble_frontend` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 320 | fn | `cli_and_gui_runtime_assembly_is_equivalent_for_same_config` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 376 | fn | `invalid_numeric_values_use_runtime_defaults` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 406 | fn | `alibaba_attachment_parser_is_opt_in_and_native_vision_is_catalogued` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 424 | fn | `explicit_legacy_permissions_remain_available_for_cli_flags` |  |
| `rust/crates/ncx-core/src/runtime_profile.rs` | 441 | fn | `media_tools_require_both_full_profile_capability_and_dashscope_key` |  |
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
| `rust/crates/ncx-core/src/search.rs` | 20 | const | `IGNORE_DIRS` |  |
| `rust/crates/ncx-core/src/search.rs` | 32 | const | `MAX_FILES` |  |
| `rust/crates/ncx-core/src/search.rs` | 33 | const | `MAX_ENTRIES` |  |
| `rust/crates/ncx-core/src/search.rs` | 34 | const | `MAX_FILE_BYTES` |  |
| `rust/crates/ncx-core/src/search.rs` | 35 | const | `DEFAULT_MAX_RESULTS` |  |
| `rust/crates/ncx-core/src/search.rs` | 39 | fn | `walk_files` |  |
| `rust/crates/ncx-core/src/search.rs` | 78 | fn | `rel_slash` |  |
| `rust/crates/ncx-core/src/search.rs` | 87 | fn | `glob_to_regex` |  |
| `rust/crates/ncx-core/src/search.rs` | 120 | fn | `grep` |  |
| `rust/crates/ncx-core/src/search.rs` | 178 | fn | `truncate_chars` |  |
| `rust/crates/ncx-core/src/search.rs` | 187 | fn | `grep_literal` |  |
| `rust/crates/ncx-core/src/search.rs` | 197 | fn | `glob` |  |
| `rust/crates/ncx-core/src/search.rs` | 220 | fn | `find_files_by_name` |  |
| `rust/crates/ncx-core/src/search.rs` | 248 | struct | `GrepTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 252 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 255 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 261 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 272 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 275 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 292 | struct | `GrepLiteralTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 296 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 299 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 304 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 316 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 320 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 336 | struct | `GlobTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 340 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 343 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 348 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 358 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 361 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 374 | struct | `FindFilesTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 378 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 381 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 387 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 410 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 414 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 446 | struct | `WebSearchTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 450 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 453 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 459 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 468 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 471 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search.rs` | 509 | struct | `WebFetchTool` |  |
| `rust/crates/ncx-core/src/search.rs` | 513 | fn | `name` |  |
| `rust/crates/ncx-core/src/search.rs` | 516 | fn | `description` |  |
| `rust/crates/ncx-core/src/search.rs` | 521 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/search.rs` | 530 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/search.rs` | 533 | fn | `execute` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 1 | module | `search_tests` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 2 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 24 | fn | `glob_to_regex_matches_expected` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 34 | fn | `grep_finds_matches_and_skips_ignored` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 44 | fn | `grep_path_glob_filters` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 53 | fn | `grep_no_match_reports_count` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 60 | fn | `grep_invalid_regex_errors` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 66 | fn | `grep_literal_accepts_regex_metacharacters` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 74 | fn | `grep_finds_gb18030_chinese_text_in_deep_directory` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 91 | fn | `grep_truncates_long_unicode_lines_without_panicking` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 101 | fn | `glob_lists_rs_files_skipping_ignored` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 110 | fn | `find_files_recurses_through_chinese_paths_and_skips_generated_dirs` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 135 | fn | `find_files_reports_when_results_are_truncated` |  |
| `rust/crates/ncx-core/src/search_tests.rs` | 155 | fn | `web_tools_blocked_in_read_only` |  |
| `rust/crates/ncx-core/src/session.rs` | 1 | module | `session` |  |
| `rust/crates/ncx-core/src/session.rs` | 17 | const | `COMPACTED_HISTORY_PREFIX` |  |
| `rust/crates/ncx-core/src/session.rs` | 21 | struct | `ContextMessages` |  |
| `rust/crates/ncx-core/src/session.rs` | 27 | struct | `CompactionSafetySnapshot` |  |
| `rust/crates/ncx-core/src/session.rs` | 39 | struct | `SafeCompactionResult` |  |
| `rust/crates/ncx-core/src/session.rs` | 47 | struct | `Session` |  |
| `rust/crates/ncx-core/src/session.rs` | 55 | fn | `new` |  |
| `rust/crates/ncx-core/src/session.rs` | 58 | fn | `with_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 72 | fn | `resume` |  |
| `rust/crates/ncx-core/src/session.rs` | 84 | fn | `fork` |  |
| `rust/crates/ncx-core/src/session.rs` | 100 | fn | `full_messages` |  |
| `rust/crates/ncx-core/src/session.rs` | 110 | fn | `add_user` |  |
| `rust/crates/ncx-core/src/session.rs` | 113 | fn | `add_user_text` |  |
| `rust/crates/ncx-core/src/session.rs` | 119 | fn | `add_assistant` |  |
| `rust/crates/ncx-core/src/session.rs` | 140 | fn | `add_tool_result` |  |
| `rust/crates/ncx-core/src/session.rs` | 150 | fn | `for_model` |  |
| `rust/crates/ncx-core/src/session.rs` | 164 | fn | `for_model_edited` |  |
| `rust/crates/ncx-core/src/session.rs` | 189 | fn | `compact` |  |
| `rust/crates/ncx-core/src/session.rs` | 204 | fn | `compact_if_needed` |  |
| `rust/crates/ncx-core/src/session.rs` | 211 | fn | `compact_safely_if_needed` |  |
| `rust/crates/ncx-core/src/session.rs` | 240 | fn | `needs_compaction` |  |
| `rust/crates/ncx-core/src/session.rs` | 244 | fn | `edited_body` |  |
| `rust/crates/ncx-core/src/session.rs` | 298 | fn | `answered_ids` |  |
| `rust/crates/ncx-core/src/session.rs` | 312 | fn | `backfill_unanswered_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 342 | fn | `append` |  |
| `rust/crates/ncx-core/src/session.rs` | 347 | fn | `append_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 369 | fn | `rewrite_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 399 | fn | `safety_snapshot` |  |
| `rust/crates/ncx-core/src/session.rs` | 485 | fn | `git_diff_summary` |  |
| `rust/crates/ncx-core/src/session.rs` | 503 | fn | `safety_marker` |  |
| `rust/crates/ncx-core/src/session.rs` | 524 | fn | `compare_safety_snapshots` |  |
| `rust/crates/ncx-core/src/session.rs` | 558 | fn | `role` |  |
| `rust/crates/ncx-core/src/session.rs` | 562 | fn | `read_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 580 | fn | `sanitize_restored_messages` |  |
| `rust/crates/ncx-core/src/session.rs` | 619 | fn | `redact_image_data` |  |
| `rust/crates/ncx-core/src/session.rs` | 641 | fn | `now_stamp` |  |
| `rust/crates/ncx-core/src/session.rs` | 648 | fn | `json_chars` |  |
| `rust/crates/ncx-core/src/session.rs` | 654 | fn | `total_chars` |  |
| `rust/crates/ncx-core/src/session.rs` | 664 | fn | `retained_conversation_history` |  |
| `rust/crates/ncx-core/src/session.rs` | 717 | fn | `message_content_text` |  |
| `rust/crates/ncx-core/src/session.rs` | 736 | fn | `estimate_tokens` |  |
| `rust/crates/ncx-core/src/session.rs` | 737 | const | `CHARS_PER_TOKEN` |  |
| `rust/crates/ncx-core/src/session.rs` | 767 | fn | `compress_tool_result` |  |
| `rust/crates/ncx-core/src/session.rs` | 795 | fn | `estimate_tokens_counts_text_and_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 824 | fn | `for_model_prepends_system` |  |
| `rust/crates/ncx-core/src/session.rs` | 835 | fn | `assistant_records_reasoning_only_when_present` |  |
| `rust/crates/ncx-core/src/session.rs` | 844 | fn | `backfill_answers_dangling_tool_calls` |  |
| `rust/crates/ncx-core/src/session.rs` | 866 | fn | `context_edit_compresses_old_tool_results_without_mutating_session` |  |
| `rust/crates/ncx-core/src/session.rs` | 898 | fn | `context_edit_drops_old_prefix_when_over_budget` |  |
| `rust/crates/ncx-core/src/session.rs` | 919 | fn | `context_edit_preserves_user_task_history_while_dropping_tool_noise` |  |
| `rust/crates/ncx-core/src/session.rs` | 964 | fn | `compact_materializes_context_edit_and_rewrites_log` |  |
| `rust/crates/ncx-core/src/session.rs` | 989 | fn | `automatic_compaction_does_not_trigger_under_budget` |  |
| `rust/crates/ncx-core/src/session.rs` | 1006 | fn | `safe_compaction_preserves_prohibitions_and_workspace_evidence` |  |
| `rust/crates/ncx-core/src/session.rs` | 1035 | fn | `safe_compaction_validates_long_requirements_at_the_marker_boundary` |  |
| `rust/crates/ncx-core/src/session.rs` | 1065 | fn | `logs_messages_as_jsonl_and_resumes_body` |  |
| `rust/crates/ncx-core/src/session.rs` | 1084 | fn | `resume_backfills_dangling_tool_call` |  |
| `rust/crates/ncx-core/src/session.rs` | 1109 | fn | `log_redacts_inline_image_data` |  |
| `rust/crates/ncx-core/src/session.rs` | 1124 | fn | `fork_uses_seed_without_touching_source_log` |  |
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
| `rust/crates/ncx-core/src/session_index.rs` | 151 | fn | `latest_resumable_for_workspace` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 167 | fn | `record` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 172 | fn | `record_turn` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 182 | fn | `record_turn_with_title` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 222 | fn | `set_title` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 239 | fn | `set_archived` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 249 | fn | `snapshot_path` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 254 | fn | `save_snapshot` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 268 | fn | `load_snapshot` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 274 | fn | `load` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 289 | fn | `save` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 307 | fn | `new_session_id` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 316 | fn | `default_index_path` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 324 | fn | `normalized_workspace` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 339 | fn | `summarize` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 418 | fn | `first_text` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 432 | fn | `clip` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 441 | fn | `fallback_title` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 464 | fn | `redact_messages` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 471 | fn | `string_field` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 479 | fn | `usize_field` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 487 | fn | `safe_file_stem` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 500 | fn | `now_stamp` |  |
| `rust/crates/ncx-core/src/session_index.rs` | 511 | fn | `parse_ts_ms` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 1 | module | `session_index_tests` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 4 | fn | `parse_ts_ms_orders_legacy_iso_before_ms_epoch` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 16 | fn | `tmp_path` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 20 | fn | `msgs` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 35 | fn | `summarize_pulls_title_snippet_counts_and_tools` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 56 | fn | `generated_title_is_persisted_and_survives_later_turns` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 81 | fn | `index_upserts_and_sorts_newest_first` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 120 | fn | `latest_resumable_session_is_scoped_to_workspace_and_skips_archived` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 156 | fn | `persists_and_loads_legacy_rows` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 174 | fn | `snapshot_round_trip_redacts_image_data` |  |
| `rust/crates/ncx-core/src/session_index_tests.rs` | 197 | fn | `session_ids_are_unique` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 1 | module | `session_query_tools` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 11 | const | `MAX_RESULTS` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 13 | fn | `session_query_tools` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 26 | struct | `SessionQueryTool` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 33 | fn | `new` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 36 | fn | `store` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 48 | fn | `name` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 51 | fn | `description` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 63 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 80 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 84 | fn | `execute` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 99 | fn | `search_sessions` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 122 | fn | `session_trace` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 132 | fn | `read_events` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 149 | fn | `search_events` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 171 | fn | `event_trace` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 183 | fn | `list_threads` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 187 | fn | `read_visible_thread` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 196 | fn | `visible_messages` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 211 | fn | `summary_json` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 228 | fn | `summary_text` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 247 | fn | `session_id` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 251 | fn | `missing_session_id` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 254 | fn | `limit` |  |
| `rust/crates/ncx-core/src/session_query_tools.rs` | 268 | fn | `searches_visible_thread_projection_without_tool_logs` |  |
| `rust/crates/ncx-core/src/skills.rs` | 1 | module | `skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 29 | const | `INDEX_HEADER` |  |
| `rust/crates/ncx-core/src/skills.rs` | 35 | enum | `SkillCapability` |  |
| `rust/crates/ncx-core/src/skills.rs` | 45 | fn | `parse` |  |
| `rust/crates/ncx-core/src/skills.rs` | 54 | fn | `is_available` |  |
| `rust/crates/ncx-core/src/skills.rs` | 74 | struct | `Skill` |  |
| `rust/crates/ncx-core/src/skills.rs` | 95 | fn | `load_body` |  |
| `rust/crates/ncx-core/src/skills.rs` | 104 | fn | `is_builtin` |  |
| `rust/crates/ncx-core/src/skills.rs` | 112 | fn | `builtin_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 114 | const | `BUILTINS` |  |
| `rust/crates/ncx-core/src/skills.rs` | 146 | fn | `discover_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 149 | fn | `discover_skills_with_home` |  |
| `rust/crates/ncx-core/src/skills.rs` | 182 | fn | `scan_skill_path` |  |
| `rust/crates/ncx-core/src/skills.rs` | 194 | fn | `load_skill_file` |  |
| `rust/crates/ncx-core/src/skills.rs` | 219 | fn | `scan_root` |  |
| `rust/crates/ncx-core/src/skills.rs` | 238 | fn | `skills_index_block` |  |
| `rust/crates/ncx-core/src/skills.rs` | 262 | fn | `parse_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 285 | fn | `frontmatter_lines` |  |
| `rust/crates/ncx-core/src/skills.rs` | 304 | fn | `strip_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 319 | fn | `find_closing_fence` |  |
| `rust/crates/ncx-core/src/skills.rs` | 329 | fn | `unquote` |  |
| `rust/crates/ncx-core/src/skills.rs` | 340 | fn | `home_dir` |  |
| `rust/crates/ncx-core/src/skills.rs` | 350 | fn | `tmp` |  |
| `rust/crates/ncx-core/src/skills.rs` | 357 | fn | `write_skill` |  |
| `rust/crates/ncx-core/src/skills.rs` | 365 | fn | `fs_only` |  |
| `rust/crates/ncx-core/src/skills.rs` | 370 | fn | `discovers_and_parses_frontmatter` |  |
| `rust/crates/ncx-core/src/skills.rs` | 386 | fn | `parses_explicit_media_capabilities_without_guessing_from_names` |  |
| `rust/crates/ncx-core/src/skills.rs` | 398 | fn | `unknown_capability_is_never_silently_exposed_as_general` |  |
| `rust/crates/ncx-core/src/skills.rs` | 411 | fn | `name_falls_back_to_dir` |  |
| `rust/crates/ncx-core/src/skills.rs` | 420 | fn | `builtins_are_always_present_and_loadable` |  |
| `rust/crates/ncx-core/src/skills.rs` | 432 | fn | `filesystem_skill_shadows_builtin` |  |
| `rust/crates/ncx-core/src/skills.rs` | 446 | fn | `workspace_shadows_home_same_name` |  |
| `rust/crates/ncx-core/src/skills.rs` | 465 | fn | `index_block_lists_name_and_description` |  |
| `rust/crates/ncx-core/src/skills.rs` | 479 | fn | `always_apply_skill_body_is_injected_without_model_selection` |  |
| `rust/crates/ncx-core/src/skills.rs` | 495 | fn | `empty_when_no_filesystem_skills` |  |
| `rust/crates/ncx-core/src/skills.rs` | 503 | fn | `malformed_frontmatter_skipped_or_dir_named` |  |
| `rust/crates/ncx-core/src/skills.rs` | 518 | fn | `enabled_codex_plugin_skills_are_discovered_and_disabled_ones_are_not` |  |
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
| `rust/crates/ncx-core/src/test_support.rs` | 1 | module | `test_support` |  |
| `rust/crates/ncx-core/src/test_support.rs` | 8 | fn | `unique_temp_dir` |  |
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
| `rust/crates/ncx-core/src/tool_recovery.rs` | 10 | enum | `ToolCapability` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 26 | fn | `fmt` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 47 | enum | `ToolFailureClass` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 60 | fn | `fmt` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 66 | fn | `retryable` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 72 | fn | `classify_tool_result` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 138 | fn | `infer_capabilities` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 193 | fn | `fallback_call` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 229 | fn | `resolve_unique_missing_read` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 261 | fn | `classifies_failures_without_marking_success_text` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 292 | fn | `fallback_routes_only_known_compatible_calls` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 299 | fn | `fixture` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 309 | fn | `registry_falls_back_from_invalid_regex_to_literal_search` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 323 | fn | `registry_treats_directory_read_as_directory_listing` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 337 | fn | `registry_reads_utf16_and_gb18030_files_without_mojibake` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 366 | fn | `registry_backtracks_unique_missing_read_by_basename` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 392 | fn | `registry_does_not_guess_when_recursive_read_is_ambiguous` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 410 | struct | `FlakyReadTool` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 414 | struct | `FailingWriteTool` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 421 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 424 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 428 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 432 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 441 | fn | `name` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 444 | fn | `description` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 448 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 452 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 456 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 469 | fn | `registry_retries_transient_read_once` |  |
| `rust/crates/ncx-core/src/tool_recovery.rs` | 487 | fn | `registry_never_retries_mutating_tools` |  |
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
| `rust/crates/ncx-core/src/tools.rs` | 37 | const | `DEFAULT_VISIBLE_TOOL_LIMIT` |  |
| `rust/crates/ncx-core/src/tools.rs` | 39 | const | `ALWAYS_VISIBLE_TOOLS` |  |
| `rust/crates/ncx-core/src/tools.rs` | 57 | struct | `ToolCatalogEntry` |  |
| `rust/crates/ncx-core/src/tools.rs` | 67 | enum | `ApprovalDecision` |  |
| `rust/crates/ncx-core/src/tools.rs` | 75 | fn | `approved` |  |
| `rust/crates/ncx-core/src/tools.rs` | 84 | struct | `SessionGrants` |  |
| `rust/crates/ncx-core/src/tools.rs` | 98 | trait | `ApprovalHandler` |  |
| `rust/crates/ncx-core/src/tools.rs` | 99 | fn | `request` |  |
| `rust/crates/ncx-core/src/tools.rs` | 104 | struct | `ToolContext` |  |
| `rust/crates/ncx-core/src/tools.rs` | 169 | fn | `new` |  |
| `rust/crates/ncx-core/src/tools.rs` | 202 | fn | `with_search` |  |
| `rust/crates/ncx-core/src/tools.rs` | 209 | fn | `with_memory` |  |
| `rust/crates/ncx-core/src/tools.rs` | 215 | fn | `with_approver` |  |
| `rust/crates/ncx-core/src/tools.rs` | 221 | fn | `with_user_question_handler` |  |
| `rust/crates/ncx-core/src/tools.rs` | 225 | fn | `with_goal_service` |  |
| `rust/crates/ncx-core/src/tools.rs` | 232 | fn | `with_lsp_provider` |  |
| `rust/crates/ncx-core/src/tools.rs` | 238 | fn | `with_approval_policy` |  |
| `rust/crates/ncx-core/src/tools.rs` | 244 | fn | `with_require_edit_approval` |  |
| `rust/crates/ncx-core/src/tools.rs` | 250 | fn | `with_plan_mode` |  |
| `rust/crates/ncx-core/src/tools.rs` | 257 | fn | `with_session_grants` |  |
| `rust/crates/ncx-core/src/tools.rs` | 263 | fn | `with_timeout` |  |
| `rust/crates/ncx-core/src/tools.rs` | 269 | fn | `with_hooks` |  |
| `rust/crates/ncx-core/src/tools.rs` | 275 | fn | `with_skills` |  |
| `rust/crates/ncx-core/src/tools.rs` | 281 | fn | `with_genome` |  |
| `rust/crates/ncx-core/src/tools.rs` | 285 | fn | `with_context_entries` |  |
| `rust/crates/ncx-core/src/tools.rs` | 294 | trait | `Tool` |  |
| `rust/crates/ncx-core/src/tools.rs` | 295 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools.rs` | 296 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools.rs` | 297 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools.rs` | 301 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 308 | fn | `call_is_read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 311 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools.rs` | 313 | fn | `to_schema` |  |
| `rust/crates/ncx-core/src/tools.rs` | 328 | struct | `ToolRegistry` |  |
| `rust/crates/ncx-core/src/tools.rs` | 338 | fn | `harness_diagnostics` |  |
| `rust/crates/ncx-core/src/tools.rs` | 383 | fn | `replace_service` |  |
| `rust/crates/ncx-core/src/tools.rs` | 389 | fn | `service` |  |
| `rust/crates/ncx-core/src/tools.rs` | 397 | fn | `install_plugin` |  |
| `rust/crates/ncx-core/src/tools.rs` | 407 | fn | `new` |  |
| `rust/crates/ncx-core/src/tools.rs` | 412 | fn | `empty` |  |
| `rust/crates/ncx-core/src/tools.rs` | 425 | fn | `register_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 441 | fn | `unregister_middleware` |  |
| `rust/crates/ncx-core/src/tools.rs` | 449 | fn | `register` |  |
| `rust/crates/ncx-core/src/tools.rs` | 476 | fn | `replace_tools` |  |
| `rust/crates/ncx-core/src/tools.rs` | 505 | fn | `rebuild_tool_indexes` |  |
| `rust/crates/ncx-core/src/tools.rs` | 533 | fn | `schema_for` |  |
| `rust/crates/ncx-core/src/tools.rs` | 544 | fn | `get` |  |
| `rust/crates/ncx-core/src/tools.rs` | 548 | fn | `is_read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 552 | fn | `call_is_read_only` |  |
| `rust/crates/ncx-core/src/tools.rs` | 560 | fn | `schemas` |  |
| `rust/crates/ncx-core/src/tools.rs` | 567 | fn | `schemas_for_query` |  |
| `rust/crates/ncx-core/src/tools.rs` | 570 | fn | `schemas_limited_for_query` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 1 | module | `builtins` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 2 | struct | `UpdatePlanTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 12 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 30 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 71 | struct | `ShellTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 76 | fn | `needs_escalation` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 89 | fn | `incompatible_windows_syntax` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 107 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 110 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 116 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 128 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 158 | fn | `resolve_shell_workdir` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 167 | fn | `authorize_shell` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 195 | fn | `request_shell_approval` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 222 | fn | `run_shell` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 241 | fn | `approve_failed_retry` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 263 | struct | `RememberTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 267 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 270 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 276 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 286 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 319 | struct | `SkillTool` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 323 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 326 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 332 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 341 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/builtins.rs` | 344 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 1 | module | `catalog` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 2 | struct | `ToolSearchTool` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 12 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 22 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 25 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 67 | fn | `tool_words` |  |
| `rust/crates/ncx-core/src/tools/catalog.rs` | 81 | fn | `catalog_score` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 1 | module | `execution` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 3 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 8 | fn | `execute_with_recovery` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 49 | fn | `execute_attempt` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 68 | fn | `effective_context` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 82 | fn | `enter_middleware` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 105 | fn | `leave_middleware` |  |
| `rust/crates/ncx-core/src/tools/execution.rs` | 122 | fn | `execute_with_hooks` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 1 | module | `file` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 2 | struct | `ReadFileTool` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 6 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 9 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 14 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 25 | fn | `read_only` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 28 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 71 | struct | `ApplyPatchTool` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 75 | fn | `name` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 78 | fn | `description` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 104 | fn | `parameters` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 113 | fn | `execute` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 150 | fn | `escaping_targets` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 169 | fn | `approve_patch` |  |
| `rust/crates/ncx-core/src/tools/file.rs` | 198 | fn | `patch_approval_details` |  |
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
| `rust/crates/ncx-core/src/tools/tests.rs` | 296 | fn | `windows_shell_rejects_posix_only_syntax_before_execution` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 313 | fn | `essential_recursive_discovery_tools_are_always_visible` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 334 | fn | `schema_desc` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 347 | fn | `empty_genome_leaves_schema_and_catalog_byte_identical` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 364 | fn | `genome_override_reaches_schema_and_catalog` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 394 | fn | `skill_tool_loads_body_and_reports_unknown` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 426 | fn | `skill_tool_registered_only_when_skills_present` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 446 | fn | `pre_tool_hook_can_block_execution` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 465 | fn | `post_tool_hook_output_is_returned` |  |
| `rust/crates/ncx-core/src/tools/tests.rs` | 485 | fn | `compaction_recovery_blocks_writes_but_keeps_reads_available` |  |
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
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 1 | module | `workspace_promotion` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 13 | struct | `FileFingerprint` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 17 | type | `WorkspaceSnapshot` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 19 | fn | `snapshot` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 43 | fn | `promote` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 88 | fn | `fingerprint` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 106 | fn | `remove_empty_parents` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 119 | fn | `temp` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 123 | fn | `setup` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 138 | fn | `promotes_add_modify_and_delete` |  |
| `rust/crates/ncx-core/src/workspace_promotion.rs` | 160 | fn | `conflict_fails_before_any_change_is_applied` |  |
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
| `rust/crates/ncx-mcp/src/lib.rs` | 21 | const | `PROTOCOL` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 23 | const | `REQ_TIMEOUT` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 28 | struct | `McpToolDef` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 47 | struct | `McpToolAnnotations` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 59 | fn | `explicitly_read_only` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 65 | struct | `McpClient` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 76 | fn | `connect` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 104 | fn | `initialize` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 117 | fn | `write_msg` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 127 | fn | `notify` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 135 | fn | `request` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 178 | fn | `list_tools` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 188 | fn | `call_tool` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 199 | fn | `parse_tool_def` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 234 | fn | `hide_child_console` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 241 | fn | `hide_child_console` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 244 | fn | `drop` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 252 | fn | `format_content` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 295 | fn | `unique_temp_dir` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 297 | static | `SEQUENCE` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 318 | fn | `format_content_joins_text_blocks` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 324 | fn | `format_content_includes_structured` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 334 | fn | `format_content_empty_error` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 342 | fn | `mcp_annotations_round_trip_with_wire_camel_case` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 366 | fn | `malformed_or_partial_mcp_annotations_are_retained_for_fail_closed_policy` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 385 | fn | `write_mock_server` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 413 | fn | `python` |  |
| `rust/crates/ncx-mcp/src/lib.rs` | 420 | fn | `connects_lists_and_calls_against_mock_server` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 7 | const | `PROTOCOL_VERSION` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 9 | fn | `default_harness_profile` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 21 | fn | `new` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 28 | fn | `as_str` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 35 | fn | `fmt` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 49 | enum | `GoalPhase` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 58 | struct | `GoalBlockReason` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 68 | struct | `GoalSnapshot` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 83 | struct | `GoalRef` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 90 | enum | `GoalActivation` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 99 | struct | `GoalView` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 106 | struct | `ThreadMetadata` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 123 | enum | `ThreadItem` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 174 | fn | `id` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 190 | enum | `TurnStatus` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 200 | enum | `ExecutionMode` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 208 | struct | `TurnUsage` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 219 | struct | `Turn` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 234 | struct | `Thread` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 243 | fn | `into_visible` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 276 | struct | `StoredModelContext` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 289 | enum | `ClientRequest` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 586 | enum | `ResponsePayload` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 619 | struct | `ServerResponse` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 631 | enum | `Event` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 662 | struct | `EventEnvelope` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 671 | fn | `new` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 683 | enum | `ProtocolError` |  |
| `rust/crates/ncx-protocol/src/lib.rs` | 688 | fn | `fmt` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 4 | fn | `event_round_trip_preserves_thread_and_turn_ownership` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 20 | fn | `durable_ids_reject_empty_values` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 28 | fn | `legacy_turn_and_submit_default_to_agent_mode` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 49 | fn | `completed_thread` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 75 | fn | `visible_projection_keeps_each_request_and_only_the_final_answer` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 115 | fn | `visible_projection_preserves_generated_artifacts` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 137 | fn | `assert_round_trip` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 150 | fn | `memory_list_request_carries_workspace_snapshot` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 163 | fn | `goal_thread_and_item_requests_use_frontend_camel_case` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 223 | fn | `plugin_and_interaction_requests_use_frontend_camel_case` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 261 | fn | `forge_and_settings_requests_use_frontend_camel_case` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 320 | fn | `status_refresh_generation_is_optional_but_legacy_unit_requests_are_rejected` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 340 | fn | `permission_mode_request_serializes_thread_id_in_camel_case` |  |
| `rust/crates/ncx-protocol/src/tests.rs` | 355 | fn | `legacy_thread_metadata_defaults_harness_profile_to_full` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 1 | module | `anthropic` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 8 | struct | `AnthropicProvider` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 17 | fn | `new` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 29 | fn | `confirmed_model` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 32 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 76 | fn | `convert_tool` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 83 | fn | `convert_messages` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 128 | fn | `parse_response` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 181 | fn | `serve_once` |  |
| `rust/crates/ncx-provider/src/anthropic.rs` | 210 | fn | `failed_chat_preserves_status_without_echoing_html_body` |  |
| `rust/crates/ncx-provider/src/api.rs` | 1 | module | `api` |  |
| `rust/crates/ncx-provider/src/api.rs` | 8 | enum | `StreamDelta` |  |
| `rust/crates/ncx-provider/src/api.rs` | 14 | trait | `Provider` |  |
| `rust/crates/ncx-provider/src/api.rs` | 15 | fn | `model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 19 | fn | `confirmed_model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 22 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/api.rs` | 29 | fn | `chat_streaming` |  |
| `rust/crates/ncx-provider/src/api.rs` | 50 | fn | `model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 53 | fn | `confirmed_model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 57 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/api.rs` | 70 | fn | `chat_streaming` |  |
| `rust/crates/ncx-provider/src/api.rs` | 100 | fn | `model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 103 | fn | `confirmed_model` |  |
| `rust/crates/ncx-provider/src/api.rs` | 106 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/api.rs` | 117 | fn | `provider_error` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 1 | module | `chat_probe` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 14 | const | `MAX_PROBE_RESPONSE_BYTES` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 18 | struct | `ProviderChatProbeRequest` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 27 | fn | `new` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 44 | struct | `ProviderChatProbeResult` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 49 | trait | `ProviderChatProbeClient` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 51 | fn | `probe` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 55 | struct | `HttpProviderChatProbeClient` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 58 | fn | `probe` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 147 | fn | `chat_probe_endpoint` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 173 | fn | `server` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 187 | fn | `response` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 196 | fn | `openai_probe_uses_chat_completions_and_reports_confirmed_model` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 220 | fn | `anthropic_probe_uses_messages_headers_and_shape` |  |
| `rust/crates/ncx-provider/src/chat_probe.rs` | 242 | fn | `failed_probe_exposes_only_status_not_provider_body_or_token` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 1 | module | `dashscope_media` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 6 | const | `DASHSCOPE_MEDIA_BASE_URL` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 8 | const | `DEFAULT_IMAGE_MODEL` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 9 | const | `DEFAULT_VIDEO_MODEL` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 12 | enum | `MediaKind` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 18 | struct | `MediaGenerationRequest` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 27 | struct | `MediaGenerationResult` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 34 | trait | `MediaProvider` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 35 | fn | `generate` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 40 | struct | `DashScopeMediaProvider` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 52 | fn | `new` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 68 | fn | `with_models` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 73 | fn | `model` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 80 | fn | `endpoint` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 88 | fn | `request_body` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 112 | fn | `submit` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 140 | fn | `wait_result` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 185 | fn | `generate` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 194 | fn | `collect_urls` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 211 | fn | `api_error` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 226 | fn | `image_and_video_requests_use_distinct_models_and_parameters` |  |
| `rust/crates/ncx-provider/src/dashscope_media.rs` | 250 | fn | `result_parser_supports_image_and_video_shapes` |  |
| `rust/crates/ncx-provider/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 1 | module | `model_catalog` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 9 | const | `MAX_RESPONSE_BYTES` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 11 | const | `MAX_MODELS` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 14 | struct | `ProviderCatalogRequest` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 22 | fn | `new` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 37 | struct | `DiscoveredProviderModel` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 44 | trait | `ProviderCatalogClient` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 46 | fn | `discover` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 53 | struct | `HttpProviderCatalogClient` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 56 | fn | `discover` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 115 | fn | `catalog_endpoint` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 144 | fn | `parse_catalog_models` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 159 | fn | `normalize_model` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 182 | fn | `price_per_million` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 196 | fn | `valid_model_id` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 211 | fn | `serve_once` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 240 | fn | `endpoint_preserves_prefix_and_replaces_known_request_suffixes` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 257 | fn | `parser_normalizes_common_shapes_prices_and_deduplicates` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 270 | fn | `parser_accepts_models_and_root_array_variants` |  |
| `rust/crates/ncx-provider/src/model_catalog.rs` | 282 | fn | `http_client_uses_protocol_specific_auth_and_never_echoes_error_body` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 1 | module | `provider` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 21 | const | `DEFAULT_STREAM_OPEN_TIMEOUT_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 23 | const | `STREAM_OPEN_TIMEOUT_MIN_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 24 | const | `STREAM_OPEN_TIMEOUT_MAX_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 25 | const | `DEFAULT_STREAM_IDLE_TIMEOUT_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 26 | const | `STREAM_IDLE_TIMEOUT_MIN_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 27 | const | `STREAM_IDLE_TIMEOUT_MAX_S` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 28 | const | `MAX_SSE_LINE_BYTES` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 31 | fn | `stream_open_timeout_s` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 40 | fn | `backoff_sleep` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 46 | fn | `stream_open_timeout_from` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 55 | fn | `stream_idle_timeout_s` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 63 | fn | `stream_idle_timeout_from` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 76 | struct | `DeepSeekProvider` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 87 | const | `SUPPORTS_STREAMING` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 88 | fn | `new` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 92 | fn | `with_opts` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 114 | fn | `confirmed_model` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 118 | fn | `body` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 138 | fn | `chat` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 172 | fn | `post` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 207 | fn | `chat_stream` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 297 | fn | `consume_sse` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 374 | fn | `finish_stream` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 385 | fn | `feed_sse_bytes` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 436 | struct | `StreamAgg` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 447 | struct | `ToolFrag` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 454 | fn | `progress_marker` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 469 | fn | `has_progress` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 473 | fn | `ingest` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 536 | fn | `finish` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 567 | struct | `HttpErr` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 574 | fn | `from_reqwest` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 593 | fn | `serve_once` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 622 | fn | `stream_open_timeout_defaults_when_unset` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 628 | fn | `stream_open_timeout_honors_env_within_bounds` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 633 | fn | `stream_open_timeout_clamps_and_tolerates_garbage` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 640 | fn | `stream_idle_timeout_is_bounded_and_tolerates_garbage` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 649 | fn | `stream_progress_ignores_heartbeats_but_tracks_model_and_tool_deltas` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 670 | fn | `provider_default_max_retries_is_three` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 676 | fn | `provider_sets_max_retries` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 688 | fn | `endpoint_appends_chat_completions_without_double_slash` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 694 | fn | `failed_chat_exposes_only_status_not_upstream_body` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 716 | fn | `failed_stream_exposes_only_status_not_upstream_body` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 733 | fn | `stream_agg_aggregates_content_and_tool_calls` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 778 | fn | `stream_agg_synthesizes_id_when_missing` |  |
| `rust/crates/ncx-provider/src/provider.rs` | 794 | fn | `sse_decoder_preserves_chinese_split_at_every_byte` |  |
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
| `rust/crates/ncx-sandbox/src/lib.rs` | 21 | struct | `PolicyService` |  |
| `rust/crates/ncx-sandbox/src/lib.rs` | 28 | type | `PolicySnapshot` |  |
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
| `rust/crates/ncx-sandbox/src/policy.rs` | 156 | fn | `base` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 157 | static | `SEQUENCE` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 170 | fn | `read_only_forbids_writes` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 179 | fn | `workspace_write_allows_inside_only` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 190 | fn | `workspace_write_denies_system_temp_by_default` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 205 | fn | `workspace_write_honors_extra_writable_roots` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 214 | fn | `danger_full_access_allows_everything` |  |
| `rust/crates/ncx-sandbox/src/policy.rs` | 222 | fn | `relative_path_resolves_against_workspace` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 21 | trait | `ThreadStore` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 23 | fn | `create` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 24 | fn | `create_many` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 28 | fn | `create_with_rollback` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 35 | fn | `fork_with_rollback` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 50 | fn | `discard_if_unchanged` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 60 | fn | `mark_runtime_activation` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 61 | fn | `list` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 62 | fn | `read` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 67 | fn | `read_with_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 71 | fn | `read_model_context` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 75 | fn | `replace_model_context` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 81 | fn | `read_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 84 | fn | `compare_and_set_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 92 | fn | `claim_goal_round` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 99 | fn | `update_metadata` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 106 | fn | `set_harness_profile_if_idle` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 112 | fn | `fork` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 113 | fn | `claim_turn` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 114 | fn | `append_item` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 121 | fn | `finish_turn` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 133 | struct | `PersistedState` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 154 | struct | `ThreadRollbackSnapshot` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 162 | fn | `thread_id` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 168 | enum | `GoalExpectation` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 172 | struct | `ActiveTurn` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 179 | struct | `StoreState` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 183 | struct | `JsonThreadStore` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 190 | fn | `open` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 206 | fn | `mutate` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 221 | fn | `inspect` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 235 | fn | `snapshot_for_state` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 263 | fn | `advance_thread_write_epoch` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 270 | fn | `fork_into_state` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 322 | fn | `default_thread_store_path` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 332 | fn | `create` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 346 | fn | `create_with_rollback` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 364 | fn | `create_many` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 382 | fn | `discard_if_unchanged` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 412 | fn | `mark_runtime_activation` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 422 | fn | `list` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 435 | fn | `read` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 439 | fn | `read_with_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 452 | fn | `read_model_context` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 459 | fn | `replace_model_context` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 483 | fn | `read_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 487 | fn | `compare_and_set_goal` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 535 | fn | `update_metadata` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 551 | fn | `set_harness_profile_if_idle` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 584 | fn | `claim_goal_round` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 658 | fn | `fork` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 662 | fn | `fork_with_rollback` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 681 | fn | `claim_turn` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 690 | fn | `append_item` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 720 | fn | `finish_turn` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 752 | enum | `ThreadStoreError` |  |
| `rust/crates/ncx-thread-store/src/lib.rs` | 773 | fn | `fmt` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 1 | module | `storage` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 8 | fn | `claim_turn_in_state` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 53 | fn | `require_owner` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 68 | fn | `find_turn_mut` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 84 | fn | `acquire_global_lock` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 100 | fn | `acquire_turn_lease` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 122 | fn | `lock_is_contended` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 126 | fn | `recover_orphaned_turns` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 165 | fn | `turn_lock_path` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 183 | fn | `load_state` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 199 | fn | `recover_state` |  |
| `rust/crates/ncx-thread-store/src/storage.rs` | 219 | fn | `save_state` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 1 | module | `tests` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 6 | fn | `temp_store` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 14 | fn | `thread` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 29 | fn | `goal` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 43 | fn | `write_epoch` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 57 | fn | `goal_compare_and_set_is_durable_and_rejects_stale_revision_without_writing` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 102 | fn | `fork_copies_durable_goal_snapshot` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 116 | fn | `legacy_store_without_goal_map_opens_with_no_goal` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 138 | fn | `read_with_goal_returns_the_thread_and_goal_from_one_persisted_snapshot` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 183 | fn | `turn` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 196 | fn | `goal_turn` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 211 | fn | `goal_round_admission_atomically_claims_turn_and_increments_counter` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 244 | fn | `rejected_goal_round_changes_neither_turn_nor_counter` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 267 | fn | `one_thread_accepts_only_one_active_turn` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 290 | fn | `harness_profile_update_rechecks_turns_inside_the_atomic_store_transaction` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 314 | fn | `different_threads_hold_active_turns_concurrently` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 333 | fn | `items_are_owned_by_the_claimed_turn_and_persisted` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 359 | fn | `fork_keeps_history_but_changes_durable_identity` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 380 | fn | `completed_turn_survives_store_reopen` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 414 | fn | `compacted_model_context_is_replaced_and_survives_reopen` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 439 | fn | `fork_copies_model_context_under_the_new_thread_identity` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 458 | fn | `rollback_receipt_discards_an_unchanged_fork_and_all_of_its_side_domains` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 491 | fn | `rollback_receipt_never_discards_a_target_changed_after_provisioning` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 509 | fn | `rollback_receipt_rejects_an_aba_write_that_restores_the_original_thread` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 534 | fn | `rollback_receipt_rejects_aba_writes_in_side_domains` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 569 | fn | `all_successful_thread_write_apis_advance_the_durable_epoch` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 635 | fn | `provisioning_and_goal_round_writes_initialize_or_advance_epochs` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 690 | fn | `running_turn_is_recovered_as_failed_after_restart` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 714 | fn | `second_store_does_not_recover_or_overwrite_a_live_owner` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 753 | fn | `cross_process_lease_helper` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 774 | fn | `live_turn_lease_is_respected_across_processes` |  |
| `rust/crates/ncx-thread-store/src/tests.rs` | 828 | fn | `corrupt_primary_recovers_from_last_backup` |  |
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
| `rust/crates/ncx-tools/src/executor.rs` | 21 | const | `MAX_OUTPUT` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 26 | struct | `ExecResult` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 36 | fn | `ok` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 41 | fn | `render` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 72 | struct | `PolicyExecutor` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 78 | fn | `default` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 87 | fn | `new` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 92 | fn | `run` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 99 | fn | `run_with_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 172 | fn | `command_with_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 190 | const | `CREATE_NO_WINDOW` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 197 | fn | `base_command` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 217 | fn | `which_bash` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 227 | fn | `build_env` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 274 | struct | `Job` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 288 | fn | `contain` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 326 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 334 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 355 | fn | `ok_requires_zero_exit` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 370 | fn | `render_includes_exit_code` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 382 | fn | `render_includes_stderr_and_timeout` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 396 | fn | `render_sandbox_denied` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 406 | fn | `render_truncates_huge_output` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 417 | fn | `run_echo_returns_stdout` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 431 | fn | `windows_shell_preserves_chinese_output` |  |
| `rust/crates/ncx-tools/src/executor.rs` | 446 | fn | `run_nonzero_exit_is_captured` |  |
| `rust/crates/ncx-tools/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 1 | module | `managed` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 15 | const | `MAX_BUFFERED_BYTES` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 19 | struct | `ProcessOutputChunk` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 26 | struct | `ProcessSnapshot` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 32 | struct | `ManagedProcess` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 44 | struct | `OutputBuffer` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 51 | fn | `spawn_managed` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 93 | fn | `poll` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 121 | fn | `write_stdin` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 136 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 148 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 155 | fn | `spawn_reader` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 196 | fn | `push_chunk` |  |
| `rust/crates/ncx-tools/src/managed.rs` | 213 | fn | `managed_process_returns_incremental_output_and_exit` |  |
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
| `rust/crates/ncx-tools/src/patch.rs` | 446 | static | `TEST_TMP_SEQUENCE` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 448 | fn | `tmpdir` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 464 | fn | `allow_all` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 470 | fn | `tmpdir_creates_unique_directories` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 483 | fn | `parse_requires_begin_and_end` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 489 | fn | `parse_add_file` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 499 | fn | `parse_rejects_add_line_without_plus` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 505 | fn | `parse_update_with_locator_and_change` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 516 | fn | `add_file_writes_to_disk` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 530 | fn | `add_file_rejects_existing` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 543 | fn | `update_replaces_matched_lines` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 558 | fn | `update_uses_locator_to_disambiguate` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 571 | fn | `update_with_whitespace_fallback` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 583 | fn | `update_failure_to_locate_is_atomic` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 597 | fn | `delete_file` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 611 | fn | `move_file_writes_dest_removes_source` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 628 | fn | `unwritable_path_blocks_whole_patch` |  |
| `rust/crates/ncx-tools/src/patch.rs` | 640 | fn | `summary_orders_a_r_m_d` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 1 | module | `pty` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 13 | const | `MAX_BUFFERED_BYTES` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 17 | struct | `PtyOutputChunk` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 23 | struct | `PtySnapshot` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 31 | struct | `PtyProcess` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 42 | struct | `OutputBuffer` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 49 | fn | `spawn_pty` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 94 | fn | `write` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 107 | fn | `resize` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 118 | fn | `poll` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 142 | fn | `terminate` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 154 | fn | `drop` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 158 | fn | `shell_command` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 175 | fn | `spawn_reader` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 224 | fn | `push_chunk` |  |
| `rust/crates/ncx-tools/src/pty.rs` | 243 | fn | `raw_pty_accepts_stdin_and_returns_output` |  |
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
| `rust/crates/ncx-tools/src/text_encoding.rs` | 1 | module | `text_encoding` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 12 | enum | `TextEncoding` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 22 | fn | `label` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 32 | fn | `is_plain_utf8` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 39 | struct | `DecodedText` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 45 | fn | `decode_text` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 101 | fn | `decode_text_lossy` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 112 | struct | `Utf8StreamDecoder` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 117 | fn | `push` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 120 | fn | `finish` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 124 | fn | `decode` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 164 | fn | `decoded` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 171 | fn | `decode_with` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 185 | fn | `likely_bomless_utf16` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 206 | fn | `looks_binary` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 223 | fn | `contains_cjk` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 234 | fn | `decodes_utf8_bom_utf16_and_gb18030` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 251 | fn | `rejects_obvious_binary_data` |  |
| `rust/crates/ncx-tools/src/text_encoding.rs` | 256 | fn | `stream_decoder_preserves_utf8_split_at_every_byte` |  |
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
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 1 | module | `p1_seedance_tos_smoke` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 15 | const | `MODEL` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 17 | const | `PROJECT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 18 | const | `SHOT_ID` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 19 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 37 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 281 | fn | `seed_db` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 297 | fn | `poll_until_succeeded` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 331 | fn | `elapsed_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_seedance_tos_smoke.rs` | 335 | fn | `total_tokens` |  |
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
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 159 | struct | `P1DryRunWorkflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 163 | struct | `P1LiveSeedanceWorkflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 168 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 193 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 242 | fn | `run` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 270 | fn | `approve` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 275 | fn | `is_approved` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 281 | fn | `main` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 298 | fn | `temporal_client` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 305 | fn | `run_worker` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 325 | fn | `start_dry_run_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 344 | fn | `wait_dry_run_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 355 | fn | `start_live_seedance_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 375 | fn | `wait_live_seedance_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 386 | fn | `start_workflow` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 405 | fn | `signal_approval` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 420 | fn | `wait_result` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 431 | fn | `task_queue` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 435 | fn | `workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 439 | fn | `dry_run_workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 446 | fn | `live_workflow_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 450 | fn | `dry_run_out_dir` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 463 | fn | `live_out_dir` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 476 | fn | `shot_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 480 | fn | `env_or_default` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 488 | fn | `print_help` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 530 | fn | `submit_live_seedance_job_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 575 | fn | `poll_live_seedance_job_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 687 | fn | `persist_live_seedance_outputs_activity` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 884 | fn | `require_live_opt_in` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 899 | fn | `resolve_ark_api_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 910 | fn | `seed_live_db` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 949 | fn | `live_seedance_payload` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 962 | fn | `live_db_path` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 966 | fn | `write_live_marker` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 974 | fn | `parse_state` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 978 | fn | `required_string` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 988 | fn | `state_kind` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 999 | fn | `state_reason` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1010 | fn | `artifact_exists` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1022 | fn | `artifact_tos_uri` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1034 | fn | `validation_exists` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1046 | fn | `record_validation_once` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1054 | fn | `live_video_tos_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1061 | fn | `live_rough_tos_key` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1068 | fn | `live_video_artifact_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1072 | fn | `live_rough_artifact_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1079 | fn | `live_video_validation_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1086 | fn | `live_rough_validation_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1093 | fn | `sanitize_id` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1111 | fn | `now_unix_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1118 | fn | `elapsed_since_unix_ms` |  |
| `rust/crates/ncx-video-agent/src/bin/p1_temporal_probe.rs` | 1122 | fn | `total_tokens` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 1 | module | `db` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 8 | struct | `Database` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 14 | fn | `open` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 18 | fn | `connection` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 22 | fn | `connection_mut` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 26 | fn | `create_project` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 35 | fn | `create_chapter` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 44 | fn | `create_scene` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 55 | fn | `create_shot` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 83 | fn | `create_artifact` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 100 | fn | `create_project_artifact` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 118 | fn | `open_db` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 128 | fn | `require_json1` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 136 | fn | `init_schema` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 257 | fn | `ensure_artifact_project_id` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 280 | fn | `schema_creates_tables_wal_and_json1` |  |
| `rust/crates/ncx-video-agent/src/db.rs` | 317 | fn | `duplicate_idempotency_key_is_rejected` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 1 | module | `dry_run` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 25 | struct | `LocalDryRunOutput` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 33 | fn | `run_local_p1_dry_run` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 251 | fn | `elapsed_ms` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 255 | fn | `seed_project` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 262 | fn | `seed_agent_artifacts` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 404 | fn | `write_structured_agent_pass` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 430 | fn | `remove_previous_sqlite` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 450 | fn | `make_color_clip` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 469 | fn | `make_local_tts_placeholder_audio` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 490 | fn | `local_file_hash_marker` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 495 | fn | `sha256_file_hash_marker` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 523 | fn | `local_p1_dry_run_produces_rough_cut_and_trace` |  |
| `rust/crates/ncx-video-agent/src/dry_run.rs` | 590 | fn | `local_p1_dry_run_can_be_repeated_in_same_output_dir` |  |
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
| `rust/crates/ncx-video-agent/src/jobs.rs` | 45 | fn | `submit_job_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 104 | fn | `settle_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 138 | fn | `mark_job_status` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 151 | fn | `record_job_latency_ms` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 168 | fn | `fail_job_and_release_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 205 | fn | `reserve_and_insert_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 258 | fn | `release_failed_reservation` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 285 | fn | `load_job_by_key` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 299 | fn | `load_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 313 | fn | `row_to_job` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 340 | fn | `canonical_json` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 382 | fn | `seeded_db` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 403 | fn | `idempotency_key_canonicalizes_json_object_order` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 417 | fn | `submit_job_is_idempotent_and_reserves_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 473 | fn | `submit_job_refuses_to_resubmit_ambiguous_existing_job_without_provider_id` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 516 | fn | `submit_job_refuses_to_retry_submit_failed_job_without_provider_id` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 558 | fn | `settle_budget_reconciles_project_and_job_once` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 591 | fn | `concurrent_reservations_never_exceed_project_budget` |  |
| `rust/crates/ncx-video-agent/src/jobs.rs` | 644 | fn | `failed_provider_job_releases_reserved_budget_once` |  |
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
| `rust/crates/ncx-video-agent/src/l0.rs` | 294 | struct | `ShotForL0` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 301 | fn | `is_boundary_reference` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 305 | fn | `candidate_text_fields` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 313 | fn | `is_cjk` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 320 | fn | `parse_fasttext_label` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 333 | fn | `seeded_scene` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 344 | fn | `l0_rejects_unclosed_references` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 369 | fn | `l0_repairs_english_text_when_chinese_required` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 394 | fn | `l0_passes_valid_chinese_scene_and_duration_budget` |  |
| `rust/crates/ncx-video-agent/src/l0.rs` | 440 | fn | `parses_fasttext_language_labels` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 1 | module | `lib` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 66 | enum | `VideoAgentError` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 94 | type | `Result` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 102 | static | `NEXT_ID` |  |
| `rust/crates/ncx-video-agent/src/lib.rs` | 104 | fn | `temp_db_path` |  |
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
| `rust/crates/ncx-video-agent/src/structured.rs` | 22 | fn | `stage` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 30 | fn | `artifact_kind` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 42 | struct | `StructuredValidationReport` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 50 | fn | `pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 58 | fn | `repair` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 68 | fn | `validate_brief_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 111 | fn | `validate_chapters_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 162 | fn | `validate_shots_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 263 | fn | `validate_assets_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 318 | fn | `record_structured_validation_if_pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 347 | fn | `record_structured_agent_validation_if_pass` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 391 | fn | `json_content_hash` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 404 | fn | `chapter_budgets_from_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 418 | fn | `shot_ids_from_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 431 | fn | `insert_project_artifact` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 453 | fn | `finish` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 465 | fn | `required_string` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 469 | fn | `is_boundary_reference` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 473 | fn | `node_kind_name` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 480 | fn | `reasoning_mode_name` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 488 | fn | `positive_or_null` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 496 | fn | `canonical_json` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 539 | fn | `structured_chain_validates_brief_chapters_shots_and_assets` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 563 | fn | `shots_validator_rejects_duration_reference_and_missing_routing_fields` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 585 | fn | `invalid_artifact_does_not_get_a_pass_record` |  |
| `rust/crates/ncx-video-agent/src/structured.rs` | 604 | fn | `agent_validation_records_context_packet_contract_evidence` |  |
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
| `rust/crates/ncx-video-agent/src/trace.rs` | 224 | fn | `export_artifact_validations_for_row` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 255 | fn | `query_json_rows` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 270 | fn | `query_json_rows_without_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 301 | fn | `trace_exports_jobs_artifacts_and_validation_by_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 400 | fn | `trace_exports_only_project_owned_artifacts` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 448 | fn | `project_shot_trace_rejects_cross_project_shot` |  |
| `rust/crates/ncx-video-agent/src/trace.rs` | 471 | fn | `trace_exports_live_seedance_tos_shape_for_strict_verifier` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 1 | module | `validation` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 7 | struct | `ValidationInput` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 18 | fn | `record_validation` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 40 | fn | `assert_artifacts_passed` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 66 | fn | `db_with_artifact` |  |
| `rust/crates/ncx-video-agent/src/validation.rs` | 89 | fn | `downstream_contract_rejects_missing_and_non_pass_records` |  |
## Web

| 路径 | 行 | 类型 | 名称 | 摘要 |
| --- | ---: | --- | --- | --- |
| `rust/gui/src/App.svelte` | 1 | module | `App` |  |
| `rust/gui/src/App.svelte` | 30 | symbol | `sidebar` |  |
| `rust/gui/src/App.svelte` | 32 | symbol | `usage` |  |
| `rust/gui/src/App.svelte` | 33 | symbol | `activeView` |  |
| `rust/gui/src/App.svelte` | 34 | symbol | `ThemeMode` |  |
| `rust/gui/src/App.svelte` | 35 | symbol | `themeMode` |  |
| `rust/gui/src/App.svelte` | 36 | symbol | `setTheme` |  |
| `rust/gui/src/App.svelte` | 42 | symbol | `cycleTheme` |  |
| `rust/gui/src/App.svelte` | 46 | symbol | `isImage` |  |
| `rust/gui/src/App.svelte` | 50 | symbol | `thread` |  |
| `rust/gui/src/App.svelte` | 56 | symbol | `note` |  |
| `rust/gui/src/App.svelte` | 57 | symbol | `workspace` |  |
| `rust/gui/src/App.svelte` | 58 | symbol | `threadLifecycle` |  |
| `rust/gui/src/App.svelte` | 62 | symbol | `goalController` |  |
| `rust/gui/src/App.svelte` | 63 | symbol | `composer` |  |
| `rust/gui/src/App.svelte` | 64 | symbol | `fileBrowser` |  |
| `rust/gui/src/App.svelte` | 65 | symbol | `gitWorkspace` |  |
| `rust/gui/src/App.svelte` | 66 | symbol | `checkpointController` |  |
| `rust/gui/src/App.svelte` | 67 | symbol | `memoryController` |  |
| `rust/gui/src/App.svelte` | 68 | symbol | `forgeController` |  |
| `rust/gui/src/App.svelte` | 69 | symbol | `pluginController` |  |
| `rust/gui/src/App.svelte` | 70 | symbol | `dshSlots` |  |
| `rust/gui/src/App.svelte` | 71 | symbol | `modelControls` |  |
| `rust/gui/src/App.svelte` | 76 | symbol | `settingsController` |  |
| `rust/gui/src/App.svelte` | 82 | symbol | `panels` |  |
| `rust/gui/src/App.svelte` | 83 | symbol | `slashController` |  |
| `rust/gui/src/App.svelte` | 101 | symbol | `runtime` |  |
| `rust/gui/src/App.svelte` | 109 | symbol | `observedGoalThread` |  |
| `rust/gui/src/App.svelte` | 110 | symbol | `observedGoalBusy` |  |
| `rust/gui/src/App.svelte` | 112 | symbol | `threadId` |  |
| `rust/gui/src/App.svelte` | 113 | symbol | `busy` |  |
| `rust/gui/src/App.svelte` | 119 | symbol | `scroller` |  |
| `rust/gui/src/App.svelte` | 121 | symbol | `scrollDown` |  |
| `rust/gui/src/App.svelte` | 126 | symbol | `savedTheme` |  |
| `rust/gui/src/App.svelte` | 128 | symbol | `disposed` |  |
| `rust/gui/src/App.svelte` | 149 | symbol | `openSettings` |  |
| `rust/gui/src/components/AppUtilityPanels.svelte` | 1 | module | `AppUtilityPanels` |  |
| `rust/gui/src/components/AppUtilityPanels.svelte` | 18 | symbol | `ThemeMode` |  |
| `rust/gui/src/components/Composer.svelte` | 1 | module | `Composer` |  |
| `rust/gui/src/components/Composer.svelte` | 4 | symbol | `MenuOption` |  |
| `rust/gui/src/components/Composer.svelte` | 5 | symbol | `SlashCommand` |  |
| `rust/gui/src/components/Composer.svelte` | 6 | symbol | `QueuedTurn` |  |
| `rust/gui/src/components/ConversationView.svelte` | 1 | module | `ConversationView` |  |
| `rust/gui/src/components/ConversationView.svelte` | 4 | symbol | `ToolEntry` |  |
| `rust/gui/src/components/ConversationView.svelte` | 5 | symbol | `ToolGroup` |  |
| `rust/gui/src/components/ConversationView.svelte` | 6 | symbol | `ReasoningMsg` |  |
| `rust/gui/src/components/ConversationView.svelte` | 7 | symbol | `Message` |  |
| `rust/gui/src/components/ConversationView.svelte` | 39 | symbol | `copiedIndex` |  |
| `rust/gui/src/components/ConversationView.svelte` | 41 | symbol | `feedback` |  |
| `rust/gui/src/components/ConversationView.svelte` | 42 | symbol | `localPreviews` |  |
| `rust/gui/src/components/ConversationView.svelte` | 43 | symbol | `lastAssistantIndex` |  |
| `rust/gui/src/components/ConversationView.svelte` | 49 | symbol | `lastUserIndex` |  |
| `rust/gui/src/components/ConversationView.svelte` | 55 | symbol | `copyMessage` |  |
| `rust/gui/src/components/ConversationView.svelte` | 63 | symbol | `toggleFeedback` |  |
| `rust/gui/src/components/ConversationView.svelte` | 65 | symbol | `next` |  |
| `rust/gui/src/components/ConversationView.svelte` | 69 | symbol | `openArtifact` |  |
| `rust/gui/src/components/ConversationView.svelte` | 73 | symbol | `localArtifacts` |  |
| `rust/gui/src/components/ConversationView.svelte` | 75 | symbol | `matches` |  |
| `rust/gui/src/components/ConversationView.svelte` | 78 | symbol | `openLocalArtifact` |  |
| `rust/gui/src/components/ConversationView.svelte` | 82 | symbol | `noteTone` |  |
| `rust/gui/src/components/ConversationView.svelte` | 91 | symbol | `paths` |  |
| `rust/gui/src/components/ConversationView.svelte` | 100 | symbol | `openRenderedLink` |  |
| `rust/gui/src/components/ConversationView.svelte` | 102 | symbol | `anchor` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 1 | module | `CustomProvidersSettings` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 6 | symbol | `Provider` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 8 | symbol | `providers` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 9 | symbol | `editingId` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 10 | symbol | `name` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 11 | symbol | `busy` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 12 | symbol | `load` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 14 | symbol | `routes` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 18 | symbol | `reset` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 19 | symbol | `edit` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 20 | symbol | `save` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 25 | symbol | `discover` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 30 | symbol | `activate` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 40 | symbol | `probeChat` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 41 | symbol | `model` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 45 | symbol | `result` |  |
| `rust/gui/src/components/CustomProvidersSettings.svelte` | 52 | symbol | `remove` |  |
| `rust/gui/src/components/DshSlotOverlay.svelte` | 1 | module | `DshSlotOverlay` |  |
| `rust/gui/src/components/ForgeControls.svelte` | 1 | module | `ForgeControls` |  |
| `rust/gui/src/components/ForgeControls.svelte` | 5 | symbol | `active` |  |
| `rust/gui/src/components/InteractionDialogs.svelte` | 1 | module | `InteractionDialogs` |  |
| `rust/gui/src/components/InteractionDialogs.svelte` | 2 | symbol | `Approval` |  |
| `rust/gui/src/components/InteractionDialogs.svelte` | 3 | symbol | `UserQuestion` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 1 | module | `ModelCatalogSettings` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 4 | symbol | `CatalogModel` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 17 | symbol | `CatalogProvider` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 18 | symbol | `ProviderRoute` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 55 | symbol | `normalizeBaseUrl` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 57 | symbol | `isSelectedModel` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 60 | symbol | `selectedFirst` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 66 | symbol | `presetRoutes` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 68 | symbol | `credentialInputs` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 69 | symbol | `credentialBusy` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 70 | symbol | `credentialMessage` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 71 | symbol | `routeFor` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 72 | symbol | `loadPresetRoutes` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 73 | symbol | `routes` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 77 | symbol | `saveCredential` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 78 | symbol | `token` |  |
| `rust/gui/src/components/ModelCatalogSettings.svelte` | 80 | symbol | `first` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 1 | module | `PluginSettings` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 2 | symbol | `UiSlot` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 3 | symbol | `ExternalPlugin` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 4 | symbol | `CodexPlugin` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 16 | symbol | `PluginMarketplace` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 20 | symbol | `DshCategory` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 21 | symbol | `DshMarketItem` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 22 | symbol | `DshMarketPreview` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 23 | symbol | `ProviderRouteDiagnostics` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 24 | symbol | `ProviderActivationDiagnostics` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 25 | symbol | `HarnessDiagnostics` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 63 | symbol | `visibleDshItems` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 64 | symbol | `capabilityDiagnostics` |  |
| `rust/gui/src/components/PluginSettings.svelte` | 65 | symbol | `activationLabels` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 1 | module | `SessionSidebar` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 4 | symbol | `SessionRow` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 77 | symbol | `menuId` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 79 | symbol | `renameTarget` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 80 | symbol | `renameDraft` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 81 | symbol | `renameError` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 82 | symbol | `renameSaving` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 83 | symbol | `projectOpen` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 84 | symbol | `archivedProjectOpen` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 85 | symbol | `groupByWorkspace` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 87 | symbol | `groups` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 89 | symbol | `path` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 90 | symbol | `key` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 91 | symbol | `group` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 97 | symbol | `projectGroups` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 99 | symbol | `archivedProjectGroups` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 100 | symbol | `toggleProject` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 104 | symbol | `toggleArchivedProject` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 108 | symbol | `collapseProjects` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 112 | symbol | `beginRename` |  |
| `rust/gui/src/components/SessionSidebar.svelte` | 117 | symbol | `submitRename` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 1 | module | `SettingsModal` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 6 | symbol | `Settings` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 41 | symbol | `CatalogModel` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 54 | symbol | `CatalogProvider` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 55 | symbol | `ExternalPlugin` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 56 | symbol | `UiSlot` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 57 | symbol | `CodexPlugin` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 69 | symbol | `PluginMarketplace` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 73 | symbol | `HarnessDiagnostics` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 74 | symbol | `DshCategory` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 75 | symbol | `DshMarketItem` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 76 | symbol | `DshMarketPreview` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 184 | symbol | `SettingsSection` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 186 | symbol | `activeSection` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 187 | symbol | `settingsShell` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 188 | symbol | `dialogTitle` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 189 | symbol | `restoreFocus` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 190 | symbol | `sections` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 198 | symbol | `closeSettings` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 202 | symbol | `handleDialogKeydown` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 210 | symbol | `focusable` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 218 | symbol | `first` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 219 | symbol | `last` |  |
| `rust/gui/src/components/SettingsModal.svelte` | 235 | symbol | `target` |  |
| `rust/gui/src/components/TopBar.svelte` | 1 | module | `TopBar` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 1 | module | `WorkspacePanels` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 5 | symbol | `Checkpoint` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 6 | symbol | `Branch` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 7 | symbol | `Commit` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 8 | symbol | `DirEntry` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 9 | symbol | `FilePreview` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 10 | symbol | `FileChange` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 11 | symbol | `SessionRow` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 12 | symbol | `MemoryNote` |  |
| `rust/gui/src/components/WorkspacePanels.svelte` | 13 | symbol | `MemoryMergeStatus` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 1 | module | `app-runtime-controller.svelte` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 11 | symbol | `AppRuntimeController` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 42 | symbol | `status` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 57 | symbol | `normalize` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 58 | symbol | `normalizedLeft` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 59 | symbol | `normalizedRight` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 60 | symbol | `windowsPath` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 72 | symbol | `generation` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 76 | symbol | `status` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 90 | symbol | `unlistenProtocol` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 92 | symbol | `envelope` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 101 | symbol | `unlistenUi` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 123 | symbol | `listeners` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 137 | symbol | `routeChanged` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 146 | symbol | `workspaceDidChange` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 166 | symbol | `pickerSessionId` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 173 | symbol | `directory` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 179 | symbol | `previousId` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 180 | symbol | `previousTitle` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 181 | symbol | `previousMessages` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 182 | symbol | `previousWorkspace` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 184 | symbol | `threadId` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 199 | symbol | `created` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 208 | symbol | `workspace` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 209 | symbol | `workspaceDidChange` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 226 | symbol | `currentWorkspace` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 227 | symbol | `reconcileError` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 275 | symbol | `approval` |  |
| `rust/gui/src/lib/app-runtime-controller.svelte.ts` | 285 | symbol | `question` |  |
| `rust/gui/src/lib/app-server-client.ts` | 1 | module | `app-server-client` |  |
| `rust/gui/src/lib/app-server-client.ts` | 4 | symbol | `AppServerProtocolMethod` |  |
| `rust/gui/src/lib/app-server-client.ts` | 6 | symbol | `ProtocolThreadItem` |  |
| `rust/gui/src/lib/app-server-client.ts` | 16 | symbol | `ProtocolThread` |  |
| `rust/gui/src/lib/app-server-client.ts` | 29 | symbol | `ProtocolGoalSnapshot` |  |
| `rust/gui/src/lib/app-server-client.ts` | 41 | symbol | `ProtocolGoalView` |  |
| `rust/gui/src/lib/app-server-client.ts` | 46 | symbol | `ProtocolEventEnvelope` |  |
| `rust/gui/src/lib/app-server-client.ts` | 54 | symbol | `SessionRow` |  |
| `rust/gui/src/lib/app-server-client.ts` | 68 | symbol | `AppServerOutcome` |  |
| `rust/gui/src/lib/app-server-client.ts` | 79 | symbol | `AppServerRequest` |  |
| `rust/gui/src/lib/app-server-client.ts` | 83 | symbol | `appServerRequest` |  |
| `rust/gui/src/lib/app-server-client.ts` | 85 | symbol | `outcome` |  |
| `rust/gui/src/lib/app-server-client.ts` | 93 | symbol | `ProtocolSequenceGate` |  |
| `rust/gui/src/lib/app-server-client.ts` | 98 | symbol | `previous` |  |
| `rust/gui/src/lib/app-server-client.ts` | 104 | symbol | `normalizeWorkspacePath` |  |
| `rust/gui/src/lib/app-server-client.ts` | 106 | symbol | `normalized` |  |
| `rust/gui/src/lib/app-server-client.ts` | 113 | symbol | `historicalFallbackTitle` |  |
| `rust/gui/src/lib/app-server-client.ts` | 115 | symbol | `normalized` |  |
| `rust/gui/src/lib/app-server-client.ts` | 118 | symbol | `withoutPrefix` |  |
| `rust/gui/src/lib/app-server-client.ts` | 119 | symbol | `chars` |  |
| `rust/gui/src/lib/app-server-client.ts` | 122 | symbol | `threadToSessionRow` |  |
| `rust/gui/src/lib/app-server-client.ts` | 124 | symbol | `userMessages` |  |
| `rust/gui/src/lib/app-server-client.ts` | 125 | symbol | `assistantMessages` |  |
| `rust/gui/src/lib/app-server-client.ts` | 126 | symbol | `toolCalls` |  |
| `rust/gui/src/lib/app-server-client.ts` | 127 | symbol | `snippet` |  |
| `rust/gui/src/lib/app-server-client.ts` | 128 | symbol | `firstUserMessage` |  |
| `rust/gui/src/lib/buglecat-assets.ts` | 1 | symbol | `BugleCatAssetSize` |  |
| `rust/gui/src/lib/buglecat-assets.ts` | 1 | module | `buglecat-assets` |  |
| `rust/gui/src/lib/buglecat-assets.ts` | 2 | symbol | `buglecatAsset` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 1 | module | `checkpoint-controller.svelte` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 2 | symbol | `Checkpoint` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 6 | symbol | `RestoreReport` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 9 | symbol | `CheckpointController` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 35 | symbol | `generation` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 36 | symbol | `checkpoints` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 41 | symbol | `operation` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 49 | symbol | `operation` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 50 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 52 | symbol | `checkpoint` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 65 | symbol | `operation` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 66 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 68 | symbol | `report` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 84 | symbol | `generation` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 85 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 87 | symbol | `files` |  |
| `rust/gui/src/lib/checkpoint-controller.svelte.ts` | 95 | symbol | `operation` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 1 | module | `composer-controller.svelte` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 6 | symbol | `IMAGE_EXTENSIONS` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 8 | symbol | `isImageAttachment` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 9 | symbol | `ComposerController` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 27 | symbol | `picked` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 29 | symbol | `paths` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 39 | symbol | `items` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 44 | symbol | `file` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 47 | symbol | `bytes` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 48 | symbol | `extension` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 49 | symbol | `path` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 57 | symbol | `targetSessionId` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 82 | symbol | `next` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 89 | symbol | `text` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 95 | symbol | `images` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 96 | symbol | `files` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 97 | symbol | `mentions` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 98 | symbol | `fullText` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 99 | symbol | `shown` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 102 | symbol | `selectedImages` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 103 | symbol | `selectedExecutionMode` |  |
| `rust/gui/src/lib/composer-controller.svelte.ts` | 126 | symbol | `slash` |  |
| `rust/gui/src/lib/conversation-model.ts` | 1 | module | `conversation-model` |  |
| `rust/gui/src/lib/conversation-model.ts` | 2 | symbol | `ToolEntry` |  |
| `rust/gui/src/lib/conversation-model.ts` | 4 | symbol | `ToolGroup` |  |
| `rust/gui/src/lib/conversation-model.ts` | 5 | symbol | `ReasoningMessage` |  |
| `rust/gui/src/lib/conversation-model.ts` | 6 | symbol | `ConversationMessage` |  |
| `rust/gui/src/lib/conversation-model.ts` | 15 | symbol | `REASONING_DISPLAY_MAX_CHARS` |  |
| `rust/gui/src/lib/conversation-model.ts` | 17 | symbol | `REASONING_OMITTED` |  |
| `rust/gui/src/lib/conversation-model.ts` | 18 | symbol | `settleCompletedToolGroups` |  |
| `rust/gui/src/lib/conversation-model.ts` | 27 | symbol | `appendReasoning` |  |
| `rust/gui/src/lib/conversation-model.ts` | 29 | symbol | `combined` |  |
| `rust/gui/src/lib/conversation-model.ts` | 31 | symbol | `tailLength` |  |
| `rust/gui/src/lib/conversation-model.ts` | 34 | symbol | `hideCompletedToolActivity` |  |
| `rust/gui/src/lib/conversation-model.ts` | 38 | symbol | `keepConversationConclusions` |  |
| `rust/gui/src/lib/conversation-model.ts` | 40 | symbol | `compacted` |  |
| `rust/gui/src/lib/conversation-model.ts` | 41 | symbol | `pendingAnswer` |  |
| `rust/gui/src/lib/conversation-model.ts` | 60 | symbol | `toolGroupFailureCount` |  |
| `rust/gui/src/lib/dsh-slot-controller.svelte.ts` | 1 | module | `dsh-slot-controller.svelte` |  |
| `rust/gui/src/lib/dsh-slot-controller.svelte.ts` | 4 | symbol | `DshSlotController` |  |
| `rust/gui/src/lib/dsh-slot-controller.svelte.ts` | 16 | symbol | `overlay` |  |
| `rust/gui/src/lib/dsh-slot-controller.svelte.ts` | 35 | symbol | `overlay` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 1 | module | `file-browser-controller.svelte` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 2 | symbol | `DirEntry` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 4 | symbol | `FilePreview` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 5 | symbol | `FileBrowserController` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 27 | symbol | `generation` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 28 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 30 | symbol | `entries` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 43 | symbol | `parent` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 49 | symbol | `generation` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 50 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 52 | symbol | `content` |  |
| `rust/gui/src/lib/file-browser-controller.svelte.ts` | 60 | symbol | `input` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 1 | module | `forge-controller.svelte` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 2 | symbol | `ForgeRuntimeStatus` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 4 | symbol | `ForgeJobSummary` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 15 | symbol | `ForgeJobStatus` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 28 | symbol | `ForgeController` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 76 | symbol | `workspace` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 81 | symbol | `lifecycle` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 82 | symbol | `operation` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 83 | symbol | `poll` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 108 | symbol | `confirmed` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 112 | symbol | `workspace` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 117 | symbol | `lifecycle` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 118 | symbol | `operation` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 119 | symbol | `poll` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 121 | symbol | `job` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 142 | symbol | `lifecycle` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 143 | symbol | `poll` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 144 | symbol | `workspace` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 145 | symbol | `generation` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 148 | symbol | `job` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 167 | symbol | `status` |  |
| `rust/gui/src/lib/forge-controller.svelte.ts` | 187 | symbol | `operation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 1 | module | `git-workspace-controller.svelte` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 2 | symbol | `BranchInfo` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 4 | symbol | `Commit` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 5 | symbol | `FileChange` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 6 | symbol | `DiffPreview` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 7 | symbol | `normalizeDiffPreview` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 14 | symbol | `GitWorkspaceController` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 54 | symbol | `generation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 55 | symbol | `branches` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 60 | symbol | `operation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 69 | symbol | `operation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 70 | symbol | `name` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 71 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 85 | symbol | `operation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 86 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 103 | symbol | `generation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 104 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 106 | symbol | `commits` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 116 | symbol | `generation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 118 | symbol | `workspaceGeneration` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 119 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 123 | symbol | `files` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 134 | symbol | `previewGeneration` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 146 | symbol | `generation` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 147 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 149 | symbol | `response` |  |
| `rust/gui/src/lib/git-workspace-controller.svelte.ts` | 163 | symbol | `operation` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 1 | module | `goal-controller.svelte` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 3 | symbol | `GoalController` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 18 | symbol | `phase` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 44 | symbol | `generation` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 47 | symbol | `view` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 62 | symbol | `confirmed` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 70 | symbol | `current` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 71 | symbol | `threadId` |  |
| `rust/gui/src/lib/goal-controller.svelte.ts` | 76 | symbol | `next` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 1 | module | `memory-controller.svelte` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 3 | symbol | `MemoryNote` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 5 | symbol | `MemoryMergeStatus` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 6 | symbol | `MemoryController` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 33 | symbol | `generation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 34 | symbol | `workspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 36 | symbol | `notes` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 44 | symbol | `operation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 52 | symbol | `operation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 54 | symbol | `workspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 56 | symbol | `removed` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 69 | symbol | `operation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 71 | symbol | `workspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 73 | symbol | `status` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 86 | symbol | `operations` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 87 | symbol | `workspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 88 | symbol | `generation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 94 | symbol | `status` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 108 | symbol | `status` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 130 | symbol | `operation` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 132 | symbol | `workspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 134 | symbol | `tags` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 135 | symbol | `saved` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 151 | symbol | `expectedWorkspace` |  |
| `rust/gui/src/lib/memory-controller.svelte.ts` | 164 | symbol | `operation` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 1 | module | `model-controls-controller.svelte` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 3 | symbol | `ReasoningEffortOption` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 5 | symbol | `ProviderRouteOption` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 6 | symbol | `CatalogRoute` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 7 | symbol | `PresetSelection` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 8 | symbol | `option` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 9 | symbol | `AUTO` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 10 | symbol | `reasoningEffortsForModel` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 12 | symbol | `id` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 28 | symbol | `REASONING_EFFORTS` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 30 | symbol | `PERMISSION_MODES` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 36 | symbol | `ModelControlsController` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 63 | symbol | `protocol` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 74 | symbol | `routes` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 77 | symbol | `presetEnabled` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 82 | symbol | `models` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 88 | symbol | `normalizedCurrent` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 89 | symbol | `sameProvider` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 90 | symbol | `visible` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 103 | symbol | `previous` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 108 | symbol | `updated` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 122 | symbol | `previous` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 125 | symbol | `providerId` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 126 | symbol | `selected` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 130 | symbol | `updated` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 149 | symbol | `previous` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 161 | symbol | `threadId` |  |
| `rust/gui/src/lib/model-controls-controller.svelte.ts` | 166 | symbol | `previous` |  |
| `rust/gui/src/lib/panel-controller.svelte.ts` | 1 | module | `panel-controller.svelte` |  |
| `rust/gui/src/lib/panel-controller.svelte.ts` | 6 | symbol | `PanelController` |  |
| `rust/gui/src/lib/panel-controller.svelte.ts` | 35 | symbol | `generation` |  |
| `rust/gui/src/lib/panel-controller.svelte.ts` | 36 | symbol | `panel` |  |
| `rust/gui/src/lib/panel-controller.svelte.ts` | 51 | symbol | `generation` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 1 | module | `plugin-controller.svelte` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 3 | symbol | `ProviderRouteDiagnostics` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 5 | symbol | `ProviderActivationDiagnostics` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 6 | symbol | `HarnessDiagnostics` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 7 | symbol | `ExternalPlugin` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 8 | symbol | `CodexPlugin` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 13 | symbol | `DshUiSlotContribution` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 14 | symbol | `MarketplaceSource` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 18 | symbol | `PluginMarketplace` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 19 | symbol | `DshCategory` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 20 | symbol | `DshMarketItem` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 25 | symbol | `DshMarketPreview` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 29 | symbol | `PluginController` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 49 | symbol | `results` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 60 | symbol | `failed` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 103 | symbol | `result` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 135 | symbol | `selected` |  |
| `rust/gui/src/lib/plugin-controller.svelte.ts` | 144 | symbol | `selected` |  |
| `rust/gui/src/lib/protocol-version.ts` | 1 | module | `protocol-version` |  |
| `rust/gui/src/lib/protocol-version.ts` | 3 | symbol | `APP_SERVER_PROTOCOL_VERSION` |  |
| `rust/gui/src/lib/protocol-version.ts` | 5 | symbol | `APP_SERVER_PROTOCOL_METHODS` |  |
| `rust/gui/src/lib/protocol-version.ts` | 78 | symbol | `AppServerProtocolMethod` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 1 | module | `settings-controller.svelte` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 4 | symbol | `Settings` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 20 | symbol | `ConfigLocation` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 21 | symbol | `CatalogModel` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 26 | symbol | `CatalogProvider` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 27 | symbol | `ModelCatalogResponse` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 28 | symbol | `SettingsController` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 65 | symbol | `settings` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 80 | symbol | `unavailable` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 99 | symbol | `catalog` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 101 | symbol | `models` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 113 | symbol | `settings` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 115 | symbol | `catalog` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 116 | symbol | `models` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 125 | symbol | `selected` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 131 | symbol | `refreshed` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 141 | symbol | `current` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 156 | symbol | `settings` |  |
| `rust/gui/src/lib/settings-controller.svelte.ts` | 157 | symbol | `updates` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 1 | symbol | `SIDEBAR_DEFAULT_WIDTH` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 1 | module | `sidebar-controller.svelte` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 2 | symbol | `SIDEBAR_MIN_WIDTH` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 3 | symbol | `SIDEBAR_MAX_WIDTH` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 4 | symbol | `SidebarController` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 14 | symbol | `savedWidth` |  |
| `rust/gui/src/lib/sidebar-controller.svelte.ts` | 54 | symbol | `viewportMax` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 1 | module | `slash-controller.svelte` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 3 | symbol | `SlashCommand` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 5 | symbol | `CustomCommand` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 6 | symbol | `SlashActions` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 12 | symbol | `SlashController` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 26 | symbol | `input` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 32 | symbol | `filter` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 33 | symbol | `head` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 60 | symbol | `threadId` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 62 | symbol | `currentTitle` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 63 | symbol | `title` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 74 | symbol | `rows` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 80 | symbol | `scope` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 85 | symbol | `target` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 94 | symbol | `action` |  |
| `rust/gui/src/lib/slash-controller.svelte.ts` | 95 | symbol | `setInput` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 1 | module | `thread-controller.svelte` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 6 | symbol | `Approval` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 8 | symbol | `UserQuestion` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 9 | symbol | `UiEvent` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 27 | symbol | `QueuedTurn` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 29 | symbol | `ThreadCallbacks` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 33 | symbol | `ThreadController` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 75 | symbol | `next` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 81 | symbol | `next` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 186 | symbol | `unbound` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 205 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 217 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 226 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 241 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 245 | symbol | `last` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 246 | symbol | `entry` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 254 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 267 | symbol | `group` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 268 | symbol | `tool` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 271 | symbol | `candidate` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 280 | symbol | `kind` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 283 | symbol | `payload` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 292 | symbol | `approval` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 298 | symbol | `question` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 327 | symbol | `restored` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 333 | symbol | `kind` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 334 | symbol | `url` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 346 | symbol | `cached` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 380 | symbol | `message` |  |
| `rust/gui/src/lib/thread-controller.svelte.ts` | 391 | symbol | `start` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 1 | module | `thread-lifecycle-controller.svelte` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 6 | symbol | `WorkspaceRecovery` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 10 | symbol | `ThreadLifecycleController` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 54 | symbol | `current` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 75 | symbol | `selection` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 80 | symbol | `threadId` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 98 | symbol | `current` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 113 | symbol | `flight` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 130 | symbol | `generation` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 132 | symbol | `metadata` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 133 | symbol | `results` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 136 | symbol | `threads` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 144 | symbol | `current` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 158 | symbol | `session` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 165 | symbol | `title` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 170 | symbol | `session` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 180 | symbol | `previousId` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 181 | symbol | `previousTitle` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 182 | symbol | `previousMessages` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 191 | symbol | `id` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 193 | symbol | `created` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 209 | symbol | `navigation` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 210 | symbol | `previousId` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 211 | symbol | `previousTitle` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 212 | symbol | `previousWorkspace` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 220 | symbol | `activationAccepted` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 230 | symbol | `visible` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 257 | symbol | `recovery` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 285 | symbol | `navigation` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 286 | symbol | `previousId` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 287 | symbol | `previousTitle` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 288 | symbol | `previousWorkspace` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 291 | symbol | `forkTitle` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 292 | symbol | `newThreadId` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 294 | symbol | `activationAccepted` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 300 | symbol | `forked` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 318 | symbol | `recovery` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 345 | symbol | `time` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 347 | symbol | `difference` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 351 | symbol | `date` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 398 | symbol | `workspace` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 399 | symbol | `current` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 404 | symbol | `suffix` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 457 | symbol | `next` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 469 | symbol | `base` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 470 | symbol | `titles` |  |
| `rust/gui/src/lib/thread-lifecycle-controller.svelte.ts` | 471 | symbol | `index` |  |
| `rust/gui/src/lib/ui-format.ts` | 1 | symbol | `ToolOutcome` |  |
| `rust/gui/src/lib/ui-format.ts` | 1 | module | `ui-format` |  |
| `rust/gui/src/lib/ui-format.ts` | 2 | symbol | `baseName` |  |
| `rust/gui/src/lib/ui-format.ts` | 4 | symbol | `formatTokens` |  |
| `rust/gui/src/lib/ui-format.ts` | 5 | symbol | `formatCost` |  |
| `rust/gui/src/lib/ui-format.ts` | 6 | symbol | `currencySymbol` |  |
| `rust/gui/src/lib/ui-format.ts` | 7 | symbol | `currencyName` |  |
| `rust/gui/src/lib/ui-format.ts` | 8 | symbol | `priceSourceName` |  |
| `rust/gui/src/lib/ui-format.ts` | 9 | symbol | `toolOutcome` |  |
| `rust/gui/src/lib/ui-format.ts` | 11 | symbol | `exit` |  |
| `rust/gui/src/lib/ui-format.ts` | 12 | symbol | `trimmed` |  |
| `rust/gui/src/lib/ui-format.ts` | 13 | symbol | `body` |  |
| `rust/gui/src/lib/ui-format.ts` | 22 | symbol | `toolStatusLabel` |  |
| `rust/gui/src/lib/ui-format.ts` | 24 | symbol | `outcome` |  |
| `rust/gui/src/lib/ui-format.ts` | 29 | symbol | `diffLineClass` |  |
| `rust/gui/src/lib/ui-format.ts` | 37 | symbol | `escapeHtml` |  |
| `rust/gui/src/lib/ui-format.ts` | 39 | symbol | `inlineMarkdown` |  |
| `rust/gui/src/lib/ui-format.ts` | 48 | symbol | `renderMarkdown` |  |
| `rust/gui/src/lib/ui-format.ts` | 50 | symbol | `lines` |  |
| `rust/gui/src/lib/ui-format.ts` | 51 | symbol | `output` |  |
| `rust/gui/src/lib/ui-format.ts` | 52 | symbol | `index` |  |
| `rust/gui/src/lib/ui-format.ts` | 53 | symbol | `unordered` |  |
| `rust/gui/src/lib/ui-format.ts` | 54 | symbol | `ordered` |  |
| `rust/gui/src/lib/ui-format.ts` | 55 | symbol | `closeLists` |  |
| `rust/gui/src/lib/ui-format.ts` | 59 | symbol | `rowCells` |  |
| `rust/gui/src/lib/ui-format.ts` | 61 | symbol | `line` |  |
| `rust/gui/src/lib/ui-format.ts` | 64 | symbol | `buffer` |  |
| `rust/gui/src/lib/ui-format.ts` | 73 | symbol | `headers` |  |
| `rust/gui/src/lib/ui-format.ts` | 75 | symbol | `rows` |  |
| `rust/gui/src/lib/ui-format.ts` | 77 | symbol | `table` |  |
| `rust/gui/src/lib/ui-format.ts` | 82 | symbol | `heading` |  |
| `rust/gui/src/lib/ui-format.ts` | 86 | symbol | `unorderedItem` |  |
| `rust/gui/src/lib/ui-format.ts` | 88 | symbol | `orderedItem` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 1 | symbol | `TokenUsage` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 1 | module | `usage-controller.svelte` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 2 | symbol | `UsageController` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 16 | symbol | `symbol` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 17 | symbol | `formattedCost` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 18 | symbol | `costText` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 41 | symbol | `protocolUsage` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 49 | symbol | `stored` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 57 | symbol | `prompt` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 58 | symbol | `completion` |  |
| `rust/gui/src/lib/usage-controller.svelte.ts` | 66 | symbol | `stored` |  |
| `rust/gui/src/main.ts` | 1 | module | `main` |  |
| `rust/gui/src/main.ts` | 4 | symbol | `app` |  |
