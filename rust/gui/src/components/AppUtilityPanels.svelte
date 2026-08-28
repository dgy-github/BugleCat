<script lang="ts">
  import SettingsModal from "./SettingsModal.svelte";
  import WorkspacePanels from "./WorkspacePanels.svelte";
  import type { AppRuntimeController } from "../lib/app-runtime-controller.svelte";
  import type { CheckpointController } from "../lib/checkpoint-controller.svelte";
  import type { ComposerController } from "../lib/composer-controller.svelte";
  import type { FileBrowserController } from "../lib/file-browser-controller.svelte";
  import type { GitWorkspaceController } from "../lib/git-workspace-controller.svelte";
  import type { MemoryController } from "../lib/memory-controller.svelte";
  import type { ForgeController } from "../lib/forge-controller.svelte";
  import { reasoningEffortsForModel, type ModelControlsController } from "../lib/model-controls-controller.svelte";
  import type { PanelController } from "../lib/panel-controller.svelte";
  import type { PluginController } from "../lib/plugin-controller.svelte";
  import type { SettingsController } from "../lib/settings-controller.svelte";
  import type { ThreadController } from "../lib/thread-controller.svelte";
  import type { ThreadLifecycleController } from "../lib/thread-lifecycle-controller.svelte";
  import { currencyName, currencySymbol, diffLineClass, priceSourceName } from "../lib/ui-format";

  type ThemeMode = "system" | "light" | "dark";
  let {
    panels, checkpointController, thread, gitWorkspace, runtime, fileBrowser,
    threadLifecycle, memoryController, forgeController, settingsController, pluginController,
    modelControls, composer, themeMode = $bindable(), setThemeMode,
  }: {
    panels: PanelController;
    checkpointController: CheckpointController;
    thread: ThreadController;
    gitWorkspace: GitWorkspaceController;
    runtime: AppRuntimeController;
    fileBrowser: FileBrowserController;
    threadLifecycle: ThreadLifecycleController;
    memoryController: MemoryController;
    forgeController: ForgeController;
    settingsController: SettingsController;
    pluginController: PluginController;
    modelControls: ModelControlsController;
    composer: ComposerController;
    themeMode: ThemeMode;
    setThemeMode: (mode: ThemeMode) => void;
  } = $props();
</script>

<WorkspacePanels
  bind:rightPanel={panels.current} reloadPanel={panels.reload}
  bind:checkpointLabel={checkpointController.label} checkpointBusy={checkpointController.busy}
  checkpoints={checkpointController.checkpoints} checkpointFiles={checkpointController.files}
  saveCheckpoint={checkpointController.save} loadCheckpoints={checkpointController.refresh}
  toggleCheckpointDetail={checkpointController.toggleDetail} restoreCheckpoint={checkpointController.restore}
  busy={thread.busy} bind:newBranch={gitWorkspace.newBranch} branchBusy={gitWorkspace.busy}
  branches={gitWorkspace.branches} branchCommits={gitWorkspace.branchCommits}
  createBranch={gitWorkspace.createBranch} loadBranches={gitWorkspace.refreshBranches}
  toggleBranchDetail={gitWorkspace.toggleBranchDetail} switchBranch={gitWorkspace.switchBranch}
  chooseWorkspace={runtime.chooseWorkspace} bind:filePreview={fileBrowser.preview}
  filesPath={fileBrowser.path} filesEntries={fileBrowser.entries} insertMention={fileBrowser.insertMention}
  filesUp={fileBrowser.up} pickFile={fileBrowser.pick} diffFiles={gitWorkspace.diffFiles}
  diffOpenFiles={gitWorkspace.diffOpenFiles} toggleFile={gitWorkspace.toggleFile} {diffLineClass}
  bind:historyOpen={threadLifecycle.historyOpen} sessions={threadLifecycle.sessions}
  switchingSession={thread.switching} resumeSession={threadLifecycle.resume}
  forkSession={threadLifecycle.fork} openSessionLog={threadLifecycle.openLog}
  openSessionSnapshot={threadLifecycle.openSnapshot} refreshSessions={threadLifecycle.refresh}
  bind:newNote={memoryController.newNote} bind:newNoteTags={memoryController.newNoteTags}
  hermesBusy={memoryController.busy} notes={memoryController.notes} addNote={memoryController.add}
  consolidateMemory={memoryController.consolidate} loadNotes={memoryController.refresh}
  mergeMemory={memoryController.mergeWithModel} cancelMemoryMerge={memoryController.cancelMerge}
  memoryMergeStatus={memoryController.mergeStatus}
  openMemoryFile={memoryController.openFile} fmtTs={memoryController.formatTimestamp}
  {forgeController}
