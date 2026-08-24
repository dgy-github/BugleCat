<script lang="ts">
  type ToolEntry = { name: string; args?: string; result?: string };
  type ToolGroup = { role: "tool_group"; tools: ToolEntry[]; settled: boolean };
  type ReasoningMsg = { role: "reasoning"; text: string; settled: boolean };
  type Message =
    | { role: "user" | "assistant" | "note" | "compact"; text: string }
    | ReasoningMsg
    | ToolGroup;

  let {
    messages,
    busy,
    streamingIdx,
    reasoningIdx,
    scroller = $bindable(),
    renderMarkdown,
    toolGroupFailureCount,
    toolOutcome,
  }: {
    messages: Message[];
    busy: boolean;
    streamingIdx: number | null;
    reasoningIdx: number | null;
    scroller: HTMLDivElement | undefined;
    renderMarkdown: (source: string) => string;
    toolGroupFailureCount: (group: ToolGroup) => number;
    toolOutcome: (result: string) => "ok" | "err" | "empty";
  } = $props();
</script>

<div class="scroll" bind:this={scroller}>
  {#if messages.length === 0}
    <div class="empty-wrap">
      <div class="empty-mark">✦</div>
      <p class="empty">让我检查或修改你的工作区。<br />试试「列出文件」或「用 apply_patch 创建 hello.txt」。</p>
    </div>
  {/if}
  {#each messages as message}
    {#if message.role === "user"}
      <div class="msg user"><div class="bubble">{message.text}</div></div>
    {:else if message.role === "assistant"}
      <div class="msg assistant"><div class="bubble md">{@html renderMarkdown(message.text)}</div></div>
    {:else if message.role === "note"}
      <div class="msg note">{message.text}</div>
    {:else if message.role === "compact"}
      <div class="msg compact"><span aria-hidden="true">◇</span>{message.text}</div>
    {:else if message.role === "reasoning"}
      <details class="reasoning-run" class:settled={message.settled}>
        <summary>
          <span class="reasoning-caret" aria-hidden="true">›</span>
          <span class="reasoning-label">思考过程</span>
          <span class="reasoning-status">{message.settled ? "查看" : "思考中…"}</span>
        </summary>
        <pre class="reasoning-content">{message.text}</pre>
      </details>
    {:else}
      <details class="tool-run" class:settled={message.settled} open={!message.settled}>
        <summary>
          <span class="tool-run-caret" aria-hidden="true">›</span>
          <span class="tool-run-icon" aria-hidden="true">⌘</span>
          <span class="tool-run-label">已执行 {message.tools.length} 个工具</span>
          {#if toolGroupFailureCount(message) > 0}
            <span class="tool-run-status error">{toolGroupFailureCount(message)} 个失败</span>
          {:else}
            <span class="tool-run-status">查看明细</span>
          {/if}
        </summary>
        <div class="tool-timeline">
          {#each message.tools as tool}
            <details
              class="tool-event"
              class:running={tool.result === undefined}
              class:error={tool.result !== undefined && toolOutcome(tool.result) === "err"}
            >
              <summary>
                <span class="tool-event-caret" aria-hidden="true">›</span>
                <span class="tool-event-icon" aria-hidden="true">⚙</span>
                <span class="tname">{tool.name}</span>
                {#if tool.result === undefined}
                  <span class="trunning">运行中</span>
                {:else}
                  <span class="tstatus {toolOutcome(tool.result)}">{toolOutcome(tool.result) === "err" ? "失败" : "完成"}</span>
                {/if}
              </summary>
              {#if tool.args}<pre class="tool-detail tool-args">参数：{tool.args}</pre>{/if}
              {#if tool.result !== undefined && toolOutcome(tool.result) !== "empty"}
                <pre class="tool-detail tool-result">{tool.result}</pre>
              {/if}
            </details>
          {/each}
        </div>
      </details>
    {/if}
  {/each}
  {#if busy && streamingIdx === null && reasoningIdx === null}
    <div class="thinking"><span class="tdot"></span><span class="tdot"></span><span class="tdot"></span> 思考中…</div>
  {/if}
</div>
