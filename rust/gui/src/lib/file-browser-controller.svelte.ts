import { invoke } from "@tauri-apps/api/core";

export type DirEntry = { name: string; path: string; is_dir: boolean };
export type FilePreview = { path: string; content: string };

export class FileBrowserController {
  path = $state("");
  entries = $state<DirEntry[]>([]);
  preview = $state<FilePreview | null>(null);
  private loadGeneration = 0;

  constructor(
    private readonly notify: (message: string) => void,
    private readonly readInput: () => string,
    private readonly writeInput: (value: string) => void,
    private readonly workspace: () => string,
  ) {}

  reset = (): void => {
    this.loadGeneration += 1;
    this.path = "";
    this.entries = [];
    this.preview = null;
  };

  load = async (relativePath: string): Promise<void> => {
    const generation = ++this.loadGeneration;
    const expectedWorkspace = this.workspace();
    try {
      const entries = await invoke<DirEntry[]>("list_dir", { rel: relativePath, expectedWorkspace });
      if (generation !== this.loadGeneration) return;
      this.entries = entries;
      this.path = relativePath;
      this.preview = null;
    } catch (error) {
      if (generation === this.loadGeneration) this.notify(`读取目录失败：${error}`);
    }
  };

  up = (): void => {
    if (this.preview) { this.preview = null; return; }
    if (!this.path) return;
    const parent = this.path.includes("/") ? this.path.slice(0, this.path.lastIndexOf("/")) : "";
    void this.load(parent);
  };

  pick = async (entry: DirEntry): Promise<void> => {
    if (entry.is_dir) { await this.load(entry.path); return; }
    const generation = this.loadGeneration;
    const expectedWorkspace = this.workspace();
    try {
      const content = await invoke<string>("read_workspace_file", { rel: entry.path, expectedWorkspace });
      if (generation === this.loadGeneration) this.preview = { path: entry.path, content };
    } catch (error) {
      if (generation === this.loadGeneration) this.preview = { path: entry.path, content: `（无法预览：${error}）` };
    }
  };

  insertMention = (path: string): void => {
    const input = this.readInput();
    this.writeInput(input ? `${input} @${path}` : `@${path}`);
  };
}
