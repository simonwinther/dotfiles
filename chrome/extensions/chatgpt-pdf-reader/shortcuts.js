(() => {
  const KEY = "__chatgptPdfShortcutCleanup";
  window[KEY]?.();

  function onKeydown(event) {
    if (!event.altKey || !event.shiftKey || event.ctrlKey || event.metaKey || event.repeat || event.isComposing) return;
    if (event.code !== "KeyY" && event.key.toLowerCase() !== "y") return;

    // An old listener can survive an extension reload until the page refreshes.
    if (!chrome.runtime?.id) { cleanup(); return; }
    event.preventDefault();
    event.stopImmediatePropagation();
    chrome.runtime.sendMessage({ type: "chatgpt-pdf-toggle" }).catch((error) => {
      console.warn("PDF reader shortcut:", error.message);
      if (!chrome.runtime?.id) cleanup();
    });
  }

  function cleanup() {
    document.removeEventListener("keydown", onKeydown, true);
    if (window[KEY] === cleanup) delete window[KEY];
  }

  document.addEventListener("keydown", onKeydown, true);
  window[KEY] = cleanup;
})();
