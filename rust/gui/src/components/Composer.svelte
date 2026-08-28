<script lang="ts">
  import type { ProtocolGoalView } from "../lib/app-server-client";
  import type { ProviderRouteOption } from "../lib/model-controls-controller.svelte";
  type MenuOption = { id: string; label: string; desc: string };
  type SlashCommand = { id: string; label: string; desc: string; run: () => void };
  type QueuedTurn = { text: string; images: string[]; shown: string; executionMode: "agent" | "orchestrator" };

  let {
    models, currentModel, currentProvider, currentProviderName, currentProtocol, routeLabel, routes, header, modelMenuOpen = $bindable(), selectModel, selectRouteModel,
    reasoningEffort, reasoningMenuOpen = $bindable(), reasoningEfforts, selectReasoningEffort, reasoningLabel,
    permissionMode, modeMenuOpen = $bindable(), permissionModes, selectMode, modeIcon, modeLabel,
    goalView, goalStatusLabel, goalRemainingRounds, goalLoading, goalMenuOpen = $bindable(), pauseGoal, resumeGoal,
    busy, workspace, needsWorkspace, wsName, chooseWorkspace, turnCount, stepCount,
    tokIn, tokOut, priceIn, priceOut, priceCurrency, cost, fmtTok, currencySymbol, fmtCost,
    queued = $bindable(), attached, isImage, baseName, removeAttachment,
    showSlash, slashMatches, slashIdx = $bindable(), runSlash,
    input = $bindable(), onKey, handlePaste, attachFiles, stopping, stopGeneration, send,
  }: {
    models: string[]; currentModel: string; currentProvider: string; currentProviderName: string; currentProtocol: string; routeLabel: string; routes: ProviderRouteOption[]; header: string; modelMenuOpen: boolean; selectModel: (model: string) => void; selectRouteModel: (route: ProviderRouteOption, model: string) => void;
    reasoningEffort: string; reasoningMenuOpen: boolean; reasoningEfforts: MenuOption[];
    selectReasoningEffort: (id: string) => void; reasoningLabel: (id: string) => string;
    permissionMode: string; modeMenuOpen: boolean; permissionModes: MenuOption[];
    selectMode: (id: string) => void; modeIcon: (id: string) => string; modeLabel: (id: string) => string;
    goalView: ProtocolGoalView | null; goalStatusLabel: string; goalRemainingRounds: number; goalLoading: boolean; goalMenuOpen: boolean;
    pauseGoal: () => void; resumeGoal: () => void;
    busy: boolean; workspace: string; needsWorkspace: boolean; wsName: string; chooseWorkspace: () => void; turnCount: number; stepCount: number;
    tokIn: number; tokOut: number; priceIn: number; priceOut: number; priceCurrency: "CNY" | "USD"; cost: number;
    fmtTok: (value: number) => string; currencySymbol: (currency: "CNY" | "USD") => string; fmtCost: (value: number) => string;
    queued: QueuedTurn[]; attached: string[]; isImage: (path: string) => boolean; baseName: (path: string) => string;
    removeAttachment: (path: string) => void; showSlash: boolean; slashMatches: SlashCommand[]; slashIdx: number;
    runSlash: (command: SlashCommand) => void; input: string; onKey: (event: KeyboardEvent) => void;
    handlePaste: (event: ClipboardEvent) => void; attachFiles: () => void; stopping: boolean;
    stopGeneration: () => void; send: () => void;
  } = $props();
</script>

