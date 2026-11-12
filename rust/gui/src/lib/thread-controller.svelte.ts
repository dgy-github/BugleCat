import {
  appendReasoning, hideCompletedToolActivity, keepConversationConclusions,
  settleCompletedToolGroups, type ConversationMessage, type ToolEntry, type ToolGroup,
} from "./conversation-model";
import type { UsageController } from "./usage-controller.svelte";

export type Approval = { session_id: string; id: number; command: string; reason: string; cwd: string; details: string };
export type UserQuestion = { session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean };
export type UiEvent =
  | { kind: "ready"; model: string; provider_id: string; provider_protocol: string; sandbox: string; workspace: string; session_id: string; models: string[]; permission_mode: string; reasoning_effort: string; needs_workspace: boolean }
  | { kind: "assistant_delta"; session_id: string; text: string }
  | { kind: "verification"; session_id: string; status: string; candidate_hash: string }
  | { kind: "reasoning_delta"; session_id: string; text: string }
  | { kind: "context_compacted"; session_id: string; original_chars: number; edited_chars: number; dropped_messages: number; compressed_tool_results: number }
  | { kind: "orchestrator_stage"; session_id: string; stage: string; detail: string }
  | { kind: "orchestrator_activity"; session_id: string; worker: number; tool: string; phase: "started" | "finished"; failure?: string | null }
  | { kind: "goal_run_started"; session_id: string }
  | { kind: "human_turn_started"; session_id: string }
  | { kind: "assistant"; session_id: string; text: string; model: string; confirmed_model?: string | null }
  | { kind: "tool_start"; session_id: string; name: string; args: string }
  | { kind: "tool_result"; session_id: string; name: string; result: string }
  | { kind: "approval"; session_id: string; id: number; command: string; reason: string; cwd: string; details: string }
  | { kind: "question"; session_id: string; id: number; question: string; options: string[]; allow_free_text: boolean }
  | { kind: "done"; session_id: string; final_text: string; stop_reason: string; usage: Record<string, number>; task_id?: string }
  | { kind: "session_title"; session_id: string; title: string }
  | { kind: "loaded"; session_id: string; messages: { role: string; text: string; model?: string; confirmed_model?: string; verification?: string; tools?: ToolEntry[]; kind?: "image" | "video" | "file"; name?: string; url?: string }[] }
  | { kind: "error"; session_id: string; message: string };

type QueuedTurn = { text: string; images: string[]; shown: string; executionMode: "agent" | "orchestrator" };
type ThreadCallbacks = {
  refreshSessions: () => void; scrollDown: () => void; dequeue: () => void;
  ready: (event: Extract<UiEvent, { kind: "ready" }>) => void;
};

export class ThreadController {
  messages = $state<ConversationMessage[]>([]);
  trajectory = $state<ConversationMessage[]>([]);
  queued = $state<QueuedTurn[]>([]);
  busy = $state(false);
  reasoningIndex = $state<number | null>(null);
  streamingIndex = $state<number | null>(null);
  runningSessions = $state(new Set<string>());
  goalRunningSessions = $state(new Set<string>());
  stopping = $state(false);
  switching = $state(false);
  currentId = $state("");
  title = $state("新会话");
  approval = $state<Approval | null>(null);
  question = $state<UserQuestion | null>(null);
  questionAnswer = $state("");

  private readonly messagesBySession = new Map<string, ConversationMessage[]>();
  private readonly trajectoryBySession = new Map<string, ConversationMessage[]>();
  private readonly trajectoryStartBySession = new Map<string, number>();
  private readonly queuesBySession = new Map<string, QueuedTurn[]>();
  private readonly approvalsBySession = new Map<string, Approval>();
  private readonly questionsBySession = new Map<string, UserQuestion>();
  private readonly skippedLoadedSessions = new Set<string>();
  // A navigation operation temporarily clears currentId before its durable
  // target is created. Only that target's ready snapshot may bind the empty
  // UI; an old session's delayed ready event must be ignored.
  private pendingReadySession = "";
  // A failed workspace rollback can leave the UI intentionally unbound. In
  // that state even a matching-looking Ready event must be rejected until the
  // next explicit navigation installs a new expected session.
  private readyFenceClosed = false;
  private trajectoryStart = 0;
  private pendingVerification: string | undefined;
  private pendingAssistantDelta = "";
  private pendingReasoningDelta = "";
  private deltaFlushFrame: number | null = null;

