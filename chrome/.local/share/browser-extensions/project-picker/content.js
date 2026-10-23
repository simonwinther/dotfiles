(() => {
  if (globalThis.projectPickerSync?.version === "0.1.4") return;
  globalThis.projectPickerSync?.stop();
  let pending;
  let running;
  let lastAttempt = 0;
  let again = false;
  let stopped = false;
  let interval;

  const active = () => document.visibilityState === "visible";

  function stop() {
    stopped = true;
    clearTimeout(pending);
    clearInterval(interval);
    running?.abort();
    window.removeEventListener("focus", focusChanged);
    document.removeEventListener("visibilitychange", focusChanged);
    try { chrome.runtime.onMessage.removeListener(onMessage); } catch {}
  }

  async function send(message) {
    try {
      if (!chrome.runtime.id) { stop(); return; }
      return await chrome.runtime.sendMessage(message);
    } catch (error) {
      if (/context invalidated/i.test(error.message) || !chrome.runtime.id) {
        stop();
        return;
      }
      throw error;
    }
  }

  async function sync() {
    pending = undefined;
    if (stopped || !active()) return;
    if (running) { again = true; return; }
    lastAttempt = Date.now();
    running = new AbortController();
    const controller = running;
    try {
      const ready = await send({ type: "project-picker-status", code: "syncing" });
      if (!ready?.ok) return;
      const snapshot = await ProjectPickerAPI.collect({
        request: ProjectPickerAPI.requester(fetch.bind(globalThis), controller.signal),
        cookie: () => document.cookie,
        signal: controller.signal,
      });
      await send({ type: "project-picker-snapshot", snapshot });
    } catch (error) {
      if (!controller.signal.aborted && !stopped) {
        try {
          await send({ type: "project-picker-error", code: ProjectPickerAPI.errorCode(error), stage: error.stage, kind: error.name, error: error.message });
        } catch {}
      }
    } finally {
      running = undefined;
      if (again) { again = false; schedule(true); }
    }
  }

  function schedule(changed = false) {
    if (stopped || !active()) return;
    clearTimeout(pending);
    if (running) { again ||= changed || running.signal.aborted; return; }
    const minimumDelay = changed ? 5000 : 15000;
    pending = setTimeout(sync, Math.max(1000, minimumDelay - (Date.now() - lastAttempt)));
  }

  function focusChanged() {
    if (active()) schedule();
    else {
      clearTimeout(pending);
    }
  }

  function onMessage(message) {
    if (message?.type === "project-picker-refresh") schedule(true);
  }
  globalThis.projectPickerSync = { version: "0.1.4", stop };
  chrome.runtime.onMessage.addListener(onMessage);
  window.addEventListener("focus", focusChanged);
  document.addEventListener("visibilitychange", focusChanged);
  interval = setInterval(() => schedule(), 30000);
  schedule();
})();
