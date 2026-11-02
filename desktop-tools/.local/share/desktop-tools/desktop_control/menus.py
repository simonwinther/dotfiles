"""Menus component with injected dependencies."""
from __future__ import annotations
import json
import bar_config
import bar_catalog
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
        panel = {"bar": "styles", "theme": "themes", "background": "wallpapers"}.get(group)
        if panel is None:
            if group not in {"font", "power-profile", "hardware", "toggle", "config", "reminder-set"} and group not in storage.read_json(bar_catalog.menus_path(self.ctx.paths.config, self.ctx.env)):
                raise ValueError(f"Unknown menu: {group}")
            panel = "menu/" + group
        if not bar_config.open_panel(self.ctx.paths.runtime, panel, self.store.state()):
            executable = self.ctx.executable("desktop-bar-native")
            self.renderer.prepare_theme()
            self.ctx.processes.execv(executable, [executable, "--menu", panel])


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
        if not bar_config.open_panel(self.ctx.paths.runtime, "keybindings", self.store.state()):
            executable = self.ctx.executable("desktop-bar-native")
            self.renderer.prepare_theme()
            self.ctx.processes.execv(executable, [executable, "--menu", "keybindings"])
