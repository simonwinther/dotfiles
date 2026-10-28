"""ThemeCatalog component with injected dependencies."""
from __future__ import annotations
from pathlib import Path
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore


@dataclass
class ThemeCatalog:
    ctx: Context
    store: StateStore

    def theme_directories(self) -> list[Path]:
        return [self.ctx.expand(x) for x in self.store.settings().get("themeDirectories", [str(self.ctx.paths.config / "desktop/themes")])] + [self.ctx.paths.data / "desktop/themes"]


    def theme_path(self, name: str) -> Path:
        if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name):
            raise ValueError("Theme names use lowercase letters, digits, underscores, and hyphens")
        for directory in self.theme_directories():
            candidate = directory / name
            if candidate.is_dir():
                return candidate
        raise ValueError(f"Unknown theme: {name}")


    def selected_theme(self) -> str:
        return self.store.state().get("theme", self.store.settings().get("defaultTheme", "tokyoled"))


    def selected_font(self) -> str:
        return self.store.state().get("font", self.store.settings().get("defaultFont", "JetBrainsMono Nerd Font"))
