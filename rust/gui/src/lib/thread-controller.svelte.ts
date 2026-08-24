import {
  appendReasoning, hideCompletedToolActivity, keepConversationConclusions,
  settleCompletedToolGroups, type ConversationMessage, type ToolEntry, type ToolGroup,
} from "./conversation-model";
import type { UsageController } from "./usage-controller.svelte";

export type Approval = { session_id: string; id: number; command: string; reason: string; cwd: string; details: string };
export type UserQuestion = { session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean };
export type UiEvent =
  | { kind: "ready"; model: string; sandbox: string; workspace: string; session_id: string; models: string[]; permission_mode: string; reasoning_effort: string; needs_workspace: boolean }
  | { kind: "assistant_delta"; session_id: string; text: string }
  | { kind: "reasoning_delta"; session_id: string; text: string }
  | { kind: "context_compacted"; session_id: string; original_chars: number; edited_chars: number; dropped_messages: number; compressed_tool_results: number }
  | { kind: "assistant"; session_id: string; text: string }
  | { kind: "tool_start"; session_id: string; name: string; args: string }
  | { kind: "tool_result"; session_id: string; name: string; result: string }
  | { kind: "approval"; session_id: string; id: number; command: string; reason: string; cwd: string; details: string }
  | { kind: "question"; session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean }
  | { kind: "done"; session_id: string; final_text: string; stop_reason: string; usage: Record<string, number> }
  | { kind: "session_title"; session_id: string; title: string }
  | { kind: "loaded"; session_id: string; messages: { role: string; text: string; tools?: ToolEntry[] }[] }
  | { kind: "error"; session_id: string; message: string };

type QueuedTurn = { text: string; images: string[]; shown: string };
type ThreadCallbacks = {
  refreshSessions: () => void; scrollDown: () => void; dequeue: () => void;
  ready: (event: Extract<UiEvent, { kind: "ready" }>) => void;
};

export class ThreadController {
  messages = $state<ConversationMessage[]>([]);
  queued = $state<QueuedTurn[]>([]);
  busy = $state(false);
  reasoningIndex = $state<number | null>(null);
  streamingIndex = $state<number | null>(null);
  runningSessions = $state(new Set<string>());
  stopping = $state(false);
  switching = $state(false);
  currentId = $state("");
  title = $state("新会话");
  approval = $state<Approval | null>(null);
  question = $state<UserQuestion | null>(null);
  questionAnswer = $state("");

  private readonly messagesBySession = new Map<string, ConversationMessage[]>();
  private readonly queuesBySession = new Map<string, QueuedTurn[]>();
  private readonly approvalsBySession = new Map<string, Approval>();
  private readonly questionsBySession = new Map<string, UserQuestion>();

  constructor(private readonly usage: UsageController, private readonly callbacks: ThreadCallbacks) {}

  accepts(sessionId: string): boolean {
    return sessionId === "" || (this.currentId !== "" && sessionId === this.currentId);
  }

  setRunning(sessionId: string, running: boolean): void {
    const next = new Set(this.runningSessions);
    if (running) next.add(sessionId); else next.delete(sessionId);
    this.runningSessions = next;
  }

  stash(sessionId: string): void {
    if (sessionId) {
      this.queuesBySession.set(sessionId, [...this.queued]);
      this.messagesBySession.set(sessionId, this.clone(this.messages));
    }
    this.queued = [];
    this.approval = null;
    this.question = null;
  }

  restore(sessionId: string): void {
    this.queued = [...(this.queuesBySession.get(sessionId) || [])];
    this.approval = this.approvalsBySession.get(sessionId) || null;
    this.question = this.questionsBySession.get(sessionId) || null;
    this.questionAnswer = "";
    this.messages = this.clone(this.messagesBySession.get(sessionId) || []);
  }

  clearPrompts(sessionId: string): void {
    this.approvalsBySession.delete(sessionId);
    this.questionsBySession.delete(sessionId);
    this.approval = null;
    this.question = null;
  }

  removeApproval(sessionId: string): void { this.approvalsBySession.delete(sessionId); this.approval = null; }
  removeQuestion(sessionId: string): void { this.questionsBySession.delete(sessionId); this.question = null; this.questionAnswer = ""; }

