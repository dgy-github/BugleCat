import { invoke } from "@tauri-apps/api/core";
import { appServerRequest } from "./app-server-client";

export type MemoryNote = { ts: number; tags: string[]; text: string };

export class MemoryController {
  notes = $state<MemoryNote[]>([]);
  busy = $state(false);
  newNote = $state("");
  newNoteTags = $state("");

  constructor(private readonly notify: (message: string) => void) {}

  load = async (): Promise<void> => {
    this.notes = await appServerRequest<MemoryNote[]>({ method: "memoryList" });
  };

  refresh = async (): Promise<void> => {
    this.busy = true;
    try { await this.load(); }
    catch (error) { this.notify(`记忆加载失败：${error}`); }
    finally { this.busy = false; }
  };

  consolidate = async (): Promise<void> => {
    this.busy = true;
    try {
      const removed = await appServerRequest<number>({ method: "memoryConsolidate" });
      this.notify(`记忆：合并了 ${removed} 条近重复经验。`);
      await this.load();
    } catch (error) { this.notify(`记忆整理失败：${error}`); }
    finally { this.busy = false; }
  };

  add = async (): Promise<void> => {
    if (!this.newNote.trim()) return;
    this.busy = true;
    try {
      const tags = this.newNoteTags.split(",").map((tag) => tag.trim()).filter(Boolean);
      const saved = await appServerRequest<boolean>({ method: "memoryAdd", params: { note: this.newNote, tags } });
      this.notify(saved ? "记忆：已保存。" : "记忆：已存在（未重复）。");
      this.newNote = "";
      this.newNoteTags = "";
      await this.load();
    } catch (error) { this.notify(`记忆添加失败：${error}`); }
    finally { this.busy = false; }
  };

  openFile = async (): Promise<void> => {
    try { await invoke("open_memory_file"); }
    catch (error) { this.notify(`打开记忆文件失败：${error}`); }
  };

  formatTimestamp = (timestamp: number): string => {
    try { return new Date(timestamp * 1000).toLocaleString(); }
    catch { return String(timestamp); }
  };
}
