"""Clipboard component with injected dependencies."""
from __future__ import annotations
import json
import bar_config
import subprocess
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .applications import Applications


@dataclass
class Clipboard:
    ctx: Context
    applications: Applications

    def launcher(self, args: list[str]) -> int | None:
        if "--dmenu" in args:
            return self.applications.run_application(["desktop-bar-native", *args])
        panel = bar_config.launcher_panel(args)
        if panel == "clipboard" or any(args[i] in ("-m", "--modules") and args[i + 1] == "clipboard" for i in range(len(args) - 1)):
            self.remember_clipboard_target()
        if panel not in ("apps", "notifications") and not (self.ctx.paths.runtime.parent / "desktop-search/search.sock").exists():
            self.ctx.processes.run(["systemctl", "--user", "start", "desktop-search.service"], check=False)
        if not bar_config.open_panel(self.ctx.paths.runtime, panel, storage.read_json(self.ctx.paths.state / "state.json")):
            self.applications.run_application(["desktop-bar-native", "--menu", panel])


    def clipboard(self, args: list[str]) -> int | None:
        return self.launcher(args if args else ["-m", "clipboard"])


    def remember_clipboard_target(self) -> None:
        try:
            window = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activewindow"], capture=True).stdout)
            target = window.get("address", "")
        except (OSError, subprocess.CalledProcessError, ValueError, AttributeError):
            target = ""
        storage.atomic_write(self.ctx.paths.runtime / "clipboard-target.json", json.dumps({"address": target}))


    def clipboard_paste(self, args: list[str]) -> int | None:
        target = storage.read_json(self.ctx.paths.runtime / "clipboard-target.json").get("address")
        if not target:
            raise ValueError("No clipboard paste target; the item remains copied")
        # The provider calls us only after wl-copy has acquired the selection.
        # Wait for the picker to release keyboard focus before delivering the shortcut.
        deadline = self.ctx.clock.monotonic() + 2
        while self.ctx.clock.monotonic() < deadline:
            layers = json.loads(self.ctx.processes.run(["hyprctl", "-j", "layers"], capture=True).stdout)
            if not any(layer.get("namespace", "") == "clipboard-picker"
                       or layer.get("namespace", "").startswith(("desktop-bar-menu-", "desktop-bar-panel-"))
                       for monitor in layers.values() for level in monitor.get("levels", {}).values()
                       for layer in level):
                window = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activewindow"], capture=True).stdout)
                if window.get("address") != target:
                    raise ValueError("Paste target changed; the item remains copied")
                terminal = window.get("class", "").lower() in {
                    "com.mitchellh.ghostty", "ghostty", "kitty", "alacritty", "foot", "footclient",
                    "org.wezfurlong.wezterm", "wezterm", "org.gnome.terminal", "gnome-terminal-server",
                    "konsole", "xterm", "urxvt", "st-256color",
                }
                mods = "CTRL SHIFT" if terminal else "CTRL"
                selector = json.dumps("address:" + target)
                self.ctx.processes.run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.send_shortcut({{ mods = "{mods}", key = "V", window = {selector} }}))'])
                return
            self.ctx.clock.sleep(0.02)
        raise ValueError("Clipboard picker still has focus; the item remains copied")


    def copy_data(self, data: bytes, mime: str) -> None:
        self.ctx.processes.background(["wl-copy", "--foreground", "--type", mime], operation="clipboard-owner", input=data)


    def copy_text(self, text: str) -> None:
        self.copy_data(text.encode(), "text/plain;charset=utf-8")
