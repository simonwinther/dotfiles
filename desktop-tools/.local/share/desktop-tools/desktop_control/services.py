"""Services component with injected dependencies."""
from __future__ import annotations
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .context import Context


@dataclass
class Services:
    ctx: Context

    def restart(self, group: str) -> int:
        groups = {
            "launcher": ["desktop-search", "desktop-bar"],
            "bar": ["desktop-bar"],
            "theme": ["desktop-swayosd", "desktop-bar"],
        }
        if group not in groups:
            raise ValueError(f"Unknown service group: {group}")
        return self.ctx.processes.run(["systemctl", "--user", "try-reload-or-restart" if group == "theme" else "try-restart", *[x + ".service" for x in groups[group]]], check=False).returncode
