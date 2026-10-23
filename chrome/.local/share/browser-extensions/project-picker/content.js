(() => {
  if (globalThis.projectPickerSyncInstalled) return;
  globalThis.projectPickerSyncInstalled = true;
  let pending;
  let running;
  let lastAttempt = 0;
  let again = false;

  const active = () => document.visibilityState === "visible" && document.hasFocus();

  async function sync() {
    pending = undefined;
    if (!active()) return;
    if (running) { again = true; return; }
    lastAttempt = Date.now();
    running = new AbortController();
    const controller = running;
    try {
      const snapshot = await ProjectPickerAPI.collect({
        request: ProjectPickerAPI.requester(fetch.bind(globalThis), controller.signal),
        cookie: () => document.cookie,
        signal: controller.signal,
      });
      if (!active()) return;
      await chrome.runtime.sendMessage({ type: "project-picker-snapshot", snapshot });
    } catch (error) {
      if (!controller.signal.aborted) {
        await chrome.runtime.sendMessage({ type: "project-picker-error", error: error.message }).catch(() => {});
      }
    } finally {
      running = undefined;
      if (again) { again = false; schedule(true); }
    }
  }

  function schedule(changed = false) {
    if (!active()) return;
    clearTimeout(pending);
    if (running) { again ||= changed || running.signal.aborted; return; }
    const minimumDelay = changed ? 5000 : 15000;
    pending = setTimeout(sync, Math.max(1000, minimumDelay - (Date.now() - lastAttempt)));
  }

  function focusChanged() {
    if (active()) schedule();
    else {
      clearTimeout(pending);
      running?.abort();
    }
  }

  chrome.runtime.onMessage.addListener((message) => {
    if (message?.type === "project-picker-refresh") schedule(true);
  });
  window.addEventListener("focus", focusChanged);
  window.addEventListener("blur", focusChanged);
  document.addEventListener("visibilitychange", focusChanged);
  setInterval(() => schedule(), 5 * 60 * 1000);
  schedule();
})();
