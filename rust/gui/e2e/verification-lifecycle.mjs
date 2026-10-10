// Real WebView2 GUI lifecycle against a localhost fixture. No paid Jev calls.
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, mkdirSync, readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { createHash } from "node:crypto";

const repo = fileURLToPath(new URL("../../../", import.meta.url));
const out = resolve(process.env.NCX_JEV_LIFECYCLE_REPORT || join(repo, "reports/gui-lifecycle-20261010.json"));
const shotDir = resolve(out, "..");
mkdirSync(shotDir, { recursive: true });
const exe = resolve(process.env.NCX_JEV_GUI_EXE || join(repo, "rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release/ncx-gui.exe"));
const scenarios = [];
const exeSha256=createHash("sha256").update(readFileSync(exe)).digest("hex");
const wait = (ms) => new Promise((done) => setTimeout(done, ms));
const assert = (condition, text) => { if (!condition) throw new Error(text); };
function emit(response, delta, finish = null) {
  response.write(`data: ${JSON.stringify({ model: "lifecycle-fixture", choices: [{ delta, ...(finish ? { finish_reason: finish } : {}) }] })}\n\n`);
}

for (const [index, mode] of ["off", "shadow", "enforce"].entries()) {
  const root = mkdtempSync(join(tmpdir(), `ncx-lifecycle-${mode}-`));
  const workspace = join(root, "workspace");
  const profile = join(root, "profile");
  mkdirSync(workspace, { recursive: true });
  mkdirSync(join(profile, ".nanocodex"), { recursive: true });
  let phase = "write";
  let requests = 0;
  let writeRequests = 0;
  let slowStarted = false;
  let networkFaultRequests = 0;
  const sockets = new Set();
  const server = createServer(async (request, response) => {
    if (request.method === "GET") { response.end(JSON.stringify({ data: [{ id: "lifecycle-fixture" }] })); return; }
    for await (const _ of request) { /* No message or key retention. */ }
    requests += 1;
    if (phase === "disconnect") { networkFaultRequests += 1; request.socket.destroy(); return; }
    response.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
    if (phase === "write") {
      writeRequests += 1;
      emit(response, { tool_calls: [{ index: 0, id: "lifecycle-write", type: "function", function: { name: "apply_patch", arguments: JSON.stringify({ patch: "*** Begin Patch\n*** Add File: marker.txt\n+once\n*** End Patch" }) } }] });
      emit(response, {}, "tool_calls"); response.end("data: [DONE]\n\n"); phase = "slow"; return;
    }
    if (phase === "slow") { slowStarted = true; emit(response, { content: "Waiting for the slow fixture" }); return; }
    emit(response, { content: "Tests not run and result unverified" });
    emit(response, {}, "stop"); response.end("data: [DONE]\n\n");
  });
  server.on("connection", (socket) => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const base = `http://127.0.0.1:${server.address().port}/v1`;
  writeFileSync(join(profile, ".nanocodex/config.toml"), `api_key = "fixture-only"\nbase_url = "${base}"\nmodel = "lifecycle-fixture"\nprovider_protocol = "openai"\nworkspace = '${workspace}'\nmax_retries = 0\n`);
  const port = 9460 + index;
  const env = { ...process.env, HOME: profile, USERPROFILE: profile,
    NCX_JEV_MODE: mode, NCX_JEV_API_KEY: "", NANOCODEX_HARNESS_PROFILE: "minimal",
    NANOCODEX_API_KEY: "fixture-only", NANOCODEX_BASE_URL: base, NANOCODEX_MODEL: "lifecycle-fixture",
    NANOCODEX_PROVIDER_PROTOCOL: "openai", NANOCODEX_TURN_TIMEOUT_S: "10",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` };
  const gui = spawn(exe, [], { cwd: workspace, env, windowsHide: true, stdio: "ignore" });
  let browser;
  try {
    const deadline = Date.now() + 45_000;
    while (Date.now() < deadline) {
      assert(gui.exitCode === null, `GUI exit ${gui.exitCode}`);
      try { if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) break; } catch {}
      await wait(200);
    }
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    let page;
    for (let n = 0; n < 100 && !page; n += 1) {
      for (const p of browser.contexts().flatMap((c) => c.pages())) {
        try { if (await p.evaluate(() => Boolean(window.__TAURI_INTERNALS__?.invoke))) page = p; } catch {}
      }
      if (!page) await wait(100);
    }
    assert(page, "GUI IPC unavailable");
    const threadId = `jev-lifecycle-${mode}-${Date.now()}`;
    await page.evaluate(async ({ workspace, threadId }) => window.__TAURI_INTERNALS__.invoke("app_server_request", {
      request: { method: "threadCreateActivate", params: { threadId, workspace, title: "Jev lifecycle", harnessProfile: "minimal" } }
    }), { workspace, threadId });
    async function reload() {
      await page.reload();
      const close = page.getByRole("button", { name: "关闭设置", exact: true });
      await close.waitFor({ state: "visible", timeout: 2000 }).catch(() => {});
      if (await close.isVisible()) await close.click();
    }
    await reload();
    await page.getByRole("button", { name: "打开会话“Jev lifecycle”", exact: true }).click({ timeout: 15_000 });
    async function send(text) {
      await page.getByRole("textbox", { name: "任务内容" }).fill(text);
      await page.getByRole("button", { name: "发送", exact: true }).click();
    }
    await send("Create marker.txt once, then wait for the slow fixture.");
    // Workspace writes may ask for explicit approval under the active profile.
    const writeDeadline = Date.now() + 8000;
    while (!slowStarted && Date.now() < writeDeadline) {
      const allow = page.getByRole("button", { name: /允许一次|允许本次|同意|批准本次/ }).first();
      if (await allow.isVisible().catch(() => false)) await allow.click();
      await wait(100);
    }
    assert(slowStarted && existsSync(join(workspace, "marker.txt")), "write/slow stage not reached");
    assert(readFileSync(join(workspace, "marker.txt"), "utf8").trim() === "once", "unexpected write content");
    await page.getByRole("button", { name: "停止生成", exact: true }).click();
    await page.getByRole("button", { name: "停止生成", exact: true }).waitFor({ state: "hidden", timeout: 15_000 });
    assert(await page.getByText("规则核验通过", { exact: true }).count() === 0, "false verified after cancellation");
    const cancelScreenshot = join(shotDir, `gui-cancel-${mode}.png`);
    await page.screenshot({ path: cancelScreenshot });
    await reload();
    assert(await page.getByText("规则核验通过", { exact: true }).count() === 0, "false verified on restoration");
    assert(writeRequests === 1, "restoration repeated the write request");

    phase = "disconnect";
    await send("Report the network failure honestly.");
    await page.getByRole("button", { name: "停止生成", exact: true }).waitFor({ state: "hidden", timeout: 25_000 });
    assert(networkFaultRequests > 0, "disconnect fixture was not reached");
    assert(await page.getByText("规则核验通过", { exact: true }).count() === 0, "false verified after network fault");

    phase = "finish";
    const sessionsDir = join(workspace, ".nanocodex/sessions");
    if (mode !== "off") mkdirSync(join(sessionsDir, `${threadId}.verification.jsonl`), { recursive: true });
    await send("Report that tests are unverified.");
    await page.getByText("Tests not run and result unverified", { exact: true }).waitFor({ timeout: 20_000 });
    await page.getByRole("button", { name: "停止生成", exact: true }).waitFor({ state: "hidden", timeout: 15_000 });
    if (mode !== "off") await page.getByText("核验记录故障", { exact: true }).waitFor({ timeout: 10_000 });
    assert(await page.getByText("规则核验通过", { exact: true }).count() === 0, "false verified after persistence fault");
    const faultScreenshot = join(shotDir, `gui-fault-${mode}.png`);
    await page.screenshot({ path: faultScreenshot });
    await reload();
    assert(writeRequests === 1 && readFileSync(join(workspace, "marker.txt"), "utf8").trim() === "once", "duplicate write after restoration");
    assert(await page.getByText("规则核验通过", { exact: true }).count() === 0, "restored record manufactured verified");
    const sourceFile = readdirSync(sessionsDir).find((name) => name === `${threadId}.sources.jsonl`);
    const sources = sourceFile ? readFileSync(join(sessionsDir, sourceFile), "utf8").trim().split("\n").filter(Boolean).map(JSON.parse) : [];
    const writes = sources.filter((record) => record.kind === "ToolCall" && record.tool_name === "apply_patch");
    assert(writes.length === 1, "durable trace must contain exactly one write");
    scenarios.push({ mode, root, threadId, requests, writeRequests, durableWriteCalls: writes.length,
      cancelCompleted: true, networkFaultRequests, restoredWithoutPass: true,
      persistenceFaultDisplayed: mode !== "off", newJevAttempts: 0, passedLabels: 0, marker: "once",
      cancelScreenshot, faultScreenshot });
    writeFileSync(out, `${JSON.stringify({ schema: "buglecat.gui.verification-lifecycle.v1", exe, exeSha256, scenarios, paidCalls: 0 }, null, 2)}\n`);
    console.log(JSON.stringify(scenarios.at(-1)));
  } finally {
    await browser?.close();
    if (gui.pid) spawnSync("taskkill", ["/PID", String(gui.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
    for (const socket of sockets) socket.destroy();
    await new Promise((done) => server.close(done));
  }
}
