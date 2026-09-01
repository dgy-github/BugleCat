import { appServerRequest } from "./app-server-client";
import type { Settings } from "./settings-controller.svelte";

export type ReasoningEffortOption = { id: string; label: string; desc: string };
export type ProviderRouteOption = { id: string; name: string; protocol: "openai" | "anthropic"; models: string[]; active: boolean; kind: "custom" | "preset" };
type CatalogRoute = { id: string; name: string; models: { model_id: string }[] };
type PresetSelection = { model_id: string; price_in: number; price_out: number; price_currency: "CNY" | "USD" };
const option = (id: string, label: string, desc: string): ReasoningEffortOption => ({ id, label, desc });
const AUTO = option("auto", "模型自动", "由当前模型和供应商选择思考强度");

export function reasoningEffortsForModel(model: string): ReasoningEffortOption[] {
  const id = model.trim().toLowerCase();
  if (id.includes("deepseek")) return [
    AUTO, option("off", "关闭思考", "关闭 DeepSeek thinking"),
    option("high", "深度思考", "DeepSeek high"), option("max", "智能体增强", "DeepSeek max，适合复杂工具任务"),
  ];
  if (/^gpt-5\.6-(sol|terra)$/.test(id)) return [AUTO, ...[
    ["low", "低"], ["medium", "中"], ["high", "高"], ["xhigh", "超高"], ["max", "最大"], ["ultra", "极限"],
  ].map(([value, label]) => option(value, label, `OpenAI ${value} reasoning effort`))];
  if (/^gpt-5\.6-luna$/.test(id)) return [AUTO, ...[
    ["low", "低"], ["medium", "中"], ["high", "高"], ["xhigh", "超高"], ["max", "最大"],
  ].map(([value, label]) => option(value, label, `OpenAI ${value} reasoning effort`))];
  if (/^gpt-5\.(2|5)/.test(id) || /^gpt-5\.(3|4)/.test(id)) return [AUTO, ...[
    ["low", "低"], ["medium", "中"], ["high", "高"], ["xhigh", "超高"],
  ].map(([value, label]) => option(value, label, `OpenAI ${value} reasoning effort`))];
  return [AUTO, option("off", "关闭思考", "不发送 reasoning effort"), option("low", "低", "低思考强度"), option("medium", "中", "中等思考强度"), option("high", "高", "高思考强度")];
}

export const REASONING_EFFORTS = reasoningEffortsForModel("");
export const PERMISSION_MODES = [
  { id: "plan", label: "规划模式", desc: "只读，不改文件" },
  { id: "default", label: "默认", desc: "改文件前询问" },
  { id: "accept-edits", label: "自动接受编辑", desc: "编辑直接应用，命令询问" },
  { id: "bypass", label: "全权放行", desc: "危险：所有操作不询问" },
];

export class ModelControlsController {
  currentModel = $state("");
  currentProvider = $state("");
  currentProtocol = $state("");
  models = $state<string[]>([]);
  routes = $state<ProviderRouteOption[]>([]);
  modelMenuOpen = $state(false);
  reasoningEffort = $state("auto");
  reasoningMenuOpen = $state(false);
  permissionMode = $state("accept-edits");
  modeMenuOpen = $state(false);

  constructor(
    private readonly notify: (message: string) => void,
    private readonly priceApplied: (priceIn: number, priceOut: number, currency: "CNY" | "USD") => void,
    private readonly currentThreadId: () => string,
  ) {}

  applyModel = (model: string, models: string[]): void => {
    this.currentModel = model;
    this.models = models;
  };

  get reasoningEfforts(): ReasoningEffortOption[] { return reasoningEffortsForModel(this.currentModel); }
  get currentProviderName(): string { return this.routes.find((route) => route.id === this.currentProvider)?.name || this.currentProvider; }
  get routeLabel(): string {
    const protocol = this.currentProtocol === "anthropic" ? "Claude 协议" : this.currentProtocol === "openai" ? "OpenAI 协议" : this.currentProtocol;
    return [this.currentProviderName, protocol].filter(Boolean).join(" · ");
  }

