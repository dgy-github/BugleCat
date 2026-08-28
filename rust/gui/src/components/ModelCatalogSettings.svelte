<script lang="ts">
  import { onMount } from "svelte";
  import { appServerRequest } from "../lib/app-server-client";
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
  type ProviderRoute = { id: string; api_key_masked: string; has_api_key: boolean };

  let {
    model = $bindable(),
    baseUrl,
    modelCatalog,
    officialProviders,
    yunmoProvider,
    openRouterProvider,
    catalogRefreshing,
    yunmoRefreshing,
    presetSaving,
    currencySymbol,
    currencyName,
    priceSourceName,
    applyModelPreset,
    openPriceSource,
    refreshOpenRouterModels,
    refreshYunmoModels,
  }: {
    model: string;
    baseUrl: string;
    modelCatalog: { providers: CatalogProvider[] } | null;
    officialProviders: CatalogProvider[];
    yunmoProvider: CatalogProvider | null;
    openRouterProvider: CatalogProvider | null;
    catalogRefreshing: boolean;
    yunmoRefreshing: boolean;
    presetSaving: string;
    currencySymbol: (currency: "CNY" | "USD") => string;
    currencyName: (currency: "CNY" | "USD") => string;
    priceSourceName: (source: "official_direct" | "aggregator") => string;
    applyModelPreset: (provider: CatalogProvider, model: CatalogModel) => void;
    openPriceSource: (url: string) => void;
    refreshOpenRouterModels: () => void;
    refreshYunmoModels: () => void;
  } = $props();

  const normalizeBaseUrl = (value: string): string => value.trim().replace(/\/+$/, "");

  const isSelectedModel = (candidate: CatalogModel): boolean =>
    model === candidate.model_id && normalizeBaseUrl(baseUrl) === normalizeBaseUrl(candidate.base_url);

  const selectedFirst = (models: CatalogModel[]): CatalogModel[] =>
    models
      .map((candidate, index) => ({ candidate, index }))
      .sort((left, right) => Number(isSelectedModel(right.candidate)) - Number(isSelectedModel(left.candidate)) || left.index - right.index)
      .map(({ candidate }) => candidate);

  let presetRoutes = $state<ProviderRoute[]>([]);
  let credentialInputs = $state<Record<string, string>>({});
  let credentialBusy = $state("");
  let credentialMessage = $state("");
  const routeFor = (providerId: string) => presetRoutes.find((route) => route.id === `preset:${providerId}`);
  const loadPresetRoutes = async () => {
    const routes = await appServerRequest<ProviderRoute[]>({ method: "customProviderList" });
    presetRoutes = routes.filter((route) => route.id.startsWith("preset:"));
  };
  onMount(() => { loadPresetRoutes().catch(() => {}); });
  const saveCredential = async (provider: CatalogProvider) => {
    const token = credentialInputs[provider.id]?.trim() ?? "";
    if (!token) { credentialMessage = `请输入 ${provider.name} Token`; return; }
    const first = provider.models[0];
    if (!first) { credentialMessage = `${provider.name} 暂无可配置模型`; return; }
    credentialBusy = provider.id; credentialMessage = "";
    try {
      await appServerRequest({ method: "customProviderSave", params: {
        id: `preset:${provider.id}`, name: provider.name, protocol: "openai",
        baseUrl: first.base_url, apiKey: token, models: provider.models.map((item) => item.model_id),
      } });
      credentialInputs[provider.id] = "";
      await loadPresetRoutes();
      credentialMessage = `${provider.name} Token 已独立保存，不会与其他模型商共用`;
    } catch (error) { credentialMessage = String(error).replace(/^Error:\s*/, ""); }
    finally { credentialBusy = ""; }
  };
</script>

