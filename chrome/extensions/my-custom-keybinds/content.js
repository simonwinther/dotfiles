(function startMyCustomKeybinds() {
  "use strict";

  var SEQUENCE_TIMEOUT_MS = 1400;
  var OVERLAY_ID = "my-custom-keybinds-overlay";
  var sequence = [];
  var sequenceTimer = 0;

  document.addEventListener("keydown", handleKeydown, true);

  function handleKeydown(event) {
    if (event.defaultPrevented || isModifiedOnly(event)) {
      return;
    }

    if (event.key === "Escape" && closeOverlay()) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }

    var chord = findChord(event);

    if (chord) {
      event.preventDefault();
      event.stopPropagation();
      runKeybind(chord);
      return;
    }

    if (isEditableTarget(event.target)) {
      resetSequence();
      return;
    }

    if (event.altKey || event.ctrlKey || event.metaKey) {
      resetSequence();
      return;
    }

    handleSequence(event);
  }

  function findChord(event) {
    return MyCustomKeybinds.all().find(function matchChord(keybind) {
      var shortcut = keybind.shortcut;

      if (!shortcut || shortcut.type !== "chord") {
        return false;
      }

      return normalizeKey(event.key) === normalizeKey(shortcut.key) &&
        Boolean(shortcut.altKey) === event.altKey &&
        Boolean(shortcut.ctrlKey) === event.ctrlKey &&
        Boolean(shortcut.metaKey) === event.metaKey &&
        Boolean(shortcut.shiftKey) === event.shiftKey;
    });
  }

  function handleSequence(event) {
    var key = normalizeKey(event.key);
    var nextSequence = sequence.concat(key);
    var matches = findSequenceMatches(nextSequence);

    if (matches.length === 0) {
      resetSequence();
      nextSequence = [key];
      matches = findSequenceMatches(nextSequence);
    }

    if (matches.length === 0) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    sequence = nextSequence;
    window.clearTimeout(sequenceTimer);

    var exactMatch = matches.find(function exact(keybind) {
      return keybind.shortcut.keys.length === sequence.length;
    });

    if (exactMatch) {
      resetSequence();
      runKeybind(exactMatch);
      return;
    }

    sequenceTimer = window.setTimeout(resetSequence, SEQUENCE_TIMEOUT_MS);
  }

  function findSequenceMatches(keys) {
    return MyCustomKeybinds.all().filter(function matchSequence(keybind) {
      var shortcut = keybind.shortcut;

      if (!shortcut || shortcut.type !== "sequence") {
        return false;
      }

      return keys.every(function keyMatches(key, index) {
        return normalizeKey(shortcut.keys[index]) === key;
      });
    });
  }

  function runKeybind(keybind) {
    if (keybind.action === "show-keybinds") {
      toggleOverlay();
      return;
    }

    chrome.runtime.sendMessage(
      {
        type: "my-custom-keybinds:run-action",
        action: keybind.action
      },
      function handleResponse(response) {
        if (chrome.runtime.lastError) {
          return;
        }

        if (response && !response.ok) {
          console.error("[My Custom Keybinds]", response.error);
        }
      }
    );
  }

  function toggleOverlay() {
    if (!closeOverlay()) {
      openOverlay();
    }
  }

  function openOverlay() {
    var existing = document.getElementById(OVERLAY_ID);

    if (existing) {
      existing.remove();
    }

    var host = document.createElement("div");
    host.id = OVERLAY_ID;
    var shadow = host.attachShadow({
      mode: "open"
    });

    shadow.appendChild(buildOverlay());
    document.documentElement.appendChild(host);
  }

  function closeOverlay() {
    var existing = document.getElementById(OVERLAY_ID);

    if (!existing) {
      return false;
    }

    existing.remove();
    return true;
  }

  function buildOverlay() {
    var wrapper = document.createElement("div");
    wrapper.className = "backdrop";

    var panel = document.createElement("section");
    panel.className = "panel";
    panel.setAttribute("aria-label", "My Custom Keybinds");
    panel.setAttribute("role", "dialog");

    var header = document.createElement("header");
    header.className = "header";

    var title = document.createElement("h1");
    title.textContent = "My Custom Keybinds";

    var closeButton = document.createElement("button");
    closeButton.type = "button";
    closeButton.className = "close";
    closeButton.textContent = "Close";
    closeButton.addEventListener("click", closeOverlay);

    header.append(title, closeButton);

    var list = document.createElement("div");
    list.className = "list";

    MyCustomKeybinds.all().forEach(function addKeybind(keybind) {
      var row = document.createElement("article");
      row.className = "row";

      var shortcut = document.createElement("kbd");
      shortcut.textContent = keybind.shortcut.display;

      var copy = document.createElement("div");
      copy.className = "copy";

      var name = document.createElement("strong");
      name.textContent = keybind.name;

      var description = document.createElement("p");
      description.textContent = keybind.description;

      copy.append(name, description);
      row.append(shortcut, copy);
      list.appendChild(row);
    });

    var style = document.createElement("style");
    style.textContent = [
      ":host { all: initial; color-scheme: light dark; }",
      ".backdrop {",
      "  position: fixed;",
      "  inset: 0;",
      "  z-index: 2147483647;",
      "  display: grid;",
      "  place-items: start center;",
      "  box-sizing: border-box;",
      "  padding: 12vh 20px 20px;",
      "  background: rgba(17, 24, 39, 0.32);",
      "  font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;",
      "}",
      ".panel {",
      "  width: min(520px, 100%);",
      "  box-sizing: border-box;",
      "  border: 1px solid rgba(148, 163, 184, 0.42);",
      "  border-radius: 8px;",
      "  background: rgba(255, 255, 255, 0.96);",
      "  box-shadow: 0 22px 70px rgba(15, 23, 42, 0.28);",
      "  color: #111827;",
      "  overflow: hidden;",
      "}",
      ".header {",
      "  display: flex;",
      "  align-items: center;",
      "  justify-content: space-between;",
      "  gap: 16px;",
      "  padding: 14px 16px;",
      "  border-bottom: 1px solid #e5e7eb;",
      "}",
      "h1 {",
      "  margin: 0;",
      "  font-size: 15px;",
      "  line-height: 1.3;",
      "  font-weight: 700;",
      "  letter-spacing: 0;",
      "}",
      ".close {",
      "  appearance: none;",
      "  border: 1px solid #d1d5db;",
      "  border-radius: 6px;",
      "  background: #f9fafb;",
      "  color: #111827;",
      "  cursor: pointer;",
      "  font: inherit;",
      "  font-size: 12px;",
      "  line-height: 1;",
      "  padding: 7px 9px;",
      "}",
      ".close:hover { background: #eef2ff; border-color: #a5b4fc; }",
      ".list { display: grid; }",
      ".row {",
      "  display: grid;",
      "  grid-template-columns: minmax(72px, auto) 1fr;",
      "  gap: 14px;",
      "  align-items: center;",
      "  padding: 14px 16px;",
      "  border-bottom: 1px solid #eef2f7;",
      "}",
      ".row:last-child { border-bottom: 0; }",
      "kbd {",
      "  justify-self: start;",
      "  min-width: 48px;",
      "  box-sizing: border-box;",
      "  border: 1px solid #cbd5e1;",
      "  border-bottom-width: 2px;",
      "  border-radius: 6px;",
      "  background: #f8fafc;",
      "  color: #0f172a;",
      "  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;",
      "  font-size: 12px;",
      "  line-height: 1;",
      "  padding: 7px 9px;",
      "  text-align: center;",
      "  white-space: nowrap;",
      "}",
      ".copy { min-width: 0; }",
      "strong {",
      "  display: block;",
      "  color: #111827;",
      "  font-size: 13px;",
      "  line-height: 1.3;",
      "}",
      "p {",
      "  margin: 3px 0 0;",
      "  color: #4b5563;",
      "  font-size: 12px;",
      "  line-height: 1.45;",
      "}",
      "@media (prefers-color-scheme: dark) {",
      "  .backdrop { background: rgba(2, 6, 23, 0.46); }",
      "  .panel { background: rgba(17, 24, 39, 0.97); border-color: rgba(71, 85, 105, 0.8); color: #f9fafb; }",
      "  .header { border-bottom-color: #334155; }",
      "  h1, strong { color: #f9fafb; }",
      "  p { color: #cbd5e1; }",
      "  .row { border-bottom-color: #273449; }",
      "  kbd { background: #0f172a; border-color: #475569; color: #e5e7eb; }",
      "  .close { background: #111827; border-color: #4b5563; color: #f9fafb; }",
      "  .close:hover { background: #1e293b; border-color: #64748b; }",
      "}",
      "@media (max-width: 420px) {",
      "  .backdrop { padding: 10vh 12px 12px; }",
      "  .row { grid-template-columns: 1fr; gap: 8px; }",
      "}"
    ].join("");

    wrapper.addEventListener("click", function closeFromBackdrop(event) {
      if (event.target === wrapper) {
        closeOverlay();
      }
    });

    panel.append(header, list);
    wrapper.append(style, panel);

    return wrapper;
  }

  function resetSequence() {
    sequence = [];
    window.clearTimeout(sequenceTimer);
  }

  function normalizeKey(key) {
    return String(key).toLowerCase();
  }

  function isModifiedOnly(event) {
    return ["Alt", "Control", "Meta", "Shift"].indexOf(event.key) !== -1;
  }

  function isEditableTarget(target) {
    if (!target || target === document.body) {
      return false;
    }

    if (target.isContentEditable) {
      return true;
    }

    var editableSelector = "input, textarea, select, [role='textbox']";

    return Boolean(target.closest && target.closest(editableSelector));
  }
})();
