<script lang="ts">
  type UiSlot = { plugin: string; slot: "settings.plugins.tab" | "sidebar.footer.action" | "shell.overlay"; id: string; label: string; order: number; description: string; url?: string };
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
    ui_slots: UiSlot[];
    ui_slot_error?: string;
  };
  type PluginMarketplace = {
    path: string;
    marketplace: { name: string; plugins: { name: string; source: { source: string } }[] };
  };
  type DshCategory = { id: string; en: string; zh: string; count: number };
  type DshMarketItem = { source: string; id: string; name: string; summary: string; repository?: string; package?: string; version?: string; category: string; compatibility: "review" | "incompatible"; compatibilityReason: string };
  type DshMarketPreview = { package: string; version: string; compatibility: "convertible" | "ui-adapter" | "incompatible"; compatible: boolean; reason: string; risks: string[] };
  type ProviderRouteDiagnostics = { active_provider_id: string; protocol: string; base_url: string; model: string; has_api_key: boolean; route_count: number };
  type ProviderActivationDiagnostics = { generation: number; status: "idle" | "validating" | "active" | "failed"; last_error?: string | null; updated_at_ms: number };
  type HarnessDiagnostics = Record<string, boolean | ProviderRouteDiagnostics | ProviderActivationDiagnostics> & { provider_route?: ProviderRouteDiagnostics; provider_activation?: ProviderActivationDiagnostics };

  let {
    harnessDiagnostics,
    alibabaAttachmentParserEnabled = $bindable(),
    externalPlugins,
    codexPlugins,
    pluginMarketplaces,
    dshSource = $bindable(), dshManifestUrl = $bindable(), dshQuery = $bindable(), dshCategory = $bindable(),
    dshCategories, dshItems, dshPreview, dshSelected, dshLoading, dshError,
    searchDshMarketplace, previewDshMarketplace, installDshMarketplace,
    addExternalPlugin,
    upgradeExternalPlugin,
    toggleExternalPlugin,
    addCodexPlugin,
    upgradeCodexPlugin,
    toggleCodexPlugin,
    removeCodexPlugin,
    installMarketplacePlugin,
  }: {
    harnessDiagnostics: HarnessDiagnostics | null;
    alibabaAttachmentParserEnabled: boolean;
    externalPlugins: ExternalPlugin[];
    codexPlugins: CodexPlugin[];
    pluginMarketplaces: PluginMarketplace[];
    dshSource: string; dshManifestUrl: string; dshQuery: string; dshCategory: string; dshCategories: DshCategory[]; dshItems: DshMarketItem[];
    dshPreview: DshMarketPreview | null; dshSelected: DshMarketItem | null; dshLoading: boolean; dshError: string;
    searchDshMarketplace: () => void; previewDshMarketplace: (item: DshMarketItem) => void;
    installDshMarketplace: (upgrade?: boolean) => void;
    addExternalPlugin: () => void;
    upgradeExternalPlugin: () => void;
    toggleExternalPlugin: (plugin: ExternalPlugin) => void;
    addCodexPlugin: () => void;
    upgradeCodexPlugin: () => void;
    toggleCodexPlugin: (plugin: CodexPlugin) => void;
    removeCodexPlugin: (plugin: CodexPlugin) => void;
    installMarketplacePlugin: (path: string, name: string, upgrade?: boolean) => void;
  } = $props();
  const visibleDshItems = $derived(dshCategory === "all" ? dshItems : dshItems.filter((item) => item.category === dshCategory));
  const capabilityDiagnostics = $derived(harnessDiagnostics ? Object.entries(harnessDiagnostics).filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean") : []);
  const activationLabels = { idle: "尚未切换", validating: "正在验证模型目录", active: "目录验证通过，Route 已切换", failed: "目录验证或切换失败" } as const;
</script>

<section class="plugin-settings" aria-label="Harness 插件设置">
  <div class="catalog-head">
    <div><strong>本地能力插件</strong><p>按需启用，不具备附件能力的主模型不会自动调用外部解析服务。</p></div>
  </div>
  <div class="config-entry">
    <span><strong>阿里本地附件解析</strong><code>ncx.alibaba-attachment-parser</code></span>
    <code>使用已配置的阿里视觉模型解析图片；默认关闭</code>
    <button class="plain" aria-pressed={alibabaAttachmentParserEnabled} onclick={() => (alibabaAttachmentParserEnabled = !alibabaAttachmentParserEnabled)}>{alibabaAttachmentParserEnabled ? "已启用（保存后当前对话下一轮生效）" : "启用"}</button>
  </div>
  <div class="catalog-head">
    <div>
      <strong>Harness 运行诊断</strong>
      <p>绿色表示当前 Profile 已挂载对应能力。MCP 默认关闭，只有明确启用后才连接外部进程。</p>
    </div>
  </div>
  {#if harnessDiagnostics}
    <div class="plugin-diagnostics">
      {#each capabilityDiagnostics as [name, active]}
        <span class:active class="plugin-state">{active ? "●" : "○"} {name}</span>
      {/each}
    </div>
    {#if harnessDiagnostics.provider_route}
      <div class="config-entry provider-route-diagnostic">
        <span><strong>当前 Provider Route</strong><code>{harnessDiagnostics.provider_route.active_provider_id}</code></span>
        <code>{harnessDiagnostics.provider_route.protocol} · {harnessDiagnostics.provider_route.model}</code>
        <code>{harnessDiagnostics.provider_route.base_url} · Token {harnessDiagnostics.provider_route.has_api_key ? "已配置" : "缺失"} · 自定义模型商 {harnessDiagnostics.provider_route.route_count}</code>
      </div>
    {/if}
    {#if harnessDiagnostics.provider_activation}
      <div class="config-entry provider-activation-diagnostic" class:failed={harnessDiagnostics.provider_activation.status === "failed"}>
        <span><strong>最近一次模型切换</strong><code>#{harnessDiagnostics.provider_activation.generation}</code></span>
        <code>{activationLabels[harnessDiagnostics.provider_activation.status]}{harnessDiagnostics.provider_activation.updated_at_ms ? ` · ${new Date(harnessDiagnostics.provider_activation.updated_at_ms).toLocaleString()}` : ""}</code>
        {#if harnessDiagnostics.provider_activation.last_error}<span role="alert">{harnessDiagnostics.provider_activation.last_error}</span>{/if}
      </div>
    {/if}
  {/if}
  <div class="catalog-head dsh-market-head">
    <div><strong>DSH Community Marketplace</strong><p>接入 dshfind、DeepSeek 1024 Store 与同源 HTTPS 标准目录。目录结果只用于浏览，安装前必须由 Host 再次核验。</p></div>
  </div>
  <div class="dsh-market-toolbar">
    <select bind:value={dshSource} aria-label="DSH 市场源" onchange={() => (dshCategory = "all")}>
      <option value="dshfind">dshfind</option>
      <option value="dsh-1024store">DeepSeek 1024 Store</option>
      <option value="standard-http">标准 HTTP v1</option>
    </select>
    {#if dshSource === "standard-http"}<input bind:value={dshManifestUrl} placeholder="https://example.com/catalog-source.json" aria-label="标准市场清单地址" />{/if}
    <input bind:value={dshQuery} placeholder="搜索插件" aria-label="搜索 DSH 插件" onkeydown={(event) => { if (event.key === "Enter") searchDshMarketplace(); }} />
    <button class="plain" onclick={searchDshMarketplace} disabled={dshLoading}>{dshLoading ? "加载中…" : "搜索"}</button>
  </div>
  {#if dshCategories.length}
    <div class="dsh-category-strip" aria-label="DSH 插件分类">
      <button class:active={dshCategory === "all"} aria-pressed={dshCategory === "all"} onclick={() => (dshCategory = "all")}>全部</button>
      {#each dshCategories as category}
        <button class:active={dshCategory === category.id} aria-pressed={dshCategory === category.id} onclick={() => (dshCategory = category.id)}>{category.zh}<small>{category.count}</small></button>
      {/each}
    </div>
  {/if}
  {#if dshError}<div class="dsh-market-error" role="alert">{dshError}</div>{/if}
  {#if visibleDshItems.length}
    <div class="dsh-market-grid">
      {#each visibleDshItems as item}
        <article class="dsh-market-item" class:incompatible={item.compatibility === "incompatible"} data-category={item.category}>
          <div><strong>{item.name}</strong><code>{item.package && item.version ? `${item.package}@${item.version}` : item.id}</code></div>
          <p>{item.summary}</p>
          <div class="dsh-market-compat"><span class={item.compatibility}>{item.compatibility === "incompatible" ? "不兼容" : "待核验"}</span><small>{item.compatibilityReason}</small></div>
          <button class="plain" disabled={dshLoading || item.compatibility === "incompatible"} onclick={() => previewDshMarketplace(item)}>风险预览</button>
        </article>
      {/each}
    </div>
  {:else if dshItems.length && dshCategory !== "all"}
    <p class="settings-note">当前搜索结果中没有这个分类的插件。</p>
  {/if}
  {#if dshPreview && dshSelected}
    <div class="dsh-preview" class:blocked={!dshPreview.compatible}>
      <div><strong>{dshSelected.name} · {dshPreview.compatibility === "convertible" ? "可转换" : dshPreview.compatibility === "ui-adapter" ? "可安全映射 UI" : "不兼容"}</strong><p>{dshPreview.reason}</p></div>
      <ul>{#each dshPreview.risks as risk}<li>{risk}</li>{/each}</ul>
      <div class="dsh-preview-actions"><button class="plain" disabled={!dshPreview.compatible || dshLoading} onclick={() => installDshMarketplace(false)}>安装</button><button class="plain" disabled={!dshPreview.compatible || dshLoading} onclick={() => installDshMarketplace(true)}>升级</button></div>
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
    {@const settingsSlots = codexPlugins.flatMap((plugin) => plugin.ui_slots || []).filter((item) => item.slot === "settings.plugins.tab").sort((a, b) => a.order - b.order)}
    {#if settingsSlots.length}
      <div class="dsh-settings-slots" aria-label="DSH 设置页插件入口">
        {#each settingsSlots as slot}
          <div><strong>{slot.label}</strong><small>{slot.description}</small>{#if slot.url}<a href={slot.url} target="_blank" rel="noreferrer">插件主页 ↗</a>{/if}</div>
        {/each}
      </div>
    {/if}
    {#each codexPlugins as plugin}
      <div class="config-entry">
        <span>{plugin.manifest.name} <code>{plugin.manifest.version || "未标版本"}</code></span>
        <code>{plugin.manifest.description || plugin.root}</code>
        <span>{plugin.skill_roots} Skills · MCP {plugin.has_mcp ? "有" : "无"} · Apps {plugin.has_apps ? `${plugin.app_count} 个` : "无"} · Hooks {plugin.has_hooks ? "有" : "无"}</span>
        {#if plugin.ui_slots?.length}<span class="ui-slot-summary">DSH UI：{plugin.ui_slots.map((item) => item.slot).join(" · ")}</span>{/if}
        {#if plugin.ui_slot_error}<span class="ui-slot-error">UI 声明未加载：{plugin.ui_slot_error}</span>{/if}
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
