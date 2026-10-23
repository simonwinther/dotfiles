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
