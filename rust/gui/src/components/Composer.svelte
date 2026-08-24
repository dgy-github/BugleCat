<script lang="ts">
  type MenuOption = { id: string; label: string; desc: string };
  type SlashCommand = { id: string; label: string; desc: string; run: () => void };
  type QueuedTurn = { text: string; images: string[]; shown: string };

  let {
    models, currentModel, header, modelMenuOpen = $bindable(), selectModel,
    reasoningEffort, reasoningMenuOpen = $bindable(), reasoningEfforts, selectReasoningEffort, reasoningLabel,
    permissionMode, modeMenuOpen = $bindable(), permissionModes, selectMode, modeIcon, modeLabel,
    busy, workspace, needsWorkspace, wsName, chooseWorkspace,
    tokIn, tokOut, priceIn, priceOut, priceCurrency, cost, fmtTok, currencySymbol, fmtCost,
    queued = $bindable(), attached, isImage, baseName, removeAttachment,
    showSlash, slashMatches, slashIdx = $bindable(), runSlash,
    input = $bindable(), onKey, handlePaste, attachFiles, stopping, stopGeneration, send,
  }: {
    models: string[]; currentModel: string; header: string; modelMenuOpen: boolean; selectModel: (model: string) => void;
    reasoningEffort: string; reasoningMenuOpen: boolean; reasoningEfforts: MenuOption[];
    selectReasoningEffort: (id: string) => void; reasoningLabel: (id: string) => string;
    permissionMode: string; modeMenuOpen: boolean; permissionModes: MenuOption[];
    selectMode: (id: string) => void; modeIcon: (id: string) => string; modeLabel: (id: string) => string;
    busy: boolean; workspace: string; needsWorkspace: boolean; wsName: string; chooseWorkspace: () => void;
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
  <div class="composer-meta">
    <div class="model-wrap">
      <button class="model-pill" onclick={() => { modeMenuOpen = false; reasoningMenuOpen = false; modelMenuOpen = !modelMenuOpen; }} disabled={models.length === 0 || busy} title="切换模型">{currentModel || header} ▾</button>
      {#if modelMenuOpen}
        <button class="menu-backdrop" aria-label="关闭" onclick={() => (modelMenuOpen = false)}></button>
        <div class="model-menu" role="menu">
          {#each models as model}
            <button class="model-opt" role="menuitemradio" aria-checked={model === currentModel} onclick={() => selectModel(model)}><span class="opt-check">{model === currentModel ? "✓" : ""}</span><span class="opt-name">{model}</span></button>
          {/each}
        </div>
      {/if}
    </div>
    <div class="reasoning-wrap">
      <button class="reasoning-pill" onclick={() => { modeMenuOpen = false; modelMenuOpen = false; reasoningMenuOpen = !reasoningMenuOpen; }} disabled={busy} title="切换 DeepSeek 思考模式">思考：{reasoningLabel(reasoningEffort)} ▾</button>
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
    <button class="ws-pill" class:warn={needsWorkspace} onclick={chooseWorkspace} title={needsWorkspace ? "当前在主目录（非项目），点击选择项目目录" : `工作区：${workspace}（点击切换）`}>📁 {needsWorkspace ? "选择项目目录" : wsName || "选择项目目录"}</button>
    {#if tokIn || tokOut}<span class="usage" title="本会话累计 token（输入 / 输出）">用量 ↑{fmtTok(tokIn)} ↓{fmtTok(tokOut)}{#if priceIn || priceOut}{" · ≈"}{currencySymbol(priceCurrency)}{fmtCost(cost)}{/if}</span>{/if}
  </div>
  {#if queued.length}<div class="attachments">{#each queued as turn, index}<span class="chip queued-chip" title={turn.shown}>⏳ {turn.shown.split("\n")[0].slice(0, 40)}<button class="chipx" onclick={() => (queued = queued.filter((_, itemIndex) => itemIndex !== index))} aria-label="移除">×</button></span>{/each}</div>{/if}
  {#if attached.length}<div class="attachments">{#each attached as path}<span class="chip" title={path}>{isImage(path) ? "🖼" : "📄"} {baseName(path)}<button class="chipx" onclick={() => removeAttachment(path)} aria-label="移除">×</button></span>{/each}</div>{/if}
  {#if showSlash && slashMatches.length}
    <div class="slash-menu" role="listbox" aria-label="命令">{#each slashMatches as command, index}<button class="slash-item" class:on={index === slashIdx} role="option" aria-selected={index === slashIdx} onmouseenter={() => (slashIdx = index)} onclick={() => runSlash(command)}><span class="slash-cmd">/{command.id}</span><span class="slash-label">{command.label}</span><span class="slash-desc">{command.desc}</span></button>{/each}</div>
  {/if}
  {#if needsWorkspace}<div class="ws-warn"><span>⚠ 当前工作区是主目录（非项目），已暂停对话以免误操作。请选择项目目录。</span><button class="plain" onclick={chooseWorkspace}>选择项目目录</button></div>{/if}
  <div class="composer-row">
    <button class="toolbtn attach" title="添加文件/图片" onclick={attachFiles} aria-label="添加">📎</button>
    <textarea bind:value={input} onkeydown={onKey} oninput={() => { if (input.startsWith("/")) slashIdx = 0; }} onpaste={handlePaste} placeholder={needsWorkspace ? "请先选择项目目录…" : "给 nanocodex 发消息…（/ 唤出命令，Enter 发送，Shift+Enter 换行，Ctrl+V 粘贴图片）"} rows="2"></textarea>
    <button class="stop-btn" class:visible={busy} onclick={stopGeneration} disabled={!busy} title={stopping ? "再次停止" : "停止生成"} aria-label="停止生成" tabindex={busy ? 0 : -1}>■</button>
    <button onclick={send} disabled={needsWorkspace || (input.trim() === "" && attached.length === 0) || (busy && queued.length >= 2)}>{busy ? "排队" : "发送"}</button>
  </div>
</footer>