  constructor(private readonly usage: UsageController, private readonly callbacks: ThreadCallbacks) {}

  accepts(sessionId: string): boolean {
    return sessionId === "" || (this.currentId !== "" && sessionId === this.currentId);
  }

  setRunning(sessionId: string, running: boolean): void {
    const next = new Set(this.runningSessions);
    if (running) next.add(sessionId); else next.delete(sessionId);
    this.runningSessions = next;
  }

  setGoalRunning(sessionId: string, running: boolean): void {
    const next = new Set(this.goalRunningSessions);
    if (running) next.add(sessionId); else next.delete(sessionId);
    this.goalRunningSessions = next;
  }

  get currentGoalRunning(): boolean { return this.goalRunningSessions.has(this.currentId); }

  get trajectoryMessages(): ConversationMessage[] {
    return this.busy ? this.messages.slice(this.trajectoryStart) : this.trajectory;
  }

  beginTurn(message: Extract<ConversationMessage, { role: "user" }>): void {
    this.trajectoryStart = this.messages.length;
    this.trajectoryStartBySession.set(this.currentId, this.trajectoryStart);
    this.messages.push(message);
    this.trajectory = [];
  }

  stash(sessionId: string): void {
    this.flushPendingDeltas();
    if (sessionId) {
      this.queuesBySession.set(sessionId, [...this.queued]);
      this.messagesBySession.set(sessionId, this.clone(this.messages));
      this.trajectoryBySession.set(sessionId, this.clone(this.trajectory));
      this.trajectoryStartBySession.set(sessionId, this.trajectoryStart);
    }
    this.queued = [];
    this.approval = null;
    this.question = null;
  }

  evictSessions(sessionIds: Iterable<string>): void {
    for (const id of sessionIds) {
      this.messagesBySession.delete(id);
      this.trajectoryBySession.delete(id);
      this.trajectoryStartBySession.delete(id);
      this.queuesBySession.delete(id);
      this.approvalsBySession.delete(id);
      this.questionsBySession.delete(id);
      this.skippedLoadedSessions.delete(id);
    }
  }

  recordBackgroundError(sessionId: string, noteText: string): void {
    if (!sessionId) return;
    const cached = this.clone(this.messagesBySession.get(sessionId) || []);
    cached.push({ role: "note", text: noteText });
    this.messagesBySession.set(sessionId, cached);
    this.queuesBySession.delete(sessionId);
  }

  restore(sessionId: string): void {
    this.queued = [...(this.queuesBySession.get(sessionId) || [])];
    this.approval = this.approvalsBySession.get(sessionId) || null;
    this.question = this.questionsBySession.get(sessionId) || null;
    this.questionAnswer = "";
    this.messages = this.clone(this.messagesBySession.get(sessionId) || []);
    this.trajectory = this.clone(this.trajectoryBySession.get(sessionId) || []);
    this.trajectoryStart = this.trajectoryStartBySession.get(sessionId) ?? this.messages.length;
  }

  clearPrompts(sessionId: string): void {
    this.approvalsBySession.delete(sessionId);
    this.questionsBySession.delete(sessionId);
    this.approval = null;
    this.question = null;
  }

