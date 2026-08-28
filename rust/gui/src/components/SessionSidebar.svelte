<script lang="ts">
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

<aside class="sidebar" class:collapsed={!sidebarOpen}>
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
      </span>
    </div>
    {#snippet sessionItem(s: SessionRow)}
      <div class="recent-item" class:active={s.session_id === currentSessionId} class:running={runningSessions.has(s.session_id)} class:archived={s.archived}
        role="treeitem" tabindex="-1" aria-selected={s.session_id === currentSessionId}
        onclick={(event) => { if (!(event.target as HTMLElement).closest(".recent-menu-wrap")) resumeSession(s.session_id, s.title); }}
        onkeydown={(event) => { if (event.key === "Enter" || event.key === " ") resumeSession(s.session_id, s.title); }}>
        <button class="recent-main" title={s.snippet || s.title} disabled={switchingSession}
          aria-label={`打开会话“${s.title}”`}>
          <span class="recent-dot">●</span>
          <span class="recent-text">
            <span class="recent-title">{s.title || "（未命名）"}</span>
            <span class="recent-when">{runningSessions.has(s.session_id) ? "执行中" : fmtWhen(s.updated_at)}{s.archived ? " · 已归档" : ""}</span>
          </span>
        </button>
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
            </div>
          {/if}
        </span>
      </div>
    {/snippet}

    {#if projectGroups.length === 0}
      <div class="side-empty">{archivedCount ? "暂无最近会话" : "暂无项目会话"}</div>
    {/if}
    {#each projectGroups as group}
      <section class="project-group" class:current={group.path === workspace}>
        <button class="project-toggle" class:open={projectOpen[group.path] !== false}
          aria-expanded={projectOpen[group.path] !== false} onclick={() => toggleProject(group.path)}
          title={group.path}>
          <span class="project-caret" aria-hidden="true"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m6 3.5 4.5 4.5L6 12.5"/></svg></span>
          <span class="project-folder" aria-hidden="true"><img src={buglecatAsset("project", 24)} alt="" /></span>
          <span class="project-name">{baseName(group.path)}</span>
          <span class="project-count">{group.sessions.length}</span>
        </button>
        {#if projectOpen[group.path] !== false}
          <div class="project-sessions">
            {#each group.sessions as s}
              {@render sessionItem(s)}
            {/each}
          </div>
        {/if}
      </section>
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
            <section class="project-group archived-project-group" class:current={group.path === workspace}>
              <button class="project-toggle" class:open={archivedProjectOpen[group.path] !== false}
                aria-expanded={archivedProjectOpen[group.path] !== false} onclick={() => toggleArchivedProject(group.path)}
                title={group.path}>
                <span class="project-caret" aria-hidden="true"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m6 3.5 4.5 4.5L6 12.5"/></svg></span>
                <span class="project-folder" aria-hidden="true"><img src={buglecatAsset("project", 24)} alt="" /></span>
                <span class="project-name">{baseName(group.path)}</span>
                <span class="project-count">{group.sessions.length}</span>
              </button>
              {#if archivedProjectOpen[group.path] !== false}
                <div class="project-sessions">
                  {#each group.sessions as s}
                    {@render sessionItem(s)}
                  {/each}
                </div>
              {/if}
            </section>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

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
