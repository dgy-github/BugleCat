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

  constructor(private readonly notify: (message: string) => void) {}

  refresh = async (): Promise<void> => {
    this.loading = true;
    try {
      const [runtime, job] = await Promise.all([
        appServerRequest<ForgeRuntimeStatus>({ method: "forgeRuntimeStatusRead" }),
        appServerRequest<ForgeJobStatus>({ method: "forgeJobStatusRead" }),
      ]);
      this.runtime = runtime;
      this.job = job;
      if (job.status === "running" || job.status === "cancelling") void this.poll(job.generation);
    } catch (error) {
      this.notify(`Forge 状态读取失败：${error}`);
    } finally {
      this.loading = false;
    }
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
    this.loading = true;
    try {
      this.job = await appServerRequest<ForgeJobStatus>({
        method: "forgeJobStart",
        params: {
          rounds: this.rounds,
          repeats: this.repeats,
          timeoutS: this.timeoutS,
          budgetS: this.budgetS,
          teacher: this.teacher,
          acceptMargin: this.acceptMargin,
        },
      });
      void this.poll(this.job.generation);
    } catch (error) {
      this.notify(`Forge 启动失败：${error}`);
    } finally {
      this.loading = false;
    }
  };

  cancel = async (): Promise<void> => {
    try {
      this.job = await appServerRequest<ForgeJobStatus>({ method: "forgeJobCancel" });
    } catch (error) {
      this.notify(`Forge 取消失败：${error}`);
    }
  };

  private poll = async (generation: number): Promise<void> => {
    while (true) {
      await new Promise((resolve) => setTimeout(resolve, 700));
      try {
        const status = await appServerRequest<ForgeJobStatus>({ method: "forgeJobStatusRead" });
        if (status.generation !== generation) return;
        this.job = status;
        if (status.status === "running" || status.status === "cancelling") continue;
        if (status.status === "completed") this.notify("Forge 训练完成，安全摘要已生成。");
        else if (status.status === "cancelled") this.notify("Forge 训练已取消，完整进程树已停止。");
        else this.notify(`Forge 未完成：${status.error || status.status}`);
        return;
      } catch (error) {
        this.notify(`Forge 状态轮询失败：${error}`);
        return;
      }
    }
  };
}
