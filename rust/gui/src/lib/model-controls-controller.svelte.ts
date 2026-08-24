import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "./settings-controller.svelte";

export const REASONING_EFFORTS = [
  { id: "auto", label: "智能体自动", desc: "普通请求用高强度，复杂智能体任务自动增强" },
  { id: "off", label: "关闭思考", desc: "直接回答，不启用思考模式" },
  { id: "high", label: "深度思考", desc: "启用 DeepSeek high 思考强度" },
  { id: "max", label: "智能体增强", desc: "启用 DeepSeek max，适合复杂工具任务" },
];
export const PERMISSION_MODES = [
  { id: "plan", label: "规划模式", desc: "只读，不改文件" },
  { id: "default", label: "默认", desc: "改文件前询问" },
  { id: "accept-edits", label: "自动接受编辑", desc: "编辑直接应用，命令询问" },
  { id: "bypass", label: "全权放行", desc: "危险：所有操作不询问" },
];

export class ModelControlsController {
  currentModel = $state("");
  models = $state<string[]>([]);
  modelMenuOpen = $state(false);
  reasoningEffort = $state("auto");
  reasoningMenuOpen = $state(false);
  permissionMode = $state("accept-edits");
  modeMenuOpen = $state(false);

  constructor(
    private readonly notify: (message: string) => void,
    private readonly priceApplied: (priceIn: number, priceOut: number, currency: "CNY" | "USD") => void,
  ) {}

  applyModel = (model: string, models: string[]): void => {
    this.currentModel = model;
    this.models = models;
  };

  selectModel = async (model: string): Promise<void> => {
    this.modelMenuOpen = false;
    if (!model || model === this.currentModel) return;
    const previous = this.currentModel;
    this.currentModel = model;
    try {
      await invoke("set_model", { model });
      try {
        const updated = await invoke<Settings>("get_settings");
        this.models = updated.available_models;
        this.priceApplied(updated.price_in, updated.price_out, updated.price_currency);
      } catch { /* status refresh will synchronize pricing */ }
    } catch (error) {
      this.currentModel = previous;
      this.notify(`切换模型失败：${error}`);
    }
  };

  selectReasoningEffort = async (id: string): Promise<void> => {
    this.reasoningMenuOpen = false;
    if (!id || id === this.reasoningEffort) return;
    const previous = this.reasoningEffort;
    this.reasoningEffort = id;
    try { await invoke("save_settings", { updates: { reasoning_effort: id } }); }
    catch (error) { this.reasoningEffort = previous; this.notify(`切换思考程度失败：${error}`); }
  };

  selectMode = async (id: string): Promise<void> => {
    this.modeMenuOpen = false;
    if (id === this.permissionMode) return;
    const previous = this.permissionMode;
    this.permissionMode = id;
    try { await invoke("set_permission_mode", { mode: id }); }
    catch (error) { this.permissionMode = previous; this.notify(`切换权限模式失败：${error}`); }
  };

  reasoningLabel = (id: string): string => REASONING_EFFORTS.find((option) => option.id === id)?.label ?? id;
  modeLabel = (id: string): string => PERMISSION_MODES.find((option) => option.id === id)?.label ?? id;
  modeIcon = (id: string): string => id === "plan" ? "📋" : id === "bypass" ? "⚠️" : "🛡";
}