<footer>
  {#if queued.length}<div class="attachments">{#each queued as turn, index}<span class="chip queued-chip" title={turn.shown}>⏳ {turn.shown.split("\n")[0].slice(0, 40)}<button class="chipx" onclick={() => (queued = queued.filter((_, itemIndex) => itemIndex !== index))} aria-label="移除">×</button></span>{/each}</div>{/if}
  {#if attached.length}<div class="attachments">{#each attached as path}<span class="chip" title={path}>{isImage(path) ? "🖼" : "📄"} {baseName(path)}<button class="chipx" onclick={() => removeAttachment(path)} aria-label="移除">×</button></span>{/each}</div>{/if}
  {#if showSlash && slashMatches.length}
    <div class="slash-menu" role="listbox" aria-label="命令">{#each slashMatches as command, index}<button class="slash-item" class:on={index === slashIdx} role="option" aria-selected={index === slashIdx} onmouseenter={() => (slashIdx = index)} onclick={() => runSlash(command)}><span class="slash-cmd">/{command.id}</span><span class="slash-label">{command.label}</span><span class="slash-desc">{command.desc}</span></button>{/each}</div>
  {/if}
  {#if needsWorkspace}<div class="ws-warn"><span>⚠ 当前工作区是主目录（非项目），已暂停对话以免误操作。请选择项目目录。</span><button class="plain" onclick={chooseWorkspace}>选择项目目录</button></div>{/if}
  <div class="composer-shell">
    <textarea bind:value={input} onkeydown={onKey} oninput={() => { if (input.startsWith("/")) slashIdx = 0; }} onpaste={handlePaste} placeholder={needsWorkspace ? "请先选择项目目录…" : "给智能体发消息"} rows="2"></textarea>
    <div class="composer-meta">
      <button class="toolbtn attach" title="添加文件/图片" onclick={attachFiles} aria-label="添加">＋</button>
    <div class="model-wrap">
      <button class="model-pill" onclick={() => { modeMenuOpen = false; reasoningMenuOpen = false; modelMenuOpen = !modelMenuOpen; }} disabled={models.length === 0} title={`${routeLabel || "当前 Route"} · ${busy ? "当前任务继续使用原 Route，下一轮使用新 Route" : "切换模型"}`}><span class="model-provider">{currentProviderName || currentProvider || "Route"}</span><span>{currentModel || header}</span> ▾</button>
      {#if modelMenuOpen}
        <button class="menu-backdrop" aria-label="关闭" onclick={() => (modelMenuOpen = false)}></button>
        <div class="model-menu" role="menu">
          {#each routes as route (route.id)}
            <div class="model-route-head" class:active={route.active}><strong>{route.name}</strong><span>{route.protocol}</span></div>
            {#each route.models as model}
              <button class="model-opt" data-provider={route.id} data-model={model} role="menuitemradio" aria-checked={route.id === currentProvider && model === currentModel} onclick={() => selectRouteModel(route, model)}><span class="opt-check">{route.id === currentProvider && model === currentModel ? "✓" : ""}</span><span class="opt-name">{model}</span></button>
            {/each}
          {/each}
        </div>
      {/if}
    </div>
    <div class="reasoning-wrap">
      <button class="reasoning-pill" onclick={() => { modeMenuOpen = false; modelMenuOpen = false; reasoningMenuOpen = !reasoningMenuOpen; }} title={busy ? "当前运行不变，可选择下次会话使用的思考级别" : "切换思考级别；下次会话生效"}>思考：{reasoningLabel(reasoningEffort)} ▾</button>
      {#if reasoningMenuOpen}
        <button class="menu-backdrop" aria-label="关闭" onclick={() => (reasoningMenuOpen = false)}></button>
        <div class="model-menu reasoning-menu" role="menu">
          {#each reasoningEfforts as option}
            <button class="model-opt" role="menuitemradio" aria-checked={option.id === reasoningEffort} onclick={() => selectReasoningEffort(option.id)}><span class="opt-check">{option.id === reasoningEffort ? "✓" : ""}</span><span class="opt-text"><span class="opt-name">{option.label}</span><span class="opt-id">{option.desc}</span></span></button>
          {/each}
        </div>
      {/if}
    </div>
    <div class="approval-wrap">
      <button class="approval-pill" class:danger={permissionMode === "bypass"} class:plan={permissionMode === "plan"} onclick={() => { modelMenuOpen = false; reasoningMenuOpen = false; modeMenuOpen = !modeMenuOpen; }} title="权限模式">{modeIcon(permissionMode)} {modeLabel(permissionMode)} ▾</button>
      {#if modeMenuOpen}
        <button class="menu-backdrop" aria-label="关闭" onclick={() => (modeMenuOpen = false)}></button>
        <div class="approval-menu" role="menu">
          {#each permissionModes as option}
            <button class="approval-opt" role="menuitemradio" aria-checked={permissionMode === option.id} onclick={() => selectMode(option.id)}><span class="opt-check">{permissionMode === option.id ? "✓" : ""}</span><span class="opt-text"><span class="opt-name">{modeIcon(option.id)} {option.label}</span><span class="opt-id">{option.desc}</span></span></button>
          {/each}
        </div>
      {/if}
    </div>
    {#if goalView}
      <div class="goal-wrap">
        <button class="goal-pill" class:armed={goalView.activation === "armed"} class:blocked={goalView.goal.phase === "blocked"} onclick={() => { modelMenuOpen = false; reasoningMenuOpen = false; modeMenuOpen = false; goalMenuOpen = !goalMenuOpen; }} disabled={goalLoading} title="查看并控制当前会话的长期目标">目标：{goalStatusLabel} {goalView.activation === "armed" ? "●" : "○"}</button>
        {#if goalMenuOpen}
          <button class="menu-backdrop" aria-label="关闭" onclick={() => (goalMenuOpen = false)}></button>
          <div class="model-menu goal-menu" role="dialog" aria-label="长期目标状态">
            <strong>{goalView.goal.objective}</strong>
            <span>状态：{goalStatusLabel}</span>
            <span>自动续轮：{goalView.activation === "armed" ? "已允许" : "已关闭"}</span>
            <span>剩余上限：{goalRemainingRounds} 轮</span>
            {#if goalView.goal.blockedReason}<span class="goal-blocker">阻塞：{goalView.goal.blockedReason.message}</span>{/if}
            {#if goalView.activation === "armed" && goalView.goal.phase === "active"}
              <button class="goal-action pause" onclick={pauseGoal} disabled={goalLoading}>暂停自动续轮</button>
            {:else if goalView.goal.phase !== "complete" && goalRemainingRounds > 0}
              <p>继续会使用当前模型商，可能产生模型费用。</p>
              <button class="goal-action resume" onclick={resumeGoal} disabled={goalLoading}>确认并继续</button>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
      <span class="composer-spacer"></span>
      {#if busy}
        <button class="stop-btn visible" onclick={stopGeneration} title={stopping ? "再次停止" : "停止生成"} aria-label="停止生成">■</button>
      {:else}
        <button class="send-round" onclick={send} disabled={needsWorkspace || (input.trim() === "" && attached.length === 0)} aria-label="发送">↑</button>
      {/if}
    </div>
  </div>
  <div class="composer-underbar">
    <button class="ws-pill" class:warn={needsWorkspace} onclick={chooseWorkspace} title={needsWorkspace ? "当前在主目录（非项目），点击选择项目目录" : `工作区：${workspace}（点击切换）`}>📁 {needsWorkspace ? "选择项目目录" : wsName || "选择项目目录"}</button>
    {#if turnCount || tokIn || tokOut}<div class="composer-usage" title="本会话累计统计">{turnCount} 轮 · {stepCount} 步　|　输入 {fmtTok(tokIn)} token · 输出 {fmtTok(tokOut)} token{#if priceIn || priceOut}　|　累计 ≈ {currencySymbol(priceCurrency)}{fmtCost(cost)}{/if}</div>{/if}
  </div>
</footer>
