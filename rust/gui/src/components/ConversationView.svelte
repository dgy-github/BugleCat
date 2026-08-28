<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { buglecatAsset } from "../lib/buglecat-assets";
  type ToolEntry = { name: string; args?: string; result?: string };
  type ToolGroup = { role: "tool_group"; tools: ToolEntry[]; settled: boolean };
  type ReasoningMsg = { role: "reasoning"; text: string; settled: boolean };
  type Message =
    | { role: "user"; text: string; images?: string[] }
    | { role: "assistant" | "note" | "compact"; text: string }
    | { role: "orchestration"; stage: string; text: string }
    | { role: "orchestrator_activity"; worker: number; tool: string; phase: "started" | "finished"; failure?: string }
    | { role: "artifact"; kind: "image" | "video" | "file"; name: string; url: string }
    | ReasoningMsg
    | ToolGroup;

  let {
    messages,
    activeView,
    busy,
    streamingIdx,
    reasoningIdx,
    scroller = $bindable(),
    renderMarkdown,
    toolGroupFailureCount,
    toolOutcome,
    forkCurrent,
  }: {
    messages: Message[];
    activeView: "chat" | "trajectory";
    busy: boolean;
    streamingIdx: number | null;
    reasoningIdx: number | null;
    scroller: HTMLDivElement | undefined;
    renderMarkdown: (source: string) => string;
    toolGroupFailureCount: (group: ToolGroup) => number;
    toolOutcome: (result: string) => "ok" | "err" | "empty";
    forkCurrent: () => void;
  } = $props();

  let copiedIndex = $state<number | null>(null);
  let feedback = $state<Record<number, "good" | "bad">>({});
  let localPreviews = $state<Record<string, string | null>>({});
  let lastAssistantIndex = $derived.by(() => {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index]?.role === "assistant") return index;
    }
    return -1;
  });
  let lastUserIndex = $derived.by(() => {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (messages[index]?.role === "user") return index;
    }
    return -1;
  });

  async function copyMessage(text: string, index: number) {
    try {
      await navigator.clipboard.writeText(text);
      copiedIndex = index;
      window.setTimeout(() => { if (copiedIndex === index) copiedIndex = null; }, 1000);
    } catch { /* Clipboard denial leaves the action unchanged. */ }
  }

  function toggleFeedback(index: number, value: "good" | "bad") {
    const next = { ...feedback };
    if (next[index] === value) delete next[index]; else next[index] = value;
    feedback = next;
  }

  function openArtifact(url: string) {
    void invoke("open_url", { url });
  }

  function localArtifacts(text: string): string[] {
    const matches = text.match(/[A-Za-z]:\\(?:[^<>:"|?*\r\n`]+\\)*[^\\<>:"|?*\r\n`]+\.(?:png|jpe?g|webp|gif|bmp|svg|mp4|webm|mov|pdf)\b/gi) || [];
    return [...new Set(matches.map((path) => path.trim()))];
  }

  function openLocalArtifact(path: string) {
    void invoke("open_local_artifact", { path });
  }

  function noteTone(text: string): "neutral" | "success" | "warning" | "error" {
    if (/(错误|失败|不可用|拒绝|崩溃)/.test(text)) return "error";
    if (/(警告|注意|超时|阻断|⚠)/.test(text)) return "warning";
    if (/(已切换|已保存|已创建|已更新|成功|完成)/.test(text)) return "success";
    return "neutral";
  }

  $effect(() => {
    const paths = messages.flatMap((message) => message.role === "assistant" ? localArtifacts(message.text) : message.role === "user" ? (message.images ?? []) : []);
    for (const path of paths) {
      if (Object.prototype.hasOwnProperty.call(localPreviews, path)) continue;
      localPreviews = { ...localPreviews, [path]: null };
      void invoke<string>("local_artifact_preview", { path })
        .then((preview) => { localPreviews = { ...localPreviews, [path]: preview }; })
        .catch(() => { /* Unsupported/large images keep the clickable fallback card. */ });
    }
  });

  function openRenderedLink(event: MouseEvent) {
    const anchor = (event.target as HTMLElement).closest<HTMLAnchorElement>("a[href]");
    if (!anchor || !/^https?:\/\//.test(anchor.href)) return;
    event.preventDefault();
    openArtifact(anchor.href);
  }
</script>

<div class="scroll" bind:this={scroller}>
  {#if activeView === "trajectory"}
    <div class="trajectory-view">
      <div class="trajectory-head"><strong>本轮运行轨迹</strong><span>{messages.length} 个事件</span></div>
      {#if messages.length === 0}<div class="trajectory-empty">发送消息后，这里会按顺序显示模型、思考和工具事件。</div>{/if}
      {#each messages as message, index}
        <div class="trajectory-row" data-role={message.role}>
          <span class="trajectory-index">{String(index + 1).padStart(2, "0")}</span>
          <span class="trajectory-kind">{message.role === "user" ? "用户" : message.role === "assistant" ? "回答" : message.role === "reasoning" ? "思考" : message.role === "tool_group" ? "工具" : message.role === "artifact" ? "产物" : message.role === "compact" ? "压缩" : message.role === "orchestration" ? "编排" : message.role === "orchestrator_activity" ? `W${message.worker}` : "状态"}</span>
          <span class="trajectory-summary">{message.role === "tool_group" ? `${message.tools.map((tool) => tool.name).join(", ")} · ${message.settled ? "已完成" : "执行中"}` : message.role === "artifact" ? message.name : message.role === "orchestrator_activity" ? `${message.tool} · ${message.phase === "started" ? "执行中" : message.failure ? `失败 (${message.failure})` : "完成"}` : message.text.split("\n")[0]?.slice(0, 160)}</span>
        </div>
      {/each}
    </div>
  {:else}
  {#if messages.length === 0}
    <div class="empty-wrap">
      <img class="empty-cat" src={buglecatAsset("empty", 64)} alt="妙脆角猫咪" />
      <p class="empty"><strong>妙脆角猫咪准备好了</strong><br />把要检查、修改或生成的任务交给我。</p>
    </div>
  {/if}
  {#each messages as message, index}
    {#if message.role === "user"}
      <div class="message-block user-block"><div class="msg user"><div class="bubble">{#if message.text}<div>{message.text}</div>{/if}{#if message.images?.length}<div class="user-image-grid">{#each message.images as path}<button class="user-image-card" onclick={() => openLocalArtifact(path)} title={`打开 ${path}`}>{#if localPreviews[path]}<img src={localPreviews[path] || ""} alt={path.split(/[\\/]/).pop() || "上传图片"} />{:else}<span>图片加载中…</span>{/if}</button>{/each}</div>{/if}</div></div><div class="message-actions user-actions"><button onclick={() => copyMessage(message.text, index)} title="复制消息">{copiedIndex === index ? "✓" : "▢"}</button></div></div>
    {:else if message.role === "assistant"}
      <div class="message-block assistant-block">
        <div class="msg assistant"><img class="message-cat" src={buglecatAsset("avatar", 32)} alt="妙脆角猫咪" />
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="bubble md" onclick={openRenderedLink}>{@html renderMarkdown(message.text)}</div>
        </div>
        {#if message.confirmedModel && message.model && message.confirmedModel !== message.model}
          <div class="assistant-model mismatch" title="API 响应的 model 字段与请求值不一致；该字段不证明中转站上游的内部模型">请求 {message.model} → 响应字段 {message.confirmedModel}</div>
        {:else if message.confirmedModel}
          <div class="assistant-model confirmed" title={`API 返回的 model 字段；请求模型：${message.model || "未知"}。该字段不证明中转站上游的内部模型。`}>响应字段 · {message.confirmedModel}</div>
        {:else if message.model}
          <div class="assistant-model" title="服务端未返回 model 字段，本条仅显示请求型号">请求模型 · {message.model}</div>
        {/if}
        {#each localArtifacts(message.text) as path}
          <button class="local-artifact-card" class:has-preview={Boolean(localPreviews[path])} onclick={() => openLocalArtifact(path)} title={`打开 ${path}`}>
            {#if localPreviews[path]}<img src={localPreviews[path] || ""} alt={path.split(/[\\/]/).pop() || "生成图片"} />{:else}<span class="local-artifact-icon" aria-hidden="true">▧</span>{/if}
            <span class="local-artifact-copy"><strong>{path.split(/[\\/]/).pop()}</strong><code>{path}</code></span><span aria-hidden="true">↗</span>
          </button>
        {/each}
        {#if index !== streamingIdx}
          <div class="message-actions assistant-actions">
            <button onclick={() => copyMessage(message.text, index)} title="复制回答">{copiedIndex === index ? "✓" : "▢"}</button>
            <button class:on={feedback[index] === "good"} onclick={() => toggleFeedback(index, "good")} title="回答有帮助">♡</button>
            <button class:on={feedback[index] === "bad"} onclick={() => toggleFeedback(index, "bad")} title="回答需改进">♧</button>
            {#if index === lastAssistantIndex}<button onclick={forkCurrent} disabled={busy} title={busy ? "执行完成后可从这里分叉" : "从这条回答分叉新会话"}>⑂</button>{/if}
          </div>
        {/if}
      </div>
    {:else if message.role === "note"}
      <div class="msg note {noteTone(message.text)}">{message.text}</div>
    {:else if message.role === "compact"}
      <div class="msg compact"><span aria-hidden="true">◇</span>{message.text}</div>
    {:else if message.role === "orchestration"}
      <div class="msg orchestration"><span aria-hidden="true">◎</span><strong>{message.stage}</strong> · {message.text}</div>
    {:else if message.role === "artifact"}
      <button class="artifact-card" onclick={() => openArtifact(message.url)} title={`打开${message.name}`}>
        {#if message.kind === "image"}<img src={message.url} alt={message.name} />{:else}<span class="artifact-icon">{message.kind === "video" ? "▶" : "↗"}</span>{/if}
        <span class="artifact-copy"><strong>{message.name}</strong><small>{message.kind === "image" ? "图片" : message.kind === "video" ? "视频" : "文件"} · 点击打开</small><code>{message.url}</code></span>
        <span class="artifact-open" aria-hidden="true">↗</span>
      </button>
    {:else if message.role === "reasoning"}
      {#if !(busy && message.settled && index > lastUserIndex)}
      <details class="reasoning-run" class:settled={message.settled} class:current-run={busy && index > lastUserIndex}>
        <summary>
          <span class="reasoning-caret" aria-hidden="true">›</span>
          <span class="reasoning-label">思考过程</span>
          <span class="reasoning-status">{message.settled ? "查看" : "思考中…"}</span>
        </summary>
        <pre class="reasoning-content">{message.text}</pre>
      </details>
      {/if}
    {:else}
      {#if !(busy && message.settled && index > lastUserIndex)}
      <details class="tool-run" class:settled={message.settled} class:current-run={busy && index > lastUserIndex} open={!message.settled}>
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
    {/if}
  {/each}
  {#if busy && streamingIdx === null && reasoningIdx === null}
    <div class="thinking"><img class="thinking-cat" src={buglecatAsset("thinking", 24)} alt="" aria-hidden="true" /> 思考中…</div>
  {/if}
  {/if}
</div>
