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

## Same-session durable goals

The reviewed Harness keeps a long-running goal as durable, revisioned session
state while keeping permission to schedule another paid model round strictly
process-local. Nanocodex adopts that split instead of treating a goal as a
prompt-only plan or as another transcript item.

### Durable contract

- A thread owns at most one `GoalSnapshot`: stable ID, monotonically increasing
  revision, non-empty objective, phase (`active`, `paused`, `blocked`, or
  `complete`), optional structured block reason, positive round limit, durable
  `roundsStarted`, and timestamps.
- Goal state lives in the Thread Store's own persistence domain. It is not part
  of model context or the visible Turn transcript. Fork copies the durable
  snapshot atomically with the source thread.
- Every edit or transition supplies the exact goal ID and revision. The store
  compares and writes under the same process mutex, cross-process file lock,
  reload, and atomic-save transaction. A stale reference performs zero writes.
- A new goal may replace only a completed goal. Other replacement and all
  lifecycle-transition rules are enforced by the App Server domain owner; the
  store provides atomic compare-and-set rather than a second state machine.
- Old `threads-v2.json` files deserialize with an empty goal map. No transcript
  or model-context migration is required.

### Process-local activation and authority

- `armed` / `disarmed` is never persisted. Opening, resuming, forking, replacing
  an Agent, or reloading the driver always starts disarmed; only an explicit
  top-level human action may arm automatic continuation.
- Model tools may read goal state. Create/edit/clear/resume and changes to round
  limits require host-attested direct-human authority. A model may complete or
  block only from the exact admitted goal round; `blocked` additionally needs
  the same blocker in at least three consecutive goal rounds.
- Tool calls authenticate the exact live root Agent and open Turn. Subagents,
  stale turns, synthetic messages, and omitted message-source metadata cannot
  inherit human authority.

### Continuation safety

- Automatic continuation is opt-in and visibly reports its maximum rounds and
  cost implications. The desktop default is disarmed; it does not silently copy
  the upstream 256-round default into a paid local route.
- Before reserving every automatic round, the driver flushes durable session and
  goal state. It rechecks goal ID/revision/phase/activation, round limit, current
  Agent lifecycle, cancellation, and competing ordinary user input after every
  await boundary.
- Ordinary user input wins over queued automatic work. Cancellation, max-token
  termination, checkpoint failure, stale revision, rejected prompt, queue
  failure, runtime disposal, and Agent replacement fail closed by pausing,
  blocking, or disarming instead of retrying without authority.
- The driver cannot bypass the active Provider Route, approval/policy service,
  sandbox, token/cost accounting, or existing Turn ownership lease.

### Delivery order and evidence

1. Protocol domain types and storage-level atomic CAS, including stale/no-write,
   reopen, legacy-file, and fork-copy tests.
2. App Server lifecycle state machine and process-local activation tests.
3. Authenticated model tools and three-round blocked threshold tests.
4. Serialized continuation driver with checkpoint, race, cancellation, budget,
   and disposal tests.
5. GUI status plus explicit arm/pause/resume controls, followed by a no-paid-call
   WebView E2E. Real paid continuation is not enabled merely to test the UI.

### Desktop worker integration status

- `goalResume` now crosses the App Server runtime adapter only after the exact
  durable transition succeeds. The desktop scheduler accepts the thread ID and
  starts a session-scoped Goal worker; scheduler rejection immediately revokes
  process-local activation while leaving the durable active phase recoverable.
- The worker uses `GoalRoundDriver::reserve_next` before every model call,
  reloads the current Provider Route and Harness Profile, executes through
  `AgentLoop::run_goal_round` with exact Goal authority, persists model context
  before completing the Turn, and repeats only while the Goal remains armed.
  Assistant messages and usage remain ordinary protocol Turn data; hidden
  `GoalMessage` prompts never enter the visible transcript.
- A direct human prompt arriving during automatic continuation is installed in
  a deferred slot while the run-state lock is held. The admitted Goal round is
  cancelled and paused, then the human prompt is started in the same thread as
  soon as the Goal worker releases the lease. This closes the previous
  “session busy, retry later” behavior without allowing overlapping writers.
- A Goal resumed by a direct-human model tool is detected after that human Turn
  commits and is scheduled through the same worker. Restart, thread activation,
  fork, provider rebuild failure, checkpoint failure, and worker creation
  failure remain fail-closed.
- The desktop host owns every coordinator, ordinary Turn, and Goal worker join
  handle. Shutdown rejects new work, broadcasts cancellation, sends the worker
  shutdown command, and joins every owned thread; the lifecycle regression
  proves no Goal worker is detached on application exit.
- A deterministic WebView E2E now runs against a temporary localhost OpenAI
  fixture and isolated home directory. It proves two admitted automatic rounds,
  two durably completed Turns, exact `get_goal`/`update_goal` completion, four
  model requests on the isolated route, hidden Goal prompts staying out of the
  visible transcript, and both assistant conclusions rendering after a normal
  sidebar resume. No external Provider or real credential is used.
- The E2E exposed and fixed a resume-order defect: the legacy `loaded` snapshot
  could arrive after `threadReadVisible` and erase later durable Goal turns.
  Resume now suppresses that one compatibility event and makes the protocol
  Thread the final UI authority.
- Current evidence: App Server 28/28, Config 39/39, GUI Rust 107/107, targeted
  Core Profile/Goal execution, Vite production build (147 modules), and the
  no-network WebView E2E all pass.

### Provider route truth and model provenance

- Each assistant item keeps two deliberately separate values: the model ID the
  client requested from the active Provider Route and the optional `model`
  field returned by the API response. Model self-identification is never used
  as routing evidence.
- Live events, durable protocol Turns, visible-history projection, model
  context, orchestrator telemetry, and sidebar resume now preserve the same
  requested/response pair. The previous resume mapper restored only assistant
  text and silently dropped both model fields; this is fixed.
- UI wording says `response model field`, not `confirmed upstream model`.
  Relays may echo or rewrite an alias, so even a matching response field does
  not prove which internal upstream implementation served the request.
- The localhost WebView fixture deliberately requests `mock-goal-model` while
  returning `mock-confirmed-model`. After a full sidebar resume the page shows
  both values, while all four HTTP request bodies still contain only the
  requested route model. GUI Rust and Vite production gates pass.
- The desktop `ready` snapshot now carries the active Provider ID and protocol
  alongside the model. Composer renders Provider + model directly and exposes
  the protocol in the model menu header. `SetModel` continues to refresh this
  one snapshot after an atomic Route commit, so the UI cannot mix a new model
  label with an old Provider. During a running Turn the control remains
  available and explicitly says the current Turn keeps its old Route while the
  next Turn uses the new Route.
- The same no-network E2E creates and activates a localhost custom OpenAI Route,
  observes `goal-e2e-relay`, `openai`, and `mock-goal-model` in Composer, then
  completes the two-round Goal through that Route. GUI Rust is now 109/109.
- Composer now loads every configured, keyed Provider Route through the existing
  App Server directory, groups its models by Provider, and pins the active
  Provider first. The compact button uses the configured human-readable name;
  stable IDs remain available in diagnostics and activation state. Selecting
  another group calls the same catalog-validation and
  atomic activation boundary used by Settings; it never swaps only the model
  string. A failed candidate restores the complete previous UI Route and the
  backend leaves its active Route byte-for-byte unchanged.
- The compatibility-only `legacy` marker is no longer user-facing. Preset Routes
  resolve their visible Provider ID from the maintained catalog endpoint; an
  unmatched manual endpoint is explicitly labeled `manual`, while custom Routes
  retain their stable configured ID.
- The WebView test configures three localhost Routes with duplicate model IDs:
  primary, backup, and an invalid candidate. It switches primary -> backup from
  Composer without changing the transcript, verifies diagnostics moved to the
  backup Route despite the identical model ID, then verifies the invalid model
  is rejected and backup remains active. GUI Rust is now 110/110.
- Preset Providers join the same menu only when BugleCat owns a usable,
  independent credential for them. DeepSeek requires the stored DeepSeek key
  and Yunmo requires the stored Yunmo token; unavailable catalog cards are not
  presented as executable Routes. Preset selection reuses `modelPresetApply`,
  including endpoint, model-list, pricing, currency, and activation checks.
- Activating a custom relay now atomically resets input/output price estimates
  to zero. A custom endpoint has no trustworthy catalog price, so retaining the
  previous Provider's rates would fabricate cost telemetry. The UI refreshes
  its estimator from the committed settings immediately after activation.
- The isolated WebView environment supplies a placeholder DeepSeek credential
  and proves the DeepSeek group appears, but never clicks it or contacts the
  official endpoint. It also proves the localhost custom Route reports unknown
  zero pricing after the switch. Config 39/39 remains green.

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

## Durable per-Thread Harness profiles

BugleCat now persists a `harnessProfile` on each Thread instead of using a
process-wide environment switch. Empty Threads may select `full`, `coding`,
`readonly`, `minimal`, or `headless`; the App Server locks the field as soon as
the first Turn exists. Resume reconstructs the runtime from persisted metadata,
Fork inherits it, and orchestrator workers receive the same explicit profile.
Unknown profiles are validated before Thread creation or metadata mutation.

The desktop selector rebuilds an empty active Agent immediately after a change,
so the first prompt cannot run with a stale composition. Protocol, app-server,
core, GUI, and real WebView tests cover legacy defaulting, invalid-create
rejection, empty-Thread mutation, post-Turn locking, persistence, and Fork
inheritance without a paid model call.

## Atomic preset provider routes

Curated providers now own credentials, protocol, endpoint, model catalog and
selection through stable `preset:<provider-id>` routes. Legacy DeepSeek and
Yunmo credential fields are lazily migrated on first preset activation; a
saved preset-route credential then takes precedence. Route storage and the
flat compatibility snapshot commit as one transaction and restore the prior
directory if the snapshot write fails. Custom relays retain unknown pricing,
while preset activation writes the audited catalog price and currency.

The settings catalog exposes an independent masked Token entry per provider.
Real WebView coverage fills that control, checks masked feedback, commits a
DeepSeek preset, rejects a Yunmo preset without a credential, and proves both
the active route and current transcript remain unchanged after the failure.
The same isolated localhost fixture proves the next model turn reads the newly
committed route and preserves requested/response model metadata.
