import { open } from "@tauri-apps/plugin-dialog";
import { appServerRequest } from "./app-server-client";

export type ProviderRouteDiagnostics = { active_provider_id: string; protocol: string; base_url: string; model: string; has_api_key: boolean; route_count: number };
export type ProviderActivationDiagnostics = { generation: number; status: "idle" | "validating" | "active" | "failed"; last_error?: string | null; updated_at_ms: number };
export type HarnessDiagnostics = Record<"llm" | "provider_directory" | "provider_catalog" | "provider_chat_probe" | "interaction" | "policy" | "context" | "memory" | "compaction" | "mcp" | "attachment" | "media" | "cost_telemetry" | "alibaba_attachment_parser" | "image_generation_ready" | "video_generation_ready" | "external_tools_ready", boolean> & { provider_route: ProviderRouteDiagnostics; provider_activation: ProviderActivationDiagnostics };
export type ExternalPlugin = { manifest: { id: string; name: string; version: string; capabilities: string[] }; root: string; enabled: boolean };
export type CodexPlugin = {
  manifest: { name: string; version?: string; description?: string; keywords: string[] };
  root: string; enabled: boolean; skill_roots: number; has_mcp: boolean; has_apps: boolean;
  app_count: number; has_hooks: boolean; ui_slots: DshUiSlotContribution[]; ui_slot_error?: string;
};
export type DshUiSlotContribution = { plugin: string; slot: "settings.plugins.tab" | "sidebar.footer.action" | "shell.overlay"; id: string; label: string; order: number; description: string; url?: string };
export type MarketplaceSource =
  | { source: "local"; path: string }
  | { source: "git"; url: string; path?: string; ref?: string }
  | { source: "npm"; package: string; version?: string };
export type PluginMarketplace = { path: string; marketplace: { name: string; plugins: { name: string; source: MarketplaceSource }[] } };
export type DshCategory = { id: string; en: string; zh: string; count: number };
export type DshMarketItem = {
  source: string; id: string; name: string; summary: string; repository?: string;
  package?: string; version?: string; compatibility: "review" | "incompatible";
  category: string; compatibilityReason: string;
};
export type DshMarketPreview = {
  package: string; version: string; compatibility: "convertible" | "ui-adapter" | "incompatible";
  compatible: boolean; reason: string; risks: string[];
};

export class PluginController {
  diagnostics = $state<HarnessDiagnostics | null>(null);
  externalPlugins = $state<ExternalPlugin[]>([]);
  codexPlugins = $state<CodexPlugin[]>([]);
  marketplaces = $state<PluginMarketplace[]>([]);
  dshSource = $state("dshfind");
  dshManifestUrl = $state("");
  dshQuery = $state("");
  dshCategory = $state("all");
  dshCategories = $state<DshCategory[]>([]);
  dshItems = $state<DshMarketItem[]>([]);
  dshPreview = $state<DshMarketPreview | null>(null);
  dshSelected = $state<DshMarketItem | null>(null);
  dshLoading = $state(false);
  dshError = $state("");

  constructor(private readonly notify: (message: string) => void) {}

  load = async (): Promise<void> => {
    const [diagnostics, externalPlugins, codexPlugins, marketplaces] = await Promise.all([
      appServerRequest<HarnessDiagnostics>({ method: "harnessDiagnosticsRead" }),
      appServerRequest<ExternalPlugin[]>({ method: "externalPluginList" }),
      appServerRequest<CodexPlugin[]>({ method: "codexPluginList" }),
      appServerRequest<PluginMarketplace[]>({ method: "marketplaceList" }),
    ]);
    this.diagnostics = diagnostics;
    this.externalPlugins = externalPlugins;
    this.codexPlugins = codexPlugins;
    this.marketplaces = marketplaces;
  };

  addExternal = async (): Promise<void> => this.installExternal(false);
  upgradeExternal = async (): Promise<void> => this.installExternal(true);

  toggleExternal = async (plugin: ExternalPlugin): Promise<void> => {
    try {
      await appServerRequest({ method: "externalPluginSetEnabled", params: { id: plugin.manifest.id, enabled: !plugin.enabled } });
      this.externalPlugins = await appServerRequest<ExternalPlugin[]>({ method: "externalPluginList" });
    } catch (error) { this.notify(`插件状态修改失败：${error}`); }
  };

  addCodex = async (): Promise<void> => this.installCodex(false);
  upgradeCodex = async (): Promise<void> => this.installCodex(true);