  skipNextLoaded(sessionId: string): void { this.skippedLoadedSessions.add(sessionId); }
  clearSkippedLoaded(sessionId: string): void { this.skippedLoadedSessions.delete(sessionId); }
  expectReady(sessionId: string): void {
    this.pendingReadySession = sessionId;
    this.readyFenceClosed = false;
  }
  sealReadyFence(): void {
    this.readyFenceClosed = true;
  }
  // A ready fence is consumed only by the matching Ready event after it has
  // passed the active-session checks below. In particular, callers restoring
  // from a failed workspace transition must not clear it early: while the UI
  // is empty, that would let a delayed Ready from another workspace rebind it.
  private consumeExpectedReady(sessionId: string): void {
    if (this.pendingReadySession === sessionId) this.pendingReadySession = "";
  }

  removeApproval(sessionId: string): void { this.approvalsBySession.delete(sessionId); this.approval = null; }
  removeQuestion(sessionId: string): void { this.questionsBySession.delete(sessionId); this.question = null; this.questionAnswer = ""; }

  handle(event: UiEvent): void {
    if (event.kind !== "assistant_delta" && event.kind !== "reasoning_delta") {
      this.flushPendingDeltas();
    }
    switch (event.kind) {
      case "ready": this.handleReady(event); break;
      case "assistant_delta":
        if (!this.accepts(event.session_id) || event.text === "") return;
        if (this.pendingReasoningDelta) this.flushPendingDeltas();
        this.pendingAssistantDelta += event.text;
        this.scheduleDeltaFlush();
        return;
      case "reasoning_delta":
        if (!this.accepts(event.session_id) || event.text === "") return;
        if (this.pendingAssistantDelta) this.flushPendingDeltas();
        this.pendingReasoningDelta += event.text;
        this.scheduleDeltaFlush();
        return;
      case "verification": {
        if (!this.accepts(event.session_id)) break;
        this.pendingVerification = event.status;
        const draft = this.streamingIndex === null ? undefined : this.messages[this.streamingIndex];
        if (draft?.role === "assistant") draft.verification = event.status;
        else if (event.status === "invalidated") {
          const final = [...this.messages].reverse().find((message) => message.role === "assistant");
          if (final?.role === "assistant") final.verification = "unverified";
        }
        break;
      }
      case "context_compacted": {
        const compactMsg: ConversationMessage = { role: "compact", text: `已更新上下文摘要：${event.original_chars.toLocaleString()} → ${event.edited_chars.toLocaleString()} 字符，归纳 ${event.dropped_messages} 条历史消息，保留近期完整对话与工具调用。摘要用于恢复任务，代码事实以工作区、diff 和测试结果为准。` };
        if (this.accepts(event.session_id)) this.messages.push(compactMsg);
        else this.mutateBackgroundMessages(event.session_id, (messages) => { messages.push(compactMsg); });
        break;
      }
      case "orchestrator_stage": {
        const stageMsg: ConversationMessage = { role: "orchestration", stage: event.stage, text: event.detail };
        if (this.accepts(event.session_id)) this.messages.push(stageMsg);
        else this.mutateBackgroundMessages(event.session_id, (messages) => { messages.push(stageMsg); });
        break;
      }
      case "orchestrator_activity": this.handleOrchestratorActivity(event); break;
      case "goal_run_started":
        this.setRunning(event.session_id, true); this.setGoalRunning(event.session_id, true);
        if (this.accepts(event.session_id)) { this.busy = true; this.stopping = false; this.trajectoryStart = this.messages.length; }
        break;
      case "human_turn_started":
        this.setRunning(event.session_id, true); this.setGoalRunning(event.session_id, false);
        if (this.accepts(event.session_id)) { this.busy = true; this.stopping = false; }
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
    // Events from background sessions must not move the active reader's viewport.
    if (this.accepts(event.session_id)) this.callbacks.scrollDown();
  }

  private scheduleDeltaFlush(): void {
    if (this.deltaFlushFrame !== null) return;
    if (typeof window === "undefined" || typeof window.requestAnimationFrame !== "function") {
      this.flushPendingDeltas();
      this.callbacks.scrollDown();
      return;
    }
    this.deltaFlushFrame = window.requestAnimationFrame(() => {
      this.deltaFlushFrame = null;
      this.flushPendingDeltas();
      this.callbacks.scrollDown();
    });
  }

  flushPendingDeltas(): void {
    if (this.deltaFlushFrame !== null && typeof window !== "undefined" && typeof window.cancelAnimationFrame === "function") {
      window.cancelAnimationFrame(this.deltaFlushFrame);
    }
    this.deltaFlushFrame = null;
    if (this.pendingReasoningDelta) {
      const text = this.pendingReasoningDelta;
      this.pendingReasoningDelta = "";
      this.handleReasoningDelta({ text });
    }
    if (this.pendingAssistantDelta) {
      const text = this.pendingAssistantDelta;
      this.pendingAssistantDelta = "";
      this.handleAssistantDelta({ text });
    }
  }

  private handleReady(event: Extract<UiEvent, { kind: "ready" }>): void {
    if (this.readyFenceClosed) return;
    if (this.currentId === "" && this.pendingReadySession && event.session_id !== this.pendingReadySession) return;
    if (this.currentId !== "" && event.session_id !== this.currentId) return;
    if (event.session_id) {
      const unbound = this.currentId === "";
      this.currentId = event.session_id;
      this.consumeExpectedReady(event.session_id);
      this.usage.restore(this.currentId);
      if (unbound) this.restore(this.currentId);
    }
    this.callbacks.ready(event);
    this.callbacks.refreshSessions();
  }

  private handleAssistantDelta(event: { text: string }): void {
    this.settleReasoning();
    if (this.streamingIndex === null) {
      if (event.text === "") return;
      settleCompletedToolGroups(this.messages);
      this.pendingVerification = undefined;
      this.messages.push({ role: "assistant", text: event.text, verification: "draft" });
      this.streamingIndex = this.messages.length - 1;
    } else {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant") message.text += event.text;
    }
  }

  private handleReasoningDelta(event: { text: string }): void {
    if (event.text === "") return;
    if (this.reasoningIndex === null) {
      settleCompletedToolGroups(this.messages);
      this.messages.push({ role: "reasoning", text: appendReasoning("", event.text), settled: false });
      this.reasoningIndex = this.messages.length - 1;
    } else {
      const message = this.reasoningIndex === null ? undefined : this.messages[this.reasoningIndex];
      if (message?.role === "reasoning") message.text = appendReasoning(message.text, event.text);
    }
  }

  private mutateBackgroundMessages(
    sessionId: string,
    mutate: (messages: ConversationMessage[]) => ConversationMessage[] | void,
  ): void {
    if (!sessionId || !this.messagesBySession.has(sessionId)) return;
    const cached = this.clone(this.messagesBySession.get(sessionId) || []);
    const next = mutate(cached);
    this.messagesBySession.set(sessionId, next ?? cached);
  }

  private handleAssistant(event: Extract<UiEvent, { kind: "assistant" }>): void {
    if (!this.accepts(event.session_id)) {
      this.mutateBackgroundMessages(event.session_id, (messages) => {
        if (event.text.trim() === "") return;
        settleCompletedToolGroups(messages);
        const last = messages.at(-1);
        if (last?.role === "assistant" && last.verification === "draft") {
          last.text = event.text;
          last.model = event.model;
          last.confirmedModel = event.confirmed_model || undefined;
          last.verification = undefined;
        } else {
          messages.push({ role: "assistant", text: event.text, model: event.model, confirmedModel: event.confirmed_model || undefined });
        }
      });
      return;
    }
    this.settleReasoning();
    if (this.streamingIndex !== null) {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant") {
        if (event.text.trim() === "") this.messages.splice(this.streamingIndex, 1); else { message.text = event.text; message.model = event.model; message.confirmedModel = event.confirmed_model || undefined; message.verification = this.pendingVerification; }
      }
      this.streamingIndex = null;
    } else if (event.text.trim() !== "") {
      settleCompletedToolGroups(this.messages);
      this.messages.push({ role: "assistant", text: event.text, model: event.model, confirmedModel: event.confirmed_model || undefined, verification: this.pendingVerification });
    }
    this.pendingVerification = undefined;
  }

