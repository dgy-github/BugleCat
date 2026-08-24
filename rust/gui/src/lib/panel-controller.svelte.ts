import type { CheckpointController } from "./checkpoint-controller.svelte";
import type { FileBrowserController } from "./file-browser-controller.svelte";
import type { GitWorkspaceController } from "./git-workspace-controller.svelte";
import type { MemoryController } from "./memory-controller.svelte";
import type { ThreadController } from "./thread-controller.svelte";

export class PanelController {
  current = $state("");

  constructor(
    private readonly files: FileBrowserController,
    private readonly git: GitWorkspaceController,
    private readonly checkpoints: CheckpointController,
    private readonly memory: MemoryController,
    private readonly thread: ThreadController,
  ) {}

  openFiles = async (): Promise<void> => this.toggle("files", () => this.files.load(""));
  openBranches = async (): Promise<void> => this.toggle("branches", this.git.refreshBranches);
  openDiff = async (): Promise<void> => this.toggle("diff", this.git.loadDiff);
  openCheckpoints = async (): Promise<void> => this.toggle("checkpoints", this.checkpoints.refresh);
  openMemory = async (): Promise<void> => this.toggle("memory", this.memory.refresh);

  reload = async (): Promise<void> => {
    try {
      if (this.current === "files") await this.files.load(this.files.path);
      else if (this.current === "branches") await this.git.refreshBranches();
      else if (this.current === "diff") await this.git.loadDiff();
      else if (this.current === "memory") await this.memory.refresh();
      else if (this.current === "checkpoints") await this.checkpoints.refresh();
    } catch (error) { this.thread.messages.push({ role: "note", text: `刷新失败：${error}` }); }
  };

  private toggle = async (name: string, load: () => Promise<void>): Promise<void> => {
    if (this.current === name) { this.current = ""; return; }
    this.current = name;
    await load();
  };
}
