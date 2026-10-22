(function renderKeybindPage() {
  "use strict";

  var list = document.getElementById("keybind-list");

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
})();
