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

  constructor(
    private readonly notify: (message: string) => void,
    private readonly turnIsBusy: () => boolean,
  ) {}

  load = async (): Promise<void> => {
    this.checkpoints = await invoke<Checkpoint[]>("get_checkpoints");
  };

  refresh = async (): Promise<void> => {
    this.busy = true;
    try { await this.load(); }
    catch (error) { this.notify(`检查点加载失败：${error}`); }
    finally { this.busy = false; }
  };

  save = async (): Promise<void> => {
    this.busy = true;
    try {
      const checkpoint = await invoke<Checkpoint>("create_checkpoint", { label: this.label });
      this.label = "";
      await this.load();
      this.notify(`检查点已保存：${checkpoint.id}`);
    } catch (error) {
      this.notify(`检查点失败：${error}`);
    } finally { this.busy = false; }
  };

  restore = async (id: string): Promise<void> => {
    if (this.turnIsBusy() || this.busy || !window.confirm(`恢复检查点 ${id}？`)) return;
    this.busy = true;
    try {
      const report = await invoke<RestoreReport>("restore_checkpoint", { id });
      await this.load();
      this.notify(`已恢复 ${report.checkpoint_id}：${report.restored_files} 个文件，删除 ${report.deleted_files} 个。`);
    } catch (error) {
      this.notify(`恢复失败：${error}`);
    } finally { this.busy = false; }
  };

  toggleDetail = async (id: string): Promise<void> => {
    if (id in this.files) {
      const { [id]: _drop, ...rest } = this.files;
      this.files = rest;
      return;
    }
    try { this.files = { ...this.files, [id]: await invoke<string[]>("checkpoint_files", { id }) }; }
    catch (error) { this.files = { ...this.files, [id]: [`加载失败：${error}`] }; }
  };
}
