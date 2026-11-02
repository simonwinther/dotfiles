"""Small on-demand catalogs for the native desktop shell."""

import configparser
import hashlib
import json
import locale
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import urllib.parse
import urllib.request


def menus_path(config, environment=None):
    environment = os.environ if environment is None else environment
    return Path(environment.get("DESKTOP_MENUS_FILE") or config / "desktop/menus.json")


def data_directories(data):
    return list(dict.fromkeys([data, *map(Path, os.environ.get(
        "XDG_DATA_DIRS", "/usr/local/share:/usr/share"
    ).split(":"))]))


def application_command(command):
    """Keep launched applications independent of the menu or bar that opened them."""
    command = list(command)
    if not command:
        raise ValueError("An application command is required")
    if Path(command[0]).name in {"uwsm-app", "uwsm", "app2unit", "systemd-run"}:
        return command
    if shutil.which("uwsm-app"):
        return ["uwsm-app", "--", *command]
    if shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "--collect", "--quiet", "--", *command]
    return command


def application_entries(data):
    seen = set()
    result = []
    icons = IconLookup(data)
    languages = [os.environ.get("LC_MESSAGES", locale.getlocale()[0] or "en").split(".")[0]]
    languages += [languages[0].split("_")[0]]
    desktops = set(os.environ.get("XDG_CURRENT_DESKTOP", "Hyprland").split(":"))
    for directory in data_directories(data):
        root = directory / "applications"
        for path in sorted(root.rglob("*.desktop")) if root.is_dir() else []:
            identity = str(path.relative_to(root)).replace("/", "-")
            if identity in seen:
                continue
            seen.add(identity)
            parser = configparser.ConfigParser(interpolation=None, strict=False)
            try:
                parser.read(path, encoding="utf-8")
                entry = parser["Desktop Entry"]
                if entry.get("Type", "Application") != "Application" or any(
                    entry.getboolean(key, False) for key in ("Hidden", "NoDisplay")
                ):
                    continue
                if entry.get("TryExec") and not shutil.which(entry["TryExec"]):
                    continue
                only = set(filter(None, entry.get("OnlyShowIn", "").split(";")))
                excluded = set(filter(None, entry.get("NotShowIn", "").split(";")))
                if (only and not only.intersection(desktops)) or excluded.intersection(desktops):
                    continue
                def translated(key):
                    return next((entry[f"{key}[{language}]"] for language in languages
                                 if f"{key}[{language}]" in entry), entry.get(key, ""))
                name = translated("Name")
                if not name or not (entry.get("Exec") or entry.getboolean("DBusActivatable", False)):
                    continue
                result.append({
                    "id": identity, "name": name,
                    "detail": translated("GenericName") or translated("Comment"),
                    "keywords": translated("Keywords"),
                    "image": icons.path(entry.get("Icon", "")),
                    "command": ["bar", "launch", str(path.resolve())],
                })
            except (OSError, ValueError, KeyError, configparser.Error):
                continue
    return sorted(result, key=lambda item: item["name"].casefold())


class IconLookup:
    """Resolve app icons once per scan without probing absent directories per app."""

    def __init__(self, data):
        self.locations = []
        self.resolved = {"": ""}
        for directory in data_directories(data):
            for theme in ("hicolor", "Adwaita", "Yaru"):
                for size in ("scalable", "48x48", "32x32", "64x64", "256x256", "symbolic"):
                    path = directory / "icons" / theme / size / "apps"
                    if path.is_dir():
                        self.locations.append((path, (".svg", ".png")))
            path = directory / "pixmaps"
            if path.is_dir():
                self.locations.append((path, (".svg", ".png", "")))

    def path(self, name):
        if name not in self.resolved:
            source = Path(name)
            if source.is_absolute():
                self.resolved[name] = name if source.is_file() else ""
            else:
                self.resolved[name] = next((str(path) for directory, suffixes in self.locations
                    for suffix in suffixes if (path := directory / (name + suffix)).is_file()), "")
        return self.resolved[name]


def thumbnail(source, cache, fit=False):
    source = Path(source).resolve(strict=True)
    stat = source.stat()
    identity = f"{source}:{stat.st_mtime_ns}:{stat.st_size}" + (":fit" if fit else "")
    key = hashlib.sha256(identity.encode()).hexdigest()
    directory = cache / "desktop/bar-thumbnails"
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    directory.chmod(0o700)
    target = directory / (key + ".png")
    if not target.exists():
        with tempfile.TemporaryDirectory(prefix=".thumbnail-", dir=directory) as temporary:
            output = Path(temporary) / "image.png"
            subprocess.run([
                "magick", "-limit", "memory", "64MiB", "-limit", "map", "128MiB",
                "-define", "jpeg:size=640x360",
                str(source) + "[0]", "-auto-orient", "-thumbnail", "320x180" if fit else "320x180^",
                "-background", "none", "-gravity", "center", "-extent", "320x180", str(output),
            ], check=True, capture_output=True, timeout=15)
            output.chmod(0o600)
            os.replace(output, target)
    target.touch()
    for old in sorted(directory.glob("*.png"), key=lambda path: path.stat().st_mtime, reverse=True)[256:]:
        old.unlink(missing_ok=True)
    return str(target)


