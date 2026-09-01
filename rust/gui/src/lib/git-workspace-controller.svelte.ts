import { invoke } from "@tauri-apps/api/core";

export type BranchInfo = { name: string; current: boolean };
export type Commit = { hash: string; subject: string; when: string };
export type FileChange = { path: string; added: number; removed: number; kind: string };

export class GitWorkspaceController {
  branches = $state<BranchInfo[]>([]);
  newBranch = $state("");
  busy = $state(false);
  branchCommits = $state<Record<string, Commit[]>>({});
  diffFiles = $state<FileChange[]>([]);
  diffOpenFiles = $state<Record<string, string>>({});
  private diffGeneration = 0;
  private workspaceGeneration = 0;
  // A reset invalidates every operation from the previous workspace. Track
  // operation tokens so an older `finally` cannot hide a newer spinner.
  private nextBusyOperation = 0;
  private readonly activeBusyOperations = new Set<number>();

  constructor(
    private readonly notify: (message: string) => void,
    private readonly workspace: () => string,
  ) {}

  reset = (): void => {
    this.workspaceGeneration += 1;
    this.diffGeneration += 1;
    this.activeBusyOperations.clear();
    this.busy = false;
    this.branches = [];
    this.newBranch = "";
    this.branchCommits = {};
    this.diffFiles = [];
    this.diffOpenFiles = {};
  };

  loadBranches = async (expectedWorkspace = this.workspace()): Promise<void> => {
    const generation = this.workspaceGeneration;
    const branches = await invoke<BranchInfo[]>("git_branches", { expectedWorkspace });
    if (generation === this.workspaceGeneration) this.branches = branches;
  };

  refreshBranches = async (): Promise<void> => {
    const operation = this.beginBusy();
    try { await this.loadBranches(); }
    catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`分支加载失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  createBranch = async (): Promise<void> => {
    if (!this.newBranch.trim()) return;
    const operation = this.beginBusy();
    const name = this.newBranch;
    const expectedWorkspace = this.workspace();
    try {
      await invoke("git_create_branch", { name, expectedWorkspace });
      if (!this.isBusyOperation(operation)) return;
      this.notify(`已新建并切换到分支 ${name}。`);
      this.newBranch = "";
      await this.loadBranches(expectedWorkspace);
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`新建分支失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  switchBranch = async (name: string): Promise<void> => {
    if (this.busy) return;
    const operation = this.beginBusy();
    const expectedWorkspace = this.workspace();
    try {
      await invoke("git_switch_branch", { name, expectedWorkspace });
      if (!this.isBusyOperation(operation)) return;
      this.notify(`已切换到分支 ${name}。`);
      await this.loadBranches(expectedWorkspace);
    } catch (error) {
      if (this.isBusyOperation(operation)) this.notify(`切换失败：${error}`);
    } finally { this.endBusy(operation); }
  };

  toggleBranchDetail = async (name: string): Promise<void> => {
    if (name in this.branchCommits) {
      const { [name]: _drop, ...rest } = this.branchCommits;
      this.branchCommits = rest;
      return;
    }
    const generation = this.workspaceGeneration;
    const expectedWorkspace = this.workspace();
    try {
      const commits = await invoke<Commit[]>("git_log", { name, limit: 10, expectedWorkspace });
      if (generation === this.workspaceGeneration) this.branchCommits = { ...this.branchCommits, [name]: commits };
    } catch (error) {
      if (generation === this.workspaceGeneration) {
        this.branchCommits = { ...this.branchCommits, [name]: [{ hash: "", subject: `加载失败：${error}`, when: "" }] };
      }
    }
  };

  loadDiff = async (): Promise<void> => {
    const generation = ++this.diffGeneration;
    const workspaceGeneration = this.workspaceGeneration;
    const expectedWorkspace = this.workspace();
    this.diffOpenFiles = {};
    try {
      const files = await invoke<FileChange[]>("git_changes", { expectedWorkspace });
      if (generation === this.diffGeneration && workspaceGeneration === this.workspaceGeneration) this.diffFiles = files;
    } catch (error) {
      if (generation === this.diffGeneration && workspaceGeneration === this.workspaceGeneration) {
        this.diffFiles = [];
        this.notify(`Diff 失败：${error}`);
      }
    }
  };

  toggleFile = async (path: string): Promise<void> => {
    if (path in this.diffOpenFiles) {
      const { [path]: _drop, ...rest } = this.diffOpenFiles;
      this.diffOpenFiles = rest;
      return;
    }
    const generation = this.workspaceGeneration;
    const expectedWorkspace = this.workspace();
    try {
      const diff = await invoke<string>("git_file_diff", { path, expectedWorkspace });
      if (generation === this.workspaceGeneration) this.diffOpenFiles = { ...this.diffOpenFiles, [path]: diff };
    } catch (error) {
      if (generation === this.workspaceGeneration) this.diffOpenFiles = { ...this.diffOpenFiles, [path]: `diff failed: ${error}` };
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
