import { appServerRequest } from "./app-server-client";

export type ForgeRuntimeStatus = { available: boolean; schema?: string; reason?: string };
export type ForgeJobSummary = {
  mode: string;
  rounds: number;
  acceptedRounds: number;
  championTrain?: number | null;
  championHoldout?: number | null;
  testBaseline?: number | null;
  testChampion?: number | null;
  testRuns?: number | null;
  reportFile: string;
};
export type ForgeJobStatus = {
  generation: number;
  status: string;
  startedAtMs?: number | null;
  rounds?: number | null;
  repeats?: number | null;
  timeoutS?: number | null;
  budgetS?: number | null;
  teacher?: string | null;
  acceptMargin?: number | null;
  summary?: ForgeJobSummary | null;
  error?: string | null;
};

export class ForgeController {
  runtime = $state<ForgeRuntimeStatus | null>(null);
  job = $state<ForgeJobStatus | null>(null);
  loading = $state(false);
  rounds = $state(1);
  repeats = $state(1);
  timeoutS = $state(120);
  budgetS = $state(600);
  teacher = $state("panel");
  acceptMargin = $state(1);
  private lifecycleGeneration = 0;
  private pollGeneration = 0;
  private nextLoadingOperation = 0;
  private readonly activeLoadingOperations = new Set<number>();

  constructor(
    private readonly notify: (message: string) => void,
    private readonly workspace: () => string,
  ) {}

  // The Forge job is process-owned and intentionally keeps running when this
  // view goes away. Only its UI observer is disposed, so a remount can read
  // the durable status without an old poll mutating the new view.
  dispose = (): void => {
    this.lifecycleGeneration += 1;
    this.pollGeneration += 1;
    this.activeLoadingOperations.clear();
    this.loading = false;
  };

  // Forge jobs belong to the process, but their status projection belongs to
  // the selected workspace. Invalidate every observer from the old workspace
  // and clear its projection without cancelling the process-owned job.
  workspaceChanged = (): void => {
    this.lifecycleGeneration += 1;
    this.pollGeneration += 1;
    this.activeLoadingOperations.clear();
    this.loading = false;
    this.runtime = null;
    this.job = null;
    // The new workspace has already been committed before this callback. Start
    // a fresh, generation-fenced projection so Forge controls do not remain
    // disabled until the user manually opens the panel and clicks Refresh.
    if (this.workspace()) void this.refresh();
  };

  refresh = async (): Promise<void> => {
    const workspace = this.workspace();
    if (!workspace) {
      this.notify("Forge 状态读取失败：当前工作区不可用，请重新打开项目");
      return;
    }
    const lifecycle = this.lifecycleGeneration;
    const operation = this.beginLoading();
    const poll = ++this.pollGeneration;
    try {
      const [runtime, job] = await Promise.all([
        appServerRequest<ForgeRuntimeStatus>({ method: "forgeRuntimeStatusRead" }),
        appServerRequest<ForgeJobStatus>({
          method: "forgeJobStatusRead",
          params: { workspace },
        }),
      ]);
      if (!this.isActive(lifecycle, operation)) return;
      this.runtime = runtime;
      this.job = job;
      if (job.status === "running" || job.status === "cancelling") {
        void this.poll(job.generation, lifecycle, poll, workspace);
      }
    } catch (error) {
      if (this.isActive(lifecycle, operation)) this.notify(`Forge 状态读取失败：${error}`);
    } finally { this.endLoading(operation); }
  };

  start = async (): Promise<void> => {
    if (!this.runtime?.available) {
      this.notify(`Forge 运行时不可用：${this.runtime?.reason || "请安装完整版本"}`);
      return;
    }
    const confirmed = window.confirm(
      `Forge 会执行多轮模型调用并可能产生费用。确认开始 ${this.rounds} 轮训练，最长 ${this.budgetS} 秒吗？`,
    );
    if (!confirmed) return;
    const workspace = this.workspace();
    if (!workspace) {
      this.notify("Forge 启动失败：当前工作区不可用，请重新打开项目");
      return;
    }
    const lifecycle = this.lifecycleGeneration;
    const operation = this.beginLoading();
    const poll = ++this.pollGeneration;
    try {
      const job = await appServerRequest<ForgeJobStatus>({
        method: "forgeJobStart",
        params: {
          workspace,
          rounds: this.rounds,
          repeats: this.repeats,
          timeoutS: this.timeoutS,
          budgetS: this.budgetS,
          teacher: this.teacher,
          acceptMargin: this.acceptMargin,
        },
      });
      if (!this.isActive(lifecycle, operation)) return;
      this.job = job;
      void this.poll(job.generation, lifecycle, poll, workspace);
    } catch (error) {
      if (this.isActive(lifecycle, operation)) this.notify(`Forge 启动失败：${error}`);
    } finally { this.endLoading(operation); }
  };

  cancel = async (): Promise<void> => {
    const lifecycle = this.lifecycleGeneration;
    const poll = ++this.pollGeneration;
    const workspace = this.workspace();
    const generation = this.job?.generation;
    if (!workspace || generation === undefined) return;
    try {
      const job = await appServerRequest<ForgeJobStatus>({
        method: "forgeJobCancel",
        params: { workspace, generation },
      });
      if (!this.isCurrentPoll(lifecycle, poll)) return;
      this.job = job;
      if (job.status === "running" || job.status === "cancelling") {
        void this.poll(job.generation, lifecycle, poll, workspace);
      }
    } catch (error) {
      if (this.isCurrentPoll(lifecycle, poll)) this.notify(`Forge 取消失败：${error}`);
    }
  };

  private poll = async (generation: number, lifecycle: number, poll: number, workspace: string): Promise<void> => {
    while (true) {
      await new Promise((resolve) => setTimeout(resolve, 700));
      if (!this.isCurrentPoll(lifecycle, poll)) return;
      try {
        const status = await appServerRequest<ForgeJobStatus>({
          method: "forgeJobStatusRead",
          params: { workspace, generation },
        });
        if (!this.isCurrentPoll(lifecycle, poll)) return;
        if (status.generation !== generation) return;
        this.job = status;
        if (status.status === "running" || status.status === "cancelling") continue;
        if (status.status === "completed") this.notify("Forge 训练完成，安全摘要已生成。");
        else if (status.status === "cancelled") this.notify("Forge 训练已取消，完整进程树已停止。");
        else this.notify(`Forge 未完成：${status.error || status.status}`);
        return;
      } catch (error) {
        if (this.isCurrentPoll(lifecycle, poll)) this.notify(`Forge 状态轮询失败：${error}`);
        return;
      }
    }
  };

  private beginLoading = (): number => {
    const operation = ++this.nextLoadingOperation;
    this.activeLoadingOperations.add(operation);
    this.loading = true;
    return operation;
  };

  private endLoading = (operation: number): void => {
    if (!this.activeLoadingOperations.delete(operation)) return;
    this.loading = this.activeLoadingOperations.size > 0;
  };

  private isActive = (lifecycle: number, operation: number): boolean =>
    lifecycle === this.lifecycleGeneration && this.activeLoadingOperations.has(operation);

  private isCurrentPoll = (lifecycle: number, poll: number): boolean =>
    lifecycle === this.lifecycleGeneration && poll === this.pollGeneration;
}
