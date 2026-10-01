const CHATGPT_URL = /^https:\/\/(chatgpt\.com|chat\.openai\.com)\//;
const MESSAGE = "chatgpt-pdf-toggle";
const recentToggles = new Map();

async function toggleReader(tab, source) {
  if (!tab?.id || !CHATGPT_URL.test(tab.url || "")) return false;

  // Some browser windows deliver both the native command and the page keydown.
  // Treat those as one keypress without blocking successive page keypresses.
  const previous = recentToggles.get(tab.id);
  const now = Date.now();
  if (previous && previous.source !== source && now - previous.time < 300) return true;
  recentToggles.set(tab.id, { source, time: now });

  try {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ["content.js"]
    });
    await chrome.action.setBadgeText({ tabId: tab.id, text: "" });
    await chrome.action.setTitle({ tabId: tab.id, title: "Toggle PDF reader (Alt+Shift+Y)" });
    return true;
  } catch (error) {
    console.warn("Could not toggle PDF reader:", error.message);
    await Promise.allSettled([
      chrome.action.setBadgeText({ tabId: tab.id, text: "!" }),
      chrome.action.setTitle({ tabId: tab.id, title: `PDF reader: ${error.message}` })
    ]);
    return false;
  }
}

chrome.action.onClicked.addListener((tab) => { void toggleReader(tab, "toolbar"); });

chrome.commands.onCommand.addListener(async (command, tab) => {
  if (command !== "toggle-reader") return;
  if (!tab?.id) [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  await toggleReader(tab, "command");
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type !== MESSAGE || sender.frameId !== 0 || !CHATGPT_URL.test(sender.url || "")) return;
  toggleReader(sender.tab, "page").then((ok) => sendResponse({ ok }));
  return true;
});

chrome.tabs.onRemoved.addListener((tabId) => recentToggles.delete(tabId));

// Install/reload the shortcut listener in already-open conversations too.
chrome.runtime.onInstalled.addListener(async () => {
  const tabs = await chrome.tabs.query({ url: ["https://chatgpt.com/*", "https://chat.openai.com/*"] });
  await Promise.allSettled(tabs.filter((tab) => tab.id).map((tab) => chrome.scripting.executeScript({
    target: { tabId: tab.id },
    files: ["shortcuts.js"]
  })));
});
