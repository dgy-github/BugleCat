<script lang="ts">
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

  let {
    model = $bindable(),
    modelCatalog,
    officialProviders,
    openRouterProvider,
    catalogRefreshing,
    presetSaving,
    currencySymbol,
    currencyName,
    priceSourceName,
    applyModelPreset,
    openPriceSource,
    refreshOpenRouterModels,
  }: {
    model: string;
    modelCatalog: { providers: CatalogProvider[] } | null;
    officialProviders: CatalogProvider[];
    openRouterProvider: CatalogProvider | null;
    catalogRefreshing: boolean;
    presetSaving: string;
    currencySymbol: (currency: "CNY" | "USD") => string;
    currencyName: (currency: "CNY" | "USD") => string;
    priceSourceName: (source: "official_direct" | "aggregator") => string;
    applyModelPreset: (provider: CatalogProvider, model: CatalogModel) => void;
    openPriceSource: (url: string) => void;
    refreshOpenRouterModels: () => void;
  } = $props();
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
        <div class="catalog-models">
          {#each provider.models as model}
            <article class:active={model === model.model_id} class="catalog-model">
              <div class="catalog-model-name">
                <strong>{model.display_name}</strong>
                <code>{model.model_id}</code>
              </div>
              <p>{currencySymbol(model.price_currency)}{model.price_in} 输入 / {currencySymbol(model.price_currency)}{model.price_out} 输出（每百万 Token，{currencyName(model.price_currency)}）</p>
              <span class="catalog-price-source">{priceSourceName(model.price_source)}</span>
              <small class="catalog-audit-note">已按官网核验：{model.updated_at}</small>
              {#if model.pricing_note}
                <small class="catalog-pricing-note">{model.pricing_note}</small>
              {/if}
              <div class="catalog-model-actions">
                <button
                  class="catalog-select"
                  onclick={() => applyModelPreset(provider, model)}
                  disabled={presetSaving === `${provider.id}/${model.model_id}`}
                >
                  {presetSaving === `${provider.id}/${model.model_id}` ? "应用中…" : "使用官方直连"}
                </button>
                <button class="catalog-source" onclick={() => openPriceSource(model.source_url)}>官方价格来源</button>
              </div>
            </article>
          {/each}
        </div>
      </div>
    {/each}
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
        <div class="catalog-models">
          {#each openRouterProvider.models as model}
            <article class:active={model === model.model_id} class:aggregator={true} class="catalog-model">
              <div class="catalog-model-name">
                <strong>{model.display_name}</strong>
                <code>{model.model_id}</code>
              </div>
              <p>{currencySymbol(model.price_currency)}{model.price_in} 输入 / {currencySymbol(model.price_currency)}{model.price_out} 输出（每百万 Token，{currencyName(model.price_currency)}）</p>
              <span class="catalog-price-source aggregator">{priceSourceName(model.price_source)}，非厂商官方报价</span>
              <div class="catalog-model-actions">
                <button
                  class="catalog-select"
                  onclick={() => applyModelPreset(openRouterProvider, model)}
                  disabled={presetSaving === `${openRouterProvider.id}/${model.model_id}`}
                >
                  {presetSaving === `${openRouterProvider.id}/${model.model_id}` ? "应用中…" : "使用 OpenRouter 渠道"}
                </button>
                <button class="catalog-source" onclick={() => openPriceSource(model.source_url)}>渠道价格来源</button>
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
