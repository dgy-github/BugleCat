<script lang="ts">
  import { mountMotion } from "../lib/motion.js";
  import cat from "../assets/welcome-cat.webp";

  let { selectPrompt }: { selectPrompt: (prompt: string) => void } = $props();
  let motion: ReturnType<typeof mountMotion> | undefined;
  let enabled = $state(true);
  let imageMissing = $state(false);
  const suggestions = [
    { number: "01", title: "认识这个项目", description: "梳理结构，找到下一步", prompt: "请扫描当前项目，梳理架构、启动方式和需要关注的问题。" },
    { number: "02", title: "一起解决问题", description: "从线索出发，定位原因", prompt: "请帮我定位并修复一个问题：" },
    { number: "03", title: "把想法做出来", description: "从一个小功能开始", prompt: "我想在当前项目中实现一个功能：" },
  ];
  function layered(node: HTMLElement) {
    motion = mountMotion(node);
    return { destroy() { motion?.destroy(); motion = undefined; } };
  }
</script>

<section class="welcome-scene" aria-labelledby="welcome-heading">
  <div class="welcome-eyebrow"><span></span> BUGLECAT / YOUR LOCAL WORKSPACE</div>
  <div class="welcome-hero">
    <div class="welcome-copy">
      <p class="welcome-kicker">你的想法，值得动手。</p>
      <h1 id="welcome-heading">从这里，<br />做点<span>好东西。</span></h1>
      <p class="welcome-description">读懂代码，理清问题，把下一步变成看得见的进展。妙脆角猫咪陪你一起完成。</p>
      <a class="welcome-start" href="#task-input">开始一个任务 <span aria-hidden="true">↗</span></a>
    </div>
    <div class="welcome-visual-wrap">
      <!-- Direction keys operate this focusable visual; touch users have separate buttons. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="welcome-visual" use:layered data-motion data-tilt="0" data-shift="10" tabindex="0" role="group" aria-label="猫咪分层插画，可使用方向键调整，Escape 回正">
        <div class="welcome-orbit" aria-hidden="true"></div>
        <div class="welcome-art">
          {#if !imageMissing}<img src={cat} alt="抱着橙色毛线球的妙脆角猫咪" width="1024" height="1024" onerror={() => imageMissing = true} />{:else}<span class="welcome-image-fallback">BugleCat<br />陪你把想法变成现实</span>{/if}
        </div>
        <div class="welcome-label" aria-hidden="true"><span>一起，慢慢做好。</span><small>ONE TASK AT A TIME</small></div>
        <span class="welcome-coordinate" aria-hidden="true">想法 → 代码 → 作品</span>
      </div>
      <div class="welcome-motion-controls" aria-label="插画动效">
        <button aria-label="插画向左移动" onclick={() => motion?.preview(-0.7, 0)}>←</button>
        <button aria-label="插画回正" onclick={() => motion?.preview(0, 0)}>回正</button>
        <button aria-label="插画向右移动" onclick={() => motion?.preview(0.7, 0)}>→</button>
        <button class="motion-switch" aria-pressed={enabled} onclick={() => { enabled = !enabled; motion?.setEnabled(enabled); }}>{enabled ? "关闭动效" : "开启动效"}</button>
      </div>
    </div>
  </div>
  <div class="welcome-suggestions" aria-label="任务建议">
    {#each suggestions as item}
      <button class="welcome-suggestion" onclick={() => selectPrompt(item.prompt)}>
        <span class="suggestion-number">{item.number}</span>
        <span><strong>{item.title}</strong><small>{item.description}</small></span>
        <span class="suggestion-arrow" aria-hidden="true">↗</span>
      </button>
    {/each}
  </div>
</section>