<section class="model-catalog" aria-label="模型厂商目录">
  <div class="catalog-head">
    <div>
      <strong>厂商官方直连目录</strong>
      <p>这里的单价来自各厂商官网，选择后会填写该厂商接口、模型、费用和币种；显示的是当前公开输入/输出价，缓存、长上下文阶梯和限时价格会单独说明；API 密钥仍需自行配置。</p>
    </div>
  </div>
  {#if modelCatalog}
    {#each officialProviders as provider}
      <div class="catalog-provider">
        <h4>{provider.name}</h4>
        <div class="catalog-credential">
          <span>{routeFor(provider.id)?.has_api_key ? `独立 Token ${routeFor(provider.id)?.api_key_masked}` : "未配置独立 Token"}</span>
          <input type="password" bind:value={credentialInputs[provider.id]} placeholder={routeFor(provider.id)?.has_api_key ? "留空保持当前 Token" : `输入 ${provider.name} Token`} autocomplete="off" />
          <button onclick={() => saveCredential(provider)} disabled={credentialBusy === provider.id}>{credentialBusy === provider.id ? "保存中…" : "保存 Token"}</button>
        </div>
        <div class="catalog-models">
          {#each selectedFirst(provider.models) as catalogModel (catalogModel.model_id)}
            <article class:active={isSelectedModel(catalogModel)} class="catalog-model">
              <div class="catalog-model-name">
                <strong>{catalogModel.display_name}</strong>
                <code>{catalogModel.model_id}</code>
              </div>
              <p>{currencySymbol(catalogModel.price_currency)}{catalogModel.price_in} 输入 / {currencySymbol(catalogModel.price_currency)}{catalogModel.price_out} 输出（每百万 Token，{currencyName(catalogModel.price_currency)}）</p>
              <span class="catalog-price-source">{priceSourceName(catalogModel.price_source)}</span>
              <small class="catalog-audit-note">已按官网核验：{catalogModel.updated_at}</small>
              {#if catalogModel.pricing_note}
                <small class="catalog-pricing-note">{catalogModel.pricing_note}</small>
              {/if}
              <div class="catalog-model-actions">
                <button
                  class="catalog-select"
                  onclick={() => applyModelPreset(provider, catalogModel)}
                  disabled={presetSaving === `${provider.id}/${catalogModel.model_id}`}
                >
                  {presetSaving === `${provider.id}/${catalogModel.model_id}` ? "应用中…" : "使用官方直连"}
                </button>
                <button class="catalog-source" onclick={() => openPriceSource(catalogModel.source_url)}>官方价格来源</button>
              </div>
            </article>
          {/each}
        </div>
      </div>
    {/each}
    {#if credentialMessage}<p class="catalog-credential-message">{credentialMessage}</p>{/if}
    {#if yunmoProvider}
      <div class="catalog-aggregator">
        <div class="catalog-head"><div><strong>云末 AI 中转站</strong><p>模型列表由带鉴权的 /v1/models 实时获取；切换时保持同一套 Agent Harness。</p></div><button class="catalog-refresh" onclick={refreshYunmoModels} disabled={yunmoRefreshing}>{yunmoRefreshing ? "刷新中…" : "刷新云末模型"}</button></div>
        <div class="catalog-credential">
          <span>{routeFor(yunmoProvider.id)?.has_api_key ? `独立 Token ${routeFor(yunmoProvider.id)?.api_key_masked}` : "也可在连接设置保存云末 Token"}</span>
          <input type="password" bind:value={credentialInputs[yunmoProvider.id]} placeholder="输入云末独立 Token" autocomplete="off" />
          <button onclick={() => saveCredential(yunmoProvider)} disabled={credentialBusy === yunmoProvider.id}>{credentialBusy === yunmoProvider.id ? "保存中…" : "保存 Token"}</button>
        </div>
        <div class="catalog-models">
          {#each selectedFirst(yunmoProvider.models) as relayModel (relayModel.model_id)}
            <article class:active={isSelectedModel(relayModel)} class:aggregator={true} class="catalog-model">
              <div class="catalog-model-name"><strong>{relayModel.display_name}</strong><code>{relayModel.model_id}</code></div>
              <p>实际费用以中转站控制台为准</p>
              <span class="catalog-price-source aggregator">中转渠道，非 OpenAI 官方直连</span>
              {#if relayModel.pricing_note}<small class="catalog-pricing-note">{relayModel.pricing_note}</small>{/if}
              <div class="catalog-model-actions"><button class="catalog-select" onclick={() => applyModelPreset(yunmoProvider, relayModel)} disabled={presetSaving === `${yunmoProvider.id}/${relayModel.model_id}`}>{presetSaving === `${yunmoProvider.id}/${relayModel.model_id}` ? "切换中…" : "切换到云末 GPT"}</button><button class="catalog-source" onclick={() => openPriceSource(relayModel.source_url)}>中转站</button></div>
            </article>
          {/each}
        </div>
      </div>
    {/if}
    <div class="catalog-aggregator">
      <div class="catalog-head">
        <div>
          <strong>OpenRouter 聚合平台（可选）</strong>
          <p>按需加载全量模型。这里显示的是经 OpenRouter 调用时的渠道价格，不是原厂官方直连价。</p>
        </div>
        <button class="catalog-refresh" onclick={refreshOpenRouterModels} disabled={catalogRefreshing}>
          {catalogRefreshing ? "加载中…" : "加载 OpenRouter 聚合模型"}
        </button>
      </div>
      {#if openRouterProvider}
        <div class="catalog-credential">
          <span>{routeFor(openRouterProvider.id)?.has_api_key ? `独立 Token ${routeFor(openRouterProvider.id)?.api_key_masked}` : "未配置 OpenRouter 独立 Token"}</span>
          <input type="password" bind:value={credentialInputs[openRouterProvider.id]} placeholder="输入 OpenRouter Token" autocomplete="off" />
          <button onclick={() => saveCredential(openRouterProvider)} disabled={credentialBusy === openRouterProvider.id}>{credentialBusy === openRouterProvider.id ? "保存中…" : "保存 Token"}</button>
        </div>
        <div class="catalog-models">
          {#each selectedFirst(openRouterProvider.models) as catalogModel (catalogModel.model_id)}
            <article class:active={isSelectedModel(catalogModel)} class:aggregator={true} class="catalog-model">
              <div class="catalog-model-name">
                <strong>{catalogModel.display_name}</strong>
                <code>{catalogModel.model_id}</code>
              </div>
              <p>{currencySymbol(catalogModel.price_currency)}{catalogModel.price_in} 输入 / {currencySymbol(catalogModel.price_currency)}{catalogModel.price_out} 输出（每百万 Token，{currencyName(catalogModel.price_currency)}）</p>
              <span class="catalog-price-source aggregator">{priceSourceName(catalogModel.price_source)}，非厂商官方报价</span>
              <div class="catalog-model-actions">
                <button
                  class="catalog-select"
                  onclick={() => applyModelPreset(openRouterProvider, catalogModel)}
                  disabled={presetSaving === `${openRouterProvider.id}/${catalogModel.model_id}`}
                >
                  {presetSaving === `${openRouterProvider.id}/${catalogModel.model_id}` ? "应用中…" : "使用 OpenRouter 渠道"}
                </button>
                <button class="catalog-source" onclick={() => openPriceSource(catalogModel.source_url)}>渠道价格来源</button>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <p class="catalog-empty">正在读取模型厂商目录…</p>
  {/if}
</section>
