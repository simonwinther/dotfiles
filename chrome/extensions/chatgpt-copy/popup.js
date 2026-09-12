const toggle = document.getElementById("toggle");
const status = document.getElementById("status");
const formatPicker = document.getElementById("format-picker");
const formatInputs = document.querySelectorAll('input[name="format"]');
const formatHint = document.getElementById("format-hint");
const error = document.getElementById("error");
let settings = { enabled: true, format: "latex" };
let saving = false;

function render() {
  toggle.checked = settings.enabled;
  toggle.disabled = saving;
  formatPicker.disabled = saving || !settings.enabled;
  status.textContent = settings.enabled
    ? `Enabled · ${settings.format === "latex" ? "LaTeX" : "Markdown"}`
    : "Disabled";

  for (const input of formatInputs) {
    input.checked = input.value === settings.format;
  }

  formatHint.textContent = !settings.enabled
    ? "Uses the page’s normal copy behavior."
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
    format: items.format === "markdown" ? "markdown" : "latex"
  };
  render();
});

toggle.addEventListener("change", () => {
  saveSettings({ enabled: toggle.checked });
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