  handle(event: UiEvent): void {
    switch (event.kind) {
      case "ready": this.handleReady(event); break;
      case "assistant_delta": this.handleAssistantDelta(event); break;
      case "reasoning_delta": this.handleReasoningDelta(event); break;
      case "context_compacted":
        if (this.accepts(event.session_id)) this.messages.push({ role: "compact", text: `已自动压缩上下文：${event.original_chars.toLocaleString()} → ${event.edited_chars.toLocaleString()} 字符，清理 ${event.dropped_messages} 条旧消息和 ${event.compressed_tool_results} 条工具结果；关键要求、完成结果和当前计划已保留。` });
        break;
      case "assistant": this.handleAssistant(event); break;
      case "tool_start": this.handleToolStart(event); break;
      case "tool_result": this.handleToolResult(event); break;
      case "approval": this.handleApproval(event); break;
      case "question": this.handleQuestion(event); break;
      case "done": this.handleDone(event); break;
      case "session_title": if (this.accepts(event.session_id)) this.title = event.title; this.callbacks.refreshSessions(); break;
      case "loaded": this.handleLoaded(event); break;
      case "error": this.handleError(event); break;
    }
    this.callbacks.scrollDown();
  }

  private handleReady(event: Extract<UiEvent, { kind: "ready" }>): void {
    if (this.currentId !== "" && event.session_id !== this.currentId) return;
    this.callbacks.ready(event);
    if (event.session_id) {
      const unbound = this.currentId === "";
      this.currentId = event.session_id;
      this.usage.restore(this.currentId);
      if (unbound) this.restore(this.currentId);
    }
    this.callbacks.refreshSessions();
  }

  private handleAssistantDelta(event: Extract<UiEvent, { kind: "assistant_delta" }>): void {
    if (!this.accepts(event.session_id)) return;
    this.settleReasoning();
    if (this.streamingIndex === null) {
      if (event.text === "") return;
      settleCompletedToolGroups(this.messages);
      this.messages.push({ role: "assistant", text: event.text });
      this.streamingIndex = this.messages.length - 1;
    } else {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant") message.text += event.text;
    }
  }

  private handleReasoningDelta(event: Extract<UiEvent, { kind: "reasoning_delta" }>): void {
    if (!this.accepts(event.session_id) || event.text === "") return;
    if (this.reasoningIndex === null) {
      settleCompletedToolGroups(this.messages);
      this.messages.push({ role: "reasoning", text: appendReasoning("", event.text), settled: false });
      this.reasoningIndex = this.messages.length - 1;
    } else {
      const message = this.messages[this.reasoningIndex];
      if (message?.role === "reasoning") message.text = appendReasoning(message.text, event.text);
    }
  }

