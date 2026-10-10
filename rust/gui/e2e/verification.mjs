// Runs built binaries with a localhost main-model fixture. --live sends exactly
// one synthetic GUI candidate to Jev. Credentials stay in child environments.
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const repo = fileURLToPath(new URL("../../../", import.meta.url));
const restoreOnly = process.env.NCX_JEV_RESTORE_ROOT;
const report = process.env.NCX_JEV_SMOKE_REPORT || join(repo, `reports/${restoreOnly ? "restore_smoke" : "runtime_smoke"}_20261010_r1r2.json`);
const shotDir=dirname(report);
mkdirSync(shotDir,{recursive:true});
const root = restoreOnly || mkdtempSync(join(tmpdir(), "ncx-jev-smoke-"));
const taskRoot = join(root, "workspace");
const profile = join(root, "profile");
mkdirSync(taskRoot, { recursive: true });
mkdirSync(join(profile, ".nanocodex"), { recursive: true });
const guiExe = resolve(process.env.NCX_JEV_GUI_EXE || join(repo, "rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release/ncx-gui.exe"));
const cliExe = resolve(process.env.NCX_JEV_CLI_EXE || join(repo, "rust/target/release/ncx.exe"));
const live = !restoreOnly && process.argv.includes("--live");
if (live && !process.env.NCX_JEV_API_KEY) throw new Error("live key missing");
let requests = 0;
const server = createServer(async (request, response) => {
  if (request.method === "GET") {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ data: [{ id: "jev-smoke-model" }] }));
    return;
  }
  for await (const _ of request) { /* No user content is printed or retained. */ }
  requests += 1;
  response.writeHead(200, { "content-type": "text/event-stream" });
  const content = live && requests > 1 ? "The requested feature is completed" : "Tests not run and result unverified";
  response.write(`data: ${JSON.stringify({ model: "jev-smoke-model", choices: [{ delta: { content } }] })}\n\n`);
  response.end(`data: ${JSON.stringify({ model: "jev-smoke-model", choices: [{ delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } })}\n\ndata: [DONE]\n\n`);
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const base = `http://127.0.0.1:${server.address().port}/v1`;
writeFileSync(join(profile, ".nanocodex/config.toml"), `api_key = "fixture-only"\nbase_url = "${base}"\nmodel = "jev-smoke-model"\nprovider_protocol = "openai"\nworkspace = '${taskRoot}'\n`);
const env = { ...process.env, USERPROFILE: profile, HOME: profile,
  NCX_JEV_MODE: "shadow", NCX_JEV_API_KEY: "", NANOCODEX_HARNESS_PROFILE: "minimal",
  NANOCODEX_API_KEY: "fixture-only", NANOCODEX_BASE_URL: base, NANOCODEX_MODEL: "jev-smoke-model",
  NANOCODEX_PROVIDER_PROTOCOL: "openai", NANOCODEX_TURN_TIMEOUT_S: "30" };
