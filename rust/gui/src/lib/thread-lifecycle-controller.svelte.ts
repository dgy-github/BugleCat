import { invoke } from "@tauri-apps/api/core";
import { appServerRequest, threadToSessionRow, type ProtocolThread, type SessionRow } from "./app-server-client";
import type { ConversationMessage } from "./conversation-model";
import type { ThreadController } from "./thread-controller.svelte";
import type { UsageController } from "./usage-controller.svelte";

type WorkspaceRecovery = {
  restore: (workspace: string) => Promise<string>;
  reconcile: () => Promise<string>;
};

export class ThreadLifecycleController {
  sessions = $state<SessionRow[]>([]);
  showArchived = $state(false);
  historyOpen = $state(false);
  selectedHarnessProfile = $state("full");
  activeHarnessProfile = $state("full");
  harnessProfileMenuOpen = $state(false);
  // Refreshes fan out into one list request plus many thread reads. Keep only
  // the newest projection: a slow response from an earlier workspace/session
  // event must not overwrite a newer sidebar snapshot.
  private refreshGeneration = 0;
  private profileSelectionGeneration = 0;
  private profileSelectionQueue: Promise<void> = Promise.resolve();
  // Workspace activation changes a process-global CWD. Associate recovery
  // with the navigation that initiated it so a late failure cannot overwrite a
  // newer binding.
  private navigationGeneration = 0;

  readonly harnessProfiles = [
    { id: "full", label: "全功能", desc: "完整工具与上下文，适合复杂任务" },
    { id: "coding", label: "编程", desc: "面向代码开发的工具组合" },
    { id: "readonly", label: "只读", desc: "仅分析与读取，不修改工作区" },
    { id: "minimal", label: "轻量", desc: "减少工具和上下文，响应更轻" },
    { id: "headless", label: "自动化", desc: "适合无界面批处理与流水线" },
  ];

  constructor(
    private readonly thread: ThreadController,
    private readonly usage: UsageController,
    private readonly workspace: () => string,
    private readonly workspaceRecovery: WorkspaceRecovery,
  ) {}

