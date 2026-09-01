import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { appServerRequest, ProtocolSequenceGate, type ProtocolEventEnvelope } from "./app-server-client";
import type { ComposerController } from "./composer-controller.svelte";
import type { ModelControlsController } from "./model-controls-controller.svelte";
import type { SidebarController } from "./sidebar-controller.svelte";
import type { SlashController } from "./slash-controller.svelte";
import type { ThreadController, UiEvent } from "./thread-controller.svelte";
import type { ThreadLifecycleController } from "./thread-lifecycle-controller.svelte";
import type { UsageController } from "./usage-controller.svelte";

export class AppRuntimeController {
  header = $state("连接中…");
  workspace = $state("");
  needsWorkspace = $state(false);
  sandboxMode = $state("");
  private readonly sequenceGate = new ProtocolSequenceGate();
  private readonly unlisten: Array<() => void | Promise<void>> = [];
  private started = false;
  private lifecycleGeneration = 0;
  private workspacePickerOpen = false;

  constructor(
    private readonly sidebar: SidebarController,
    private readonly thread: ThreadController,
    private readonly lifecycle: ThreadLifecycleController,
    private readonly composer: ComposerController,
    private readonly models: ModelControlsController,
    private readonly usage: UsageController,
    private readonly slash: SlashController,
    private readonly workspaceChanged: () => void,
  ) {}

  get workspaceName(): string {
    return this.workspace ? this.workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || this.workspace : "";
  }

  // Thread activation/forking selects a durable Thread workspace inside the
  // backend before its worker has finished rebuilding. If that later step
  // fails, the lifecycle controller uses these two methods to restore (or at
  // least truthfully reconcile) the process-wide CWD before it rebinds UI.
  restoreWorkspace = async (path: string): Promise<string> => {
    const workspace = await appServerRequest<string>({ method: "workspaceSet", params: { path } });
    this.workspace = workspace;
    this.workspaceChanged();
    return workspace;
  };

  reconcileWorkspace = async (): Promise<string> => {
    try {
      const status = await appServerRequest<{ workspace: string }>({ method: "runtimeStatusRead" });
      this.workspace = status.workspace;
      this.workspaceChanged();
      return status.workspace;
    } catch (error) {
      // A failed rollback plus an unreadable status leaves the CWD unknown.
      // Block new prompts rather than continuing to display the old project.
      this.workspace = "";
      this.needsWorkspace = true;
      this.workspaceChanged();
      throw error;
    }
  };

  start = async (): Promise<void> => {
    if (this.started) return;
    this.started = true;
    const generation = ++this.lifecycleGeneration;
    try {
      this.sidebar.restoreWidth();
      try {
        const status = await appServerRequest<{ model: string; sandbox: string; approval: string; permission_mode: string; reasoning_effort: string; workspace: string; price_in: number; price_out: number; price_currency: "CNY" | "USD" }>({ method: "runtimeStatusRead" });
        if (!this.isActive(generation)) return;
        this.header = `${status.model} · ${status.sandbox}`;
        this.sandboxMode = status.sandbox;
        this.workspace = status.workspace;
        this.models.currentModel = status.model;
        if (status.permission_mode) this.models.permissionMode = status.permission_mode;
        if (status.reasoning_effort) this.models.reasoningEffort = status.reasoning_effort;
        this.usage.setPrice(status.price_in, status.price_out, status.price_currency);
      } catch {
        if (this.isActive(generation)) this.header = "配置错误";
      }
      if (!this.isActive(generation)) return;
      void this.lifecycle.refresh();
      const unlistenProtocol = await listen<ProtocolEventEnvelope>("ncx://protocol-event", (message) => {
        if (!this.isActive(generation)) return;
        const envelope = message.payload;
        if (!this.sequenceGate.accept(envelope)) return;
        if (["threadCreated", "threadUpdated", "turnCompleted"].includes(envelope.event.type)) void this.lifecycle.refresh();
      });
      if (!this.isActive(generation)) {
        this.disposeListener(unlistenProtocol);
        return;
      }
      this.unlisten.push(unlistenProtocol);
      const unlistenUi = await listen<UiEvent>("ncx://event", (event) => {
        if (this.isActive(generation)) this.thread.handle(event.payload);
      });
      if (!this.isActive(generation)) {
        this.disposeListener(unlistenUi);
        return;
      }
      this.unlisten.push(unlistenUi);
      appServerRequest({ method: "runtimeReadyRefresh" }).catch(() => {});
      void this.slash.loadCustomCommands();
    } catch (error) {
      // An older start can reject after a later mount has already started a
      // new generation. It must not dispose that newer generation's listeners.
      if (!this.isActive(generation)) return;
      await this.stop();
      throw error;
    }
  };