  private handleToolStart(event: Extract<UiEvent, { kind: "tool_start" }>): void {
    if (!this.accepts(event.session_id)) {
      this.mutateBackgroundMessages(event.session_id, (messages) => {
        for (const msg of messages) {
          if (msg.role === "assistant" && msg.verification === "draft") msg.verification = undefined;
        }
        const last = messages.at(-1);
        const entry: ToolEntry = { name: event.name, args: event.args };
        if (last?.role === "tool_group") last.tools.push(entry);
        else messages.push({ role: "tool_group", tools: [entry], settled: false });
      });
      return;
    }
    this.settleReasoning();
    if (this.streamingIndex !== null) {
      const message = this.messages[this.streamingIndex];
      if (message?.role === "assistant") {
        // Text streamed before a tool call is a finished progress note, not the
        // turn's answer: drop the draft badge (only the final reply is verified).
        if (message.text.trim() === "") this.messages.splice(this.streamingIndex, 1);
        else if (message.verification === "draft") message.verification = undefined;
      }
    }
    this.streamingIndex = null;
    const last = this.messages.at(-1);
    const entry: ToolEntry = { name: event.name, args: event.args };
    if (last?.role === "tool_group") last.tools.push(entry); else this.messages.push({ role: "tool_group", tools: [entry], settled: false });
  }

