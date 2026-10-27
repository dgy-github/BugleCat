<script lang="ts">
  import { hoverDepth } from "../lib/hover-depth";
  import { buglecatAsset } from "../lib/buglecat-assets";
  import type { DshUiSlotContribution } from "../lib/plugin-controller.svelte";
  type SessionRow = {
    session_id: string;
    workspace: string;
    title: string;
    snippet: string;
    updated_at: string;
    has_snapshot: boolean;
    archived: boolean;
  };

  let {
    sidebarOpen,
    switchingSession,
    currentSessionId,
    runningSessions,
    recentSessions,
    archivedSessions,
    archivedCount,
    showArchived = $bindable(),
    workspace,
    sidebarResizing,
    sidebarWidth,
    SIDEBAR_MIN_WIDTH,
    SIDEBAR_MAX_WIDTH,
    SIDEBAR_DEFAULT_WIDTH,
    toggleSidebar,
    newSession,
    resumeSession,
    forkSession,
    openSessionLog,
    archiveSession,
    archiveSessions,
    deleteSessions,
    renameSession,
    chooseWorkspace,
    openSettings,
    dshFooterActions,
    openDshSlot,
    fmtWhen,
    baseName,
    beginSidebarResize,
    setSidebarWidth,
    handleSidebarResizeKey,
  }: {
    sidebarOpen: boolean;
    switchingSession: boolean;
    currentSessionId: string;
    runningSessions: Set<string>;
    recentSessions: SessionRow[];
    archivedSessions: SessionRow[];
    archivedCount: number;
    showArchived: boolean;
    workspace: string;
    sidebarResizing: boolean;
    sidebarWidth: number;
    SIDEBAR_MIN_WIDTH: number;
    SIDEBAR_MAX_WIDTH: number;
    SIDEBAR_DEFAULT_WIDTH: number;
    toggleSidebar: () => void;
    newSession: () => void;
    resumeSession: (id: string, title: string) => void;
    forkSession: (id: string, title: string) => void;
    openSessionLog: (id: string) => void;
    archiveSession: (id: string, archived: boolean) => void;
    archiveSessions: (ids: string[], archived: boolean) => Promise<void>;
    deleteSessions: (ids: string[]) => Promise<void>;
    renameSession: (id: string, title: string) => Promise<void>;
    chooseWorkspace: () => void;
    openSettings: () => void;
    dshFooterActions: DshUiSlotContribution[];
    openDshSlot: (slot: DshUiSlotContribution) => void;
    fmtWhen: (value: string) => string;
    baseName: (path: string) => string;
    beginSidebarResize: (event: PointerEvent) => void;
    setSidebarWidth: (width: number) => void;
    handleSidebarResizeKey: (event: KeyboardEvent) => void;
  } = $props();

  let menuId = $state("");
  let renameTarget = $state<SessionRow | null>(null);
  let renameDraft = $state("");
  let renameError = $state("");
  let renameSaving = $state(false);
  let projectOpen = $state<Record<string, boolean>>({});
  let archivedProjectOpen = $state<Record<string, boolean>>({});

  function groupByWorkspace(sessions: SessionRow[]) {
    const groups = new Map<string, { path: string; sessions: SessionRow[] }>();
    for (const session of sessions) {
      const path = session.workspace || "未指定项目";
      const key = path.toLocaleLowerCase();
      const group = groups.get(key) || { path, sessions: [] };
      group.sessions.push(session);
      groups.set(key, group);
    }
    return Array.from(groups.values());
  }

  const projectGroups = $derived(groupByWorkspace(recentSessions));
  const archivedProjectGroups = $derived(groupByWorkspace(archivedSessions));

  function toggleProject(path: string) {
    projectOpen = { ...projectOpen, [path]: projectOpen[path] === false };
  }

  function toggleArchivedProject(path: string) {
    archivedProjectOpen = { ...archivedProjectOpen, [path]: archivedProjectOpen[path] === false };
  }

  function collapseProjects() {
    projectOpen = Object.fromEntries(projectGroups.map((group) => [group.path, false]));
  }

  type ProjectGroup = { path: string; sessions: SessionRow[] };

  // Project-row menu ("recent:<path>" / "archived:<path>") and multi-select mode.
  let groupMenuKey = $state("");
  let selecting = $state(false);
  let selected = $state<Set<string>>(new Set());
  let selectAnchor = "";
  let batchBusy = $state(false);

  const allSessions = $derived([...recentSessions, ...archivedSessions]);
  const selectedRows = $derived(allSessions.filter((s) => selected.has(s.session_id)));
  /** Sessions in the order they are drawn, for Shift+click range selection. */
  const visibleOrder = $derived([
    ...projectGroups.flatMap((g) => (projectOpen[g.path] !== false ? g.sessions : [])),
    ...(showArchived ? archivedProjectGroups.flatMap((g) => (archivedProjectOpen[g.path] !== false ? g.sessions : [])) : []),
  ]);

  function startSelecting(initial: SessionRow[] = []) {
    menuId = ""; groupMenuKey = "";
    selecting = true;
    selected = new Set(initial.map((s) => s.session_id));
    selectAnchor = "";
  }

  function stopSelecting() {
    selecting = false; selected = new Set(); selectAnchor = "";
  }

  function toggleSelected(session: SessionRow, range: boolean) {
    const next = new Set(selected);
    const from = visibleOrder.findIndex((s) => s.session_id === selectAnchor);
    const to = visibleOrder.findIndex((s) => s.session_id === session.session_id);
    if (range && from >= 0 && to >= 0) {
      for (const s of visibleOrder.slice(Math.min(from, to), Math.max(from, to) + 1)) next.add(s.session_id);
    } else if (next.has(session.session_id)) next.delete(session.session_id);
    else next.add(session.session_id);
    selected = next;
    selectAnchor = session.session_id;
  }

  function groupSelection(group: ProjectGroup): "all" | "some" | "none" {
    const count = group.sessions.filter((s) => selected.has(s.session_id)).length;
    return count === 0 ? "none" : count === group.sessions.length ? "all" : "some";
  }

  function toggleGroupSelected(group: ProjectGroup) {
    const next = new Set(selected);
    const selectAll = groupSelection(group) !== "all";
    for (const s of group.sessions) {
      if (selectAll) next.add(s.session_id); else next.delete(s.session_id);
    }
    selected = next;
  }

  /** Every session in every project group, collapsed ones included, plus
   *  the archived ones while that list is shown. */
  function selectAll() {
    selected = new Set([...recentSessions, ...(showArchived ? archivedSessions : [])].map((s) => s.session_id));
  }

  async function archiveRows(rows: SessionRow[], archived: boolean) {
    const ids = rows.filter((s) => s.archived !== archived).map((s) => s.session_id);
    if (!ids.length || batchBusy) return;
    batchBusy = true;
    try { await archiveSessions(ids, archived); }
    finally { batchBusy = false; }
  }

  async function archiveSelected(archived: boolean) {
    await archiveRows(selectedRows, archived);
    stopSelecting();
  }

  function archiveGroup(group: ProjectGroup, archived: boolean) {
    groupMenuKey = "";
    void archiveRows(group.sessions, archived);
  }

  // Permanent delete, always behind a confirmation dialog.
  let deleteTarget = $state<SessionRow[] | null>(null);
  const deleteProjects = $derived(deleteTarget ? [...new Set(deleteTarget.map((s) => baseName(s.workspace || "未指定项目")))] : []);

  function requestDelete(rows: SessionRow[]) {
    menuId = ""; groupMenuKey = "";
    if (rows.length) deleteTarget = rows;
  }

  async function confirmDelete() {
    if (!deleteTarget || batchBusy) return;
    const ids = deleteTarget.map((s) => s.session_id);
    batchBusy = true;
    try { await deleteSessions(ids); }
    finally { batchBusy = false; deleteTarget = null; stopSelecting(); }
  }

  function onSidebarKey(event: KeyboardEvent) {
    if (event.key === "Escape" && deleteTarget && !batchBusy) { deleteTarget = null; return; }
    if (event.key === "Escape" && selecting && !renameTarget) stopSelecting();
  }

  function beginRename(session: SessionRow) {
    menuId = ""; renameTarget = session; renameDraft = session.title; renameError = "";
    queueMicrotask(() => document.querySelector<HTMLInputElement>("#session-rename-input")?.select());
  }

  async function submitRename() {
    if (!renameTarget || renameSaving) return;
    renameSaving = true; renameError = "";
    try { await renameSession(renameTarget.session_id, renameDraft); renameTarget = null; }
    catch (error) { renameError = String(error).replace(/^Error:\s*/, ""); }
    finally { renameSaving = false; }
  }