  toggleCodex = async (plugin: CodexPlugin): Promise<void> => {
    try {
      await appServerRequest({ method: "codexPluginSetEnabled", params: { name: plugin.manifest.name, enabled: !plugin.enabled } });
      await this.refreshCodex();
    } catch (error) { this.notify(`Codex 插件状态修改失败：${error}`); }
  };

  removeCodex = async (plugin: CodexPlugin): Promise<void> => {
    try {
      await appServerRequest({ method: "codexPluginUninstall", params: { name: plugin.manifest.name } });
      await this.refreshCodex();
    } catch (error) { this.notify(`Codex 插件卸载失败：${error}`); }
  };

  installMarketplace = async (marketplacePath: string, pluginName: string, upgrade = false): Promise<void> => {
    try {
      await appServerRequest({ method: "marketplacePluginInstall", params: { marketplacePath, pluginName, upgrade } });
      await this.refreshCodex();
    } catch (error) { this.notify(`Marketplace 插件${upgrade ? "升级" : "安装"}失败：${error}`); }
  };

  searchDsh = async (): Promise<void> => {
    if (this.dshLoading) return;
    this.dshLoading = true; this.dshError = ""; this.dshPreview = null; this.dshSelected = null;
    try {
      const result = await appServerRequest<{ items: DshMarketItem[]; categories: DshCategory[] }>({
        method: "dshMarketplaceSearch",
        params: { source: this.dshSource, manifestUrl: this.dshManifestUrl.trim() || null, query: this.dshQuery.trim() },
      });
      this.dshItems = result.items;
      this.dshCategories = result.categories;
      if (this.dshCategory !== "all" && !result.categories.some((category) => category.id === this.dshCategory)) this.dshCategory = "all";
    } catch (error) { this.dshError = String(error).replace(/^Error:\s*/, ""); this.dshItems = []; this.dshCategories = []; this.dshCategory = "all"; }
    finally { this.dshLoading = false; }
  };

  previewDsh = async (item: DshMarketItem): Promise<void> => {
    if (this.dshLoading || item.compatibility === "incompatible") return;
    this.dshLoading = true; this.dshError = ""; this.dshSelected = item; this.dshPreview = null;
    try {
      this.dshPreview = await appServerRequest<DshMarketPreview>({ method: "dshMarketplacePreview", params: { item } });
    } catch (error) { this.dshError = String(error).replace(/^Error:\s*/, ""); }
    finally { this.dshLoading = false; }
  };

  installDsh = async (upgrade = false): Promise<void> => {
    if (this.dshLoading || !this.dshSelected || !this.dshPreview?.compatible) return;
    this.dshLoading = true; this.dshError = "";
    try {
      await appServerRequest({ method: "dshMarketplaceInstall", params: { item: this.dshSelected, upgrade } });
      await this.refreshCodex();
      this.notify(`DSH 插件已${upgrade ? "升级" : "安装"}为 nanocodex 资源插件：${this.dshSelected.name}`);
    } catch (error) { this.dshError = String(error).replace(/^Error:\s*/, ""); }
    finally { this.dshLoading = false; }
  };

  private installExternal = async (upgrade: boolean): Promise<void> => {
    const selected = await open({ directory: true, multiple: false, title: upgrade ? "选择更高版本的插件目录" : "选择包含 plugin.toml 的插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await appServerRequest({ method: "externalPluginInstall", params: { source: selected, upgrade } });
      this.externalPlugins = await appServerRequest<ExternalPlugin[]>({ method: "externalPluginList" });
    } catch (error) { this.notify(`插件${upgrade ? "升级" : "安装"}失败：${error}`); }
  };

  private installCodex = async (upgrade: boolean): Promise<void> => {
    const selected = await open({ directory: true, multiple: false, title: upgrade ? "选择新版 Codex 插件目录" : "选择包含 .codex-plugin/plugin.json 的插件目录" });
    if (!selected || Array.isArray(selected)) return;
    try {
      await appServerRequest({ method: "codexPluginInstall", params: { source: selected, upgrade } });
      await this.refreshCodex();
    } catch (error) { this.notify(`Codex 插件${upgrade ? "升级" : "安装"}失败：${error}`); }
  };

  private refreshCodex = async (): Promise<void> => {
    this.codexPlugins = await appServerRequest<CodexPlugin[]>({ method: "codexPluginList" });
  };
}
