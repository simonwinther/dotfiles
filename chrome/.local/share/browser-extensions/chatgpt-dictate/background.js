const CHATGPT_URL_RE = /^https:\/\/(chatgpt\.com|chat\.openai\.com)\//;
const VOLUME_DUCK_HOST = "com.simon.chatgptdictate.audioduck";
const VOLUME_DUCK_MESSAGE = "chatgpt-dictate-volume-duck";
const volumeDuckedTabs = new Set();

const tabVolumeDuckClient = (tabId) => `chatgpt-dictate-tab-${tabId}`;

const sendNativeVolumeDuck = (action, tabId) => {
  chrome.runtime.sendNativeMessage(
    VOLUME_DUCK_HOST,
    { action, client: tabVolumeDuckClient(tabId), factor: 0.3 },
    () => {
      void chrome.runtime.lastError;
    }
  );
};

const setTabVolumeDuck = (tabId, active) => {
  if (!tabId) {
    return;
  }

  if (active) {
    volumeDuckedTabs.add(tabId);
    sendNativeVolumeDuck("start", tabId);
    return;
  }

  volumeDuckedTabs.delete(tabId);
  sendNativeVolumeDuck("stop", tabId);
};

chrome.commands.onCommand.addListener(async (command) => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });

  if (!tab?.id || !tab.url || !CHATGPT_URL_RE.test(tab.url)) {
    return;
  }

  try {
    await chrome.tabs.sendMessage(tab.id, { type: "chatgpt-dictate-command", command });
  } catch {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ["content.js"]
    });
    await chrome.tabs.sendMessage(tab.id, { type: "chatgpt-dictate-command", command });
  }
});

chrome.runtime.onMessage.addListener((message, sender) => {
  if (message?.type !== VOLUME_DUCK_MESSAGE) {
    return;
  }

  setTabVolumeDuck(sender.tab?.id, Boolean(message.active));
});

chrome.tabs.onRemoved.addListener((tabId) => {
  setTabVolumeDuck(tabId, false);
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading" || (changeInfo.url && !CHATGPT_URL_RE.test(changeInfo.url))) {
    setTabVolumeDuck(tabId, false);
  }
});
