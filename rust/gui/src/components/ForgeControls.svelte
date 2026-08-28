<script lang="ts">
  import type { ForgeController } from "../lib/forge-controller.svelte";

  let { forge }: { forge: ForgeController } = $props();
  const active = $derived(forge.job?.status === "running" || forge.job?.status === "cancelling");
</script>

<section class="forge-card" data-testid="forge-controls">
  <div class="forge-title">
    <strong>Harness Forge</strong>
    <span class:ready={forge.runtime?.available} data-testid="forge-runtime-status">
      {forge.runtime?.available ? "运行时可用" : forge.runtime ? "运行时不可用" : "检测中"}
    </span>
  </div>
  <p class="emptyline">自动提出、评测并择优合并 Agent 改进。会产生多轮模型调用和费用，开始前必须再次确认。</p>
  {#if forge.runtime && !forge.runtime.available}<p class="forge-error">{forge.runtime.reason || "完整 Forge 运行时未安装"}</p>{/if}
  <div class="forge-grid">
    <label>训练轮数<input aria-label="Forge 训练轮数" type="number" min="1" max="5" bind:value={forge.rounds} disabled={active} /></label>
    <label>重复评测<input aria-label="Forge 重复评测" type="number" min="1" max="3" bind:value={forge.repeats} disabled={active} /></label>
    <label>单任务超时<input aria-label="Forge 单任务超时" type="number" min="30" max="300" bind:value={forge.timeoutS} disabled={active} /></label>
    <label>总时限<input aria-label="Forge 总时限" type="number" min="60" max="3600" bind:value={forge.budgetS} disabled={active} /></label>
    <label>教师<select aria-label="Forge 教师" bind:value={forge.teacher} disabled={active}><option value="panel">Panel</option><option value="codex">Codex</option><option value="claude">Claude</option><option value="api">API</option></select></label>
    <label>接受门差值<input aria-label="Forge 接受门差值" type="number" min="1" max="3" bind:value={forge.acceptMargin} disabled={active} /></label>
  </div>
  <div class="checkpoint-create">
    <button onclick={forge.start} disabled={forge.loading || active || !forge.runtime?.available}>确认后开始 Forge</button>
    {#if active}<button class="deny" onclick={forge.cancel} disabled={forge.job?.status === "cancelling"}>{forge.job?.status === "cancelling" ? "取消中…" : "取消 Forge"}</button>{/if}
    <button class="plain" onclick={forge.refresh} disabled={forge.loading}>刷新状态</button>
  </div>
  <p class="emptyline" data-testid="forge-job-status">任务状态：{forge.job?.status || "idle"}</p>
  {#if forge.job?.error}<p class="forge-error">{forge.job.error}</p>{/if}
  {#if forge.job?.summary}<div class="forge-summary">
    <strong>安全结果摘要</strong>
    <span>轮次 {forge.job.summary.rounds}，接受 {forge.job.summary.acceptedRounds}</span>
    {#if forge.job.summary.championTrain != null}<span>冠军训练分 {forge.job.summary.championTrain}</span>{/if}
    {#if forge.job.summary.championHoldout != null}<span>冠军保留分 {forge.job.summary.championHoldout}</span>{/if}
    {#if forge.job.summary.testBaseline != null && forge.job.summary.testChampion != null}<span>测试 {forge.job.summary.testBaseline} → {forge.job.summary.testChampion}</span>{/if}
    <code>{forge.job.summary.reportFile}</code>
  </div>{/if}
</section>
