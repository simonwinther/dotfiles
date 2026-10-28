"""Clipboard component with injected dependencies."""
from __future__ import annotations
import json
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
        if any(args[i] in ("-m", "--modules") and args[i + 1] == "clipboard" for i in range(len(args) - 1)):
            return self.clipboard(args)
        self.remember_clipboard_target()
        self.ctx.processes.run(["systemctl", "--user", "start", "desktop-elephant.service", "desktop-walker.service"], check=False)
        self.applications.run_application(["walker", "--width", "644", "--maxheight", "300", "--minheight", "300", *args])


    def clipboard(self, args: list[str]) -> int | None:
        self.remember_clipboard_target()
        width, height = 1000, 520
        try:
            monitors = json.loads(self.ctx.processes.run(["hyprctl", "-j", "monitors"], capture=True, check=False).stdout)
            monitor = next((m for m in monitors if m.get("focused")), monitors[0] if monitors else {})
            scale = max(float(monitor.get("scale", 1)), 0.1)
            w, h = monitor.get("width", 1920), monitor.get("height", 1080)
            if monitor.get("transform", 0) in (1, 3, 5, 7):
                w, h = h, w
            width, height = min(width, int(w / scale) - 80), min(height, int(h / scale) - 160)
        except (OSError, ValueError, TypeError):
            pass
        compact = width < 900 or height < 460
        width, height = max(360, width), max(300, height)
        self.ctx.processes.run(["systemctl", "--user", "start", "desktop-elephant.service", "desktop-walker.service"], check=False)
        modules = [] if any(x in args for x in ("-m", "--modules")) else ["-m", "clipboard"]
        self.applications.run_application(["walker", "--theme", "desktop-clipboard-compact" if compact else "desktop-clipboard",
                         "--width", str(width), "--height", str(height),
                         *modules, *args])


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
        # Wait for Walker to release keyboard focus before delivering the shortcut.
        deadline = self.ctx.clock.monotonic() + 2
        while self.ctx.clock.monotonic() < deadline:
            layers = json.loads(self.ctx.processes.run(["hyprctl", "-j", "layers"], capture=True).stdout)
            if not any(layer.get("namespace") == "walker"
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
