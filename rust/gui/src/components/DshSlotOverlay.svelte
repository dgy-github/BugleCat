<script lang="ts">
  import type { DshSlotController } from "../lib/dsh-slot-controller.svelte";

  let { controller }: { controller: DshSlotController } = $props();

  $effect(() => {
    controller.reconcile();
  });
</script>

{#if controller.overlay}
  <div class="modal-backdrop dsh-slot-backdrop" role="presentation" onclick={controller.close}>
    <div class="dsh-slot-overlay" role="dialog" aria-modal="true" aria-label={controller.overlay.label}>
      <div><strong>{controller.overlay.label}</strong><button class="plain" aria-label="关闭插件界面" onclick={controller.closeOverlay}>×</button></div>
      <p>{controller.overlay.description || "该界面由 DSH UI Slots 声明安全映射，未执行第三方 React 代码。"}</p>
      {#if controller.overlay.url}<a href={controller.overlay.url} target="_blank" rel="noreferrer">打开插件主页 ↗</a>{/if}
    </div>
  </div>
{/if}
