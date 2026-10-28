"""Menus component with injected dependencies."""
from __future__ import annotations
import json
import re
from dataclasses import dataclass, field
from typing import Any, Callable, TYPE_CHECKING
from .commands import Command
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .catalog import ThemeCatalog
    from .theme_render import ThemeRenderer
    from .wallpaper import Wallpaper
    from .reminders import Reminders


@dataclass
class Menus:
    ctx: Context
    store: StateStore
    catalog: ThemeCatalog
    renderer: ThemeRenderer
    wallpaper: Wallpaper
    reminders: Reminders
    execute: Callable[[Command], int]

    def menu(self, args: list[str]) -> int | None:
        group = args[0] if args else "main"
        if group == "background":
            self.wallpaper.wallpaper(["grid"]); return
        if group == "reminder-set":
            delay = self.ctx.processes.run([self.ctx.executable("walker"), "--dmenu", "--inputonly", "--placeholder", "Minutes"], input="", check=False, capture=True)
            if not delay.returncode and delay.stdout.strip():
                message = self.ctx.processes.run([self.ctx.executable("walker"), "--dmenu", "--inputonly", "--placeholder", "Reminder"], input="", check=False, capture=True)
                if not message.returncode:
                    self.reminders.reminder([delay.stdout.strip(), message.stdout.strip()])
            return
        self.renderer.prepare_theme()
        menus = storage.read_json(self.ctx.paths.config / "desktop/menus.json")
        stack = []
        while True:
            choices = dict(menus.get(group, {}).get("items", {}))
            if group == "theme":
                names = sorted({p.name for d in self.catalog.theme_directories() if d.is_dir() for p in d.iterdir() if p.is_dir()})
                choices = {("●  " if name == seself.catalog.selected_theme else "○  ") + name.replace("-", " ").title(): ["theme", "set", name] for name in names}
                choices["󰸌  Generate from wallpaper…"] = ["theme", "generate"]
            elif group == "font":
                families = self.ctx.processes.run(["fc-list", ":spacing=mono", "--format", "%{family[0]}\n"], capture=True).stdout.splitlines()
                choices = {("●  " if family == seself.catalog.selected_font else "○  ") + family: ["font", "set", family] for family in sorted(set(families))}
            elif group == "config":
                choices = {name: ["config", name] for name in ["Hyprland configuration", "Hyprland", "Keybindings", "Input", "Monitors", "Hypridle", "Hyprlock", "Night light", "SwayOSD", "Walker", "Waybar", "XCompose", "Desktop menu"]}
            elif group in ("hardware", "toggle"):
                devices = json.loads(self.ctx.processes.run(["hyprctl", "-j", "devices"], capture=True).stdout)
                if not any("touchpad" in m.get("name", "").lower() for m in devices.get("mice", [])):
                    choices = {label: command for label, command in choices.items() if command != ["toggle", "touchpad"]}
            elif group == "power-profile":
                result = self.ctx.processes.run(["powerprofilesctl", "list"], capture=True, check=False)
                choices = {name.replace("-", " ").title(): ["exec", "powerprofilesctl", "set", name] for name in ("power-saver", "balanced", "performance") if re.search(r"(?m)^\s*\*?\s*" + name + r":", result.stdout)}
            if not choices:
                raise ValueError(f"No choices available for menu: {group}")
            if stack:
                choices["󰁍  Back"] = ["back"]
            result = self.ctx.processes.run([self.ctx.executable("walker"), "--theme", "desktop-menu", "--dmenu", "--width", "350", "--minheight", "1", "--maxheight", "630", "--placeholder", menus.get(group, {}).get("title", group.replace("-", " ").title() + "…")],
                         input="\n".join(choices) + "\n", check=False, capture=True)
            selected = result.stdout.strip()
            if result.returncode or selected not in choices:
                if not stack:
                    return
                group = stack.pop()
                continue
            command, *rest = choices[selected]
            if command == "back":
                group = stack.pop()
            elif command == "menu" and rest[0] not in ("background", "reminder-set"):
                stack.append(group); group = rest[0]
            else:
                return self.execute(Command.parse(command, rest))


    def active_keybindings(self) -> list[dict[str, Any]]:
        # Query the compositor after unbinds, overrides and generated bindings apply.
        bindings = json.loads(self.ctx.processes.run(["hyprctl", "-j", "binds"], capture=True).stdout)
        missing_keys = [binding for binding in bindings if binding.get("description", "").strip()
                        and not (binding.get("key") or binding.get("keycode") or binding.get("catch_all"))]
        if missing_keys:
            # Some Hyprland versions expose Lua keycodes only in the text response.
            # Match live records by identity; a reload can change their row order.
            readable = []
            for line in self.ctx.processes.run(["hyprctl", "binds"], capture=True).stdout.splitlines():
                if line.startswith("bind"):
                    readable.append({})
                elif line.startswith("\t") and readable:
                    field, separator, value = line[1:].partition(": ")
                    if separator:
                        readable[-1][field] = value
            fields = ("modmask", "submap", "description", "dispatcher", "arg")
            for binding in missing_keys:
                keys = {entry["key"] for entry in readable if entry.get("key")
                        and all(entry.get(field) == str(binding.get(field, "")) for field in fields)}
                if len(keys) != 1:
                    raise ValueError(f"Hyprland did not report an unambiguous key for: {binding['description']}")
                binding["display_key"] = keys.pop()
        return bindings


    def keybindings(self) -> None:
        modifiers = ((64, "SUPER"), (4, "CTRL"), (8, "ALT"), (1, "SHIFT"),
                     (2, "CAPS"), (16, "MOD2"), (32, "MOD3"), (128, "MOD5"))
        lines = set()
        for binding in self.active_keybindings():
            description = " ".join(binding.get("description", "").split())
            key = f"code:{binding['keycode']}" if binding.get("keycode") else binding.get("key", "")
            if not key and binding.get("catch_all"):
                key = "catchall"
            shortcut = binding.get("display_key") or " + ".join([name for mask, name in modifiers if binding.get("modmask", 0) & mask] + [key]) if key or binding.get("display_key") else ""
            if not description or not shortcut:
                continue
            if binding.get("submap"):
                shortcut = f"[{binding['submap']}] {shortcut}"
            for flag, label in (("release", "release"), ("longPress", "long press")):
                if binding.get(flag):
                    shortcut += f" ({label})"
            lines.add(f"{shortcut}  {description}")
        if not lines:
            raise ValueError("Hyprland has no described keybindings")
        self.renderer.prepare_theme()
        self.ctx.processes.run([self.ctx.executable("walker"), "--theme", "desktop-rounded", "--dmenu", "--placeholder", "Keybindings"], input="\n".join(sorted(lines)), check=False)
