<script lang="ts">
  import { onMount } from "svelte";
  import ConversationView from "./components/ConversationView.svelte";
  import Composer from "./components/Composer.svelte";
  import InteractionDialogs from "./components/InteractionDialogs.svelte";
  import WorkspacePanels from "./components/WorkspacePanels.svelte";
  import SettingsModal from "./components/SettingsModal.svelte";
  import SessionSidebar from "./components/SessionSidebar.svelte";
  import TopBar from "./components/TopBar.svelte";
  import { baseName, currencyName, currencySymbol, diffLineClass, formatCost as fmtCost, formatTokens as fmtTok, priceSourceName, renderMarkdown, toolOutcome, toolStatusLabel } from "./lib/ui-format";
  import { SidebarController, SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from "./lib/sidebar-controller.svelte";
  import { appendReasoning, hideCompletedToolActivity, keepConversationConclusions, settleCompletedToolGroups, toolGroupFailureCount, type ConversationMessage as Msg, type ToolEntry, type ToolGroup } from "./lib/conversation-model";
  import { UsageController } from "./lib/usage-controller.svelte";
  import { FileBrowserController } from "./lib/file-browser-controller.svelte";
  import { GitWorkspaceController } from "./lib/git-workspace-controller.svelte";
  import { CheckpointController } from "./lib/checkpoint-controller.svelte";
  import { MemoryController } from "./lib/memory-controller.svelte";
  import { PluginController } from "./lib/plugin-controller.svelte";
  import { SettingsController } from "./lib/settings-controller.svelte";
  import { SlashController } from "./lib/slash-controller.svelte";
  import { ModelControlsController, PERMISSION_MODES, REASONING_EFFORTS } from "./lib/model-controls-controller.svelte";
  import { ThreadController } from "./lib/thread-controller.svelte";
  import { ThreadLifecycleController } from "./lib/thread-lifecycle-controller.svelte";
  import { ComposerController, isImageAttachment } from "./lib/composer-controller.svelte";
  import { PanelController } from "./lib/panel-controller.svelte";
  import { AppRuntimeController } from "./lib/app-runtime-controller.svelte";

  const sidebar = new SidebarController();
  const usage = new UsageController();

  const isImage = isImageAttachment;

  // UiEvent and per-Thread state are owned by ThreadController.
  const thread = new ThreadController(usage, {
    refreshSessions: () => void threadLifecycle.refresh(),
    scrollDown: () => scrollDown(),
    dequeue: () => composer.dequeue(),
    ready: (event) => runtime.handleReady(event),
  });
  const threadLifecycle = new ThreadLifecycleController(thread, usage, () => runtime.workspace);
  const composer = new ComposerController(thread, () => runtime.needsWorkspace, () => scrollDown());
  const fileBrowser = new FileBrowserController(
    (text) => thread.messages.push({ role: "note", text }),
    () => composer.input,
    (value) => (composer.input = value),
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
  const panels = new PanelController(fileBrowser, gitWorkspace, checkpointController, memoryController, thread);
  const slashController = new SlashController(
    () => composer.input,
    (value) => (composer.input = value),
    (text) => thread.messages.push({ role: "note", text }),
    {
      newSession: () => void threadLifecycle.create(),
      forkCurrent: () => thread.currentId ? void threadLifecycle.fork(thread.currentId, thread.title) : thread.messages.push({ role: "note", text: "当前会话还没有快照，无法分叉（先发一条消息）。" }),
      openModel: () => (modelControls.modelMenuOpen = true),
      openSettings: () => void openSettings(),
      showUsage: () => thread.messages.push({ role: "note", text: usage.summary() }),
      openCheckpoints: () => void panels.openCheckpoints(), openFiles: () => void panels.openFiles(), openDiff: () => void panels.openDiff(),
      openBranches: () => void panels.openBranches(), openMemory: () => void panels.openMemory(),
      refreshSessions: threadLifecycle.refresh, currentThreadId: () => thread.currentId, currentTitle: () => thread.title,
      setTitle: (title) => (thread.title = title),
    },
  );
  composer.connectSlash(slashController);
  const runtime = new AppRuntimeController(sidebar, thread, threadLifecycle, composer, modelControls, usage, slashController);

  let scroller = $state<HTMLDivElement>();
  function scrollDown() {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }

  onMount(runtime.start);

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
    bind:showRecent={threadLifecycle.showRecent}
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
    chooseWorkspace={runtime.chooseWorkspace}
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
      rightPanel={panels.current}
      toggleSidebar={sidebar.toggle}
      openFiles={panels.openFiles}
      openDiff={panels.openDiff}
      openBranches={panels.openBranches}
      openHermes={panels.openMemory}
      openCheckpoints={panels.openCheckpoints}
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
      header={runtime.header}
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
      workspace={runtime.workspace}
      needsWorkspace={runtime.needsWorkspace}
      wsName={runtime.workspaceName}
      chooseWorkspace={runtime.chooseWorkspace}
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


  <WorkspacePanels
    bind:rightPanel={panels.current}
    reloadPanel={panels.reload}
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
    chooseWorkspace={runtime.chooseWorkspace}
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
