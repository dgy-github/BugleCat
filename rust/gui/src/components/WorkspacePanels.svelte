<script lang="ts">
  import ForgeControls from "./ForgeControls.svelte";
  import type { ForgeController } from "../lib/forge-controller.svelte";
  import type { DiffPreview } from "../lib/git-workspace-controller.svelte";
  type Checkpoint = { id: string; label: string; created_at: string; files: number; skipped: number };
  type Branch = { name: string; current: boolean };
  type Commit = { hash: string; subject: string; when: string };
  type DirEntry = { name: string; path: string; is_dir: boolean };
  type FilePreview = { path: string; content: string };
  type FileChange = { path: string; kind: string; added: number; removed: number };
  type SessionRow = { session_id: string; title: string; snippet: string; updated_at: string; user_messages: number; assistant_messages: number; tool_calls: number; has_snapshot: boolean };
  type MemoryNote = { ts: number; tags: string[]; text: string };
  type MemoryMergeStatus = { generation: number; status: string; requestedModel?: string | null; removed?: number | null; error?: string | null };

  let {
    rightPanel = $bindable(), reloadPanel, checkpointLabel = $bindable(), checkpointBusy, checkpoints, checkpointFiles,
    saveCheckpoint, loadCheckpoints, toggleCheckpointDetail, restoreCheckpoint, busy,
    newBranch = $bindable(), branchBusy, branches, branchCommits, createBranch, loadBranches, toggleBranchDetail, switchBranch,
    chooseWorkspace, filePreview = $bindable(), filesPath, filesEntries, insertMention, filesUp, pickFile,
    diffFiles, diffOpenFiles, toggleFile, diffLineClass,
    historyOpen = $bindable(), sessions, switchingSession, resumeSession, forkSession, openSessionLog, openSessionSnapshot, refreshSessions,
    newNote = $bindable(), newNoteTags = $bindable(), hermesBusy, notes, addNote, consolidateMemory, mergeMemory, cancelMemoryMerge, memoryMergeStatus, loadNotes, openMemoryFile, fmtTs, forgeController,
  }: {
    rightPanel: string; reloadPanel: () => void; checkpointLabel: string; checkpointBusy: boolean; checkpoints: Checkpoint[];
    checkpointFiles: Record<string, string[]>; saveCheckpoint: () => void; loadCheckpoints: () => void;
    toggleCheckpointDetail: (id: string) => void; restoreCheckpoint: (id: string) => void; busy: boolean;
    newBranch: string; branchBusy: boolean; branches: Branch[]; branchCommits: Record<string, Commit[]>;
    createBranch: () => void; loadBranches: () => void; toggleBranchDetail: (name: string) => void; switchBranch: (name: string) => void;
    chooseWorkspace: () => void; filePreview: FilePreview | null; filesPath: string; filesEntries: DirEntry[];
    insertMention: (path: string) => void; filesUp: () => void; pickFile: (entry: DirEntry) => void;
    diffFiles: FileChange[]; diffOpenFiles: Record<string, DiffPreview>; toggleFile: (path: string) => void; diffLineClass: (line: string) => string;
    historyOpen: boolean; sessions: SessionRow[]; switchingSession: boolean; resumeSession: (id: string) => void;
    forkSession: (id: string) => void; openSessionLog: (id: string) => void; openSessionSnapshot: (id: string) => void; refreshSessions: () => void;
    newNote: string; newNoteTags: string; hermesBusy: boolean; notes: MemoryNote[]; addNote: () => void;
    consolidateMemory: () => void; mergeMemory: () => void; cancelMemoryMerge: () => void; memoryMergeStatus: MemoryMergeStatus | null;
    loadNotes: () => void; openMemoryFile: () => void; fmtTs: (timestamp: number) => string;
    forgeController: ForgeController;
  } = $props();
</script>

