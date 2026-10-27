import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

// Sidebar project-row menu and multi-select batch archive, against an isolated profile.
const root = fileURLToPath(new URL("..", import.meta.url));
const originalHome = process.env.USERPROFILE || process.env.HOME || "";
const profileHome = mkdtempSync(join(tmpdir(), "ncx-sidebar-batch-home-"));
mkdirSync(join(profileHome, ".nanocodex"), { recursive: true });
writeFileSync(join(profileHome, ".nanocodex", "config.toml"), 'api_key = "e2e-placeholder"\nbase_url = "http://127.0.0.1:9/v1"\nmodel = "e2e-model"\n');
const workspaceA = mkdtempSync(join(tmpdir(), "ncx-batch-a-"));
const workspaceB = mkdtempSync(join(tmpdir(), "ncx-batch-b-"));
const cdpUrl = "http://127.0.0.1:9222";
const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri dev -- --target x86_64-pc-windows-msvc"]
  : ["run", "tauri", "dev"];
const child = spawn(executable, args, {
  cwd: root,
  env: { ...process.env, USERPROFILE: profileHome, HOME: profileHome, CARGO_HOME: process.env.CARGO_HOME || join(originalHome, ".cargo"), RUSTUP_HOME: process.env.RUSTUP_HOME || join(originalHome, ".rustup"), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9222" },
  stdio: ["ignore", "pipe", "pipe"],
});
child.stdout.on("data", (chunk) => process.stdout.write(chunk));
child.stderr.on("data", (chunk) => process.stderr.write(chunk));

const nameA = basename(workspaceA);
const nameB = basename(workspaceB);
const threadsA = ["A1", "A2", "A3"];
const threadsB = ["B1", "B2"];

let browser;
try {
  await waitForCdp();
  browser = await chromium.connectOverCDP(cdpUrl);
  const page = await waitForPage(browser);
  const call = (method, params) => page.evaluate(async ({ method, params }) => {
    const outcome = await window.__TAURI_INTERNALS__.invoke("app_server_request", { request: { method, params } });
    return outcome.response.payload.data;
  }, { method, params });
  const stamp = Date.now();
  for (const title of threadsA) await call("threadCreate", { threadId: `batch-${title}-${stamp}`, workspace: workspaceA, title });
  for (const title of threadsB) await call("threadCreate", { threadId: `batch-${title}-${stamp}`, workspace: workspaceB, title });
  const archivedOf = async (titles) => {
    const threads = await call("threadList", { includeArchived: true });
    return titles.map((title) => threads.find((t) => t.id === `batch-${title}-${stamp}`)?.archived);
  };
  const waitArchived = async (titles, expected, what) => {
    const deadline = Date.now() + 15_000;
    let last;
    while (Date.now() < deadline) {
      last = await archivedOf(titles);
      if (last.every((value) => value === expected)) return;
      await delay(150);
    }
    throw new Error(`${what}: expected archived=${expected} for ${titles}, got ${JSON.stringify(last)}`);
  };
  const sidebar = page.locator("aside.sidebar");
  const recentGroupNames = () => sidebar.locator(".side-recents > .project-group .project-name").allTextContents();

  await page.reload();
  await waitForPage(browser);
  await sidebar.locator(".project-name", { hasText: nameA }).first().waitFor({ timeout: 30_000 });
  await sidebar.locator(".project-name", { hasText: nameB }).first().waitFor();
  // A fresh profile opens Settings on first launch; close it so the sidebar is reachable.
  const settingsCancel = page.getByRole("button", { name: "取消", exact: true });
  if (await settingsCancel.waitFor({ timeout: 5_000 }).then(() => true, () => false)) await settingsCancel.click();

  // 1. Project-row menu archives the whole directory and drops it from the list.
  await sidebar.getByRole("button", { name: `项目“${nameA}”的操作` }).click();
  await sidebar.getByRole("menuitem", { name: /归档全部并移出列表（3）/ }).click();
  await waitArchived(threadsA, true, "project archive");
  await page.waitForFunction((name) => ![...document.querySelectorAll("aside.sidebar .side-recents > .project-group .project-name")]
    .some((node) => node.textContent === name), nameA);
  if ((await recentGroupNames()).includes(nameA)) throw new Error("archived project still listed");
  await waitArchived(threadsB, false, "other project untouched");

  // 2. Multi-select two sessions across the list and archive them together.
  await sidebar.getByRole("button", { name: "批量选择会话" }).click();
  await sidebar.getByRole("button", { name: "选择会话“B1”" }).click();
  await sidebar.getByRole("button", { name: "选择会话“B2”" }).click();
  await sidebar.getByText("已选 2").waitFor();
  await sidebar.getByRole("toolbar", { name: "批量操作" }).getByRole("button", { name: "归档", exact: true }).click();
  await waitArchived(threadsB, true, "batch archive");
  if (await sidebar.getByRole("toolbar", { name: "批量操作" }).count()) throw new Error("select mode should end after a batch action");

  // 3. In the archived list, a directory checkbox plus Shift+click range select, then batch restore.
  const archiveToggle = sidebar.locator(".side-archive-toggle");
  if ((await archiveToggle.getAttribute("aria-expanded")) !== "true") await archiveToggle.click();
  await sidebar.getByRole("button", { name: "批量选择会话" }).click();
  await sidebar.getByRole("button", { name: `选择“${nameB}”下的全部会话` }).click();
  await sidebar.getByText("已选 2").waitFor();
  await sidebar.getByRole("button", { name: `取消选择“${nameB}”下的全部会话` }).click();
  await sidebar.getByText("已选 0").waitFor();
  const archivedA = sidebar.locator(".side-archived-list .project-group").filter({ hasText: nameA });
  const aRows = archivedA.locator(".recent-item");
  await aRows.first().click();
  await aRows.last().click({ modifiers: ["Shift"] });
  await sidebar.getByText("已选 3").waitFor();
  await page.keyboard.press("Escape");
  if (await sidebar.getByRole("toolbar", { name: "批量操作" }).count()) throw new Error("Escape should leave select mode");
  await sidebar.getByRole("button", { name: "批量选择会话" }).click();
  await aRows.first().click();
  await aRows.last().click({ modifiers: ["Shift"] });
  await sidebar.getByRole("toolbar", { name: "批量操作" }).getByRole("button", { name: "取消归档" }).click();
  await waitArchived(threadsA, false, "batch restore");
  await waitArchived(threadsB, true, "unselected sessions stay archived");
  await sidebar.locator(".side-recents > .project-group .project-name", { hasText: nameA }).first().waitFor();

  // 4. The archived project's own menu restores the rest of the directory.
  await sidebar.getByRole("button", { name: `项目“${nameB}”的操作` }).click();
  await sidebar.getByRole("menuitem", { name: /全部取消归档（2）/ }).click();
  await waitArchived(threadsB, false, "project restore");

  // 5. Deleting is permanent and always confirmed; project folders stay on disk.
  const existing = async (titles) => {
    const threads = await call("threadList", { includeArchived: true });
    return titles.filter((title) => threads.some((t) => t.id === `batch-${title}-${stamp}`));
  };
  const waitDeleted = async (titles, what) => {
    const deadline = Date.now() + 15_000;
    let left;
    while (Date.now() < deadline) {
      left = await existing(titles);
      if (!left.length) return;
      await delay(150);
    }
    throw new Error(`${what}: still stored ${JSON.stringify(left)}`);
  };
  const deleteDialog = page.getByRole("alertdialog");
  // Step 4's restore re-sorts the list; let B settle so a click can't land on a shifted row.
  await page.waitForFunction((name) => {
    const recent = [...document.querySelectorAll("aside.sidebar .side-recents > .project-group")]
      .find((group) => group.querySelector(".project-name")?.textContent === name);
    const archived = [...document.querySelectorAll("aside.sidebar .side-archived-list .project-name")]
      .some((node) => node.textContent === name);
    return recent?.querySelector(".project-count")?.textContent === "2" && !archived;
  }, nameB);
  await page.waitForTimeout(400);
  await sidebar.getByRole("button", { name: `项目“${nameB}”的操作` }).click();
  await sidebar.getByRole("menuitem", { name: /删除项目会话（2）/ }).click();
  await deleteDialog.getByText("永久删除 2 个会话？").waitFor();
  await deleteDialog.getByRole("button", { name: "取消" }).click();
  if ((await existing(threadsB)).length !== 2) throw new Error("cancel must not delete");
  await sidebar.getByRole("button", { name: `项目“${nameB}”的操作` }).click();
  await sidebar.getByRole("menuitem", { name: /删除项目会话（2）/ }).click();
  await deleteDialog.getByRole("button", { name: "删除", exact: true }).click();
  await waitDeleted(threadsB, "project delete");
  await page.waitForFunction((name) => ![...document.querySelectorAll("aside.sidebar .project-name")]
    .some((node) => node.textContent === name), nameB);
  if ((await existing(threadsA)).length !== 3) throw new Error("other project must survive a project delete");

  // Select-all reaches collapsed project groups too; then batch delete.
  // Collapse project A so select-all has to reach a closed group.
  await sidebar.locator(".side-recents > .project-group .project-toggle", { hasText: nameA }).first().click();
  await sidebar.getByRole("button", { name: "批量选择会话" }).click();
  await sidebar.getByRole("toolbar", { name: "批量操作" }).getByRole("button", { name: "全选" }).click();
  const selectedText = await sidebar.locator(".side-batch-count").textContent();
  if (Number(selectedText?.replace(/\D/g, "")) < 3) throw new Error(`select-all missed a collapsed project: ${selectedText}`);
  await page.keyboard.press("Escape");
  await sidebar.getByRole("button", { name: "批量选择会话" }).click();
  await sidebar.getByRole("button", { name: `选择“${nameA}”下的全部会话` }).click();
  await sidebar.getByText("已选 3").waitFor();
  await sidebar.getByRole("toolbar", { name: "批量操作" }).getByRole("button", { name: "删除" }).click();
  await deleteDialog.getByText("永久删除 3 个会话？").waitFor();
  await deleteDialog.getByRole("button", { name: "删除", exact: true }).click();
  await waitDeleted(threadsA, "batch delete");
  for (const dir of [workspaceA, workspaceB]) {
    if (!existsSync(dir)) throw new Error(`project folder was deleted: ${dir}`);
  }
  console.log("sidebar batch e2e: ok (project archive, multi-select archive, group checkbox, shift range, escape, restore, project delete, select-all, batch delete)");
} catch (error) {
  for (const page of browser?.contexts().flatMap((c) => c.pages()) || []) {
    if (!page.url().includes("localhost:5179")) continue;
    await page.screenshot({ path: join(tmpdir(), "ncx-sidebar-batch-failure.png") }).catch(() => {});
    console.error("layout:", await page.evaluate(() => ["aside.sidebar", ".workarea", ".project-row", ".project-act"].map((sel) => {
      const node = document.querySelector(sel); const r = node?.getBoundingClientRect();
      return `${sel} ${r ? `${r.left},${r.top} ${r.width}x${r.height}` : "missing"}`;
    }).join(" | ")).catch(() => ""));
  }
  throw error;
} finally {
  await browser?.close();
  stopTree(child.pid);
  for (const dir of [workspaceA, workspaceB]) rmSync(dir, { recursive: true, force: true });
}

async function waitForCdp() {
  const deadline = Date.now() + 300_000;
  while (Date.now() < deadline) {
    try { if ((await fetch(`${cdpUrl}/json/version`)).ok) return; } catch {}
    await delay(250);
  }
  throw new Error("WebView2 CDP endpoint did not become ready");
}

async function waitForPage(connectedBrowser) {
  const deadline = Date.now() + 120_000;
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
