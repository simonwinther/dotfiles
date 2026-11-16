const toggle = document.getElementById("toggle");
const status = document.getElementById("status");
const formatPicker = document.getElementById("format-picker");
const formatInputs = document.querySelectorAll('input[name="format"]');
const formatHint = document.getElementById("format-hint");
const openPdfs = document.getElementById("open-pdfs");
const fileAccess = document.getElementById("file-access");
const openCurrentRow = document.getElementById("open-current-row");
const openCurrent = document.getElementById("open-current");
const error = document.getElementById("error");
const VIEWER = chrome.runtime.getURL("viewer.html");
let settings = { enabled: true, format: "latex", openPdfs: true };
let loaded = false;
let saving = false;

function render() {
  toggle.checked = settings.enabled;
  toggle.disabled = saving || !loaded;
  openPdfs.checked = settings.openPdfs;
  openPdfs.disabled = saving || !loaded;
  formatPicker.disabled = saving || !loaded || !settings.enabled;
  status.textContent = !loaded
    ? "Loading settings…"
    : settings.enabled
      ? `Enabled · ${settings.format === "latex" ? "LaTeX" : "Markdown"}`
      : "Disabled";

  for (const input of formatInputs) {
    input.checked = input.value === settings.format;
  }

  formatHint.textContent = !settings.enabled
    ? "Copies plain text, like Chrome’s viewer."
    : settings.format === "latex"
      ? "Text and equations for a LaTeX document."
      : "Markdown formatting with math delimiters.";
}

chrome.storage.local.get(settings, (items) => {
  if (chrome.runtime.lastError) {
    status.textContent = "Settings unavailable";
    showError("Could not load settings. Reopen this popup to retry.");
    return;
  }

  settings = {
    enabled: items.enabled !== false,
    format: items.format === "markdown" ? "markdown" : "latex",
    openPdfs: items.openPdfs !== false
  };
  loaded = true;
  render();
});

toggle.addEventListener("change", () => saveSettings({ enabled: toggle.checked }));
openPdfs.addEventListener("change", () => saveSettings({ openPdfs: openPdfs.checked }));

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") {
    return;
  }

  if (changes.enabled) {
    settings.enabled = changes.enabled.newValue !== false;
  }
  if (changes.format) {
    settings.format = changes.format.newValue === "markdown" ? "markdown" : "latex";
  }
  if (changes.openPdfs) {
    settings.openPdfs = changes.openPdfs.newValue !== false;
  }
  render();
});

for (const input of formatInputs) {
  input.addEventListener("change", () => {
    if (input.checked) {
      saveSettings({ format: input.value });
    }
  });
}

function saveSettings(changes) {
  const previous = settings;
  settings = { ...settings, ...changes };
  saving = true;
  error.hidden = true;
  render();

  chrome.storage.local.set(changes, () => {
    if (chrome.runtime.lastError) {
      settings = previous;
      showError("Could not save settings. Please try again.");
    }

    saving = false;
    render();
  });
}

function showError(message) {
  error.textContent = message;
  error.hidden = false;
}

chrome.extension.isAllowedFileSchemeAccess().then((allowed) => {
  fileAccess.hidden = allowed;
});

document.getElementById("file-access-settings").addEventListener("click", () => {
  chrome.tabs.create({ url: `chrome://extensions/?id=${chrome.runtime.id}` });
});

// Offer the viewer for PDF tabs that Chrome opened before the extension took over.
chrome.tabs.query({ active: true, currentWindow: true }).then(([tab]) => {
  if (!tab?.url || tab.url.startsWith(VIEWER) || !/^(https?|file):/i.test(tab.url)) return;
  if (!/\.pdf([?#]|$)/i.test(tab.url) && !/[?&]format=pdf\b/i.test(tab.url)) return;
  openCurrentRow.hidden = false;
  openCurrent.addEventListener("click", () => {
    chrome.runtime.sendMessage({ type: "open-in-viewer", tabId: tab.id, url: tab.url });
    window.close();
  });
});

render();
