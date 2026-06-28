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
    | { kind: "ready"; model: string; sandbox: string; workspace: string; session_id: string; models: string[]; permission_mode: string }
    | { kind: "assistant_delta"; text: string }
    | { kind: "assistant"; text: string }
    | { kind: "tool_start"; name: string; args: string }
    | { kind: "tool_result"; name: string; result: string }
    | { kind: "approval"; id: number; command: string; reason: string; cwd: string; details: string }
    | { kind: "done"; final_text: string; stop_reason: string; usage: Record<string, number> }
    | { kind: "loaded"; messages: { role: string; text: string }[] }
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
    price_in: number;
    price_out: number;
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
    | { role: "tool"; name: string; args?: string; result?: string; collapsed?: boolean };

  let messages = $state<Msg[]>([]);
  let input = $state("");
  let attached = $state<string[]>([]); // absolute file paths attached to the next turn
  let queued = $state<{ text: string; images: string[]; shown: string }[]>([]); // pending turns
  let busy = $state(false);
  // File explorer (workspace tree)
  type DirEntry = { name: string; path: string; is_dir: boolean };
  let filesOpen = $state(false);
  let filesPath = $state("");
  let filesEntries = $state<DirEntry[]>([]);
  let header = $state("连接中…");
  let workspace = $state("");
  let sessionTitle = $state("新会话");
  let sidebarOpen = $state(true);
  let sandboxMode = $state("");
  let tokIn = $state(0);
  let tokOut = $state(0);
  // Per-1M-token prices (from config); 0 = unknown → cost is hidden.
  let priceIn = $state(0);
  let priceOut = $state(0);
  const cost = $derived((tokIn / 1e6) * priceIn + (tokOut / 1e6) * priceOut);
  let streamingIdx = $state<number | null>(null); // index of the bubble being streamed
  const fmtTok = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`);
  const fmtCost = (n: number) => (n >= 1 ? n.toFixed(2) : n.toFixed(4));

  // ── Collapsible tool output ───────────────────────────────────────────────
  // Large results auto-collapse so a single dump can't bury the conversation.
  const COLLAPSE_LINES = 12;
  const COLLAPSE_CHARS = 800;
  const isLong = (s: string) =>
    !!s && (s.length > COLLAPSE_CHARS || s.split("\n").length > COLLAPSE_LINES);
  const lineCount = (s: string = "") => (s ? s.split("\n").length : 0);
  // Multi-line → "N 行"; single long line → "N 字" so a char-triggered collapse isn't mislabeled.
  const collapsedHint = (s: string = "") => {
    const lines = lineCount(s);
    return lines > 1 ? `${lines} 行 · 点击展开` : `${s.length} 字 · 点击展开`;
  };
  // Per-line class for unified-diff coloring.
  const diffLineClass = (ln: string) => {
    if (ln.startsWith("+++") || ln.startsWith("---") || ln.startsWith("diff ") || ln.startsWith("index ")) return "dl-meta";
    if (ln.startsWith("@@")) return "dl-hunk";
    if (ln.startsWith("+")) return "dl-add";
    if (ln.startsWith("-")) return "dl-del";
    return "";
  };
  // ── Minimal, safe Markdown renderer (escape-first; only emits known tags) ──
  const esc = (s: string) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  // Inline spans on already-escaped text: code, bold, italic, http(s) links.
  function inlineMd(s: string): string {
    s = s.replace(/`([^`]+)`/g, (_m, c) => `<code>${c}</code>`);
    s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    s = s.replace(/__([^_]+)__/g, "<strong>$1</strong>");
    s = s.replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>");
    s = s.replace(
      /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
      '<a href="$2" target="_blank" rel="noreferrer">$1</a>',
    );
    return s;
  }
  function renderMarkdown(src: string): string {
    const lines = (src || "").replace(/\r\n/g, "\n").split("\n");
    const out: string[] = [];
    let i = 0;
    let ul = false, ol = false;
    const closeLists = () => {
      if (ul) { out.push("</ul>"); ul = false; }
      if (ol) { out.push("</ol>"); ol = false; }
    };
    const rowCells = (r: string) =>
      r.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
    while (i < lines.length) {
      const line = lines[i];
      const fence = line.match(/^```(\w*)\s*$/);
      if (fence) {
        closeLists();
        const buf: string[] = [];
        i++;
        while (i < lines.length && !/^```\s*$/.test(lines[i])) { buf.push(lines[i]); i++; }
        i++;
        out.push(`<pre class="md-code"><code>${esc(buf.join("\n"))}</code></pre>`);
        continue;
      }
      if (/^\s*\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[i + 1])) {
        closeLists();
        const headers = rowCells(line);
        i += 2;
        const rows: string[][] = [];
        while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) { rows.push(rowCells(lines[i])); i++; }
        let t = '<table class="md-table"><thead><tr>';
        t += headers.map((h) => `<th>${inlineMd(esc(h))}</th>`).join("");
        t += "</tr></thead><tbody>";
        for (const r of rows) t += "<tr>" + r.map((c) => `<td>${inlineMd(esc(c))}</td>`).join("") + "</tr>";
        out.push(t + "</tbody></table>");
        continue;
      }
      const h = line.match(/^(#{1,6})\s+(.*)$/);
      if (h) { closeLists(); const l = h[1].length; out.push(`<h${l} class="md-h">${inlineMd(esc(h[2]))}</h${l}>`); i++; continue; }
      if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) { closeLists(); out.push("<hr/>"); i++; continue; }
      if (/^\s*>\s?/.test(line)) { closeLists(); out.push(`<blockquote>${inlineMd(esc(line.replace(/^\s*>\s?/, "")))}</blockquote>`); i++; continue; }
      const um = line.match(/^\s*[-*+]\s+(.*)$/);
      if (um) { if (ol) { out.push("</ol>"); ol = false; } if (!ul) { out.push("<ul>"); ul = true; } out.push(`<li>${inlineMd(esc(um[1]))}</li>`); i++; continue; }
      const om = line.match(/^\s*\d+\.\s+(.*)$/);
      if (om) { if (ul) { out.push("</ul>"); ul = false; } if (!ol) { out.push("<ol>"); ol = true; } out.push(`<li>${inlineMd(esc(om[1]))}</li>`); i++; continue; }
      if (line.trim() === "") { closeLists(); i++; continue; }
      closeLists();
      out.push(`<p>${inlineMd(esc(line))}</p>`);
      i++;
    }
    closeLists();
    return out.join("\n");
  }

  // Branch / checkpoint expand-to-detail.
  type Commit = { hash: string; subject: string; when: string };
  let branchCommits = $state<Record<string, Commit[]>>({});
  async function toggleBranchDetail(name: string) {
    if (name in branchCommits) {
      const { [name]: _drop, ...rest } = branchCommits;
      branchCommits = rest;
      return;
    }
    try {
      branchCommits = { ...branchCommits, [name]: await invoke<Commit[]>("git_log", { name, limit: 10 }) };
    } catch (e) {
      branchCommits = { ...branchCommits, [name]: [{ hash: "", subject: `加载失败：${e}`, when: "" }] };
    }
  }
  let checkpointFiles = $state<Record<string, string[]>>({});
  async function toggleCheckpointDetail(id: string) {
    if (id in checkpointFiles) {
      const { [id]: _drop, ...rest } = checkpointFiles;
      checkpointFiles = rest;
      return;
    }
    try {
      checkpointFiles = { ...checkpointFiles, [id]: await invoke<string[]>("checkpoint_files", { id }) };
    } catch (e) {
      checkpointFiles = { ...checkpointFiles, [id]: [`加载失败：${e}`] };
    }
  }
  function toggleTool(m: Msg) {
    if (m.role === "tool" && m.result !== undefined) m.collapsed = !m.collapsed;
  }
  let rightPanel = $state(""); // "" | files | branches | diff | memory | checkpoints
  const PANEL_TITLES: Record<string, string> = {
    files: "文件", branches: "Git 分支", diff: "工作区改动", memory: "项目记忆", checkpoints: "检查点",
  };
  let currentSessionId = $state("");
  // Topbar model quick-switch
  let currentModel = $state("");
  let models = $state<string[]>([]);
  let modelMenuOpen = $state(false);
  async function selectModel(m: string) {
    modelMenuOpen = false;
    if (!m || m === currentModel) return;
    const prev = currentModel;
    currentModel = m; // optimistic; `ready` will confirm
    try {
      await invoke("set_model", { model: m });
    } catch (e) {
      currentModel = prev;
      messages.push({ role: "note", text: `切换模型失败：${e}` });
    }
  }
  // Claude-Code-style permission modes (single composer selector).
  let permissionMode = $state("accept-edits");
  let modeMenuOpen = $state(false);
  const PERMISSION_MODES = [
    { id: "plan", label: "规划模式", desc: "只读，不改文件" },
    { id: "default", label: "默认", desc: "改文件前询问" },
    { id: "accept-edits", label: "自动接受编辑", desc: "编辑直接应用，命令询问" },
    { id: "bypass", label: "全权放行", desc: "危险：所有操作不询问" },
  ];
  const modeLabel = (id: string) => PERMISSION_MODES.find((o) => o.id === id)?.label ?? id;
  const modeIcon = (id: string) => (id === "plan" ? "📋" : id === "bypass" ? "⚠️" : "🛡");
  async function selectMode(id: string) {
    modeMenuOpen = false;
    if (id === permissionMode) return;
    const prev = permissionMode;
    permissionMode = id; // optimistic; `ready` confirms
    try {
      await invoke("set_permission_mode", { mode: id });
    } catch (e) {
      permissionMode = prev;
      messages.push({ role: "note", text: `切换权限模式失败：${e}` });
    }
  }
  let scroller: HTMLDivElement;

  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(async () => {
    // Header falls back to a direct status call until the agent thread is Ready.
    try {
      const s = await invoke<{ model: string; sandbox: string; approval: string; permission_mode: string; price_in: number; price_out: number }>("get_status");
      header = `${s.model} · ${s.sandbox}`;
      sandboxMode = s.sandbox;
      currentModel = s.model;
      if (s.permission_mode) permissionMode = s.permission_mode;
      priceIn = s.price_in || 0;
      priceOut = s.price_out || 0;
    } catch (e) {
      header = "配置错误";
    }
    refreshSessions();

    await listen<UiEvent>("ncx://event", (ev) => {
      const p = ev.payload;
      switch (p.kind) {
        case "ready":
          header = `${p.model} · ${p.sandbox}`;
          workspace = p.workspace;
          sandboxMode = p.sandbox;
          currentModel = p.model;
          if (p.models?.length) models = p.models;
          if (p.permission_mode) permissionMode = p.permission_mode;
          // Learn the active session's real id so 最近会话 can mark/return to it.
          if (p.session_id) currentSessionId = p.session_id;
          refreshSessions();
          break;
        case "assistant_delta":
          if (streamingIdx === null) {
            if (p.text === "") break; // ignore an empty leading delta (no bubble yet)
            messages.push({ role: "assistant", text: p.text });
            streamingIdx = messages.length - 1;
          } else {
            const m = messages[streamingIdx];
            if (m && m.role === "assistant") m.text += p.text;
          }
          break;
        case "assistant":
          if (streamingIdx !== null) {
            const m = messages[streamingIdx];
            if (m && m.role === "assistant") {
              // Tool-only turn (no narration) → drop the empty bubble instead of
              // leaving a blank box; a run of these is what made the "striped" rows.
              if (p.text.trim() === "") messages.splice(streamingIdx, 1);
              else m.text = p.text;
            }
            streamingIdx = null;
          } else if (p.text.trim() !== "") {
            messages.push({ role: "assistant", text: p.text });
          }
          break;
        case "tool_start":
          // A streamed bubble that turned out empty (tool-only turn) leaves no box.
          if (streamingIdx !== null) {
            const m = messages[streamingIdx];
            if (m && m.role === "assistant" && m.text.trim() === "") messages.splice(streamingIdx, 1);
          }
          streamingIdx = null;
          messages.push({ role: "tool", name: p.name, args: p.args });
          break;
        case "approval":
          approval = { id: p.id, command: p.command, reason: p.reason, cwd: p.cwd, details: p.details };
          break;
        case "tool_result": {
          // Attach the result to the most recent unfinished tool entry.
          const last = messages.find(
            (m) => m.role === "tool" && m.name === p.name && m.result === undefined,
          ) as Extract<Msg, { role: "tool" }> | undefined;
          const collapsed = isLong(p.result);
          if (last) { last.result = p.result; last.collapsed = collapsed; }
          else messages.push({ role: "tool", name: p.name, result: p.result, collapsed });
          break;
        }
        case "done":
          // The completed reply already arrived as an `assistant` event; only a
          // non-normal stop adds a note.
          if (p.stop_reason !== "completed") {
            messages.push({ role: "note", text: `[${p.stop_reason}] ${p.final_text}` });
          }
          {
            const u = p.usage || {};
            tokIn += u.prompt_tokens || 0;
            tokOut += u.completion_tokens || 0;
          }
          streamingIdx = null;
          busy = false;
          refreshSessions();
          dequeue();
          break;
        case "loaded":
          messages = p.messages
            .filter((m) => m.text.trim() !== "") // drop empty tool-only assistant turns
            .map((m) =>
              m.role === "user" || m.role === "assistant"
                ? { role: m.role, text: m.text }
                : { role: "note", text: m.text },
            );
          streamingIdx = null;
          busy = false;
          refreshSessions(); // keep the session you just left visible in 最近会话
          break;
        case "error":
          streamingIdx = null;
          messages.push({ role: "note", text: `错误：${p.message}` });
          busy = false;
          break;
      }
      scrollDown();
    });
    // The agent thread's initial `ready` can fire before this listener exists
    // (Tauri events aren't buffered), so the active session id would be missed.
    // Now that we're listening, ask the backend to re-emit it.
    invoke("request_ready").catch(() => {});
  });

  async function attachFiles() {
    try {
      const picked = await open({ multiple: true });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      for (const p of paths) if (!attached.includes(p)) attached.push(p);
    } catch (e) {
      messages.push({ role: "note", text: `添加失败：${e}` });
    }
  }

  function removeAttachment(p: string) {
    attached = attached.filter((x) => x !== p);
  }

  // Paste an image from the clipboard (Ctrl+V) → temp file → attach.
  async function handlePaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const it of items) {
      if (it.kind === "file" && it.type.startsWith("image/")) {
        e.preventDefault();
        const file = it.getAsFile();
        if (!file) continue;
        try {
          const buf = new Uint8Array(await file.arrayBuffer());
          const ext = (it.type.split("/")[1] || "png").replace("jpeg", "jpg");
          const path = await invoke<string>("save_temp_image", { bytes: Array.from(buf), ext });
          if (!attached.includes(path)) attached.push(path);
        } catch (err) {
          messages.push({ role: "note", text: `粘贴图片失败：${err}` });
        }
      }
    }
  }

  // File explorer over the workspace (with inline content preview).
  let filePreview = $state<{ path: string; content: string } | null>(null);
  async function loadDir(rel: string) {
    try {
      filesEntries = await invoke<DirEntry[]>("list_dir", { rel });
      filesPath = rel;
      filePreview = null;
    } catch (e) {
      messages.push({ role: "note", text: `读取目录失败：${e}` });
    }
  }
  async function openFiles() {
    if (rightPanel === "files") { rightPanel = ""; return; }
    rightPanel = "files";
    await loadDir("");
  }
  function filesUp() {
    if (filePreview) { filePreview = null; return; } // back from preview → listing
    if (!filesPath) return;
    const parent = filesPath.includes("/") ? filesPath.slice(0, filesPath.lastIndexOf("/")) : "";
    loadDir(parent);
  }
  async function pickFile(entry: DirEntry) {
    if (entry.is_dir) {
      loadDir(entry.path);
      return;
    }
    try {
      const content = await invoke<string>("read_workspace_file", { rel: entry.path });
      filePreview = { path: entry.path, content };
    } catch (e) {
      filePreview = { path: entry.path, content: `（无法预览：${e}）` };
    }
  }
  function insertMention(path: string) {
    input = input ? `${input} @${path}` : `@${path}`;
  }

  async function chooseWorkspace() {
    try {
      const dir = await open({ directory: true, multiple: false });
      if (!dir || Array.isArray(dir)) return;
      const set = await invoke<string>("set_workspace", { path: dir });
      workspace = set;
      messages.push({ role: "note", text: `已切换工作区到 ${set}，agent 已重载。` });
    } catch (e) {
      messages.push({ role: "note", text: `切换工作区失败：${e}` });
    }
  }

  async function dispatch(text: string, images: string[], shown: string) {
    messages.push({ role: "user", text: shown });
    busy = true;
    scrollDown();
    try {
      await invoke("send_prompt", { text, images });
    } catch (e) {
      messages.push({ role: "note", text: `发送失败：${e}` });
      busy = false;
      dequeue();
    }
  }
  function dequeue() {
    if (!busy && queued.length > 0) {
      const next = queued.shift();
      if (next) dispatch(next.text, next.images, next.shown);
    }
  }
  function send() {
    const text = input.trim();
    if (!text && attached.length === 0) return;
    // Images route through the vision pipeline; other files become @mentions.
    const images = attached.filter(isImage);
    const files = attached.filter((p) => !isImage(p));
    const mentions = files.map((p) => `@${p}`).join(" ");
    const fullText = [text, mentions].filter(Boolean).join("\n");
    const shown = attached.length ? `${text}${text ? "\n" : ""}📎 ${attached.map(baseName).join(", ")}` : text;
    input = "";
    const imgs = images;
    attached = [];
    if (busy) {
      // Queue up to 2 follow-up turns while the agent works.
      if (queued.length >= 2) {
        messages.push({ role: "note", text: "队列已满（2 条），请先等当前任务完成。" });
        return;
      }
      queued.push({ text: fullText, images: imgs, shown });
      return;
    }
    dispatch(fullText, imgs, shown);
  }

  function onKey(e: KeyboardEvent) {
    // Slash-command palette navigation takes precedence while it's open.
    if (showSlash && slashMatches.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); slashIdx = (slashIdx + 1) % slashMatches.length; return; }
      if (e.key === "ArrowUp") { e.preventDefault(); slashIdx = (slashIdx - 1 + slashMatches.length) % slashMatches.length; return; }
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); runSlash(slashMatches[Math.min(slashIdx, slashMatches.length - 1)]); return; }
      if (e.key === "Escape") { e.preventDefault(); input = ""; return; }
    }
    // Enter sends; Shift+Enter inserts a newline.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function decide(decision: "deny" | "once" | "always") {
    if (!approval) return;
    const id = approval.id;
    approval = null;
    try {
      await invoke("approve", { id, decision });
    } catch (e) {
      messages.push({ role: "note", text: `审批失败：${e}` });
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
      messages.push({ role: "note", text: `设置加载失败：${e}` });
    }
  }

  async function openConfigFile() {
    try {
      await invoke("open_config_file");
      configLocation = await invoke<ConfigLocation>("get_config_location");
    } catch (e) {
      messages.push({ role: "note", text: `打开配置失败：${e}` });
    }
  }

  async function openConfigDir() {
    try {
      await invoke("open_config_dir");
      configLocation = await invoke<ConfigLocation>("get_config_location");
    } catch (e) {
      messages.push({ role: "note", text: `打开配置文件夹失败：${e}` });
    }
  }

  async function saveSettings() {
    if (!settings) return;
    saving = true;
    const updates: Record<string, string> = {
      model: settings.model,
      base_url: settings.base_url,
      reasoning_effort: settings.reasoning_effort,
      max_iterations: String(settings.max_iterations),
      max_tool_calls: String(settings.max_tool_calls),
      context_edit_enabled: String(settings.context_edit_enabled),
      context_edit_max_chars: String(settings.context_edit_max_chars),
      context_edit_keep_recent_messages: String(settings.context_edit_keep_recent_messages),
      context_edit_max_tool_result_chars: String(settings.context_edit_max_tool_result_chars),
      price_in: String(settings.price_in),
      price_out: String(settings.price_out),
    };
    if (apiKeyInput.trim()) updates.api_key = apiKeyInput.trim();
    try {
      await invoke("save_settings", { updates });
      priceIn = Number(settings.price_in) || 0; // reflect new rate immediately
      priceOut = Number(settings.price_out) || 0;
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
    if (rightPanel === "checkpoints") { rightPanel = ""; return; }
    rightPanel = "checkpoints";
    checkpointBusy = true;
    try {
      await loadCheckpoints();
    } catch (e) {
      messages.push({ role: "note", text: `检查点加载失败：${e}` });
    }
    checkpointBusy = false;
  }

  async function saveCheckpoint() {
    checkpointBusy = true;
    try {
      const cp = await invoke<Checkpoint>("create_checkpoint", { label: checkpointLabel });
      checkpointLabel = "";
      await loadCheckpoints();
      messages.push({ role: "note", text: `检查点已保存：${cp.id}` });
    } catch (e) {
      messages.push({ role: "note", text: `检查点失败：${e}` });
    }
    checkpointBusy = false;
  }

  async function restoreCheckpoint(id: string) {
    if (busy || checkpointBusy) return;
    if (!window.confirm(`恢复检查点 ${id}？`)) return;
    checkpointBusy = true;
    try {
      const report = await invoke<RestoreReport>("restore_checkpoint", { id });
      await loadCheckpoints();
      messages.push({
        role: "note",
        text: `已恢复 ${report.checkpoint_id}：${report.restored_files} 个文件，删除 ${report.deleted_files} 个。`,
      });
    } catch (e) {
      messages.push({ role: "note", text: `恢复失败：${e}` });
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
    archived: boolean;
  };
  let branchOpen = $state(false);
  let branches = $state<BranchInfo[]>([]);
  let newBranch = $state("");
  let branchBusy = $state(false);
  type FileChange = { path: string; added: number; removed: number; kind: string };
  let diffOpen = $state(false);
  let diffFiles = $state<FileChange[]>([]);
  let diffOpenFiles = $state<Record<string, string>>({}); // path -> loaded diff text
  let historyOpen = $state(false);
  let sessions = $state<SessionRow[]>([]);
  // Pin the active session to the top of 最近会话 so it's always findable.
  let showArchived = $state(false);
  // Pin the active session to the top; hide archived ones unless toggled (the
  // active session always stays visible even if archived).
  const orderedSessions = $derived.by(() => {
    const visible = sessions.filter(
      (s) => showArchived || !s.archived || s.session_id === currentSessionId,
    );
    if (!currentSessionId) return visible;
    return [
      ...visible.filter((s) => s.session_id === currentSessionId),
      ...visible.filter((s) => s.session_id !== currentSessionId),
    ];
  });
  const archivedCount = $derived(sessions.filter((s) => s.archived).length);

  // 13-digit ms-epoch string → compact relative / date label.
  function fmtWhen(ms: string): string {
    const t = Number(ms);
    if (!t) return "";
    const diff = Date.now() - t;
    if (diff < 60_000) return "刚刚";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
    const d = new Date(t);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getMonth() + 1}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }
  async function archiveSession(id: string, archived: boolean) {
    try {
      await invoke("archive_session", { sessionId: id, archived });
      const s = sessions.find((x) => x.session_id === id);
      if (s) s.archived = archived; // optimistic
      refreshSessions();
    } catch (e) {
      messages.push({ role: "note", text: `归档失败：${e}` });
    }
  }

  async function loadBranches() {
    branches = await invoke<BranchInfo[]>("git_branches");
  }
  async function openBranches() {
    if (rightPanel === "branches") { rightPanel = ""; return; }
    rightPanel = "branches";
    branchBusy = true;
    try {
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `分支加载失败：${e}` });
    }
    branchBusy = false;
  }
  async function createBranch() {
    if (!newBranch.trim()) return;
    branchBusy = true;
    try {
      await invoke("git_create_branch", { name: newBranch });
      messages.push({ role: "note", text: `已新建并切换到分支 ${newBranch}。` });
      newBranch = "";
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `新建分支失败：${e}` });
    }
    branchBusy = false;
  }
  async function switchBranch(name: string) {
    if (branchBusy) return;
    branchBusy = true;
    try {
      await invoke("git_switch_branch", { name });
      messages.push({ role: "note", text: `已切换到分支 ${name}。` });
      await loadBranches();
    } catch (e) {
      messages.push({ role: "note", text: `切换失败：${e}` });
    }
    branchBusy = false;
  }
  async function openDiff() {
    if (rightPanel === "diff") { rightPanel = ""; return; }
    rightPanel = "diff";
    diffOpenFiles = {};
    try {
      diffFiles = await invoke<FileChange[]>("git_changes");
    } catch (e) {
      diffFiles = [];
      messages.push({ role: "note", text: `Diff 失败：${e}` });
    }
  }
  async function toggleFile(path: string) {
    if (path in diffOpenFiles) {
      const { [path]: _drop, ...rest } = diffOpenFiles;
      diffOpenFiles = rest;
      return;
    }
    try {
      const d = await invoke<string>("git_file_diff", { path });
      diffOpenFiles = { ...diffOpenFiles, [path]: d };
    } catch (e) {
      diffOpenFiles = { ...diffOpenFiles, [path]: `diff failed: ${e}` };
    }
  }
  async function reloadPanel() {
    try {
      if (rightPanel === "files") await loadDir(filesPath);
      else if (rightPanel === "branches") await loadBranches();
      else if (rightPanel === "diff") { diffOpenFiles = {}; diffFiles = await invoke<FileChange[]>("git_changes"); }
      else if (rightPanel === "memory") await loadNotes();
      else if (rightPanel === "checkpoints") await loadCheckpoints();
    } catch (e) {
      messages.push({ role: "note", text: `刷新失败：${e}` });
    }
  }
  async function refreshSessions() {
    try {
      sessions = await invoke<SessionRow[]>("list_sessions");
    } catch {
      /* index may not exist yet */
    }
  }
  function toggleSidebar() {
    sidebarOpen = !sidebarOpen;
  }
  async function newSession() {
    messages = [];
    sessionTitle = "新会话";
    currentSessionId = "";
    try {
      await invoke("new_session");
    } catch (e) {
      messages.push({ role: "note", text: `新建会话失败：${e}` });
    }
  }
  async function resumeSession(id: string, title = "") {
    busy = true;
    sessionTitle = title || "会话";
    currentSessionId = id;
    try {
      await invoke("resume_session", { sessionId: id });
    } catch (e) {
      busy = false;
      messages.push({ role: "note", text: `继续会话失败：${e}` });
    }
  }
  async function forkSession(id: string, title = "") {
    busy = true;
    sessionTitle = title ? `${title}（分叉）` : "分叉";
    try {
      await invoke("fork_session", { sessionId: id });
      messages.push({ role: "note", text: "已从该会话分叉出新会话。" });
    } catch (e) {
      busy = false;
      messages.push({ role: "note", text: `分叉失败：${e}` });
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
    if (rightPanel === "memory") { rightPanel = ""; return; }
    rightPanel = "memory";
    hermesBusy = true;
    try {
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `记忆加载失败：${e}` });
    }
    hermesBusy = false;
  }
  async function consolidateMemory() {
    hermesBusy = true;
    try {
      const removed = await invoke<number>("memory_consolidate");
      messages.push({ role: "note", text: `记忆：合并了 ${removed} 条近重复经验。` });
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `记忆整理失败：${e}` });
    }
    hermesBusy = false;
  }
  async function addNote() {
    if (!newNote.trim()) return;
    hermesBusy = true;
    try {
      const tags = newNoteTags.split(",").map((t) => t.trim()).filter(Boolean);
      const saved = await invoke<boolean>("memory_add", { note: newNote, tags });
      messages.push({ role: "note", text: saved ? "记忆：已保存。" : "记忆：已存在（未重复）。" });
      newNote = "";
      newNoteTags = "";
      await loadNotes();
    } catch (e) {
      messages.push({ role: "note", text: `记忆添加失败：${e}` });
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

  // ── Slash command palette (type `/` in the composer) ──────────────────────
  let slashIdx = $state(0);
  const showSlash = $derived(input.startsWith("/") && !input.includes("\n"));
  const slashFilter = $derived(showSlash ? input.slice(1).trim().toLowerCase() : "");
  function forkCurrent() {
    if (currentSessionId) forkSession(currentSessionId, sessionTitle);
    else messages.push({ role: "note", text: "当前会话还没有快照，无法分叉（先发一条消息）。" });
  }
  function cmdUsage() {
    const c = priceIn || priceOut ? ` · ≈¥${fmtCost(cost)}` : "";
    messages.push({ role: "note", text: `本会话用量：输入 ${tokIn} / 输出 ${tokOut} tokens${c}` });
  }
  async function cmdMcp() {
    try {
      const rows = await invoke<{ name: string; command: string }[]>("list_mcp");
      messages.push({
        role: "note",
        text: rows.length
          ? `MCP 服务器（${rows.length}）：\n` + rows.map((r) => `· ${r.name} — ${r.command}`).join("\n")
          : "未配置 MCP 服务器（~/.nanocodex/mcp.toml）。",
      });
    } catch (e) {
      messages.push({ role: "note", text: `读取 MCP 失败：${e}` });
    }
  }
  function cmdFeedback() {
    invoke("open_url", { url: "https://github.com/dgy-github/nanocodex/issues" }).catch((e) =>
      messages.push({ role: "note", text: `打开反馈页失败：${e}` }),
    );
  }
  function cmdUltrareview() {
    input = "请用最严格的标准复查刚才的改动 / 结论：逐条列出潜在 bug、边界情况、错误假设与遗漏，并给出具体修正。";
  }
  function cmdBtw() {
    input = "补充说明：";
  }
  function cmdSoon(name: string) {
    messages.push({ role: "note", text: `「${name}」规划中（需要专门的后台支持），下一步实现。` });
  }
  function cmdRename() {
    messages.push({ role: "note", text: "会话重命名规划中（需要后端写入会话索引），下一步实现。" });
  }
  type SlashCmd = { id: string; label: string; desc: string; run: () => void };
  const SLASH_COMMANDS: SlashCmd[] = [
    { id: "new", label: "新建会话", desc: "开始一个空会话", run: () => newSession() },
    { id: "fork", label: "分叉会话", desc: "从当前会话分叉一个新会话", run: () => forkCurrent() },
    { id: "rename", label: "重命名会话", desc: "给当前会话改名（规划中）", run: () => cmdRename() },
    { id: "model", label: "切换模型", desc: "打开模型选择", run: () => (modelMenuOpen = true) },
    { id: "config", label: "设置", desc: "打开设置面板", run: () => openSettings() },
    { id: "usage", label: "用量", desc: "显示本会话 token / 费用", run: () => cmdUsage() },
    { id: "rewind", label: "检查点", desc: "查看 / 恢复检查点", run: () => openCheckpoints() },
    { id: "files", label: "文件", desc: "浏览 / 预览工作区文件", run: () => openFiles() },
    { id: "diff", label: "改动", desc: "查看工作区 diff", run: () => openDiff() },
    { id: "branches", label: "分支", desc: "Git 分支", run: () => openBranches() },
    { id: "memory", label: "记忆", desc: "项目记忆", run: () => openHermes() },
    { id: "mcp", label: "MCP", desc: "列出已配置的 MCP 服务器", run: () => cmdMcp() },
    { id: "feedback", label: "反馈", desc: "打开 GitHub Issues", run: () => cmdFeedback() },
    { id: "ultrareview", label: "严格复查", desc: "用更严格标准复查（填入模板）", run: () => cmdUltrareview() },
    { id: "btw", label: "补充说明", desc: "插入一条旁注（填入模板）", run: () => cmdBtw() },
    { id: "schedule", label: "定时任务", desc: "定时运行（规划中）", run: () => cmdSoon("定时任务") },
    { id: "workflows", label: "多-agent 编排", desc: "orchestrator 编排（规划中）", run: () => cmdSoon("多-agent 编排") },
  ];
  const slashMatches = $derived(
    showSlash
      ? SLASH_COMMANDS.filter((c) => c.id.includes(slashFilter) || c.label.toLowerCase().includes(slashFilter))
      : [],
  );
  function runSlash(c: SlashCmd) {
    input = "";
    c.run();
  }
</script>

<main class="app">
  <aside class="sidebar" class:collapsed={!sidebarOpen}>
    <div class="side-head">
      <span class="side-brand">nanocodex</span>
      <button class="side-collapse" onclick={toggleSidebar} title="收起侧边栏" aria-label="收起侧边栏">‹</button>
    </div>
    <button class="new-session" onclick={newSession}>
      <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      新会话
    </button>

    <div class="side-recents">
      <div class="side-h">
        最近会话
        {#if archivedCount}
          <button class="side-h-toggle" onclick={() => (showArchived = !showArchived)}>
            {showArchived ? "隐藏归档" : `已归档 ${archivedCount}`}
          </button>
        {/if}
      </div>
      {#if sessions.length === 0}
        <div class="side-empty">暂无会话</div>
      {/if}
      {#each orderedSessions as s}
        <div class="recent-item" class:active={s.session_id === currentSessionId} class:archived={s.archived}>
          <button class="recent-main" title={s.snippet || s.title} disabled={busy || !s.has_snapshot}
            onclick={() => resumeSession(s.session_id, s.title)}>
            <span class="recent-dot">●</span>
            <span class="recent-text">
              <span class="recent-title">{s.title || "（未命名）"}</span>
              <span class="recent-when">{fmtWhen(s.updated_at)}{s.archived ? " · 已归档" : ""}</span>
            </span>
          </button>
          <button class="recent-act" title="从此处分叉新会话" disabled={busy || !s.has_snapshot}
            onclick={() => forkSession(s.session_id, s.title)} aria-label="分叉">⑂</button>
          <button class="recent-act" title={s.archived ? "取消归档" : "归档此会话"}
            onclick={() => archiveSession(s.session_id, !s.archived)} aria-label="归档">{s.archived ? "↩" : "🗄"}</button>
        </div>
      {/each}
    </div>

    <div class="side-foot">
      <button class="foot-ws" title={`工作区：${workspace}（点击切换）`} onclick={chooseWorkspace}>
        📁 {workspace ? baseName(workspace) : "选择工作区"}
      </button>
      <button class="foot-gear" title="设置" onclick={openSettings} aria-label="设置">⚙</button>
    </div>
  </aside>

  <div class="workarea">
  <section class="main">
    <header class="topbar">
      <button class="collapse" onclick={toggleSidebar} title={sidebarOpen ? "收起侧边栏" : "展开侧边栏"} aria-label="Toggle sidebar">▣</button>
      <span class="title">{sessionTitle}</span>
      <div class="model-wrap">
        <button class="model-pill" onclick={() => (modelMenuOpen = !modelMenuOpen)}
          disabled={models.length === 0} title="切换模型">
          {currentModel || header} ▾
        </button>
        {#if modelMenuOpen}
          <button class="menu-backdrop" aria-label="关闭" onclick={() => (modelMenuOpen = false)}></button>
          <div class="model-menu" role="menu">
            {#each models as m}
              <button class="model-opt" role="menuitemradio" aria-checked={m === currentModel}
                onclick={() => selectModel(m)}>
                <span class="opt-check">{m === currentModel ? "✓" : ""}</span>
                <span class="opt-name">{m}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
      {#if sandboxMode}<span class="meta">{sandboxMode}</span>{/if}
      {#if busy}<span class="spinner" title="处理中…">●</span>{/if}
      <span class="topbar-actions">
        <button class="tbtn" class:on={rightPanel === "files"} onclick={openFiles} title="文件" aria-label="文件">
          <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h3.5l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
        </button>
        <button class="tbtn" class:on={rightPanel === "diff"} onclick={openDiff} title="改动" aria-label="改动">
          <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M5 8h7M8.5 4.5v7M5 17h7"/></svg>
        </button>
        <button class="tbtn" class:on={rightPanel === "branches"} onclick={openBranches} title="分支" aria-label="分支">
          <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="6" cy="6" r="2.2"/><circle cx="6" cy="18" r="2.2"/><circle cx="18" cy="8" r="2.2"/><path d="M6 8.2v7.6M6 13a6 6 0 0 0 6-6h3.8"/></svg>
        </button>
        <button class="tbtn" class:on={rightPanel === "memory"} onclick={openHermes} title="记忆" aria-label="记忆">
          <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M5 4h11a2 2 0 0 1 2 2v14H7a2 2 0 0 1-2-2z"/><path d="M9 4v16"/></svg>
        </button>
        <button class="tbtn" class:on={rightPanel === "checkpoints"} onclick={openCheckpoints} title="检查点" aria-label="检查点">
          <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M12 8v4.2l2.8 1.7"/></svg>
        </button>
      </span>
    </header>

    <div class="scroll" bind:this={scroller}>
      {#if messages.length === 0}
        <div class="empty-wrap">
          <div class="empty-mark">✦</div>
          <p class="empty">让我检查或修改你的工作区。<br />试试「列出文件」或「用 apply_patch 创建 hello.txt」。</p>
        </div>
      {/if}
      {#each messages as m}
        {#if m.role === "user"}
          <div class="msg user"><div class="bubble">{m.text}</div></div>
        {:else if m.role === "assistant"}
          <div class="msg assistant"><div class="bubble md">{@html renderMarkdown(m.text)}</div></div>
        {:else if m.role === "note"}
          <div class="msg note">{m.text}</div>
        {:else if m.role === "tool"}
          <div class="tool" class:collapsed={m.collapsed}>
            <button
              class="tool-head"
              onclick={() => toggleTool(m)}
              disabled={m.result === undefined}
              aria-expanded={m.result !== undefined && !m.collapsed}
              title={m.result === undefined ? "运行中…" : m.collapsed ? "展开" : "折叠"}
            >
              <span class="tcaret" aria-hidden="true">{m.result === undefined ? "•" : m.collapsed ? "▸" : "▾"}</span>
              <span class="tname">⚙ {m.name}</span>
              {#if m.args}<code class="targs">{m.args}</code>{/if}
              {#if m.result === undefined}
                <span class="trunning">运行中…</span>
              {:else if m.collapsed}
                <span class="tcollapsed-hint">{collapsedHint(m.result)}</span>
              {/if}
            </button>
            {#if m.result !== undefined && !m.collapsed}
              <pre class="tresult">{m.result}</pre>
            {/if}
          </div>
        {/if}
      {/each}
      {#if busy && streamingIdx === null}
        <div class="thinking"><span class="tdot"></span><span class="tdot"></span><span class="tdot"></span> 思考中…</div>
      {/if}
    </div>

    <footer>
      <div class="composer-meta">
        <div class="approval-wrap">
          <button class="approval-pill" class:danger={permissionMode === "bypass"} class:plan={permissionMode === "plan"}
            onclick={() => (modeMenuOpen = !modeMenuOpen)}
            title="权限模式（Claude Code 四态）">
            {modeIcon(permissionMode)} {modeLabel(permissionMode)} ▾
          </button>
          {#if modeMenuOpen}
            <button class="menu-backdrop" aria-label="关闭" onclick={() => (modeMenuOpen = false)}></button>
            <div class="approval-menu" role="menu">
              {#each PERMISSION_MODES as opt}
                <button class="approval-opt" role="menuitemradio" aria-checked={permissionMode === opt.id}
                  onclick={() => selectMode(opt.id)}>
                  <span class="opt-check">{permissionMode === opt.id ? "✓" : ""}</span>
                  <span class="opt-text">
                    <span class="opt-name">{modeIcon(opt.id)} {opt.label}</span>
                    <span class="opt-id">{opt.desc}</span>
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        {#if tokIn || tokOut}
          <span class="usage" title="本会话累计 token（输入 / 输出）{priceIn || priceOut ? ' · 费用按设置的单价估算' : ''}">用量 ↑{fmtTok(tokIn)} ↓{fmtTok(tokOut)}{#if priceIn || priceOut}{" · ≈¥"}{fmtCost(cost)}{/if}</span>
        {/if}
      </div>
      {#if queued.length}
        <div class="attachments">
          {#each queued as q, i}
            <span class="chip queued-chip" title={q.shown}>
              ⏳ {q.shown.split("\n")[0].slice(0, 40)}
              <button class="chipx" onclick={() => (queued = queued.filter((_, j) => j !== i))} aria-label="Remove">×</button>
            </span>
          {/each}
        </div>
      {/if}
      {#if attached.length}
        <div class="attachments">
          {#each attached as p}
            <span class="chip" title={p}>
              {isImage(p) ? "🖼" : "📄"} {baseName(p)}
              <button class="chipx" onclick={() => removeAttachment(p)} aria-label="Remove">×</button>
            </span>
          {/each}
        </div>
      {/if}
      {#if showSlash && slashMatches.length}
        <div class="slash-menu" role="listbox" aria-label="命令">
          {#each slashMatches as c, i}
            <button class="slash-item" class:on={i === slashIdx} role="option" aria-selected={i === slashIdx}
              onmouseenter={() => (slashIdx = i)} onclick={() => runSlash(c)}>
              <span class="slash-cmd">/{c.id}</span>
              <span class="slash-label">{c.label}</span>
              <span class="slash-desc">{c.desc}</span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="composer-row">
        <button class="toolbtn attach" title="添加文件/图片" onclick={attachFiles} aria-label="添加">📎</button>
        <textarea
          bind:value={input}
          onkeydown={onKey}
          oninput={() => { if (input.startsWith("/")) slashIdx = 0; }}
          onpaste={handlePaste}
          placeholder="给 nanocodex 发消息…（/ 唤出命令，Enter 发送，Shift+Enter 换行，Ctrl+V 粘贴图片）"
          rows="2"
        ></textarea>
        <button onclick={send} disabled={(input.trim() === "" && attached.length === 0) || (busy && queued.length >= 2)}>
          {busy ? "排队" : "发送"}
        </button>
      </div>
    </footer>
  </section>

  {#if approval}
    <div class="overlay">
      <div class="modal">
        <h3>需要审批</h3>
        <p class="areason">{approval.reason}</p>
        <div class="afield"><span>操作</span><code>{approval.command}</code></div>
        <div class="afield"><span>目录</span><code>{approval.cwd}</code></div>
        {#if approval.details}
          <pre class="adetails">{approval.details}</pre>
        {/if}
        <div class="abtns">
          <button class="deny" onclick={() => decide("deny")}>拒绝</button>
          <button class="plain" onclick={() => decide("always")} title="本次会话始终允许（命令 / 编辑）">始终允许</button>
          <button class="ok" onclick={() => decide("once")}>批准</button>
        </div>
      </div>
    </div>
  {/if}

  {#if rightPanel === "checkpoints"}
    <aside class="rightpanel">
      <div class="rp-head"><span class="rp-title">检查点</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
      <div class="rp-body">
        <div class="checkpoint-create">
          <input bind:value={checkpointLabel} placeholder="标签" />
          <button onclick={saveCheckpoint} disabled={checkpointBusy}>保存</button>
          <button class="plain" onclick={loadCheckpoints} disabled={checkpointBusy}>刷新</button>
        </div>
        <div class="checkpoint-list">
          {#if checkpoints.length === 0}
            <p class="emptyline">暂无检查点。</p>
          {/if}
          {#each checkpoints as cp}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <button class="link-row" onclick={() => toggleCheckpointDetail(cp.id)} title="查看快照文件">
                  <span class="wt-caret">{cp.id in checkpointFiles ? "▾" : "▸"}</span>
                  <strong>{cp.label || "（无标签）"}</strong>
                </button>
                <code>{cp.id}</code>
              </div>
              <div class="checkpoint-meta">
                <span>{cp.created_at}</span>
                <span>{cp.files} 个文件</span>
                <span>跳过 {cp.skipped}</span>
              </div>
              <button class="restore" onclick={() => restoreCheckpoint(cp.id)} disabled={busy || checkpointBusy}>
                恢复
              </button>
              {#if cp.id in checkpointFiles}
                <div class="detail-list">
                  {#if checkpointFiles[cp.id].length === 0}
                    <div class="detail-row">（无文件）</div>
                  {/if}
                  {#each checkpointFiles[cp.id] as fp}
                    <div class="detail-row"><code class="dl-path">{fp}</code></div>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </aside>
  {/if}

  {#if rightPanel === "branches"}
    <aside class="rightpanel">
      <div class="rp-head"><span class="rp-title">Git 分支</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
      <div class="rp-body">
        <div class="checkpoint-create">
          <input bind:value={newBranch} placeholder="新分支名" />
          <button onclick={createBranch} disabled={branchBusy}>新建并切换</button>
          <button class="plain" onclick={loadBranches} disabled={branchBusy}>刷新</button>
        </div>
        <div class="checkpoint-list">
          {#if branches.length === 0}
            <p class="emptyline">暂无分支。</p>
          {/if}
          {#each branches as b}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <button class="link-row" onclick={() => toggleBranchDetail(b.name)} title="查看最近提交">
                  <span class="wt-caret">{b.name in branchCommits ? "▾" : "▸"}</span>
                  <strong>{b.current ? "● " : ""}{b.name}</strong>
                </button>
              </div>
              <button class="restore" onclick={() => switchBranch(b.name)} disabled={branchBusy || b.current}>
                {b.current ? "当前" : "切换"}
              </button>
              {#if b.name in branchCommits}
                <div class="detail-list">
                  {#if branchCommits[b.name].length === 0}
                    <div class="detail-row">（无提交）</div>
                  {/if}
                  {#each branchCommits[b.name] as c}
                    <div class="detail-row">
                      {#if c.hash}<code class="dl-hash">{c.hash}</code>{/if}
                      <span class="dl-subj">{c.subject}</span>
                      {#if c.when}<span class="dl-when">{c.when}</span>{/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </aside>
  {/if}

  {#if rightPanel === "files"}
    <aside class="rightpanel">
      <div class="rp-head"><span class="rp-title">文件</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
      <div class="rp-body">
        {#if filePreview}
          <div class="fx-bar">
            <button class="plain" onclick={() => (filePreview = null)}>‹ 返回</button>
            <code class="fx-path" title={filePreview.path}>{filePreview.path}</code>
            <button class="plain" onclick={() => { if (filePreview) insertMention(filePreview.path); }} title="把 @引用 插入输入框">＋@引用</button>
          </div>
          <pre class="fx-preview">{filePreview.content}</pre>
        {:else}
          <div class="fx-bar">
            <button class="plain" onclick={filesUp} disabled={!filesPath}>↑ 上级</button>
            <code class="fx-path">/{filesPath}</code>
          </div>
          <div class="wt-list">
            {#if filesEntries.length === 0}
              <p class="emptyline">（空）</p>
            {/if}
            {#each filesEntries as e}
              <button class="fx-row" onclick={() => pickFile(e)} title={e.is_dir ? "打开文件夹" : "预览文件"}>
                <span class="fx-ic">{e.is_dir ? "📁" : "📄"}</span>
                <span class="fx-name">{e.name}</span>
                <span class="fx-go">›</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </aside>
  {/if}

  {#if rightPanel === "diff"}
    <aside class="rightpanel">
      <div class="rp-head"><span class="rp-title">工作区改动</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
      <div class="rp-body">
        <div class="wt-list">
          {#if diffFiles.length === 0}
            <p class="emptyline">工作区没有改动。</p>
          {/if}
          {#each diffFiles as f}
            <div class="wt-file">
              <button class="wt-head" onclick={() => toggleFile(f.path)}>
                <span class="wt-caret">{f.path in diffOpenFiles ? "▾" : "▸"}</span>
                <span class="wt-kind wt-{f.kind}">{f.kind[0].toUpperCase()}</span>
                <span class="wt-path">{f.path}</span>
                <span class="wt-stat">
                  {#if f.added >= 0}<span class="wt-add">+{f.added}</span>{/if}
                  {#if f.removed >= 0}<span class="wt-del">-{f.removed}</span>{/if}
                </span>
              </button>
              {#if f.path in diffOpenFiles}
                <pre class="wt-diff">{#each diffOpenFiles[f.path].split("\n") as ln}<span class="dl {diffLineClass(ln)}">{ln === "" ? " " : ln}</span>{/each}</pre>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </aside>
  {/if}

  {#if historyOpen}
    <div class="overlay">
      <div class="modal">
        <h3>会话历史</h3>
        <div class="checkpoint-list">
          {#if sessions.length === 0}
            <p class="emptyline">暂无保存的会话。</p>
          {/if}
          {#each sessions as s}
            <div class="checkpoint-row">
              <div class="checkpoint-main">
                <strong>{s.title || "（未命名）"}</strong>
                <code>{s.snippet}</code>
              </div>
              <div class="session-actions">
                <button class="plain" onclick={() => resumeSession(s.session_id)} disabled={busy || !s.has_snapshot} title="继续此会话">继续</button>
                <button class="restore" onclick={() => forkSession(s.session_id)} disabled={busy || !s.has_snapshot} title="从此处分叉新会话">⑂ 分叉</button>
              </div>
              <div class="checkpoint-meta">
                <span>{s.updated_at}</span>
                <span>{s.user_messages} 问 · {s.assistant_messages} 答 · {s.tool_calls} 工具</span>
                {#if !s.has_snapshot}<span>（无快照）</span>{/if}
              </div>
            </div>
          {/each}
        </div>
        <div class="abtns">
          <button class="plain" onclick={refreshSessions}>刷新</button>
          <button class="deny" onclick={() => (historyOpen = false)}>关闭</button>
        </div>
      </div>
    </div>
  {/if}

  {#if rightPanel === "memory"}
    <aside class="rightpanel">
      <div class="rp-head"><span class="rp-title">项目记忆</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
      <div class="rp-body">
        <p class="emptyline">已验证的经验，会作为线索在未来会话中被回忆。（骨架自进化 / Hermes 是另一个功能，见 forge。）</p>
        <div class="checkpoint-create">
          <input bind:value={newNote} placeholder="记录一条已验证的经验…" />
          <input bind:value={newNoteTags} placeholder="标签（逗号分隔）" style="max-width:140px" />
          <button onclick={addNote} disabled={hermesBusy}>添加</button>
        </div>
        <div class="checkpoint-create">
          <button onclick={consolidateMemory} disabled={hermesBusy}>整理：合并重复</button>
          <button class="plain" onclick={loadNotes} disabled={hermesBusy}>刷新</button>
          <span class="emptyline">{notes.length} 条</span>
        </div>
        <div class="checkpoint-list">
          {#if notes.length === 0}
            <p class="emptyline">暂无经验。</p>
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
      </div>
    </aside>
  {/if}

  {#if settings}
    <div class="overlay">
      <div class="modal">
        <h3>设置</h3>
        {#if configLocation}
          <div class="config-entry">
            <span>配置</span>
            <code title={configLocation.config_path}>{configLocation.config_path}</code>
            <button class="plain" onclick={openConfigFile}>打开文件</button>
            <button class="plain" onclick={openConfigDir}>打开文件夹</button>
          </div>
        {/if}
        <label>
          <span>模型</span>
          <select bind:value={settings.model}>
            {#each settings.available_models as m}<option value={m}>{m}</option>{/each}
          </select>
        </label>
        <p class="settings-note">权限（沙箱 / 审批）由顶部输入框旁的「权限模式」控制：规划 / 默认 / 自动接受编辑 / 全权放行。</p>
        <label>
          <span>推理强度</span>
          <input bind:value={settings.reasoning_effort} placeholder="auto | low | medium | high | max | off" />
        </label>
        <label>
          <span>模型调用上限</span>
          <input type="number" min="1" bind:value={settings.max_iterations} />
        </label>
        <label>
          <span>工具调用上限</span>
          <input type="number" min="0" bind:value={settings.max_tool_calls} />
        </label>
        <label class="check">
          <span>上下文裁剪</span>
          <input type="checkbox" bind:checked={settings.context_edit_enabled} />
        </label>
        <label>
          <span>上下文字符上限</span>
          <input type="number" min="1" bind:value={settings.context_edit_max_chars} />
        </label>
        <label>
          <span>保留最近消息数</span>
          <input type="number" min="1" bind:value={settings.context_edit_keep_recent_messages} />
        </label>
        <label>
          <span>工具结果字符上限</span>
          <input type="number" min="1" bind:value={settings.context_edit_max_tool_result_chars} />
        </label>
        <label>
          <span>输入单价 ¥/百万</span>
          <input type="number" min="0" step="0.01" bind:value={settings.price_in} placeholder="0 = 不计费" />
        </label>
        <label>
          <span>输出单价 ¥/百万</span>
          <input type="number" min="0" step="0.01" bind:value={settings.price_out} placeholder="0 = 不计费" />
        </label>
        <label>
          <span>Base URL</span>
          <input bind:value={settings.base_url} />
        </label>
        <label>
          <span>API 密钥</span>
          <input
            type="password"
            bind:value={apiKeyInput}
            placeholder={settings.has_api_key ? `保持当前（${settings.api_key_masked}）` : "设置 API 密钥"}
          />
        </label>
        <div class="abtns">
          <button class="deny" onclick={() => (settings = null)}>取消</button>
          <button class="ok" onclick={saveSettings} disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  {/if}
  </div>
</main>
