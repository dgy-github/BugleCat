<script lang="ts">
  import PluginSettings from "./PluginSettings.svelte";
  import ModelCatalogSettings from "./ModelCatalogSettings.svelte";
  type Settings = {
    model: string;
    base_url: string;
    vl_model: string;
    vl_base_url: string;
    reasoning_effort: string;
    max_iterations: number;
    max_tool_calls: number;
    context_edit_enabled: boolean;
    context_edit_max_chars: number;
    context_edit_keep_recent_messages: number;
    context_edit_max_tool_result_chars: number;
    price_in: number;
    price_out: number;
    price_currency: "CNY" | "USD";
    api_key_masked: string;
    has_api_key: boolean;
    vl_api_key_masked: string;
    has_vl_api_key: boolean;
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
  type HarnessDiagnostics = Record<string, boolean>;

  let {
    settings = $bindable(),
    apiKeyInput = $bindable(),
    vlApiKeyInput = $bindable(),
    configLocation,
    modelCatalog,
    officialProviders,
    openRouterProvider,
    catalogRefreshing,
    presetSaving,
    saving,
    harnessDiagnostics,
    externalPlugins,
    codexPlugins,
    pluginMarketplaces,
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
    addExternalPlugin,
    upgradeExternalPlugin,
    toggleExternalPlugin,
    addCodexPlugin,
    upgradeCodexPlugin,
    toggleCodexPlugin,
    removeCodexPlugin,
    installMarketplacePlugin,
    saveSettings,
  }: {
    settings: Settings | null;
    apiKeyInput: string;
    vlApiKeyInput: string;
    configLocation: { config_path: string; config_dir: string } | null;
    modelCatalog: { providers: CatalogProvider[] } | null;
    officialProviders: CatalogProvider[];
    openRouterProvider: CatalogProvider | null;
    catalogRefreshing: boolean;
    presetSaving: string;
    saving: boolean;
    harnessDiagnostics: HarnessDiagnostics | null;
    externalPlugins: ExternalPlugin[];
    codexPlugins: CodexPlugin[];
    pluginMarketplaces: PluginMarketplace[];
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
    addExternalPlugin: () => void;
    upgradeExternalPlugin: () => void;
    toggleExternalPlugin: (plugin: ExternalPlugin) => void;
    addCodexPlugin: () => void;
    upgradeCodexPlugin: () => void;
    toggleCodexPlugin: (plugin: CodexPlugin) => void;
    removeCodexPlugin: (plugin: CodexPlugin) => void;
    installMarketplacePlugin: (path: string, name: string, upgrade?: boolean) => void;
    saveSettings: () => void;
  } = $props();
</script>

{#if settings}
  <div class="overlay">
    <div class="modal">
      <h3>设置</h3>
      {#if configLocation}
        <div class="config-entry">
          <span>配置</span>
          <code title={configLocation.config_path}>{configLocation.config_path}</code>
          <button class="plain" onclick={openConfigFile}>打开文件</button>
          <button class="plain" onclick={openConfigDir}>打开文件夹</button>
        </div>
      {/if}
      <label>
        <span>模型</span>
        <select bind:value={settings.model}>
          {#each settings.available_models as m}<option value={m}>{m}</option>{/each}
        </select>
      </label>
      <ModelCatalogSettings
        bind:model={settings.model}
        {modelCatalog}
        {officialProviders}
        {openRouterProvider}
        {catalogRefreshing}
        {presetSaving}
        {currencySymbol}
        {currencyName}
        {priceSourceName}
        {applyModelPreset}
        {openPriceSource}
        {refreshOpenRouterModels}
      />
      <p class="settings-note">权限（沙箱 / 审批）由顶部输入框旁的「权限模式」控制：规划 / 默认 / 自动接受编辑 / 全权放行。</p>
      <label>
        <span>思考程度</span>
        <select bind:value={settings.reasoning_effort}>
          {#each REASONING_EFFORTS as option}
            <option value={option.id}>{option.label}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>模型调用上限</span>
        <input type="number" min="1" bind:value={settings.max_iterations} />
      </label>
      <label>
        <span>工具调用上限</span>
        <input type="number" min="0" bind:value={settings.max_tool_calls} />
      </label>
      <label class="check">
        <span>上下文裁剪</span>
        <input type="checkbox" bind:checked={settings.context_edit_enabled} />
      </label>
      <label>
        <span>上下文字符上限</span>
        <input type="number" min="1" bind:value={settings.context_edit_max_chars} />
      </label>
      <label>
        <span>保留最近消息数</span>
        <input type="number" min="1" bind:value={settings.context_edit_keep_recent_messages} />
      </label>
      <label>
        <span>工具结果字符上限</span>
        <input type="number" min="1" bind:value={settings.context_edit_max_tool_result_chars} />
      </label>
      <label>
        <span>输入单价 {currencySymbol(settings.price_currency)}/百万</span>
        <input type="number" min="0" step="0.01" bind:value={settings.price_in} placeholder="0 = 不计费" />
      </label>
      <label>
        <span>输出单价 {currencySymbol(settings.price_currency)}/百万</span>
        <input type="number" min="0" step="0.01" bind:value={settings.price_out} placeholder="0 = 不计费" />
      </label>
      <label>
        <span>价格币种</span>
        <select bind:value={settings.price_currency}>
          <option value="CNY">人民币（CNY）</option>
          <option value="USD">美元（USD）</option>
        </select>
      </label>
      <p class="settings-note price-note">当前费用来源：{currentPriceSourceName()}</p>
      <label>
        <span>Base URL</span>
        <input bind:value={settings.base_url} />
      </label>
      <label>
        <span>API 密钥</span>
        <input
          type="password"
          bind:value={apiKeyInput}
          placeholder={settings.has_api_key ? `保持当前（${settings.api_key_masked}）` : "设置 API 密钥"}
        />
      </label>
      <p class="settings-note">
        图片附件会发送到下面的视觉解析模型；阿里百炼生图和 Wan 视频也复用这里保存的第二套密钥。视觉接口留空可沿用主模型，媒体生成不会把主模型密钥发送给阿里云。
      </p>
      <label>
        <span>图片/文件解析模型</span>
        <input bind:value={settings.vl_model} placeholder="例如：qwen3.7-plus" />
      </label>
      <label>
        <span>图片/文件解析接口</span>
        <input bind:value={settings.vl_base_url} placeholder="留空则沿用主模型接口" />
      </label>
      <label>
        <span>视觉解析 / 阿里百炼媒体密钥</span>
        <input
          type="password"
          bind:value={vlApiKeyInput}
          placeholder={settings.has_vl_api_key ? `保持当前（${settings.vl_api_key_masked}）` : "媒体生成需要单独配置 DASHSCOPE Key"}
        />
      </label>
      <PluginSettings
        {harnessDiagnostics}
        {externalPlugins}
        {codexPlugins}
        {pluginMarketplaces}
        {addExternalPlugin}
        {upgradeExternalPlugin}
        {toggleExternalPlugin}
        {addCodexPlugin}
        {upgradeCodexPlugin}
        {toggleCodexPlugin}
        {removeCodexPlugin}
        {installMarketplacePlugin}
      />
      <div class="abtns">
        <button class="deny" onclick={() => (settings = null)}>取消</button>
        <button class="ok" onclick={saveSettings} disabled={saving}>
          {saving ? "保存中…" : "保存"}
        </button>
      </div>
    </div>
  </div>
{/if}
