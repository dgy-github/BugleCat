import { invoke } from "@tauri-apps/api/core";
import { appServerRequest, threadToSessionRow, type ProtocolThread, type SessionRow } from "./app-server-client";
import type { ThreadController } from "./thread-controller.svelte";
import type { UsageController } from "./usage-controller.svelte";

export class ThreadLifecycleController {
  sessions = $state<SessionRow[]>([]);
  showRecent = $state(false);
  showArchived = $state(false);
  historyOpen = $state(false);

  constructor(
    private readonly thread: ThreadController,
    private readonly usage: UsageController,
    private readonly workspace: () => string,
  ) {}

  get recentSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => !session.archived)); }
  get archivedSessions(): SessionRow[] { return this.pinActive(this.sessions.filter((session) => session.archived)); }
  get archivedCount(): number { return this.sessions.filter((session) => session.archived).length; }

  refresh = async (): Promise<void> => {
    try {
      const metadata = await appServerRequest<{ id: string }[]>({ method: "threadList", params: { includeArchived: true } });
      const threads = await Promise.all(metadata.slice(0, 50).map((item) =>
        appServerRequest<ProtocolThread>({ method: "threadReadVisible", params: { threadId: item.id } })));
      this.usage.replaceProtocolUsage(threads.map((item) => [item.metadata.id, item.turns.reduce(
        (sum, turn) => ({
          prompt_tokens: sum.prompt_tokens + (turn.usage?.tokens?.prompt_tokens || 0),
          completion_tokens: sum.completion_tokens + (turn.usage?.tokens?.completion_tokens || 0),
        }), { prompt_tokens: 0, completion_tokens: 0 },
      )]));
      this.sessions = threads.map(threadToSessionRow);
      const current = this.sessions.find((session) => session.session_id === this.thread.currentId);
      if (current) { this.thread.title = current.title || "会话"; this.usage.restore(this.thread.currentId); }
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
      await appServerRequest<ProtocolThread>({ method: "threadCreateActivate", params: { threadId: id, workspace: this.workspace(), title: "(no prompt yet)" } });
      if (this.thread.currentId === "") { this.thread.currentId = id; this.thread.restore(id); }
    } catch (error) {
      this.thread.busy = false; this.thread.stopping = false; this.thread.switching = false;
      this.thread.currentId = previousId; this.thread.title = previousTitle; this.thread.restore(previousId);
      this.thread.messages = previousMessages; this.usage.restore(this.thread.currentId);
      this.thread.messages.push({ role: "note", text: `新建会话失败：${error}` });
    }
  };

  resume = async (id: string, title = ""): Promise<void> => {
    if (this.thread.switching || id === this.thread.currentId) return;
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
      await appServerRequest({ method: "threadActivate", params: { threadId: id } });
    } catch (error) {
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
    this.thread.title = title ? `${title}（分叉）` : "分叉"; this.thread.currentId = ""; this.usage.reset();
    try {
      await appServerRequest<ProtocolThread>({ method: "threadForkActivate", params: { threadId: id, newThreadId: this.newThreadId() } });
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

  private openSessionResource = async (command: string, id: string, label: string): Promise<void> => {
    try { await invoke(command, { sessionId: id }); }
    catch (error) { this.thread.messages.push({ role: "note", text: `${label}：${error}` }); }
  };
}
