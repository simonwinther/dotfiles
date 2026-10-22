// Optional browser regression test. Requires Playwright and an installed Chromium.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const extension = fileURLToPath(new URL(".", import.meta.url));
const context = await chromium.launchPersistentContext(mkdtempSync(join(tmpdir(), "chatgpt-pdf-idle-")), {
  executablePath: process.env.CHROMIUM_PATH || execFileSync("which", ["chromium"], { encoding: "utf8" }).trim(), headless: true,
  args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`]
});

try {
  let mode = "normal";
  let stalledRequest;
  const fixture = '<!doctype html><title>Reader lifecycle</title><main><article data-turn="user"><div data-message-author-role="user" data-message-id="u1"><div class="markdown"><p>SAVED_PROMPT</p><img alt="fixture" src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7"></div></div></article></main><textarea>Keep this draft</textarea>';
  const normal = [
    { id: "u1", author: { role: "user" }, content: { content_type: "text", parts: ["SAVED_PROMPT"] } },
    { id: "a1", author: { role: "assistant" }, content: { content_type: "text", parts: [String.raw`Saved answer: \(\frac{1}{2}\).`] } }
  ];
  await context.route("https://chatgpt.com/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/auth/session") return route.fulfill({ json: { accessToken: "fixture-session" } });
    if (path.startsWith("/backend-api/")) {
      if (mode === "loading") { stalledRequest = route; return; }
      const messages = mode === "many" ? Array.from({ length: 80 }, (_, index) => ({
        id: `m${index}`, author: { role: index % 2 ? "assistant" : "user" },
        content: { content_type: "text", parts: [`Unique message ${index}.`] }
      })) : normal;
      return route.fulfill({ json: { messages, page_info: { has_previous_page: false } } });
    }
    return route.fulfill({ contentType: "text/html", body: fixture });
  });
  const worker = context.serviceWorkers()[0] || await context.waitForEvent("serviceworker");
  assert.equal(await worker.evaluate(() => chrome.tabs.onRemoved.hasListeners()), false, "Unrelated tab closes do not wake the background worker");
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("https://chatgpt.com/c/lifecycle");
  const tabId = await worker.evaluate(async () => (await chrome.tabs.query({ url: "https://chatgpt.com/c/lifecycle" }))[0].id);

  async function probe(action) {
    const [result] = await worker.evaluate(async ({ tabId, action }) => chrome.scripting.executeScript({
      target: { tabId }, args: [action],
      func: (action) => {
        if (action === "install") {
          const timers = new Set();
          const frames = new Set();
          const heldFrames = new Set();
          const observers = new Set();
          const listeners = new Set();
          const images = [];
          const requests = new Set();
          const counts = { timers: 0, frames: 0, observers: 0, listeners: 0 };
          const probe = { timers, frames, heldFrames, observers, listeners, images, requests, counts, holdFrames: false, holdImages: false, holdStyles: false };
          window.__pdfLifecycleProbe = probe;
          const nativeTimeout = window.setTimeout;
          const nativeClear = window.clearTimeout;
          const nativeFrame = window.requestAnimationFrame;
          const nativeCancel = window.cancelAnimationFrame;
          window.setTimeout = (callback, delay, ...args) => {
            const id = nativeTimeout(() => { timers.delete(id); counts.timers++; callback(...args); }, delay);
            timers.add(id); return id;
          };
          window.clearTimeout = (id) => { timers.delete(id); nativeClear(id); };
          let heldId = 1000000000;
          window.requestAnimationFrame = (callback) => {
            if (probe.holdFrames) { const id = heldId++; heldFrames.add(id); frames.add(id); return id; }
            const id = nativeFrame((time) => { frames.delete(id); counts.frames++; callback(time); });
            frames.add(id); return id;
          };
          window.cancelAnimationFrame = (id) => { frames.delete(id); if (!heldFrames.delete(id)) nativeCancel(id); };
          const NativeObserver = window.ResizeObserver;
          window.ResizeObserver = class extends NativeObserver {
            constructor(callback) { super((...args) => { counts.observers++; callback(...args); }); }
            observe(target, options) { observers.add(this); super.observe(target, options); }
            disconnect() { observers.delete(this); super.disconnect(); }
          };
          const nativeAdd = EventTarget.prototype.addEventListener;
          EventTarget.prototype.addEventListener = function(type, callback, options) {
            if (!options?.signal) return nativeAdd.call(this, type, callback, options);
            const record = {};
            listeners.add(record);
            nativeAdd.call(options.signal, "abort", () => listeners.delete(record), { once: true });
            return nativeAdd.call(this, type, (...args) => { counts.listeners++; callback(...args); }, options);
          };
          const nativeFetch = window.fetch;
          window.fetch = async (...args) => {
            const record = {};
            requests.add(record);
            try {
              if (probe.holdStyles && String(args[0]).endsWith("vendor/temml/math.css")) {
                const signal = args[1].signal;
                return await new Promise((resolve, reject) => {
                  signal.throwIfAborted();
                  nativeAdd.call(signal, "abort", () => reject(signal.reason), { once: true });
                });
              }
              return await nativeFetch(...args);
            } finally { requests.delete(record); }
          };
          const nativeDecode = HTMLImageElement.prototype.decode;
          HTMLImageElement.prototype.decode = function() {
            images.push(new WeakRef(this));
            return probe.holdImages ? new Promise(() => {}) : nativeDecode.call(this);
          };
        }
        const state = window.__pdfLifecycleProbe;
        if (action === "hold-images") state.holdImages = true;
        if (action === "hold-styles") state.holdStyles = true;
        if (action === "hold-frames") { state.holdImages = false; state.holdFrames = true; }
        if (action === "normal") { state.holdImages = false; state.holdFrames = false; state.holdStyles = false; }
        return {
          timers: state.timers.size, frames: state.frames.size, observers: state.observers.size,
          listeners: state.listeners.size, requests: state.requests.size, counts: { ...state.counts },
          imagesWithSource: state.images.filter((ref) => ref.deref()?.hasAttribute("src")).length,
          reader: Boolean(window.__chatgptPdfReader), helpers: Boolean(window.__chatgptPdfHistory || window.__chatgptPdfMath),
          shortcut: typeof window.__chatgptPdfShortcutCleanup === "function"
        };
      }
    }), { tabId, action });
    return result.result;
  }

  function assertClosed(state) {
    for (const key of ["timers", "frames", "observers", "listeners", "requests", "imagesWithSource"]) assert.equal(state[key], 0, key);
    assert.equal(state.reader, false);
    assert.equal(state.helpers, false);
    assert.equal(state.shortcut, true);
  }
  async function inspect() { return probe("inspect"); }
  async function close() {
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => ![...document.documentElement.children].some((node) => node.shadowRoot?.querySelector("dialog")));
    // Let cancellation promise continuations settle in the renderer.
    await page.evaluate(() => Promise.resolve());
    assertClosed(await inspect());
    assert.equal(await page.locator("textarea").inputValue(), "Keep this draft");
    assert.ok((await page.locator("main img").getAttribute("src")).startsWith("data:image/"));
  }
  async function until(check) {
    for (let attempt = 0; attempt < 100; attempt++) {
      const state = await inspect();
      if (check(state)) return;
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    throw new Error("Reader did not reach the requested lifecycle phase.");
  }

  const initial = await probe("install");
  assertClosed(initial);
  await page.keyboard.press("Alt+Shift+y");
  await page.waitForFunction(() => document.activeElement?.shadowRoot?.activeElement?.className === "viewport");
  assert.equal((await inspect()).timers, 0, "Finished rendering leaves no timeout timers");
  await close();
  const idle = await inspect();
  await page.setViewportSize({ width: 900, height: 800 });
  await page.locator("textarea").fill("Keep this draft");
  await page.keyboard.type("x");
  await page.keyboard.press("Backspace");
  await new Promise((resolve) => setTimeout(resolve, 1700));
  const afterIdle = await inspect();
  assertClosed(afterIdle);
  assert.deepEqual(afterIdle.counts, idle.counts, "No reader callbacks while closed, even after resizing or typing");

  await probe("hold-images");
  await page.keyboard.press("Alt+Shift+y");
  await until((state) => state.imagesWithSource > 0 && state.timers > 0);
  await close();

  await probe("hold-frames");
  mode = "many";
  await page.keyboard.press("Alt+Shift+y");
  await until((state) => state.frames > 0);
  await close();

  await probe("normal");
  mode = "loading";
  await page.keyboard.press("Alt+Shift+y");
  await until((state) => state.requests > 0 && state.timers > 0);
  await close();
  await stalledRequest?.abort().catch(() => {});

  mode = "normal";
  await probe("hold-styles");
  await page.keyboard.press("Alt+Shift+y");
  await until((state) => state.requests > 0);
  await close();
  assert.deepEqual(errors, []);
  console.log("PASS: closed reader has no timers, frames, observers, reader listeners, requests, images or helper state; cancels math setup, history, image waits and pagination; no reader callbacks during idle, resize or typing; unrelated tab closes do not wake the worker.");
} finally {
  await context.close();
}