  refreshRoutes = async (): Promise<void> => {
    try {
      const [configured, settings, catalog] = await Promise.all([
        appServerRequest<(Omit<ProviderRouteOption, "kind"> & { has_api_key: boolean; selected_model: string })[]>({ method: "customProviderList" }),
        appServerRequest<Settings>({ method: "settingsRead" }),
        appServerRequest<{ providers: CatalogRoute[] }>({ method: "modelCatalogRead" }),
      ]);
      const routes: ProviderRouteOption[] = configured
        .filter((route) => route.has_api_key && route.models.length > 0)
        .map((route) => ({ id: route.id, name: route.name, protocol: route.protocol, models: route.models, active: false, kind: route.id.startsWith("preset:") ? "preset" : "custom" }));
      const presetEnabled = (id: string): boolean => id === this.currentProvider || `preset:${id}` === this.currentProvider
        || (id === "deepseek" && settings.has_deepseek_api_key)
        || (id === "yunmo" && settings.has_yunmo_api_key);
      for (const provider of catalog.providers) {
        if (!presetEnabled(provider.id) || routes.some((route) => route.id === provider.id || route.id === `preset:${provider.id}`)) continue;
        const models = provider.models.map((model) => model.model_id).filter(Boolean);
        if (models.length) routes.push({ id: provider.id, name: provider.name, protocol: "openai", models, active: false, kind: "preset" });
      }
      if (!routes.some((route) => route.id === this.currentProvider) && this.currentProvider && this.models.length) {
        routes.push({ id: this.currentProvider, name: this.currentProvider, protocol: this.currentProtocol === "anthropic" ? "anthropic" : "openai", models: [...this.models], active: true, kind: "preset" });
      }
      const normalizedCurrent = this.currentProvider.replace(/^preset:/, "");
      const sameProvider = routes.filter((route) => route.id.replace(/^preset:/, "") === normalizedCurrent);
      const visible = sameProvider.find((route) => route.id === this.currentProvider)
        ?? sameProvider.find((route) => route.id === `preset:${normalizedCurrent}`)
        ?? sameProvider[0];
      // The Composer is a model picker for the active supplier. Provider
      // discovery and activation belong in Settings, so configured but
      // unselected suppliers must not flood this compact runtime menu.
      this.routes = visible ? [{ ...visible, active: true }] : [];
    } catch { /* Keep the current ready snapshot usable if the directory is unavailable. */ }
  };

  selectModel = async (model: string): Promise<void> => {
    this.modelMenuOpen = false;
    if (!model || model === this.currentModel) return;
    const previous = this.currentModel;
    this.currentModel = model;
    try {
      await appServerRequest({ method: "runtimeModelSet", params: { model } });
      try {
        const updated = await appServerRequest<Settings>({ method: "settingsRead" });
        this.models = updated.available_models;
        this.priceApplied(updated.price_in, updated.price_out, updated.price_currency);
      } catch { /* status refresh will synchronize pricing */ }
      this.notify(`已切换至 ${model}，当前会话下一轮将立即使用该模型。`);
    } catch (error) {
      this.currentModel = previous;
      this.notify(`切换模型失败：${error}`);
    }
  };

  selectRouteModel = async (route: ProviderRouteOption, model: string): Promise<void> => {
    this.modelMenuOpen = false;
    if (route.id === this.currentProvider) return this.selectModel(model);
    const previous = { provider: this.currentProvider, protocol: this.currentProtocol, model: this.currentModel, models: [...this.models], routes: [...this.routes] };
    try {
      if (route.kind === "preset") {
        const providerId = route.id.startsWith("preset:") ? route.id.slice("preset:".length) : route.id;
        const selected = await appServerRequest<PresetSelection>({ method: "modelPresetApply", params: { providerId, modelId: model } });
        this.priceApplied(selected.price_in, selected.price_out, selected.price_currency);
      } else {
        await appServerRequest({ method: "customProviderActivate", params: { id: route.id, model } });
        const updated = await appServerRequest<Settings>({ method: "settingsRead" });
        this.priceApplied(updated.price_in, updated.price_out, updated.price_currency);
      }
      this.currentProvider = route.id;
      this.currentProtocol = route.protocol;
      this.currentModel = model;
      this.models = [...route.models];
      await this.refreshRoutes();
      this.notify(`已切换至 ${route.name} / ${model}，当前会话下一轮立即使用新 Route。`);
    } catch (error) {
      this.currentProvider = previous.provider; this.currentProtocol = previous.protocol;
      this.currentModel = previous.model; this.models = previous.models; this.routes = previous.routes;
      this.notify(`切换 Provider 失败，当前 Route 未改变：${error}`);
    }
  };

  selectReasoningEffort = async (id: string): Promise<void> => {
    this.reasoningMenuOpen = false;
    if (!id || id === this.reasoningEffort) return;
    const previous = this.reasoningEffort;
    this.reasoningEffort = id;
    try {
      await appServerRequest({ method: "settingsUpdate", params: { updates: { reasoning_effort: id } } });
      this.notify(`思考程度已切换为 ${this.reasoningLabel(id)}；当前运行不变，下次会话生效。`);
    }
    catch (error) { this.reasoningEffort = previous; this.notify(`切换思考程度失败：${error}`); }
  };

  selectMode = async (id: string): Promise<void> => {
    this.modeMenuOpen = false;
    if (id === this.permissionMode) return;
    const threadId = this.currentThreadId();
    if (!threadId) {
      this.notify("当前没有可重建的会话，无法切换权限模式。");
      return;
    }
    const previous = this.permissionMode;
    this.permissionMode = id;
    try {
      await appServerRequest({ method: "runtimePermissionModeSet", params: { threadId, mode: id } });
    }
    catch (error) {
      // Do not roll a newly selected session back to the mode that belonged to
      // the request's old Thread. The backend rejects that stale rebuild.
      if (this.currentThreadId() === threadId) this.permissionMode = previous;
      this.notify(`切换权限模式失败：${error}`);
    }
  };

  reasoningLabel = (id: string): string => this.reasoningEfforts.find((option) => option.id === id)?.label ?? id;
  modeLabel = (id: string): string => PERMISSION_MODES.find((option) => option.id === id)?.label ?? id;
  modeIcon = (id: string): string => id === "plan" ? "📋" : id === "bypass" ? "⚠️" : "🛡";
}
