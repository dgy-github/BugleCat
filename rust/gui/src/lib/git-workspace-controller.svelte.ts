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

  constructor(private readonly notify: (message: string) => void) {}

  loadBranches = async (): Promise<void> => {
    this.branches = await invoke<BranchInfo[]>("git_branches");
  };

  refreshBranches = async (): Promise<void> => {
    this.busy = true;
    try { await this.loadBranches(); }
    catch (error) { this.notify(`分支加载失败：${error}`); }
    finally { this.busy = false; }
  };

  createBranch = async (): Promise<void> => {
    if (!this.newBranch.trim()) return;
    this.busy = true;
    const name = this.newBranch;
    try {
      await invoke("git_create_branch", { name });
      this.notify(`已新建并切换到分支 ${name}。`);
      this.newBranch = "";
      await this.loadBranches();
    } catch (error) {
      this.notify(`新建分支失败：${error}`);
    } finally { this.busy = false; }
  };

  switchBranch = async (name: string): Promise<void> => {
    if (this.busy) return;
    this.busy = true;
    try {
      await invoke("git_switch_branch", { name });
      this.notify(`已切换到分支 ${name}。`);
      await this.loadBranches();
    } catch (error) {
      this.notify(`切换失败：${error}`);
    } finally { this.busy = false; }
  };

  toggleBranchDetail = async (name: string): Promise<void> => {
    if (name in this.branchCommits) {
      const { [name]: _drop, ...rest } = this.branchCommits;
      this.branchCommits = rest;
      return;
    }
    try {
      this.branchCommits = { ...this.branchCommits, [name]: await invoke<Commit[]>("git_log", { name, limit: 10 }) };
    } catch (error) {
      this.branchCommits = { ...this.branchCommits, [name]: [{ hash: "", subject: `加载失败：${error}`, when: "" }] };
    }
  };

  loadDiff = async (): Promise<void> => {
    this.diffOpenFiles = {};
    try { this.diffFiles = await invoke<FileChange[]>("git_changes"); }
    catch (error) { this.diffFiles = []; this.notify(`Diff 失败：${error}`); }
  };

  toggleFile = async (path: string): Promise<void> => {
    if (path in this.diffOpenFiles) {
      const { [path]: _drop, ...rest } = this.diffOpenFiles;
      this.diffOpenFiles = rest;
      return;
    }
    try {
      const diff = await invoke<string>("git_file_diff", { path });
      this.diffOpenFiles = { ...this.diffOpenFiles, [path]: diff };
    } catch (error) {
      this.diffOpenFiles = { ...this.diffOpenFiles, [path]: `diff failed: ${error}` };
    }
  };
}
