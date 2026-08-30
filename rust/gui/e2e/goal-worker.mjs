import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const profileHome = mkdtempSync(join(tmpdir(), "ncx-goal-e2e-home-"));
const targetDir = fileURLToPath(new URL("../src-tauri/target/", import.meta.url));
const e2eExecutable = join(targetDir, "x86_64-pc-windows-msvc", "debug", "ncx-gui.exe");
const cdpPort = 9334;
const cdpUrl = `http://127.0.0.1:${cdpPort}`;
const requestedModel = "mock-goal-model";
const confirmedModel = "mock-confirmed-model";
const backupModel = requestedModel;
const mock = { requests: [], secretSeen: false };
const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url?.endsWith("/models")) {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ data: [{ id: requestedModel }] }));
    return;
  }
  if (request.method !== "POST" || !request.url?.endsWith("/chat/completions")) {
    response.writeHead(404, { "content-type": "application/json" });
    response.end(JSON.stringify({ error: "not found" }));
    return;
  }
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  mock.secretSeen ||= request.headers.authorization !== "Bearer e2e-placeholder-key";
  mock.requests.push(body);
  const call = mock.requests.length;
  console.log(JSON.stringify({ mockGoalRequest: call, model: body.model, messageCount: body.messages?.length || 0, toolCount: body.tools?.length || 0 }));
  response.writeHead(200, {
    "content-type": "text/event-stream",
    "cache-control": "no-cache",
    connection: "close",
  });
  if (call === 1) {
    sendContent(response, "第一轮已完成一项确定性进展。", "stop");
  } else if (call === 2) {
    sendTool(response, "goal-get-e2e", "get_goal", {});
  } else if (call === 3) {
    const result = [...body.messages].reverse().find((message) => message.role === "tool" && message.tool_call_id === "goal-get-e2e");
    if (!result) throw new Error("mock did not receive get_goal result");
    const view = JSON.parse(result.content);
    sendTool(response, "goal-complete-e2e", "update_goal", {
      goal_id: view.goal.id,
      revision: view.goal.revision,
      action: "complete",
    });
  } else if (call === 4) {
    sendContent(response, "E2E 长期目标已完整结束。", "stop");
  } else {
    sendContent(response, "unexpected extra model call", "stop");
  }
  response.end("data: [DONE]\n\n");
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string") throw new Error("mock provider did not bind a TCP port");
const baseUrl = `http://127.0.0.1:${address.port}/v1`;

const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri dev -- --target x86_64-pc-windows-msvc"]
  : ["run", "tauri", "dev"];
const originalHome = process.env.USERPROFILE || process.env.HOME || "";
const child = spawn(executable, args, {
  cwd: root,
  env: {
    ...process.env,
    USERPROFILE: profileHome,
    HOME: profileHome,
    CARGO_HOME: process.env.CARGO_HOME || join(originalHome, ".cargo"),
    RUSTUP_HOME: process.env.RUSTUP_HOME || join(originalHome, ".rustup"),
    CARGO_INCREMENTAL: "0",
    CARGO_BUILD_JOBS: "2",
    CARGO_TARGET_DIR: targetDir,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    NANOCODEX_PROVIDER_PROTOCOL: "openai",
    NANOCODEX_BASE_URL: baseUrl,
    NANOCODEX_API_KEY: "e2e-placeholder-key",
    NANOCODEX_DEEPSEEK_API_KEY: "e2e-placeholder-key",
    NANOCODEX_MODEL: requestedModel,
    NANOCODEX_MODELS: requestedModel,
    NANOCODEX_MAX_RETRIES: "0",
  },
  stdio: ["ignore", "pipe", "pipe"],
});
child.stdout.on("data", (chunk) => process.stdout.write(chunk));
child.stderr.on("data", (chunk) => process.stderr.write(chunk));

