<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

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
  let busy = $state(false);
  let header = $state("connecting…");
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

  async function send() {
    const text = input.trim();
    if (!text || busy) return;
    messages.push({ role: "user", text });
    input = "";
    busy = true;
    scrollDown();
    try {
      await invoke("send_prompt", { text });
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
</script>

<main>
  <header>
    <span class="brand">nanocodex</span>
    <span class="meta">{header}</span>
    {#if busy}<span class="spinner" title="working…">●</span>{/if}
    <button class="toolbtn" title="Branches" onclick={openBranches} aria-label="Branches">⎇</button>
    <button class="toolbtn" title="Diff" onclick={openDiff} aria-label="Diff">±</button>
    <button class="toolbtn" title="History" onclick={openHistory} aria-label="History">⌃</button>
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
    <textarea
      bind:value={input}
      onkeydown={onKey}
      placeholder="Message nanocodex…  (Enter to send, Shift+Enter for newline)"
      rows="2"
    ></textarea>
    <button onclick={send} disabled={busy || input.trim() === ""}>Send</button>
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
