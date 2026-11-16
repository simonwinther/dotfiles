// Optional browser regression test. Requires Playwright and an installed Chromium.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import http from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const extension = fileURLToPath(new URL("..", import.meta.url));
const fixture = readFileSync(new URL("fixtures/article.pdf", import.meta.url));

const server = http.createServer((request, response) => {
  if (!request.url.startsWith("/article.pdf")) {
    response.writeHead(request.url === "/favicon.ico" ? 204 : 404).end();
    return;
  }
  response.writeHead(200, { "content-type": "application/pdf", "content-length": fixture.length });
  response.end(fixture);
}).listen(0);
const pdfUrl = `http://localhost:${server.address().port}/article.pdf`;

const context = await chromium.launchPersistentContext(mkdtempSync(join(tmpdir(), "pdf-copy-")), {
  executablePath: process.env.CHROMIUM_PATH || execFileSync("which", ["chromium"], { encoding: "utf8" }).trim(),
  headless: true,
  args: ["--headless=new", `--disable-extensions-except=${extension}`, `--load-extension=${extension}`]
});

// Selects from `from` ("|" marks the start) to the end of `to` and dispatches a copy event.
async function copySelection(page, from, to) {
  return page.evaluate(([from, to]) => {
    const spans = [...document.querySelectorAll('.page[data-page-number="1"] .textLayer span[role="presentation"]')];
    let text = "";
    const starts = spans.map((span) => {
      const start = text.length;
      text += span.textContent;
      return start;
    });
    const start = text.indexOf(from.replace("|", "")) + Math.max(0, from.indexOf("|"));
    const end = text.indexOf(to, start) + to.length;
    const locate = (offset) => {
      const index = starts.findLastIndex((value) => value <= offset);
      return [spans[index].firstChild, offset - starts[index]];
    };
    const range = document.createRange();
    range.setStart(...locate(start));
    const [node, offset] = locate(end - 1);
    range.setEnd(node, offset + 1);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
    const clipboardData = new DataTransfer();
    node.parentElement.dispatchEvent(new ClipboardEvent("copy", { clipboardData, bubbles: true, cancelable: true }));
    return clipboardData.getData("text/plain");
  }, [from, to]);
}

try {
  const worker = context.serviceWorkers()[0] || await context.waitForEvent("serviceworker");
  const extensionOrigin = `chrome-extension://${new URL(worker.url()).host}`;
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });

  // PDF responses open in the viewer.
  await worker.evaluate(async () => {
    while ((await chrome.declarativeNetRequest.getDynamicRules()).length < 2) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  });
  await page.goto(pdfUrl).catch(() => {});
  await page.waitForURL((url) => url.href.startsWith(`${extensionOrigin}/viewer.html?file=`));
  await page.waitForSelector('.page[data-page-number="1"] .textLayer span[role="presentation"]');
  assert.equal(await page.textContent("#page-count"), "3");
  assert.equal(await page.title(), "article.pdf");

  // Copies convert after the page model is ready; the first copy may run before it.
  await page.waitForTimeout(1500);
  const expected = String.raw`et \(\mathbf{v}\) be the vector \([-2, b, 7, 2, a]^T\). Compute \(\mathbf{v}^T\mathbf{v}\) and \(\mathbf{v}\mathbf{v}^T\).`;
  assert.equal(await copySelection(page, "1. L|et", "vvT ."), expected);
  assert.equal(await page.textContent("#toast"), "Copied as LaTeX");

  // The context menu asks the viewer tab to copy in a given format.
  // Headless Chromium cannot grant clipboard access to extension pages, so record the write.
  const tabId = await page.evaluate(async () => {
    navigator.clipboard.writeText = async (text) => {
      window.copiedText = text;
    };
    return (await chrome.tabs.getCurrent()).id;
  });
  await worker.evaluate((tabId) => chrome.runtime.sendMessage({ type: "copy-as", format: "markdown", tabId }), tabId);
  await page.waitForFunction(() => document.querySelector("#toast").textContent === "Copied as Markdown");
  assert.equal(await page.evaluate(() => window.copiedText), expected.replace(/\\\(|\\\)/g, "$"));

  await page.evaluate(() => chrome.storage.local.set({ format: "markdown" }));
  await page.waitForTimeout(100);
  assert.equal(await copySelection(page, "Compute v = B.|[", "x, y"), String.raw`$\mathbf{v} = B.[x, y]^T$`);

  // Off leaves copying to PDF.js, which copies plain text.
  await page.evaluate(() => chrome.storage.local.set({ enabled: false }));
  await page.waitForTimeout(100);
  assert.match(await copySelection(page, "|Show every", "step"), /^Show every\s+step$/);
  await page.evaluate(() => chrome.storage.local.set({ enabled: true, format: "latex" }));

  // Toolbar basics.
  const zoom = await page.inputValue("#zoom");
  await page.click("#zoom-in");
  await page.waitForFunction((before) => document.querySelector("#zoom").value !== before, zoom);
  await page.keyboard.press("Control+f");
  await page.fill("#find-input", "matrix");
  await page.waitForFunction(() => /^\d+\/\d+$/.test(document.querySelector("#find-count").textContent));
  await page.keyboard.press("Escape");
  assert.equal(await page.isHidden("#findbar"), true);

  // The page copy control and the popup load without errors.
  assert.equal(await page.locator("#better-copy-controls").count(), 1);
  const popup = await context.newPage();
  popup.on("pageerror", (error) => errors.push(`popup: ${error.message}`));
  await popup.goto(`${extensionOrigin}/popup.html`);
  await popup.waitForFunction(() => !document.getElementById("toggle").disabled);
  await popup.close();

  // Chrome's viewer can take the PDF back without being redirected again.
  await page.click("#open-in-chrome");
  await page.waitForURL(pdfUrl);
  await page.waitForTimeout(500);
  assert.equal(page.url(), pdfUrl);

  assert.deepEqual(errors, []);
  console.log("viewer browser test passed");
} finally {
  await context.close();
  server.close();
}
