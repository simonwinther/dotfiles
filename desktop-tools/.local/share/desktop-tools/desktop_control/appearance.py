"""Appearance component with injected dependencies."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import re
import sys
from dataclasses import dataclass
from typing import TYPE_CHECKING
from .rendering import colors, hyprlock_palette
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .catalog import ThemeCatalog
    from .theme_render import ThemeRenderer
    from .services import Services
    from .wallpaper import Wallpaper


@dataclass
class Appearance:
    ctx: Context
    store: StateStore
    catalog: ThemeCatalog
    renderer: ThemeRenderer
    services: Services
    wallpaper: Wallpaper

    def font(self, args: list[str]) -> int | None:
        action = args[0] if args else "current"
        if action == "current":
            print(self.catalog.selected_font())
            return
        families = sorted(set(self.ctx.processes.run(["fc-list", ":spacing=mono", "--format", "%{family[0]}\n"], capture=True).stdout.splitlines()))
        if action == "list":
            print("\n".join(families))
            return
        if action != "set" or len(args) not in (2, 3) or args[1] not in families or (len(args) == 3 and args[2] != "--no-reload"):
            raise ValueError("font: list, current, or set an installed monospace font")
        with self.store.state_lock():
            value = self.store.state(); value["font"] = args[1]
            self.renderer._prepare_theme(value)
            self.store.save_state(value)
        if "--no-reload" not in args:
            self.ctx.processes.run(["gsettings", "set", "org.gnome.desktop.interface", "monospace-font-name", args[1] + " 11"], check=False)
            self.ctx.processes.run(["makoctl", "reload"], check=False)
            self.ctx.processes.run(["pkill", "-USR2", "-u", str(self.ctx.uid), "-x", "ghostty"], check=False)
            self.ctx.processes.run(["pkill", "-USR1", "-u", str(self.ctx.uid), "-x", "kitty"], check=False)
            self.services.restart("theme")


    def theme(self, args: list[str]) -> int | None:
        action = args[0] if args else "current"
        if action == "list":
            print("\n".join(sorted({p.name for d in self.catalog.theme_directories() if d.is_dir() for p in d.iterdir() if p.is_dir()})))
        elif action == "current":
            print(self.catalog.selected_theme())
        elif action in ("init", "set"):
            with self.store.state_lock():
                name = args[1] if action == "set" and len(args) > 1 else self.catalog.selected_theme()
                value = self.store.state(); value["theme"] = name
                if action == "set":
                    value["terminalTheme"] = True
                    value["desktopTheme"] = True
                source, _, light = self.renderer._prepare_theme(value)
                self.store.save_state(value)
            if action == "set" and "--no-reload" not in args:
                self.apply_appearance(source, light)
                self.ctx.processes.run(["hyprctl", "reload"], check=False)
                self.ctx.processes.run(["makoctl", "reload"], check=False)
                self.ctx.processes.run(["pkill", "-USR2", "-u", str(self.ctx.uid), "-x", "ghostty"], check=False)
                self.ctx.processes.run(["pkill", "-USR1", "-u", str(self.ctx.uid), "-x", "kitty"], check=False)
                self.services.restart("theme")
        elif action == "generate":
            image_args = [x for x in args[1:] if not x.startswith("--")]
            if not image_args:
                chosen = self.ctx.processes.run(["zenity", "--file-selection", "--title=Choose a wallpaper for the new palette"], check=False, capture=True)
                if chosen.returncode or not chosen.stdout.strip():
                    return
                image_args = [chosen.stdout.strip()]
            if len(image_args) != 1:
                raise ValueError("usage: desktopctl theme generate [IMAGE] [--light] [--no-reload]")
            image = self.ctx.expand(image_args[0]).resolve()
            if not image.is_file():
                raise ValueError(f"Missing image: {image}")
            mode = "light" if "--light" in args else "dark"
            config = self.ctx.paths.runtime / "matugen.toml"
            storage.atomic_write(config, "[config]\n[templates]\n")
            result = self.ctx.processes.run(["matugen", "image", image, "--dry-run", "--json", "hex", "--config", config,
                          "--source-color-index", "0", "--mode", mode], capture=True)
            roles = json.loads(result.stdout)["colors"]
            def role(name):
                value = roles[name][mode]["color"]
                if not re.fullmatch(r"#[0-9a-fA-F]{6}", value):
                    raise ValueError("Matugen returned an invalid color")
                return value
            stem = re.sub(r"[^a-z0-9]+", "-", image.stem.lower()).strip("-")[:32] or "wallpaper"
            name = f"generated-{stem}-{hashlib.sha256(image.read_bytes()).hexdigest()[:8]}-{mode}"
            directory = self.ctx.paths.data / "desktop/themes" / name
            palette = {"background": role("background"), "foreground": role("on_surface"), "accent": role("primary"), "cursor": role("primary")}
            ansi = ["surface_container_lowest", "error", "tertiary", "secondary", "primary", "tertiary", "inverse_primary", "on_surface_variant",
                    "surface_bright", "error", "tertiary", "secondary", "primary", "tertiary", "inverse_primary", "on_surface"]
            palette.update({f"color{i}": role(key) for i, key in enumerate(ansi)})
            storage.atomic_write(directory / "colors.toml", "\n".join(f'{key} = "{value}"' for key, value in palette.items()) + "\n")
            if mode == "light":
                storage.atomic_write(directory / "light.mode", "")
            self.theme(["set", name, *(["--no-reload"] if "--no-reload" in args else [])])
            if "--no-reload" not in args:
                self.wallpaper.wallpaper(["set", str(image)])
            print(name)
        else:
            raise ValueError("theme: list, current, init, set NAME [--no-reload], or generate IMAGE")


    def apply_appearance(self, source: Path, light: bool) -> None:
        values = dict([
            ("color-scheme", "prefer-light" if light else "prefer-dark"),
            ("gtk-theme", "Adwaita" if light else "Adwaita-dark"),
            ("icon-theme", (source / "icons.theme").read_text().strip() if (source / "icons.theme").exists() else "Yaru-magenta"),
        ])
        values.update(storage.read_json(self.ctx.paths.config / "desktop/interface.json"))
        if self.store.state().get("font"):
            values["monospace-font-name"] = self.catalog.selected_font() + " 11"
        for key, value in values.items():
            self.ctx.processes.run(["gsettings", "set", "org.gnome.desktop.interface", key, value], check=False)


    def lock_config(self) -> str:
        source = (self.ctx.paths.config / "hypr/hyprlock.conf").read_text().replace("~/.local/state/desktop", str(self.ctx.paths.state))
        include = f"source = {self.ctx.paths.state / 'theme/hyprlock.conf'}"
        # Login can precede desktop preparation; no generated theme files are required.
        fallback = source.replace(include, hyprlock_palette({
            "background": "#000000", "foreground": "#a9b1d6", "accent": "#449dab",
        }).rstrip())
        try:
            # Read a snapshot without recovery, migration or the shared appearance
            # lock: an in-flight theme change must not hold up login or suspend.
            preferences = storage.read_json(self.ctx.paths.state / "state.json")
            defaults = self.store.settings()
            theme = self.catalog.theme_path(preferences.get("theme", defaults.get("defaultTheme", "tokyoled")))
            template = theme / "hyprlock.conf"
            palette = template.read_text() if template.is_file() else hyprlock_palette(colors(theme))
            themed = source.replace(include, palette.rstrip())
            family = preferences.get("font", defaults.get("defaultFont", "JetBrainsMono Nerd Font"))
            if (not isinstance(family, str) or not family.strip()
                    or re.search(r"[\x00-\x1f\x7f-\x9f\ud800-\udfff]", family)):
                raise ValueError("Invalid lock screen font")
            themed = re.sub(r"(?m)^(\s*font_family\s*=).*$", lambda m: m[1] + " " + family, themed)
            image = preferences.get("lockBackground")
            if image is not None and not isinstance(image, str):
                raise ValueError("Invalid lock screen background")
            if image and self.ctx.expand(image).is_file():
                storage.atomic_link(self.ctx.paths.state / "lock-background", self.ctx.expand(image).resolve())
                themed = themed.replace(str(self.ctx.paths.state / "background"), str(self.ctx.paths.state / "lock-background"))
            return themed
        except (OSError, ValueError, TypeError, AttributeError) as error:
            # Invalid JSON shapes, missing palettes and unwritable appearance state
            # must not prevent the locker from starting. Locker failures still propagate.
            print(f"desktopctl: lock appearance unavailable; using defaults: {error}", file=sys.stderr)
            return fallback


    def appearance(self, args: list[str]) -> int | None:
        if args[:1] == ["terminal-colors"] and args[1:] in (["personal"], ["desktop"]):
            with self.store.state_lock():
                value = self.store.state(); value["terminalTheme"] = args[1] == "desktop"
                self.renderer._prepare_theme(value)
                self.store.save_state(value)
            self.ctx.processes.run(["pkill", "-USR2", "-u", str(self.ctx.uid), "-x", "ghostty"], check=False)
            self.services.restart("theme")
        elif args == ["lock-background", "reset"]:
            with self.store.state_lock():
                value = self.store.state(); value.pop("lockBackground", None); self.store.save_state(value)
        elif args[:1] == ["lock-background"]:
            if len(args) > 1:
                image = self.ctx.expand(args[1]).resolve()
            else:
                result = self.ctx.processes.run(["zenity", "--file-selection", "--title=Choose lock screen background", "--file-filter=Images | *.png *.jpg *.jpeg *.webp"], capture=True, check=False)
                if result.returncode or not result.stdout.strip():
                    return
                image = self.ctx.expand(result.stdout.strip()).resolve()
            if not image.is_file() or image.suffix.lower() not in (".png", ".jpg", ".jpeg", ".webp"):
                raise ValueError("Choose an existing PNG, JPEG or WebP image")
            with self.store.state_lock():
                value = self.store.state(); value["lockBackground"] = str(image); self.store.save_state(value)
        else:
            raise ValueError("appearance: terminal-colors personal|desktop, or lock-background [IMAGE|reset]")
