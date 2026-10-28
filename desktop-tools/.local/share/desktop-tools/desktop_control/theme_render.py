"""ThemeRenderer component with injected dependencies."""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import uuid
from dataclasses import dataclass, field
from typing import Any, TYPE_CHECKING
from .rendering import _theme_colors, hyprlock_palette, prepare_terminal_theme
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .catalog import ThemeCatalog


THEME_FILES = ("walker.css", "swayosd.css", "waybar.css", "mako.ini", "hyprland.conf", "hyprlock.conf",
               "btop.theme", "eza.yml")



@dataclass
class ThemeRenderer:
    ctx: Context
    store: StateStore
    catalog: ThemeCatalog
    renderer_digest: str = field(init=False)

    def __post_init__(self) -> None:
        folder = Path(__file__).parent
        self.renderer_digest = hashlib.sha256(b"".join((folder / name).read_bytes() for name in ("rendering.py", "theme_render.py"))).hexdigest()

    def prepare_theme(self, name: str | None=None, *, follow_terminal: bool | None=None) -> tuple[Path, dict[str, str], bool]:
        # Every launch path uses this lock; state-changing callers already holding
        # it use the private helper with their pending preference snapshot.
        with self.store.state_lock():
            return self._prepare_theme(self.store.state(), name, follow_terminal=follow_terminal)


    def _prepare_theme(self, value: dict[str, Any], name: str | None=None, *, follow_terminal: bool | None=None) -> tuple[Path, dict[str, str], bool]:
        defaults = self.store.settings()
        name = name or value.get("theme", defaults.get("defaultTheme", "tokyoled"))
        source = self.catalog.theme_path(name)
        inputs = {
            "renderer": self.renderer_digest,
            "name": name,
            "font": value.get("font", defaults.get("defaultFont", "JetBrainsMono Nerd Font")),
            "terminalTheme": value.get("terminalTheme", False) if follow_terminal is None else follow_terminal,
            "light": (source / "light.mode").exists(),
            "files": {filename: (source / filename).read_text() if (source / filename).is_file() else None
                      for filename in THEME_FILES},
            "colors": (source / "colors.toml").read_text(),
            "walker": {},
        }
        template_root = self.ctx.paths.config / "walker/templates"
        for template in sorted(template_root.iterdir()) if template_root.is_dir() else []:
            if template.is_dir():
                inputs["walker"][template.name] = {item.name: item.read_text() for item in sorted(template.iterdir())
                                                    if item.is_file()}
        c = _theme_colors(inputs["colors"], name)
        key = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode()).hexdigest()
        target = self.ctx.paths.state / "theme"
        generation = target.resolve()
        if not target.is_symlink() or not self._cached_theme(generation, key):
            generations = self.ctx.paths.state / "theme-generations"
            generations.mkdir(parents=True, exist_ok=True, mode=0o700)
            with tempfile.TemporaryDirectory(prefix=".staging-", dir=generations) as directory:
                staged = Path(directory) / "theme"
                staged.mkdir(mode=0o700)
                self._render_theme(staged, inputs, c)
                paths = sorted(staged.rglob("*"))
                files = {str(path.relative_to(staged)): hashlib.sha256(path.read_bytes()).hexdigest()
                         for path in paths if path.is_file()}
                directories = [str(path.relative_to(staged)) for path in paths if path.is_dir()]
                storage.atomic_write(staged / ".generation.json", json.dumps({"key": key, "files": files,
                                                                      "directories": directories}) + "\n")
                # Keep completed generations so readers can pin a resolved path
                # while another process publishes the next theme.
                generation = generations / (key + "-" + uuid.uuid4().hex)
                staged.rename(generation)
        # These fixed aliases all traverse the one publication pointer. They are
        # also repaired on cache hits after XDG_RUNTIME_DIR has been recreated.
        self._theme_aliases(target)
        self._publish_theme_link(target, generation)
        config_dirs = self.ctx.env.get("XDG_CONFIG_DIRS", "/etc/xdg").split(":")
        self.ctx.env["XDG_CONFIG_DIRS"] = ":".join([str(self.ctx.paths.runtime), *[p for p in config_dirs if p != str(self.ctx.paths.runtime)]])
        return source, c, inputs["light"]


    def _cached_theme(self, generation: Path, key: str) -> bool:
        try:
            manifest = storage.read_json(generation / ".generation.json")
            return (manifest.get("key") == key and bool(manifest.get("files"))
                    and "walker/themes" in manifest.get("directories", [])
                    and all((generation / path).is_dir() and not (generation / path).is_symlink()
                            for path in manifest["directories"])
                    and all(not (generation / path).is_symlink()
                            and hashlib.sha256((generation / path).read_bytes()).hexdigest() == digest
                            for path, digest in manifest["files"].items()))
        except (OSError, ValueError, AttributeError, TypeError):
            return False


    def _publish_theme_link(self, path: Path, target: Path) -> None:
        if path.is_symlink() and path.readlink() == target:
            return
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        with tempfile.TemporaryDirectory(prefix=f".{path.name}.", dir=path.parent) as directory:
            pending = Path(directory) / "link"
            pending.symlink_to(target)
            if path.is_dir() and not path.is_symlink():
                # Linux rename exchange migrates the old nonempty generated
                # directory without ever making its public path disappear.
                import ctypes
                libc = ctypes.CDLL(None, use_errno=True)
                exchange = libc.renameat2
                exchange.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
                exchange.restype = ctypes.c_int
                if exchange(-100, os.fsencode(pending), -100, os.fsencode(path), 2):
                    error = ctypes.get_errno()
                    raise OSError(error, os.strerror(error), path)
            else:
                os.replace(pending, path)


    def _theme_aliases(self, target: Path) -> None:
        aliases = ((self.ctx.paths.state / "fontconfig.conf", target / "fontconfig.conf"),
                   (self.ctx.paths.runtime / "walker/themes", target / "walker/themes"))
        for alias, destination in aliases:
            # Seed the old directory before converting its aliases so migration
            # readers keep seeing the old content until the pointer switches.
            if target.is_dir() and not target.is_symlink() and alias.exists() and not alias.is_symlink():
                destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                if alias.is_dir():
                    shutil.copytree(alias, destination, dirs_exist_ok=True)
                else:
                    storage.atomic_write(destination, alias.read_bytes())
            self._publish_theme_link(alias, destination)


    def _render_theme(self, target: Path, inputs: dict[str, Any], c: dict[str, str]) -> None:
        family = inputs["font"]
        css = {
            "base": c["background"], "text": c["foreground"], "border": c["accent"],
            "selected-text": c["accent"], "background-color": c["background"],
            "border-color": c["accent"], "label": c["foreground"], "image": c["foreground"],
            "progress": c["accent"], "foreground": c["foreground"], "background": c["background"],
        }
        css_text = "\n".join(f"@define-color {k} {v};" for k, v in css.items()) + "\n"
        templates = {
            "walker.css": css_text,
            "swayosd.css": css_text,
            "waybar.css": css_text,
            "mako.ini": f"background-color={c['background']}\ntext-color={c['foreground']}\nborder-color={c['accent']}\nborder-size=2\nfont=JetBrainsMono Nerd Font 11\n",
            "hyprland.conf": "general {\n    col.active_border = rgba(" + c['accent'][1:] + "ee)\n    col.inactive_border = rgba(" + c['color8'][1:] + "aa)\n}\n",
            "hyprlock.conf": hyprlock_palette(c),
        }
        for filename, fallback in templates.items():
            original = inputs["files"][filename]
            content = original if original is not None else fallback
            if filename == "mako.ini":
                content = re.sub(r"(?m)^font=.*$", lambda _: "font=" + family + " 11", content)
            storage.atomic_write(target / filename, content)
        prepare_terminal_theme(target, c, inputs["terminalTheme"], family)
        storage.atomic_write(target / "colors.toml", inputs["colors"])
        fallback = "\n".join(f'theme[{key}]="{value}"' for key, value in {
            "main_bg": c["background"], "main_fg": c["foreground"], "title": c["foreground"],
            "hi_fg": c["accent"], "selected_bg": c["accent"], "selected_fg": c["background"],
        }.items()) + "\n"
        btop = inputs["files"]["btop.theme"]
        storage.atomic_write(target / "current.theme", btop if btop is not None else fallback)
        eza = inputs["files"]["eza.yml"]
        if eza is not None:
            storage.atomic_write(target / "theme.yml", eza)
        if inputs["light"]:
            storage.atomic_write(target / "light.mode", "")
        storage.atomic_write(target / "name", inputs["name"] + "\n")
        walker_root = target / "walker/themes"
        walker_root.mkdir(parents=True, mode=0o700)
        for template, items in inputs["walker"].items():
            walker = walker_root / ("desktop-" + template)
            walker.mkdir(mode=0o700)
            for filename, content in items.items():
                if filename == "style.css":
                    content = (target / "walker.css").read_text() + "\n" + content
                    content += "\n* { font-family: " + json.dumps(family) + ", monospace; }\n"
                storage.atomic_write(walker / filename, content)
