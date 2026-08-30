import { invoke } from "@tauri-apps/api/core";
import { appServerRequest, threadToSessionRow, type ProtocolThread, type SessionRow } from "./app-server-client";
import type { ThreadController } from "./thread-controller.svelte";
import type { UsageController } from "./usage-controller.svelte";

export class ThreadLifecycleController {
  sessions = $state<SessionRow[]>([]);
  showArchived = $state(false);
  historyOpen = $state(false);
  selectedHarnessProfile = $state("full");
  activeHarnessProfile = $state("full");
  harnessProfileMenuOpen = $state(false);

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
  ) {}

  get recentSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => !session.archived)); }
  get archivedSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => session.archived)); }
  get archivedCount(): number { return this.sessions.filter((session) => session.archived).length; }
  get harnessProfileLocked(): boolean {
    const current = this.sessions.find((session) => session.session_id === this.thread.currentId);
    return Boolean(current?.has_snapshot || this.thread.messages.some((message) => message.role === "user"));
  }
  harnessProfileLabel = (id: string): string => this.harnessProfiles.find((profile) => profile.id === id)?.label || id;

  selectHarnessProfile = async (profile: string): Promise<void> => {
    this.harnessProfileMenuOpen = false;
    if (this.harnessProfileLocked) {
      this.thread.messages.push({ role: "note", text: "Profile 决定工具和上下文；本会话已有消息，已锁定。请新建会话后切换。" });
      return;
    }
    if (!this.thread.currentId) {
      this.selectedHarnessProfile = profile;
      this.activeHarnessProfile = profile;
      return;
    }
    try {
      await appServerRequest({ method: "threadHarnessProfileSet", params: { threadId: this.thread.currentId, harnessProfile: profile } });
      // Rebuild the empty active Thread immediately so its first turn uses the
      // persisted composition instead of the profile used at creation time.
      await appServerRequest({ method: "threadActivate", params: { threadId: this.thread.currentId } });
      this.selectedHarnessProfile = profile;
      this.activeHarnessProfile = profile;
      const current = this.sessions.find((session) => session.session_id === this.thread.currentId);
      if (current) current.harness_profile = profile;
    } catch (error) {
      this.thread.messages.push({ role: "note", text: `切换 Harness Profile 失败：${error}` });
    }
  };

  refresh = async (): Promise<void> => {
    try {
      const metadata = await appServerRequest<{ id: string }[]>({ method: "threadList", params: { includeArchived: true } });
      const results = await Promise.allSettled(metadata.slice(0, 50).map((item) =>
        appServerRequest<ProtocolThread>({ method: "threadReadVisible", params: { threadId: item.id } })));
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
    await appServerRequest({ method: "threadRename", params: { threadId: id, title } });
    const session = this.sessions.find((item) => item.session_id === id);
    if (session) session.title = title;
    if (this.thread.currentId === id) this.thread.title = title;
    await this.refresh();
  };

  create = async (): Promise<void> => {
    if (this.thread.switching) return;
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
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    this.thread.stash(previousId);
    this.thread.switching = true;
    this.thread.title = title || "会话";
    this.thread.currentId = id;
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
        this.thread.messages = visible.turns.flatMap((turn) => turn.items.flatMap((item) => {
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
      this.thread.busy = false; this.thread.stopping = false; this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.usage.restore(previousId);
      this.thread.restore(previousId);
      this.thread.messages.push({ role: "note", text: `继续会话失败：${error}` });
    }
  };

  fork = async (id: string, title = ""): Promise<void> => {
    if (this.thread.switching) return;
    const previousId = this.thread.currentId;
    const previousTitle = this.thread.title;
    this.thread.stash(previousId);
    this.thread.switching = true; this.thread.busy = false;
    const forkTitle = this.nextForkTitle(title || "分叉会话");
    this.thread.title = forkTitle; this.thread.currentId = ""; this.usage.reset();
    try {
      const newThreadId = this.newThreadId();
      const forked = await appServerRequest<ProtocolThread>({ method: "threadForkActivate", params: { threadId: id, newThreadId } });
      this.activeHarnessProfile = forked.metadata.harnessProfile || "full";
      this.selectedHarnessProfile = this.activeHarnessProfile;
      await appServerRequest({ method: "threadRename", params: { threadId: newThreadId, title: forkTitle } });
      this.thread.currentId = newThreadId; this.thread.title = forkTitle;
      this.thread.switching = false;
      await this.refresh();
    } catch (error) {
      this.thread.busy = false; this.thread.stopping = false; this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.usage.restore(previousId); this.thread.restore(previousId);
      this.thread.messages.push({ role: "note", text: `分叉失败：${error}` });
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

  private pinActive(sessions: SessionRow[]): SessionRow[] {
    if (!this.thread.currentId) return sessions;
    return [...sessions.filter((item) => item.session_id === this.thread.currentId), ...sessions.filter((item) => item.session_id !== this.thread.currentId)];
  }

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
