import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const cdpUrl = "http://127.0.0.1:9222";
const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri dev -- --target x86_64-pc-windows-msvc"]
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
    const nonce = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const a = `e2e-thread-a-${nonce}`;
    const b = `e2e-thread-b-${nonce}`;
    const title = `协议恢复 ${nonce}`;
    const ta = `e2e-turn-a-${nonce}`;
    const tb = `e2e-turn-b-${nonce}`;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    const existing = await call("threadList", { includeArchived: true });
    for (const metadata of existing.response.payload.data || []) {
      if (metadata.workspace === "e2e" && !metadata.archived) {
        await call("threadArchive", { threadId: metadata.id, archived: true });
      }
    }
    await call("threadCreate", { threadId: a, workspace: "e2e", title });
    await call("threadCreate", { threadId: b, workspace: "e2e", title: "B" });
    await call("turnStart", { threadId: a, turnId: ta });
    await call("turnStart", { threadId: b, turnId: tb });
    let sameThreadRejected = false;
    try {
      await call("turnStart", { threadId: a, turnId: `overlap-${nonce}` });
    } catch {
      sameThreadRejected = true;
    }
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "userMessage", id: `u-${nonce}`, text: "用户要求" } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "toolCall", id: `c-${nonce}`, name: "shell", arguments: { cmd: "secret" } } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "toolResult", id: `r-${nonce}`, callId: `c-${nonce}`, output: "SECRET_TOOL_OUTPUT", success: true } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "assistantMessage", id: `p-${nonce}`, text: "中间播报" } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "assistantMessage", id: `f-${nonce}`, text: "最终结论" } });
    await call("turnComplete", { threadId: a, turnId: ta, status: "completed", error: null, usage: { tokens: { prompt_tokens: 12, completion_tokens: 3 }, estimatedCost: 0.01, currency: "CNY" } });
    await call("turnComplete", { threadId: b, turnId: tb, status: "completed", error: null, usage: { tokens: {}, estimatedCost: null, currency: null } });
    const visible = await call("threadReadVisible", { threadId: a });
    const plugins = await call("codexPluginList");
    const marketplaces = await call("marketplaceList");
    return { sameThreadRejected, visible, plugins, marketplaces, a, b, title };
  });
  const serialized = JSON.stringify(evidence.visible);
  if (!evidence.sameThreadRejected) throw new Error("same-thread overlap was accepted");
  if (!serialized.includes("用户要求") || !serialized.includes("最终结论")) throw new Error(`visible transcript missing result: ${serialized}`);
  if (serialized.includes("SECRET_TOOL_OUTPUT") || serialized.includes("中间播报")) throw new Error(`visible transcript leaked internal output: ${serialized}`);
  if (!Array.isArray(evidence.plugins.response.payload.data)) throw new Error("plugin list did not use protocol data response");
  if (!Array.isArray(evidence.marketplaces.response.payload.data)) throw new Error("marketplace list did not use protocol data response");
  await page.reload();
  await page.getByRole("button", { name: /最近会话/ }).click();
  await page.getByText(evidence.title, { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText(evidence.title, { exact: true }).click();
  await page.getByText("最终结论", { exact: true }).waitFor({ timeout: 15_000 });
  await page.evaluate(async ({ a, b }) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    await call("threadArchive", { threadId: a, archived: true });
    await call("threadArchive", { threadId: b, archived: true });
  }, { a: evidence.a, b: evidence.b });
  console.log("protocol e2e: ok (concurrency, ownership, visible projection, history reload/open, plugin marketplace)");
} finally {
  await browser?.close();
  stopTree(child.pid);
}

async function waitForCdp() {
  const deadline = Date.now() + 120_000;
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