{#snippet panelHeader(title: string)}
  <div class="rp-head"><span class="rp-title">{title}</span><span class="rp-actions"><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div>
{/snippet}

{#if rightPanel === "checkpoints"}
  <aside class="rightpanel">{@render panelHeader("检查点")}<div class="rp-body">
    <div class="checkpoint-create"><input bind:value={checkpointLabel} placeholder="标签" /><button onclick={saveCheckpoint} disabled={checkpointBusy}>保存</button><button class="plain" onclick={loadCheckpoints} disabled={checkpointBusy}>刷新</button></div>
    <div class="checkpoint-list">{#if checkpoints.length === 0}<p class="emptyline">暂无检查点。</p>{/if}
      {#each checkpoints as checkpoint}<div class="checkpoint-row"><div class="checkpoint-main"><button class="link-row" onclick={() => toggleCheckpointDetail(checkpoint.id)} title="查看快照文件"><span class="wt-caret">{checkpoint.id in checkpointFiles ? "▾" : "▸"}</span><strong>{checkpoint.label || "（无标签）"}</strong></button><code>{checkpoint.id}</code></div><div class="checkpoint-meta"><span>{checkpoint.created_at}</span><span>{checkpoint.files} 个文件</span><span>跳过 {checkpoint.skipped}</span></div><button class="restore" onclick={() => restoreCheckpoint(checkpoint.id)} disabled={busy || checkpointBusy}>恢复</button>{#if checkpoint.id in checkpointFiles}<div class="detail-list">{#if checkpointFiles[checkpoint.id].length === 0}<div class="detail-row">（无文件）</div>{/if}{#each checkpointFiles[checkpoint.id] as path}<div class="detail-row"><code class="dl-path">{path}</code></div>{/each}</div>{/if}</div>{/each}
    </div></div></aside>
{:else if rightPanel === "branches"}
  <aside class="rightpanel">{@render panelHeader("Git 分支")}<div class="rp-body">
    <div class="checkpoint-create"><input bind:value={newBranch} placeholder="新分支名" /><button onclick={createBranch} disabled={branchBusy}>新建并切换</button><button class="plain" onclick={loadBranches} disabled={branchBusy}>刷新</button></div>
    <div class="checkpoint-list">{#if branches.length === 0}<p class="emptyline">暂无分支。</p>{/if}{#each branches as branch}<div class="checkpoint-row"><div class="checkpoint-main"><button class="link-row" onclick={() => toggleBranchDetail(branch.name)} title="查看最近提交"><span class="wt-caret">{branch.name in branchCommits ? "▾" : "▸"}</span><strong>{branch.current ? "● " : ""}{branch.name}</strong></button></div><button class="restore" onclick={() => switchBranch(branch.name)} disabled={branchBusy || branch.current}>{branch.current ? "当前" : "切换"}</button>{#if branch.name in branchCommits}<div class="detail-list">{#if branchCommits[branch.name].length === 0}<div class="detail-row">（无提交）</div>{/if}{#each branchCommits[branch.name] as commit}<div class="detail-row">{#if commit.hash}<code class="dl-hash">{commit.hash}</code>{/if}<span class="dl-subj">{commit.subject}</span>{#if commit.when}<span class="dl-when">{commit.when}</span>{/if}</div>{/each}</div>{/if}</div>{/each}</div>
  </div></aside>
{:else if rightPanel === "files"}
  <aside class="rightpanel"><div class="rp-head"><span class="rp-title">文件</span><span class="rp-actions"><button class="plain" onclick={chooseWorkspace}>打开项目</button><button class="plain rp-refresh" onclick={reloadPanel}>刷新</button><button class="rp-close" onclick={() => (rightPanel = "")} aria-label="关闭">×</button></span></div><div class="rp-body">
    {#if filePreview}<div class="fx-bar"><button class="plain" onclick={() => (filePreview = null)}>‹ 返回</button><code class="fx-path" title={filePreview.path}>{filePreview.path}</code><button class="plain" onclick={() => filePreview && insertMention(filePreview.path)}>＋@引用</button></div><pre class="fx-preview">{filePreview.content}</pre>
    {:else}<div class="fx-bar"><button class="plain" onclick={filesUp} disabled={!filesPath}>↑ 上级</button><code class="fx-path">/{filesPath}</code><button class="plain" onclick={chooseWorkspace}>打开其它项目…</button></div><div class="wt-list">{#if filesEntries.length === 0}<p class="emptyline">（空）</p>{/if}{#each filesEntries as entry}<button class="fx-row" onclick={() => pickFile(entry)} title={entry.is_dir ? "打开文件夹" : "预览文件"}><span class="fx-ic">{entry.is_dir ? "📁" : "📄"}</span><span class="fx-name">{entry.name}</span><span class="fx-go">›</span></button>{/each}</div>{/if}
  </div></aside>
{:else if rightPanel === "diff"}
  <aside class="rightpanel">{@render panelHeader("工作区改动")}<div class="rp-body"><div class="wt-list">{#if diffFiles.length === 0}<p class="emptyline">工作区没有改动。</p>{/if}{#each diffFiles as file}<div class="wt-file"><button class="wt-head" onclick={() => toggleFile(file.path)}><span class="wt-caret">{Object.hasOwn(diffOpenFiles, file.path) ? "▾" : "▸"}</span><span class="wt-kind wt-{file.kind}">{file.kind[0].toUpperCase()}</span><span class="wt-path">{file.path}</span><span class="wt-stat">{#if file.added >= 0}<span class="wt-add">+{file.added}</span>{/if}{#if file.removed >= 0}<span class="wt-del">-{file.removed}</span>{/if}</span></button>{#if Object.hasOwn(diffOpenFiles, file.path)}{#if diffOpenFiles[file.path].truncated}<p class="wt-diff-notice">预览已截断，以保持界面流畅；请在编辑器中打开文件查看完整差异。</p>{/if}<pre class="wt-diff">{#each diffOpenFiles[file.path].text.split("\n") as line}<span class="dl {diffLineClass(line)}">{line === "" ? " " : line}</span>{/each}</pre>{/if}</div>{/each}</div></div></aside>
{:else if rightPanel === "memory"}
  <aside class="rightpanel">{@render panelHeader("项目记忆")}<div class="rp-body"><p class="emptyline">已验证的经验，会作为线索在未来会话中被回忆。</p><div class="checkpoint-create"><input bind:value={newNote} placeholder="记录一条已验证的经验…" /><input bind:value={newNoteTags} placeholder="标签（逗号分隔）" style="max-width:140px" /><button onclick={addNote} disabled={hermesBusy}>添加</button></div><div class="checkpoint-create"><button onclick={consolidateMemory} disabled={hermesBusy}>快速去重</button><button onclick={mergeMemory} disabled={hermesBusy}>模型整理</button>{#if memoryMergeStatus?.status === "running" || memoryMergeStatus?.status === "cancelling"}<button class="deny" onclick={cancelMemoryMerge} disabled={memoryMergeStatus.status === "cancelling"}>{memoryMergeStatus.status === "cancelling" ? "取消中…" : "取消"}</button>{/if}<button class="plain" onclick={loadNotes} disabled={hermesBusy}>刷新</button><button class="plain" onclick={openMemoryFile}>打开文件</button><span class="emptyline">{notes.length} 条</span></div>{#if memoryMergeStatus && memoryMergeStatus.status !== "idle"}<p class="emptyline">模型整理：{memoryMergeStatus.status}{#if memoryMergeStatus.requestedModel} · {memoryMergeStatus.requestedModel}{/if}</p>{/if}<div class="checkpoint-list">{#if notes.length === 0}<p class="emptyline">暂无经验。</p>{/if}{#each notes as note}<div class="checkpoint-row"><div class="checkpoint-main"><strong>{note.text}</strong>{#if note.tags.length}<code>{note.tags.join(", ")}</code>{/if}</div><div class="checkpoint-meta"><span>{fmtTs(note.ts)}</span></div></div>{/each}</div><ForgeControls forge={forgeController} /></div></aside>
{/if}

{#if historyOpen}
  <div class="overlay"><div class="modal"><h3>会话历史</h3><div class="checkpoint-list">{#if sessions.length === 0}<p class="emptyline">暂无保存的会话。</p>{/if}{#each sessions as session}<div class="checkpoint-row"><div class="checkpoint-main"><strong>{session.title || "（未命名）"}</strong><code>{session.snippet}</code></div><div class="session-actions"><button class="plain" onclick={() => resumeSession(session.session_id)} disabled={switchingSession || !session.has_snapshot}>继续</button><button class="restore" onclick={() => forkSession(session.session_id)} disabled={busy || !session.has_snapshot}>⑂ 分叉</button><button class="plain" onclick={() => openSessionLog(session.session_id)}>日志</button><button class="plain" onclick={() => openSessionSnapshot(session.session_id)} disabled={!session.has_snapshot}>快照</button></div><div class="checkpoint-meta"><span>{session.updated_at}</span><span>{session.user_messages} 问 · {session.assistant_messages} 答 · {session.tool_calls} 工具</span>{#if !session.has_snapshot}<span>（无快照）</span>{/if}</div></div>{/each}</div><div class="abtns"><button class="plain" onclick={refreshSessions}>刷新</button><button class="deny" onclick={() => (historyOpen = false)}>关闭</button></div></div></div>
{/if}
