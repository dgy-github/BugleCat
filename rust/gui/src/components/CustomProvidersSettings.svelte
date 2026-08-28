<script lang="ts">
  import { onMount } from "svelte";
  import { appServerRequest } from "../lib/app-server-client";

  let { onActivated = () => {} }: { onActivated?: (baseUrl: string, model: string, models: string[]) => void | Promise<void> } = $props();

  type Provider = { id: string; name: string; protocol: "openai" | "anthropic"; base_url: string; api_key_masked: string; has_api_key: boolean; models: string[]; active: boolean; selected_model: string };
  let providers = $state<Provider[]>([]);
  let editingId = $state<string | null>(null);
  let name = $state(""); let protocol = $state<"openai" | "anthropic">("openai"); let baseUrl = $state(""); let apiKey = $state(""); let modelsText = $state("");
  let busy = $state(""); let message = $state("");

  const load = async () => {
    const routes = await appServerRequest<Provider[]>({ method: "customProviderList" });
    providers = routes.filter((route) => !route.id.startsWith("preset:"));
  };
  onMount(() => { load().catch((error) => message = String(error)); });
  const reset = () => { editingId = null; name = ""; protocol = "openai"; baseUrl = ""; apiKey = ""; modelsText = ""; };
  const edit = (item: Provider) => { editingId = item.id; name = item.name; protocol = item.protocol; baseUrl = item.base_url; apiKey = ""; modelsText = item.models.join("\n"); };
  const save = async () => {
    busy = "save"; message = "";
    try { await appServerRequest({ method: "customProviderSave", params: { id: editingId, name, protocol, baseUrl, apiKey: apiKey || null, models: modelsText.split(/[\n,]/).map(v => v.trim()).filter(Boolean) } }); await load(); reset(); message = "模型商已保存"; }
    catch (error) { message = String(error); } finally { busy = ""; }
  };
  const discover = async (item: Provider) => {
    busy = `discover:${item.id}`; message = "";
    try { const models = await appServerRequest<string[]>({ method: "customProviderModelsDiscover", params: { id: item.id } }); edit(item); modelsText = models.join("\n"); await save(); message = `已获取 ${models.length} 个模型`; }
    catch (error) { message = String(error); } finally { busy = ""; }
  };
  const activate = async (item: Provider, model: string) => {
    busy = `activate:${item.id}`; message = "";
    try {
      await appServerRequest({ method: "customProviderActivate", params: { id: item.id, model } });
      await onActivated(item.base_url, model, item.models);
      await load();
      message = `模型目录验证通过，已切换到 ${item.name} / ${model}；目录可用不代表对话权限已开通`;
    }
    catch (error) { message = String(error); } finally { busy = ""; }
  };
  const probeChat = async (item: Provider) => {
    const model = item.selected_model || item.models[0];
    if (!model) { message = "请先获取或填写至少一个模型 ID"; return; }
    busy = `probe:${item.id}`; message = "";
    try {
      const result = await appServerRequest<{ requested_model: string; confirmed_model?: string | null; protocol: string }>({ method: "customProviderChatProbe", params: { id: item.id, model } });
      message = result.confirmed_model
        ? `对话接口可用：请求 ${result.requested_model}，响应 model 字段 ${result.confirmed_model}（不证明中转上游内部型号）`
        : `对话接口可用：${result.requested_model}；服务端未返回 model 字段`;
    } catch (error) { message = `对话接口测试失败：${String(error).replace(/^Error:\s*/, "")}`; }
    finally { busy = ""; }
  };
  const remove = async (item: Provider) => {
    if (!confirm(`删除模型商“${item.name}”？`)) return;
    try { await appServerRequest({ method: "customProviderDelete", params: { id: item.id } }); await load(); } catch (error) { message = String(error); }
  };
</script>

<section class="custom-providers">
  <div class="custom-provider-title"><div><strong>拓展模型商</strong><p>新增 OpenAI Compatible 或 Anthropic Messages 协议中转站。获取模型只验证目录；“测试对话”会真实请求 1 个输出 Token，可能产生极小费用。</p></div><button onclick={reset}>＋ 新增模型商</button></div>
  {#if providers.length}
    <div class="custom-provider-list">
      {#each providers as item (item.id)}
        <article class:active={item.active}>
          <div><strong>{item.name}</strong><span>{item.protocol === "openai" ? "OpenAI Compatible" : "Anthropic Messages"}</span>{#if item.active}<em>当前使用</em>{/if}</div>
          <code>{item.base_url}</code><small>{item.has_api_key ? `Token ${item.api_key_masked}` : "未配置 Token"}</small>
          <div class="custom-provider-models" role="radiogroup" aria-label={`${item.name} 模型列表`}>{#each item.models as model}<button class:selected={item.active && item.selected_model === model} aria-pressed={item.active && item.selected_model === model} onclick={() => activate(item, model)} disabled={!!busy}>{item.active && item.selected_model === model ? "✓ " : ""}{model}</button>{/each}</div>
          <div class="custom-provider-card-actions"><button onclick={() => edit(item)}>编辑</button><button onclick={() => discover(item)} disabled={!!busy}>{busy === `discover:${item.id}` ? "获取中（最多 15 秒）…" : "获取模型"}</button><button onclick={() => probeChat(item)} disabled={!!busy || !item.has_api_key || !item.models.length}>{busy === `probe:${item.id}` ? "测试中（最多 20 秒）…" : "测试对话"}</button><button class="danger" onclick={() => remove(item)}>删除</button></div>
        </article>
      {/each}
    </div>
  {/if}
  <div class="custom-provider-form">
    <h4>{editingId ? "编辑模型商" : "新增模型商"}</h4>
    <label><span>名称</span><input bind:value={name} placeholder="例如：公司 GPT 中转站" /></label>
    <label><span>协议</span><select bind:value={protocol}><option value="openai">OpenAI Compatible</option><option value="anthropic">Anthropic Messages / Claude</option></select></label>
    <label><span>Base URL</span><input bind:value={baseUrl} placeholder={protocol === "openai" ? "https://example.com/v1" : "https://example.com/v1"} /></label>
    <label><span>Token</span><input type="password" bind:value={apiKey} placeholder={editingId ? "留空保持原 Token" : "输入 Token"} autocomplete="off" /></label>
    <label><span>模型 ID（每行一个）</span><textarea bind:value={modelsText} rows="3" placeholder={protocol === "openai" ? "gpt-5.6-sol" : "claude-sonnet-4-5"}></textarea></label>
    <div class="custom-provider-actions"><button class="ok" onclick={save} disabled={busy === "save"}>{busy === "save" ? "保存中…" : "保存模型商"}</button>{#if editingId}<button onclick={reset}>取消编辑</button>{/if}</div>
  </div>
  {#if message}<p class="custom-provider-message">{message}</p>{/if}
</section>
