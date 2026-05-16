(() => {
  if (window.__chatgptDictateShortcutsLoaded) {
    return;
  }

  window.__chatgptDictateShortcutsLoaded = true;

  const COMMANDS = {
    TOGGLE_SUBMIT: "toggle-dictation-submit",
    CANCEL: "cancel-dictation",
    FINISH: "finish-dictation"
  };

  const KEYBOARD_COMMANDS = {
    d: COMMANDS.TOGGLE_SUBMIT,
    c: COMMANDS.CANCEL,
    s: COMMANDS.FINISH
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

  const startDictation = async () => {
    const editor = findPromptEditor();
    if (editor) {
      editor.focus();
    }

    return clickButton(findDictateStartButton());
  };

  const stopDictation = async ({ submit }) => {
    const stopButton = findDictateStopButton();
    if (!clickButton(stopButton)) {
      return false;
    }

    if (!submit) {
      await waitFor(() => !isDictating(), { timeout: 12000 });
      return true;
    }

    const sendButton = await waitForSendButtonReady();
    return clickButton(sendButton);
  };

  const submitPrompt = async () => {
    const sendButton = await waitForSendButtonReady();
    return clickButton(sendButton);
  };

  const cancelDictation = async () => {
    const cancelButton = findDictateCancelButton();
    if (clickButton(cancelButton)) {
      return true;
    }

    const stopButton = findDictateStopButton();
    return clickButton(stopButton);
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

  document.addEventListener("keydown", (event) => {
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
  }, true);

  chrome.runtime.onMessage.addListener((message) => {
    if (message?.type !== "chatgpt-dictate-command") {
      return;
    }

    runCommand(message.command);
  });
})();
