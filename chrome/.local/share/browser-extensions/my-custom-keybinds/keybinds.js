(function registerKeybinds(root) {
  "use strict";

  var keybinds = [
    {
      id: "duplicate-tab",
      name: "Clone tab",
      description: "Open a duplicate of the current tab.",
      action: "duplicate-tab",
      command: "duplicate-tab",
      shortcut: {
        type: "chord",
        key: "c",
        altKey: true,
        display: "Alt+C"
      }
    },
    {
      id: "show-keybinds",
      name: "Show keybinds",
      description: "Open the keybind reference.",
      action: "show-keybinds",
      command: "show-keybinds",
      shortcut: {
        type: "chord",
        key: "k",
        altKey: true,
        display: "Alt+K"
      }
    }
  ];

  function cloneKeybind(keybind) {
    return {
      id: keybind.id,
      name: keybind.name,
      description: keybind.description,
      action: keybind.action,
      command: keybind.command,
      shortcut: Object.assign({}, keybind.shortcut, {
        keys: keybind.shortcut.keys && keybind.shortcut.keys.slice()
      })
    };
  }

  function all() {
    return keybinds.map(cloneKeybind);
  }

  function byAction(action) {
    return all().find(function findAction(keybind) {
      return keybind.action === action;
    });
  }

  function byCommand(command) {
    return all().find(function findCommand(keybind) {
      return keybind.command === command;
    });
  }

  root.MyCustomKeybinds = {
    all: all,
    byAction: byAction,
    byCommand: byCommand
  };
})(typeof globalThis === "undefined" ? this : globalThis);
