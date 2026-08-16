import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = fileURLToPath(new URL("..", import.meta.url));
const cdpUrl = "http://127.0.0.1:9222";
const executable = process.platform === "win32" ? process.env.ComSpec || "cmd.exe" : "npm";
const args = process.platform === "win32"
  ? ["/d", "/s", "/c", "npm run tauri dev"]
  : ["run", "tauri", "dev"];
const child = spawn(executable, args, {
  cwd: root,
  env: {
    ...process.env,
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
  await runChoiceTest(page);
  await runFreeTextTest(page);
  await runCancelTest(page);
  console.log("question e2e: ok (choice, free text, cancel)");
} finally {
  await browser?.close();
  stopTree(child.pid);
}

async function runChoiceTest(page) {
  const answer = invokeQuestion(page, "E2E option question", ["Alpha", "Beta"], false);
  await page.getByText("E2E option question", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Alpha", exact: true }).click();
  assertEqual(await answer, "Alpha", "choice answer");
}

async function runFreeTextTest(page) {
  const answer = invokeQuestion(page, "E2E text question", [], true);
  await page.getByText("E2E text question", { exact: true }).waitFor();
  await page.locator(".question-modal textarea").fill("typed answer");
  await page.locator(".question-modal .ok").click();
  assertEqual(await answer, "typed answer", "free-text answer");
}

async function runCancelTest(page) {
  const answer = invokeQuestion(page, "E2E cancel question", [], true);
  await page.getByText("E2E cancel question", { exact: true }).waitFor();
  await page.locator(".question-modal .deny").click();
  assertEqual(await answer, null, "cancel answer");
}

function invokeQuestion(page, question, options, allowFreeText) {
  return page.evaluate(
    ({ question, options, allowFreeText }) =>
      window.__TAURI_INTERNALS__.invoke("e2e_ask_question", {
        question,
        options,
        allowFreeText,
      }),
    { question, options, allowFreeText },
  );
}

async function waitForCdp() {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${cdpUrl}/json/version`);
      if (response.ok) return;
    } catch {}
    await delay(250);
  }
  throw new Error("WebView2 CDP endpoint did not become ready");
}

async function waitForPage(connectedBrowser) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const pages = connectedBrowser.contexts().flatMap((context) => context.pages());
    for (const page of pages.filter((candidate) => candidate.url().includes("localhost:5179"))) {
      try {
        await page.waitForLoadState("domcontentloaded");
        const ready = await page.evaluate(() => Boolean(window.__TAURI_INTERNALS__?.invoke));
        if (!ready) continue;
        await delay(500);
        if (!page.isClosed()) {
          console.log(`question e2e: connected to ${page.url()}`);
          return page;
        }
      } catch {}
    }
    await delay(100);
  }
  throw new Error("Tauri WebView page was not found over CDP");
}

function stopTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    try {
      process.kill(pid, "SIGTERM");
    } catch {}
  }
}

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
