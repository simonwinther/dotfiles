const toggle = document.getElementById("toggle");
const status = document.getElementById("status");

function render(enabled) {
  toggle.checked = enabled;
  status.textContent = enabled ? "Enabled" : "Disabled";
}

chrome.storage.local.get({ enabled: true }, (items) => {
  if (!chrome.runtime.lastError) {
    render(items.enabled !== false);
  }
});

toggle.addEventListener("change", () => {
  const enabled = toggle.checked;
  chrome.storage.local.set({ enabled }, () => {
    if (chrome.runtime.lastError) {
      render(!enabled);
      return;
    }

    render(enabled);
  });
});