  get recentSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => !session.archived)); }
  get archivedSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => session.archived)); }
  get archivedCount(): number { return this.sessions.filter((session) => session.archived).length; }
  get harnessProfileLocked(): boolean {
    const current = this.sessions.find((session) => session.session_id === this.thread.currentId);
    return Boolean(current?.has_snapshot || this.thread.messages.some((message) => message.role === "user"));
  }
  harnessProfileLabel = (id: string): string => this.harnessProfiles.find((profile) => profile.id === id)?.label || id;

  // The app server changes its working directory before creating the new
  // session. Drop the old workspace projection immediately so a refresh that
  // started under the previous directory cannot repopulate the sidebar or
  // usage map after the switch.
  workspaceChanged = (): void => {
    this.invalidateRefresh();
    this.invalidateProfileSelection();
    this.sessions = [];
    this.usage.replaceProtocolUsage([]);
    this.usage.reset();
  };

  reset = this.workspaceChanged;

  selectHarnessProfile = async (profile: string): Promise<void> => {
    this.harnessProfileMenuOpen = false;
    const selection = ++this.profileSelectionGeneration;
    if (this.harnessProfileLocked) {
      this.thread.messages.push({ role: "note", text: "Profile 决定工具和上下文；本会话已有消息，已锁定。请新建会话后切换。" });
      return;
    }
    const threadId = this.thread.currentId;
    if (!threadId) {
      this.selectedHarnessProfile = profile;
      this.activeHarnessProfile = profile;
      return;
    }
    this.invalidateRefresh();
    await this.enqueueProfileSelection(async () => {
      if (!this.isCurrentProfileSelection(threadId, selection)) return;
      try {
        await appServerRequest({ method: "threadHarnessProfileSet", params: { threadId, harnessProfile: profile } });
        if (!this.isCurrentProfileSelection(threadId, selection)) return;
        // Rebuild the empty active Thread immediately so its first turn uses the
        // persisted composition instead of the profile used at creation time.
        await appServerRequest({ method: "threadActivate", params: { threadId } });
        if (!this.isCurrentProfileSelection(threadId, selection)) return;
        this.selectedHarnessProfile = profile;
        this.activeHarnessProfile = profile;
        const current = this.sessions.find((session) => session.session_id === threadId);
        if (current) current.harness_profile = profile;
      } catch (error) {
        if (this.isCurrentProfileSelection(threadId, selection)) {
          this.thread.messages.push({ role: "note", text: `切换 Harness Profile 失败：${error}` });
        }
      }
    });
  };

  refresh = async (): Promise<void> => {
    const generation = ++this.refreshGeneration;
    try {
      const metadata = await appServerRequest<{ id: string }[]>({ method: "threadList", params: { includeArchived: true } });
      const results = await Promise.allSettled(metadata.slice(0, 50).map((item) =>
        appServerRequest<ProtocolThread>({ method: "threadReadVisible", params: { threadId: item.id } })));
      if (generation !== this.refreshGeneration) return;
      const threads = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
      this.usage.replaceProtocolUsage(threads.map((item) => [item.metadata.id, item.turns.reduce(
        (sum, turn) => ({
          prompt_tokens: sum.prompt_tokens + (turn.usage?.tokens?.prompt_tokens || 0),
          completion_tokens: sum.completion_tokens + (turn.usage?.tokens?.completion_tokens || 0),
        }), { prompt_tokens: 0, completion_tokens: 0 },
      )]));
      this.sessions = threads.map(threadToSessionRow);
      const current = this.sessions.find((session) => session.session_id === this.thread.currentId);
      if (current) {
        this.thread.title = current.title || "会话";
        this.activeHarnessProfile = current.harness_profile || "full";
        this.selectedHarnessProfile = this.activeHarnessProfile;
        this.usage.restore(this.thread.currentId);
      }
    } catch (error) { console.error("会话协议加载失败", error); }
  };

  archive = async (id: string, archived: boolean): Promise<void> => {
    this.invalidateRefresh();
    try {
      await appServerRequest({ method: "threadArchive", params: { threadId: id, archived } });
      const session = this.sessions.find((item) => item.session_id === id);
      if (session) session.archived = archived;
      void this.refresh();
    } catch (error) { this.thread.messages.push({ role: "note", text: `归档失败：${error}` }); }
  };

  rename = async (id: string, value: string): Promise<void> => {
    const title = value.trim().replace(/\s+/g, " ");
    if (!title) throw new Error("会话名称不能为空");
    if ([...title].length > 36) throw new Error("会话名称不能超过 36 个字符");
    this.invalidateRefresh();
    await appServerRequest({ method: "threadRename", params: { threadId: id, title } });
    const session = this.sessions.find((item) => item.session_id === id);
    if (session) session.title = title;
    if (this.thread.currentId === id) this.thread.title = title;
    await this.refresh();
  };

  create = async (): Promise<void> => {
    if (this.thread.switching) return;
    this.invalidateRefresh();
    this.invalidateProfileSelection();
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    const previousMessages = [...this.thread.messages];
    this.thread.stash(previousId);
    this.thread.switching = true;
    this.thread.busy = false;
    this.thread.messages = [];
    this.thread.title = "新会话";
    this.thread.currentId = "";
    this.usage.reset();
    try {
      const id = this.newThreadId();
      this.thread.expectReady(id);
      const created = await appServerRequest<ProtocolThread>({ method: "threadCreateActivate", params: { threadId: id, workspace: this.workspace(), title: "(no prompt yet)", harnessProfile: this.selectedHarnessProfile } });
      this.activeHarnessProfile = created.metadata.harnessProfile || this.selectedHarnessProfile;
      if (this.thread.currentId === "") { this.thread.currentId = id; this.thread.restore(id); }
      this.thread.switching = false;
    } catch (error) {
      this.thread.busy = false; this.thread.stopping = false; this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.thread.restore(previousId);
      this.thread.messages = previousMessages; this.usage.restore(this.thread.currentId);
      this.thread.messages.push({ role: "note", text: `新建会话失败：${error}` });
    }
  };

  resume = async (id: string, title = ""): Promise<void> => {
    if (this.thread.switching) return;
    this.invalidateRefresh();
    this.invalidateProfileSelection();
    const navigation = this.beginNavigation();
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    const previousWorkspace = this.workspace();
    this.thread.stash(previousId);
    this.thread.switching = true;
    this.thread.title = title || "会话";
    this.thread.currentId = id;
    this.thread.expectReady(id);
    this.usage.restore(id);
    this.thread.restore(id);
    try {
      this.thread.busy = this.thread.runningSessions.has(id);
      // Activation emits a legacy snapshot through `loaded`. Suppress that one
      // event, then make the protocol Thread the final authority so a stale
      // snapshot cannot erase later durable turns (for example, Goal worker
      // rounds completed after the last legacy snapshot write).
      this.thread.skipNextLoaded(id);
      await appServerRequest({ method: "threadActivate", params: { threadId: id } });
      const visible = await appServerRequest<ProtocolThread>({ method: "threadReadVisible", params: { threadId: id } });
      this.activeHarnessProfile = visible.metadata.harnessProfile || "full";
      this.selectedHarnessProfile = this.activeHarnessProfile;
      if (!this.thread.busy || this.thread.messages.length === 0) {
        this.thread.messages = visible.turns.flatMap((turn): ConversationMessage[] => turn.items.flatMap((item): ConversationMessage[] => {
          if (item.type === "userMessage") return [{ role: "user" as const, text: item.text }];
          if (item.type === "assistantMessage") return [{
            role: "assistant" as const,
            text: item.text,
            model: item.model,
            confirmedModel: item.confirmedModel,
          }];
          if (item.type === "artifact") return [{ role: "artifact" as const, kind: item.kind, name: item.name, url: item.url }];
          return [];
        }));
      }
      this.thread.switching = false;
    } catch (error) {
      this.thread.clearSkippedLoaded(id);
      const recovery = await this.restoreWorkspaceAfterNavigationFailure(navigation, id, previousWorkspace);
      if (!recovery.current) return;
      if (!recovery.restored) {
        this.leaveNavigationUnbound(
          navigation,
          id,
          `继续会话失败：${error}${recovery.detail}。当前未绑定会话，请新建会话后重试。`,
        );
        return;
      }
      if (previousId) this.thread.expectReady(previousId);
      else this.thread.sealReadyFence();
      this.thread.busy = false;
      this.thread.stopping = false;
      this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.usage.restore(previousId);
      this.thread.restore(previousId);
      this.thread.messages.push({ role: "note", text: `继续会话失败：${error}` });
      void this.refresh();
    }
  };

  fork = async (id: string, title = ""): Promise<void> => {
    if (this.thread.switching) return;
    this.invalidateRefresh();
    this.invalidateProfileSelection();
    const navigation = this.beginNavigation();
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    const previousWorkspace = this.workspace();
    this.thread.stash(previousId);
    this.thread.switching = true; this.thread.busy = false;
    const forkTitle = this.nextForkTitle(title || "分叉会话");
    const newThreadId = this.newThreadId();
    this.thread.title = forkTitle; this.thread.currentId = ""; this.usage.reset();
    try {
      this.thread.expectReady(newThreadId);
      const forked = await appServerRequest<ProtocolThread>({ method: "threadForkActivate", params: { threadId: id, newThreadId } });
      this.activeHarnessProfile = forked.metadata.harnessProfile || "full";
      this.selectedHarnessProfile = this.activeHarnessProfile;
      await appServerRequest({ method: "threadRename", params: { threadId: newThreadId, title: forkTitle } });
      this.thread.currentId = newThreadId; this.thread.title = forkTitle;
      this.thread.switching = false;
      await this.refresh();
    } catch (error) {
      const recovery = await this.restoreWorkspaceAfterNavigationFailure(navigation, newThreadId, previousWorkspace);
      if (!recovery.current) return;
      if (!recovery.restored) {
        this.leaveNavigationUnbound(
          navigation,
          newThreadId,
          `分叉会话失败：${error}${recovery.detail}。当前未绑定会话，请新建会话后重试。`,
        );
        return;
      }
      if (previousId) this.thread.expectReady(previousId);
      else this.thread.sealReadyFence();
      this.thread.busy = false;
      this.thread.stopping = false;
      this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.usage.restore(previousId); this.thread.restore(previousId);
      this.thread.messages.push({ role: "note", text: `分叉失败：${error}` });
      void this.refresh();
    }
  };

  openLog = async (id: string): Promise<void> => this.openSessionResource("open_session_log", id, "打开会话日志失败");
  openSnapshot = async (id: string): Promise<void> => this.openSessionResource("open_session_snapshot", id, "打开会话快照失败");

  formatWhen = (stamp: string): string => {
    const time = /^\d+$/.test(stamp) ? Number(stamp) : Date.parse(stamp);
    if (!time || Number.isNaN(time)) return "";
    const difference = Date.now() - time;
    if (difference < 60_000) return "刚刚";
    if (difference < 3_600_000) return `${Math.floor(difference / 60_000)} 分钟前`;
    if (difference < 86_400_000) return `${Math.floor(difference / 3_600_000)} 小时前`;
    const date = new Date(time); const pad = (value: number) => String(value).padStart(2, "0");
    return `${date.getMonth() + 1}/${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
  };

  private beginNavigation = (): number => {
    this.navigationGeneration += 1;
    return this.navigationGeneration;
  };

  private ownsNavigation = (generation: number, expectedId: string): boolean =>
    generation === this.navigationGeneration
      && this.thread.switching
      && (this.thread.currentId === expectedId || this.thread.currentId === "");

  private restoreWorkspaceAfterNavigationFailure = async (
    generation: number,
    expectedId: string,
    previousWorkspace: string,
  ): Promise<{ current: boolean; restored: boolean; detail: string }> => {
    if (!this.ownsNavigation(generation, expectedId)) return { current: false, restored: false, detail: "" };
    this.thread.sealReadyFence();
    if (previousWorkspace) {
      try {
        await this.workspaceRecovery.restore(previousWorkspace);
        return {
          current: this.ownsNavigation(generation, expectedId),
          restored: this.ownsNavigation(generation, expectedId),
          detail: "",
        };
      } catch (rollback) {
        return this.reconcileFailedWorkspaceRollback(generation, expectedId, `；恢复原工作区失败：${rollback}`);
      }
    }
    return this.reconcileFailedWorkspaceRollback(generation, expectedId, "；原工作区不可用，无法回滚");
  };

  private reconcileFailedWorkspaceRollback = async (
    generation: number,
    expectedId: string,
    detail: string,
  ): Promise<{ current: boolean; restored: false; detail: string }> => {
    if (!this.ownsNavigation(generation, expectedId)) return { current: false, restored: false, detail: "" };
    try {
      const workspace = await this.workspaceRecovery.reconcile();
      const suffix = workspace ? `；当前工作区：${workspace}` : "；当前工作区未能确认";
      return { current: this.ownsNavigation(generation, expectedId), restored: false, detail: `${detail}${suffix}` };
    } catch {
      return {
        current: this.ownsNavigation(generation, expectedId),
        restored: false,
        detail: `${detail}；当前工作区未能确认，已禁止继续操作`,
      };
    }
  };

  private leaveNavigationUnbound = (
    generation: number,
    expectedId: string,
    message: string,
  ): void => {
    if (!this.ownsNavigation(generation, expectedId)) return;
    // Keep the ready fence sealed: a delayed Ready from the old session must
    // never bind this UI after the backend stayed in a different workspace.
    this.thread.sealReadyFence();
    this.thread.busy = false;
    this.thread.stopping = false;
    this.thread.switching = false;
    this.thread.currentId = "";
    this.thread.title = "新会话";
    this.thread.queued = [];
    this.thread.messages = [{ role: "note", text: message }];
    this.usage.reset();
    void this.refresh();
  };

  private pinActive(sessions: SessionRow[]): SessionRow[] {
    if (!this.thread.currentId) return sessions;
    return [...sessions.filter((item) => item.session_id === this.thread.currentId), ...sessions.filter((item) => item.session_id !== this.thread.currentId)];
  }

  private invalidateRefresh = (): void => {
    this.refreshGeneration += 1;
  };

  private invalidateProfileSelection = (): void => {
    this.profileSelectionGeneration += 1;
  };

  private enqueueProfileSelection = (task: () => Promise<void>): Promise<void> => {
    const next = this.profileSelectionQueue.then(task, task);
    // Keep the queue usable even if a future task has an unexpected failure.
    this.profileSelectionQueue = next.catch(() => {});
    return next;
  };

  private isCurrentProfileSelection = (threadId: string, selection: number): boolean =>
    this.thread.currentId === threadId && selection === this.profileSelectionGeneration;

  private newThreadId(): string { return `thread-${crypto.randomUUID()}`; }

  private nextForkTitle(source: string): string {
    const base = source.replace(/\s+\(\d+\)$/u, "").trim() || "分叉会话";
    const titles = new Set(this.sessions.map((session) => session.title));
    let index = 1;
    while (titles.has(`${base} (${index})`)) index += 1;
    return `${base} (${index})`;
  }

  private openSessionResource = async (command: string, id: string, label: string): Promise<void> => {
    try { await invoke(command, { sessionId: id }); }
    catch (error) { this.thread.messages.push({ role: "note", text: `${label}：${error}` }); }
  };
}
