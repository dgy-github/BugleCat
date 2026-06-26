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
  let settings = $state<Settings | null>(null);
  let apiKeyInput = $state("");
  let saving = $state(false);

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
      settings = await invoke<Settings>("get_settings");
      apiKeyInput = "";
    } catch (e) {
      messages.push({ role: "note", text: `Settings load failed: ${e}` });
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
</script>

<main>
  <header>
    <span class="brand">nanocodex</span>
    <span class="meta">{header}</span>
    {#if busy}<span class="spinner" title="working…">●</span>{/if}
    <button class="gear" title="Settings" onclick={openSettings} aria-label="Settings">⚙</button>
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

  {#if settings}
    <div class="overlay">
      <div class="modal">
        <h3>Settings</h3>
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