</script>

<svelte:window onkeydown={onSidebarKey} />
<aside class="sidebar" class:collapsed={!sidebarOpen} use:hoverDepth={".new-session, .side-brand-mark, .recent-item"}>
  <div class="side-head">
    <span class="side-brand"><img class="side-brand-mark" src={buglecatAsset("avatar", 24)} alt="BugleCat 猫咪图标" /><span>BugleCat</span><small>HARNESS</small></span>
    <button class="side-collapse" onclick={toggleSidebar} title="收起侧边栏" aria-label="收起侧边栏"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4" width="17" height="16" rx="3"/><path d="M9 4v16M15.5 9l-3 3 3 3"/></svg></button>
  </div>
  <button class="new-session" onclick={newSession}>
    <img class="ni buglecat-button-icon" src={buglecatAsset("new-chat", 24)} alt="" aria-hidden="true" />
    新会话
  </button>

  <div class="side-recents">
    <div class="side-workspace-label">
      <span>项目</span>
      <span class="side-workspace-actions">
        <button onclick={chooseWorkspace} title="切换或添加工作区" aria-label="切换或添加工作区"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3.5 7.5A2.5 2.5 0 0 1 6 5h3.2l2 2H18a2.5 2.5 0 0 1 2.5 2.5V17A2.5 2.5 0 0 1 18 19.5H6A2.5 2.5 0 0 1 3.5 17z"/><path d="M16.5 10.5v5M14 13h5"/></svg></button>
        <button onclick={collapseProjects} title="折叠全部项目" aria-label="折叠全部项目"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 9 4-4 4 4M16 15l-4 4-4-4"/></svg></button>
        <button class:active={selecting} onclick={() => (selecting ? stopSelecting() : startSelecting())}
          title={selecting ? "退出批量选择" : "批量选择会话"} aria-label={selecting ? "退出批量选择" : "批量选择会话"} aria-pressed={selecting}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="6" height="6" rx="1.5"/><path d="m5.6 7 1.1 1.1L8.6 6"/><rect x="4" y="14" width="6" height="6" rx="1.5"/><path d="M13.5 7h6.5M13.5 17h6.5"/></svg></button>
      </span>
    </div>
    {#snippet sessionItem(s: SessionRow)}
      <div class="recent-item" class:active={!selecting && s.session_id === currentSessionId} class:running={runningSessions.has(s.session_id)} class:archived={s.archived}
        class:selecting class:selected={selecting && selected.has(s.session_id)}
        role="treeitem" tabindex="-1" aria-selected={selecting ? selected.has(s.session_id) : s.session_id === currentSessionId}
        onclick={(event) => {
          if (selecting) { toggleSelected(s, event.shiftKey); return; }
          if (!(event.target as HTMLElement).closest(".recent-menu-wrap")) resumeSession(s.session_id, s.title);
        }}
        onkeydown={(event) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          if (selecting) { event.preventDefault(); toggleSelected(s, event.shiftKey); } else resumeSession(s.session_id, s.title);
        }}>
        <button class="recent-main" title={s.snippet || s.title} disabled={!selecting && switchingSession}
          aria-label={selecting ? `选择会话“${s.title}”` : `打开会话“${s.title}”`}>
          {#if selecting}<span class="select-box" class:checked={selected.has(s.session_id)} aria-hidden="true"></span>{/if}
          <span class="recent-dot">●</span>
          <span class="recent-text">
            <span class="recent-title">{s.title || "（未命名）"}</span>
            <span class="recent-when">{runningSessions.has(s.session_id) ? "执行中" : fmtWhen(s.updated_at)}{s.archived ? " · 已归档" : ""}</span>
          </span>
        </button>
        {#if !selecting}
        <span class="recent-menu-wrap">
          <button class="recent-act" class:open={menuId === s.session_id} title={`会话“${s.title}”的操作`}
            onclick={() => (menuId = menuId === s.session_id ? "" : s.session_id)} aria-label={`会话“${s.title}”的操作`}>•••</button>
          {#if menuId === s.session_id}
            <button class="menu-backdrop" aria-label="关闭会话菜单" onclick={() => (menuId = "")}></button>
            <div class="recent-menu" role="menu">
              <button role="menuitem" onclick={() => beginRename(s)}>✎ <span>重命名</span></button>
              <button role="menuitem" disabled={switchingSession || !s.has_snapshot} onclick={() => { menuId = ""; forkSession(s.session_id, s.title); }}>⑂ <span>分叉会话</span></button>
              <button role="menuitem" onclick={() => { menuId = ""; openSessionLog(s.session_id); }}>▤ <span>打开会话日志</span></button>
              <button role="menuitem" onclick={() => { menuId = ""; archiveSession(s.session_id, !s.archived); }}>{s.archived ? "↩" : "⌑"} <span>{s.archived ? "取消归档" : "归档会话"}</span></button>
              <button role="menuitem" onclick={() => startSelecting([s])}>☑ <span>批量选择…</span></button>
            </div>
          {/if}
        </span>
        {/if}
      </div>
    {/snippet}

    {#snippet projectGroup(group: ProjectGroup, open: boolean, toggle: (path: string) => void, archivedList: boolean)}
      {@const menuKey = `${archivedList ? "archived" : "recent"}:${group.path}`}
      {@const groupState = groupSelection(group)}
      <section class="project-group" class:archived-project-group={archivedList} class:current={group.path === workspace}>
        <div class="project-row" class:selecting>
          {#if selecting}
            <button class="project-select" onclick={() => toggleGroupSelected(group)}
              aria-label={`${groupState === "all" ? "取消选择" : "选择"}“${baseName(group.path)}”下的全部会话`}
              aria-pressed={groupState === "all" ? true : groupState === "some" ? "mixed" : false}>
              <span class="select-box" class:checked={groupState === "all"} class:mixed={groupState === "some"} aria-hidden="true"></span>
            </button>
          {/if}
          <button class="project-toggle" class:open
            aria-expanded={open} onclick={() => toggle(group.path)}
            title={group.path}>
            <span class="project-caret" aria-hidden="true"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m6 3.5 4.5 4.5L6 12.5"/></svg></span>
            <span class="project-folder" aria-hidden="true"><img src={buglecatAsset("project", 24)} alt="" /></span>
            <span class="project-name">{baseName(group.path)}</span>
            <span class="project-count">{group.sessions.length}</span>
          </button>
          {#if !selecting}
            <span class="recent-menu-wrap project-menu-wrap">
              <button class="recent-act project-act" class:open={groupMenuKey === menuKey}
                title={`项目“${baseName(group.path)}”的操作`} aria-label={`项目“${baseName(group.path)}”的操作`}
                onclick={() => (groupMenuKey = groupMenuKey === menuKey ? "" : menuKey)}>•••</button>
              {#if groupMenuKey === menuKey}
                <button class="menu-backdrop" aria-label="关闭项目菜单" onclick={() => (groupMenuKey = "")}></button>
                <div class="recent-menu" role="menu">
                  {#if archivedList}
                    <button role="menuitem" disabled={batchBusy} onclick={() => archiveGroup(group, false)}>↩ <span>全部取消归档（{group.sessions.length}）</span></button>
                  {:else}
                    <button role="menuitem" disabled={batchBusy} onclick={() => archiveGroup(group, true)}>⌑ <span>归档全部并移出列表（{group.sessions.length}）</span></button>
                  {/if}
                  <button role="menuitem" onclick={() => startSelecting(group.sessions)}>☑ <span>批量选择…</span></button>
                  <button role="menuitem" class="menu-danger" disabled={batchBusy} onclick={() => requestDelete(group.sessions)}>✕ <span>删除项目会话（{group.sessions.length}）</span></button>
                </div>
              {/if}
            </span>
          {/if}
        </div>
        {#if open}
          <div class="project-sessions">
            {#each group.sessions as s}
              {@render sessionItem(s)}
            {/each}
          </div>
        {/if}
      </section>
    {/snippet}

    {#if projectGroups.length === 0}
      <div class="side-empty">{archivedCount ? "暂无最近会话" : "暂无项目会话"}</div>
    {/if}
    {#each projectGroups as group}
      {@render projectGroup(group, projectOpen[group.path] !== false, toggleProject, false)}
    {/each}

    {#if archivedCount}
      <button class="side-archive-toggle" class:open={showArchived}
        aria-expanded={showArchived} onclick={() => (showArchived = !showArchived)}>
        <span class="side-archive-main">
          <span class="side-archive-caret" aria-hidden="true">›</span>
          <span>已归档</span>
        </span>
        <span class="side-archive-count">{archivedCount}</span>
      </button>
      {#if showArchived}
        <div class="side-archived-list">
          {#each archivedProjectGroups as group}
            {@render projectGroup(group, archivedProjectOpen[group.path] !== false, toggleArchivedProject, true)}
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  {#if selecting}
    <div class="side-batch-bar" role="toolbar" aria-label="批量操作">
      <span class="side-batch-count">已选 {selectedRows.length}</span>
      <button onclick={selectAll} disabled={batchBusy}>全选</button>
      <button disabled={batchBusy || !selectedRows.some((r) => !r.archived)} onclick={() => archiveSelected(true)}>归档</button>
      <button disabled={batchBusy || !selectedRows.some((r) => r.archived)} onclick={() => archiveSelected(false)}>取消归档</button>
      <button class="side-batch-delete" disabled={batchBusy || !selectedRows.length} onclick={() => requestDelete(selectedRows)}>删除</button>
      <button class="side-batch-done" onclick={stopSelecting}>完成</button>
    </div>
  {/if}

  {#if dshFooterActions.length}
  <div class="side-plugin-actions">
    {#each dshFooterActions as action}
      <button class="foot-plugin" title={action.description || action.label} onclick={() => openDshSlot(action)}>◇ {action.label}</button>
    {/each}
  </div>
  {/if}
  <div class="side-foot">
    <button class="foot-gear" title="设置" onclick={openSettings} aria-label="设置">⚙</button>
  </div>
</aside>
{#if sidebarOpen}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="sidebar-resizer"
    class:active={sidebarResizing}
    role="separator"
    aria-label="调整侧边栏宽度"
    aria-orientation="vertical"
    aria-valuemin={SIDEBAR_MIN_WIDTH}
    aria-valuemax={SIDEBAR_MAX_WIDTH}
    aria-valuenow={sidebarWidth}
    tabindex="0"
    title="拖动调整侧边栏宽度，双击恢复默认"
    onpointerdown={beginSidebarResize}
    ondblclick={() => setSidebarWidth(SIDEBAR_DEFAULT_WIDTH)}
    onkeydown={handleSidebarResizeKey}
  ></div>
{/if}
{#if renameTarget}
  <div class="overlay session-rename-overlay" role="presentation">
    <form class="modal session-rename-modal" onsubmit={(event) => { event.preventDefault(); void submitRename(); }}>
      <h3>重命名会话</h3>
      <label for="session-rename-input"><span>会话名称</span></label>
      <input id="session-rename-input" bind:value={renameDraft} maxlength="36" disabled={renameSaving} />
      <div class="rename-count">{[...renameDraft].length}/36</div>
      {#if renameError}<div class="rename-error" role="alert">{renameError}</div>{/if}
      <div class="abtns"><button type="button" class="deny" disabled={renameSaving} onclick={() => (renameTarget = null)}>取消</button><button type="submit" class="ok" disabled={renameSaving || !renameDraft.trim()}>{renameSaving ? "保存中…" : "重命名"}</button></div>
    </form>
  </div>
{/if}
{#if deleteTarget}
  <div class="overlay session-delete-overlay" role="presentation">
    <div class="modal session-delete-modal" role="alertdialog" aria-modal="true" aria-labelledby="session-delete-title" aria-describedby="session-delete-desc">
      <h3 id="session-delete-title">永久删除 {deleteTarget.length} 个会话？</h3>
      <p id="session-delete-desc">会话记录将从 BugleCat 中移除且无法恢复；项目文件夹和其中的文件不会被删除。</p>
      <div class="delete-projects">
        {deleteProjects.slice(0, 3).join("、")}{deleteProjects.length > 3 ? ` 等 ${deleteProjects.length} 个项目` : ""}
      </div>
      {#if deleteTarget.some((s) => s.session_id === currentSessionId)}
        <div class="delete-current">包含当前打开的会话，删除前会先切换到新会话。</div>
      {/if}
      <div class="abtns"><button type="button" class="deny" disabled={batchBusy} onclick={() => (deleteTarget = null)}>取消</button><button type="button" class="ok danger" disabled={batchBusy} onclick={() => void confirmDelete()}>{batchBusy ? "删除中…" : "删除"}</button></div>
    </div>
  </div>
{/if}
