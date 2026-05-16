const CHATGPT_URL_RE = /^https:\/\/(chatgpt\.com|chat\.openai\.com)\//;

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
