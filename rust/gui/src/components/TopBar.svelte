<script lang="ts">
  import { buglecatAsset } from "../lib/buglecat-assets";
  let {
    sidebarOpen, sessionTitle, busy, rightPanel, toggleSidebar,
    activeView = $bindable(), openFiles, openDiff, openBranches, openHermes, openCheckpoints,
    themeMode, cycleTheme,
  }: {
    sidebarOpen: boolean; sessionTitle: string; busy: boolean; rightPanel: string; activeView: "chat" | "trajectory";
    toggleSidebar: () => void; openFiles: () => void; openDiff: () => void;
    openBranches: () => void; openHermes: () => void; openCheckpoints: () => void;
    themeMode: "system" | "light" | "dark"; cycleTheme: () => void;
  } = $props();
</script>

<header class="topbar">
  {#if !sidebarOpen}<button class="collapse" onclick={toggleSidebar} title="展开侧边栏" aria-label="展开侧边栏"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="4" width="17" height="16" rx="3"/><path d="M9 4v16"/><path d="m12.5 9 3 3-3 3"/></svg></button>{/if}
  <span class="title">{sessionTitle}</span>
  {#if busy}<img class="status-cat" src={buglecatAsset("processing", 24)} alt="处理中" title="处理中…" />{/if}
  <nav class="conversation-tabs" aria-label="会话视图">
    <button class="conversation-tab" class:active={activeView === "chat"} onclick={() => (activeView = "chat")}>对话</button>
    <button class="conversation-tab" class:active={activeView === "trajectory"} onclick={() => (activeView = "trajectory")}>轨迹</button>
  </nav>
  <span class="topbar-actions" aria-label="工作区工具">
    <button class="tbtn theme-quick" onclick={cycleTheme} title={`外观：${themeMode === "system" ? "跟随系统" : themeMode === "light" ? "浅色" : "深色"}`} aria-label="切换外观主题">{themeMode === "system" ? "◐" : themeMode === "light" ? "☀" : "☾"}</button>
    <button class="tbtn files" class:on={rightPanel === "files"} onclick={openFiles} title="浏览工作区文件" aria-label="浏览工作区文件" aria-pressed={rightPanel === "files"}><svg class="ni" viewBox="0 0 24 24" aria-hidden="true"><path class="icon-soft" d="M2.75 7.25A2.75 2.75 0 0 1 5.5 4.5h3.12c.54 0 1.05.22 1.43.6l1.4 1.4h7.05a2.75 2.75 0 0 1 2.75 2.75v1H2.75z"/><path class="icon-main" d="M3.5 9h17a1.5 1.5 0 0 1 1.45 1.88l-1.7 6.5a2.75 2.75 0 0 1-2.66 2.05H5.42a2.75 2.75 0 0 1-2.67-2.08L1.98 14.3A4.4 4.4 0 0 1 3.5 9Z"/></svg></button>
    <button class="tbtn diff" class:on={rightPanel === "diff"} onclick={openDiff} title="查看工作区改动" aria-label="查看工作区改动" aria-pressed={rightPanel === "diff"}><svg class="ni" viewBox="0 0 24 24" aria-hidden="true"><path class="icon-soft" d="M5 3.25h6.25A2.75 2.75 0 0 1 14 6v12A2.75 2.75 0 0 1 11.25 20.75H5A2.75 2.75 0 0 1 2.25 18V6A2.75 2.75 0 0 1 5 3.25Z"/><path class="icon-main" d="M17.75 4.25a1 1 0 0 1 1 1V7.5H21a1 1 0 1 1 0 2h-2.25v2.25a1 1 0 1 1-2 0V9.5H14.5a1 1 0 1 1 0-2h2.25V5.25a1 1 0 0 1 1-1ZM15 16h6a1 1 0 1 1 0 2h-6a1 1 0 1 1 0-2Z"/></svg></button>
    <button class="tbtn branches" class:on={rightPanel === "branches"} onclick={openBranches} title="管理 Git 分支" aria-label="管理 Git 分支" aria-pressed={rightPanel === "branches"}><svg class="ni" viewBox="0 0 24 24" aria-hidden="true"><path class="icon-soft" d="M6.25 7.5a2.25 2.25 0 1 0 0-4.5 2.25 2.25 0 0 0 0 4.5Zm0 13.5a2.25 2.25 0 1 0 0-4.5 2.25 2.25 0 0 0 0 4.5ZM18 10.25a2.25 2.25 0 1 0 0-4.5 2.25 2.25 0 0 0 0 4.5Z"/><path class="icon-main" d="M6.25 6.5a1 1 0 0 1 1 1v5.1c3.9-.43 6.84-2.08 8.1-4.55a1 1 0 1 1 1.78.9c-1.66 3.28-5.27 5.25-9.88 5.69v1.86a1 1 0 1 1-2 0v-9a1 1 0 0 1 1-1Z"/></svg></button>
    <button class="tbtn memory" class:on={rightPanel === "memory"} onclick={openHermes} title="打开项目记忆" aria-label="打开项目记忆" aria-pressed={rightPanel === "memory"}><svg class="ni" viewBox="0 0 24 24" aria-hidden="true"><path class="icon-soft" d="M6 3h11.25A2.75 2.75 0 0 1 20 5.75V19a2 2 0 0 1-2 2H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3Z"/><path class="icon-main" d="M7 3h2.25v18H7V3Zm6.5 4h3a1 1 0 1 1 0 2h-3a1 1 0 1 1 0-2Zm0 4h3a1 1 0 1 1 0 2h-3a1 1 0 1 1 0-2Z"/></svg></button>
    <button class="tbtn checkpoints" class:on={rightPanel === "checkpoints"} onclick={openCheckpoints} title="查看与恢复检查点" aria-label="查看与恢复检查点" aria-pressed={rightPanel === "checkpoints"}><svg class="ni" viewBox="0 0 24 24" aria-hidden="true"><path class="icon-soft" d="M12 2.5a9.5 9.5 0 1 1-8.93 6.25 1 1 0 0 1 1.88.68A7.5 7.5 0 1 0 7 6.32V8.5a1 1 0 0 1-2 0V4a1 1 0 0 1 1-1h4.5a1 1 0 0 1 0 2H8.47A9.45 9.45 0 0 1 12 2.5Z"/><path class="icon-main" d="M12 6.5a1 1 0 0 1 1 1v4l2.75 1.65a1 1 0 1 1-1.03 1.7l-3.24-1.94A1 1 0 0 1 11 12V7.5a1 1 0 0 1 1-1Z"/></svg></button>
  </span>
</header>
