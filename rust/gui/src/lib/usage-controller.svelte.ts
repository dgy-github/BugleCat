type TokenUsage = { prompt_tokens: number; completion_tokens: number };

export class UsageController {
  promptTokens = $state(0);
  completionTokens = $state(0);
  priceIn = $state(0);
  priceOut = $state(0);
  currency = $state<"CNY" | "USD">("CNY");
  private readonly protocolUsage = new Map<string, TokenUsage>();

  get cost(): number {
    return (this.promptTokens / 1e6) * this.priceIn + (this.completionTokens / 1e6) * this.priceOut;
  }

  summary(): string {
    const symbol = this.currency === "USD" ? "$" : "¥";
    const formattedCost = this.cost >= 1 ? this.cost.toFixed(2) : this.cost.toFixed(4);
    const costText = this.priceIn || this.priceOut ? ` · ≈${symbol}${formattedCost}` : "";
    return `本会话用量：输入 ${this.promptTokens} / 输出 ${this.completionTokens} tokens${costText}`;
  }

  setPrice(priceIn: number, priceOut: number, currency: "CNY" | "USD"): void {
    this.priceIn = priceIn || 0;
    this.priceOut = priceOut || 0;
    this.currency = currency || "CNY";
  }

  replaceProtocolUsage(entries: Iterable<[string, TokenUsage]>): void {
    this.protocolUsage.clear();
    for (const [threadId, usage] of entries) this.protocolUsage.set(threadId, usage);
  }

  reset(): void {
    this.promptTokens = 0;
    this.completionTokens = 0;
  }

  restore(sessionId: string): void {
    this.reset();
    if (!sessionId) return;
    const protocolUsage = this.protocolUsage.get(sessionId);
    if (protocolUsage) {
      this.promptTokens = protocolUsage.prompt_tokens;
      this.completionTokens = protocolUsage.completion_tokens;
      this.persist(sessionId);
      return;
    }
    try {
      const stored = JSON.parse(localStorage.getItem(this.storageKey(sessionId)) || "null");
      if (Number.isFinite(stored?.prompt_tokens) && stored.prompt_tokens >= 0) this.promptTokens = stored.prompt_tokens;
      if (Number.isFinite(stored?.completion_tokens) && stored.completion_tokens >= 0) this.completionTokens = stored.completion_tokens;
    } catch { /* missing or invalid local usage is treated as zero */ }
  }

  add(sessionId: string, usage: Record<string, number>, activeSessionId: string): void {
    if (!sessionId) return;
    const prompt = usage.prompt_tokens || 0;
    const completion = usage.completion_tokens || 0;
    if (sessionId === activeSessionId) {
      this.promptTokens += prompt;
      this.completionTokens += completion;
      this.persist(sessionId);
      return;
    }
    try {
      const stored = JSON.parse(localStorage.getItem(this.storageKey(sessionId)) || "null");
      localStorage.setItem(this.storageKey(sessionId), JSON.stringify({
        prompt_tokens: (Number(stored?.prompt_tokens) || 0) + prompt,
        completion_tokens: (Number(stored?.completion_tokens) || 0) + completion,
      }));
    } catch { /* storage is optional */ }
  }

  persist(sessionId: string): void {
    if (!sessionId) return;
    try {
      localStorage.setItem(this.storageKey(sessionId), JSON.stringify({
        prompt_tokens: this.promptTokens,
        completion_tokens: this.completionTokens,
      }));
    } catch { /* storage is optional */ }
  }

  private storageKey(sessionId: string): string {
    return `ncx.sessionUsage.${sessionId}`;
  }
}