let browser;
try {
  await waitForCdp();
  browser = await chromium.connectOverCDP(cdpUrl);
  const page = await waitForPage(browser);
  const evidence = await page.evaluate(async (workspace) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const nonce = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const threadId = `goal-e2e-${nonce}`;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    await call("customProviderSave", {
      id: "goal-e2e-relay",
      name: "Goal E2E Relay",
      protocol: "openai",
      baseUrl: workspace.baseUrl,
      apiKey: "e2e-placeholder-key",
      models: ["mock-goal-model"],
    });
    await call("customProviderSave", {
      id: "goal-e2e-backup",
      name: "Goal E2E Backup",
      protocol: "openai",
      baseUrl: workspace.baseUrl,
      apiKey: "e2e-placeholder-key",
      models: [workspace.backupModel],
    });
    await call("customProviderSave", {
      id: "goal-e2e-invalid",
      name: "Goal E2E Invalid",
      protocol: "openai",
      baseUrl: workspace.baseUrl,
      apiKey: "e2e-placeholder-key",
      models: ["missing-model"],
    });
    await call("customProviderActivate", { id: "goal-e2e-relay", model: "mock-goal-model" });
    await call("threadCreateActivate", { threadId, workspace: workspace.root, title: "Goal E2E", harnessProfile: "minimal" });
    const created = await call("goalCreate", { threadId, objective: "完成无网络自动续轮验证", maxGoalRounds: 4 });
    const createdGoal = created.response.payload.data.goal;
    await call("goalResume", { threadId, goal: { id: createdGoal.id, revision: createdGoal.revision } });
    const deadline = Date.now() + 30_000;
    let view;
    let thread;
    while (Date.now() < deadline) {
      view = (await call("goalRead", { threadId })).response.payload.data;
      thread = (await call("threadRead", { threadId })).response.payload.data;
      const completedGoalTurns = thread.turns.filter((turn) =>
        turn.status === "completed" && turn.items.some((item) => item.type === "goalMessage"));
      if (view?.goal?.phase === "complete" && completedGoalTurns.length === 2) break;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (view?.goal?.phase !== "complete") throw new Error(`goal did not complete: ${JSON.stringify(view)}`);
    if (!thread) throw new Error("goal thread was not readable");
    const visible = (await call("threadReadVisible", { threadId })).response.payload.data;
    return { threadId, view, thread, visible };
  }, { root, baseUrl, backupModel });
  console.log(JSON.stringify({
    durableAssistantTexts: evidence.thread.turns.flatMap((turn) => turn.items.filter((item) => item.type === "assistantMessage").map((item) => item.text)),
    visibleAssistantTexts: evidence.visible.turns.flatMap((turn) => turn.items.filter((item) => item.type === "assistantMessage").map((item) => item.text)),
  }));
  // The test drives the host protocol directly, so the Svelte controller does
  // not observe the synthetic thread activation. Reload through the normal app
  // bootstrap path before asserting what a user sees in the active transcript.
  await page.reload();
  await page.waitForLoadState("domcontentloaded");
  const goalThread = page.getByRole("button", { name: "打开会话“Goal E2E”", exact: true });
  await goalThread.waitFor({ timeout: 15_000 });
  await goalThread.click();
  const routeButton = page.locator(".model-pill").filter({ hasText: "Goal E2E Relay" });
  await routeButton.waitFor({ timeout: 10_000 });
  if (!(await routeButton.textContent())?.includes(requestedModel)) throw new Error("Composer route label omitted the active model");
  const openBackdrop = page.locator(".menu-backdrop:visible");
  if (await openBackdrop.count()) await openBackdrop.first().click({ force: true });
  await routeButton.click({ force: true });
  await page.locator(".model-route-head.active").getByText("openai", { exact: true }).waitFor({ timeout: 10_000 });
  await page.locator(".model-wrap > .menu-backdrop").click({ force: true });
  await page.getByText("第一轮已完成一项确定性进展。", { exact: true }).waitFor({ timeout: 10_000 });
  await page.getByText("E2E 长期目标已完整结束。", { exact: true }).waitFor({ timeout: 10_000 });
  await page.getByText(`请求 ${requestedModel} → 响应字段 ${confirmedModel}`, { exact: true }).last().waitFor({ timeout: 10_000 });

  if (!(await page.locator(".model-menu:visible").count())) await routeButton.click({ force: true });
  await page.locator(".model-menu:visible").waitFor({ timeout: 10_000 });
  if (await page.locator(".model-route-head").count() !== 1) throw new Error("unselected providers leaked into the Composer model menu");
  console.log(JSON.stringify({
    composerRouteGroups: await page.locator(".model-route-head").allTextContents(),
    composerRouteOptions: await page.locator(".model-opt").evaluateAll((items) => items.map((item) => ({ provider: item.getAttribute("data-provider"), model: item.getAttribute("data-model"), text: item.textContent }))),
  }));
  await page.locator(".model-wrap > .menu-backdrop").click({ force: true });
  await page.evaluate(async (model) => {
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "customProviderActivate", params: { id: "goal-e2e-backup", model } } });
  }, backupModel);
  const backupRouteButton = page.locator(".model-pill").filter({ hasText: new RegExp(`Goal E2E Backup.*${backupModel}`) });
  await backupRouteButton.waitFor({ timeout: 10_000 });
  await page.getByText("第一轮已完成一项确定性进展。", { exact: true }).waitFor({ timeout: 10_000 });
  await page.getByText("E2E 长期目标已完整结束。", { exact: true }).waitFor({ timeout: 10_000 });
  const switchedRoute = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const diagnostics = await invoke("app_server_request", { request: { method: "harnessDiagnosticsRead" } });
    const settings = await invoke("app_server_request", { request: { method: "settingsRead" } });
    return { route: diagnostics.response.payload.data.provider_route, priceIn: settings.response.payload.data.price_in, priceOut: settings.response.payload.data.price_out };
  });
  if (switchedRoute.route.active_provider_id !== "goal-e2e-backup" || switchedRoute.route.model !== backupModel) {
    throw new Error(`cross-provider Route did not commit: ${JSON.stringify(switchedRoute)}`);
  }
  if (switchedRoute.priceIn !== 0 || switchedRoute.priceOut !== 0) throw new Error(`custom Route inherited stale pricing: ${JSON.stringify(switchedRoute)}`);
  await page.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "customProviderActivate", params: { id: "goal-e2e-invalid", model: "missing-model" } } }); }
    catch { /* expected */ }
  });
  const routeAfterFailure = await page.evaluate(async () => {
    const result = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "harnessDiagnosticsRead" } });
    return result.response.payload.data.provider_route;
  });
  if (routeAfterFailure.active_provider_id !== "goal-e2e-backup" || routeAfterFailure.model !== backupModel) {
    throw new Error(`failed candidate changed the active Route: ${JSON.stringify(routeAfterFailure)}`);
  }

  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.getByRole("heading", { name: "设置", exact: true }).waitFor();
  await page.locator(".settings-nav").getByRole("button", { name: /模型与费用/ }).click();
  const deepseekCard = page.locator(".catalog-provider").filter({ has: page.getByRole("heading", { name: "DeepSeek", exact: true }) });
  await deepseekCard.locator(".catalog-credential input").fill("e2e-preset-placeholder-key");
  await deepseekCard.getByRole("button", { name: "保存 Token" }).click();
  await page.getByText("DeepSeek Token 已独立保存，不会与其他模型商共用", { exact: true }).waitFor({ timeout: 10_000 });
  await deepseekCard.getByText(/独立 Token \*\*\*\*-key/).waitFor({ timeout: 10_000 });
  await page.getByRole("button", { name: "关闭设置" }).click();

  const presetEvidence = await page.evaluate(async (threadId) => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    const before = await call("threadReadVisible", { threadId });
    await call("modelPresetApply", { providerId: "deepseek", modelId: "deepseek-v4-flash" });
    const committed = await call("harnessDiagnosticsRead");
    const committedRoutes = await call("customProviderList");
    let failed = false;
    try { await call("modelPresetApply", { providerId: "yunmo", modelId: "gpt-5.6-sol" }); }
    catch { failed = true; }
    const afterFailure = await call("harnessDiagnosticsRead");
    const routesAfterFailure = await call("customProviderList");
    const after = await call("threadReadVisible", { threadId });
    return {
      failed,
      before: before.response.payload.data,
      after: after.response.payload.data,
      committed: committed.response.payload.data.provider_route,
      committedRoute: committedRoutes.response.payload.data.find((route) => route.id === "preset:deepseek"),
      afterFailure: afterFailure.response.payload.data.provider_route,
      routeAfterFailure: routesAfterFailure.response.payload.data.find((route) => route.id === "preset:deepseek"),
    };
  }, evidence.threadId);
  if (presetEvidence.committed.active_provider_id !== "preset:deepseek" || presetEvidence.committedRoute?.selected_model !== "deepseek-v4-flash" || !presetEvidence.committedRoute?.active) {
    throw new Error(`preset Route did not commit atomically: ${JSON.stringify(presetEvidence)}`);
  }
  if (!presetEvidence.failed || JSON.stringify(presetEvidence.routeAfterFailure) !== JSON.stringify(presetEvidence.committedRoute)) {
    throw new Error(`failed preset switch did not roll back: ${JSON.stringify(presetEvidence)}`);
  }
  if (JSON.stringify(presetEvidence.before) !== JSON.stringify(presetEvidence.after)) {
    throw new Error("preset switch changed the current transcript");
  }

  const goalTurns = evidence.thread.turns.filter((turn) => turn.items.some((item) => item.type === "goalMessage"));
  const assistantItems = evidence.visible.turns.flatMap((turn) => turn.items.filter((item) => item.type === "assistantMessage"));
  if (evidence.view.goal.roundsStarted !== 2) throw new Error(`expected two admitted rounds, got ${evidence.view.goal.roundsStarted}`);
  if (evidence.view.activation !== "disarmed") throw new Error("completed goal remained armed");
  if (goalTurns.length !== 2 || goalTurns.some((turn) => turn.status !== "completed")) throw new Error("goal turns were not durably completed");
  if (JSON.stringify(evidence.visible).includes("goalMessage") || JSON.stringify(evidence.visible).includes("Continue the persisted objective")) {
    throw new Error("hidden GoalMessage leaked into visible history");
  }
  if (mock.requests.length !== 4) throw new Error(`expected four mock model requests, got ${mock.requests.length}`);
  if (mock.secretSeen) throw new Error("mock provider received an unexpected credential");
  if (mock.requests.some((request) => request.model !== requestedModel)) throw new Error("goal worker did not use the isolated model route");
  if (assistantItems.some((item) => item.model !== requestedModel || item.confirmedModel !== confirmedModel)) {
    throw new Error(`model route metadata was not preserved: ${JSON.stringify(assistantItems)}`);
  }
  await page.evaluate(async (threadId) => {
    await window.__TAURI_INTERNALS__.invoke("app_server_request", {
      request: { method: "threadArchive", params: { threadId, archived: true } },
    });
  }, evidence.threadId);
  console.log(JSON.stringify({
    goalWorkerE2e: true,
    noExternalProvider: true,
    admittedRounds: evidence.view.goal.roundsStarted,
    durableTurns: goalTurns.length,
    hiddenPromptStayedHidden: true,
    exactGoalToolCompletion: true,
    requestedAndConfirmedModelsVisibleAfterResume: true,
    composerProviderProtocolModelVisible: true,
    composerCrossProviderSwitchPreservedTranscript: true,
    failedProviderSwitchKeptRoute: true,
    unselectedProvidersHiddenFromComposer: true,
    presetTokenEntryInteractive: true,
    legacyPresetCredentialMigratedToRoute: true,
    presetSwitchFailureRolledBack: true,
    presetSwitchPreservedCurrentTranscript: true,
    customProviderPricingResetToUnknown: true,
    modelRequests: mock.requests.length,
  }));
} catch (error) {
  console.error(JSON.stringify({ goalWorkerE2eFailure: true, modelRequests: mock.requests.length, requestModels: mock.requests.map((request) => request.model) }));
  throw error;
} finally {
  await browser?.close();
  stopTree(child.pid);
  stopExactE2eApp();
  await new Promise((resolve) => server.close(resolve));
  rmSync(profileHome, { recursive: true, force: true });
}

