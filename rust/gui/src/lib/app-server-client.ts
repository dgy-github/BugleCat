import { invoke } from "@tauri-apps/api/core";

export type ProtocolThreadItem =
  | { type: "userMessage"; id: string; text: string }
  | { type: "goalMessage"; id: string; text: string; goalId: string; revision: number; round: number }
  | { type: "assistantMessage"; id: string; text: string; model?: string; confirmedModel?: string }
  | { type: "toolCall"; id: string; name: string; arguments: unknown }
  | { type: "toolResult"; id: string; callId: string; output: string; success: boolean }
  | { type: "artifact"; id: string; kind: "image" | "video" | "file"; name: string; url: string }
  | { type: "reasoning"; id: string; summary: string }
  | { type: "contextCompaction"; id: string; summary: string; droppedItems: number };

export type ProtocolThread = {
  metadata: { id: string; workspace: string; title: string; archived: boolean; harnessProfile: string; createdAt: number; updatedAt: number };
  turns: {
    id: string;
    status: string;
    executionMode?: "agent" | "orchestrator";
    items: ProtocolThreadItem[];
    startedAt: number;
    completedAt?: number;
    usage?: { tokens?: Record<string, number>; estimatedCost?: number; currency?: string };
  }[];
};

export type ProtocolGoalSnapshot = {
  id: string;
  revision: number;
  objective: string;
  phase: "active" | "paused" | "blocked" | "complete";
  blockedReason?: { code: string; message: string };
  maxGoalRounds: number;
  roundsStarted: number;
  createdAt: number;
  updatedAt: number;
};

export type ProtocolGoalView = {
  goal: ProtocolGoalSnapshot;
  activation: "armed" | "disarmed";
};

export type ProtocolEventEnvelope = {
  protocolVersion: number;
  sequence: number;
  threadId: string;
  turnId?: string;
  event: { type: string; data?: unknown };
};

export type SessionRow = {
  session_id: string;
  workspace: string;
  title: string;
  snippet: string;
  user_messages: number;
  assistant_messages: number;
  tool_calls: number;
  updated_at: string;
  has_snapshot: boolean;
  archived: boolean;
  harness_profile: string;
};

type AppServerOutcome<T> = {
  response: { protocolVersion: number; payload: { type: string; data: T } };
};

export async function appServerRequest<T>(request: Record<string, unknown>): Promise<T> {
  const outcome = await invoke<AppServerOutcome<T>>("app_server_request", { request });
  if (outcome.response.protocolVersion !== 3) {
    throw new Error(`不支持的协议版本 ${outcome.response.protocolVersion}`);
  }
  return outcome.response.payload.data;
}

/** Rejects cross-version, malformed, duplicate and stale events per Thread. */
export class ProtocolSequenceGate {
  private readonly sequences = new Map<string, number>();

  accept(envelope: ProtocolEventEnvelope): boolean {
    if (envelope.protocolVersion !== 3 || !envelope.threadId) return false;
    const previous = this.sequences.get(envelope.threadId) || 0;
    if (envelope.sequence <= previous) return false;
    this.sequences.set(envelope.threadId, envelope.sequence);
    return true;
  }
}

export function normalizeWorkspacePath(path: string): string {
  let normalized = path.trim();
  if (normalized.startsWith("\\\\?\\UNC\\")) normalized = `\\\\${normalized.slice(8)}`;
  else if (normalized.startsWith("\\\\?\\")) normalized = normalized.slice(4);
  normalized = normalized.replace(/\//g, "\\");
  if (/^[a-z]:\\/i.test(normalized)) normalized = `${normalized[0].toUpperCase()}${normalized.slice(1)}`;
  return normalized.length > 3 ? normalized.replace(/\\+$/, "") : normalized;
}

function historicalFallbackTitle(text: string): string {
  const normalized = text.trim().replace(/\s+/g, " ");
  if (!normalized) return "新会话";
  if (["你好", "您好", "在吗", "嗨", "hello", "hi"].includes(normalized.toLowerCase())) return "日常问候";
  const withoutPrefix = normalized.replace(/^(可以帮我|能不能帮我|能否帮我|请帮我|麻烦帮我|帮我)[ ，,：:]*/, "");
  const chars = Array.from(withoutPrefix || normalized);
  return `${chars.slice(0, 24).join("")}${chars.length > 24 ? "…" : ""}`;
}

export function threadToSessionRow(thread: ProtocolThread): SessionRow {
  let userMessages = 0;
  let assistantMessages = 0;
  let toolCalls = 0;
  let snippet = "";
  let firstUserMessage = "";
  for (const item of thread.turns.flatMap((turn) => turn.items)) {
    if (item.type === "userMessage") {
      userMessages += 1;
      snippet = item.text;
      if (!firstUserMessage) firstUserMessage = item.text;
    }
    else if (item.type === "assistantMessage") { assistantMessages += 1; snippet = item.text; }
    else if (item.type === "toolCall") toolCalls += 1;
  }
  return {
    session_id: thread.metadata.id,
    workspace: normalizeWorkspacePath(thread.metadata.workspace),
    title: thread.metadata.title === "(no prompt yet)" ? historicalFallbackTitle(firstUserMessage) : thread.metadata.title,
    snippet: Array.from(snippet).slice(0, 200).join(""),
    user_messages: userMessages,
    assistant_messages: assistantMessages,
    tool_calls: toolCalls,
    updated_at: String(thread.metadata.updatedAt),
    has_snapshot: thread.turns.length > 0,
    archived: thread.metadata.archived,
    harness_profile: thread.metadata.harnessProfile || "full",
  };
}
