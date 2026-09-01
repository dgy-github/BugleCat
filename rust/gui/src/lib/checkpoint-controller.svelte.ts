import { invoke } from "@tauri-apps/api/core";

export type Checkpoint = {
  id: string; label: string; created_at: string; files: number; skipped: number; total_bytes: number;
};
type RestoreReport = {
  checkpoint_id: string; safety_checkpoint_id?: string | null; restored_files: number; deleted_files: number;
};

export class CheckpointController {
  checkpoints = $state<Checkpoint[]>([]);
  label = $state("");
  busy = $state(false);
  files = $state<Record<string, string[]>>({});
  private loadGeneration = 0;
  private nextBusyOperation = 0;
  private readonly activeBusyOperations = new Set<number>();

  constructor(
    private readonly notify: (message: string) => void,
    private readonly turnIsBusy: () => boolean,
    private readonly workspace: () => string,
  ) {}

  reset = (): void => {
    this.loadGeneration += 1;
    this.activeBusyOperations.clear();
    this.busy = false;
    this.checkpoints = [];
    this.files = {};
    this.label = "";
  };

  load = async (expectedWorkspace = this.workspace()): Promise<void> => {
    const generation = ++this.loadGeneration;
    const checkpoints = await invoke<Checkpoint[]>("get_checkpoints", { expectedWorkspace });
    if (generation === this.loadGeneration) this.checkpoints = checkpoints;
  };

  refresh = async (): Promise<void> => {
    const operation = this.beginBusy();
    try { await this.load(); }
    catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`检查点加载失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  save = async (): Promise<void> => {
    const operation = this.beginBusy();
    const expectedWorkspace = this.workspace();
    try {
      const checkpoint = await invoke<Checkpoint>("create_checkpoint", { label: this.label, expectedWorkspace });
      if (!this.isBusyOperation(operation)) return;
      this.label = "";
      await this.load(expectedWorkspace);
      if (!this.isBusyOperation(operation)) return;
      this.notify(`检查点已保存：${checkpoint.id}`);
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`检查点失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  restore = async (id: string): Promise<void> => {
    if (this.turnIsBusy() || this.busy || !window.confirm(`恢复检查点 ${id}？`)) return;
    const operation = this.beginBusy();
    const expectedWorkspace = this.workspace();
    try {
      const report = await invoke<RestoreReport>("restore_checkpoint", { id, expectedWorkspace });
      if (!this.isBusyOperation(operation)) return;
      await this.load(expectedWorkspace);
      if (!this.isBusyOperation(operation)) return;
      this.notify(`已恢复 ${report.checkpoint_id}：${report.restored_files} 个文件，删除 ${report.deleted_files} 个。`);
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`恢复失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  toggleDetail = async (id: string): Promise<void> => {
    if (id in this.files) {
      const { [id]: _drop, ...rest } = this.files;
      this.files = rest;
      return;
    }
    const generation = this.loadGeneration;
    const expectedWorkspace = this.workspace();
    try {
      const files = await invoke<string[]>("checkpoint_files", { id, expectedWorkspace });
      if (generation === this.loadGeneration) this.files = { ...this.files, [id]: files };
    } catch (error) {
      if (generation === this.loadGeneration) this.files = { ...this.files, [id]: [`加载失败：${error}`] };
    }
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
