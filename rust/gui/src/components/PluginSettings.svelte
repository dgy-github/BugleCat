<script lang="ts">
  type ExternalPlugin = { manifest: { id: string; name: string; version: string }; enabled: boolean };
  type CodexPlugin = {
    manifest: { name: string; version?: string; description?: string };
    root: string;
    enabled: boolean;
    skill_roots: number;
    has_mcp: boolean;
    has_apps: boolean;
    app_count: number;
    has_hooks: boolean;
  };
  type PluginMarketplace = {
    path: string;
    marketplace: { name: string; plugins: { name: string; source: { source: string } }[] };
  };

  let {
    harnessDiagnostics,
    externalPlugins,
    codexPlugins,
    pluginMarketplaces,
    addExternalPlugin,
    upgradeExternalPlugin,
    toggleExternalPlugin,
    addCodexPlugin,
    upgradeCodexPlugin,
    toggleCodexPlugin,
    removeCodexPlugin,
    installMarketplacePlugin,
  }: {
    harnessDiagnostics: Record<string, boolean> | null;
    externalPlugins: ExternalPlugin[];
    codexPlugins: CodexPlugin[];
    pluginMarketplaces: PluginMarketplace[];
    addExternalPlugin: () => void;
    upgradeExternalPlugin: () => void;
    toggleExternalPlugin: (plugin: ExternalPlugin) => void;
    addCodexPlugin: () => void;
    upgradeCodexPlugin: () => void;
    toggleCodexPlugin: (plugin: CodexPlugin) => void;
    removeCodexPlugin: (plugin: CodexPlugin) => void;
    installMarketplacePlugin: (path: string, name: string, upgrade?: boolean) => void;
  } = $props();
</script>

<section class="plugin-settings" aria-label="Harness 插件设置">
  <div class="catalog-head">
    <div>
      <strong>Harness 运行诊断</strong>
      <p>绿色表示当前 Profile 已挂载对应能力。MCP 默认关闭，只有明确启用后才连接外部进程。</p>
    </div>
  </div>
  {#if harnessDiagnostics}
    <div class="plugin-diagnostics">
      {#each Object.entries(harnessDiagnostics) as [name, active]}
        <span class:active class="plugin-state">{active ? "●" : "○"} {name}</span>
      {/each}
    </div>
  {/if}
  <div class="catalog-head">
    <div><strong>进程隔离外部插件</strong><p>只接受 plugin.toml 和目录内相对命令；DLL、SO、DYLIB 会被拒绝。启停在下次运行时装配时生效。</p></div>
    <div><button class="plain" onclick={addExternalPlugin}>安装本地插件</button><button class="plain" onclick={upgradeExternalPlugin}>升级插件</button></div>
  </div>
  {#if externalPlugins.length}
    {#each externalPlugins as plugin}
      <div class="config-entry">
        <span>{plugin.manifest.name} <code>{plugin.manifest.version}</code></span>
        <code>{plugin.manifest.id} · 协议 v1 · 正式能力握手</code>
        <button class="plain" onclick={() => toggleExternalPlugin(plugin)}>{plugin.enabled ? "停用" : "启用"}</button>
      </div>
    {/each}
  {:else}<p class="settings-note">当前工作区未安装外部插件。</p>{/if}
  <div class="catalog-head">
    <div><strong>OpenAI Codex 资源插件</strong><p>兼容 .codex-plugin/plugin.json，仅装载 Skills、MCP、Apps 和 Hooks 等资源，不直接执行 DLL。</p></div>
    <div><button class="plain" onclick={addCodexPlugin}>安装本地资源包</button><button class="plain" onclick={upgradeCodexPlugin}>升级资源包</button></div>
  </div>
  {#if codexPlugins.length}
    {#each codexPlugins as plugin}
      <div class="config-entry">
        <span>{plugin.manifest.name} <code>{plugin.manifest.version || "未标版本"}</code></span>
        <code>{plugin.manifest.description || plugin.root}</code>
        <span>{plugin.skill_roots} Skills · MCP {plugin.has_mcp ? "有" : "无"} · Apps {plugin.has_apps ? `${plugin.app_count} 个` : "无"} · Hooks {plugin.has_hooks ? "有" : "无"}</span>
        <button class="plain" onclick={() => toggleCodexPlugin(plugin)}>{plugin.enabled ? "停用" : "启用"}</button>
        <button class="plain" onclick={() => removeCodexPlugin(plugin)}>卸载</button>
      </div>
    {/each}
  {:else}<p class="settings-note">当前工作区未安装 Codex 资源插件。</p>{/if}
  <div class="catalog-head">
    <div><strong>插件 Marketplace</strong><p>自动发现 OpenAI、Claude 与 Cursor 兼容目录；本地来源直接安装，Git/NPM 来源先下载到工作区隔离暂存目录，校验后再安装。</p></div>
  </div>
  {#if pluginMarketplaces.length}
    {#each pluginMarketplaces as entry}
      <p class="settings-note">{entry.marketplace.name} · {entry.path}</p>
      {#each entry.marketplace.plugins as plugin}
        <div class="config-entry">
          <span>{plugin.name}</span>
          <code>{plugin.source.source}</code>
          <button class="plain" onclick={() => installMarketplacePlugin(entry.path, plugin.name)}>安装</button>
          <button class="plain" onclick={() => installMarketplacePlugin(entry.path, plugin.name, true)}>升级</button>
        </div>
      {/each}
    {/each}
  {:else}<p class="settings-note">当前工作区未发现 Marketplace 清单。</p>{/if}
</section>
