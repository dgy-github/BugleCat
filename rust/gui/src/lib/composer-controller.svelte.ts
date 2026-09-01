import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { appServerRequest } from "./app-server-client";
import type { SlashController } from "./slash-controller.svelte";
import type { ThreadController } from "./thread-controller.svelte";

const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
export const isImageAttachment = (path: string): boolean => IMAGE_EXTENSIONS.includes((path.split(".").pop() || "").toLowerCase());

export class ComposerController {
  input = $state("");
  attached = $state<string[]>([]);
  executionMode = $state<"agent" | "orchestrator">("agent");
  executionMenuOpen = $state(false);
  private slash: SlashController | null = null;

  constructor(
    private readonly thread: ThreadController,
    private readonly needsWorkspace: () => boolean,
    private readonly scrollDown: () => void,
  ) {}

  connectSlash(slash: SlashController): void { this.slash = slash; }

  attachFiles = async (): Promise<void> => {
    try {
      const picked = await open({ multiple: true });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      for (const path of paths) if (!this.attached.includes(path)) this.attached.push(path);
    } catch (error) { this.thread.messages.push({ role: "note", text: `添加失败：${error}` }); }
  };

  removeAttachment = (path: string): void => {
    this.attached = this.attached.filter((item) => item !== path);
  };

  handlePaste = async (event: ClipboardEvent): Promise<void> => {
    const items = event.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (item.kind !== "file" || !item.type.startsWith("image/")) continue;
      event.preventDefault();
      const file = item.getAsFile();
      if (!file) continue;
      try {
        const bytes = new Uint8Array(await file.arrayBuffer());
        const extension = (item.type.split("/")[1] || "png").replace("jpeg", "jpg");
        const path = await invoke<string>("save_temp_image", { bytes: Array.from(bytes), ext: extension });
        if (!this.attached.includes(path)) this.attached.push(path);
      } catch (error) { this.thread.messages.push({ role: "note", text: `粘贴图片失败：${error}` }); }
    }
  };

  dispatch = async (text: string, images: string[], shown: string, executionMode = this.executionMode): Promise<void> => {
    if (this.thread.switching) return;
    const targetSessionId = this.thread.currentId;
    this.thread.beginTurn({ role: "user", text: shown, images: [...images] });
    this.thread.setRunning(targetSessionId, true);
    this.thread.busy = true;
    this.scrollDown();
    try { await appServerRequest({ method: "turnSubmit", params: { threadId: targetSessionId, text, images, executionMode } }); }
    catch (error) {
      this.thread.setRunning(targetSessionId, false);
      this.thread.messages.push({ role: "note", text: `发送失败：${error}` });
      this.thread.busy = false; this.thread.stopping = false; this.dequeue();
    }
  };

  stop = async (): Promise<void> => {
    if (!this.thread.busy) return;
    this.thread.stopping = true;
    this.thread.queued = [];
    this.thread.clearPrompts(this.thread.currentId);
    try { await appServerRequest({ method: "turnInterruptLatest", params: { threadId: this.thread.currentId } }); }
    catch (error) { this.thread.stopping = false; this.thread.messages.push({ role: "note", text: `停止失败：${error}` }); }
  };

  dequeue = (): void => {
    if (this.thread.switching) return;
    if (!this.thread.busy && this.thread.queued.length > 0) {
      const next = this.thread.queued.shift();
      if (next) void this.dispatch(next.text, next.images, next.shown, next.executionMode);
    }
  };

  send = (): void => {
    if (this.thread.switching) return;
    const text = this.input.trim();
    if (!text && this.attached.length === 0) return;
    if (this.needsWorkspace()) {
      this.thread.messages.push({ role: "note", text: "请先选择项目目录（左下角「工作区」或下方按钮），再开始对话。" });
      return;
    }
    const images = this.attached.filter(isImageAttachment);
    const files = this.attached.filter((path) => !isImageAttachment(path));
    const mentions = files.map((path) => `@\"${path}\"`).join(" ");
    const fullText = [text, mentions].filter(Boolean).join("\n");
    const shown = files.length
      ? `${text}${text ? "\n" : ""}📎 ${files.map((path) => path.split(/[\\/]/).pop() || path).join(", ")}`
      : text;
    const selectedImages = [...images];
    const selectedExecutionMode = this.executionMode;
    if (selectedExecutionMode === "orchestrator" && selectedImages.length > 0) {
      this.thread.messages.push({ role: "note", text: "多 Agent 模式暂不支持图片附件；请移除图片，或切换到 Agent 模式。" });
      return;
    }
    this.input = "";
    this.attached = [];
    if (this.thread.busy && this.thread.currentGoalRunning) {
      // A direct human instruction preempts automatic Goal continuation. The
      // backend cancels the admitted round, pauses the Goal, then executes this
      // input in the same transcript.
      void this.dispatch(fullText, selectedImages, shown, selectedExecutionMode);
      return;
    }
    if (this.thread.busy) {
      if (this.thread.queued.length >= 2) { this.thread.messages.push({ role: "note", text: "队列已满（2 条），请先等当前任务完成。" }); return; }
      this.thread.queued.push({ text: fullText, images: selectedImages, shown, executionMode: selectedExecutionMode });
      return;
    }
    void this.dispatch(fullText, selectedImages, shown, selectedExecutionMode);
  };

  onKey = (event: KeyboardEvent): void => {
    const slash = this.slash;
    if (slash?.visible && slash.matches.length) {
      if (event.key === "ArrowDown") { event.preventDefault(); slash.index = (slash.index + 1) % slash.matches.length; return; }
      if (event.key === "ArrowUp") { event.preventDefault(); slash.index = (slash.index - 1 + slash.matches.length) % slash.matches.length; return; }
      if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); slash.run(slash.matches[Math.min(slash.index, slash.matches.length - 1)]); return; }
      if (event.key === "Escape") { event.preventDefault(); this.input = ""; return; }
    }
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); this.send(); }
  };
}