def cover(url, cache):
    """Fetch bounded artwork only when a player changes its cover URL."""
    parsed = urllib.parse.urlsplit(url)
    directory = cache / "desktop/bar-covers"
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    directory.chmod(0o700)
    key = hashlib.sha256(url.encode()).hexdigest()
    target = directory / (key + ".png")
    if target.is_file():
        return str(target)
    limit = 8 * 1024 * 1024
    if parsed.scheme == "file" and parsed.netloc in ("", "localhost"):
        source = Path(urllib.request.url2pathname(parsed.path))
        if source.stat().st_size > limit:
            raise ValueError("Artwork exceeds the size limit")
        contents = source.read_bytes()
    elif parsed.scheme in ("https", "http"):
        request = urllib.request.Request(url, headers={"User-Agent": "Desktop-Bar/1.0"})
        with urllib.request.urlopen(request, timeout=4) as response:
            if urllib.parse.urlsplit(response.url).scheme not in ("https", "http"):
                raise ValueError("Unsupported artwork redirect")
            contents = response.read(limit + 1)
    else:
        raise ValueError("Unsupported artwork URL")
    if len(contents) > limit:
        raise ValueError("Artwork exceeds the size limit")
    with tempfile.TemporaryDirectory(prefix=".cover-", dir=directory) as temporary:
        source = Path(temporary) / "source"
        source.write_bytes(contents)
        output = Path(temporary) / "cover.png"
        subprocess.run(["magick", "-limit", "memory", "64MiB", "-limit", "map", "128MiB", "-limit", "width", "8192", "-limit", "height", "8192", str(source) + "[0]", "-auto-orient", "-thumbnail", "128x128^", "-gravity", "center", "-extent", "128x128", "PNG32:" + str(output)], check=True, timeout=5, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        output.chmod(0o600)
        output.replace(target)
    for old in sorted(directory.glob("*.png"), key=lambda path: path.stat().st_mtime, reverse=True)[32:]:
        old.unlink(missing_ok=True)
    return str(target)


def handle(args, *, preferences, config, data, cache, theme_directories, theme_path,
           theme_backgrounds, selected_theme, environment=None):
    """Return whether this is an internal catalog command, shared by both CLIs."""
    if args[:1] == ["cover"] and len(args) == 2:
        print(cover(args[1], cache))
    elif args[:1] == ["thumbnail"] and (len(args) == 2 or len(args) == 3 and args[2] == "fit"):
        print(thumbnail(args[1], cache, fit=len(args) == 3))
    elif args[:1] == ["launch"] and len(args) == 2:
        path = Path(args[1]).resolve(strict=True)
        if path.suffix != ".desktop":
            raise ValueError("Choose an installed desktop application")
        subprocess.Popen(application_command(["gio", "launch", str(path)]), start_new_session=True,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    elif args[:1] == ["catalog"] and len(args) == 2:
        kind = args[1]
        if kind.startswith("menu/"):
            menus = json.loads(menus_path(config, environment).read_text())
            menu = menus.get(kind.removeprefix("menu/"), {})
            group = kind.removeprefix("menu/")
            if group == "font":
                result = subprocess.run(["fc-list", ":spacing=mono", "--format", "%{family[0]}\n"], capture_output=True, text=True, check=True, timeout=5)
                menu = {"items": {name: ["font", "set", name] for name in sorted(set(result.stdout.splitlines())) if name}}
            elif group == "power-profile":
                result = subprocess.run(["powerprofilesctl", "list"], capture_output=True, text=True, check=True, timeout=5)
                import re
                menu = {"items": {name.replace("-", " ").title(): ["exec", "powerprofilesctl", "set", name] for name in ("power-saver", "balanced", "performance") if re.search(r"(?m)^\s*\*?\s*" + name + r":", result.stdout)}}
            elif group == "config":
                menu = {"items": {name: ["config", name] for name in ["Hyprland configuration", "Hyprland", "Keybindings", "Input", "Monitors", "Hypridle", "Hyprlock", "Night light", "SwayOSD", "Desktop search", "Weather", "XCompose", "Desktop menu"]}}
            elif group in ("hardware", "toggle"):
                result = subprocess.run(["hyprctl", "-j", "devices"], capture_output=True, text=True, check=True, timeout=3)
                if not any("touchpad" in item.get("name", "").lower() for item in json.loads(result.stdout).get("mice", [])):
                    menu = {**menu, "items": {label: command for label, command in menu.get("items", {}).items() if command != ["toggle", "touchpad"]}}
            entries = [{"id": label, "name": label, "command": command}
                       for label, command in menu.get("items", {}).items()]
        elif kind == "controls":
            path = config / "desktop/bar-actions.json"
            entries = json.loads(path.read_text()) if path.is_file() else []
            if not isinstance(entries, list) or any(
                not isinstance(entry, dict) or not isinstance(entry.get("name"), str)
                or not isinstance(entry.get("command"), list) or not entry["command"]
                or not all(isinstance(arg, str) and arg for arg in entry["command"])
                for entry in entries
            ):
                raise ValueError("bar-actions.json must contain named actions with argument lists")
        elif kind == "apps":
            entries = application_entries(data)
        elif kind == "themes":
            names = sorted({path.name for root in theme_directories() if root.is_dir()
                            for path in root.iterdir() if path.is_dir()})
            entries = []
            for name in names:
                try:
                    palette = tomllib.loads((theme_path(name) / "colors.toml").read_text())
                    entries.append({"id": name, "name": name.replace("-", " ").title(),
                                    "colors": [palette.get(f"color{i}", "#555555") for i in range(1, 7)],
                                    "selected": name == selected_theme(),
                                    "command": ["theme", "set", name]})
                except (OSError, ValueError):
                    continue
        elif kind == "wallpapers":
            entries = [{"id": str(path), "name": path.stem.replace("-", " "),
                        "detail": path.name, "image": str(path),
                        "selected": str(path) == preferences.get("wallpaper"),
                        "command": ["wallpaper", "set", str(path)]}
                       for path in theme_backgrounds(selected_theme())]
        else:
            raise ValueError("Unknown native panel catalog")
        print(json.dumps(entries))
    else:
        return False
    return True
