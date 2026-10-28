"""Services component with injected dependencies."""
from __future__ import annotations
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context


@dataclass
class Services:
    ctx: Context

    def selected_units(self, units: list[str]) -> list[str]:
        if not storage.read_json(self.ctx.paths.config / "desktop/bar-outputs.json").get("minimal", ["eDP-1"]):
            return [unit for unit in units if unit != "desktop-waybar-minimal"]
        return units


    def restart(self, group: str) -> int:
        groups = {
            "launcher": ["desktop-elephant", "desktop-walker"],
            "bar": ["desktop-waybar", "desktop-waybar-minimal", "desktop-workspaces"],
            "theme": ["desktop-walker", "desktop-waybar", "desktop-waybar-minimal", "desktop-swayosd"],
        }
        if group not in groups:
            raise ValueError(f"Unknown service group: {group}")
        return self.ctx.processes.run(["systemctl", "--user", "try-restart", *[x + ".service" for x in self.selected_units(groups[group])]], check=False).returncode