  private handleAssistant(event: Extract<UiEvent, { kind: "assistant" }>): void {
    if (!this.accepts(event.session_id)) return;
    this.settleReasoning();
    if (this.streamingIndex !== null) {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant") {
        if (event.text.trim() === "") this.messages.splice(this.streamingIndex, 1); else message.text = event.text;
      }
      this.streamingIndex = null;
    } else if (event.text.trim() !== "") {
      settleCompletedToolGroups(this.messages);
      this.messages.push({ role: "assistant", text: event.text });
    }
  }

  private handleToolStart(event: Extract<UiEvent, { kind: "tool_start" }>): void {
    if (!this.accepts(event.session_id)) return;
    this.settleReasoning();
    if (this.streamingIndex !== null) {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant" && message.text.trim() === "") this.messages.splice(this.streamingIndex, 1);
    }
    this.streamingIndex = null;
    const last = this.messages.at(-1);
    const entry: ToolEntry = { name: event.name, args: event.args };
    if (last?.role === "tool_group") last.tools.push(entry); else this.messages.push({ role: "tool_group", tools: [entry], settled: false });
  }

  private handleToolResult(event: Extract<UiEvent, { kind: "tool_result" }>): void {
    if (!this.accepts(event.session_id)) return;
    let group: ToolGroup | undefined;
    let tool: ToolEntry | undefined;
    for (const message of this.messages) {
      if (message.role !== "tool_group") continue;
      const candidate = message.tools.find((item) => item.name === event.name && item.result === undefined);
      if (candidate) { group = message; tool = candidate; break; }
    }
    if (tool && group) tool.result = event.result;
    else this.messages.push({ role: "tool_group", tools: [{ name: event.name, result: event.result }], settled: false });
  }

  private handleApproval(event: Extract<UiEvent, { kind: "approval" }>): void {
    const approval: Approval = { session_id: event.session_id, id: event.id, command: event.command, reason: event.reason, cwd: event.cwd, details: event.details };
    this.approvalsBySession.set(event.session_id, approval);
    if (this.accepts(event.session_id)) this.approval = approval;
  }

  private handleQuestion(event: Extract<UiEvent, { kind: "question" }>): void {
    const question: UserQuestion = { session_id: event.session_id, id: event.id, question: event.question, options: event.options, allow_free_text: event.allow_free_text };
    this.questionsBySession.set(event.session_id, question);
    if (this.accepts(event.session_id)) { this.question = question; this.questionAnswer = ""; }
  }

  private handleDone(event: Extract<UiEvent, { kind: "done" }>): void {
    this.setRunning(event.session_id, false);
    this.usage.add(event.session_id, event.usage || {}, this.currentId);
    if (!this.accepts(event.session_id)) { this.callbacks.refreshSessions(); return; }
    settleCompletedToolGroups(this.messages);
    this.settleReasoning();
    this.removeReasoning();
    this.messages = hideCompletedToolActivity(this.messages);
    if (event.stop_reason === "completed") this.messages = keepConversationConclusions(this.messages, event.final_text);
    else this.messages.push({ role: "note", text: `[${event.stop_reason}] ${event.final_text}` });
    this.streamingIndex = null;
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.busy = this.runningSessions.has(this.currentId);
    this.stopping = false;
    this.callbacks.refreshSessions();
    if (!this.switching) this.callbacks.dequeue();
  }

  private handleLoaded(event: Extract<UiEvent, { kind: "loaded" }>): void {
    if (!this.accepts(event.session_id)) return;
    const restored = event.messages.flatMap((message): ConversationMessage[] => {
      if ((message.role === "user" || message.role === "assistant") && message.text.trim()) return [{ role: message.role, text: message.text }];
      if (message.role === "tool_group" && message.tools?.length) return [{ role: "tool_group", tools: message.tools, settled: true }];
      if ((message.role === "note" || message.role === "compact") && message.text.trim()) return [{ role: message.role, text: message.text }];
      return [];
    });
    const cached = this.messagesBySession.get(event.session_id);
    this.messages = this.runningSessions.has(event.session_id) && cached?.length ? this.clone(cached) : hideCompletedToolActivity(restored);
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.streamingIndex = null;
    this.reasoningIndex = null;
    this.busy = this.runningSessions.has(event.session_id);
    this.stopping = false;
    this.switching = false;
    this.callbacks.refreshSessions();
    if (!this.busy) this.callbacks.dequeue();
  }

  private handleError(event: Extract<UiEvent, { kind: "error" }>): void {
    this.setRunning(event.session_id, false);
    if (!this.accepts(event.session_id)) return;
    settleCompletedToolGroups(this.messages);
    this.settleReasoning();
    this.removeReasoning();
    this.messages = hideCompletedToolActivity(this.messages);
    this.streamingIndex = null;
    this.messages.push({ role: "note", text: `错误：${event.message}` });
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.busy = false;
    this.stopping = false;
    this.switching = false;
  }

  private settleReasoning(): void {
    if (this.reasoningIndex === null) return;
    const message = this.messages[this.reasoningIndex];
    if (message?.role === "reasoning") message.settled = true;
    this.reasoningIndex = null;
  }

  private removeReasoning(): void {
    this.messages = this.messages.filter((message) => message.role !== "reasoning");
    this.reasoningIndex = null;
  }

  private clone(messages: ConversationMessage[]): ConversationMessage[] {
    return messages.map((message) => message.role === "tool_group"
      ? { ...message, tools: message.tools.map((tool) => ({ ...tool })) }
      : { ...message });
  }
}