  stop = async (): Promise<void> => {
    this.lifecycleGeneration += 1;
    this.started = false;
    const listeners = this.unlisten.splice(0);
    // Tauri's runtime implementation returns an async unlisten function even
    // though its TypeScript declaration permits a synchronous callback. Do not
    // leave a teardown rejection as an unhandled promise during window close.
    await Promise.allSettled(listeners.map((unlisten) => Promise.resolve().then(unlisten)));
  };

  private isActive = (generation: number): boolean => this.started && generation === this.lifecycleGeneration;

  private disposeListener = (unlisten: () => void | Promise<void>): void => {
    void Promise.resolve().then(unlisten).catch(() => {});
  };

  handleReady = (event: Extract<UiEvent, { kind: "ready" }>): void => {
    const routeChanged = this.models.currentProvider !== event.provider_id || this.models.currentProtocol !== event.provider_protocol;
    // Resume/Fork can activate a Thread from another project without going
    // through the picker. Reset the same workspace projections as the picker
    // only when the backend has actually selected a different workspace; the
    // picker already performed that reset before its matching ready event.
    // The first Ready event is also a projection transition when startup
    // began before the host had reported its workspace. Treating "" → path
    // as unchanged leaves workspace-bound observers (such as Forge) stuck
    // after their intentionally fail-closed initial read.
    const workspaceDidChange = this.workspace !== event.workspace;
    this.header = `${event.model} · ${event.sandbox}`;
    this.workspace = event.workspace;
    this.needsWorkspace = event.needs_workspace;
    this.sandboxMode = event.sandbox;
    this.models.currentModel = event.model;
    this.models.currentProvider = event.provider_id;
    this.models.currentProtocol = event.provider_protocol;
    if (event.models?.length) this.models.models = event.models;
    if (event.permission_mode) this.models.permissionMode = event.permission_mode;
    if (event.reasoning_effort) this.models.reasoningEffort = event.reasoning_effort;
    if (workspaceDidChange) {
      this.workspaceChanged();
      void this.lifecycle.refresh();
    }
    if (routeChanged || this.models.routes.length === 0) void this.models.refreshRoutes();
  };