  private handleOrchestratorActivity(event: Extract<UiEvent, { kind: "orchestrator_activity" }>): void {
    if (!this.accepts(event.session_id)) {
      this.mutateBackgroundMessages(event.session_id, (messages) => {
        if (event.phase === "finished") {
          for (let index = messages.length - 1; index >= 0; index -= 1) {
            const message = messages[index];
            if (message?.role === "orchestrator_activity" && message.worker === event.worker && message.tool === event.tool && message.phase === "started") {
              message.phase = "finished";
              message.failure = event.failure || undefined;
              return;
            }
          }
        }
        messages.push({ role: "orchestrator_activity", worker: event.worker, tool: event.tool, phase: event.phase, failure: event.failure || undefined });
      });
      return;
    }
    if (event.phase === "finished") {
      for (let index = this.messages.length - 1; index >= 0; index -= 1) {
        const message = this.messages[index];
        if (message?.role === "orchestrator_activity" && message.worker === event.worker && message.tool === event.tool && message.phase === "started") {
          message.phase = "finished";
          message.failure = event.failure || undefined;
          return;
        }
      }
    }
    this.messages.push({ role: "orchestrator_activity", worker: event.worker, tool: event.tool, phase: event.phase, failure: event.failure || undefined });
  }

  private handleToolResult(event: Extract<UiEvent, { kind: "tool_result" }>): void {
    if (!this.accepts(event.session_id)) {
      this.mutateBackgroundMessages(event.session_id, (messages) => {
        let tool: ToolEntry | undefined;
        for (const message of messages) {
          if (message.role !== "tool_group") continue;
          const candidate = message.tools.find((item) => item.name === event.name && item.result === undefined);
          if (candidate) { tool = candidate; break; }
        }
        if (tool) tool.result = event.result;
        else messages.push({ role: "tool_group", tools: [{ name: event.name, result: event.result }], settled: false });
        for (const artifact of this.mediaArtifacts(event.name, event.result)) messages.push(artifact);
      });
      return;
    }
    let group: ToolGroup | undefined;
    let tool: ToolEntry | undefined;
    for (const message of this.messages) {
      if (message.role !== "tool_group") continue;
      const candidate = message.tools.find((item) => item.name === event.name && item.result === undefined);
      if (candidate) { group = message; tool = candidate; break; }
    }
    if (tool && group) tool.result = event.result;
    else this.messages.push({ role: "tool_group", tools: [{ name: event.name, result: event.result }], settled: false });
    for (const artifact of this.mediaArtifacts(event.name, event.result)) this.messages.push(artifact);
  }

