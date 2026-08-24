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
  import { appServerRequest, ProtocolSequenceGate, threadToSessionRow, type ProtocolEventEnvelope, type ProtocolThread, type SessionRow } from "./lib/app-server-client";
  import { diffLineClass, renderMarkdown, toolOutcome, toolStatusLabel } from "./lib/ui-format";
  import { SidebarController, SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from "./lib/sidebar-controller.svelte";
  import { appendReasoning, hideCompletedToolActivity, keepConversationConclusions, settleCompletedToolGroups, toolGroupFailureCount, type ConversationMessage as Msg, type ToolEntry, type ToolGroup } from "./lib/conversation-model";
  import { UsageController } from "./lib/usage-controller.svelte";
  import { FileBrowserController } from "./lib/file-browser-controller.svelte";
  import { GitWorkspaceController } from "./lib/git-workspace-controller.svelte";
  import { CheckpointController } from "./lib/checkpoint-controller.svelte";
  import { MemoryController } from "./lib/memory-controller.svelte";
  import { PluginController } from "./lib/plugin-controller.svelte";
  import { SettingsController, type Settings } from "./lib/settings-controller.svelte";
  import { SlashController } from "./lib/slash-controller.svelte";
  import { ModelControlsController, PERMISSION_MODES, REASONING_EFFORTS } from "./lib/model-controls-controller.svelte";
  import { ThreadController, type UiEvent } from "./lib/thread-controller.svelte";
  import { ThreadLifecycleController } from "./lib/thread-lifecycle-controller.svelte";

  const protocolSequenceGate = new ProtocolSequenceGate();
  const sidebar = new SidebarController();
  const usage = new UsageController();

  const IMAGE_EXTS = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
  const isImage = (p: string) => IMAGE_EXTS.includes((p.split(".").pop() || "").toLowerCase());
  const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

  // UiEvent and per-Thread state are owned by ThreadController.
  let input = $state("");
  const thread = new ThreadController(usage, {
    refreshSessions: () => void threadLifecycle.refresh(),
    scrollDown: () => scrollDown(),
    dequeue: () => dequeue(),
    ready: (event) => {
      header = `${event.model} · ${event.sandbox}`;
      workspace = event.workspace;
      needsWorkspace = event.needs_workspace;
      sandboxMode = event.sandbox;
      modelControls.currentModel = event.model;
      if (event.models?.length) modelControls.models = event.models;
      if (event.permission_mode) modelControls.permissionMode = event.permission_mode;
      if (event.reasoning_effort) modelControls.reasoningEffort = event.reasoning_effort;
    },
  });
  const threadLifecycle = new ThreadLifecycleController(thread, usage, () => workspace);
  const fileBrowser = new FileBrowserController(
    (text) => thread.messages.push({ role: "note", text }),
    () => input,
    (value) => (input = value),
  );
  const gitWorkspace = new GitWorkspaceController((text) => thread.messages.push({ role: "note", text }));
  const checkpointController = new CheckpointController(
    (text) => thread.messages.push({ role: "note", text }),
    () => thread.busy,
  );
  const memoryController = new MemoryController((text) => thread.messages.push({ role: "note", text }));
  const pluginController = new PluginController((text) => thread.messages.push({ role: "note", text }));
  const modelControls = new ModelControlsController(
    (text) => thread.messages.push({ role: "note", text }),
    (priceIn, priceOut, currency) => usage.setPrice(priceIn, priceOut, currency),
  );
  const settingsController = new SettingsController(
    pluginController,
    (text) => thread.messages.push({ role: "note", text }),
    modelControls.applyModel,
    (priceIn, priceOut, currency) => usage.setPrice(priceIn, priceOut, currency),
  );
  const slashController = new SlashController(
    () => input,
    (value) => (input = value),
    (text) => thread.messages.push({ role: "note", text }),
    {
      newSession: () => void threadLifecycle.create(),
      forkCurrent: () => thread.currentId ? void threadLifecycle.fork(thread.currentId, thread.title) : thread.messages.push({ role: "note", text: "当前会话还没有快照，无法分叉（先发一条消息）。" }),
      openModel: () => (modelControls.modelMenuOpen = true),
      openSettings: () => void openSettings(),
      showUsage: () => showUsage(),
      openCheckpoints: () => void openCheckpoints(), openFiles: () => void openFiles(), openDiff: () => void openDiff(),
      openBranches: () => void openBranches(), openMemory: () => void openHermes(),
      refreshSessions: threadLifecycle.refresh, currentThreadId: () => thread.currentId, currentTitle: () => thread.title,
      setTitle: (title) => (thread.title = title),
    },
  );
  let attached = $state<string[]>([]); // absolute file paths attached to the next turn
  // File explorer (workspace tree)
  let header = $state("连接中…");
  let workspace = $state("");
  let needsWorkspace = $state(false); // true when cwd is home/root — block prompts
  // Last path segment of the workspace, for the header pill (full path on hover).
  const wsName = $derived(
    workspace ? workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || workspace : "",
  );
  let sandboxMode = $state("");
  const fmtTok = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`);
  const fmtCost = (n: number) => (n >= 1 ? n.toFixed(2) : n.toFixed(4));
  const currencySymbol = (currency: "CNY" | "USD") => currency === "USD" ? "$" : "¥";
  const currencyName = (currency: "CNY" | "USD") => currency === "USD" ? "美元" : "人民币";
  const priceSourceName = (source: "official_direct" | "aggregator") =>
    source === "official_direct" ? "厂商官方直连价" : "OpenRouter 聚合渠道价";

  // Branch / checkpoint expand-to-detail.
  let rightPanel = $state(""); // "" | files | branches | diff | memory | checkpoints
  const PANEL_TITLES: Record<string, string> = {
    files: "文件", branches: "Git 分支", diff: "工作区改动", memory: "项目记忆", checkpoints: "检查点",
  };
  let scroller = $state<HTMLDivElement>();


  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(async () => {
    sidebar.restoreWidth();
    // Header falls back to a direct status call until the agent thread is Ready.
    try {
      const s = await invoke<{ model: string; sandbox: string; approval: string; permission_mode: string; reasoning_effort: string; price_in: number; price_out: number; price_currency: "CNY" | "USD" }>("get_status");
      header = `${s.model} · ${s.sandbox}`;
      sandboxMode = s.sandbox;
      modelControls.currentModel = s.model;
      if (s.permission_mode) modelControls.permissionMode = s.permission_mode;
      if (s.reasoning_effort) modelControls.reasoningEffort = s.reasoning_effort;
      usage.setPrice(s.price_in, s.price_out, s.price_currency);
    } catch (e) {
      header = "配置错误";
    }
    void threadLifecycle.refresh();

    await listen<ProtocolEventEnvelope>("ncx://protocol-event", (message) => {
      const envelope = message.payload;
      if (!protocolSequenceGate.accept(envelope)) return;
      if (["threadCreated", "threadUpdated", "turnCompleted"].includes(envelope.event.type)) {
        void threadLifecycle.refresh();
      }
    });

    await listen<UiEvent>("ncx://event", (event) => thread.handle(event.payload));
    // The agent thread's initial `ready` can fire before this listener exists
    // (Tauri events aren't buffered), so the active session id would be missed.
    // Now that we're listening, ask the backend to re-emit it.
    invoke("request_ready").catch(() => {});
    slashController.loadCustomCommands();
  });

  async function attachFiles() {
    try {
      const picked = await open({ multiple: true });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      for (const p of paths) if (!attached.includes(p)) attached.push(p);
    } catch (e) {
      thread.messages.push({ role: "note", text: `添加失败：${e}` });
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
          thread.messages.push({ role: "note", text: `粘贴图片失败：${err}` });
        }
      }
    }
  }

  async function openFiles() {
    if (rightPanel === "files") { rightPanel = ""; return; }
    rightPanel = "files";
    await fileBrowser.load("");
  }


  async function chooseWorkspace() {
    const previousSessionId = thread.currentId;
    const previousTitle = thread.title;
    const previousMessages = [...thread.messages];
    try {
      const dir = await open({ directory: true, multiple: false });
      if (!dir || Array.isArray(dir)) return;
      // Reject every event from the old project as soon as the switch starts.
      thread.currentId = "";
      thread.messages = [];
      thread.title = "新会话";
      usage.reset();
      thread.queued = [];
      attached = [];
      const set = await invoke<string>("set_workspace", { path: dir });
      workspace = set;
      // Switching project starts a fresh conversation — the old one belongs to
      // the old workspace, and set_workspace already reloaded the agent into a
      // new session. Reset the conversation-scoped UI state to match.
      thread.messages.push({ role: "note", text: `已切换工作区到 ${set}，已开始新会话。` });
      void threadLifecycle.refresh();
    } catch (e) {
      thread.currentId = previousSessionId;
      thread.title = previousTitle;
      thread.messages = previousMessages;
      usage.restore(thread.currentId);
      thread.messages.push({ role: "note", text: `切换工作区失败：${e}` });
    }
  }

  async function dispatch(text: string, images: string[], shown: string) {
    const targetSessionId = thread.currentId;
    thread.messages.push({ role: "user", text: shown });
    thread.setRunning(targetSessionId, true);
    thread.busy = true;
    scrollDown();
    try {
      await appServerRequest({
        method: "turnSubmit",
        params: { threadId: targetSessionId, text, images },
      });
    } catch (e) {
      thread.setRunning(targetSessionId, false);
      thread.messages.push({ role: "note", text: `发送失败：${e}` });
      thread.busy = false;
      thread.stopping = false;
      dequeue();
    }
  }

  async function stopGeneration() {
    if (!thread.busy) return;
    thread.stopping = true;
    // A stop applies to the active turn, so do not start queued follow-ups
    // after its cancellation event arrives.
    thread.queued = [];
    thread.clearPrompts(thread.currentId);
    try {
      await appServerRequest({
        method: "turnInterruptLatest",
        params: { threadId: thread.currentId },
      });
    } catch (e) {
      thread.stopping = false;
      thread.messages.push({ role: "note", text: `停止失败：${e}` });
    }
  }
  function dequeue() {
    if (!thread.busy && thread.queued.length > 0) {
      const next = thread.queued.shift();
      if (next) dispatch(next.text, next.images, next.shown);
    }
  }
  function send() {
    const text = input.trim();
    if (!text && attached.length === 0) return;
    if (needsWorkspace) {
      thread.messages.push({ role: "note", text: "请先选择项目目录（左下角「工作区」或下方按钮），再开始对话。" });
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
    if (thread.busy) {
      // Queue up to 2 follow-up turns while the agent works.
      if (thread.queued.length >= 2) {
        thread.messages.push({ role: "note", text: "队列已满（2 条），请先等当前任务完成。" });
        return;
      }
      thread.queued.push({ text: fullText, images: imgs, shown });
      return;
    }
    dispatch(fullText, imgs, shown);
  }

  function onKey(e: KeyboardEvent) {
    // Slash-command palette navigation takes precedence while it's open.
    if (slashController.visible && slashController.matches.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); slashController.index = (slashController.index + 1) % slashController.matches.length; return; }
      if (e.key === "ArrowUp") { e.preventDefault(); slashController.index = (slashController.index - 1 + slashController.matches.length) % slashController.matches.length; return; }
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); slashController.run(slashController.matches[Math.min(slashController.index, slashController.matches.length - 1)]); return; }
      if (e.key === "Escape") { e.preventDefault(); input = ""; return; }
    }
    // Enter sends; Shift+Enter inserts a newline.
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function decide(decision: "deny" | "once" | "always") {
    if (!thread.approval) return;
    const id = thread.approval.id;
    thread.removeApproval(thread.approval.session_id);
    try {
      await invoke("approve", { id, decision });
    } catch (e) {
      thread.messages.push({ role: "note", text: `审批失败：${e}` });
    }
  }

  async function answerUserQuestion(answer: string | null) {
    if (!thread.question) return;
    const id = thread.question.id;
    thread.removeQuestion(thread.question.session_id);
    try {
      await invoke("answer_question", { id, answer });
    } catch (e) {
      thread.messages.push({ role: "note", text: `回答问题失败：${e}` });
    }
  }

  const openSettings = settingsController.open;




  async function openCheckpoints() {
    if (rightPanel === "checkpoints") { rightPanel = ""; return; }
    rightPanel = "checkpoints";
    await checkpointController.refresh();
  }



  async function openBranches() {
    if (rightPanel === "branches") { rightPanel = ""; return; }
    rightPanel = "branches";
    await gitWorkspace.refreshBranches();
  }
  async function openDiff() {
    if (rightPanel === "diff") { rightPanel = ""; return; }
    rightPanel = "diff";
    await gitWorkspace.loadDiff();
  }

  async function reloadPanel() {
    try {
      if (rightPanel === "files") await fileBrowser.load(fileBrowser.path);
      else if (rightPanel === "branches") await gitWorkspace.refreshBranches();
      else if (rightPanel === "diff") await gitWorkspace.loadDiff();
      else if (rightPanel === "memory") await memoryController.refresh();
      else if (rightPanel === "checkpoints") await checkpointController.refresh();
    } catch (e) {
      thread.messages.push({ role: "note", text: `刷新失败：${e}` });
    }
  }

  async function openHermes() {
    if (rightPanel === "memory") { rightPanel = ""; return; }
    rightPanel = "memory";
    await memoryController.refresh();
  }


  function showUsage() {
    const costText = usage.priceIn || usage.priceOut
      ? ` · ≈${currencySymbol(usage.currency)}${fmtCost(usage.cost)}`
      : "";
    thread.messages.push({ role: "note", text: `本会话用量：输入 ${usage.promptTokens} / 输出 ${usage.completionTokens} tokens${costText}` });
  }
</script>

<main class="app" style={`--sidebar-width: ${sidebar.width}px`}>
  <SessionSidebar
    sidebarOpen={sidebar.open}
    switchingSession={thread.switching}
    currentSessionId={thread.currentId}
    runningSessions={thread.runningSessions}
    recentSessions={threadLifecycle.recentSessions}
    archivedSessions={threadLifecycle.archivedSessions}
    archivedCount={threadLifecycle.archivedCount}
    bind:showRecent={threadLifecycle.showRecent}
    bind:showArchived={threadLifecycle.showArchived}
    {workspace}
    sidebarResizing={sidebar.resizing}
    sidebarWidth={sidebar.width}
    {SIDEBAR_MIN_WIDTH}
    {SIDEBAR_MAX_WIDTH}
    {SIDEBAR_DEFAULT_WIDTH}
    toggleSidebar={sidebar.toggle}
    newSession={threadLifecycle.create}
    resumeSession={threadLifecycle.resume}
    forkSession={threadLifecycle.fork}
    openSessionLog={threadLifecycle.openLog}
    archiveSession={threadLifecycle.archive}
    {chooseWorkspace}
    {openSettings}
    fmtWhen={threadLifecycle.formatWhen}
    {baseName}
    beginSidebarResize={sidebar.beginResize}
    setSidebarWidth={sidebar.setWidth}
    handleSidebarResizeKey={sidebar.handleResizeKey}
  />
  <div class="workarea">
  <section class="main">
    <TopBar
      sidebarOpen={sidebar.open}
      sessionTitle={thread.title}
      busy={thread.busy}
      {rightPanel}
      toggleSidebar={sidebar.toggle}
      {openFiles}
      {openDiff}
      {openBranches}
      {openHermes}
      {openCheckpoints}
    />

    <ConversationView
      bind:scroller
      messages={thread.messages}
      busy={thread.busy}
      streamingIdx={thread.streamingIndex}
      reasoningIdx={thread.reasoningIndex}
      {renderMarkdown}
      {toolGroupFailureCount}
      {toolOutcome}
    />

    <Composer
      models={modelControls.models}
      currentModel={modelControls.currentModel}
      {header}
      bind:modelMenuOpen={modelControls.modelMenuOpen}
      selectModel={modelControls.selectModel}
      reasoningEffort={modelControls.reasoningEffort}
      bind:reasoningMenuOpen={modelControls.reasoningMenuOpen}
      reasoningEfforts={REASONING_EFFORTS}
      selectReasoningEffort={modelControls.selectReasoningEffort}
      reasoningLabel={modelControls.reasoningLabel}
      permissionMode={modelControls.permissionMode}
      bind:modeMenuOpen={modelControls.modeMenuOpen}
      permissionModes={PERMISSION_MODES}
      selectMode={modelControls.selectMode}
      modeIcon={modelControls.modeIcon}
      modeLabel={modelControls.modeLabel}
      busy={thread.busy}
      {workspace}
      {needsWorkspace}
      {wsName}
      {chooseWorkspace}
      tokIn={usage.promptTokens}
      tokOut={usage.completionTokens}
      priceIn={usage.priceIn}
      priceOut={usage.priceOut}
      priceCurrency={usage.currency}
      cost={usage.cost}
      {fmtTok}
      {currencySymbol}
      {fmtCost}
      bind:queued={thread.queued}
      {attached}
      {isImage}
      {baseName}
      {removeAttachment}
      showSlash={slashController.visible}
      slashMatches={slashController.matches}
      bind:slashIdx={slashController.index}
      runSlash={slashController.run}
      bind:input
      {onKey}
      {handlePaste}
      {attachFiles}
      stopping={thread.stopping}
      {stopGeneration}
      {send}
    />
  </section>

  <InteractionDialogs
    approval={thread.approval}
    userQuestion={thread.question}
    bind:questionAnswer={thread.questionAnswer}
    {decide}
    {answerUserQuestion}
  />


  <WorkspacePanels
    bind:rightPanel
    {reloadPanel}
    bind:checkpointLabel={checkpointController.label}
    checkpointBusy={checkpointController.busy}
    checkpoints={checkpointController.checkpoints}
    checkpointFiles={checkpointController.files}
    saveCheckpoint={checkpointController.save}
    loadCheckpoints={checkpointController.refresh}
    toggleCheckpointDetail={checkpointController.toggleDetail}
    restoreCheckpoint={checkpointController.restore}
    busy={thread.busy}
    bind:newBranch={gitWorkspace.newBranch}
    branchBusy={gitWorkspace.busy}
    branches={gitWorkspace.branches}
    branchCommits={gitWorkspace.branchCommits}
    createBranch={gitWorkspace.createBranch}
    loadBranches={gitWorkspace.refreshBranches}
    toggleBranchDetail={gitWorkspace.toggleBranchDetail}
    switchBranch={gitWorkspace.switchBranch}
    {chooseWorkspace}
    bind:filePreview={fileBrowser.preview}
    filesPath={fileBrowser.path}
    filesEntries={fileBrowser.entries}
    insertMention={fileBrowser.insertMention}
    filesUp={fileBrowser.up}
    pickFile={fileBrowser.pick}
    diffFiles={gitWorkspace.diffFiles}
    diffOpenFiles={gitWorkspace.diffOpenFiles}
    toggleFile={gitWorkspace.toggleFile}
    {diffLineClass}
    bind:historyOpen={threadLifecycle.historyOpen}
    sessions={threadLifecycle.sessions}
    switchingSession={thread.switching}
    resumeSession={threadLifecycle.resume}
    forkSession={threadLifecycle.fork}
    openSessionLog={threadLifecycle.openLog}
    openSessionSnapshot={threadLifecycle.openSnapshot}
    refreshSessions={threadLifecycle.refresh}
    bind:newNote={memoryController.newNote}
    bind:newNoteTags={memoryController.newNoteTags}
    hermesBusy={memoryController.busy}
    notes={memoryController.notes}
    addNote={memoryController.add}
    consolidateMemory={memoryController.consolidate}
    loadNotes={memoryController.refresh}
    openMemoryFile={memoryController.openFile}
    fmtTs={memoryController.formatTimestamp}
  />


  <SettingsModal
    bind:settings={settingsController.settings}
    bind:apiKeyInput={settingsController.apiKeyInput}
    bind:vlApiKeyInput={settingsController.vlApiKeyInput}
    configLocation={settingsController.configLocation}
    modelCatalog={settingsController.catalog}
    officialProviders={settingsController.officialProviders}
    openRouterProvider={settingsController.openRouterProvider}
    catalogRefreshing={settingsController.catalogRefreshing}
    presetSaving={settingsController.presetSaving}
    saving={settingsController.saving}
    harnessDiagnostics={pluginController.diagnostics}
    externalPlugins={pluginController.externalPlugins}
    codexPlugins={pluginController.codexPlugins}
    pluginMarketplaces={pluginController.marketplaces}
    {REASONING_EFFORTS}
    {currencySymbol}
    {currencyName}
    {priceSourceName}
    currentPriceSourceName={settingsController.currentPriceSourceName}
    openConfigFile={settingsController.openConfigFile}
    openConfigDir={settingsController.openConfigDir}
    applyModelPreset={settingsController.applyPreset}
    openPriceSource={settingsController.openPriceSource}
    refreshOpenRouterModels={settingsController.refreshOpenRouter}
    addExternalPlugin={pluginController.addExternal}
    upgradeExternalPlugin={pluginController.upgradeExternal}
    toggleExternalPlugin={pluginController.toggleExternal}
    addCodexPlugin={pluginController.addCodex}
    upgradeCodexPlugin={pluginController.upgradeCodex}
    toggleCodexPlugin={pluginController.toggleCodex}
    removeCodexPlugin={pluginController.removeCodex}
    installMarketplacePlugin={pluginController.installMarketplace}
    saveSettings={settingsController.save}
  />
  </div>
</main>
