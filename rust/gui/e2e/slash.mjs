import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const originalHome = process.env.USERPROFILE || process.env.HOME || "";
const profileHome = mkdtempSync(join(tmpdir(), "ncx-slash-e2e-home-"));
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
    CARGO_TARGET_DIR: resolve(root, "src-tauri/target"),
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
  const composer = page.locator("footer textarea");
  await composer.fill("/");
  const commandIds = await page.locator(".slash-cmd").allTextContents();
  for (const expected of ["/history", "/review", "/security-review", "/verify", "/docx", "/pdf", "/pptx", "/xlsx"]) {
    if (!commandIds.includes(expected)) throw new Error(`${expected} missing from GUI slash menu`);
  }
  for (const placeholder of ["/schedule", "/workflows"]) {
    if (commandIds.includes(placeholder)) throw new Error(`${placeholder} placeholder is still executable`);
  }

  await composer.fill("/verify provider switch");
  await page.locator(".slash-item").filter({ hasText: "/verify" }).click();
  const verifyPrompt = await composer.inputValue();
  if (!verifyPrompt.includes("实际 diff") || !verifyPrompt.includes("本次范围或目标：provider switch")) {
    throw new Error(`verify command did not preserve its scope: ${verifyPrompt}`);
  }

  await composer.fill("/pdf D:\\docs\\report.pdf");
  await page.locator(".slash-item").filter({ hasText: "/pdf" }).click();
  const pdfPrompt = await composer.inputValue();
  if (!pdfPrompt.includes("D:\\docs\\report.pdf") || !pdfPrompt.includes("不要手工解析二进制格式")) {
    throw new Error(`pdf command did not expand to a concrete document task: ${pdfPrompt}`);
  }

  await composer.fill("/history");
  await page.locator(".slash-item").filter({ hasText: "/history" }).click();
  await page.getByRole("heading", { name: "会话历史" }).waitFor();
  console.log(JSON.stringify({
    slashParity: true,
    realCommands: commandIds.filter((id) => ["/history", "/review", "/security-review", "/verify", "/docx", "/pdf", "/pptx", "/xlsx"].includes(id)),
    placeholdersHidden: true,
    argumentPreserved: true,
    historyPanelOpened: true,
  }, null, 2));
} finally {
  await browser?.close().catch(() => {});
  if (process.platform === "win32" && child.pid) {
    spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore" });
  } else child.kill();
}

async function waitForCdp() {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${cdpUrl}/json/version`);
      if (response.ok) return;
    } catch { /* dev app is still compiling */ }
    await new Promise((resolve) => setTimeout(resolve, 500));
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
      } catch { /* the WebView is still loading or reconnecting */ }
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("BugleCat page did not become ready");
}
