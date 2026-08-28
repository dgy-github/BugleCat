import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
const slotFixture = mkdtempSync(join(tmpdir(), "ncx-dsh-ui-slot-"));
const localArtifactPath = join(slotFixture, "generated.png");
writeFileSync(localArtifactPath, "image");
const slotPluginName = `e2e-dsh-ui-${Date.now()}`;
mkdirSync(join(slotFixture, ".codex-plugin"), { recursive: true });
mkdirSync(join(slotFixture, ".ncx"), { recursive: true });
writeFileSync(join(slotFixture, ".codex-plugin", "plugin.json"), JSON.stringify({ name: slotPluginName, version: "1.0.0", interface: { dshUiSlots: "./.ncx/ui-slots.json" } }));
writeFileSync(join(slotFixture, ".ncx", "ui-slots.json"), JSON.stringify([
  { slot: "settings.plugins.tab", id: "e2e-settings", label: "E2E DSH 界面", order: 10, description: "声明式设置入口" },
  { slot: "sidebar.footer.action", id: "e2e-sidebar", label: "E2E DSH 界面", order: 10, description: "打开声明式界面" },
  { slot: "shell.overlay", id: "e2e-overlay", label: "E2E DSH 界面", order: 10, description: "声明式 Overlay" },
]));
child.stdout.on("data", (chunk) => process.stdout.write(chunk));
child.stderr.on("data", (chunk) => process.stderr.write(chunk));

