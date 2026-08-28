import { invoke } from "@tauri-apps/api/core";
import { appServerRequest } from "./app-server-client";

export type SlashCommand = { id: string; label: string; desc: string; run: () => void };
type CustomCommand = { scope: string; name: string; slash: string; path: string };
type SlashActions = {
  newSession: () => void; forkCurrent: () => void; openModel: () => void; openSettings: () => void;
  showUsage: () => void; openCheckpoints: () => void; openFiles: () => void; openDiff: () => void;
  openBranches: () => void; openMemory: () => void; openHistory: () => void; refreshSessions: () => Promise<void>;
  currentThreadId: () => string; currentTitle: () => string; setTitle: (title: string) => void;
};

export class SlashController {
  index = $state(0);
  private customCommands = $state<CustomCommand[]>([]);
  private argument = "";

  constructor(
    private readonly readInput: () => string,
    private readonly writeInput: (value: string) => void,
    private readonly notify: (message: string) => void,
    private readonly actions: SlashActions,
  ) {}

  get visible(): boolean {
    const input = this.readInput();
    return input.startsWith("/") && !input.includes("\n");
  }

  get matches(): SlashCommand[] {
    if (!this.visible) return [];
    const filter = this.readInput().slice(1).trim().toLowerCase();
    const head = filter.split(/\s+/)[0] ?? "";
    return [
      ...this.builtins().filter((command) => command.id.includes(head) || command.label.toLowerCase().includes(head)),
      ...this.customCommands.map((command) => ({
        id: command.slash.slice(1), label: command.name, desc: `自定义命令 · ${command.scope}`,
        run: () => this.runCustom(command.slash),
      })).filter((command) => command.id.includes(head) || command.label.toLowerCase().includes(head)),
    ];
  }

  loadCustomCommands = async (): Promise<void> => {
    try { this.customCommands = await invoke<CustomCommand[]>("get_custom_commands"); }
    catch { /* custom commands are optional */ }
  };

  run = (command: SlashCommand): void => {
    this.argument = this.readInput().replace(/^\/\S+\s*/, "");
    this.writeInput("");
    command.run();
  };

  private runCustom = async (slash: string): Promise<void> => {
    try { this.writeInput(await invoke<string>("expand_custom_command", { slash, arg: this.argument })); }
    catch (error) { this.notify(`自定义命令展开失败：${error}`); }
  };

  private rename = async (): Promise<void> => {
    const threadId = this.actions.currentThreadId();
    if (!threadId) return;
    const currentTitle = this.actions.currentTitle();
    const title = window.prompt("输入新的会话名称", currentTitle)?.trim();
    if (!title || title === currentTitle) return;
    try {
      await appServerRequest({ method: "threadRename", params: { threadId, title } });
      this.actions.setTitle(title);
      await this.actions.refreshSessions();
    } catch (error) { this.notify(`重命名失败：${error}`); }
  };

  private showMcp = async (): Promise<void> => {
    try {
      const rows = await invoke<{ name: string; command: string }[]>("list_mcp");
      this.notify(rows.length ? `MCP 服务器（${rows.length}）：\n` + rows.map((row) => `· ${row.name} — ${row.command}`).join("\n") : "未配置 MCP 服务器（~/.nanocodex/mcp.toml）。");
    } catch (error) { this.notify(`读取 MCP 失败：${error}`); }
  };

  private prompt = (text: string): void => {
    const scope = this.argument.trim();
    this.writeInput(scope ? `${text}\n\n本次范围或目标：${scope}` : text);
  };

  private documentPrompt = (format: "docx" | "pdf" | "pptx" | "xlsx", backend: string): void => {
    const target = this.argument.trim();
    this.writeInput(
      `请处理 ${target || `我接下来指定的 .${format} 文件`}。先判断需要读取、编辑还是创建；使用 ${backend}，不要手工解析二进制格式。` +
      `${target ? `\n目标文件：${target}` : "\n如果目标文件不明确，请先询问我。"}` +
      "\n完成后说明实际操作、验证结果和输出文件路径。",
    );
  };

