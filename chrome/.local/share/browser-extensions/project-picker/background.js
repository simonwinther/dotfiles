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
  if (sender.frameId !== 0 || !PAGE.test(sender.url || "") || !sender.tab?.id) return;
  if (message?.type === "project-picker-error") {
    void status(sender.tab.id, typeof message.error === "string" ? message.error.slice(0, 200) : "Sync failed");
    return;
  }
  if (message?.type !== "project-picker-snapshot") return;
  chrome.runtime.sendNativeMessage(HOST, message.snapshot).then(async (reply) => {
    if (!reply?.ok) {
      await status(sender.tab.id, reply?.error || "The native helper rejected the update.");
      respond({ ok: false });
      return;
    }
    await status(sender.tab.id);
    respond({ ok: true });
  }).catch(async () => {
    await status(sender.tab.id, "Cannot update the local cache. Check the native helper registration.");
    respond({ ok: false });
  });
  return true;
});

chrome.webRequest.onCompleted.addListener((details) => {
  if (details.method !== "GET" && details.method !== "OPTIONS" && details.statusCode < 400) {
    void refresh(details.tabId);
  }
}, { urls: ["https://chatgpt.com/backend-api/gizmos", "https://chatgpt.com/backend-api/gizmos/*"] });

chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url && PAGE.test(change.url)) void refresh(tabId);
});

chrome.action.onClicked.addListener((tab) => {
  if (PAGE.test(tab.url || "")) void refresh(tab.id);
});
