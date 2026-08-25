import { open } from "@tauri-apps/plugin-dialog";
import { appServerRequest } from "./app-server-client";

export type HarnessDiagnostics = Record<"llm" | "interaction" | "policy" | "context" | "memory" | "compaction" | "mcp" | "attachment" | "media" | "cost_telemetry" | "image_generation_ready" | "video_generation_ready" | "external_tools_ready", boolean>;
export type ExternalPlugin = { manifest: { id: string; name: string; version: string; capabilities: string[] }; root: string; enabled: boolean };
export type CodexPlugin = {
  manifest: { name: string; version?: string; description?: string; keywords: string[] };
  root: string; enabled: boolean; skill_roots: number; has_mcp: boolean; has_apps: boolean;
  app_count: number; has_hooks: boolean;
};
export type MarketplaceSource =
  | { source: "local"; path: string }
  | { source: "git"; url: string; path?: string; ref?: string }
  | { source: "npm"; package: string; version?: string };
export type PluginMarketplace = { path: string; marketplace: { name: string; plugins: { name: string; source: MarketplaceSource }[] } };

export class PluginController {
  diagnostics = $state<HarnessDiagnostics | null>(null);
  externalPlugins = $state<ExternalPlugin[]>([]);
  codexPlugins = $state<CodexPlugin[]>([]);
  marketplaces = $state<PluginMarketplace[]>([]);

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
