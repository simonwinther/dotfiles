const HOST = "com.desktop.project_picker";
const PAGE = /^https:\/\/chatgpt\.com\//;

async function status(tabId, error) {
  await Promise.allSettled([
    chrome.action.setBadgeText({ tabId, text: error ? "!" : "" }),
    chrome.action.setTitle({ tabId, title: error ? `Project sync: ${error}` : "Project picker synced · Click to refresh" }),
  ]);
}

async function refresh(tabId) {
  if (tabId >= 0) await chrome.tabs.sendMessage(tabId, { type: "project-picker-refresh" }).catch(() => {});
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.frameId !== 0 || !PAGE.test(sender.url || "") || !Number.isInteger(sender.tab?.id)) return;
  if (!["project-picker-snapshot", "project-picker-status", "project-picker-error"].includes(message?.type)) return;
  const payload = message.type === "project-picker-snapshot" ? message.snapshot :
    { action: "status", code: message.code || "unexpected", stage: message.stage, kind: message.kind };
  chrome.runtime.sendNativeMessage(HOST, payload).then(async (reply) => {
    if (!reply?.ok) {
      await status(sender.tab.id, reply?.error || "The native helper rejected the update.");
      respond({ ok: false });
      return;
    }
    if (message.type === "project-picker-error") {
      await status(sender.tab.id, typeof message.error === "string" ? message.error.slice(0, 200) : "Sync failed");
    } else if (message.type === "project-picker-status") {
      await chrome.action.setTitle({ tabId: sender.tab.id, title: "Syncing projects…" });
    } else {
      await status(sender.tab.id);
    }
    respond({ ok: true });
  }).catch(async () => {
    await status(sender.tab.id, "Cannot update the local cache. Check the native helper registration.");
    respond({ ok: false });
  });
  return true;
});

chrome.webRequest.onCompleted.addListener((details) => {
  const path = new URL(details.url).pathname;
  const projectChange = /^\/backend-api\/(?:gizmos|projects)(?:\/|$)/.test(path) &&
    !["GET", "HEAD", "OPTIONS"].includes(details.method);
  if (details.statusCode >= 200 && details.statusCode < 300 &&
      (details.method === "DELETE" || projectChange)) {
    void refresh(details.tabId);
  }
}, { urls: ["https://chatgpt.com/backend-api/*"] });

chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url && PAGE.test(change.url)) void refresh(tabId);
});

chrome.action.onClicked.addListener((tab) => {
  if (PAGE.test(tab.url || "")) {
    chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["api.js", "content.js"] })
      .then(() => refresh(tab.id)).catch(() => status(tab.id, "Refresh this page to start syncing."));
  }
});

chrome.runtime.onInstalled.addListener(async () => {
  const tabs = await chrome.tabs.query({ url: "https://chatgpt.com/*" });
  await Promise.allSettled(tabs.map((tab) => chrome.scripting.executeScript({
    target: { tabId: tab.id }, files: ["api.js", "content.js"],
  })));
});

// The desktop picker shows projects in an open ChatGPT app window instead of
// starting a new browser. The open port also keeps this worker running.
const WINDOW_PAGE = /^https:\/\/chatgpt\.com(?:\/|$)/;
let port;

async function open(message) {
  if (message?.action !== "open" || typeof message.url !== "string" || !WINDOW_PAGE.test(message.url)) return;
  let ok = false;
  try {
    const windows = await chrome.windows.getAll({ populate: true });
    const candidates = windows.filter((window) => ["app", "popup"].includes(window.type) &&
      window.tabs?.length === 1 && WINDOW_PAGE.test(window.tabs[0].url || ""));
    const target = candidates.find((window) => window.focused) || candidates.at(-1);
    if (target) {
      await chrome.tabs.update(target.tabs[0].id, { url: message.url });
      await chrome.windows.update(target.id, { focused: true });
      ok = true;
    }
  } catch {
    ok = false;
  }
  port?.postMessage({ id: message.id, ok });
}

function listen() {
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch {
    return;
  }
  port.onMessage.addListener((message) => { void open(message); });
  port.onDisconnect.addListener(() => {
    port = undefined;
    setTimeout(listen, 10000);
  });
  port.postMessage({ action: "listen" });
}

listen();
