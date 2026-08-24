import { invoke } from "@tauri-apps/api/core";

export type ProtocolThreadItem =
  | { type: "userMessage"; id: string; text: string }
  | { type: "assistantMessage"; id: string; text: string }
  | { type: "toolCall"; id: string; name: string; arguments: unknown }
  | { type: "toolResult"; id: string; callId: string; output: string; success: boolean }
  | { type: "reasoning"; id: string; summary: string }
  | { type: "contextCompaction"; id: string; summary: string; droppedItems: number };

export type ProtocolThread = {
  metadata: { id: string; workspace: string; title: string; archived: boolean; createdAt: number; updatedAt: number };
  turns: {
    id: string;
    status: string;
    items: ProtocolThreadItem[];
    startedAt: number;
    completedAt?: number;
    usage?: { tokens?: Record<string, number>; estimatedCost?: number; currency?: string };
  }[];
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
  title: string;
  snippet: string;
  user_messages: number;
  assistant_messages: number;
  tool_calls: number;
  updated_at: string;
  has_snapshot: boolean;
  archived: boolean;
};

type AppServerOutcome<T> = {
  response: { protocolVersion: number; payload: { type: string; data: T } };
};

export async function appServerRequest<T>(request: Record<string, unknown>): Promise<T> {
  const outcome = await invoke<AppServerOutcome<T>>("app_server_request", { request });
  if (outcome.response.protocolVersion !== 2) {
    throw new Error(`不支持的协议版本 ${outcome.response.protocolVersion}`);
  }
  return outcome.response.payload.data;
}

/** Rejects cross-version, malformed, duplicate and stale events per Thread. */
export class ProtocolSequenceGate {
  private readonly sequences = new Map<string, number>();

  accept(envelope: ProtocolEventEnvelope): boolean {
    if (envelope.protocolVersion !== 2 || !envelope.threadId) return false;
    const previous = this.sequences.get(envelope.threadId) || 0;
    if (envelope.sequence <= previous) return false;
    this.sequences.set(envelope.threadId, envelope.sequence);
    return true;
  }
}

export function threadToSessionRow(thread: ProtocolThread): SessionRow {
  let userMessages = 0;
  let assistantMessages = 0;
  let toolCalls = 0;
  let snippet = "";
  for (const item of thread.turns.flatMap((turn) => turn.items)) {
    if (item.type === "userMessage") { userMessages += 1; snippet = item.text; }
    else if (item.type === "assistantMessage") { assistantMessages += 1; snippet = item.text; }
    else if (item.type === "toolCall") toolCalls += 1;
  }
  return {
    session_id: thread.metadata.id,
    title: thread.metadata.title,
    snippet: Array.from(snippet).slice(0, 200).join(""),
    user_messages: userMessages,
    assistant_messages: assistantMessages,
    tool_calls: toolCalls,
    updated_at: String(thread.metadata.updatedAt),
    has_snapshot: thread.turns.length > 0,
    archived: thread.metadata.archived,
  };
}
