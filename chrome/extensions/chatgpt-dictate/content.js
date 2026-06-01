(() => {
  const STATE_KEY = "__chatgptDictateShortcutsState";
  const previousState = window[STATE_KEY];

  if (previousState && typeof previousState.cleanup === "function") {
    previousState.cleanup();
  }

  const state = { cleanedUp: false };
  window[STATE_KEY] = state;

  const COMMANDS = {
    TOGGLE_SUBMIT: "toggle-dictation-submit",
    CANCEL: "cancel-dictation",
    FINISH: "finish-dictation"
  };

  const VOLUME_DUCK_MESSAGE = "chatgpt-dictate-volume-duck";
  const VOLUME_DUCK_HEARTBEAT_MS = 5000;

  const KEYBOARD_COMMANDS = {
    d: COMMANDS.TOGGLE_SUBMIT,
    c: COMMANDS.CANCEL,
    s: COMMANDS.FINISH
  };

  let lastDictating = false;
  let volumeDuckSyncTimer = null;
  let volumeDuckHeartbeatTimer = null;
  let volumeDuckObserver = null;
  let handleKeydown = null;
  let handleRuntimeMessage = null;

  const isCurrentScript = () => window[STATE_KEY] === state && !state.cleanedUp;

  state.cleanup = () => {
    if (state.cleanedUp) {
      return;
    }

    state.cleanedUp = true;

    if (volumeDuckSyncTimer) {
      window.clearTimeout(volumeDuckSyncTimer);
      volumeDuckSyncTimer = null;
    }

    if (volumeDuckHeartbeatTimer) {
      window.clearInterval(volumeDuckHeartbeatTimer);
      volumeDuckHeartbeatTimer = null;
    }

    if (volumeDuckObserver) {
      volumeDuckObserver.disconnect();
      volumeDuckObserver = null;
    }

    if (handleKeydown) {
      document.removeEventListener("keydown", handleKeydown, true);
      handleKeydown = null;
    }

    try {
      const runtime = globalThis.chrome?.runtime;
      if (handleRuntimeMessage && runtime?.onMessage) {
        runtime.onMessage.removeListener(handleRuntimeMessage);
      }
    } catch {
      // The extension context can be gone during reload.
    }
    handleRuntimeMessage = null;
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  const normalize = (value) => (value || "").trim().toLowerCase();

  const ariaLabel = (element) => normalize(element?.getAttribute("aria-label"));

  const isVisible = (element) => {
    if (!element || element.disabled || element.ariaDisabled === "true") {
      return false;
    }

    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none";
  };

  const buttonText = (button) => {
    return normalize([
      button.getAttribute("aria-label"),
      button.getAttribute("title"),
      button.getAttribute("data-testid"),
      button.textContent
    ].filter(Boolean).join(" "));
  };

  const visibleButtons = () => Array.from(document.querySelectorAll("button")).filter(isVisible);

  const findButton = (predicates) => {
    return visibleButtons().find((button) => {
      const text = buttonText(button);
      return predicates.some((predicate) => predicate(button, text));
    });
  };

  const clickButton = (button) => {
    if (!button) {
      return false;
    }

    button.click();
    return true;
  };

  const findPromptEditor = () => {
    return document.querySelector("#prompt-textarea")
      || document.querySelector('textarea[data-id="root"]')
      || document.querySelector('textarea[placeholder]')
      || document.querySelector('div[contenteditable="true"][id="prompt-textarea"]')
      || document.querySelector('div[contenteditable="true"]');
  };

  const findSendButton = () => {
    return findButton([
      (button) => button.id === "composer-submit-button",
      (button) => button.matches('[data-testid="send-button"]'),
      (button) => ariaLabel(button) === "send prompt",
      (_button, text) => text === "send"
    ]);
  };

  const findDictateStartButton = () => {
    return findButton([
      (button) => ariaLabel(button) === "start dictation",
      (_button, text) => text.includes("start voice input"),
      (_button, text) => text.includes("start voice"),
      (_button, text) => text.includes("voice input"),
      (_button, text) => text.includes("microphone")
    ]);
  };

  const findDictateStopButton = () => {
    return findButton([
      (button) => ariaLabel(button) === "submit dictation",
      (_button, text) => text.includes("stop dictation"),
      (_button, text) => text.includes("submit dictation"),
      (_button, text) => text.includes("stop voice input"),
      (_button, text) => text.includes("stop recording"),
      (_button, text) => text.includes("finish dictation"),
      (_button, text) => text === "stop"
    ]);
  };

  const findDictateCancelButton = () => {
    return findButton([
      (button) => ariaLabel(button) === "cancel dictation",
      (_button, text) => text.includes("cancel voice input"),
      (_button, text) => text.includes("discard dictation"),
      (_button, text) => text.includes("cancel recording"),
      (_button, text) => text === "cancel"
    ]);
  };

  const promptHasText = () => {
    const editor = findPromptEditor();

    if (!editor) {
      return false;
    }

    if ("value" in editor) {
      return editor.value.trim().length > 0;
    }

    return editor.textContent.trim().length > 0;
  };

  const waitFor = async (getValue, { timeout = 12000, interval = 150 } = {}) => {
    const expiresAt = Date.now() + timeout;

    while (Date.now() < expiresAt) {
      const value = getValue();
      if (value) {
        return value;
      }

      await sleep(interval);
    }

    return null;
  };

  const waitForSendButtonReady = async () => {
    return waitFor(() => {
      const button = findSendButton();
      return button && !button.disabled && button.ariaDisabled !== "true" && promptHasText() ? button : null;
    });
  };

  const isDictating = () => Boolean(findDictateStopButton() || findDictateCancelButton());

  const sendVolumeDuckState = (active) => {
    try {
      const runtime = globalThis.chrome?.runtime;
      if (!runtime?.id || typeof runtime.sendMessage !== "function") {
        return;
      }

      runtime.sendMessage({ type: VOLUME_DUCK_MESSAGE, active });
    } catch {
      // The old content script can outlive the extension context after reload.
    }
  };

  const setVolumeDuckState = (active, { force = false } = {}) => {
    if (!force && active === lastDictating) {
      return;
    }

    lastDictating = active;
    sendVolumeDuckState(active);
  };

  const syncVolumeDuckState = ({ force = false } = {}) => {
    setVolumeDuckState(isDictating(), { force });
  };

  const scheduleVolumeDuckSync = () => {
    if (volumeDuckSyncTimer) {
      return;
    }

    volumeDuckSyncTimer = window.setTimeout(() => {
      volumeDuckSyncTimer = null;
      syncVolumeDuckState();
    }, 100);
  };

  const startDictation = async () => {
    const editor = findPromptEditor();
    if (editor) {
      editor.focus();
    }

    const started = clickButton(findDictateStartButton());
    if (started) {
      scheduleVolumeDuckSync();
    }
    return started;
  };

  const stopDictation = async ({ submit }) => {
    const stopButton = findDictateStopButton();
    if (!clickButton(stopButton)) {
      return false;
    }
    scheduleVolumeDuckSync();

    if (!submit) {
      await waitFor(() => !isDictating(), { timeout: 12000 });
      syncVolumeDuckState();
      return true;
    }

    const sendButton = await waitForSendButtonReady();
    syncVolumeDuckState();
    return clickButton(sendButton);
  };

  const submitPrompt = async () => {
    const sendButton = await waitForSendButtonReady();
    return clickButton(sendButton);
  };

  const cancelDictation = async () => {
    const cancelButton = findDictateCancelButton();
    if (clickButton(cancelButton)) {
      scheduleVolumeDuckSync();
      return true;
    }

    const stopButton = findDictateStopButton();
    const stopped = clickButton(stopButton);
    if (stopped) {
      scheduleVolumeDuckSync();
    }
    return stopped;
  };

  const runCommand = async (command) => {
    if (command === COMMANDS.TOGGLE_SUBMIT) {
      if (isDictating()) {
        await stopDictation({ submit: true });
        return;
      }

      await startDictation();
      return;
    }

    if (command === COMMANDS.CANCEL) {
      await cancelDictation();
      return;
    }

    if (command === COMMANDS.FINISH) {
      if (isDictating()) {
        await stopDictation({ submit: false });
        return;
      }

      await submitPrompt();
    }
  };

  volumeDuckObserver = new MutationObserver(scheduleVolumeDuckSync);

  volumeDuckObserver.observe(document.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["aria-label", "disabled", "aria-disabled", "data-testid", "title", "class", "style"]
  });

  volumeDuckHeartbeatTimer = window.setInterval(() => {
    if (!isCurrentScript()) {
      return;
    }

    syncVolumeDuckState();
    if (lastDictating) {
      syncVolumeDuckState({ force: true });
    }
  }, VOLUME_DUCK_HEARTBEAT_MS);

  window.addEventListener("pagehide", () => {
    if (lastDictating) {
      sendVolumeDuckState(false);
    }
  });

  syncVolumeDuckState();

  handleKeydown = (event) => {
    if (!isCurrentScript()) {
      return;
    }

    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || event.repeat) {
      return;
    }

    const command = KEYBOARD_COMMANDS[event.key.toLowerCase()];
    if (!command) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    runCommand(command);
  };

  document.addEventListener("keydown", handleKeydown, true);

  handleRuntimeMessage = (message) => {
    if (!isCurrentScript()) {
      return;
    }

    if (message?.type !== "chatgpt-dictate-command") {
      return;
    }

    runCommand(message.command);
  };

  try {
    globalThis.chrome?.runtime?.onMessage?.addListener(handleRuntimeMessage);
  } catch {
    // The extension context can be gone during reload.
  }
})();