let cliExit = null;
if (!restoreOnly) {
const cli = spawn(cliExe, ["-w", taskRoot, "-s", "read-only", "-a", "never", "--max-iterations", "4", "Report that tests have not been run"], { cwd: taskRoot, env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
let cliOut = "";
cli.stdout.on("data", (data) => { cliOut += data.toString(); });
cli.stderr.on("data", () => {});
cliExit = await new Promise((done, reject) => { cli.once("error", reject); cli.once("exit", done); });
if (cliExit !== 0 || !cliOut.includes("result unverified")) throw new Error("CLI smoke failed");
}
const port = 9447;
const guiCandidate=live?"The requested feature is completed":"Tests not run and result unverified";
const gui = spawn(guiExe, [], { cwd: taskRoot, env: { ...env, NCX_JEV_API_KEY: live ? process.env.NCX_JEV_API_KEY : "", WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` }, windowsHide: true, stdio: "ignore" });
let browser;
try {
  const endpoint = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    if (gui.exitCode !== null) throw new Error(`GUI exited: ${gui.exitCode}`);
    try { if ((await fetch(`${endpoint}/json/version`)).ok) break; } catch {}
    await new Promise((done) => setTimeout(done, 200));
  }
  browser = await chromium.connectOverCDP(endpoint);
  let page;
  for (let attempt = 0; attempt < 100 && !page; attempt += 1) {
    for (const candidate of browser.contexts().flatMap((context) => context.pages())) {
      try { if (await candidate.evaluate(() => Boolean(window.__TAURI_INTERNALS__?.invoke))) page = candidate; } catch {}
    }
    if (!page) await new Promise((done) => setTimeout(done, 100));
  }
  if (!page) throw new Error("GUI IPC not ready");
  let threadId = `jev-smoke-${Date.now()}`;
  if (restoreOnly) {
    const history = readdirSync(join(taskRoot, ".nanocodex/sessions")).filter((name) => name.startsWith("jev-smoke-") && name.endsWith("verification.jsonl"));
    threadId = JSON.parse(readFileSync(join(taskRoot, ".nanocodex/sessions", history[0]), "utf8").trim().split("\n")[0]).session_id;
  }
  if (!restoreOnly) {
  await page.evaluate(async ({ taskRoot, threadId }) => {
    await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method: "threadCreateActivate", params: { threadId, workspace: taskRoot, title: "Jev smoke", harnessProfile: "minimal" } } });
  }, { taskRoot, threadId });
  }
  // Bind the normal sidebar/controller after direct protocol setup.
  await page.reload();
  const closeSettings = page.getByRole("button", { name: "关闭设置", exact: true });
  await closeSettings.waitFor({ state: "visible", timeout: 3000 }).catch(() => {});
  if (await closeSettings.isVisible()) { await closeSettings.click(); await closeSettings.waitFor({ state: "hidden" }); }
  await page.screenshot({ path: join(shotDir, "jev_gui_setup_20261010_r1r2.png") });
  if (!restoreOnly) {
  await page.getByRole("button", { name: "打开会话“Jev smoke”", exact: true }).click({ timeout: 15_000 });
  await page.getByRole("textbox", { name: "任务内容" }).fill("Report that tests have not been run");
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await page.getByText(guiCandidate, { exact: true }).waitFor({ timeout: 20_000 });
  await page.getByText("未验证", { exact: true }).waitFor({ timeout: 20_000 });
  await page.screenshot({ path: join(shotDir, "jev_gui_20261010_r1r2.png") });
  }
  await page.reload();
  await closeSettings.waitFor({ state: "visible", timeout: 3000 }).catch(() => {});
  if (await closeSettings.isVisible()) { await closeSettings.click(); await closeSettings.waitFor({ state: "hidden" }); }
  await page.getByText(guiCandidate, { exact: true }).waitFor({ timeout: 15_000 });
  await page.getByText("有历史核验记录 · 当前未验证", { exact: true }).waitFor({ timeout: 15_000 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(shotDir, "jev_gui_restored_20261010_r1r2.png") });
  // protocol threadReadVisible carries no verification label. The backend
  // activation emits the independent diagnostic history afterward.
  const logs = readdirSync(join(taskRoot, ".nanocodex/sessions")).filter((name) => name.endsWith("verification.jsonl"));
  const records = logs.flatMap((name) => readFileSync(join(taskRoot, ".nanocodex/sessions", name), "utf8").trim().split("\n").filter(Boolean).map(JSON.parse));
  const guiRecord = records.find((r) => r.session_id === threadId);
  if (!guiRecord || guiRecord.result.verdict === "pass") throw new Error("GUI record missing or falsely verified");
  if (live && guiRecord.result.call_status !== "ok") throw new Error(`Jev runtime status ${guiRecord.result.call_status}`);
  const evidence = { schema: "buglecat.verification.runtime-smoke.v2", root, cliExe, guiExe,
    cliExit, guiTranscriptRestored: true, unverifiedLabelVisible: true,
    mainModel: "localhost deterministic fixture", mainRequests: requests,
    newLiveJevCalls: restoreOnly ? 0 : guiRecord.result.observations.length,
    restoredHistoricalRecord: Boolean(restoreOnly), liveJevCalls: guiRecord.result.observations.length, model: guiRecord.result.observations[0]?.response.model || null,
    callStatus: guiRecord.result.call_status, verdict: guiRecord.result.verdict,
    screenshot: join(shotDir,"jev_gui_20261010_r1r2.png"), logs,
    restoredScreenshot: join(shotDir,"jev_gui_restored_20261010_r1r2.png"),
    restoredLabelText: await page.getByText("有历史核验记录 · 当前未验证", { exact: true }).textContent(),
    restoredLabelBounds: await page.getByText("有历史核验记录 · 当前未验证", { exact: true }).boundingBox(),
    retainedFixtureForAudit: true, limitations: "not a real main-model effectiveness evaluation; enforce categories remain disabled" };
  writeFileSync(report, `${JSON.stringify(evidence, null, 2)}\n`);
  console.log(JSON.stringify(evidence));
} finally {
  await browser?.close();
  if (gui.pid) spawnSync("taskkill", ["/PID", String(gui.pid), "/T", "/F"], { windowsHide: true, stdio: "ignore" });
  await new Promise((done) => server.close(done));
}
