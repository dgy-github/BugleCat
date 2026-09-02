<script lang="ts">
  import { onMount } from "svelte";
  import ConversationView from "./components/ConversationView.svelte";
  import Composer from "./components/Composer.svelte";
  import DshSlotOverlay from "./components/DshSlotOverlay.svelte";
  import InteractionDialogs from "./components/InteractionDialogs.svelte";
  import AppUtilityPanels from "./components/AppUtilityPanels.svelte";
  import SessionSidebar from "./components/SessionSidebar.svelte";
  import TopBar from "./components/TopBar.svelte";
  import { baseName, currencySymbol, formatCost as fmtCost, formatTokens as fmtTok, renderMarkdown, toolOutcome } from "./lib/ui-format";
  import { SidebarController, SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from "./lib/sidebar-controller.svelte";
  import { appendReasoning, hideCompletedToolActivity, keepConversationConclusions, settleCompletedToolGroups, toolGroupFailureCount, type ConversationMessage as Msg, type ToolEntry, type ToolGroup } from "./lib/conversation-model";
  import { UsageController } from "./lib/usage-controller.svelte";
  import { FileBrowserController } from "./lib/file-browser-controller.svelte";
  import { GitWorkspaceController } from "./lib/git-workspace-controller.svelte";
  import { CheckpointController } from "./lib/checkpoint-controller.svelte";
  import { MemoryController } from "./lib/memory-controller.svelte";
  import { ForgeController } from "./lib/forge-controller.svelte";
  import { PluginController } from "./lib/plugin-controller.svelte";
  import { DshSlotController } from "./lib/dsh-slot-controller.svelte";
  import { SettingsController } from "./lib/settings-controller.svelte";
  import { SlashController } from "./lib/slash-controller.svelte";
  import { ModelControlsController, PERMISSION_MODES } from "./lib/model-controls-controller.svelte";
  import { ThreadController } from "./lib/thread-controller.svelte";
  import { ThreadLifecycleController } from "./lib/thread-lifecycle-controller.svelte";
  import { ComposerController, isImageAttachment } from "./lib/composer-controller.svelte";
  import { PanelController } from "./lib/panel-controller.svelte";
  import { AppRuntimeController } from "./lib/app-runtime-controller.svelte";
  import { GoalController } from "./lib/goal-controller.svelte";

  const sidebar = new SidebarController();
  const usage = new UsageController();
  let activeView = $state<"chat" | "trajectory">("chat");
  type ThemeMode = "system" | "light" | "dark";
  let themeMode = $state<ThemeMode>("system");

  function setTheme(mode: ThemeMode) {
    themeMode = mode;
    document.documentElement.dataset.theme = mode;
    localStorage.setItem("nanocodex.theme", mode);
  }

  function cycleTheme() {
    setTheme(themeMode === "system" ? "light" : themeMode === "light" ? "dark" : "system");
  }

  const isImage = isImageAttachment;

  // UiEvent and per-Thread state are owned by ThreadController.
  const thread = new ThreadController(usage, {
    refreshSessions: () => void threadLifecycle.refresh(),
    scrollDown: () => scrollDown(),
    dequeue: () => composer.dequeue(),
    ready: (event) => runtime.handleReady(event),
  });
  const note = (text: string) => thread.messages.push({ role: "note", text });
  const workspace = () => runtime.workspace;
  const threadLifecycle = new ThreadLifecycleController(
    thread, usage, workspace,
    { reconcile: () => runtime.reconcileWorkspace() },
  );
  const goalController = new GoalController(thread);
  const composer = new ComposerController(thread, () => runtime.needsWorkspace, () => scrollDown());
  const fileBrowser = new FileBrowserController(note, () => composer.input, (value) => (composer.input = value), workspace);
  const gitWorkspace = new GitWorkspaceController(note, workspace);
  const checkpointController = new CheckpointController(note, () => thread.busy, workspace);
  const memoryController = new MemoryController(note, workspace);
  const forgeController = new ForgeController(note, workspace);
  const pluginController = new PluginController(note);
  const dshSlots = new DshSlotController(pluginController);
  const modelControls = new ModelControlsController(
    note,
    (priceIn, priceOut, currency) => usage.setPrice(priceIn, priceOut, currency),
    () => thread.currentId,
  );
  const settingsController = new SettingsController(
    pluginController,
    note,
    modelControls.applyModel,
    (priceIn, priceOut, currency) => usage.setPrice(priceIn, priceOut, currency),
  );
  const panels = new PanelController(fileBrowser, gitWorkspace, checkpointController, memoryController, thread);
  const slashController = new SlashController(
    () => composer.input,
    (value) => (composer.input = value),
    note,
    {
      newSession: () => void threadLifecycle.create(),
      forkCurrent: () => thread.currentId ? void threadLifecycle.fork(thread.currentId, thread.title) : thread.messages.push({ role: "note", text: "当前会话还没有快照，无法分叉（先发一条消息）。" }),
      openModel: () => (modelControls.modelMenuOpen = true),
      openSettings: () => void openSettings(),
      showUsage: () => thread.messages.push({ role: "note", text: usage.summary() }),
      openHistory: () => { threadLifecycle.historyOpen = true; void threadLifecycle.refresh(); },
      openCheckpoints: () => void panels.openCheckpoints(), openFiles: () => void panels.openFiles(), openDiff: () => void panels.openDiff(),
      openBranches: () => void panels.openBranches(), openMemory: () => void panels.openMemory(),
      refreshSessions: threadLifecycle.refresh, currentThreadId: () => thread.currentId, currentTitle: () => thread.title,
      setTitle: (title) => (thread.title = title),
    },
  );
  composer.connectSlash(slashController);
  const runtime = new AppRuntimeController(
    sidebar, thread, threadLifecycle, composer, modelControls, usage, slashController,
    () => {
      panels.workspaceChanged();
      threadLifecycle.workspaceChanged();
      forgeController.workspaceChanged();
    },
  );
  let observedGoalThread = "";
  let observedGoalBusy = false;
  $effect(() => {
    const threadId = thread.currentId;
    const busy = thread.busy;
    if (!threadId) goalController.clear();
    else if (threadId !== observedGoalThread || (observedGoalBusy && !busy)) void goalController.refresh(threadId);
    observedGoalThread = threadId;
    observedGoalBusy = busy;
  });

  let scroller = $state<HTMLDivElement>();
  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(() => {
    const savedTheme = localStorage.getItem("nanocodex.theme");
    setTheme(savedTheme === "light" || savedTheme === "dark" ? savedTheme : "system");
    let disposed = false;
    void (async () => {
      await runtime.start();
      if (disposed) return;
      await settingsController.refreshRuntimeModels();
      if (disposed) return;
      void pluginController.load().catch((error) => {
        if (!disposed) console.error("插件加载失败", error);
      });
      void forgeController.refresh();
    })().catch((error) => {
      if (disposed) return;
      console.error("应用启动失败", error);
      runtime.header = "连接错误";
    });
    return () => {
      disposed = true;
      forgeController.dispose();
      void runtime.stop();
    };
  });
  const openSettings = settingsController.open;
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
    bind:showArchived={threadLifecycle.showArchived}
    workspace={runtime.workspace}
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
    renameSession={threadLifecycle.rename}
    chooseWorkspace={runtime.chooseWorkspace}
    {openSettings}
    dshFooterActions={dshSlots.footerActions}
    openDshSlot={dshSlots.open}
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
      rightPanel={panels.current}
      bind:activeView
      toggleSidebar={sidebar.toggle}
      openFiles={panels.openFiles}
      openDiff={panels.openDiff}
      openBranches={panels.openBranches}
      openHermes={panels.openMemory}
      openCheckpoints={panels.openCheckpoints}
      {themeMode}
      {cycleTheme}
    />

    <ConversationView
      bind:scroller
      messages={activeView === "trajectory" ? thread.trajectoryMessages : thread.messages}
      {activeView}
      busy={thread.busy}
      streamingIdx={thread.streamingIndex}
      reasoningIdx={thread.reasoningIndex}
      {renderMarkdown}
      {toolGroupFailureCount}
      {toolOutcome}
      forkCurrent={() => thread.currentId && threadLifecycle.fork(thread.currentId, thread.title)}
    />

    <Composer
      models={modelControls.models}
      currentModel={modelControls.currentModel}
      currentProvider={modelControls.currentProvider}
      currentProviderName={modelControls.currentProviderName}
      currentProtocol={modelControls.currentProtocol}
      routeLabel={modelControls.routeLabel}
      routes={modelControls.routes}
      header={runtime.header}
      bind:modelMenuOpen={modelControls.modelMenuOpen}
      selectModel={modelControls.selectModel}
      selectRouteModel={modelControls.selectRouteModel}
      reasoningEffort={modelControls.reasoningEffort}
      bind:reasoningMenuOpen={modelControls.reasoningMenuOpen}
      reasoningEfforts={modelControls.reasoningEfforts}
      selectReasoningEffort={modelControls.selectReasoningEffort}
      reasoningLabel={modelControls.reasoningLabel}
      permissionMode={modelControls.permissionMode}
      bind:modeMenuOpen={modelControls.modeMenuOpen}
      permissionModes={PERMISSION_MODES}
      selectMode={modelControls.selectMode}
      modeIcon={modelControls.modeIcon}
      modeLabel={modelControls.modeLabel}
      goalView={goalController.view}
      goalStatusLabel={goalController.statusLabel}
      goalRemainingRounds={goalController.remainingRounds}
      goalLoading={goalController.loading}
      bind:goalMenuOpen={goalController.menuOpen}
      pauseGoal={goalController.pause}
      resumeGoal={goalController.resume}
      busy={thread.busy}
      switching={thread.switching}
      workspace={runtime.workspace}
      needsWorkspace={runtime.needsWorkspace}
      wsName={runtime.workspaceName}
      chooseWorkspace={runtime.chooseWorkspace}
      turnCount={thread.messages.filter((message) => message.role === "user").length}
      stepCount={thread.messages.reduce((count, message) => count + (message.role === "tool_group" ? message.tools.length : 0), 0)}
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
      attached={composer.attached}
      {isImage}
      {baseName}
      removeAttachment={composer.removeAttachment}
      showSlash={slashController.visible}
      slashMatches={slashController.matches}
      bind:slashIdx={slashController.index}
      runSlash={slashController.run}
      bind:input={composer.input}
      onKey={composer.onKey}
      handlePaste={composer.handlePaste}
      attachFiles={composer.attachFiles}
      stopping={thread.stopping}
      stopGeneration={composer.stop}
      send={composer.send}
    />
  </section>

  <InteractionDialogs
    approval={thread.approval}
    userQuestion={thread.question}
    bind:questionAnswer={thread.questionAnswer}
    decide={runtime.decide}
    answerUserQuestion={runtime.answerQuestion}
  />
  <DshSlotOverlay controller={dshSlots} />
  <AppUtilityPanels
    {panels} {checkpointController} {thread} {gitWorkspace} {runtime} {fileBrowser}
    {threadLifecycle} {memoryController} {forgeController} {settingsController} {pluginController} {modelControls} {composer}
    bind:themeMode setThemeMode={setTheme}
  />
  </div>
</main>
