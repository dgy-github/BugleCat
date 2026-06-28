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
    | { kind: "ready"; model: string; sandbox: string; workspace: string; session_id: string }
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
  let streamingIdx = $state<number | null>(null); // index of the bubble being streamed
  const fmtTok = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`);

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
  function toggleTool(m: Msg) {
    if (m.role === "tool" && m.result !== undefined) m.collapsed = !m.collapsed;
  }
  let rightPanel = $state(""); // "" | files | branches | diff | memory | checkpoints
  const PANEL_TITLES: Record<string, string> = {
    files: "文件", branches: "Git 分支", diff: "工作区改动", memory: "项目记忆", checkpoints: "检查点",
  };
  let currentSessionId = $state("");
  let approvalPolicy = $state("on-request");
  let approvalMenuOpen = $state(false);
  const APPROVAL_OPTS = [
    { id: "untrusted", label: "每条都询问", desc: "每个命令都要批准" },
    { id: "on-failure", label: "失败再问", desc: "仅当沙箱失败时再询问升权" },
    { id: "on-request", label: "按需询问", desc: "仅升权时询问（默认）" },
    { id: "never", label: "从不升权", desc: "始终留在沙箱；最严格" },
    { id: "__auto", label: "自动执行（全权）", desc: "危险：所有命令直接执行，不询问" },
  ];
  const AUTO_SANDBOX = "danger-full-access";
  const isAuto = () => sandboxMode === AUTO_SANDBOX;
  const optActive = (id: string) =>
    id === "__auto" ? isAuto() : approvalPolicy === id && !isAuto();
  const modeLabel = () =>
    isAuto() ? "自动执行" : (APPROVAL_OPTS.find((o) => o.id === approvalPolicy)?.label ?? approvalPolicy);
  let scroller: HTMLDivElement;

  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(async () => {
    // Header falls back to a direct status call until the agent thread is Ready.
    try {
      const s = await invoke<{ model: string; sandbox: string; approval: string }>("get_status");
      header = `${s.model} · ${s.sandbox}`;
      approvalPolicy = s.approval;
      sandboxMode = s.sandbox;
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
          // Learn the active session's real id so 最近会话 can mark/return to it.
          if (p.session_id) currentSessionId = p.session_id;
          refreshSessions();
          break;
        case "assistant_delta":
          if (streamingIdx === null) {
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
            if (m && m.role === "assistant") m.text = p.text;
            streamingIdx = null;
          } else {
            messages.push({ role: "assistant", text: p.text });
          }
          break;
        case "tool_start":
          streamingIdx = null; // close any in-progress stream before the tool
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
          messages = p.messages.map((m) =>
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

  // File explorer over the workspace.
  async function loadDir(rel: string) {
    try {
      filesEntries = await invoke<DirEntry[]>("list_dir", { rel });
      filesPath = rel;
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
    if (!filesPath) return;
    const parent = filesPath.includes("/") ? filesPath.slice(0, filesPath.lastIndexOf("/")) : "";
    loadDir(parent);
  }
  function pickFile(entry: DirEntry) {
    if (entry.is_dir) {
      loadDir(entry.path);
    } else {
      input = input ? `${input} @${entry.path}` : `@${entry.path}`;
      filesOpen = false;
    }
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
  async function selectApproval(policy: string) {
    approvalMenuOpen = false;
    try {
      if (policy === "__auto") {
        if (isAuto()) return;
        sandboxMode = AUTO_SANDBOX;
        await invoke("set_sandbox", { mode: AUTO_SANDBOX });
        return;
      }
      // Leaving auto mode: restore a writable sandbox.
      if (isAuto()) {
        sandboxMode = "workspace-write";
        await invoke("set_sandbox", { mode: "workspace-write" });
      }
      if (policy !== approvalPolicy) {
        approvalPolicy = policy;
        await invoke("set_approval", { policy });
      }
    } catch (e) {
      messages.push({ role: "note", text: `设置权限失败：${e}` });
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

    <nav class="side-nav">
      <button class="nav-item" onclick={openFiles}>
        <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h3.5l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
        文件
      </button>
      <button class="nav-item" onclick={openBranches}>
        <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="6" cy="6" r="2.2"/><circle cx="6" cy="18" r="2.2"/><circle cx="18" cy="8" r="2.2"/><path d="M6 8.2v7.6M6 13a6 6 0 0 0 6-6h3.8"/></svg>
        分支
      </button>
      <button class="nav-item" onclick={openDiff}>
        <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M5 8h7M8.5 4.5v7M5 17h7"/></svg>
        改动
      </button>
      <button class="nav-item" onclick={openHermes}>
        <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M5 4h11a2 2 0 0 1 2 2v14H7a2 2 0 0 1-2-2z"/><path d="M9 4v16"/></svg>
        记忆
      </button>
      <button class="nav-item" onclick={openCheckpoints}>
        <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M12 8v4.2l2.8 1.7"/></svg>
        检查点
      </button>
    </nav>

    <div class="side-recents">
      <div class="side-h">最近会话</div>
      {#if sessions.length === 0}
        <div class="side-empty">暂无会话</div>
      {/if}
      {#each sessions as s}
        <div class="recent-item" class:active={s.session_id === currentSessionId}>
          <button class="recent-main" title={s.snippet || s.title} disabled={busy || !s.has_snapshot}
            onclick={() => resumeSession(s.session_id, s.title)}>
            <span class="recent-dot">●</span>{s.title || "（未命名）"}
          </button>
          <button class="recent-fork" title="从此处分叉新会话" disabled={busy || !s.has_snapshot}
            onclick={() => forkSession(s.session_id, s.title)}>⑂</button>
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
      <span class="meta">{header}</span>
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
          <div class="msg assistant"><div class="bubble">{m.text}</div></div>
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
          <button class="approval-pill" class:danger={isAuto() || approvalPolicy === "never"}
            onclick={() => (approvalMenuOpen = !approvalMenuOpen)}
            title="权限模式">
            🛡 {modeLabel()} ▾
          </button>
          {#if approvalMenuOpen}
            <button class="menu-backdrop" aria-label="关闭" onclick={() => (approvalMenuOpen = false)}></button>
            <div class="approval-menu" role="menu">
              {#each APPROVAL_OPTS as opt}
                <button class="approval-opt" role="menuitemradio" aria-checked={optActive(opt.id)}
                  onclick={() => selectApproval(opt.id)}>
                  <span class="opt-check">{optActive(opt.id) ? "✓" : ""}</span>
                  <span class="opt-text">
                    <span class="opt-name">{opt.label}</span>
                    <span class="opt-id">{opt.desc}</span>
                  </span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        {#if tokIn || tokOut}
          <span class="usage" title="本会话累计 token（输入 / 输出）">用量 ↑{fmtTok(tokIn)} ↓{fmtTok(tokOut)}</span>
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
      <div class="composer-row">
        <button class="toolbtn attach" title="添加文件/图片" onclick={attachFiles} aria-label="添加">📎</button>
        <textarea
          bind:value={input}
          onkeydown={onKey}
          onpaste={handlePaste}
          placeholder="给 nanocodex 发消息…（Enter 发送，Shift+Enter 换行，Ctrl+V 粘贴图片）"
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
          <button class="deny" onclick={() => decide(false)}>拒绝</button>
          <button class="ok" onclick={() => decide(true)}>批准</button>
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
                <strong>{cp.label || "（无标签）"}</strong>
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
                <strong>{b.current ? "● " : ""}{b.name}</strong>
              </div>
              <button class="restore" onclick={() => switchBranch(b.name)} disabled={branchBusy || b.current}>
                {b.current ? "当前" : "切换"}
              </button>
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
        <div class="fx-bar">
          <button class="plain" onclick={filesUp} disabled={!filesPath}>↑ 上级</button>
          <code class="fx-path">/{filesPath}</code>
        </div>
        <div class="wt-list">
          {#if filesEntries.length === 0}
            <p class="emptyline">（空）</p>
          {/if}
          {#each filesEntries as e}
            <button class="fx-row" onclick={() => pickFile(e)} title={e.is_dir ? "打开文件夹" : "插入 @引用"}>
              <span class="fx-ic">{e.is_dir ? "📁" : "📄"}</span>
              <span class="fx-name">{e.name}</span>
              {#if e.is_dir}<span class="fx-go">›</span>{/if}
            </button>
          {/each}
        </div>
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
                <pre class="wt-diff">{diffOpenFiles[f.path]}</pre>
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
        <label>
          <span>沙箱</span>
          <select bind:value={settings.sandbox_mode}>
            {#each settings.sandbox_modes as s}<option value={s}>{s}</option>{/each}
          </select>
        </label>
        <label>
          <span>审批</span>
          <select bind:value={settings.approval_policy}>
            {#each settings.approval_policies as a}<option value={a}>{a}</option>{/each}
          </select>
        </label>
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
