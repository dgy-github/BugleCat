import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const originalHome = process.env.USERPROFILE || process.env.HOME || "";
const profileHome = mkdtempSync(join(tmpdir(), "ncx-orchestrator-e2e-home-"));
mkdirSync(join(profileHome, ".nanocodex"), { recursive: true });
writeFileSync(join(profileHome, ".nanocodex", "config.toml"), 'api_key = "e2e-placeholder"\nbase_url = "http://127.0.0.1:9/v1"\nmodel = "e2e-model"\n');
const cdpUrl = "http://127.0.0.1:9222";
const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri -- dev"]
  : ["run", "tauri", "--", "dev"];
const child = spawn(executable, args, {
  cwd: root,
  env: {
      ...process.env,
      USERPROFILE: profileHome,
      HOME: profileHome,
      CARGO_HOME: process.env.CARGO_HOME || join(originalHome, ".cargo"),
      RUSTUP_HOME: process.env.RUSTUP_HOME || join(originalHome, ".rustup"),
    CARGO_INCREMENTAL: "0",
    CARGO_TARGET_DIR: resolve(root, "../target-codex-check/gui-orchestrator-test"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9222",
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
  await page.getByRole("button", { name: "设置" }).click({ force: true });
  await page.getByText("多 Agent 资源预算", { exact: true }).waitFor();
  const budgetBounds = [
    ["普通任务 Worker", "1", "4"],
    ["高风险任务 Worker", "1", "6"],
    ["验证重试", "0", "3"],
    ["递归深度", "0", "2"],
    ["子任务上限", "1", "12"],
  ];
  for (const [label, min, max] of budgetBounds) {
    const input = page.locator("label").filter({ hasText: label }).locator("input");
    if (await input.getAttribute("min") !== min || await input.getAttribute("max") !== max) {
      throw new Error(`${label} bounds are not enforced in settings`);
    }
  }
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await page.getByRole("button", { name: "打开项目记忆" }).click({ force: true });
  await page.getByRole("button", { name: "模型整理", exact: true }).waitFor();
  await page.getByRole("button", { name: "快速去重", exact: true }).waitFor();
  const memoryMergeIdle = await page.evaluate(async () => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "memoryMergeStatusRead" } });
    return outcome.response.payload.data;
  });
  if (memoryMergeIdle.status !== "idle") throw new Error(`memory merge job was not idle: ${JSON.stringify(memoryMergeIdle)}`);
  await page.getByTestId("forge-controls").waitFor();
  const forgeBounds = [
    ["Forge 训练轮数", "1", "5"],
    ["Forge 重复评测", "1", "3"],
    ["Forge 单任务超时", "30", "300"],
    ["Forge 总时限", "60", "3600"],
    ["Forge 接受门差值", "1", "3"],
  ];
  for (const [label, min, max] of forgeBounds) {
    const input = page.getByLabel(label);
    if (await input.getAttribute("min") !== min || await input.getAttribute("max") !== max) {
      throw new Error(`${label} bounds are not enforced`);
    }
  }
  const forgeState = await page.evaluate(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const runtime = await invoke("app_server_request", { request: { method: "forgeRuntimeStatusRead" } });
    const job = await invoke("app_server_request", { request: { method: "forgeJobStatusRead" } });
    return { runtime: runtime.response.payload.data, job: job.response.payload.data };
  });
  if (forgeState.runtime.available) {
    if (forgeState.job.status !== "idle") throw new Error(`Forge job was not idle: ${JSON.stringify(forgeState.job)}`);
    await page.getByRole("button", { name: "确认后开始 Forge" }).waitFor();
    if (!(await page.getByText(/会产生多轮模型调用和费用/).isVisible())) throw new Error("Forge cost warning is missing");
  } else {
    const message = page.locator(".forge-error");
    await message.waitFor();
    if (!(await message.textContent())?.includes("Forge")) throw new Error("Forge unavailable state is not explained");
  }
  await page.locator(".rightpanel .rp-close").click();
  const fixture = await page.evaluate(async () => {
    const nonce = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const threadId = `e2e-orchestrator-${nonce}`;
    const turnId = `turn-${nonce}`;
    const title = `多 Agent 恢复 ${nonce}`;
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const call = (method, params) => invoke("app_server_request", { request: { method, params } });
    await call("threadCreate", { threadId, workspace: "e2e", title });
    await call("turnStart", { threadId, turnId, executionMode: "orchestrator" });
    await call("itemAppend", { threadId, turnId, item: { type: "userMessage", id: `u-${nonce}`, text: "保留这条原始要求" } });
    await call("itemAppend", { threadId, turnId, item: { type: "assistantMessage", id: `a-${nonce}`, text: "取消前已形成的可恢复结论", model: "e2e-model" } });
    await call("turnComplete", { threadId, turnId, status: "cancelled", error: "cancelled by user", usage: { tokens: { prompt_tokens: 2, completion_tokens: 1 }, estimatedCost: null, currency: null } });
    return { threadId, turnId, title };
  });

  await page.getByText(fixture.title, { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText(fixture.title, { exact: true }).click();
  await page.getByText("保留这条原始要求", { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText("取消前已形成的可恢复结论", { exact: true }).waitFor({ timeout: 30_000 });
  const transcriptBefore = await page.locator(".scroll").innerText();

  await page.getByRole("button", { name: "设置" }).click({ force: true });
  const executionSelect = page.locator("label").filter({ hasText: "Agent / 编排模式" }).locator("select");
  await executionSelect.selectOption("orchestrator");
  if (await executionSelect.inputValue() !== "orchestrator") throw new Error("GUI did not switch to orchestrator mode");
  await page.getByRole("button", { name: "取消", exact: true }).click();
  const transcriptAfter = await page.locator(".scroll").innerText();
  if (transcriptAfter !== transcriptBefore) throw new Error("mode switch changed the current transcript");

  await page.evaluate(async (sessionId) => {
    const emit = (payload) => window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "ncx://event", payload });
    await emit({ kind: "orchestrator_stage", session_id: sessionId, stage: "workers", detail: "并行执行 2 个隔离 Worker" });
    await emit({ kind: "orchestrator_activity", session_id: sessionId, worker: 2, tool: "read_file", phase: "started", failure: null });
    await emit({ kind: "orchestrator_activity", session_id: sessionId, worker: 2, tool: "read_file", phase: "finished", failure: null });
    await emit({ kind: "tool_start", session_id: sessionId, name: "shell", args: "SECRET_COMMAND" });
    await emit({ kind: "tool_result", session_id: sessionId, name: "shell", result: "SECRET_RESULT" });
    await emit({ kind: "done", session_id: sessionId, final_text: "", stop_reason: "completed", usage: {} });
  }, fixture.threadId);
  await page.getByRole("button", { name: "轨迹", exact: true }).click();
  const trajectory = page.locator(".trajectory-view");
  await trajectory.getByText(/read_file · 完成/).waitFor({ timeout: 10_000 });
  await trajectory.getByText(/shell · 已完成/).waitFor({ timeout: 10_000 });
  if (!(await trajectory.innerText()).includes("W2")) throw new Error("worker identity missing from completed trajectory");
  await page.getByRole("button", { name: "对话", exact: true }).click();
  const cleanChat = await page.locator(".scroll").innerText();
  if (cleanChat.includes("SECRET_COMMAND") || cleanChat.includes("SECRET_RESULT")) throw new Error("completed tool internals leaked into chat");

  await page.reload();
  await page.getByText(fixture.title, { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText(fixture.title, { exact: true }).click();
  await page.getByText("取消前已形成的可恢复结论", { exact: true }).waitFor({ timeout: 30_000 });
  const persisted = await page.evaluate(async ({ threadId, turnId }) => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadRead", params: { threadId } } });
    const turn = outcome.response.payload.data.turns.find((item) => item.id === turnId);
    return { executionMode: turn?.executionMode, status: turn?.status };
  }, fixture);
  if (persisted.executionMode !== "orchestrator") throw new Error(`execution mode was not persisted: ${JSON.stringify(persisted)}`);
  if (persisted.status !== "cancelled") throw new Error(`cancel status was not persisted: ${JSON.stringify(persisted)}`);

  await page.evaluate(async (threadId) => {
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadArchive", params: { threadId, archived: true } } });
  }, fixture.threadId);
  console.log(JSON.stringify({
    orchestratorModeSwitch: true,
    transcriptPreserved: true,
    executionModePersisted: persisted.executionMode,
    cancelledTurnRestored: persisted.status,
    historyReloaded: true,
    completedTrajectoryRetained: true,
    chatKeptClean: true,
    resourceBudgetControlsVisible: true,
    memoryMergeControlsVisible: true,
    memoryMergeIdleStatus: memoryMergeIdle.status,
    forgeRuntimeAvailable: forgeState.runtime.available,
    forgeJobIdleStatus: forgeState.job.status,
    forgeBoundedControlsVisible: true,
    forgeCostConfirmationVisible: true,
  }, null, 2));
} finally {
  await browser?.close().catch(() => {});
  stopTree(child.pid);
}

async function waitForCdp() {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    try { if ((await fetch(`${cdpUrl}/json/version`)).ok) return; } catch { /* compiling */ }
    await delay(500);
  }
  throw new Error("Tauri WebView CDP endpoint did not become ready");
}

async function waitForPage(connectedBrowser) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const page = connectedBrowser.contexts().flatMap((context) => context.pages())
      .find((candidate) => candidate.url().includes("localhost:5179"));
    if (page) {
      try {
        await page.locator("footer textarea").waitFor({ timeout: 2_000 });
        return page;
      } catch { /* loading */ }
    }
    await delay(250);
  }
  throw new Error("BugleCat page did not become ready");
}

function stopTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], { stdio: "ignore" });
  else child.kill();
}

function delay(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }
