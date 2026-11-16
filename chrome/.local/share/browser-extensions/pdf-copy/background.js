// Opens PDFs in the copy viewer, keeps the toolbar badge in sync, and serves the context menu.
const VIEWER = chrome.runtime.getURL("viewer.html");
const DEFAULTS = { enabled: true, format: "latex", openPdfs: true };
const PDF_RULE_IDS = [1, 2];
const LOCAL_PDF = /^file:\/\/[^?#]*\.pdf$/i;
const NOT_ATTACHMENT = [{ header: "content-disposition", values: ["attachment", "attachment;*", "attachment *"] }];

function viewerUrl(url) {
  return `${VIEWER}?file=${encodeURIComponent(url)}`;
}

// Content-Type is only known once response headers arrive (Chrome 128+).
async function syncPdfRules(openPdfs) {
  const addRules = openPdfs
    ? [
        {
          id: 1,
          priority: 1,
          action: { type: "redirect", redirect: { regexSubstitution: `${VIEWER}?file=\\0` } },
          condition: {
            regexFilter: "^https?://.*$",
            resourceTypes: ["main_frame"],
            requestMethods: ["get"],
            responseHeaders: [{ header: "content-type", values: ["application/pdf", "application/pdf;*", "application/x-pdf", "application/x-pdf;*"] }],
            excludedResponseHeaders: NOT_ATTACHMENT
          }
        },
        {
          id: 2,
          priority: 1,
          action: { type: "redirect", redirect: { regexSubstitution: `${VIEWER}?file=\\0` } },
          condition: {
            regexFilter: "^https?://[^?#]*\\.[pP][dD][fF]([?#].*)?$",
            resourceTypes: ["main_frame"],
            requestMethods: ["get"],
            responseHeaders: [{ header: "content-type", values: ["application/octet-stream", "application/octet-stream;*", "binary/octet-stream", "binary/octet-stream;*"] }],
            excludedResponseHeaders: NOT_ATTACHMENT
          }
        }
      ]
    : [];
  await chrome.declarativeNetRequest.updateDynamicRules({ removeRuleIds: PDF_RULE_IDS, addRules });
}

function applyBadge(enabled) {
  chrome.action.setBadgeText({ text: enabled ? "" : "OFF" });
}

function createMenus() {
  chrome.contextMenus.removeAll(() => {
    for (const [id, title] of [["latex", "Copy as LaTeX"], ["markdown", "Copy as Markdown"], ["plain", "Copy as plain text"]]) {
      chrome.contextMenus.create({ id: `copy-${id}`, title, contexts: ["selection"], documentUrlPatterns: [`${VIEWER}*`] });
    }
  });
}

async function initialize() {
  const settings = await chrome.storage.local.get(DEFAULTS);
  applyBadge(settings.enabled !== false);
  await syncPdfRules(settings.openPdfs !== false);
  createMenus();
}

chrome.action.setBadgeBackgroundColor({ color: "#71717a" });
chrome.runtime.onInstalled.addListener(initialize);
chrome.runtime.onStartup.addListener(initialize);

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes.enabled) applyBadge(changes.enabled.newValue !== false);
  if (changes.openPdfs) syncPdfRules(changes.openPdfs.newValue !== false);
});

// Local files never reach the network stack, so redirect them on navigation.
chrome.webNavigation.onBeforeNavigate.addListener(async ({ tabId, frameId, url }) => {
  if (frameId !== 0 || !LOCAL_PDF.test(url)) return;
  const key = `bypass:${tabId}`;
  const [{ openPdfs }, session] = await Promise.all([
    chrome.storage.local.get({ openPdfs: true }),
    chrome.storage.session.get(key)
  ]);
  // Without file access the viewer explains how to grant it.
  if (!openPdfs || session[key] === url) return;
  chrome.tabs.update(tabId, { url: viewerUrl(url) });
}, { url: [{ schemes: ["file"] }] });

chrome.webNavigation.onCommitted.addListener(async ({ tabId, frameId, url }) => {
  if (frameId !== 0) return;
  const key = `bypass:${tabId}`;
  const session = await chrome.storage.session.get(key);
  if (session[key] && session[key] !== url) await clearBypass(tabId);
});

chrome.tabs.onRemoved.addListener((tabId) => clearBypass(tabId));

async function clearBypass(tabId) {
  await chrome.storage.session.remove(`bypass:${tabId}`);
  const rules = await chrome.declarativeNetRequest.getSessionRules();
  const stale = rules.filter((rule) => rule.condition.tabIds?.includes(tabId)).map((rule) => rule.id);
  if (stale.length) await chrome.declarativeNetRequest.updateSessionRules({ removeRuleIds: stale });
}

// Lets the viewer hand a PDF back to Chrome's built-in viewer.
async function openInChrome(tabId, url) {
  await clearBypass(tabId);
  await chrome.storage.session.set({ [`bypass:${tabId}`]: url });
  if (/^https?:/i.test(url)) {
    const rules = await chrome.declarativeNetRequest.getSessionRules();
    const id = Math.max(100, ...rules.map((rule) => rule.id)) + 1;
    await chrome.declarativeNetRequest.updateSessionRules({
      addRules: [{
        id,
        priority: 2,
        action: { type: "allow" },
        condition: { urlFilter: `|${url.split("#")[0]}|`, resourceTypes: ["main_frame"], tabIds: [tabId] }
      }]
    });
  }
  await chrome.tabs.update(tabId, { url });
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type === "open-in-chrome" && sender.tab?.id !== undefined) {
    openInChrome(sender.tab.id, message.url).then(() => sendResponse({ ok: true }), (error) => sendResponse({ ok: false, error: String(error) }));
    return true;
  }
  if (message?.type === "open-in-viewer" && typeof message.tabId === "number") {
    chrome.tabs.update(message.tabId, { url: viewerUrl(message.url) }).then(() => sendResponse({ ok: true }));
    return true;
  }
  return false;
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const format = String(info.menuItemId).replace(/^copy-/, "");
  if (tab?.id !== undefined) chrome.runtime.sendMessage({ type: "copy-as", format, tabId: tab.id }).catch(() => {});
});

