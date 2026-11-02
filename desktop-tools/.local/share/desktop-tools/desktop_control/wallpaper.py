"""Wallpaper component with injected dependencies."""
from __future__ import annotations
from dataclasses import dataclass
import re
import bar_config
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore


@dataclass
class Wallpaper:
    ctx: Context
    store: StateStore

    def apply_wallpaper(self, command, image, output, duration) -> None:
        command = list(command)
        for attempt in range(2):
            if attempt:
                # A compositor reload can resize an output after awww accepted the image.
                command[command.index("--transition-type") + 1] = "none"
            self.ctx.processes.run([*command, image])
            settling = 1 if command[command.index("--transition-type") + 1] == "none" else max(1, float(duration) + .5)
            deadline = self.ctx.clock.monotonic() + settling + 2
            confirmed_since = None
            while True:
                result = self.ctx.processes.run(["awww", "query"], check=False, capture=True)
                outputs = [line for line in result.stdout.splitlines()
                           if not output or re.search(r"(?:^|: )" + re.escape(output) + r": ", line)]
                now = self.ctx.clock.monotonic()
                if not result.returncode and outputs and all(
                        line.endswith(f"currently displaying: image: {image}") for line in outputs):
                    if confirmed_since is None:
                        confirmed_since = now
                    # Output reconfiguration can launch a delayed cache restore after acceptance.
                    if now - confirmed_since >= settling:
                        return
                else:
                    confirmed_since = None
                if now >= deadline:
                    break
                self.ctx.clock.sleep(.05)
        raise ValueError(f"The wallpaper renderer did not display: {image}")

    def wallpaper(self, args: list[str]) -> int | None:
        action = args[0] if args else "current"
        value = self.store.state()
        if action == "current":
            print(value.get("wallpaper", self.store.settings().get("defaultWallpaper", "")))
            return
        if action == "grid":
            if bar_config.open_panel(self.ctx.paths.runtime, "wallpapers", value):
                return
            self.ctx.processes.execvp("background-grid", ["background-grid"])
        choices = sorted(p for d in self.store.settings().get("wallpaperDirectories", [str(self.ctx.paths.data / "wallpapers")])
                         for p in self.ctx.expand(d).rglob("*") if p.is_file() and p.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".avif"})
        if action == "next":
            if not choices:
                raise ValueError("No wallpapers found")
            current = self.ctx.expand(value.get("wallpaper", ""))
            image = choices[(choices.index(current) + 1) % len(choices)] if current in choices else choices[0]
        elif action == "restore":
            image = self.ctx.expand(value.get("wallpaper", self.store.settings().get("defaultWallpaper", "")))
        elif action == "set" and len(args) >= 2:
            image = self.ctx.expand(args[1]).resolve()
        else:
            raise ValueError("wallpaper: current, grid, next, restore, or set IMAGE [OUTPUT]")
        if not image.is_file():
            raise ValueError(f"Wallpaper is missing: {image}")
        output = args[2] if action == "set" and len(args) > 2 else None
        if self.ctx.processes.run(["awww", "query"], check=False, capture=True).returncode:
            managed = self.ctx.processes.run(["systemctl", "--user", "start", "desktop-awww.service"], check=False, capture=True)
            if managed.returncode:
                self.ctx.processes.background(["awww-daemon", "--no-cache"])
            for _ in range(80):
                if not self.ctx.processes.run(["awww", "query"], check=False, capture=True).returncode:
                    break
                self.ctx.clock.sleep(.05)
            else:
                raise ValueError("The wallpaper daemon did not become ready")
        transition = storage.read_json(self.ctx.paths.config / "background-grid/transition.json", {"type": "any", "duration": 2, "fps": 60, "step": 80})
        command = ["awww", "img", "--resize", "crop", "--transition-type", "none" if action == "restore" else transition['type'],
                   "--transition-duration", str(transition['duration']), "--transition-fps", str(transition['fps']), "--transition-step", str(transition['step'])]
        if output:
            command += ["--outputs", output]
        self.apply_wallpaper(command, image, output, transition['duration'])
        with self.store.state_lock():
            value = self.store.state(); value["wallpaper"] = str(image)
            value.setdefault("wallpapers", {})[output or "*"] = str(image)
            self.store.save_state(value)
            storage.atomic_link(self.ctx.paths.state / "background", image.resolve())
        if action == "restore":
            for monitor, saved in value.get("wallpapers", {}).items():
                if monitor != "*" and self.ctx.expand(saved).is_file():
                    self.ctx.processes.run(["awww", "img", "--outputs", monitor, "--transition-type", "none", saved], check=False)
