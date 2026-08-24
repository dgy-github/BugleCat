<script lang="ts">
  type SessionRow = {
    session_id: string;
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
    showRecent = $bindable(),
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
    chooseWorkspace,
    openSettings,
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
    showRecent: boolean;
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
    chooseWorkspace: () => void;
    openSettings: () => void;
    fmtWhen: (value: string) => string;
    baseName: (path: string) => string;
    beginSidebarResize: (event: PointerEvent) => void;
    setSidebarWidth: (width: number) => void;
    handleSidebarResizeKey: (event: KeyboardEvent) => void;
  } = $props();
</script>

<aside class="sidebar" class:collapsed={!sidebarOpen}>
  <div class="side-head">
    <span class="side-brand">nanocodex</span>
    <button class="side-collapse" onclick={toggleSidebar} title="收起侧边栏" aria-label="收起侧边栏">‹</button>
  </div>
  <button class="new-session" onclick={newSession}>
    <svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
    新会话
  </button>

  <div class="side-recents">
    {#snippet sessionItem(s: SessionRow)}
      <div class="recent-item" class:active={s.session_id === currentSessionId} class:running={runningSessions.has(s.session_id)} class:archived={s.archived}>
        <button class="recent-main" title={s.snippet || s.title} disabled={switchingSession || !s.has_snapshot}
          onclick={() => resumeSession(s.session_id, s.title)}>
          <span class="recent-dot">●</span>
          <span class="recent-text">
            <span class="recent-title">{s.title || "（未命名）"}</span>
            <span class="recent-when">{runningSessions.has(s.session_id) ? "执行中" : fmtWhen(s.updated_at)}{s.archived ? " · 已归档" : ""}</span>
          </span>
        </button>
        <button class="recent-act" title="从此处分叉新会话" disabled={switchingSession || !s.has_snapshot}
          onclick={() => forkSession(s.session_id, s.title)} aria-label="分叉">⑂</button>
        <button class="recent-act" title="打开会话日志 (JSONL)"
          onclick={() => openSessionLog(s.session_id)} aria-label="打开日志">📄</button>
        <button class="recent-act" title={s.archived ? "取消归档" : "归档此会话"}
          onclick={() => archiveSession(s.session_id, !s.archived)} aria-label="归档">{s.archived ? "↩" : "🗄"}</button>
      </div>
    {/snippet}

    <button class="side-recent-toggle" class:open={showRecent}
      aria-expanded={showRecent} onclick={() => (showRecent = !showRecent)}>
      <span class="side-recent-main">
        <span class="side-recent-caret" aria-hidden="true">›</span>
        <span>最近会话</span>
      </span>
      <span class="side-recent-count">{recentSessions.length}</span>
    </button>
    {#if showRecent}
      <div class="side-recent-list">
        {#if recentSessions.length === 0}
          <div class="side-empty">{archivedCount ? "暂无最近会话" : "暂无会话"}</div>
        {/if}
        {#each recentSessions as s}
          {@render sessionItem(s)}
        {/each}
      </div>
    {/if}

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
          {#each archivedSessions as s}
            {@render sessionItem(s)}
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <div class="side-foot">
    <button class="foot-ws" title={`工作区：${workspace}（点击切换）`} onclick={chooseWorkspace}>
      📁 {workspace ? baseName(workspace) : "选择工作区"}
    </button>
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