  private builtins(): SlashCommand[] {
    const action = this.actions;
    const setInput = this.writeInput;
    return [
      { id: "new", label: "新建会话", desc: "开始一个空会话", run: action.newSession },
      { id: "fork", label: "分叉会话", desc: "从当前会话分叉一个新会话", run: action.forkCurrent },
      { id: "rename", label: "重命名会话", desc: "给当前会话改名", run: this.rename },
      { id: "model", label: "切换模型", desc: "打开模型选择", run: action.openModel },
      { id: "config", label: "设置", desc: "打开设置面板", run: action.openSettings },
      { id: "usage", label: "用量", desc: "显示本会话 token / 费用", run: action.showUsage },
      { id: "history", label: "会话历史", desc: "打开可继续、分叉和检查的历史会话", run: action.openHistory },
      { id: "rewind", label: "检查点", desc: "查看 / 恢复检查点", run: action.openCheckpoints },
      { id: "files", label: "文件", desc: "浏览 / 预览工作区文件", run: action.openFiles },
      { id: "diff", label: "改动", desc: "查看工作区 diff", run: action.openDiff },
      { id: "branches", label: "分支", desc: "Git 分支", run: action.openBranches },
      { id: "memory", label: "记忆", desc: "项目记忆", run: action.openMemory },
      { id: "mcp", label: "MCP", desc: "列出已配置的 MCP 服务器", run: this.showMcp },
      { id: "feedback", label: "反馈", desc: "打开 GitHub Issues", run: () => invoke("open_url", { url: "https://github.com/dgy-github/nanocodex/issues" }).catch((error) => this.notify(`打开反馈页失败：${error}`)) },
      { id: "review", label: "代码审查", desc: "填入基于真实 diff 的缺陷审查任务", run: () => this.prompt("审查当前代码改动的正确性和质量。先检查实际 git diff；逐项给出文件与行号、严重程度、问题和具体修复。重点关注真实 bug、边界、错误处理和非预期行为，最后明确是否可交付。") },
      { id: "security-review", label: "安全审查", desc: "填入基于真实 diff 的安全审查任务", run: () => this.prompt("对当前改动进行安全审查。先检查实际 git diff；重点检查命令、路径或注入风险，不安全反序列化，输入校验，密钥泄漏，权限缺口及危险文件或网络操作。每项给出文件与行号、严重程度、利用方式和修复；没有可利用问题也要说明残余风险。") },
      { id: "verify", label: "验证改动", desc: "填入要求构建、测试和证据的验证任务", run: () => this.prompt("验证最近的改动是否真的可用，不要只根据代码推断。先确认实际 diff 和预期行为，再运行能覆盖改动的最窄构建、测试及必要的运行态检查，报告真实命令与结果，最后给出“已验证”或“未验证”的明确结论。") },
      { id: "docx", label: "Word 文档", desc: "读取、编辑或创建 .docx", run: () => this.documentPrompt("docx", "python-docx 或 pandoc") },
      { id: "pdf", label: "PDF 文档", desc: "提取、检查或创建 .pdf", run: () => this.documentPrompt("pdf", "pdfplumber、pypdf、reportlab 或 pandoc") },
      { id: "pptx", label: "演示文稿", desc: "读取、编辑或创建 .pptx", run: () => this.documentPrompt("pptx", "python-pptx") },
      { id: "xlsx", label: "电子表格", desc: "读取、编辑或创建 .xlsx", run: () => this.documentPrompt("xlsx", "openpyxl 或 pandas") },
      { id: "ultrareview", label: "严格复查", desc: "用更严格标准复查（填入模板）", run: () => setInput("请用最严格的标准复查刚才的改动 / 结论：逐条列出潜在 bug、边界情况、错误假设与遗漏，并给出具体修正。") },
      { id: "btw", label: "补充说明", desc: "插入一条旁注（填入模板）", run: () => setInput("补充说明：") },
    ];
  }
}
