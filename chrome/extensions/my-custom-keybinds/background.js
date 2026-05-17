importScripts("keybinds.js");

var ACTION_DEBOUNCE_MS = 300;
var lastAction = {
  id: "",
  at: 0
};

chrome.commands.onCommand.addListener(function handleCommand(command) {
  var keybind = MyCustomKeybinds.byCommand(command);

  if (!keybind) {
    return;
  }

  runAction(keybind.action).catch(function reportError(error) {
    console.error("[My Custom Keybinds]", error);
  });
});

chrome.runtime.onMessage.addListener(function handleMessage(message, sender, sendResponse) {
  if (!message || message.type !== "my-custom-keybinds:run-action") {
    return false;
  }

  var keybind = MyCustomKeybinds.byAction(message.action);

  if (!keybind) {
    sendResponse({
      ok: false,
      error: "Unknown action"
    });
    return false;
  }

  runAction(keybind.action, sender)
    .then(function complete() {
      sendResponse({
        ok: true
      });
    })
    .catch(function fail(error) {
      sendResponse({
        ok: false,
        error: error.message
      });
    });

  return true;
});

async function runAction(action, sender) {
  if (isDuplicateAction(action)) {
    return;
  }

  lastAction = {
    id: action,
    at: Date.now()
  };

  if (action === "duplicate-tab") {
    await duplicateCurrentTab(sender && sender.tab);
    return;
  }

  if (action === "show-keybinds") {
    await showKeybindsReference();
    return;
  }

  throw new Error("No handler for action: " + action);
}

function isDuplicateAction(action) {
  var now = Date.now();

  return lastAction.id === action && now - lastAction.at < ACTION_DEBOUNCE_MS;
}

async function duplicateCurrentTab(sourceTab) {
  var tab = sourceTab && sourceTab.id ? sourceTab : await getActiveTab();

  if (!tab || !tab.id) {
    throw new Error("No active tab to duplicate");
  }

  await chrome.tabs.duplicate(tab.id);
}

async function getActiveTab() {
  var tabs = await chrome.tabs.query({
    active: true,
    currentWindow: true
  });

  return tabs[0];
}

async function showKeybindsReference() {
  var url = chrome.runtime.getURL("keybinds.html");
  var tabs = await chrome.tabs.query({
    url: url
  });
  var existingTab = tabs[0];

  if (existingTab && existingTab.id) {
    await chrome.tabs.update(existingTab.id, {
      active: true
    });

    if (existingTab.windowId) {
      await chrome.windows.update(existingTab.windowId, {
        focused: true
      });
    }

    return;
  }

  await chrome.tabs.create({
    url: url,
    active: true
  });
}
