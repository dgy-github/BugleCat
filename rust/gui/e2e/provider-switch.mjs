import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const cdpUrl = "http://127.0.0.1:9222";
const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri -- dev"]
  : ["run", "tauri", "dev"];
const child = spawn(executable, args, {
  cwd: root,
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9222" },
  stdio: ["ignore", "pipe", "pipe"],
});
child.stdout.on("data", (chunk) => process.stdout.write(chunk));
child.stderr.on("data", (chunk) => process.stderr.write(chunk));

let browser;
try {
  await waitForCdp();
  browser = await chromium.connectOverCDP(cdpUrl);
  const page = await waitForPage(browser);
  const evidence = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    const nonce = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const threadId = `provider-switch-${nonce}`;
    const turnId = `turn-${nonce}`;
    await call("threadCreate", { threadId, workspace: "provider-switch-e2e", title: "Provider switch E2E" });
    await call("turnStart", { threadId, turnId });
    await call("itemAppend", { threadId, turnId, item: { type: "userMessage", id: `u-${nonce}`, text: "切换前消息" } });
    await call("itemAppend", { threadId, turnId, item: { type: "assistantMessage", id: `a-${nonce}`, text: "切换前回答" } });
    await call("turnComplete", { threadId, turnId, status: "completed", error: null, usage: { tokens: {}, estimatedCost: null, currency: null } });

    const before = await call("threadReadVisible", { threadId });
    const settings = await call("settingsRead");
    const providers = await call("customProviderList");
    const probeCandidate = providers.response.payload.data.find((provider) => provider.has_api_key && provider.models.includes("gpt-5.6-sol"));
    let chatProbe = null;
    let chatProbeError = null;
    if (probeCandidate) {
      try { chatProbe = await call("customProviderChatProbe", { id: probeCandidate.id, model: "gpt-5.6-sol" }); }
      catch (error) { chatProbeError = String(error).replace(/^Error:\s*/, ""); }
    }
    const active = providers.response.payload.data.find((provider) => provider.active);
    if (active) {
      await call("customProviderActivate", {
        id: active.id,
        model: active.selected_model || settings.response.payload.data.model,
      });
    } else {
      await call("runtimeModelSet", { model: settings.response.payload.data.model });
    }
    const after = await call("threadReadVisible", { threadId });
    const diagnostics = await call("harnessDiagnosticsRead");
    await call("threadArchive", { threadId, archived: true });
    return {
      before: before.response.payload.data,
      after: after.response.payload.data,
      requestedModel: settings.response.payload.data.model,
      activeModel: diagnostics.response.payload.data.provider_route.model,
      activationStatus: diagnostics.response.payload.data.provider_activation.status,
      activationError: diagnostics.response.payload.data.provider_activation.last_error,
      chatProbe: chatProbe?.response.payload.data || null,
      chatProbeError,
      usedCustomProvider: Boolean(active),
    };
  });

  if (JSON.stringify(evidence.before) !== JSON.stringify(evidence.after)) {
    throw new Error("provider/model switch changed the current thread projection");
  }
  if (evidence.activeModel !== evidence.requestedModel) {
    throw new Error("provider/model switch did not commit the requested model");
  }
  if (evidence.activationStatus !== "active" || evidence.activationError) {
    throw new Error("provider activation diagnostics did not report a safe successful activation");
  }
  if (evidence.chatProbe && evidence.chatProbe.requested_model !== "gpt-5.6-sol") {
    throw new Error("custom provider chat probe returned the wrong requested model");
  }
  if (evidence.chatProbeError && /bearer|token=|api_key=|sk-/i.test(evidence.chatProbeError)) {
    throw new Error("custom provider chat probe leaked a credential-shaped error");
  }
  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.getByRole("heading", { name: "设置", exact: true }).waitFor();
  await page.locator(".settings-nav").getByRole("button", { name: /插件/ }).click();
  await page.getByText("最近一次模型切换", { exact: true }).waitFor();
  await page.getByText(/目录验证通过，Route 已切换/).waitFor();
  console.log(JSON.stringify({
    providerSwitchE2e: true,
    transcriptPreserved: true,
    modelPreserved: true,
    activationDiagnosticsVisible: true,
    customProviderChatProbe: evidence.chatProbe ? "available" : evidence.chatProbeError ? "blocked" : "not-configured",
    customProviderConfirmedModel: evidence.chatProbe?.confirmed_model || null,
    usedCustomProvider: evidence.usedCustomProvider,
  }));
} finally {
  await browser?.close();
  stopTree(child.pid);
}

async function waitForCdp() {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    try { if ((await fetch(`${cdpUrl}/json/version`)).ok) return; } catch {}
    await delay(250);
  }
  throw new Error("WebView2 CDP endpoint did not become ready");
}

async function waitForPage(connectedBrowser) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    for (const page of connectedBrowser.contexts().flatMap((context) => context.pages())) {
      if (!page.url().includes("localhost:5179")) continue;
      try {
        await page.waitForLoadState("domcontentloaded");
        if (await page.evaluate(() => Boolean(window.__TAURI_INTERNALS__?.invoke))) return page;
      } catch {}
    }
    await delay(100);
  }
  throw new Error("Tauri WebView page was not found over CDP");
}

function stopTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  else { try { process.kill(pid, "SIGTERM"); } catch {} }
}

function delay(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }
