<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import ConversationView from "./components/ConversationView.svelte";
  import Composer from "./components/Composer.svelte";
  import InteractionDialogs from "./components/InteractionDialogs.svelte";
  import WorkspacePanels from "./components/WorkspacePanels.svelte";
  import SettingsModal from "./components/SettingsModal.svelte";
  import SessionSidebar from "./components/SessionSidebar.svelte";
  import TopBar from "./components/TopBar.svelte";

  const IMAGE_EXTS = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
  const isImage = (p: string) => IMAGE_EXTS.includes((p.split(".").pop() || "").toLowerCase());
  const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

  // Mirrors the Rust `UiEvent` enum (serde tag = "kind", snake_case).
  type UiEvent =
    | { kind: "ready"; model: string; sandbox: string; workspace: string; session_id: string; models: string[]; permission_mode: string; reasoning_effort: string; needs_workspace: boolean }
    | { kind: "assistant_delta"; session_id: string; text: string }
    | { kind: "reasoning_delta"; session_id: string; text: string }
    | { kind: "context_compacted"; session_id: string; original_chars: number; edited_chars: number; dropped_messages: number; compressed_tool_results: number }
    | { kind: "assistant"; session_id: string; text: string }
    | { kind: "tool_start"; session_id: string; name: string; args: string }
    | { kind: "tool_result"; session_id: string; name: string; result: string }
    | { kind: "approval"; session_id: string; id: number; command: string; reason: string; cwd: string; details: string }
    | { kind: "question"; session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean }
    | { kind: "done"; session_id: string; final_text: string; stop_reason: string; usage: Record<string, number> }
    | { kind: "session_title"; session_id: string; title: string }
    | { kind: "loaded"; session_id: string; messages: { role: string; text: string; tools?: ToolEntry[] }[] }
    | { kind: "error"; session_id: string; message: string };

  type Approval = { session_id: string; id: number; command: string; reason: string; cwd: string; details: string };
  let approval = $state<Approval | null>(null);
  const approvalsBySession = new Map<string, Approval>();
  type UserQuestion = { session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean };
  let userQuestion = $state<UserQuestion | null>(null);
  const questionsBySession = new Map<string, UserQuestion>();
  let questionAnswer = $state("");

  type Settings = {
    model: string;
    base_url: string;
    vl_model: string;
    vl_base_url: string;
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
    price_currency: "CNY" | "USD";
    api_key_masked: string;
    has_api_key: boolean;
    vl_api_key_masked: string;
    has_vl_api_key: boolean;
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
  let vlApiKeyInput = $state("");
  let saving = $state(false);
  type HarnessDiagnostics = Record<"llm" | "interaction" | "policy" | "context" | "memory" | "compaction" | "mcp" | "attachment" | "media" | "cost_telemetry", boolean>;
  type ExternalPlugin = { manifest: { id: string; name: string; version: string; capabilities: string[] }; root: string; enabled: boolean };
  type CodexPlugin = {
    manifest: { name: string; version?: string; description?: string; keywords: string[] };
    root: string;
    enabled: boolean;
    skill_roots: number;
    has_mcp: boolean;
    has_apps: boolean;
    app_count: number;
    has_hooks: boolean;
  };
  type MarketplaceSource =
    | { source: "local"; path: string }
    | { source: "git"; url: string; path?: string; ref?: string }
    | { source: "npm"; package: string; version?: string };
  type PluginMarketplace = { path: string; marketplace: { name: string; plugins: { name: string; source: MarketplaceSource }[] } };
  let harnessDiagnostics = $state<HarnessDiagnostics | null>(null);
  let externalPlugins = $state<ExternalPlugin[]>([]);
  let codexPlugins = $state<CodexPlugin[]>([]);
  let pluginMarketplaces = $state<PluginMarketplace[]>([]);

  type CatalogModel = {
    provider_id: string;
    model_id: string;
    display_name: string;
    base_url: string;
    price_in: number;
    price_out: number;
    price_currency: "CNY" | "USD";
    price_source: "official_direct" | "aggregator";
    pricing_note: string | null;
    source_url: string;
    updated_at: string;
    context_length?: number | null;
    direct_available: boolean;
  };
  type CatalogProvider = { id: string; name: string; models: CatalogModel[] };
  type ModelCatalogResponse = { providers: CatalogProvider[]; stale: boolean };
  let modelCatalog = $state<ModelCatalogResponse | null>(null);
  let catalogRefreshing = $state(false);
  let presetSaving = $state("");
  const officialProviders = $derived(
    modelCatalog?.providers.filter((provider) => provider.id !== "openrouter") ?? [],
  );
  const openRouterProvider = $derived(
    modelCatalog?.providers.find((provider) => provider.id === "openrouter") ?? null,
  );

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

  type ToolEntry = { name: string; args?: string; result?: string };
  type ToolGroup = { role: "tool_group"; tools: ToolEntry[]; settled: boolean };
  type ReasoningMsg = { role: "reasoning"; text: string; settled: boolean };
  type Msg =
    | { role: "user" | "assistant" | "note" | "compact"; text: string }
    | ReasoningMsg
    | ToolGroup;

  let messages = $state<Msg[]>([]);
  const sessionMessages = new Map<string, Msg[]>();
  let input = $state("");
  let attached = $state<string[]>([]); // absolute file paths attached to the next turn
  let queued = $state<{ text: string; images: string[]; shown: string }[]>([]); // pending turns
  const sessionQueues = new Map<string, { text: string; images: string[]; shown: string }[]>();
  let busy = $state(false);
  let reasoningIdx = $state<number | null>(null);
  const REASONING_DISPLAY_MAX_CHARS = 4000;
  const REASONING_OMITTED = "\n\n…较长思考已省略，仅保留最近内容…\n\n";
  let runningSessions = $state(new Set<string>());
  let stopping = $state(false);
  let switchingSession = $state(false);
  // File explorer (workspace tree)
  type DirEntry = { name: string; path: string; is_dir: boolean };
  let filesOpen = $state(false);
  let filesPath = $state("");
  let filesEntries = $state<DirEntry[]>([]);
  let header = $state("连接中…");
  let workspace = $state("");
  let needsWorkspace = $state(false); // true when cwd is home/root — block prompts
  // Last path segment of the workspace, for the header pill (full path on hover).
  const wsName = $derived(
    workspace ? workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || workspace : "",
  );
  let sessionTitle = $state("新会话");
  let sidebarOpen = $state(true);
  const SIDEBAR_DEFAULT_WIDTH = 250;
  const SIDEBAR_MIN_WIDTH = 190;
  const SIDEBAR_MAX_WIDTH = 440;
  let sidebarWidth = $state(SIDEBAR_DEFAULT_WIDTH);
  let sidebarResizing = $state(false);
  let sandboxMode = $state("");
  let tokIn = $state(0);
  let tokOut = $state(0);
  const protocolUsageBySession = new Map<string, { prompt_tokens: number; completion_tokens: number }>();
  const sessionUsageKey = (sessionId: string) => `ncx.sessionUsage.${sessionId}`;
  function resetSessionUsage() {
    tokIn = 0;
    tokOut = 0;
  }
  function restoreSessionUsage(sessionId: string) {
    resetSessionUsage();
    if (!sessionId) return;
    const protocolUsage = protocolUsageBySession.get(sessionId);
    if (protocolUsage) {
      tokIn = protocolUsage.prompt_tokens;
      tokOut = protocolUsage.completion_tokens;
      persistSessionUsage(sessionId);
      return;
    }
    try {
      const stored = JSON.parse(localStorage.getItem(sessionUsageKey(sessionId)) || "null");
      if (Number.isFinite(stored?.prompt_tokens) && stored.prompt_tokens >= 0) {
        tokIn = stored.prompt_tokens;
      }
      if (Number.isFinite(stored?.completion_tokens) && stored.completion_tokens >= 0) {
        tokOut = stored.completion_tokens;
      }
    } catch { /* missing or invalid local usage is treated as zero */ }
  }
  function persistSessionUsage(sessionId: string) {
    if (!sessionId) return;
    try {
      localStorage.setItem(sessionUsageKey(sessionId), JSON.stringify({
        prompt_tokens: tokIn,
        completion_tokens: tokOut,
      }));
    } catch { /* storage is optional */ }
  }
  function addSessionUsage(sessionId: string, usage: Record<string, number>) {
    if (!sessionId) return;
    const prompt = usage.prompt_tokens || 0;
    const completion = usage.completion_tokens || 0;
    if (sessionId === currentSessionId) {
      tokIn += prompt;
      tokOut += completion;
      persistSessionUsage(sessionId);
      return;
    }
    try {
      const stored = JSON.parse(localStorage.getItem(sessionUsageKey(sessionId)) || "null");
      localStorage.setItem(sessionUsageKey(sessionId), JSON.stringify({
        prompt_tokens: (Number(stored?.prompt_tokens) || 0) + prompt,
        completion_tokens: (Number(stored?.completion_tokens) || 0) + completion,
      }));
    } catch { /* storage is optional */ }
  }
  function setSessionRunning(sessionId: string, running: boolean) {
    const next = new Set(runningSessions);
    if (running) next.add(sessionId);
    else next.delete(sessionId);
    runningSessions = next;
  }
  // Per-1M-token prices (from config); 0 = unknown → cost is hidden.
  let priceIn = $state(0);
  let priceOut = $state(0);
  let priceCurrency = $state<"CNY" | "USD">("CNY");
  const cost = $derived((tokIn / 1e6) * priceIn + (tokOut / 1e6) * priceOut);
  let streamingIdx = $state<number | null>(null); // index of the bubble being streamed
  const fmtTok = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`);
  const fmtCost = (n: number) => (n >= 1 ? n.toFixed(2) : n.toFixed(4));
  const currencySymbol = (currency: "CNY" | "USD") => currency === "USD" ? "$" : "¥";
  const currencyName = (currency: "CNY" | "USD") => currency === "USD" ? "美元" : "人民币";
  const priceSourceName = (source: CatalogModel["price_source"]) =>
    source === "official_direct" ? "厂商官方直连价" : "OpenRouter 聚合渠道价";
  function currentPriceSourceName() {
    if (!settings) return "";
    const current = modelCatalog?.providers
      .flatMap((provider) => provider.models)
      .find((model) => model.model_id === settings?.model && model.base_url === settings?.base_url);
    return current
      ? priceSourceName(current.price_source)
      : "手动设置的价格，程序无法验证其是否为厂商官方价";
  }

  // ── Quiet, grouped tool activity ─────────────────────────────────────────
  // Routine command lines stay out of the conversation. Every tool stays
  // collapsed by default, while its parameters and output remain available.
  const lineCount = (s: string = "") => (s ? s.split("\n").length : 0);
  // Classify a finished tool result so the outcome (报错 / 无输出 / N 行) is
  // visible at a glance — a bare "Exit code: 0" otherwise reads as "no info".
  const toolOutcome = (result: string = ""): "err" | "empty" | "ok" => {
    const exit = result.match(/Exit code: (-?\d+)/);
    const trimmed = result.trimStart();
    const body = result
      .replace(/\n?Exit code: -?\d+\s*$/, "")
      .replace(/^STDERR:\s*/, "")
      .trim();
    if (
      (exit && exit[1] !== "0") ||
      trimmed.startsWith("Error:") ||
      trimmed.startsWith("Sandbox denied:") ||
      trimmed.startsWith("[interrupted:")
    ) return "err";
    if (body === "") return "empty";
    return "ok";
  };
  const toolStatusLabel = (result: string = "") => {
    const oc = toolOutcome(result);
    return oc === "err" ? "报错" : oc === "empty" ? "无输出" : `${lineCount(result)} 行`;
  };
  function settleCompletedToolGroups() {
    for (const message of messages) {
      if (
        message.role === "tool_group" &&
        !message.settled &&
        message.tools.length > 0 &&
        message.tools.every((tool) => tool.result !== undefined)
      ) {
        message.settled = true;
      }
    }
  }
  function settleReasoning() {
    if (reasoningIdx === null) return;
    const message = messages[reasoningIdx];
    if (message?.role === "reasoning") message.settled = true;
    reasoningIdx = null;
  }
  function removeReasoningMessages() {
    messages = messages.filter((message) => message.role !== "reasoning");
    reasoningIdx = null;
  }
  function keepConversationConclusions(finalText: string) {
    const compacted: Msg[] = [];
    let pendingAnswer: Extract<Msg, { role: "assistant" }> | null = null;
    for (const message of messages) {
      if (message.role === "user") {
        if (pendingAnswer) compacted.push(pendingAnswer);
        compacted.push({ ...message });
        pendingAnswer = null;
      } else if (message.role === "assistant") {
        // Intermediate narrations are replaced until the next user turn.
        pendingAnswer = { ...message };
      }
    }
    if (finalText.trim() !== "") pendingAnswer = { role: "assistant", text: finalText };
    if (pendingAnswer) compacted.push(pendingAnswer);
    messages = compacted;
  }
  function appendReasoning(previous: string, delta: string): string {
    const combined = previous + delta;
    if (combined.length <= REASONING_DISPLAY_MAX_CHARS) return combined;
    const tailLength = REASONING_DISPLAY_MAX_CHARS - REASONING_OMITTED.length;
    return REASONING_OMITTED + combined.slice(-tailLength);
  }
  function hideCompletedToolActivity(source: Msg[]): Msg[] {
    return source.filter((message) => message.role !== "tool_group");
  }
  function toolGroupFailureCount(group: ToolGroup) {
    return group.tools.filter(
      (tool) => tool.result !== undefined && toolOutcome(tool.result) === "err",
    ).length;
  }
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
      if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) {
        closeLists();
        // deepseek over-uses '---' as section separators; collapse consecutive /
        // leading rules so they don't render as a stack of empty "striped" lines.
        if (out.length && out[out.length - 1] !== "<hr/>") out.push("<hr/>");
        i++; continue;
      }
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
    while (out.length && out[out.length - 1] === "<hr/>") out.pop(); // drop trailing rules
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
      try {
        const updated = await invoke<Settings>("get_settings");
        models = updated.available_models;
        priceIn = updated.price_in || 0;
        priceOut = updated.price_out || 0;
        priceCurrency = updated.price_currency || "CNY";
      } catch { /* 模型已切换，费用显示将在下一次状态刷新时同步 */ }
    } catch (e) {
      currentModel = prev;
      messages.push({ role: "note", text: `切换模型失败：${e}` });
    }
  }
  let reasoningEffort = $state("auto");
  let reasoningMenuOpen = $state(false);
  const REASONING_EFFORTS = [
    { id: "auto", label: "智能体自动", desc: "普通请求用高强度，复杂智能体任务自动增强" },
    { id: "off", label: "关闭思考", desc: "直接回答，不启用思考模式" },
    { id: "high", label: "深度思考", desc: "启用 DeepSeek high 思考强度" },
    { id: "max", label: "智能体增强", desc: "启用 DeepSeek max，适合复杂工具任务" },
  ];
  const reasoningLabel = (id: string) => REASONING_EFFORTS.find((option) => option.id === id)?.label ?? id;
  async function selectReasoningEffort(id: string) {
    reasoningMenuOpen = false;
    if (!id || id === reasoningEffort) return;
    const previous = reasoningEffort;
    reasoningEffort = id;
    try {
      await invoke("save_settings", { updates: { reasoning_effort: id } });
    } catch (e) {
      reasoningEffort = previous;
      messages.push({ role: "note", text: `切换思考程度失败：${e}` });
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
  let scroller = $state<HTMLDivElement>();

  function clampSidebarWidth(width: number): number {
    const viewportMax = typeof window === "undefined"
      ? SIDEBAR_MAX_WIDTH
      : Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, Math.floor(window.innerWidth * 0.45)));
    return Math.min(viewportMax, Math.max(SIDEBAR_MIN_WIDTH, Math.round(width)));
  }

  function setSidebarWidth(width: number, persist = true) {
    sidebarWidth = clampSidebarWidth(width);
    if (persist) {
      try { localStorage.setItem("ncx.sidebarWidth", String(sidebarWidth)); } catch { /* storage is optional */ }
    }
  }

  function stopSidebarResize() {
    if (!sidebarResizing) return;
    sidebarResizing = false;
    window.removeEventListener("pointermove", resizeSidebar);
    window.removeEventListener("pointerup", stopSidebarResize);
    document.body.classList.remove("sidebar-resizing");
  }

  function resizeSidebar(event: PointerEvent) {
    if (!sidebarResizing) return;
    setSidebarWidth(event.clientX - sidebarResizeStartX + sidebarResizeStartWidth);
  }

  let sidebarResizeStartX = 0;
  let sidebarResizeStartWidth = SIDEBAR_DEFAULT_WIDTH;
  function beginSidebarResize(event: PointerEvent) {
    if (!sidebarOpen) return;
    event.preventDefault();
    sidebarResizing = true;
    sidebarResizeStartX = event.clientX;
    sidebarResizeStartWidth = sidebarWidth;
    document.body.classList.add("sidebar-resizing");
    window.addEventListener("pointermove", resizeSidebar);
    window.addEventListener("pointerup", stopSidebarResize, { once: true });
  }

  function handleSidebarResizeKey(event: KeyboardEvent) {
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault();
      setSidebarWidth(sidebarWidth + (event.key === "ArrowRight" ? 16 : -16));
    } else if (event.key === "Home") {
      event.preventDefault();
      setSidebarWidth(SIDEBAR_MIN_WIDTH);
    } else if (event.key === "End") {
      event.preventDefault();
      setSidebarWidth(SIDEBAR_MAX_WIDTH);
    }
  }

  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  function acceptsSessionEvent(sessionId: string) {
    return sessionId === "" || (currentSessionId !== "" && sessionId === currentSessionId);
  }

  onMount(async () => {
    try {
      const savedWidth = Number(localStorage.getItem("ncx.sidebarWidth"));
      if (Number.isFinite(savedWidth)) setSidebarWidth(savedWidth, false);
    } catch { /* storage is optional */ }
    // Header falls back to a direct status call until the agent thread is Ready.
    try {
      const s = await invoke<{ model: string; sandbox: string; approval: string; permission_mode: string; reasoning_effort: string; price_in: number; price_out: number; price_currency: "CNY" | "USD" }>("get_status");
      header = `${s.model} · ${s.sandbox}`;
      sandboxMode = s.sandbox;
      currentModel = s.model;
      if (s.permission_mode) permissionMode = s.permission_mode;
      if (s.reasoning_effort) reasoningEffort = s.reasoning_effort;
      priceIn = s.price_in || 0;
      priceOut = s.price_out || 0;
      priceCurrency = s.price_currency || "CNY";
    } catch (e) {
      header = "配置错误";
    }
    refreshSessions();

    await listen<ProtocolEventEnvelope>("ncx://protocol-event", (message) => {
      const envelope = message.payload;
      if (envelope.protocolVersion !== 2 || !envelope.threadId) return;
      const previous = protocolSequences.get(envelope.threadId) || 0;
      if (envelope.sequence <= previous) return;
      protocolSequences.set(envelope.threadId, envelope.sequence);
      if (["threadCreated", "threadUpdated", "turnCompleted"].includes(envelope.event.type)) {
        refreshSessions();
      }
    });

    await listen<UiEvent>("ncx://event", (ev) => {
      const p = ev.payload;
      switch (p.kind) {
        case "ready":
          if (currentSessionId !== "" && p.session_id !== currentSessionId) break;
          header = `${p.model} · ${p.sandbox}`;
          workspace = p.workspace;
          needsWorkspace = p.needs_workspace;
          sandboxMode = p.sandbox;
          currentModel = p.model;
          if (p.models?.length) models = p.models;
          if (p.permission_mode) permissionMode = p.permission_mode;
          if (p.reasoning_effort) reasoningEffort = p.reasoning_effort;
          // Learn the active session's real id so 最近会话 can mark/return to it.
          if (p.session_id) {
            const wasUnbound = currentSessionId === "";
            currentSessionId = p.session_id;
            restoreSessionUsage(currentSessionId);
            if (wasUnbound) {
              restoreSessionQueue(currentSessionId);
              restoreSessionPrompts(currentSessionId);
              restoreSessionMessages(currentSessionId);
            }
          }
          refreshSessions();
          break;
        case "assistant_delta":
          if (!acceptsSessionEvent(p.session_id)) break;
          settleReasoning();
          if (streamingIdx === null) {
            if (p.text === "") break; // ignore an empty leading delta (no bubble yet)
            settleCompletedToolGroups();
            messages.push({ role: "assistant", text: p.text });
            streamingIdx = messages.length - 1;
          } else {
            const m = messages[streamingIdx];
            if (m && m.role === "assistant") m.text += p.text;
          }
          break;
        case "reasoning_delta":
          if (!acceptsSessionEvent(p.session_id) || p.text === "") break;
          if (reasoningIdx === null) {
            settleCompletedToolGroups();
            messages.push({ role: "reasoning", text: appendReasoning("", p.text), settled: false });
            reasoningIdx = messages.length - 1;
          } else {
            const m = messages[reasoningIdx];
            if (m?.role === "reasoning") m.text = appendReasoning(m.text, p.text);
          }
          break;
        case "context_compacted":
          if (!acceptsSessionEvent(p.session_id)) break;
          messages.push({
            role: "compact",
            text: `已自动压缩上下文：${p.original_chars.toLocaleString()} → ${p.edited_chars.toLocaleString()} 字符，清理 ${p.dropped_messages} 条旧消息和 ${p.compressed_tool_results} 条工具结果；关键要求、完成结果和当前计划已保留。`,
          });
          break;
        case "assistant":
          if (!acceptsSessionEvent(p.session_id)) break;
          settleReasoning();
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
            settleCompletedToolGroups();
            messages.push({ role: "assistant", text: p.text });
          }
          break;
        case "tool_start":
          if (!acceptsSessionEvent(p.session_id)) break;
          settleReasoning();
          // A streamed bubble that turned out empty (tool-only turn) leaves no box.
          if (streamingIdx !== null) {
            const m = messages[streamingIdx];
            if (m && m.role === "assistant" && m.text.trim() === "") messages.splice(streamingIdx, 1);
          }
          streamingIdx = null;
          {
            const last = messages.at(-1);
            const entry: ToolEntry = { name: p.name, args: p.args };
            if (last?.role === "tool_group") last.tools.push(entry);
            else messages.push({ role: "tool_group", tools: [entry], settled: false });
          }
          break;
        case "approval":
          {
            const item: Approval = { session_id: p.session_id, id: p.id, command: p.command, reason: p.reason, cwd: p.cwd, details: p.details };
            approvalsBySession.set(p.session_id, item);
            if (acceptsSessionEvent(p.session_id)) approval = item;
          }
          break;
        case "question":
          {
            const item: UserQuestion = { session_id: p.session_id, id: p.id, question: p.question, options: p.options, allow_free_text: p.allow_free_text };
            questionsBySession.set(p.session_id, item);
            if (acceptsSessionEvent(p.session_id)) {
              userQuestion = item;
              questionAnswer = "";
            }
          }
          break;
        case "tool_result": {
          if (!acceptsSessionEvent(p.session_id)) break;
          // Results preserve dispatch order, so pair with the earliest pending
          // call across the compact groups. Failures stay visible and open.
          let pendingGroup: ToolGroup | undefined;
          let pendingTool: ToolEntry | undefined;
          for (const message of messages) {
            if (message.role !== "tool_group") continue;
            const candidate = message.tools.find(
              (tool) => tool.name === p.name && tool.result === undefined,
            );
            if (candidate) {
              pendingGroup = message;
              pendingTool = candidate;
              break;
            }
          }
          if (pendingTool && pendingGroup) pendingTool.result = p.result;
          else {
            pendingGroup = {
              role: "tool_group",
              tools: [{ name: p.name, result: p.result }],
              settled: false,
            };
            messages.push(pendingGroup);
          }
          break;
        }
        case "done":
          setSessionRunning(p.session_id, false);
          addSessionUsage(p.session_id, p.usage || {});
          if (!acceptsSessionEvent(p.session_id)) {
            refreshSessions();
            break;
          }
          settleCompletedToolGroups();
          settleReasoning();
          removeReasoningMessages();
          messages = hideCompletedToolActivity(messages);
          if (p.stop_reason === "completed") {
            keepConversationConclusions(p.final_text);
          } else if (p.stop_reason !== "completed") {
            messages.push({ role: "note", text: `[${p.stop_reason}] ${p.final_text}` });
          }
          streamingIdx = null;
          // Keep the completed conclusion in the per-session cache; only the
          // transient reasoning cards are removed from the visible transcript.
          sessionMessages.set(p.session_id, cloneMessages(messages));
          busy = runningSessions.has(currentSessionId);
          stopping = false;
          refreshSessions();
          if (!switchingSession) dequeue();
          break;
        case "session_title":
          refreshSessions();
          if (!acceptsSessionEvent(p.session_id)) break;
          sessionTitle = p.title;
          break;
        case "loaded":
          if (!acceptsSessionEvent(p.session_id)) break;
          {
          const restored = p.messages
            .flatMap((m): Msg[] => {
              if ((m.role === "user" || m.role === "assistant") && m.text.trim() !== "") {
                return [{ role: m.role, text: m.text }];
              }
              if (m.role === "tool_group" && m.tools?.length) {
                return [{ role: "tool_group", tools: m.tools, settled: true }];
              }
              if (m.role === "note" && m.text.trim() !== "") {
                return [{ role: "note", text: m.text }];
              }
              if (m.role === "compact" && m.text.trim() !== "") {
                return [{ role: "compact", text: m.text }];
              }
              return [];
            });
          const cached = sessionMessages.get(p.session_id);
          messages = runningSessions.has(p.session_id) && cached?.length
            ? cloneMessages(cached)
            : hideCompletedToolActivity(restored);
          sessionMessages.set(p.session_id, cloneMessages(messages));
          }
          streamingIdx = null;
          reasoningIdx = null;
          busy = runningSessions.has(p.session_id);
          stopping = false;
          switchingSession = false;
          refreshSessions(); // keep the session you just left visible in 最近会话
          if (!busy) dequeue();
          break;
        case "error":
          setSessionRunning(p.session_id, false);
          if (!acceptsSessionEvent(p.session_id)) break;
          settleCompletedToolGroups();
          settleReasoning();
          removeReasoningMessages();
          messages = hideCompletedToolActivity(messages);
          streamingIdx = null;
          messages.push({ role: "note", text: `错误：${p.message}` });
          sessionMessages.set(p.session_id, cloneMessages(messages));
          busy = false;
          stopping = false;
          switchingSession = false;
          break;
      }
      scrollDown();
    });
    // The agent thread's initial `ready` can fire before this listener exists
    // (Tauri events aren't buffered), so the active session id would be missed.
    // Now that we're listening, ask the backend to re-emit it.
    invoke("request_ready").catch(() => {});
    loadCustomCommands();
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
    const previousSessionId = currentSessionId;
    const previousTitle = sessionTitle;
    const previousMessages = [...messages];
    try {
      const dir = await open({ directory: true, multiple: false });
      if (!dir || Array.isArray(dir)) return;
      // Reject every event from the old project as soon as the switch starts.
      currentSessionId = "";
      messages = [];
      sessionTitle = "新会话";
      resetSessionUsage();
      queued = [];
      attached = [];
      const set = await invoke<string>("set_workspace", { path: dir });
      workspace = set;
      // Switching project starts a fresh conversation — the old one belongs to
      // the old workspace, and set_workspace already reloaded the agent into a
      // new session. Reset the conversation-scoped UI state to match.
      messages.push({ role: "note", text: `已切换工作区到 ${set}，已开始新会话。` });
      refreshSessions();
    } catch (e) {
      currentSessionId = previousSessionId;
      sessionTitle = previousTitle;
      messages = previousMessages;
      restoreSessionUsage(currentSessionId);
      messages.push({ role: "note", text: `切换工作区失败：${e}` });
    }
  }

  async function dispatch(text: string, images: string[], shown: string) {
    const targetSessionId = currentSessionId;
    messages.push({ role: "user", text: shown });
    setSessionRunning(targetSessionId, true);
    busy = true;
    scrollDown();
    try {
      await appServerRequest({
        method: "turnSubmit",
        params: { threadId: targetSessionId, text, images },
      });
    } catch (e) {
      setSessionRunning(targetSessionId, false);
      messages.push({ role: "note", text: `发送失败：${e}` });
      busy = false;
      stopping = false;
      dequeue();
    }
  }

  async function stopGeneration() {
    if (!busy) return;
    stopping = true;
    // A stop applies to the active turn, so do not start queued follow-ups
    // after its cancellation event arrives.
    queued = [];
    approvalsBySession.delete(currentSessionId);
    questionsBySession.delete(currentSessionId);
    approval = null;
    userQuestion = null;
    try {
      await appServerRequest({
        method: "turnInterruptLatest",
        params: { threadId: currentSessionId },
      });
    } catch (e) {
      stopping = false;
      messages.push({ role: "note", text: `停止失败：${e}` });
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
    if (needsWorkspace) {
      messages.push({ role: "note", text: "请先选择项目目录（左下角「工作区」或下方按钮），再开始对话。" });
      return;
    }
    // Images route through the vision pipeline; other files become @mentions.
    const images = attached.filter(isImage);
    const files = attached.filter((p) => !isImage(p));
    // File-picker paths can contain spaces. Quoted mentions keep each absolute
    // path as one token for the backend's attachment expander.
    const mentions = files.map((p) => `@"${p}"`).join(" ");
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
    approvalsBySession.delete(approval.session_id);
    approval = null;
    try {
      await invoke("approve", { id, decision });
    } catch (e) {
      messages.push({ role: "note", text: `审批失败：${e}` });
    }
  }

  async function answerUserQuestion(answer: string | null) {
    if (!userQuestion) return;
    const id = userQuestion.id;
    questionsBySession.delete(userQuestion.session_id);
    userQuestion = null;
    questionAnswer = "";
    try {
      await invoke("answer_question", { id, answer });
    } catch (e) {
      messages.push({ role: "note", text: `回答问题失败：${e}` });
    }
  }

  async function openSettings() {
    try {
      const [
        loadedSettings,
        loadedLocation,
        loadedCatalog,
        diagnostics,
        plugins,
        loadedCodexPlugins,
        loadedMarketplaces,
      ] = await Promise.all([
        invoke<Settings>("get_settings"),
        invoke<ConfigLocation>("get_config_location"),
        invoke<ModelCatalogResponse>("get_model_catalog"),
        invoke<HarnessDiagnostics>("get_harness_diagnostics"),
        invoke<ExternalPlugin[]>("list_external_plugins"),
        appServerRequest<CodexPlugin[]>({ method: "codexPluginList" }),
        appServerRequest<PluginMarketplace[]>({ method: "marketplaceList" }),
      ]);
      settings = loadedSettings;
      configLocation = loadedLocation;
      modelCatalog = loadedCatalog;
      harnessDiagnostics = diagnostics;
      externalPlugins = plugins;
      codexPlugins = loadedCodexPlugins;
      pluginMarketplaces = loadedMarketplaces;
      apiKeyInput = "";
      vlApiKeyInput = "";
    } catch (e) {
      messages.push({ role: "note", text: `设置加载失败：${e}` });
    }
  }

  async function addExternalPlugin() {
    const selected = await open({ directory: true, multiple: false, title: "选择包含 plugin.toml 的插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await invoke("install_external_plugin", { source: selected, upgrade: false });
      externalPlugins = await invoke<ExternalPlugin[]>("list_external_plugins");
    } catch (e) { messages.push({ role: "note", text: `插件安装失败：${e}` }); }
  }

  async function upgradeExternalPlugin() {
    const selected = await open({ directory: true, multiple: false, title: "选择更高版本的插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await invoke("install_external_plugin", { source: selected, upgrade: true });
      externalPlugins = await invoke<ExternalPlugin[]>("list_external_plugins");
    } catch (e) { messages.push({ role: "note", text: `插件升级失败：${e}` }); }
  }

  async function toggleExternalPlugin(plugin: ExternalPlugin) {
    try {
      await invoke("set_external_plugin_enabled", { id: plugin.manifest.id, enabled: !plugin.enabled });
      externalPlugins = await invoke<ExternalPlugin[]>("list_external_plugins");
    } catch (e) { messages.push({ role: "note", text: `插件状态修改失败：${e}` }); }
  }

  async function addCodexPlugin() {
    const selected = await open({ directory: true, multiple: false, title: "选择包含 .codex-plugin/plugin.json 的插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await appServerRequest({ method: "codexPluginInstall", params: { source: selected, upgrade: false } });
      codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
    } catch (e) { messages.push({ role: "note", text: `Codex 插件安装失败：${e}` }); }
  }

  async function upgradeCodexPlugin() {
    const selected = await open({ directory: true, multiple: false, title: "选择新版 Codex 插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await appServerRequest({ method: "codexPluginInstall", params: { source: selected, upgrade: true } });
      codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
    } catch (e) { messages.push({ role: "note", text: `Codex 插件升级失败：${e}` }); }
  }

  async function toggleCodexPlugin(plugin: CodexPlugin) {
    try {
      await appServerRequest({ method: "codexPluginSetEnabled", params: { name: plugin.manifest.name, enabled: !plugin.enabled } });
      codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
    } catch (e) { messages.push({ role: "note", text: `Codex 插件状态修改失败：${e}` }); }
  }

  async function removeCodexPlugin(plugin: CodexPlugin) {
    try {
      await appServerRequest({ method: "codexPluginUninstall", params: { name: plugin.manifest.name } });
      codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
    } catch (e) { messages.push({ role: "note", text: `Codex 插件卸载失败：${e}` }); }
  }

  async function installMarketplacePlugin(marketplacePath: string, pluginName: string, upgrade = false) {
    try {
      await appServerRequest({ method: "marketplacePluginInstall", params: { marketplacePath, pluginName, upgrade } });
      codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
    } catch (e) { messages.push({ role: "note", text: `Marketplace 插件${upgrade ? "升级" : "安装"}失败：${e}` }); }
  }
  function stashSessionQueue(sessionId: string) {
    if (sessionId) {
      sessionQueues.set(sessionId, [...queued]);
      sessionMessages.set(sessionId, cloneMessages(messages));
    }
    queued = [];
    approval = null;
    userQuestion = null;
  }
  function restoreSessionQueue(sessionId: string) {
    queued = [...(sessionQueues.get(sessionId) || [])];
  }
  function restoreSessionPrompts(sessionId: string) {
    approval = approvalsBySession.get(sessionId) || null;
    userQuestion = questionsBySession.get(sessionId) || null;
    questionAnswer = "";
  }
  function cloneMessages(items: Msg[]): Msg[] {
    return items.map((item) => item.role === "tool_group"
      ? { ...item, tools: item.tools.map((tool) => ({ ...tool })) }
      : { ...item });
  }
  function restoreSessionMessages(sessionId: string) {
    messages = cloneMessages(sessionMessages.get(sessionId) || []);
  }

  async function refreshOpenRouterModels() {
    catalogRefreshing = true;
    try {
      modelCatalog = await invoke<ModelCatalogResponse>("refresh_openrouter_models");
    } catch (e) {
      messages.push({ role: "note", text: `OpenRouter 模型目录刷新失败：${e}` });
    }
    catalogRefreshing = false;
  }

  async function applyModelPreset(provider: CatalogProvider, model: CatalogModel) {
    if (!settings) return;
    presetSaving = `${provider.id}/${model.model_id}`;
    try {
      const selected = await invoke<CatalogModel>("apply_model_preset", {
        providerId: provider.id,
        modelId: model.model_id,
      });
      settings.model = selected.model_id;
      settings.base_url = selected.base_url;
      settings.price_in = selected.price_in;
      settings.price_out = selected.price_out;
      settings.price_currency = selected.price_currency;
      settings.available_models = provider.models.map((item) => item.model_id);
      currentModel = selected.model_id;
      models = settings.available_models;
      priceIn = selected.price_in;
      priceOut = selected.price_out;
      priceCurrency = selected.price_currency;
    } catch (e) {
      messages.push({ role: "note", text: `应用模型预设失败：${e}` });
    }
    presetSaving = "";
  }

  function openPriceSource(url: string) {
    invoke("open_url", { url }).catch((e) =>
      messages.push({ role: "note", text: `打开价格来源失败：${e}` }),
    );
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
      vl_model: settings.vl_model,
      vl_base_url: settings.vl_base_url,
      reasoning_effort: settings.reasoning_effort,
      max_iterations: String(settings.max_iterations),
      max_tool_calls: String(settings.max_tool_calls),
      context_edit_enabled: String(settings.context_edit_enabled),
      context_edit_max_chars: String(settings.context_edit_max_chars),
      context_edit_keep_recent_messages: String(settings.context_edit_keep_recent_messages),
      context_edit_max_tool_result_chars: String(settings.context_edit_max_tool_result_chars),
      price_in: String(settings.price_in),
      price_out: String(settings.price_out),
      price_currency: settings.price_currency,
    };
    if (apiKeyInput.trim()) updates.api_key = apiKeyInput.trim();
    if (vlApiKeyInput.trim()) updates.vl_api_key = vlApiKeyInput.trim();
    try {
      await invoke("save_settings", { updates });
      priceIn = Number(settings.price_in) || 0; // reflect new rate immediately
      priceOut = Number(settings.price_out) || 0;
      priceCurrency = settings.price_currency;
      settings = null;
      apiKeyInput = "";
      vlApiKeyInput = "";
    } catch (e) {
      messages.push({ role: "note", text: `保存设置失败：${e}` });
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
  type ProtocolThreadItem =
    | { type: "userMessage"; id: string; text: string }
    | { type: "assistantMessage"; id: string; text: string }
    | { type: "toolCall"; id: string; name: string; arguments: unknown }
    | { type: "toolResult"; id: string; callId: string; output: string; success: boolean }
    | { type: "reasoning"; id: string; summary: string }
    | { type: "contextCompaction"; id: string; summary: string; droppedItems: number };
  type ProtocolThread = {
    metadata: { id: string; workspace: string; title: string; archived: boolean; createdAt: number; updatedAt: number };
    turns: { id: string; status: string; items: ProtocolThreadItem[]; startedAt: number; completedAt?: number; usage?: { tokens?: Record<string, number>; estimatedCost?: number; currency?: string } }[];
  };
  type AppServerOutcome<T> = { response: { protocolVersion: number; payload: { type: string; data: T } } };
  type ProtocolEventEnvelope = {
    protocolVersion: number;
    sequence: number;
    threadId: string;
    turnId?: string;
    event: { type: string; data?: unknown };
  };
  const protocolSequences = new Map<string, number>();

  async function appServerRequest<T>(request: Record<string, unknown>): Promise<T> {
    const outcome = await invoke<AppServerOutcome<T>>("app_server_request", { request });
    if (outcome.response.protocolVersion !== 2) throw new Error(`不支持的协议版本 ${outcome.response.protocolVersion}`);
    return outcome.response.payload.data;
  }

  function threadToSessionRow(thread: ProtocolThread): SessionRow {
    let userMessages = 0;
    let assistantMessages = 0;
    let toolCalls = 0;
    let snippet = "";
    for (const item of thread.turns.flatMap((turn) => turn.items)) {
      if (item.type === "userMessage") { userMessages += 1; snippet = item.text; }
      else if (item.type === "assistantMessage") { assistantMessages += 1; snippet = item.text; }
      else if (item.type === "toolCall") toolCalls += 1;
    }
    return {
      session_id: thread.metadata.id,
      title: thread.metadata.title,
      snippet: Array.from(snippet).slice(0, 200).join(""),
      user_messages: userMessages,
      assistant_messages: assistantMessages,
      tool_calls: toolCalls,
      updated_at: String(thread.metadata.updatedAt),
      has_snapshot: thread.turns.length > 0,
      archived: thread.metadata.archived,
    };
  }
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
  let showRecent = $state(false);
  let showArchived = $state(false);
  // Keep ordinary and archived conversations as two stable sections. The active
  // conversation is pinned only inside its own section, never above the archive
  // boundary.
  const recentSessions = $derived.by(() => {
    const visible = sessions.filter((s) => !s.archived);
    if (!currentSessionId) return visible;
    return [
      ...visible.filter((s) => s.session_id === currentSessionId),
      ...visible.filter((s) => s.session_id !== currentSessionId),
    ];
  });
  const archivedSessions = $derived.by(() => {
    const archived = sessions.filter((s) => s.archived);
    if (!currentSessionId) return archived;
    return [
      ...archived.filter((s) => s.session_id === currentSessionId),
      ...archived.filter((s) => s.session_id !== currentSessionId),
    ];
  });
  const archivedCount = $derived(sessions.filter((s) => s.archived).length);

  // 13-digit ms-epoch string → compact relative / date label.
  function fmtWhen(ms: string): string {
    // Current stamps are 13-digit ms-epoch; legacy ones are ISO strings.
    const t = /^\d+$/.test(ms) ? Number(ms) : Date.parse(ms);
    if (!t || Number.isNaN(t)) return "";
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
      await appServerRequest({ method: "threadArchive", params: { threadId: id, archived } });
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
      const metadata = await appServerRequest<{ id: string }[]>({ method: "threadList", params: { includeArchived: true } });
      const threads = await Promise.all(metadata.slice(0, 50).map((thread) =>
        appServerRequest<ProtocolThread>({ method: "threadReadVisible", params: { threadId: thread.id } })
      ));
      protocolUsageBySession.clear();
      for (const thread of threads) {
        protocolUsageBySession.set(thread.metadata.id, thread.turns.reduce(
          (sum, turn) => ({
            prompt_tokens: sum.prompt_tokens + (turn.usage?.tokens?.prompt_tokens || 0),
            completion_tokens: sum.completion_tokens + (turn.usage?.tokens?.completion_tokens || 0),
          }),
          { prompt_tokens: 0, completion_tokens: 0 },
        ));
      }
      sessions = threads.map(threadToSessionRow);
      const current = sessions.find((session) => session.session_id === currentSessionId);
      if (current) {
        sessionTitle = current.title || "会话";
        restoreSessionUsage(currentSessionId);
      }
    } catch (e) {
      console.error("会话协议加载失败", e);
    }
  }
  function toggleSidebar() {
    sidebarOpen = !sidebarOpen;
  }
  function newThreadId(): string {
    return `thread-${crypto.randomUUID()}`;
  }
  async function newSession() {
    if (switchingSession) return;
    const previousSessionId = currentSessionId;
    const previousTitle = sessionTitle;
    const previousMessages = [...messages];
    stashSessionQueue(previousSessionId);
    switchingSession = true;
    busy = false;
    messages = [];
    sessionTitle = "新会话";
    currentSessionId = "";
    resetSessionUsage();
    try {
      const id = newThreadId();
      await appServerRequest<ProtocolThread>({
        method: "threadCreateActivate",
        params: { threadId: id, workspace, title: "(no prompt yet)" },
      });
      if (currentSessionId === "") {
        currentSessionId = id;
        restoreSessionQueue(id);
        restoreSessionPrompts(id);
        restoreSessionMessages(id);
      }
    } catch (e) {
      busy = false;
      stopping = false;
      switchingSession = false;
      currentSessionId = previousSessionId;
      sessionTitle = previousTitle;
      messages = previousMessages;
      restoreSessionQueue(previousSessionId);
      restoreSessionPrompts(previousSessionId);
      restoreSessionUsage(currentSessionId);
      messages.push({ role: "note", text: `新建会话失败：${e}` });
    }
  }
  async function resumeSession(id: string, title = "") {
    if (switchingSession || id === currentSessionId) return;
    const previousId = currentSessionId;
    const previousTitle = sessionTitle;
    stashSessionQueue(previousId);
    switchingSession = true;
    sessionTitle = title || "会话";
    currentSessionId = id;
    restoreSessionUsage(currentSessionId);
    restoreSessionQueue(id);
    restoreSessionPrompts(id);
    restoreSessionMessages(id);
    try {
      busy = runningSessions.has(id);
      await appServerRequest({ method: "threadActivate", params: { threadId: id } });
    } catch (e) {
      busy = false;
      stopping = false;
      switchingSession = false;
      currentSessionId = previousId;
      sessionTitle = previousTitle;
      restoreSessionUsage(currentSessionId);
      restoreSessionQueue(previousId);
      restoreSessionPrompts(previousId);
      messages.push({ role: "note", text: `继续会话失败：${e}` });
    }
  }
  async function forkSession(id: string, title = "") {
    if (switchingSession) return;
    const previousSessionId = currentSessionId;
    const previousTitle = sessionTitle;
    stashSessionQueue(previousSessionId);
    switchingSession = true;
    busy = false;
    sessionTitle = title ? `${title}（分叉）` : "分叉";
    currentSessionId = "";
    resetSessionUsage();
    try {
      await appServerRequest<ProtocolThread>({
        method: "threadForkActivate",
        params: { threadId: id, newThreadId: newThreadId() },
      });
    } catch (e) {
      busy = false;
      stopping = false;
      switchingSession = false;
      currentSessionId = previousSessionId;
      sessionTitle = previousTitle;
      restoreSessionUsage(currentSessionId);
      restoreSessionQueue(previousSessionId);
      restoreSessionPrompts(previousSessionId);
      messages.push({ role: "note", text: `分叉失败：${e}` });
    }
  }
  async function openSessionLog(id: string) {
    try {
      await invoke("open_session_log", { sessionId: id });
    } catch (e) {
      messages.push({ role: "note", text: `打开会话日志失败：${e}` });
    }
  }
  async function openSessionSnapshot(id: string) {
    try {
      await invoke("open_session_snapshot", { sessionId: id });
    } catch (e) {
      messages.push({ role: "note", text: `打开会话快照失败：${e}` });
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
  async function openMemoryFile() {
    try {
      await invoke("open_memory_file");
    } catch (e) {
      messages.push({ role: "note", text: `打开记忆文件失败：${e}` });
    }
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
    const c = priceIn || priceOut ? ` · ≈${currencySymbol(priceCurrency)}${fmtCost(cost)}` : "";
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
  async function cmdRename() {
    if (!currentSessionId) return;
    const title = window.prompt("输入新的会话名称", sessionTitle)?.trim();
    if (!title || title === sessionTitle) return;
    try {
      await appServerRequest({
        method: "threadRename",
        params: { threadId: currentSessionId, title },
      });
      sessionTitle = title;
      await refreshSessions();
    } catch (e) {
      messages.push({ role: "note", text: `重命名失败：${e}` });
    }
  }
  // User-authored custom commands (.nanocodex|.claude/commands/*.md), surfaced
  // in the palette alongside built-in actions.
  let customCommands = $state<{ scope: string; name: string; slash: string; path: string }[]>([]);
  let slashArg = ""; // trailing args captured when a palette item is chosen
  async function loadCustomCommands() {
    try {
      customCommands = await invoke("get_custom_commands");
    } catch (e) {
      /* custom commands are optional */
    }
  }
  async function runCustom(slash: string) {
    try {
      const expanded = await invoke<string>("expand_custom_command", { slash, arg: slashArg });
      input = expanded; // fill the composer; user reviews, then Enter to send
    } catch (e) {
      messages.push({ role: "note", text: `自定义命令展开失败：${e}` });
    }
  }
  type SlashCmd = { id: string; label: string; desc: string; run: () => void };
  const SLASH_COMMANDS: SlashCmd[] = [
    { id: "new", label: "新建会话", desc: "开始一个空会话", run: () => newSession() },
    { id: "fork", label: "分叉会话", desc: "从当前会话分叉一个新会话", run: () => forkCurrent() },
    { id: "rename", label: "重命名会话", desc: "给当前会话改名", run: () => cmdRename() },
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
  const slashHead = $derived(slashFilter.split(/\s+/)[0] ?? "");
  const customSlash = $derived(
    customCommands.map((c) => ({
      id: c.slash.slice(1),
      label: c.name,
      desc: `自定义命令 · ${c.scope}`,
      run: () => runCustom(c.slash),
    })),
  );
  const slashMatches = $derived(
    showSlash
      ? [
          ...SLASH_COMMANDS.filter(
            (c) => c.id.includes(slashFilter) || c.label.toLowerCase().includes(slashFilter),
          ),
          ...customSlash.filter(
            (c) => c.id.includes(slashHead) || c.label.toLowerCase().includes(slashHead),
          ),
        ]
      : [],
  );
  function runSlash(c: SlashCmd) {
    slashArg = input.replace(/^\/\S+\s*/, "");
    input = "";
    c.run();
  }
</script>

<main class="app" style={`--sidebar-width: ${sidebarWidth}px`}>
  <SessionSidebar
    {sidebarOpen}
    {switchingSession}
    {currentSessionId}
    {runningSessions}
    {recentSessions}
    {archivedSessions}
    {archivedCount}
    bind:showRecent
    bind:showArchived
    {workspace}
    {sidebarResizing}
    {sidebarWidth}
    {SIDEBAR_MIN_WIDTH}
    {SIDEBAR_MAX_WIDTH}
    {SIDEBAR_DEFAULT_WIDTH}
    {toggleSidebar}
    {newSession}
    {resumeSession}
    {forkSession}
    {openSessionLog}
    {archiveSession}
    {chooseWorkspace}
    {openSettings}
    {fmtWhen}
    {baseName}
    {beginSidebarResize}
    {setSidebarWidth}
    {handleSidebarResizeKey}
  />
  <div class="workarea">
  <section class="main">
    <TopBar
      {sidebarOpen}
      {sessionTitle}
      {busy}
      {rightPanel}
      {toggleSidebar}
      {openFiles}
      {openDiff}
      {openBranches}
      {openHermes}
      {openCheckpoints}
    />

    <ConversationView
      bind:scroller
      {messages}
      {busy}
      {streamingIdx}
      {reasoningIdx}
      {renderMarkdown}
      {toolGroupFailureCount}
      {toolOutcome}
    />

    <Composer
      {models}
      {currentModel}
      {header}
      bind:modelMenuOpen
      {selectModel}
      {reasoningEffort}
      bind:reasoningMenuOpen
      reasoningEfforts={REASONING_EFFORTS}
      {selectReasoningEffort}
      {reasoningLabel}
      {permissionMode}
      bind:modeMenuOpen
      permissionModes={PERMISSION_MODES}
      {selectMode}
      {modeIcon}
      {modeLabel}
      {busy}
      {workspace}
      {needsWorkspace}
      {wsName}
      {chooseWorkspace}
      {tokIn}
      {tokOut}
      {priceIn}
      {priceOut}
      {priceCurrency}
      {cost}
      {fmtTok}
      {currencySymbol}
      {fmtCost}
      bind:queued
      {attached}
      {isImage}
      {baseName}
      {removeAttachment}
      {showSlash}
      {slashMatches}
      bind:slashIdx
      {runSlash}
      bind:input
      {onKey}
      {handlePaste}
      {attachFiles}
      {stopping}
      {stopGeneration}
      {send}
    />
  </section>

  <InteractionDialogs
    {approval}
    {userQuestion}
    bind:questionAnswer
    {decide}
    {answerUserQuestion}
  />


  <WorkspacePanels
    bind:rightPanel
    {reloadPanel}
    bind:checkpointLabel
    {checkpointBusy}
    {checkpoints}
    {checkpointFiles}
    {saveCheckpoint}
    {loadCheckpoints}
    {toggleCheckpointDetail}
    {restoreCheckpoint}
    {busy}
    bind:newBranch
    {branchBusy}
    {branches}
    {branchCommits}
    {createBranch}
    {loadBranches}
    {toggleBranchDetail}
    {switchBranch}
    {chooseWorkspace}
    bind:filePreview
    {filesPath}
    {filesEntries}
    {insertMention}
    {filesUp}
    {pickFile}
    {diffFiles}
    {diffOpenFiles}
    {toggleFile}
    {diffLineClass}
    bind:historyOpen
    {sessions}
    {switchingSession}
    {resumeSession}
    {forkSession}
    {openSessionLog}
    {openSessionSnapshot}
    {refreshSessions}
    bind:newNote
    bind:newNoteTags
    {hermesBusy}
    {notes}
    {addNote}
    {consolidateMemory}
    {loadNotes}
    {openMemoryFile}
    {fmtTs}
  />


  <SettingsModal
    bind:settings
    bind:apiKeyInput
    bind:vlApiKeyInput
    {configLocation}
    {modelCatalog}
    {officialProviders}
    {openRouterProvider}
    {catalogRefreshing}
    {presetSaving}
    {saving}
    {harnessDiagnostics}
    {externalPlugins}
    {codexPlugins}
    {pluginMarketplaces}
    {REASONING_EFFORTS}
    {currencySymbol}
    {currencyName}
    {priceSourceName}
    {currentPriceSourceName}
    {openConfigFile}
    {openConfigDir}
    {applyModelPreset}
    {openPriceSource}
    {refreshOpenRouterModels}
    {addExternalPlugin}
    {upgradeExternalPlugin}
    {toggleExternalPlugin}
    {addCodexPlugin}
    {upgradeCodexPlugin}
    {toggleCodexPlugin}
    {removeCodexPlugin}
    {installMarketplacePlugin}
    {saveSettings}
  />
  </div>
</main>