/>

<SettingsModal
  bind:settings={settingsController.settings} bind:apiKeyInput={settingsController.apiKeyInput}
  bind:deepseekApiKeyInput={settingsController.deepseekApiKeyInput}
  bind:yunmoApiKeyInput={settingsController.yunmoApiKeyInput} bind:vlApiKeyInput={settingsController.vlApiKeyInput}
  bind:dashscopeTokenPlanKeyInput={settingsController.dashscopeTokenPlanKeyInput}
  bind:dashscopeWorkspaceKeyInput={settingsController.dashscopeWorkspaceKeyInput}
  configLocation={settingsController.configLocation} modelCatalog={settingsController.catalog}
  officialProviders={settingsController.officialProviders} yunmoProvider={settingsController.yunmoProvider}
  openRouterProvider={settingsController.openRouterProvider} catalogRefreshing={settingsController.catalogRefreshing}
  yunmoRefreshing={settingsController.yunmoRefreshing} presetSaving={settingsController.presetSaving}
  saving={settingsController.saving} harnessDiagnostics={pluginController.diagnostics}
  externalPlugins={pluginController.externalPlugins} codexPlugins={pluginController.codexPlugins}
  pluginMarketplaces={pluginController.marketplaces} bind:dshSource={pluginController.dshSource}
  bind:dshManifestUrl={pluginController.dshManifestUrl} bind:dshQuery={pluginController.dshQuery}
  bind:dshCategory={pluginController.dshCategory} dshCategories={pluginController.dshCategories}
  dshItems={pluginController.dshItems} dshPreview={pluginController.dshPreview}
  dshSelected={pluginController.dshSelected} dshLoading={pluginController.dshLoading}
  dshError={pluginController.dshError} bind:themeMode {setThemeMode}
  executionMode={composer.executionMode}
  selectExecutionMode={(mode) => { composer.executionMode = mode; composer.executionMenuOpen = false; }}
  harnessProfile={threadLifecycle.activeHarnessProfile}
  harnessProfiles={threadLifecycle.harnessProfiles}
  selectHarnessProfile={threadLifecycle.selectHarnessProfile}
  harnessProfileLabel={threadLifecycle.harnessProfileLabel}
  harnessProfileLocked={threadLifecycle.harnessProfileLocked}
  REASONING_EFFORTS={reasoningEffortsForModel(settingsController.settings?.model ?? modelControls.currentModel)}
  {currencySymbol} {currencyName} {priceSourceName}
  currentPriceSourceName={settingsController.currentPriceSourceName}
  openConfigFile={settingsController.openConfigFile} openConfigDir={settingsController.openConfigDir}
  applyModelPreset={settingsController.applyPreset} openPriceSource={settingsController.openPriceSource}
  refreshOpenRouterModels={settingsController.refreshOpenRouter} refreshYunmoModels={settingsController.refreshYunmo}
  customProviderActivated={(model, models) => modelControls.applyModel(model, models)}
  addExternalPlugin={pluginController.addExternal} upgradeExternalPlugin={pluginController.upgradeExternal}
  toggleExternalPlugin={pluginController.toggleExternal} addCodexPlugin={pluginController.addCodex}
  upgradeCodexPlugin={pluginController.upgradeCodex} toggleCodexPlugin={pluginController.toggleCodex}
  removeCodexPlugin={pluginController.removeCodex} installMarketplacePlugin={pluginController.installMarketplace}
  searchDshMarketplace={pluginController.searchDsh} previewDshMarketplace={pluginController.previewDsh}
  installDshMarketplace={pluginController.installDsh} saveSettings={settingsController.save}
/>
