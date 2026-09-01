import type { CheckpointController } from "./checkpoint-controller.svelte";
import type { FileBrowserController } from "./file-browser-controller.svelte";
import type { GitWorkspaceController } from "./git-workspace-controller.svelte";
import type { MemoryController } from "./memory-controller.svelte";
import type { ThreadController } from "./thread-controller.svelte";

export class PanelController {
  current = $state("");
  private requestGeneration = 0;

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

  workspaceChanged = (): void => {
    this.requestGeneration += 1;
    this.current = "";
    this.files.reset();
    this.git.reset();
    this.checkpoints.reset();
    this.memory.reset();
  };

  reload = async (): Promise<void> => {
    const generation = ++this.requestGeneration;
    const panel = this.current;
    try {
      if (panel === "files") await this.files.load(this.files.path);
      else if (panel === "branches") await this.git.refreshBranches();
      else if (panel === "diff") await this.git.loadDiff();
      else if (panel === "memory") await this.memory.refresh();
      else if (panel === "checkpoints") await this.checkpoints.refresh();
    } catch (error) {
      if (generation === this.requestGeneration && this.current === panel) {
        this.thread.messages.push({ role: "note", text: `刷新失败：${error}` });
      }
    }
  };

  private toggle = async (name: string, load: () => Promise<void>): Promise<void> => {
    const generation = ++this.requestGeneration;
    if (this.current === name) { this.current = ""; return; }
    this.current = name;
    try { await load(); }
    catch (error) {
      if (generation === this.requestGeneration && this.current === name) {
        this.thread.messages.push({ role: "note", text: `加载失败：${error}` });
      }
    }
  };
}
