<script lang="ts">
  let {
    sidebarOpen, sessionTitle, busy, rightPanel, toggleSidebar,
    openFiles, openDiff, openBranches, openHermes, openCheckpoints,
  }: {
    sidebarOpen: boolean; sessionTitle: string; busy: boolean; rightPanel: string;
    toggleSidebar: () => void; openFiles: () => void; openDiff: () => void;
    openBranches: () => void; openHermes: () => void; openCheckpoints: () => void;
  } = $props();
</script>

<header class="topbar">
  <button class="collapse" onclick={toggleSidebar} title={sidebarOpen ? "收起侧边栏" : "展开侧边栏"} aria-label="切换侧边栏">▣</button>
  <span class="title">{sessionTitle}</span>
  {#if busy}<span class="spinner" title="处理中…">●</span>{/if}
  <span class="topbar-actions">
    <button class="tbtn" class:on={rightPanel === "files"} onclick={openFiles} title="文件" aria-label="文件"><svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h3.5l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg></button>
    <button class="tbtn" class:on={rightPanel === "diff"} onclick={openDiff} title="改动" aria-label="改动"><svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M5 8h7M8.5 4.5v7M5 17h7"/></svg></button>
    <button class="tbtn" class:on={rightPanel === "branches"} onclick={openBranches} title="分支" aria-label="分支"><svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="6" cy="6" r="2.2"/><circle cx="6" cy="18" r="2.2"/><circle cx="18" cy="8" r="2.2"/><path d="M6 8.2v7.6M6 13a6 6 0 0 0 6-6h3.8"/></svg></button>
    <button class="tbtn" class:on={rightPanel === "memory"} onclick={openHermes} title="记忆" aria-label="记忆"><svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"><path d="M5 4h11a2 2 0 0 1 2 2v14H7a2 2 0 0 1-2-2z"/><path d="M9 4v16"/></svg></button>
    <button class="tbtn" class:on={rightPanel === "checkpoints"} onclick={openCheckpoints} title="检查点" aria-label="检查点"><svg class="ni" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M12 8v4.2l2.8 1.7"/></svg></button>
  </span>
</header>
