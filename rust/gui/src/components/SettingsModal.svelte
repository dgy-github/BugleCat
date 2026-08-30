<script lang="ts">
  import { tick } from "svelte";
  import PluginSettings from "./PluginSettings.svelte";
  import ModelCatalogSettings from "./ModelCatalogSettings.svelte";
  import CustomProvidersSettings from "./CustomProvidersSettings.svelte";
  type Settings = {
    model: string;
    base_url: string;
    vl_model: string;
    vl_base_url: string;
    reasoning_effort: string;
    max_iterations: number;
    max_tool_calls: number;
    orchestrator_workers: number;
    orchestrator_high_workers: number;
    orchestrator_verify_retries: number;
    orchestrator_max_depth: number;
    orchestrator_max_subtasks: number;
    context_edit_enabled: boolean;
    context_edit_max_chars: number;
    context_edit_keep_recent_messages: number;
    context_edit_max_tool_result_chars: number;
    alibaba_attachment_parser_enabled: boolean;
    price_in: number;
    price_out: number;
    price_currency: "CNY" | "USD";
    api_key_masked: string;
    has_api_key: boolean;
    deepseek_api_key_masked: string;
    has_deepseek_api_key: boolean;
    yunmo_api_key_masked: string;
    has_yunmo_api_key: boolean;
    vl_api_key_masked: string;
    has_vl_api_key: boolean;
    dashscope_token_plan_key_masked: string;
    has_dashscope_token_plan_key: boolean;
    dashscope_workspace_key_masked: string;
    has_dashscope_workspace_key: boolean;
    available_models: string[];
  };
  type CatalogModel = {
    provider_id: string;
    model_id: string;
    display_name: string;
    base_url: string;
    price_in: number;
    price_out: number;
    price_currency: "CNY" | "USD";
    price_source: "official_direct" | "aggregator";
    pricing_note: string | null;
    source_url: string;
    updated_at: string;
  };
  type CatalogProvider = { id: string; name: string; models: CatalogModel[] };
  type ExternalPlugin = { manifest: { id: string; name: string; version: string }; enabled: boolean };
  type UiSlot = { plugin: string; slot: "settings.plugins.tab" | "sidebar.footer.action" | "shell.overlay"; id: string; label: string; order: number; description: string; url?: string };
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
  type HarnessDiagnostics = Record<string, boolean | { active_provider_id: string; protocol: string; base_url: string; model: string; has_api_key: boolean; route_count: number } | { generation: number; status: string; last_error?: string | null; updated_at_ms: number }>;
  type DshCategory = { id: string; en: string; zh: string; count: number };
  type DshMarketItem = { source: string; id: string; name: string; summary: string; repository?: string; package?: string; version?: string; category: string; compatibility: "review" | "incompatible"; compatibilityReason: string };
  type DshMarketPreview = { package: string; version: string; compatibility: "convertible" | "ui-adapter" | "incompatible"; compatible: boolean; reason: string; risks: string[] };

  let {
    settings = $bindable(),
    apiKeyInput = $bindable(),
    deepseekApiKeyInput = $bindable(),
    yunmoApiKeyInput = $bindable(),
    vlApiKeyInput = $bindable(),
    dashscopeTokenPlanKeyInput = $bindable(),
    dashscopeWorkspaceKeyInput = $bindable(),
    configLocation,
    modelCatalog,
    officialProviders,
    yunmoProvider,
    openRouterProvider,
    catalogRefreshing,
    yunmoRefreshing,
    presetSaving,
    saving,
    harnessDiagnostics,
    externalPlugins,
    codexPlugins,
    pluginMarketplaces,
    dshSource = $bindable(), dshManifestUrl = $bindable(), dshQuery = $bindable(), dshCategory = $bindable(),
    dshCategories, dshItems, dshPreview, dshSelected, dshLoading, dshError,
    themeMode = $bindable(), setThemeMode,
    executionMode, selectExecutionMode,
    harnessProfile, harnessProfiles, selectHarnessProfile, harnessProfileLabel, harnessProfileLocked,
    REASONING_EFFORTS,
    currencySymbol,
    currencyName,
    priceSourceName,
    currentPriceSourceName,
    openConfigFile,
    openConfigDir,
    applyModelPreset,
    openPriceSource,
    refreshOpenRouterModels,
    refreshYunmoModels,
    customProviderActivated,
    addExternalPlugin,
    upgradeExternalPlugin,
    toggleExternalPlugin,
    addCodexPlugin,
    upgradeCodexPlugin,
    toggleCodexPlugin,
    removeCodexPlugin,
    installMarketplacePlugin,
    searchDshMarketplace,
    previewDshMarketplace,
    installDshMarketplace,
    saveSettings,
  }: {
    settings: Settings | null;
    apiKeyInput: string;
    deepseekApiKeyInput: string;
    yunmoApiKeyInput: string;
    vlApiKeyInput: string;
    dashscopeTokenPlanKeyInput: string;
    dashscopeWorkspaceKeyInput: string;
    configLocation: { config_path: string; config_dir: string } | null;
    modelCatalog: { providers: CatalogProvider[] } | null;
    officialProviders: CatalogProvider[];
    yunmoProvider: CatalogProvider | null;
    openRouterProvider: CatalogProvider | null;
    catalogRefreshing: boolean;
    yunmoRefreshing: boolean;
    presetSaving: string;
    saving: boolean;
    harnessDiagnostics: HarnessDiagnostics | null;
    externalPlugins: ExternalPlugin[];
    codexPlugins: CodexPlugin[];
    pluginMarketplaces: PluginMarketplace[];
    dshSource: string; dshManifestUrl: string; dshQuery: string; dshCategory: string; dshCategories: DshCategory[]; dshItems: DshMarketItem[];
    dshPreview: DshMarketPreview | null; dshSelected: DshMarketItem | null; dshLoading: boolean; dshError: string;
    themeMode: "system" | "light" | "dark"; setThemeMode: (mode: "system" | "light" | "dark") => void;
    executionMode: "agent" | "orchestrator";
    selectExecutionMode: (mode: "agent" | "orchestrator") => void;
    harnessProfile: string;
    harnessProfiles: { id: string; label: string; desc: string }[];
    selectHarnessProfile: (profile: string) => void;
    harnessProfileLabel: (profile: string) => string;
    harnessProfileLocked: boolean;
    REASONING_EFFORTS: { id: string; label: string }[];
    currencySymbol: (currency: "CNY" | "USD") => string;
    currencyName: (currency: "CNY" | "USD") => string;
    priceSourceName: (source: "official_direct" | "aggregator") => string;
    currentPriceSourceName: () => string;
    openConfigFile: () => void;
    openConfigDir: () => void;
    applyModelPreset: (provider: CatalogProvider, model: CatalogModel) => void;
    openPriceSource: (url: string) => void;
    refreshOpenRouterModels: () => void;
    refreshYunmoModels: () => void;
    customProviderActivated: (model: string, models: string[]) => void | Promise<void>;
    addExternalPlugin: () => void;
    upgradeExternalPlugin: () => void;
    toggleExternalPlugin: (plugin: ExternalPlugin) => void;
    addCodexPlugin: () => void;
    upgradeCodexPlugin: () => void;
    toggleCodexPlugin: (plugin: CodexPlugin) => void;
    removeCodexPlugin: (plugin: CodexPlugin) => void;
    installMarketplacePlugin: (path: string, name: string, upgrade?: boolean) => void;
    searchDshMarketplace: () => void;
    previewDshMarketplace: (item: DshMarketItem) => void;
    installDshMarketplace: (upgrade?: boolean) => void;
    saveSettings: () => void;
  } = $props();

  type SettingsSection = "general" | "models" | "connection" | "skills" | "context" | "plugins";
  let activeSection = $state<SettingsSection>("general");
  let settingsShell = $state<HTMLElement>();
  let dialogTitle = $state<HTMLHeadingElement>();
  let restoreFocus: HTMLElement | null = null;
  const sections: { id: SettingsSection; icon: string; label: string; detail: string }[] = [
    { id: "general", icon: "⌘", label: "通用", detail: "模型与运行限制" },
    { id: "models", icon: "◇", label: "模型与费用", detail: "厂商目录和计价" },
    { id: "connection", icon: "⌁", label: "连接与媒体", detail: "接口、密钥和视觉" },
    { id: "skills", icon: "✦", label: "Skills", detail: "技能凭据与就绪状态" },
    { id: "context", icon: "☷", label: "上下文", detail: "压缩和保留策略" },
    { id: "plugins", icon: "▦", label: "插件", detail: "能力与运行诊断" },
  ];

  const closeSettings = () => {
    if (!saving) settings = null;
  };

  const handleDialogKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      closeSettings();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(settingsShell?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
    ) ?? []).filter((element) => element.offsetParent !== null);
    if (!focusable.length) {
      event.preventDefault();
      dialogTitle.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  $effect(() => {
    if (!settings) return;
    activeSection = "general";
    restoreFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void tick().then(() => dialogTitle?.focus());
    return () => {
      const target = restoreFocus;
      restoreFocus = null;
      void tick().then(() => target?.focus());
    };
  });
</script>

{#if settings}
  <div class="overlay settings-overlay">
    <div bind:this={settingsShell} class="settings-shell" role="dialog" aria-modal="true" aria-labelledby="settings-title" tabindex="-1" onkeydown={handleDialogKeydown}>
      <header class="settings-header">
        <div><h2 bind:this={dialogTitle} id="settings-title" tabindex="-1">设置</h2><p>配置 BugleCat 的模型、连接与扩展能力</p></div>
        <button class="settings-close" onclick={closeSettings} disabled={saving} aria-label="关闭设置">×</button>
      </header>
      <div class="settings-layout">
        <nav class="settings-nav" aria-label="设置分类">
          {#each sections as section}
            <button class:active={activeSection === section.id} aria-current={activeSection === section.id ? "page" : undefined} onclick={() => (activeSection = section.id)}>
              <span class="settings-nav-icon">{section.icon}</span>
              <span><strong>{section.label}</strong><small>{section.detail}</small></span>
            </button>
          {/each}
          {#if configLocation}
            <div class="settings-config-location">
              <span>配置文件</span><code title={configLocation.config_path}>{configLocation.config_path}</code>
              <div><button onclick={openConfigFile}>打开</button><button onclick={openConfigDir}>文件夹</button></div>
            </div>
          {/if}
        </nav>
        <div class="settings-content">
          {#if activeSection === "general"}
            <section class="settings-page"><div class="settings-page-head"><h3>通用</h3><p>选择默认模型并控制每轮 Agent 的执行规模。</p></div>
              <div class="settings-card">
                <div class="settings-appearance-row"><span><strong>外观主题</strong><small>立即生效，并在下次启动时保留</small></span><div class="theme-segments" role="group" aria-label="外观主题"><button class:active={themeMode === "system"} aria-pressed={themeMode === "system"} onclick={() => setThemeMode("system")}>跟随系统</button><button class:active={themeMode === "light"} aria-pressed={themeMode === "light"} onclick={() => setThemeMode("light")}>浅色</button><button class:active={themeMode === "dark"} aria-pressed={themeMode === "dark"} onclick={() => setThemeMode("dark")}>深色</button></div></div>
                <label><span><strong>Agent / 编排模式</strong><small>立即切换；运行中的任务不变，当前会话下一轮生效</small></span><select value={executionMode} onchange={(event) => selectExecutionMode(event.currentTarget.value as "agent" | "orchestrator")}><option value="agent">Agent（单智能体）</option><option value="orchestrator">多 Agent 编排</option></select></label>
                <label><span><strong>当前会话 Harness</strong><small>{harnessProfileLocked ? `已锁定为 ${harnessProfileLabel(harnessProfile)}；新建会话后可选择` : "决定当前会话的工具与上下文；选择后立即生效"}</small></span><select value={harnessProfile} disabled={harnessProfileLocked} onchange={(event) => selectHarnessProfile(event.currentTarget.value)}>{#each harnessProfiles as option}<option value={option.id}>{option.label} — {option.desc}</option>{/each}</select></label>
                <label><span><strong>当前模型</strong><small>保存后重建当前 Agent，当前会话下一轮立即使用</small></span><select bind:value={settings.model}>{#each settings.available_models as m}<option value={m}>{m}</option>{/each}</select></label>
                <label><span><strong>思考程度</strong><small>适配 DeepSeek Harness 推理等级</small></span><select bind:value={settings.reasoning_effort}>{#each REASONING_EFFORTS as option}<option value={option.id}>{option.label}</option>{/each}</select></label>
                <label><span><strong>模型调用上限</strong><small>单轮最多请求模型的次数</small></span><input type="number" min="1" bind:value={settings.max_iterations} /></label>
                <label><span><strong>工具调用上限</strong><small>0 表示禁用工具调用</small></span><input type="number" min="0" bind:value={settings.max_tool_calls} /></label>
              </div>
              <div class="settings-card"><div class="settings-card-title"><strong>多 Agent 资源预算</strong><small>降低数值可减少调用与费用；保存后下一轮生效</small></div>
                <label><span><strong>普通任务 Worker</strong><small>并行候选数量（1–4）</small></span><input type="number" min="1" max="4" bind:value={settings.orchestrator_workers} /></label>
                <label><span><strong>高风险任务 Worker</strong><small>复杂任务候选数量（1–6）</small></span><input type="number" min="1" max="6" bind:value={settings.orchestrator_high_workers} /></label>
                <label><span><strong>验证重试</strong><small>验证失败后的重跑次数（0–3）</small></span><input type="number" min="0" max="3" bind:value={settings.orchestrator_verify_retries} /></label>
                <label><span><strong>递归深度</strong><small>复杂任务继续拆分的层数（0–2）</small></span><input type="number" min="0" max="2" bind:value={settings.orchestrator_max_depth} /></label>
                <label><span><strong>子任务上限</strong><small>单次分解最多子任务（1–12）</small></span><input type="number" min="1" max="12" bind:value={settings.orchestrator_max_subtasks} /></label>
              </div>
              <div class="settings-callout">权限模式在对话输入框中切换，设置页不再维护第二套沙箱与审批状态。</div>
            </section>
          {:else if activeSection === "models"}
            <section class="settings-page settings-model-page"><div class="settings-page-head"><h3>模型与费用</h3><p>使用厂商官方目录，或为当前模型设置自定义计价。</p></div>
              <CustomProvidersSettings onActivated={(baseUrl, model, models) => { settings.base_url = baseUrl; settings.model = model; settings.available_models = models; customProviderActivated(model, models); }} />
              <ModelCatalogSettings bind:model={settings.model} baseUrl={settings.base_url} {modelCatalog} {officialProviders} {yunmoProvider} {openRouterProvider} {catalogRefreshing} {yunmoRefreshing} {presetSaving} {currencySymbol} {currencyName} {priceSourceName} {applyModelPreset} {openPriceSource} {refreshOpenRouterModels} {refreshYunmoModels} />
              <div class="settings-card"><div class="settings-card-title"><strong>当前模型计价</strong><small>用于会话累计费用估算</small></div>
                <label><span><strong>输入单价</strong><small>{currencySymbol(settings.price_currency)}/百万 Token</small></span><input type="number" min="0" step="0.01" bind:value={settings.price_in} /></label>
                <label><span><strong>输出单价</strong><small>{currencySymbol(settings.price_currency)}/百万 Token</small></span><input type="number" min="0" step="0.01" bind:value={settings.price_out} /></label>
                <label><span><strong>币种</strong><small>当前来源：{currentPriceSourceName()}</small></span><select bind:value={settings.price_currency}><option value="CNY">人民币 CNY</option><option value="USD">美元 USD</option></select></label>
              </div>
            </section>
          {:else if activeSection === "connection"}
            <section class="settings-page"><div class="settings-page-head"><h3>连接与媒体</h3><p>主模型和视觉/媒体能力使用相互独立的连接与密钥。</p></div>
              <div class="settings-card"><div class="settings-card-title"><strong>主模型连接</strong><small>对话、推理和工具规划</small></div>
                <label><span><strong>Base URL</strong><small>OpenAI 兼容接口地址</small></span><input bind:value={settings.base_url} /></label>
                <label><span><strong>API 密钥</strong><small>{settings.has_api_key ? `已配置 ${settings.api_key_masked}` : "尚未配置"}</small></span><input type="password" bind:value={apiKeyInput} placeholder={settings.has_api_key ? "留空保持当前密钥" : "输入 API 密钥"} /></label>
              </div>
              <div class="settings-card"><div class="settings-card-title"><strong>模型商 Token</strong><small>切换目录预设时自动使用对应 Token</small></div>
                <label><span><strong>DeepSeek Token</strong><small>{settings.has_deepseek_api_key ? `已配置 ${settings.deepseek_api_key_masked}` : "尚未单独保存"}</small></span><input type="password" bind:value={deepseekApiKeyInput} placeholder={settings.has_deepseek_api_key ? "留空保持当前 Token" : "输入 DeepSeek Token"} autocomplete="off" /></label>
                <label><span><strong>云末 AI Token</strong><small>{settings.has_yunmo_api_key ? `已配置 ${settings.yunmo_api_key_masked}` : "尚未配置"}</small></span><input type="password" bind:value={yunmoApiKeyInput} placeholder={settings.has_yunmo_api_key ? "留空保持当前 Token" : "输入云末中转 Token"} autocomplete="off" /></label>
              </div>
              <div class="settings-card"><div class="settings-card-title"><strong>视觉与阿里媒体</strong><small>图片附件、生图和 Wan 视频</small></div>
                <label><span><strong>解析模型</strong><small>图片和文件理解模型</small></span><input bind:value={settings.vl_model} placeholder="例如：qwen-vl-max" /></label>
                <label><span><strong>解析接口</strong><small>留空时视觉理解沿用主接口</small></span><input bind:value={settings.vl_base_url} placeholder="留空则沿用主模型接口" /></label>
                <label><span><strong>媒体密钥</strong><small>{settings.has_vl_api_key ? `已配置 ${settings.vl_api_key_masked}` : "需要独立 DashScope Key"}</small></span><input type="password" bind:value={vlApiKeyInput} placeholder={settings.has_vl_api_key ? "留空保持当前密钥" : "输入视觉 / DashScope 密钥"} /></label>
              </div>
              <div class="settings-callout">媒体生成不会把主模型 API Key 发送给阿里云。</div>
            </section>
          {:else if activeSection === "context"}
            <section class="settings-page"><div class="settings-page-head"><h3>上下文</h3><p>控制长会话何时压缩，以及压缩时保留多少近期信息。</p></div>
              <div class="settings-card">
                <label class="settings-switch-row"><span><strong>自动裁剪上下文</strong><small>接近上限时压缩历史工具结果和旧消息</small></span><input type="checkbox" bind:checked={settings.context_edit_enabled} /></label>
                <label><span><strong>字符上限</strong><small>超过后触发上下文整理</small></span><input type="number" min="1" bind:value={settings.context_edit_max_chars} /></label>
                <label><span><strong>保留最近消息</strong><small>压缩时完整保留的消息数量</small></span><input type="number" min="1" bind:value={settings.context_edit_keep_recent_messages} /></label>
                <label><span><strong>工具结果上限</strong><small>单条历史工具结果保留字符数</small></span><input type="number" min="1" bind:value={settings.context_edit_max_tool_result_chars} /></label>
              </div>
            </section>
          {:else if activeSection === "skills"}
            <section class="settings-page"><div class="settings-page-head"><h3>Skills 配置</h3><p>为内置技能配置专用凭据。密钥只以掩码形式回显。</p></div>
              <div class="settings-card"><div class="settings-card-title"><strong>阿里百炼 · Token Plan</strong><small>适用于支持 Token Plan 的百炼能力</small></div>
                <label><span><strong>Token Plan Key</strong><small>{settings.has_dashscope_token_plan_key ? `已配置 ${settings.dashscope_token_plan_key_masked}` : "尚未配置 sk-sp-…"}</small></span><input type="password" bind:value={dashscopeTokenPlanKeyInput} placeholder={settings.has_dashscope_token_plan_key ? "留空保持当前密钥" : "输入 sk-sp-…"} autocomplete="off" /></label>
              </div>
              <div class="settings-card"><div class="settings-card-title"><strong>阿里百炼 · Workspace</strong><small>供 dashscope-image 与 dashscope-video Skills 使用</small></div>
                <label><span><strong>Workspace Key</strong><small>{settings.has_dashscope_workspace_key ? `已配置 ${settings.dashscope_workspace_key_masked}` : "尚未配置 sk-ws-…"}</small></span><input type="password" bind:value={dashscopeWorkspaceKeyInput} placeholder={settings.has_dashscope_workspace_key ? "留空保持当前密钥" : "输入 sk-ws-…"} autocomplete="off" /></label>
                <div class="settings-callout">图片和视频生成优先使用 Workspace Key；未配置时继续兼容“连接与媒体”中的旧媒体密钥。</div>
              </div>
            </section>
          {:else}
            <section class="settings-page settings-plugin-page"><div class="settings-page-head"><h3>插件</h3><p>管理内置能力、外部进程插件和 OpenAI Codex 插件。</p></div>
              <PluginSettings {harnessDiagnostics} bind:alibabaAttachmentParserEnabled={settings.alibaba_attachment_parser_enabled} {externalPlugins} {codexPlugins} {pluginMarketplaces} bind:dshSource bind:dshManifestUrl bind:dshQuery bind:dshCategory {dshCategories} {dshItems} {dshPreview} {dshSelected} {dshLoading} {dshError} {searchDshMarketplace} {previewDshMarketplace} {installDshMarketplace} {addExternalPlugin} {upgradeExternalPlugin} {toggleExternalPlugin} {addCodexPlugin} {upgradeCodexPlugin} {toggleCodexPlugin} {removeCodexPlugin} {installMarketplacePlugin} />
            </section>
          {/if}
        </div>
      </div>
      <footer class="settings-footer"><span>{saving ? "正在写入配置…" : "此按钮只保存表单字段；标注“立即生效”的操作会单独应用"}</span><div><button class="deny" onclick={closeSettings} disabled={saving}>取消</button><button class="ok" onclick={saveSettings} disabled={saving}>{saving ? "保存中…" : "保存设置"}</button></div></footer>
    </div>
  </div>
{/if}