  private mediaArtifacts(name: string, result: string): Extract<ConversationMessage, { role: "artifact" }>[] {
    const kind = name === "generate_image" ? "image" : name === "generate_video" ? "video" : null;
    if (!kind) return [];
    try {
      const payload = JSON.parse(result) as { status?: string; urls?: unknown[] };
      if (payload.status !== "succeeded" || !Array.isArray(payload.urls)) return [];
      return payload.urls.flatMap((value, index) => typeof value === "string" && /^https?:\/\//.test(value)
        ? [{ role: "artifact" as const, kind, name: `${kind === "image" ? "生成图片" : "生成视频"} ${index + 1}`, url: value }]
        : []);
    } catch { return []; }
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
    this.setGoalRunning(event.session_id, false);
    this.setRunning(event.session_id, false);
    this.usage.add(event.session_id, event.usage || {}, this.currentId);
    if (!this.accepts(event.session_id)) {
      this.mutateBackgroundMessages(event.session_id, (messages) => {
        settleCompletedToolGroups(messages);
        const start = Math.min(this.trajectoryStartBySession.get(event.session_id) ?? messages.length, messages.length);
        this.trajectoryBySession.set(event.session_id, this.clone(messages.slice(start)));
        let settled = hideCompletedToolActivity(messages.filter((m) => m.role !== "reasoning"));
        for (const msg of settled) if (msg.role === "assistant" && msg.verification === "draft") msg.verification = undefined;
        if (event.stop_reason === "completed") settled = keepConversationConclusions(settled, event.final_text);
        else settled.push({ role: "note", text: `[${event.stop_reason}] ${event.final_text}` });
        if (event.task_id) {
          const answer = [...settled].reverse().find((m) => m.role === "assistant");
          if (answer?.role === "assistant" && !answer.taskId) answer.taskId = event.task_id;
        }
        this.trajectoryStartBySession.set(event.session_id, settled.length);
        return settled;
      });
      this.callbacks.refreshSessions();
      return;
    }
    settleCompletedToolGroups(this.messages);
    this.settleReasoning();
    this.captureTrajectory(event.session_id);
    this.removeReasoning();
    this.messages = hideCompletedToolActivity(this.messages);
    // Nothing is still streaming once the turn is done.
    for (const message of this.messages) if (message.role === "assistant" && message.verification === "draft") message.verification = undefined;
    if (event.stop_reason === "completed") this.messages = keepConversationConclusions(this.messages, event.final_text);
    else this.messages.push({ role: "note", text: `[${event.stop_reason}] ${event.final_text}` });
    if (event.task_id) {
      // Bind this turn's answer to its recorded task so a rating hits exactly it.
      const answer = [...this.messages].reverse().find((message) => message.role === "assistant");
      if (answer?.role === "assistant" && !answer.taskId) answer.taskId = event.task_id;
    }
    this.streamingIndex = null;
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.trajectoryBySession.set(event.session_id, this.clone(this.trajectory));
    this.busy = this.runningSessions.has(this.currentId);
    this.stopping = false;
    this.callbacks.refreshSessions();
    if (!this.switching) this.callbacks.dequeue();
  }