function sendContent(response, content, finishReason) {
  response.write(`data: ${JSON.stringify({ model: confirmedModel, choices: [{ delta: { content } }] })}\n\n`);
  response.write(`data: ${JSON.stringify({ model: confirmedModel, choices: [{ delta: {}, finish_reason: finishReason }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } })}\n\n`);
}

function sendTool(response, id, name, args) {
  response.write(`data: ${JSON.stringify({ model: confirmedModel, choices: [{ delta: { tool_calls: [{ index: 0, id, function: { name, arguments: JSON.stringify(args) } }] }, finish_reason: "tool_calls" }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } })}\n\n`);
}

async function waitForCdp() {
  const deadline = Date.now() + 600_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Tauri dev exited before WebView startup: ${child.exitCode}`);
    try { if ((await fetch(`${cdpUrl}/json/version`)).ok) return; } catch {}
    await delay(250);
  }
  throw new Error("WebView2 CDP endpoint did not become ready");
}

async function waitForPage(connectedBrowser) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    for (const page of connectedBrowser.contexts().flatMap((context) => context.pages())) {
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

function stopExactE2eApp() {
  if (process.platform !== "win32") return;
  spawnSync("powershell", ["-NoProfile", "-Command", "$target=$env:NCX_E2E_EXE; Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'ncx-gui.exe' -and $_.ExecutablePath -eq $target } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }"], {
    env: { ...process.env, NCX_E2E_EXE: e2eExecutable },
    stdio: "ignore",
  });
}

function delay(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }
