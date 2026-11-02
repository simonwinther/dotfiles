"""ThemeCatalog component with injected dependencies."""
from __future__ import annotations
from pathlib import Path
import bar_config
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


    def theme_backgrounds(self, name: str) -> list[Path]:
        directories = (self.theme_path(name) / "backgrounds", self.ctx.paths.data / "wallpapers" / name)
        backgrounds = sorted({p.resolve() for directory in directories for p in directory.rglob("*")
                              if p.is_file() and p.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".avif"}})
        defaults = self.store.settings()
        if name == defaults.get("defaultTheme", "tokyoled") and defaults.get("defaultWallpaper"):
            preferred = self.ctx.expand(defaults["defaultWallpaper"]).resolve()
            if preferred in backgrounds:
                backgrounds.remove(preferred)
                backgrounds.insert(0, preferred)
        return backgrounds


    def selected_font(self) -> str:
        return self.store.state().get("font", self.store.settings().get("defaultFont", "JetBrainsMono Nerd Font"))

    DEFAULT_BAR = bar_config.DEFAULT_BAR

    def bar_names(self) -> list[str]:
        return bar_config.names()

    def selected_bar(self) -> str:
        return bar_config.selected(self.store.state())
