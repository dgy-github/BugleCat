import { appServerRequest, type ProtocolGoalView } from "./app-server-client";
import type { ThreadController } from "./thread-controller.svelte";

export class GoalController {
  view = $state<ProtocolGoalView | null>(null);
  menuOpen = $state(false);
  loading = $state(false);
  private generation = 0;

  constructor(private readonly thread: ThreadController) {}

  get remainingRounds(): number {
    return this.view ? Math.max(0, this.view.goal.maxGoalRounds - this.view.goal.roundsStarted) : 0;
  }

  get statusLabel(): string {
    if (!this.view) return "";
    const phase = { active: "进行中", paused: "已暂停", blocked: "已阻塞", complete: "已完成" }[this.view.goal.phase];
    return `${phase} · ${this.view.goal.roundsStarted}/${this.view.goal.maxGoalRounds}`;
  }

  clear = (): void => {
    this.generation += 1;
    this.view = null;
    this.menuOpen = false;
    this.loading = false;
  };

  private readWithNewThreadRetry = async (threadId: string): Promise<ProtocolGoalView | null> => {
    try {
      return await appServerRequest<ProtocolGoalView | null>({ method: "goalRead", params: { threadId } });
    } catch (error) {
      // A newly-created thread can become visible to Svelte one microtask before
      // the durable app-server snapshot is readable. Retry that narrow race once;
      // unrelated Goal/store failures must still reach the visible error path.
      if (!String(error).includes(`${threadId} was not found`)) throw error;
      await new Promise((resolve) => window.setTimeout(resolve, 120));
      return appServerRequest<ProtocolGoalView | null>({ method: "goalRead", params: { threadId } });
    }
  };

  refresh = async (threadId = this.thread.currentId): Promise<void> => {
    if (!threadId) { this.clear(); return; }
    const generation = ++this.generation;
    this.loading = true;
    try {
      const view = await this.readWithNewThreadRetry(threadId);
      if (generation === this.generation && threadId === this.thread.currentId) this.view = view;
    } catch (error) {
      if (generation === this.generation && threadId === this.thread.currentId) {
        this.thread.messages.push({ role: "note", text: `读取长期目标失败：${error}` });
      }
    } finally {
      if (generation === this.generation) this.loading = false;
    }
  };

  pause = async (): Promise<void> => this.transition("goalPause", false);

  resume = async (): Promise<void> => {
    if (!this.view) return;
    const confirmed = window.confirm(
      `继续长期目标将允许在当前模型商上自动执行后续轮次，最多还可执行 ${this.remainingRounds} 轮，可能产生模型费用。是否继续？`,
    );
    if (!confirmed) return;
    await this.transition("goalResume", true);
  };

  private transition = async (method: "goalPause" | "goalResume", arm: boolean): Promise<void> => {
    const current = this.view;
    const threadId = this.thread.currentId;
    if (!current || !threadId || this.loading) return;
    this.loading = true;
    this.menuOpen = false;
    try {
      const next = await appServerRequest<ProtocolGoalView>({
        method,
        params: {
          threadId,
          goal: { id: current.goal.id, revision: current.goal.revision },
        },
      });
      if (threadId === this.thread.currentId) {
        this.view = next;
        this.thread.messages.push({
          role: "note",
          text: arm ? "长期目标已继续；自动续轮仍受轮数、取消、权限和费用边界限制。" : "长期目标已暂停，不会继续自动执行。",
        });
      }
    } catch (error) {
      if (threadId === this.thread.currentId) {
        this.thread.messages.push({ role: "note", text: `更新长期目标失败：${error}` });
        await this.refresh(threadId);
      }
    } finally {
      this.loading = false;
    }
  };
}
