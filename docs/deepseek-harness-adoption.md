# DeepSeek Harness Adoption Notes

## Upstream reference

- Repository: <https://github.com/deepseek-ai/deepseek-harness>
- Reviewed commit: `47f943859bef60e4160492346772ded9b24f765a`
- License: MIT
- Local research clone: `rust/target/research/deepseek-harness` (build output,
  not vendored source)

The reviewed upstream is a developer-preview TypeScript runtime. Its internal
APIs may change, so nanocodex adopts behavioral ideas rather than depending on
its packages.

## Adopted behavior

`ncx-core::ToolRegistry` now supports named, in-process tool middleware:

1. `before_execute` runs in registration order.
2. A layer may block dispatch. Later layers and the tool body are skipped.
3. Every entered layer receives `after_execute` in reverse order, including
   when an entered layer blocked the call.
4. `after_execute` may replace the model-facing string result.
5. Registration names are unique and can be removed explicitly.

The runtime also exposes native Rust composition points without importing the
upstream package runtime:

- `Provider` can be replaced while retaining the current tools and session.
- `AgentRuntimeProfile` is the shared CLI/GUI configuration assembly contract.
- named `TurnContextProvider` instances add query-scoped notes in stable order
  and can be removed explicitly;
- `ToolScheduler` can replace the execution strategy after the runtime has
  separated read-only batches from serial calls;
- `Tool` and `ToolMiddleware` remain dynamically registered boundaries.

The first tool-library compatibility slice is also adopted:

- every registered built-in or MCP tool receives stable capability metadata
  for catalog search and recovery planning;
- string results are normalized into invalid-input, permission, missing-path,
  wrong-target, timeout, cancellation, transient, unknown-tool, and execution
  failures without changing the public `Tool::execute -> String` contract;
- read-only transient failures receive one retry through the complete
  middleware/hook/policy pipeline;
- argument-compatible read-only fallbacks currently cover malformed regular
  expressions (`grep` -> `grep_literal`) and directory paths sent to
  `read_file` (`read_file` -> `list_directory`);
- `str_replace_editor` ports Harness's view/create/unique-replace/line-insert
  workflow while delegating every mutation to the existing `apply_patch`
  approval and sandbox boundary.

The later architecture phase replaces the old session compatibility path with
nanocodex-owned, versioned components rather than importing DeepSeek's storage
format:

- `ncx-protocol` owns Thread, Turn, Item, request/response, and Event envelopes;
- `ncx-thread-store` owns durable transcripts, model context, usage, and
  cross-process per-Thread execution leases;
- `ncx-app-server` owns protocol routing, while GUI/CLI hosts implement a small
  scheduler/resource adapter;
- `ncx-provider`, `ncx-sandbox`, and `ncx-context` remain the single owners of
  Provider, Policy, and ContextFragment contracts.

The GUI is now an app-server protocol client for conversation and OpenAI Codex
plugin lifecycle requests. Tauri supplies desktop-only effects (agent queue,
file picker, process launch), but no longer branches on protocol methods.

## Ownership retained by nanocodex

- Existing pre/post hooks remain inside the middleware pipeline.
- `ncx-sandbox` remains the policy owner.
- Existing approval checks and concrete executors remain the enforcement
  boundary.
- Built-in tools and MCP tools still use the existing `Tool` and
  `ToolRegistry` contracts.
- Tool schedulers receive only runtime-approved batches and must dispatch via
  `ToolRegistry::execute`; write barriers remain owned by `AgentLoop`.
- Mutating tools are never retried or substituted automatically, preventing a
  timeout or ambiguous failure from duplicating side effects.
- Public configuration, CLI behavior, serialized formats, and GUI behavior are
  unchanged.

Middleware is an extension point, not an authorization bypass. Security checks
must remain at the actual filesystem, process, network, or MCP execution
boundary.

The prompt change is intentionally narrow: `ncx-core::PromptAssembler`
composes the CLI's base prompt, project instructions, skills index, and plan
mode as named sections with stable ordering and explicit removal. Existing
memory recall remains query-scoped; additional context providers append
ephemeral notes without persisting them into session history.

## OpenAI Codex resource compatibility

Enabled `.codex-plugin/plugin.json` packages can contribute Skills, MCP
servers, hosted Apps metadata, and Hooks. Official path-based `mcpServers`,
single/multiple Hook resource paths, inline resources, and Interface asset
paths are parsed and confined to the plugin root. Local, URL/Git-subdir, NPM,
and legacy nanocodex Marketplace source shapes are accepted. Installation,
upgrade recovery, enable/disable, uninstall, and Marketplace installation are
routed through the versioned app-server boundary.

## Not adopted

- No Node.js, pnpm, Cordis, or DeepSeek Harness runtime dependency.
- No vendored upstream runtime or copied session format.
- No ACP transport or second approval/sandbox implementation.
- No in-process native DLL/SO plugin ABI. Untrusted executable extensions stay
  in isolated child processes; Codex plugins are resource packages.

## Verification

Offline unit tests cover middleware ordering and blocking, context registration
and removal, provider replacement, custom scheduler dispatch, bounded default
concurrency, cancellation, model-ordered tool results, failure classification,
read-only retry/fallback, Thread ownership/recovery, protocol serialization,
plugin resource confinement, and Marketplace source compatibility. Real Tauri
E2E covers cross-Thread concurrency, same-Thread rejection, visible history
projection, refresh/reopen, and plugin/Marketplace listing through app-server.