  private handleLoaded(event: Extract<UiEvent, { kind: "loaded" }>): void {
    if (this.skippedLoadedSessions.delete(event.session_id)) {
      if (this.accepts(event.session_id) && !this.runningSessions.has(event.session_id)) {
        for (const restored of event.messages) {
          if (restored.role !== "assistant" || !restored.verification) continue;
          const existing = this.messages.find((message) => message.role === "assistant" && message.text === restored.text);
          if (existing?.role === "assistant") existing.verification = restored.verification;
        }
      }
      return;
    }
    if (!this.accepts(event.session_id)) return;
    this.pendingVerification = undefined;
    const restored = event.messages.flatMap((message): ConversationMessage[] => {
      if (message.role === "user" && message.text.trim()) return [{ role: "user", text: message.text }];
      if (message.role === "assistant" && message.text.trim()) return [{ role: "assistant", text: message.text, model: message.model, confirmedModel: message.confirmed_model, verification: message.verification }];
      if (message.role === "artifact" && message.url) return [{ role: "artifact", kind: message.kind || "file", name: message.name || "生成产物", url: message.url }];
      if (message.role.startsWith("artifact_") && message.text.trim()) {
        const [name, ...urlParts] = message.text.split("\n");
        const kind = message.role.slice("artifact_".length);
        const url = urlParts.join("\n").trim();
        if (/^https?:\/\//.test(url) && (kind === "image" || kind === "video" || kind === "file")) return [{ role: "artifact", kind, name: name || "生成产物", url }];
      }
      if (message.role === "tool_group" && message.tools?.length) return [{ role: "tool_group", tools: message.tools, settled: true }];
      if ((message.role === "note" || message.role === "compact") && message.text.trim()) return [{ role: message.role, text: message.text }];
      return [];
    });
    // Protocol artifacts are authoritative session items. A legacy snapshot loaded
    // immediately after threadActivate may not contain independently persisted artifacts.
    for (const artifact of this.messages.filter((message): message is Extract<ConversationMessage, { role: "artifact" }> => message.role === "artifact")) {
      if (!restored.some((message) => message.role === "artifact" && message.url === artifact.url)) restored.push(artifact);
    }
    const cached = this.messagesBySession.get(event.session_id);
    this.messages = this.runningSessions.has(event.session_id) && cached?.length ? this.clone(cached) : hideCompletedToolActivity(restored);
    if (!this.runningSessions.has(event.session_id)) {
      this.trajectory = this.clone(this.trajectoryBySession.get(event.session_id) || []);
      this.trajectoryStart = this.messages.length;
    }
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.streamingIndex = null;
    this.reasoningIndex = null;
    this.busy = this.runningSessions.has(event.session_id);
    this.stopping = false;
    this.callbacks.refreshSessions();
    if (!this.busy) this.callbacks.dequeue();
  }

  private handleError(event: Extract<UiEvent, { kind: "error" }>): void {
    this.setGoalRunning(event.session_id, false);
    this.setRunning(event.session_id, false);
    if (!this.accepts(event.session_id)) {
      this.recordBackgroundError(event.session_id, `错误：${event.message}`);
      this.callbacks.refreshSessions();
      return;
    }
    settleCompletedToolGroups(this.messages);
    this.settleReasoning();
    this.captureTrajectory(event.session_id);
    this.removeReasoning();
    this.messages = hideCompletedToolActivity(this.messages);
    this.streamingIndex = null;
    this.queued = [];
    this.messages.push({ role: "note", text: `错误：${event.message}` });
    this.messagesBySession.set(event.session_id, this.clone(this.messages));
    this.trajectoryBySession.set(event.session_id, this.clone(this.trajectory));
    this.busy = false;
    this.stopping = false;
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

  private captureTrajectory(sessionId: string): void {
    const start = Math.min(this.trajectoryStart, this.messages.length);
    this.trajectory = this.clone(this.messages.slice(start));
    this.trajectoryBySession.set(sessionId, this.clone(this.trajectory));
    this.trajectoryStartBySession.set(sessionId, this.messages.length);
  }

  private clone(messages: ConversationMessage[]): ConversationMessage[] {
    return messages.map((message) => message.role === "tool_group"
      ? { ...message, tools: message.tools.map((tool) => ({ ...tool })) }
      : { ...message });
  }
}
