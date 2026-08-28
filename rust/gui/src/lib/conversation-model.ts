import { toolOutcome } from "./ui-format";

export type ToolEntry = { name: string; args?: string; result?: string };
export type ToolGroup = { role: "tool_group"; tools: ToolEntry[]; settled: boolean };
export type ReasoningMessage = { role: "reasoning"; text: string; settled: boolean };
export type ConversationMessage =
  | { role: "user"; text: string; images?: string[] }
  | { role: "assistant"; text: string; model?: string; confirmedModel?: string }
  | { role: "note" | "compact"; text: string }
  | { role: "orchestration"; stage: string; text: string }
  | { role: "orchestrator_activity"; worker: number; tool: string; phase: "started" | "finished"; failure?: string }
  | { role: "artifact"; kind: "image" | "video" | "file"; name: string; url: string }
  | ReasoningMessage
  | ToolGroup;

const REASONING_DISPLAY_MAX_CHARS = 4000;
const REASONING_OMITTED = "\n\n…较长思考已省略，仅保留最近内容…\n\n";

export function settleCompletedToolGroups(messages: ConversationMessage[]): void {
  for (const message of messages) {
    if (
      message.role === "tool_group" && !message.settled && message.tools.length > 0 &&
      message.tools.every((tool) => tool.result !== undefined)
    ) message.settled = true;
  }
}

export function appendReasoning(previous: string, delta: string): string {
  const combined = previous + delta;
  if (combined.length <= REASONING_DISPLAY_MAX_CHARS) return combined;
  const tailLength = REASONING_DISPLAY_MAX_CHARS - REASONING_OMITTED.length;
  return REASONING_OMITTED + combined.slice(-tailLength);
}

export function hideCompletedToolActivity(messages: ConversationMessage[]): ConversationMessage[] {
  return messages.filter((message) => message.role !== "tool_group");
}

export function keepConversationConclusions(messages: ConversationMessage[], finalText: string): ConversationMessage[] {
  const compacted: ConversationMessage[] = [];
  let pendingAnswer: Extract<ConversationMessage, { role: "assistant" }> | null = null;
  for (const message of messages) {
    if (message.role === "user") {
      if (pendingAnswer) compacted.push(pendingAnswer);
      compacted.push({ ...message });
      pendingAnswer = null;
    } else if (message.role === "assistant") {
      pendingAnswer = { ...message };
    } else if (message.role === "artifact") {
      if (pendingAnswer) { compacted.push(pendingAnswer); pendingAnswer = null; }
      compacted.push({ ...message });
    }
  }
  if (finalText.trim() !== "") pendingAnswer = pendingAnswer
    ? { ...pendingAnswer, text: finalText }
    : { role: "assistant", text: finalText };
  if (pendingAnswer) compacted.push(pendingAnswer);
  return compacted;
}

export function toolGroupFailureCount(group: ToolGroup): number {
  return group.tools.filter((tool) => tool.result !== undefined && toolOutcome(tool.result) === "err").length;
}
