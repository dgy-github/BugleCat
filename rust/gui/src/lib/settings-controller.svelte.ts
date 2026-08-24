import { invoke } from "@tauri-apps/api/core";
import type { PluginController } from "./plugin-controller.svelte";

export type Settings = {
  model: string; base_url: string; vl_model: string; vl_base_url: string; sandbox_mode: string;
  approval_policy: string; reasoning_effort: string; max_iterations: number; max_tool_calls: number;
  context_edit_enabled: boolean; context_edit_max_chars: number; context_edit_keep_recent_messages: number;
  context_edit_max_tool_result_chars: number; price_in: number; price_out: number; price_currency: "CNY" | "USD";
  api_key_masked: string; has_api_key: boolean; vl_api_key_masked: string; has_vl_api_key: boolean;
  available_models: string[]; sandbox_modes: string[]; approval_policies: string[];
};
export type ConfigLocation = { config_path: string; config_dir: string };
export type CatalogModel = {
  provider_id: string; model_id: string; display_name: string; base_url: string; price_in: number; price_out: number;
  price_currency: "CNY" | "USD"; price_source: "official_direct" | "aggregator"; pricing_note: string | null;
  source_url: string; updated_at: string; context_length?: number | null; direct_available: boolean;
};
export type CatalogProvider = { id: string; name: string; models: CatalogModel[] };
export type ModelCatalogResponse = { providers: CatalogProvider[]; stale: boolean };

export class SettingsController {
  settings = $state<Settings | null>(null);
  configLocation = $state<ConfigLocation | null>(null);
  apiKeyInput = $state("");
  vlApiKeyInput = $state("");
  saving = $state(false);
  catalog = $state<ModelCatalogResponse | null>(null);
  catalogRefreshing = $state(false);
  presetSaving = $state("");

  constructor(
    private readonly plugins: PluginController,
    private readonly notify: (message: string) => void,
    private readonly modelApplied: (model: string, models: string[]) => void,
    private readonly priceApplied: (priceIn: number, priceOut: number, currency: "CNY" | "USD") => void,
  ) {}

  get officialProviders(): CatalogProvider[] {
    return this.catalog?.providers.filter((provider) => provider.id !== "openrouter") ?? [];
  }

  get openRouterProvider(): CatalogProvider | null {
    return this.catalog?.providers.find((provider) => provider.id === "openrouter") ?? null;
  }

  open = async (): Promise<void> => {
    try {
      const [settings, configLocation, catalog] = await Promise.all([
        invoke<Settings>("get_settings"), invoke<ConfigLocation>("get_config_location"),
        invoke<ModelCatalogResponse>("get_model_catalog"), this.plugins.load(),
      ]);
      this.settings = settings;
      this.configLocation = configLocation;
      this.catalog = catalog;
      this.apiKeyInput = "";
      this.vlApiKeyInput = "";
    } catch (error) { this.notify(`设置加载失败：${error}`); }
  };

  refreshOpenRouter = async (): Promise<void> => {
    this.catalogRefreshing = true;
    try { this.catalog = await invoke<ModelCatalogResponse>("refresh_openrouter_models"); }
    catch (error) { this.notify(`OpenRouter 模型目录刷新失败：${error}`); }
    finally { this.catalogRefreshing = false; }
  };

  applyPreset = async (provider: CatalogProvider, model: CatalogModel): Promise<void> => {
    if (!this.settings) return;
    this.presetSaving = `${provider.id}/${model.model_id}`;
    try {
      const selected = await invoke<CatalogModel>("apply_model_preset", { providerId: provider.id, modelId: model.model_id });
      this.settings.model = selected.model_id;
      this.settings.base_url = selected.base_url;
      this.settings.price_in = selected.price_in;
      this.settings.price_out = selected.price_out;
      this.settings.price_currency = selected.price_currency;
      this.settings.available_models = provider.models.map((item) => item.model_id);
      this.modelApplied(selected.model_id, this.settings.available_models);
      this.priceApplied(selected.price_in, selected.price_out, selected.price_currency);
    } catch (error) { this.notify(`应用模型预设失败：${error}`); }
    finally { this.presetSaving = ""; }
  };

  currentPriceSourceName = (): string => {
    if (!this.settings) return "";
    const current = this.catalog?.providers.flatMap((provider) => provider.models)
      .find((model) => model.model_id === this.settings?.model && model.base_url === this.settings?.base_url);
    return current ? (current.price_source === "official_direct" ? "厂商官方直连价" : "OpenRouter 聚合渠道价") : "手动设置的价格，程序无法验证其是否为厂商官方价";
  };

  openPriceSource = (url: string): void => {
    invoke("open_url", { url }).catch((error) => this.notify(`打开价格来源失败：${error}`));
  };

  openConfigFile = async (): Promise<void> => this.openConfig("open_config_file", "打开配置失败");
  openConfigDir = async (): Promise<void> => this.openConfig("open_config_dir", "打开配置文件夹失败");

  save = async (): Promise<void> => {
    if (!this.settings) return;
    this.saving = true;
    const settings = this.settings;
    const updates: Record<string, string> = {
      model: settings.model, base_url: settings.base_url, vl_model: settings.vl_model, vl_base_url: settings.vl_base_url,
      reasoning_effort: settings.reasoning_effort, max_iterations: String(settings.max_iterations), max_tool_calls: String(settings.max_tool_calls),
      context_edit_enabled: String(settings.context_edit_enabled), context_edit_max_chars: String(settings.context_edit_max_chars),
      context_edit_keep_recent_messages: String(settings.context_edit_keep_recent_messages),
      context_edit_max_tool_result_chars: String(settings.context_edit_max_tool_result_chars),
      price_in: String(settings.price_in), price_out: String(settings.price_out), price_currency: settings.price_currency,
    };
    if (this.apiKeyInput.trim()) updates.api_key = this.apiKeyInput.trim();
    if (this.vlApiKeyInput.trim()) updates.vl_api_key = this.vlApiKeyInput.trim();
    try {
      await invoke("save_settings", { updates });
      this.priceApplied(Number(settings.price_in), Number(settings.price_out), settings.price_currency);
      this.settings = null;
      this.apiKeyInput = "";
      this.vlApiKeyInput = "";
    } catch (error) { this.notify(`保存设置失败：${error}`); }
    finally { this.saving = false; }
  };

  private openConfig = async (command: string, errorLabel: string): Promise<void> => {
    try { await invoke(command); this.configLocation = await invoke<ConfigLocation>("get_config_location"); }
    catch (error) { this.notify(`${errorLabel}：${error}`); }
  };
}
