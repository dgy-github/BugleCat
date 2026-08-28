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

  constructor(
    private readonly sidebar: SidebarController,
    private readonly thread: ThreadController,
    private readonly lifecycle: ThreadLifecycleController,
    private readonly composer: ComposerController,
    private readonly models: ModelControlsController,
    private readonly usage: UsageController,
    private readonly slash: SlashController,
  ) {}

  get workspaceName(): string {
    return this.workspace ? this.workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || this.workspace : "";
  }

  start = async (): Promise<void> => {
    this.sidebar.restoreWidth();
    try {
      const status = await appServerRequest<{ model: string; sandbox: string; approval: string; permission_mode: string; reasoning_effort: string; price_in: number; price_out: number; price_currency: "CNY" | "USD" }>({ method: "runtimeStatusRead" });
      this.header = `${status.model} · ${status.sandbox}`;
      this.sandboxMode = status.sandbox;
      this.models.currentModel = status.model;
      if (status.permission_mode) this.models.permissionMode = status.permission_mode;
      if (status.reasoning_effort) this.models.reasoningEffort = status.reasoning_effort;
      this.usage.setPrice(status.price_in, status.price_out, status.price_currency);
    } catch { this.header = "配置错误"; }
    void this.lifecycle.refresh();
    await listen<ProtocolEventEnvelope>("ncx://protocol-event", (message) => {
      const envelope = message.payload;
      if (!this.sequenceGate.accept(envelope)) return;
      if (["threadCreated", "threadUpdated", "turnCompleted"].includes(envelope.event.type)) void this.lifecycle.refresh();
    });
    await listen<UiEvent>("ncx://event", (event) => this.thread.handle(event.payload));
    appServerRequest({ method: "runtimeReadyRefresh" }).catch(() => {});
    void this.slash.loadCustomCommands();
  };

  handleReady = (event: Extract<UiEvent, { kind: "ready" }>): void => {
    const routeChanged = this.models.currentProvider !== event.provider_id || this.models.currentProtocol !== event.provider_protocol;
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
    if (routeChanged || this.models.routes.length === 0) void this.models.refreshRoutes();
  };

  chooseWorkspace = async (): Promise<void> => {
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    const previousMessages = [...this.thread.messages];
    try {
      const directory = await open({ directory: true, multiple: false });
      if (!directory || Array.isArray(directory)) return;
      this.thread.currentId = "";
      this.thread.messages = [];
      this.thread.title = "新会话";
      this.usage.reset();
      this.thread.queued = [];
      this.composer.attached = [];
      const workspace = await appServerRequest<string>({ method: "workspaceSet", params: { path: directory } });
      this.workspace = workspace;
      this.thread.messages.push({ role: "note", text: `已切换工作区到 ${workspace}，已开始新会话。` });
      void this.lifecycle.refresh();
    } catch (error) {
      this.thread.currentId = previousId;
      this.thread.title = previousTitle;
      this.thread.messages = previousMessages;
      this.usage.restore(previousId);
      this.thread.messages.push({ role: "note", text: `切换工作区失败：${error}` });
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