  chooseWorkspace = async (): Promise<void> => {
    if (this.thread.switching || this.workspacePickerOpen) return;
    const pickerSessionId = this.thread.currentId;
    this.workspacePickerOpen = true;
    // The native dialog resolves asynchronously. Keep session navigation
    // locked while it is visible so an older selection cannot later replace a
    // newly resumed or created session.
    this.thread.switching = true;
    try {
      const directory = await open({ directory: true, multiple: false });
      if (!directory || Array.isArray(directory)) return;
      // The picker itself is asynchronous. Capture the active session only
      // after it closes so a session navigation while the native dialog was
      // open cannot be stashed or restored under a stale id.
      if (this.thread.currentId !== pickerSessionId) return;
      const previousId = pickerSessionId;
      const previousTitle = this.thread.title;
      const previousMessages = [...this.thread.messages];
      const previousWorkspace = this.workspace;
      let workspaceChanged = false;
      let workspaceRestored = false;
      try {
        const threadId = `thread-${crypto.randomUUID()}`;
        this.thread.expectReady(threadId);
        this.thread.stash(previousId);
        this.thread.switching = true;
        this.thread.busy = false;
        this.thread.currentId = "";
        this.thread.messages = [];
        this.thread.title = "新会话";
        this.usage.reset();
        this.thread.queued = [];
        this.composer.attached = [];
        const workspace = await appServerRequest<string>({ method: "workspaceSet", params: { path: directory } });
        workspaceChanged = true;
        // Reflect the actual process workspace immediately. If the following
        // Thread creation or rollback fails, showing the previous path would
        // make workspace-bound operations fail closed but leave the user on a
        // misleading project label.
        this.workspace = workspace;
        this.workspaceChanged();
        const created = await appServerRequest<{ metadata: { harnessProfile: string } }>({
          method: "threadCreateActivate",
          params: {
            threadId,
            workspace,
            title: "(no prompt yet)",
            harnessProfile: this.lifecycle.selectedHarnessProfile,
          },
        });
        this.workspace = workspace;
        this.lifecycle.activeHarnessProfile = created.metadata.harnessProfile || this.lifecycle.selectedHarnessProfile;
        this.thread.currentId = threadId;
        this.thread.restore(threadId);
        // Ready arrives through Tauri's independent event channel. Keep the
        // fence until that accepted event consumes it; clearing here would
        // reopen the empty-state binding race during a later failure path.
        this.thread.messages.push({ role: "note", text: `已切换工作区到 ${workspace}，已开始新会话。` });
        this.thread.switching = false;
        void this.lifecycle.refresh();
      } catch (error) {
        // Stop every delayed Ready while the process CWD is being restored.
        // The matching old-session fence is installed only after rollback has
        // succeeded; on a failed rollback this stays sealed fail-closed.
        this.thread.sealReadyFence();
        let rollbackError = "";
        if (workspaceChanged && previousWorkspace) {
          try {
            await this.restoreWorkspace(previousWorkspace);
            workspaceRestored = true;
          } catch (rollback) {
            rollbackError = `；恢复原工作区也失败：${rollback}`;
            try { rollbackError += `；当前工作区：${await this.reconcileWorkspace()}`; }
            catch { rollbackError += "；当前工作区未能确认，已禁止继续操作"; }
          }
        }
        if (workspaceChanged && !workspaceRestored) {
          // CWD changed but could not be restored. Do not rebind the old
          // Thread/UI to a different workspace. Keep the original ready fence
          // installed so a delayed Ready for the old session cannot bind this
          // safe empty state to the new process CWD.
          this.thread.busy = false;
          this.thread.stopping = false;
          this.thread.switching = false;
          this.thread.currentId = "";
          this.thread.title = "新会话";
          this.thread.queued = [];
          this.thread.messages = [{
            role: "note",
            text: `已切换工作区到 ${this.workspace}，但新会话初始化失败：${error}${rollbackError}。当前未绑定会话，请新建会话后重试。`,
          }];
          this.usage.reset();
          void this.lifecycle.refresh();
          return;
        }
        // Keep rejecting delayed events until the old session is safely bound
        // again. `handleReady` consumes this fence only when it accepts the
        // matching Ready event. With no prior session, leave the fence sealed
        // so the empty state remains fail-closed.
        if (previousId) this.thread.expectReady(previousId);
        this.thread.busy = this.thread.runningSessions.has(previousId);
        this.thread.switching = false;
        this.thread.currentId = previousId;
        this.thread.title = previousTitle;
        this.thread.restore(previousId);
        this.thread.messages = previousMessages;
        this.usage.restore(previousId);
        if (workspaceRestored) void this.lifecycle.refresh();
        this.thread.messages.push({ role: "note", text: `切换工作区失败：${error}${rollbackError}` });
      }
    } catch (error) {
      this.thread.messages.push({ role: "note", text: `打开工作区选择器失败：${error}` });
    } finally {
      this.thread.switching = false;
      if (!this.thread.busy) this.composer.dequeue();
      this.workspacePickerOpen = false;
    }
  };

  decide = async (decision: "deny" | "once" | "always"): Promise<void> => {
    if (!this.thread.approval) return;
    const approval = this.thread.approval;
    try {
      await appServerRequest({ method: "interactionApprove", params: { threadId: approval.session_id || null, id: approval.id, decision } });
      this.thread.removeApproval(approval.session_id);
    }
    catch (error) { this.thread.messages.push({ role: "note", text: `审批失败：${error}` }); }
  };

  answerQuestion = async (answer: string | null): Promise<void> => {
    if (!this.thread.question) return;
    const question = this.thread.question;
    try {
      await appServerRequest({ method: "interactionAnswer", params: { threadId: question.session_id || null, id: question.id, answer } });
      this.thread.removeQuestion(question.session_id);
    }
    catch (error) { this.thread.messages.push({ role: "note", text: `回答问题失败：${error}` }); }
  };
}
