<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  const IMAGE_EXTS = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
  const isImage = (p: string) => IMAGE_EXTS.includes((p.split(".").pop() || "").toLowerCase());
  const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

  // Mirrors the Rust `UiEvent` enum (serde tag = "kind", snake_case).
  type UiEvent =
    | { kind: "ready"; model: string; sandbox: string; workspace: string }
    | { kind: "assistant"; text: string }
    | { kind: "tool_start"; name: string; args: string }
    | { kind: "tool_result"; name: string; result: string }
    | { kind: "approval"; id: number; command: string; reason: string; cwd: string; details: string }
    | { kind: "done"; final_text: string; stop_reason: string }
    | { kind: "error"; message: string };

  type Approval = { id: number; command: string; reason: string; cwd: string; details: string };
  let approval = $state<Approval | null>(null);

  type Settings = {
    model: string;
    base_url: string;
    sandbox_mode: string;
    approval_policy: string;
    reasoning_effort: string;
    max_iterations: number;
    max_tool_calls: number;
    context_edit_enabled: boolean;
    context_edit_max_chars: number;
    context_edit_keep_recent_messages: number;
    context_edit_max_tool_result_chars: number;
    api_key_masked: string;
    has_api_key: boolean;
    available_models: string[];
    sandbox_modes: string[];
    approval_policies: string[];
  };
  type ConfigLocation = {
    config_path: string;
    config_dir: string;
  };
  let settings = $state<Settings | null>(null);
  let configLocation = $state<ConfigLocation | null>(null);
  let apiKeyInput = $state("");
  let saving = $state(false);

  type Checkpoint = {
    id: string;
    label: string;
    created_at: string;
    files: number;
    skipped: number;
    total_bytes: number;
  };
  type RestoreReport = {
    checkpoint_id: string;
    safety_checkpoint_id?: string | null;
    restored_files: number;
    deleted_files: number;
  };
  let checkpointOpen = $state(false);
  let checkpoints = $state<Checkpoint[]>([]);
  let checkpointLabel = $state("");
  let checkpointBusy = $state(false);

  type Msg =
    | { role: "user" | "assistant" | "note"; text: string }
    | { role: "tool"; name: string; args?: string; result?: string };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let attached = $state<string[]>([]); // absolute file paths attached to the next turn
  let busy = $state(false);
  let header = $state("connecting…");
  let workspace = $state("");
  let scroller: HTMLDivElement;

  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(async () => {
    // Header falls back to a direct status call until the agent thread is Ready.
    try {
      const s = await invoke<{ model: string; sandbox: string }>("get_status");
      header = `${s.model} · ${s.sandbox}`;
    } catch (e) {
      header = "config error";
    }

    await listen<UiEvent>("ncx://event", (ev) => {
      const p = ev.payload;
      switch (p.kind) {
        case "ready":
          header = `${p.model} · ${p.sandbox}`;
          workspace = p.workspace;
          break;
        case "assistant":
          messages.push({ role: "assistant", text: p.text });
          break;
        case "tool_start":
          messages.push({ role: "tool", name: p.name, args: p.args });
          break;
        case "approval":
          approval = { id: p.id, command: p.command, reason: p.reason, cwd: p.cwd, details: p.details };
          break;
        case "tool_result": {
          // Attach the result to the most recent unfinished tool entry.
          const last = [...messages].reverse().find(
            (m) => m.role === "tool" && m.name === p.name && m.result === undefined,
          ) as Extract<Msg, { role: "tool" }> | undefined;
          if (last) last.result = p.result;
          else messages.push({ role: "tool", name: p.name, result: p.result });
          break;
        }
        case "done":
          // The completed reply already arrived as an `assistant` event; only a
          // non-normal stop adds a note.
          if (p.stop_reason !== "completed") {
            messages.push({ role: "note", text: `[${p.stop_reason}] ${p.final_text}` });
          }
          busy = false;
          break;
        case "error":
          messages.push({ role: "note", text: `Error: ${p.message}` });
          busy = false;
          break;
      }
      scrollDown();
    });
  });

  async function attachFiles() {
    try {
      const picked = await open({ multiple: true });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      for (const p of paths) if (!attached.includes(p)) attached.push(p);
    } catch (e) {
      messages.push({ role: "note", text: `Attach failed: ${e}` });
    }
  }

  function removeAttachment(p: string) {
    attached = attached.filter((x) => x !== p);
  }

  async function chooseWorkspace() {
    try {
      const dir = await open({ directory: true, multiple: false });
      if (!dir || Array.isArray(dir)) return;
      const set = await invoke<string>("set_workspace", { path: dir });
      workspace = set;
      messages.push({ role: "note", text: `Workspace switched to ${set}. Agent reloaded.` });
    } catch (e) {
      messages.push({ role: "note", text: `Set workspace failed: ${e}` });
    }
  }

  async function send() {
    const text = input.trim();
    if ((!text && attached.length === 0) || busy) return;
    // Images route through the vision pipeline; other files become @mentions.
    const images = attached.filter(isImage);
    const files = attached.filter((p) => !isImage(p));
    const mentions = files.map((p) => `@${p}`).join(" ");
    const fullText = [text, mentions].filter(Boolean).join("\n");
    const shown = attached.length ? `${text}${text ? "\n" : ""}📎 ${attached.map(baseName).join(", ")}` : text;
    messages.push({ role: "user", text: shown });
    input = "";
    const sentImages = images;
    attached = [];
    busy = true;
    scrollDown();
    try {
      await invoke("send_prompt", { text: fullText, images: sentImages });
    } catch (e) {
      messages.push({ role: "note", text: `Failed to send: ${e}` });
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    // Enter sends; Shift+Enter inserts a newline.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function decide(approved: boolean) {
    if (!approval) return;
    const id = approval.id;
    approval = null;
    try {
      await invoke("approve", { id, approved });
    } catch (e) {
      messages.push({ role: "note", text: `Approval failed: ${e}` });
    }
  }

  async function openSettings() {
    try {
      const [loadedSettings, loadedLocation] = await Promise.all([
        invoke<Settings>("get_settings"),
        invoke<ConfigLocation>("get_config_location"),
      ]);
      settings = loadedSettings;
      configLocation = loadedLocation;
      apiKeyInput = "";
    } catch (e) {
      messages.push({ role: "note", text: `Settings load failed: ${e}` });
    }
  }

  async function openConfigFile() {
    try {
      await invoke("open_config_file");
      configLocation = await invoke<ConfigLocation>("get_config_location");
    } catch (e) {
      messages.push({ role: "note", text: `Open config failed: ${e}` });
    }
  }

  async function openConfigDir() {
    try {
      await invoke("open_config_dir");
      configLocation = await invoke<ConfigLocation>("get_config_location");
    } catch (e) {
      messages.push({ role: "note", text: `Open config folder failed: ${e}` });
    }
  }

  async function saveSettings() {
    if (!settings) return;
    saving = true;
    const updates: Record<string, string> = {
      model: settings.model,
      base_url: settings.base_url,
      sandbox_mode: settings.sandbox_mode,
      approval_policy: settings.approval_policy,
      reasoning_effort: settings.reasoning_effort,
      max_iterations: String(settings.max_iterations),
      max_tool_calls: String(settings.max_tool_calls),
      context_edit_enabled: String(settings.context_edit_enabled),
      context_edit_max_chars: String(settings.context_edit_max_chars),
      context_edit_keep_recent_messages: String(settings.context_edit_keep_recent_messages),
      context_edit_max_tool_result_chars: String(settings.context_edit_max_tool_result_chars),
    };
    if (apiKeyInput.trim()) updates.api_key = apiKeyInput.trim();
    try {
      await invoke("save_settings", { updates });
      settings = null;
      apiKeyInput = "";
    } catch (e) {
      messages.push({ role: "note", text: `Save failed: ${e}` });
    }
    saving = false;
  }

  async function loadCheckpoints() {
    checkpoints = await invoke<Checkpoint[]>("get_checkpoints");
  }

  async function openCheckpoints() {
    checkpointOpen = true;
    checkpointBusy = true;
    try {
      await loadCheckpoints();
    } catch (e) {
      messages.push({ role: "note", text: `Checkpoint load failed: ${e}` });
    }
    checkpointBusy = false;
  }

  async function saveCheckpoint() {
    checkpointBusy = true;
    try {
      const cp = await invoke<Checkpoint>("create_checkpoint", { label: checkpointLabel });
      checkpointLabel = "";
      await loadCheckpoints();
      messages.push({ role: "note", text: `Checkpoint saved: ${cp.id}` });
    } catch (e) {
      messages.push({ role: "note", text: `Checkpoint failed: ${e}` });
    }
    checkpointBusy = false;
  }

  async function restoreCheckpoint(id: string) {
    if (busy || checkpointBusy) return;
    if (!window.confirm(`Restore checkpoint ${id}?`)) return;
    checkpointBusy = true;
    try {
      const report = await invoke<RestoreReport>("restore_checkpoint", { id });
      await loadCheckpoints();
      messages.push({
        role: "note",
        text: `Restored ${report.checkpoint_id}: ${report.restored_files} file(s), ${report.deleted_files} removed.`,
      });
    } catch (e) {
      messages.push({ role: "note", text: `Restore failed: ${e}` });
    }
    checkpointBusy = false;
  }

  // ── Phase 1: git branches, diff, session history ──────────────────────────
  type BranchInfo = { name: string; current: boolean };
  type SessionRow = {
    session_id: string;
    title: string;
    snippet: string;
    user_messages: number;
    assistant_messages: number;
    tool_calls: number;
    updated_at: string;
    has_snapshot: boolean;
  };
  let branchOpen = $state(false);
  let branches = $state<BranchInfo[]>([]);
  let newBranch = $state("");
  let branchBusy = $state(false);
  let diffOpen = $state(false);
  let diffText = $state("");
  let historyOpen = $state(false);
  let sessions = $state<SessionRow[]>([]);

  async function loadBranches() {
    branches = await invoke<BranchInfo[]>("git_branches");
  }
  async function openBranches() {
    branchOpen = true;
    branchBusy = true;
    try {
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `Branches load failed: ${e}` });
    }
    branchBusy = false;
  }
  async function createBranch() {
    if (!newBranch.trim()) return;
    branchBusy = true;
    try {
      await invoke("git_create_branch", { name: newBranch });
      messages.push({ role: "note", text: `Created and switched to branch ${newBranch}.` });
      newBranch = "";
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `Create branch failed: ${e}` });
    }
    branchBusy = false;
  }
  async function switchBranch(name: string) {
    if (branchBusy) return;
    branchBusy = true;
    try {
      await invoke("git_switch_branch", { name });
      messages.push({ role: "note", text: `Switched to branch ${name}.` });
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `Switch failed: ${e}` });
    }
    branchBusy = false;
  }
  async function openDiff() {
    diffOpen = true;
    diffText = "loading…";
    try {
      diffText = await invoke<string>("git_diff");
    } catch (e) {
      diffText = `diff failed: ${e}`;
    }
  }
  async function openHistory() {
    historyOpen = true;
    try {
      sessions = await invoke<SessionRow[]>("list_sessions");
    } catch (e) {
      messages.push({ role: "note", text: `History load failed: ${e}` });
    }
  }

  // ── Hermes: project-memory self-evolution ─────────────────────────────────
  type MemoryNote = { ts: number; tags: string[]; text: string };
  let hermesOpen = $state(false);
  let notes = $state<MemoryNote[]>([]);
  let hermesBusy = $state(false);
  let newNote = $state("");
  let newNoteTags = $state("");

  async function loadNotes() {
    notes = await invoke<MemoryNote[]>("memory_list");
  }
  async function openHermes() {
    hermesOpen = true;
    hermesBusy = true;
    try {
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `Memory load failed: ${e}` });
    }
    hermesBusy = false;
  }
  async function consolidateMemory() {
    hermesBusy = true;
    try {
      const removed = await invoke<number>("memory_consolidate");
      messages.push({ role: "note", text: `Memory: folded ${removed} near-duplicate note(s).` });
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `Memory consolidate failed: ${e}` });
    }
    hermesBusy = false;
  }
  async function addNote() {
    if (!newNote.trim()) return;
    hermesBusy = true;
    try {
      const tags = newNoteTags.split(",").map((t) => t.trim()).filter(Boolean);
      const saved = await invoke<boolean>("memory_add", { note: newNote, tags });
      messages.push({ role: "note", text: saved ? "Memory: note saved." : "Memory: already known (not duplicated)." });
      newNote = "";
      newNoteTags = "";
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `Memory add failed: ${e}` });
    }
    hermesBusy = false;
  }
  function fmtTs(ts: number): string {
    try {
      return new Date(ts * 1000).toLocaleString();
    } catch {
      return String(ts);
    }
  }
