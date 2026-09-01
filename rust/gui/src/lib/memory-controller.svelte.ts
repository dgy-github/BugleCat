import { invoke } from "@tauri-apps/api/core";
import { appServerRequest } from "./app-server-client";

export type MemoryNote = { ts: number; tags: string[]; text: string };
export type MemoryMergeStatus = { generation: number; status: string; requestedModel?: string | null; removed?: number | null; error?: string | null };

export class MemoryController {
  notes = $state<MemoryNote[]>([]);
  busy = $state(false);
  newNote = $state("");
  newNoteTags = $state("");
  mergeStatus = $state<MemoryMergeStatus | null>(null);
  private loadGeneration = 0;
  private nextBusyOperation = 0;
  private readonly activeBusyOperations = new Set<number>();

  constructor(
    private readonly notify: (message: string) => void,
    private readonly workspace: () => string,
  ) {}

  reset = (): void => {
    this.loadGeneration += 1;
    this.activeBusyOperations.clear();
    this.busy = false;
    this.notes = [];
    this.newNote = "";
    this.newNoteTags = "";
    this.mergeStatus = null;
  };

  load = async (): Promise<void> => {
    const generation = ++this.loadGeneration;
    const workspace = this.workspace();
    if (!workspace) throw new Error("当前工作区不可用，请重新打开项目记忆后再试");
    const notes = await appServerRequest<MemoryNote[]>({
      method: "memoryList",
      params: { workspace },
    });
    if (generation === this.loadGeneration) this.notes = notes;
  };

  refresh = async (): Promise<void> => {
    const operation = this.beginBusy();
    try { await this.load(); }
    catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`记忆加载失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  consolidate = async (): Promise<void> => {
    const operation = this.beginBusy();
    try {
      const workspace = this.workspace();
      if (!workspace) throw new Error("当前工作区不可用，请重新打开项目记忆后再试");
      const removed = await appServerRequest<number>({
        method: "memoryConsolidate",
        params: { workspace },
      });
      if (!this.isBusyOperation(operation)) return;
      this.notify(`记忆：合并了 ${removed} 条近重复经验。`);
      await this.load();
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`记忆整理失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  mergeWithModel = async (): Promise<void> => {
    const operation = this.beginBusy();
    try {
      const workspace = this.workspace();
      if (!workspace) throw new Error("当前工作区不可用，请重新打开项目记忆后再试");
      const status = await appServerRequest<MemoryMergeStatus>({
        method: "memoryMergeStart",
        params: { workspace },
      });
      if (!this.isBusyOperation(operation)) return;
      this.mergeStatus = status;
      await this.pollMerge(status.generation, operation, workspace);
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`模型整理启动失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  cancelMerge = async (): Promise<void> => {
    const operations = new Set(this.activeBusyOperations);
    const workspace = this.workspace();
    const generation = this.mergeStatus?.generation;
    if (!workspace || generation === undefined) {
      if (operations.size > 0) this.notify("当前没有可取消的模型整理任务");
      return;
    }
    try {
      const status = await appServerRequest<MemoryMergeStatus>({
        method: "memoryMergeCancel",
        params: { workspace, generation },
      });
      if ([...operations].some((operation) => this.isBusyOperation(operation))) this.mergeStatus = status;
    } catch (error) {
      if ([...operations].some((operation) => this.isBusyOperation(operation))) this.notify(`取消记忆整理失败：${error}`);
    }
  };

  private pollMerge = async (generation: number, operation: number, workspace: string): Promise<void> => {
    while (true) {
      await new Promise((resolve) => setTimeout(resolve, 400));
      if (!this.isBusyOperation(operation)) return;
      const status = await appServerRequest<MemoryMergeStatus>({
        method: "memoryMergeStatusRead",
        params: { workspace, generation },
      });
      if (!this.isBusyOperation(operation)) return;
      if (status.generation !== generation) return;
      this.mergeStatus = status;
      if (status.status === "running" || status.status === "cancelling") continue;
      if (status.status === "completed") {
        this.notify(`模型整理完成：合并了 ${status.removed || 0} 条近重复经验。`);
        await this.load();
      } else if (status.status === "cancelled") {
        this.notify("模型整理已取消，项目记忆未修改。");
      } else {
        this.notify(`模型整理未写入：${status.error || "任务失败"}`);
      }
      return;
    }
  };

  add = async (): Promise<void> => {
    if (!this.newNote.trim()) return;
    const operation = this.beginBusy();
    try {
      const workspace = this.workspace();
      if (!workspace) throw new Error("当前工作区不可用，请重新打开项目记忆后再试");
      const tags = this.newNoteTags.split(",").map((tag) => tag.trim()).filter(Boolean);
      const saved = await appServerRequest<boolean>({
        method: "memoryAdd",
        params: { note: this.newNote, tags, workspace },
      });
      if (!this.isBusyOperation(operation)) return;
      this.notify(saved ? "记忆：已保存。" : "记忆：已存在（未重复）。");
      this.newNote = "";
      this.newNoteTags = "";
      await this.load();
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`记忆添加失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  openFile = async (): Promise<void> => {
    try {
      const expectedWorkspace = this.workspace();
      if (!expectedWorkspace) throw new Error("当前工作区不可用，请重新打开项目记忆后再试");
      await invoke("open_memory_file", { expectedWorkspace });
    }
    catch (error) { this.notify(`打开记忆文件失败：${error}`); }
  };

  formatTimestamp = (timestamp: number): string => {
    try { return new Date(timestamp * 1000).toLocaleString(); }
    catch { return String(timestamp); }
  };

  private beginBusy = (): number => {
    const operation = ++this.nextBusyOperation;
    this.activeBusyOperations.add(operation);
    this.busy = true;
    return operation;
  };

  private endBusy = (operation: number): void => {
    if (!this.activeBusyOperations.delete(operation)) return;
    this.busy = this.activeBusyOperations.size > 0;
  };

  private isBusyOperation = (operation: number): boolean => this.activeBusyOperations.has(operation);
}
