function applyBadge(enabled) {
  chrome.action.setBadgeText({ text: enabled ? "" : "OFF" });
}

function syncBadge() {
  chrome.storage.local.get({ enabled: true }, (items) => {
    if (!chrome.runtime.lastError) {
      applyBadge(items.enabled !== false);
    }
  });
}

chrome.action.setBadgeBackgroundColor({ color: "#71717a" });
chrome.runtime.onInstalled.addListener(syncBadge);
chrome.runtime.onStartup.addListener(syncBadge);
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.enabled) {
    applyBadge(changes.enabled.newValue !== false);
  }
});
syncBadge();