</script>

<main>
  <header>
    <span class="brand">nanocodex</span>
    <span class="meta">{header}</span>
    {#if workspace}
      <button class="toolbtn" title={`Workspace: ${workspace}  (click to switch)`} onclick={chooseWorkspace} aria-label="Workspace" style="max-width:220px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">📁 {baseName(workspace)}</button>
    {:else}
      <button class="toolbtn" title="Choose workspace" onclick={chooseWorkspace} aria-label="Workspace">📁</button>
    {/if}
    {#if busy}<span class="spinner" title="working…">●</span>{/if}
    <button class="toolbtn" title="Branches" onclick={openBranches} aria-label="Branches">⎇</button>
    <button class="toolbtn" title="Diff" onclick={openDiff} aria-label="Diff">±</button>
    <button class="toolbtn" title="History" onclick={openHistory} aria-label="History">⌃</button>
    <button class="toolbtn" title="Project memory" onclick={openHermes} aria-label="Memory">📒</button>
    <button class="gear" title="Settings" onclick={openSettings} aria-label="Settings">⚙</button>
    <button class="toolbtn" title="Checkpoints" onclick={openCheckpoints} aria-label="Checkpoints">CP</button>
  </header>

  <div class="scroll" bind:this={scroller}>
    {#if messages.length === 0}
      <p class="empty">Ask me to inspect or edit the workspace. Try “list the files” or
        “create hello.txt with apply_patch”.</p>
    {/if}
    {#each messages as m}
      {#if m.role === "user"}
        <div class="msg user"><div class="bubble">{m.text}</div></div>
      {:else if m.role === "assistant"}
        <div class="msg assistant"><div class="bubble">{m.text}</div></div>
      {:else if m.role === "note"}
        <div class="msg note">{m.text}</div>
      {:else if m.role === "tool"}
        <div class="tool">
          <span class="tname">⚙ {m.name}</span>
          {#if m.args}<code class="targs">{m.args}</code>{/if}
          {#if m.result !== undefined}
            <pre class="tresult">{m.result}</pre>
          {:else}
            <span class="trunning">running…</span>
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  <footer>
    {#if attached.length}
      <div class="attachments" style="display:flex;flex-wrap:wrap;gap:6px;margin-bottom:6px">
        {#each attached as p}
          <span class="chip" title={p} style="display:inline-flex;align-items:center;gap:4px;font-size:12px;background:rgba(0,0,0,0.06);border-radius:10px;padding:2px 8px">
            {isImage(p) ? "🖼" : "📄"} {baseName(p)}
            <button class="chipx" onclick={() => removeAttachment(p)} aria-label="Remove" style="border:none;background:none;cursor:pointer;font-size:14px;line-height:1">×</button>
          </span>
        {/each}
      </div>
    {/if}
    <div class="composer-row" style="display:flex;gap:8px;align-items:flex-end">
      <button class="toolbtn" title="Attach file/image" onclick={attachFiles} aria-label="Attach">📎</button>
      <textarea
        bind:value={input}
        onkeydown={onKey}
        placeholder="Message nanocodex…  (Enter to send, Shift+Enter for newline)"
        rows="2"
        style="flex:1"
      ></textarea>
      <button onclick={send} disabled={busy || (input.trim() === "" && attached.length === 0)}>Send</button>
    </div>
  </footer>

  {#if approval}
    <div class="overlay">
      <div class="modal">
        <h3>Approval needed</h3>
        <p class="areason">{approval.reason}</p>
        <div class="afield"><span>action</span><code>{approval.command}</code></div>
        <div class="afield"><span>cwd</span><code>{approval.cwd}</code></div>
        {#if approval.details}
          <pre class="adetails">{approval.details}</pre>
        {/if}
        <div class="abtns">
          <button class="deny" onclick={() => decide(false)}>Deny</button>
          <button class="ok" onclick={() => decide(true)}>Approve</button>
        </div>
      </div>
    </div>
  {/if}

  {#if checkpointOpen}
    <div class="overlay">
      <div class="modal">
        <h3>Checkpoints</h3>
        <div class="checkpoint-create">
          <input bind:value={checkpointLabel} placeholder="Label" />
          <button onclick={saveCheckpoint} disabled={checkpointBusy}>Save</button>
          <button class="plain" onclick={loadCheckpoints} disabled={checkpointBusy}>Refresh</button>
        </div>
        <div class="checkpoint-list">
          {#if checkpoints.length === 0}
            <p class="emptyline">No checkpoints.</p>
          {/if}
          {#each checkpoints as cp}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <strong>{cp.label || "(unlabeled)"}</strong>
                <code>{cp.id}</code>
              </div>
              <div class="checkpoint-meta">
                <span>{cp.created_at}</span>
                <span>{cp.files} files</span>
                <span>{cp.skipped} skipped</span>
              </div>
              <button class="restore" onclick={() => restoreCheckpoint(cp.id)} disabled={busy || checkpointBusy}>
                Restore
              </button>
            </div>
          {/each}
        </div>
        <div class="abtns">
          <button class="deny" onclick={() => (checkpointOpen = false)}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if branchOpen}
    <div class="overlay">
      <div class="modal">
        <h3>Git branches</h3>
        <div class="checkpoint-create">
          <input bind:value={newBranch} placeholder="new-branch-name" />
          <button onclick={createBranch} disabled={branchBusy}>Create &amp; switch</button>
          <button class="plain" onclick={loadBranches} disabled={branchBusy}>Refresh</button>
        </div>
        <div class="checkpoint-list">
          {#if branches.length === 0}
            <p class="emptyline">No branches.</p>
          {/if}
          {#each branches as b}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <strong>{b.current ? "● " : ""}{b.name}</strong>
              </div>
              <button class="restore" onclick={() => switchBranch(b.name)} disabled={branchBusy || b.current}>
                {b.current ? "current" : "Switch"}
              </button>
            </div>
          {/each}
        </div>
        <div class="abtns">
          <button class="deny" onclick={() => (branchOpen = false)}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if diffOpen}
    <div class="overlay">
      <div class="modal">
        <h3>Working-tree diff</h3>
        <pre style="max-height:60vh;overflow:auto;white-space:pre-wrap;font-family:ui-monospace,Consolas,monospace;font-size:12px;text-align:left;background:rgba(0,0,0,0.04);padding:8px;border-radius:6px">{diffText}</pre>
        <div class="abtns">
          <button class="plain" onclick={openDiff}>Refresh</button>
          <button class="deny" onclick={() => (diffOpen = false)}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if historyOpen}
    <div class="overlay">
      <div class="modal">
        <h3>Session history</h3>
        <div class="checkpoint-list">
          {#if sessions.length === 0}
            <p class="emptyline">No saved sessions.</p>
          {/if}
          {#each sessions as s}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <strong>{s.title || "(untitled)"}</strong>
                <code>{s.snippet}</code>
              </div>
              <div class="checkpoint-meta">
                <span>{s.updated_at}</span>
                <span>{s.user_messages}u / {s.assistant_messages}a / {s.tool_calls}t</span>
                {#if s.has_snapshot}<span>snapshot</span>{/if}
              </div>
            </div>
          {/each}
        </div>
        <div class="abtns">
          <button class="plain" onclick={openHistory}>Refresh</button>
          <button class="deny" onclick={() => (historyOpen = false)}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if hermesOpen}
    <div class="overlay">
      <div class="modal">
        <h3>Project memory</h3>
        <p class="emptyline">Verified learnings recalled into future sessions as leads. (Harness self-evolution / "Hermes" is a separate feature — see forge.)</p>
        <div class="checkpoint-create">
          <input bind:value={newNote} placeholder="Record a verified learning…" />
          <input bind:value={newNoteTags} placeholder="tags (comma)" style="max-width:140px" />
          <button onclick={addNote} disabled={hermesBusy}>Add</button>
        </div>
        <div class="checkpoint-create">
          <button onclick={consolidateMemory} disabled={hermesBusy}>Tidy: fold duplicates</button>
          <button class="plain" onclick={loadNotes} disabled={hermesBusy}>Refresh</button>
          <span class="emptyline">{notes.length} note(s)</span>
        </div>
        <div class="checkpoint-list">
          {#if notes.length === 0}
            <p class="emptyline">No learnings yet.</p>
          {/if}
          {#each notes as n}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <strong>{n.text}</strong>
                {#if n.tags.length}<code>{n.tags.join(", ")}</code>{/if}
              </div>
              <div class="checkpoint-meta">
                <span>{fmtTs(n.ts)}</span>
              </div>
            </div>
          {/each}
        </div>
        <div class="abtns">
          <button class="deny" onclick={() => (hermesOpen = false)}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if settings}
    <div class="overlay">
      <div class="modal">
        <h3>Settings</h3>
        {#if configLocation}
          <div class="config-entry">
            <span>Config</span>
            <code title={configLocation.config_path}>{configLocation.config_path}</code>
            <button class="plain" onclick={openConfigFile}>Open file</button>
            <button class="plain" onclick={openConfigDir}>Open folder</button>
          </div>
        {/if}
        <label>
          <span>Model</span>
          <select bind:value={settings.model}>
            {#each settings.available_models as m}<option value={m}>{m}</option>{/each}
          </select>
        </label>
        <label>
          <span>Sandbox</span>
          <select bind:value={settings.sandbox_mode}>
            {#each settings.sandbox_modes as s}<option value={s}>{s}</option>{/each}
          </select>
        </label>
        <label>
          <span>Approval</span>
          <select bind:value={settings.approval_policy}>
            {#each settings.approval_policies as a}<option value={a}>{a}</option>{/each}
          </select>
        </label>
        <label>
          <span>Reasoning</span>
          <input bind:value={settings.reasoning_effort} placeholder="auto | low | medium | high | max | off" />
        </label>
        <label>
          <span>Model calls</span>
          <input type="number" min="1" bind:value={settings.max_iterations} />
        </label>
        <label>
          <span>Tool calls</span>
          <input type="number" min="0" bind:value={settings.max_tool_calls} />
        </label>
        <label class="check">
          <span>Context edit</span>
          <input type="checkbox" bind:checked={settings.context_edit_enabled} />
        </label>
        <label>
          <span>Context chars</span>
          <input type="number" min="1" bind:value={settings.context_edit_max_chars} />
        </label>
        <label>
          <span>Recent messages</span>
          <input type="number" min="1" bind:value={settings.context_edit_keep_recent_messages} />
        </label>
        <label>
          <span>Tool result chars</span>
          <input type="number" min="1" bind:value={settings.context_edit_max_tool_result_chars} />
        </label>
        <label>
          <span>Base URL</span>
          <input bind:value={settings.base_url} />
        </label>
        <label>
          <span>API key</span>
          <input
            type="password"
            bind:value={apiKeyInput}
            placeholder={settings.has_api_key ? `keep current (${settings.api_key_masked})` : "set an API key"}
          />
        </label>
        <div class="abtns">
          <button class="deny" onclick={() => (settings = null)}>Cancel</button>
          <button class="ok" onclick={saveSettings} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  {/if}
</main>