let browser;
try {
  await waitForCdp();
  browser = await chromium.connectOverCDP(cdpUrl);
  const page = await waitForPage(browser);
  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.getByRole("heading", { name: "设置", exact: true }).waitFor();
  await page.getByRole("button", { name: "深色", exact: true }).click();
  if (await page.locator("html").getAttribute("data-theme") !== "dark") throw new Error("dark theme was not applied");
  if (await page.evaluate(() => localStorage.getItem("nanocodex.theme")) !== "dark") throw new Error("theme choice was not persisted");
  await page.getByRole("button", { name: "跟随系统", exact: true }).click();
  await page.locator(".settings-nav").getByRole("button", { name: /插件/ }).click();
  await page.getByText("OpenAI Codex 资源插件", { exact: true }).waitFor();
  await page.getByText("插件 Marketplace", { exact: true }).waitFor();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  const evidence = await page.evaluate(async (localArtifactPath) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const nonce = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const a = `e2e-thread-a-${nonce}`;
    const b = `e2e-thread-b-${nonce}`;
    const profileThread = `e2e-profile-${nonce}`;
    const profileFork = `e2e-profile-fork-${nonce}`;
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
    await call("threadCreate", { threadId: profileThread, workspace: "e2e", title: "Profile", harnessProfile: "full" });
    await call("threadHarnessProfileSet", { threadId: profileThread, harnessProfile: "coding" });
    const profileBeforeTurn = await call("threadRead", { threadId: profileThread });
    await call("turnStart", { threadId: profileThread, turnId: `profile-turn-${nonce}` });
    let profileLocked = false;
    try {
      await call("threadHarnessProfileSet", { threadId: profileThread, harnessProfile: "minimal" });
    } catch {
      profileLocked = true;
    }
    await call("turnComplete", { threadId: profileThread, turnId: `profile-turn-${nonce}`, status: "completed", error: null, usage: { tokens: {}, estimatedCost: null, currency: null } });
    const profileForked = await call("threadFork", { threadId: profileThread, newThreadId: profileFork });
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
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "artifact", id: `artifact-${nonce}`, kind: "image", name: "生成图片 1", url: "https://example.com/generated-image.png" } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "assistantMessage", id: `p-${nonce}`, text: "中间播报" } });
    await call("itemAppend", { threadId: a, turnId: ta, item: { type: "assistantMessage", id: `f-${nonce}`, text: `最终结论\n\n图片路径：\`${localArtifactPath}\`` } });
    await call("turnComplete", { threadId: a, turnId: ta, status: "completed", error: null, usage: { tokens: { prompt_tokens: 12, completion_tokens: 3 }, estimatedCost: 0.01, currency: "CNY" } });
    await call("turnComplete", { threadId: b, turnId: tb, status: "completed", error: null, usage: { tokens: {}, estimatedCost: null, currency: null } });
    const visible = await call("threadReadVisible", { threadId: a });
    const plugins = await call("codexPluginList");
    const marketplaces = await call("marketplaceList");
    const diagnostics = await call("harnessDiagnosticsRead");
    const externalPlugins = await call("externalPluginList");
    const memory = await call("memoryList");
    const settings = await call("settingsRead");
    const modelCatalog = await call("modelCatalogRead");
    const providers = await call("customProviderList");
    const activeProvider = providers.response.payload.data.find((provider) => provider.active);
    if (activeProvider) {
      await call("customProviderActivate", { id: activeProvider.id, model: activeProvider.selected_model || settings.response.payload.data.model });
    } else {
      await call("runtimeModelSet", { model: settings.response.payload.data.model });
    }
    const visibleAfterModelSwitch = await call("threadReadVisible", { threadId: a });
    const diagnosticsAfterModelSwitch = await call("harnessDiagnosticsRead");
    const dshMarket = await call("dshMarketplaceSearch", { source: "dshfind", manifestUrl: null, query: "" });
    const dsh1024Market = await call("dshMarketplaceSearch", { source: "dsh-1024store", manifestUrl: null, query: "" });
    const dshCandidate = dshMarket.response.payload.data.items.find((item) => item.package && item.version);
    const dshPreview = dshCandidate ? await call("dshMarketplacePreview", { item: dshCandidate }) : null;
    return { sameThreadRejected, profileBeforeTurn, profileLocked, profileForked, visible, visibleAfterModelSwitch, plugins, marketplaces, diagnostics, diagnosticsAfterModelSwitch, externalPlugins, memory, settings, modelCatalog, providers, dshMarket, dsh1024Market, dshPreview, a, b, title };
  }, localArtifactPath);
  const serialized = JSON.stringify(evidence.visible);
  if (!evidence.sameThreadRejected) throw new Error("same-thread overlap was accepted");
  if (evidence.profileBeforeTurn.response.payload.data.metadata.harnessProfile !== "coding") throw new Error("empty thread profile change was not persisted");
  if (!evidence.profileLocked) throw new Error("profile change was accepted after the first turn");
  if (evidence.profileForked.response.payload.data.metadata.harnessProfile !== "coding") throw new Error("fork did not inherit the source profile");
  if (!serialized.includes("用户要求") || !serialized.includes("最终结论")) throw new Error(`visible transcript missing result: ${serialized}`);
  if (JSON.stringify(evidence.visibleAfterModelSwitch.response.payload.data) !== JSON.stringify(evidence.visible.response.payload.data)) throw new Error("model/provider switch replaced or changed the current transcript");
  if (evidence.diagnosticsAfterModelSwitch.response.payload.data.provider_route.model !== evidence.settings.response.payload.data.model) throw new Error("model/provider switch did not keep the selected runtime model");
  if (!serialized.includes("generated-image.png") || !serialized.includes("生成图片 1")) throw new Error(`visible transcript missing artifact: ${serialized}`);
  if (serialized.includes("SECRET_TOOL_OUTPUT") || serialized.includes("中间播报")) throw new Error(`visible transcript leaked internal output: ${serialized}`);
  if (!Array.isArray(evidence.plugins.response.payload.data)) throw new Error("plugin list did not use protocol data response");
  if (!Array.isArray(evidence.marketplaces.response.payload.data)) throw new Error("marketplace list did not use protocol data response");
  if (evidence.diagnostics.response.payload.data.llm !== true) throw new Error("harness diagnostics did not use protocol response");
  if (!Array.isArray(evidence.externalPlugins.response.payload.data)) throw new Error("external plugin list did not use protocol response");
  if (!Array.isArray(evidence.memory.response.payload.data)) throw new Error("memory list did not use protocol response");
  if (!evidence.settings.response.payload.data.model) throw new Error("settings did not use protocol response");
  if (!Array.isArray(evidence.settings.response.payload.data.available_models) || evidence.settings.response.payload.data.available_models.length === 0) throw new Error("live provider model discovery returned no models");
  if (!evidence.settings.response.payload.data.available_models.includes(evidence.settings.response.payload.data.model)) throw new Error("live provider model discovery omitted the selected model");
  if (!Array.isArray(evidence.modelCatalog.response.payload.data.providers)) throw new Error("model catalog did not use protocol response");
  if (!Array.isArray(evidence.dshMarket.response.payload.data.items) || evidence.dshMarket.response.payload.data.items.length === 0) throw new Error("DSH marketplace search returned no items");
  const dsh1024Categories = evidence.dsh1024Market.response.payload.data.categories;
  if (!Array.isArray(dsh1024Categories) || dsh1024Categories.length !== 12) throw new Error(`1024 Store category count mismatch: ${JSON.stringify(dsh1024Categories)}`);
  if (!dsh1024Categories.some((category) => category.id === "memory" && category.zh === "记忆")) throw new Error("1024 Store memory category missing");
  if (!evidence.dshPreview?.response.payload.data.compatibility) throw new Error("DSH marketplace preview did not verify a package");
  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.locator(".settings-nav").getByRole("button", { name: /插件/ }).click();
  await page.getByLabel("DSH 市场源").selectOption("dsh-1024store");
  await page.getByRole("button", { name: "搜索", exact: true }).click();
  const memoryCategory = page.locator(".dsh-category-strip").getByRole("button", { name: /记忆/ });
  await memoryCategory.waitFor({ timeout: 30_000 }).catch(async (error) => {
    const marketError = await page.locator(".dsh-market-error").textContent().catch(() => "");
    throw new Error(`分类导航未出现：${marketError || error.message}`);
  });
  await memoryCategory.click();
  const memoryItems = page.locator('.dsh-market-item[data-category="memory"]');
  if (await memoryItems.count() === 0) throw new Error("memory category showed no plugins");
  if (await page.locator('.dsh-market-item:not([data-category="memory"])').count() !== 0) throw new Error("memory category leaked other plugins");
  await page.locator(".dsh-category-strip").getByRole("button", { name: "全部", exact: true }).click();
  if (await page.locator(".dsh-market-item").count() <= await memoryItems.count()) throw new Error("all category did not restore plugins");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  const installedSlotPlugin = await page.evaluate(async ({ source, name }) => {
    const before = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "codexPluginList" } });
    for (const plugin of before.response.payload.data) {
      if (plugin.manifest.name.startsWith("e2e-dsh-ui-")) {
        await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "codexPluginUninstall", params: { name: plugin.manifest.name } } });
      }
    }
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "codexPluginInstall", params: { source, upgrade: false } } });
    const listed = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "codexPluginList" } });
    return listed.response.payload.data.find((plugin) => plugin.manifest.name === name);
  }, { source: slotFixture, name: slotPluginName });
  if (installedSlotPlugin?.ui_slots?.length !== 3) throw new Error(`declarative UI slots were not loaded: ${JSON.stringify(installedSlotPlugin)}`);
  await page.evaluate(async () => {
    const threadId = `e2e-full-ui-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadCreateActivate", params: { threadId, workspace: "e2e", title: "E2E full UI", harnessProfile: "full" } } });
  });
  await page.reload();
  const dshSlotAction = page.getByRole("button", { name: /E2E DSH 界面/ }).first();
  await dshSlotAction.waitFor({ timeout: 30_000 });
  await dshSlotAction.click();
  await page.getByRole("dialog", { name: "E2E DSH 界面" }).waitFor();
  await page.getByRole("button", { name: "关闭插件界面" }).click();
  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.locator(".settings-nav").getByRole("button", { name: /插件/ }).click();
  await page.locator(".dsh-settings-slots").getByText("E2E DSH 界面", { exact: true }).waitFor();
  const installedEntry = page.locator(".config-entry").filter({ hasText: slotPluginName });
  await installedEntry.getByRole("button", { name: "停用" }).click();
  await page.locator(".side-plugin-actions").getByText("E2E DSH 界面", { exact: false }).waitFor({ state: "detached" });
  await installedEntry.getByRole("button", { name: "启用" }).click();
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: /E2E DSH 界面/ }).waitFor();
  await page.locator('.project-toggle[title="e2e"]').waitFor({ timeout: 15_000 });
  await page.getByText(evidence.title, { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText(evidence.title, { exact: true }).click();
  await page.getByText(/最终结论/, { exact: false }).waitFor({ timeout: 15_000 });
  const localArtifactCard = page.locator(".local-artifact-card").filter({ hasText: "generated.png" });
  await localArtifactCard.waitFor({ state: "attached", timeout: 15_000 });
  if (!(await localArtifactCard.textContent())?.includes(localArtifactPath)) throw new Error("local artifact path was not clickable after reload");
  await localArtifactCard.locator("img").waitFor({ state: "attached", timeout: 15_000 });
  if (!(await localArtifactCard.locator("img").getAttribute("src"))?.startsWith("data:image/png;base64,")) throw new Error("local image was not rendered inline");
  const artifactCard = page.locator(".artifact-card").filter({ hasText: "生成图片 1" });
  await artifactCard.waitFor({ state: "attached", timeout: 15_000 });
  if (!(await artifactCard.textContent())?.includes("generated-image.png")) throw new Error("artifact link was not restored after reload");
  const renamedTitle = `重命名 ${evidence.title.slice(-12)}`;
  const sessionRow = page.locator(".recent-item").filter({ hasText: evidence.title });
  await sessionRow.getByRole("button", { name: `会话“${evidence.title}”的操作` }).click();
  await page.getByRole("menuitem", { name: "重命名" }).click();
  await page.getByLabel("会话名称").fill(`  ${renamedTitle}  `);
  await page.getByRole("button", { name: "重命名", exact: true }).click();
  await page.locator(".recent-title").getByText(renamedTitle, { exact: true }).waitFor({ timeout: 15_000 });
  const renamed = await page.evaluate(async (threadId) => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadRead", params: { threadId } } });
    return outcome.response.payload.data.metadata.title;
  }, evidence.a);
  if (renamed !== renamedTitle) throw new Error(`renamed title was not persisted: ${renamed}`);
  await page.evaluate(async ({ a, b }) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    await call("threadArchive", { threadId: a, archived: true });
    await call("threadArchive", { threadId: b, archived: true });
  }, { a: evidence.a, b: evidence.b });
  await page.evaluate(async (name) => {
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "codexPluginUninstall", params: { name } } });
  }, slotPluginName);
  const threadsBeforeUiProfile = await page.evaluate(async () => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadList", params: { includeArchived: true } } });
    return outcome.response.payload.data.map((thread) => thread.id);
  });
  await page.getByRole("button", { name: "新会话", exact: true }).click();
  const uiProfileThread = await page.waitForFunction(async (knownIds) => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadList", params: { includeArchived: true } } });
    return outcome.response.payload.data.find((thread) => !knownIds.includes(thread.id)) || null;
  }, threadsBeforeUiProfile).then((handle) => handle.jsonValue());
  const profileButton = page.locator(".profile-pill");
  await profileButton.waitFor({ state: "visible" });
  await page.waitForFunction(() => !document.querySelector(".profile-pill")?.disabled);
  await profileButton.click();
  await page.locator(".profile-menu .model-opt").filter({ hasText: "编程" }).click();
  await page.getByRole("button", { name: /Harness：编程/ }).waitFor();
  const uiProfilePersisted = await page.evaluate(async (threadId) => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadRead", params: { threadId } } });
    return outcome.response.payload.data.metadata.harnessProfile;
  }, uiProfileThread.id);
  if (uiProfilePersisted !== "coding") throw new Error(`GUI profile selection was not persisted: ${uiProfilePersisted}`);
  console.log("protocol e2e: ok (concurrency, ownership, visible projection, artifact reload, history rename/reload/open, runtime settings, memory, plugins)");
} finally {
  await browser?.close();
  stopTree(child.pid);
  rmSync(slotFixture, { recursive: true, force: true });
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
