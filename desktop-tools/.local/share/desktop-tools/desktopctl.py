#!/usr/bin/env python3
"""Portable desktop coordination. Configuration is read-only; choices are XDG state."""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tomllib
import uuid
from xml.sax.saxutils import escape as xml_escape

HOME = Path.home()
CONFIG = Path(os.environ.get("XDG_CONFIG_HOME", HOME / ".config"))
DATA = Path(os.environ.get("XDG_DATA_HOME", HOME / ".local/share"))
STATE = Path(os.environ.get("XDG_STATE_HOME", HOME / ".local/state")) / "desktop"
RUNTIME = Path(os.environ.get("XDG_RUNTIME_DIR", Path(tempfile.gettempdir()) / f"desktop-{os.getuid()}")) / "desktop"


def read_json(path: Path, default=None):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return {} if default is None else default


def atomic_write(path: Path, content: str | bytes, mode=0o600):
    raw = content.encode() if isinstance(content, str) else content
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_file() and not path.is_symlink() and path.read_bytes() == raw:
        return
    if path.is_symlink():
        raise ValueError(f"Refusing to overwrite a link: {path}")
    descriptor, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(name, mode)
        os.replace(name, path)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(name)


def atomic_link(path: Path, target: Path):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.exists() and not path.is_symlink():
        raise ValueError(f"Refusing to replace a file with a link: {path}")
    with tempfile.TemporaryDirectory(prefix=f".{path.name}.", dir=path.parent) as directory:
        link = Path(directory) / "link"
        link.symlink_to(target)
        os.replace(link, path)


@contextlib.contextmanager
def state_lock(name="state", *, wait=True):
    STATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (STATE / f"{name}.lock").open("a") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
        except BlockingIOError:
            yield False
            return
        yield True


def settings():
    return read_json(CONFIG / "desktop/settings.json")


def state():
    return read_json(STATE / "state.json")


def save_state(value):
    atomic_write(STATE / "state.json", json.dumps(value, indent=2) + "\n")


def expand(value):
    return Path(os.path.expandvars(os.path.expanduser(str(value))))


def run(argv, *, check=True, capture=False, **kwargs):
    return subprocess.run([str(x) for x in argv], check=check, text=True,
                          capture_output=capture, **kwargs)


def background(argv, **kwargs):
    return subprocess.Popen([str(x) for x in argv], start_new_session=True,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)


def executable(name):
    variable = "DESKTOP_" + re.sub(r"[^A-Z0-9]", "_", name.upper()) + "_BIN"
    selected = os.environ.get(variable) or read_json(CONFIG / "desktop/paths.json").get(name)
    if selected:
        if not Path(selected).exists():
            raise ValueError(f"Declared {name} path is missing: {selected}")
        return selected
    for directory in os.get_exec_path():
        candidate = Path(directory) / name
        if candidate.is_file() and os.access(candidate, os.X_OK):
            # Skip our portable launch adapters when resolving their native binary.
            with candidate.open("rb") as stream:
                first = stream.read(512)
            if b"desktopctl run " not in first:
                return str(candidate)
    raise ValueError(f"{name} is unavailable; install it or declare desktop/paths.json")


def theme_directories():
    return [expand(x) for x in settings().get("themeDirectories", [str(CONFIG / "desktop/themes")])] + [DATA / "desktop/themes"]


def theme_path(name):
    if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name):
        raise ValueError("Theme names use lowercase letters, digits, underscores, and hyphens")
    for directory in theme_directories():
        candidate = directory / name
        if candidate.is_dir():
            return candidate
    raise ValueError(f"Unknown theme: {name}")


def selected_theme():
    return state().get("theme", settings().get("defaultTheme", "tokyoled"))


def selected_font():
    return state().get("font", settings().get("defaultFont", "JetBrainsMono Nerd Font"))


def font(args):
    action = args[0] if args else "current"
    if action == "current":
        print(selected_font())
        return
    families = sorted(set(run(["fc-list", ":spacing=mono", "--format", "%{family[0]}\n"], capture=True).stdout.splitlines()))
    if action == "list":
        print("\n".join(families))
        return
    if action != "set" or len(args) not in (2, 3) or args[1] not in families or (len(args) == 3 and args[2] != "--no-reload"):
        raise ValueError("font: list, current, or set an installed monospace font")
    with state_lock():
        value = state(); value["font"] = args[1]; save_state(value)
        prepare_theme()
    if "--no-reload" not in args:
        run(["gsettings", "set", "org.gnome.desktop.interface", "monospace-font-name", args[1] + " 11"], check=False)
        run(["makoctl", "reload"], check=False)
        run(["pkill", "-USR2", "-u", str(os.getuid()), "-x", "ghostty"], check=False)
        run(["pkill", "-USR1", "-u", str(os.getuid()), "-x", "kitty"], check=False)
        restart("theme")


def prepare_terminal_theme(c, follow_theme):
    family = selected_font()
    quoted = json.dumps(family)
    ghostty = f"font-family = \"\"\nfont-family = {quoted}\n"
    if follow_theme:
        ghostty += "theme =\n" + "\n".join(f"{key} = {c[key]}" for key in ("background", "foreground")) + f"\ncursor-color = {c['cursor']}\n"
        ghostty += "\n".join(f"palette = {i}={c[f'color{i}']}" for i in range(16)) + "\n"
        ghostty += f"selection-background = {c.get('selection_background', c['accent'])}\nselection-foreground = {c.get('selection_foreground', c['background'])}\n"
    atomic_write(STATE / "theme/ghostty.conf", ghostty)
    kitty = "\n".join(f"{key} {c[key]}" for key in ("background", "foreground", "cursor")) + "\n"
    kitty += "\n".join(f"color{i} {c[f'color{i}']}" for i in range(16)) + f"\nfont_family {family}\n"
    atomic_write(STATE / "theme/kitty.conf", kitty)
    alacritty = f"[font]\nnormal = {{ family = {quoted}, style = 'Regular' }}\nbold = {{ family = {quoted}, style = 'Bold' }}\nitalic = {{ family = {quoted}, style = 'Italic' }}\n"
    alacritty += f"[colors.primary]\nbackground = '{c['background']}'\nforeground = '{c['foreground']}'\n[colors.cursor]\ncursor = '{c['cursor']}'\ntext = '{c['background']}'\n"
    names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
    for offset, section in [(0, "normal"), (8, "bright")]:
        alacritty += f"[colors.{section}]\n" + "\n".join(f"{name} = '{c[f'color{i + offset}']}'" for i, name in enumerate(names)) + "\n"
    atomic_write(STATE / "theme/alacritty.toml", alacritty)
    atomic_write(STATE / "fontconfig.conf", '<?xml version="1.0"?>\n<fontconfig><match target="pattern"><test name="family"><string>monospace</string></test><edit name="family" mode="assign" binding="strong"><string>' + xml_escape(family) + '</string></edit></match></fontconfig>\n')


def colors(path):
    palette = tomllib.loads((path / "colors.toml").read_text())
    for key in ("accent", "foreground", "background", *[f"color{i}" for i in range(16)]):
        if not re.fullmatch(r"#[0-9a-fA-F]{6}", palette.get(key, "")):
            raise ValueError(f"Invalid or missing color {key} in {path.name}")
    palette.setdefault("cursor", palette["accent"])
    return palette


def prepare_theme(name=None, *, follow_terminal=None):
    name = name or selected_theme()
    source = theme_path(name)
    c = colors(source)
    light = (source / "light.mode").exists()
    target = STATE / "theme"
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
        "hyprlock.conf": "\n".join(f"${k} = rgba({v[1:]}ff)" for k, v in {
            "color": c["background"], "inner_color": c["background"], "outer_color": c["foreground"],
            "font_color": c["foreground"], "check_color": c["accent"],
        }.items()) + "\n",
    }
    for filename, fallback in templates.items():
        original = source / filename
        content = original.read_text() if original.is_file() else fallback
        if filename == "mako.ini":
            content = re.sub(r"(?m)^font=.*$", lambda _: "font=" + selected_font() + " 11", content)
        atomic_write(target / filename, content)
    prepare_terminal_theme(c, state().get("terminalTheme", False) if follow_terminal is None else follow_terminal)
    atomic_write(target / "colors.toml", (source / "colors.toml").read_text())
    btop = source / "btop.theme"
    fallback = "\n".join(f'theme[{key}]="{value}"' for key, value in {
        "main_bg": c["background"], "main_fg": c["foreground"], "title": c["foreground"],
        "hi_fg": c["accent"], "selected_bg": c["accent"], "selected_fg": c["background"],
    }.items()) + "\n"
    atomic_write(target / "current.theme", btop.read_text() if btop.exists() else fallback)
    eza = source / "eza.yml"
    if eza.is_file():
        atomic_write(target / "theme.yml", eza.read_text())
    else:
        (target / "theme.yml").unlink(missing_ok=True)
    marker = target / "light.mode"
    if light:
        atomic_write(marker, "")
    elif marker.exists():
        marker.unlink()
    atomic_write(target / "name", name + "\n")
    # The runtime search path lets Walker find generated CSS without writing to managed configuration.
    template_root = CONFIG / "walker/templates"
    for template in sorted(template_root.iterdir()) if template_root.is_dir() else []:
        if not template.is_dir():
            continue
        walker = RUNTIME / "walker/themes" / ("desktop-" + template.name)
        for item in template.iterdir():
            if item.is_file():
                content = item.read_text()
                if item.name == "style.css":
                    content = (target / "walker.css").read_text() + "\n" + content
                    content += "\n* { font-family: " + json.dumps(selected_font()) + ", monospace; }\n"
                atomic_write(walker / item.name, content)
    os.environ["XDG_CONFIG_DIRS"] = str(RUNTIME) + ":" + os.environ.get("XDG_CONFIG_DIRS", "/etc/xdg")
    return source, c, light


def notify(title, body=""):
    if shutil.which("notify-send"):
        run(["notify-send", title, body], check=False)


def selected_units(units):
    if not read_json(CONFIG / "desktop/bar-outputs.json").get("minimal", ["eDP-1"]):
        return [unit for unit in units if unit != "desktop-waybar-minimal"]
    return units


def restart(group):
    groups = {
        "launcher": ["desktop-elephant", "desktop-walker"],
        "bar": ["desktop-waybar", "desktop-waybar-minimal", "desktop-workspaces"],
        "theme": ["desktop-walker", "desktop-waybar", "desktop-waybar-minimal", "desktop-swayosd"],
    }
    if group not in groups:
        raise ValueError(f"Unknown service group: {group}")
    return run(["systemctl", "--user", "try-restart", *[x + ".service" for x in selected_units(groups[group])]], check=False).returncode


def theme(args):
    action = args[0] if args else "current"
    if action == "list":
        print("\n".join(sorted({p.name for d in theme_directories() if d.is_dir() for p in d.iterdir() if p.is_dir()})))
    elif action == "current":
        print(selected_theme())
    elif action in ("init", "set"):
        name = args[1] if action == "set" and len(args) > 1 else selected_theme()
        with state_lock():
            source, _, light = prepare_theme(name, follow_terminal=True if action == "set" else None)
            value = state(); value["theme"] = name
            if action == "set":
                value["terminalTheme"] = True
                value["desktopTheme"] = True
            save_state(value)
        if action == "set" and "--no-reload" not in args:
            apply_appearance(source, light)
            run(["hyprctl", "reload"], check=False)
            run(["makoctl", "reload"], check=False)
            run(["pkill", "-USR2", "-u", str(os.getuid()), "-x", "ghostty"], check=False)
            run(["pkill", "-USR1", "-u", str(os.getuid()), "-x", "kitty"], check=False)
            restart("theme")
    elif action == "generate":
        image_args = [x for x in args[1:] if not x.startswith("--")]
        if not image_args:
            chosen = run(["zenity", "--file-selection", "--title=Choose a wallpaper for the new palette"], check=False, capture=True)
            if chosen.returncode or not chosen.stdout.strip():
                return
            image_args = [chosen.stdout.strip()]
        if len(image_args) != 1:
            raise ValueError("usage: desktopctl theme generate [IMAGE] [--light] [--no-reload]")
        image = expand(image_args[0]).resolve()
        if not image.is_file():
            raise ValueError(f"Missing image: {image}")
        mode = "light" if "--light" in args else "dark"
        config = RUNTIME / "matugen.toml"
        atomic_write(config, "[config]\n[templates]\n")
        result = run(["matugen", "image", image, "--dry-run", "--json", "hex", "--config", config,
                      "--source-color-index", "0", "--mode", mode], capture=True)
        roles = json.loads(result.stdout)["colors"]
        def role(name):
            value = roles[name][mode]["color"]
            if not re.fullmatch(r"#[0-9a-fA-F]{6}", value):
                raise ValueError("Matugen returned an invalid color")
            return value
        stem = re.sub(r"[^a-z0-9]+", "-", image.stem.lower()).strip("-")[:32] or "wallpaper"
        name = f"generated-{stem}-{hashlib.sha256(image.read_bytes()).hexdigest()[:8]}-{mode}"
        directory = DATA / "desktop/themes" / name
        palette = {"background": role("background"), "foreground": role("on_surface"), "accent": role("primary"), "cursor": role("primary")}
        ansi = ["surface_container_lowest", "error", "tertiary", "secondary", "primary", "tertiary", "inverse_primary", "on_surface_variant",
                "surface_bright", "error", "tertiary", "secondary", "primary", "tertiary", "inverse_primary", "on_surface"]
        palette.update({f"color{i}": role(key) for i, key in enumerate(ansi)})
        atomic_write(directory / "colors.toml", "\n".join(f'{key} = "{value}"' for key, value in palette.items()) + "\n")
        if mode == "light":
            atomic_write(directory / "light.mode", "")
        theme(["set", name, *(["--no-reload"] if "--no-reload" in args else [])])
        if "--no-reload" not in args:
            wallpaper(["set", str(image)])
        print(name)
    else:
        raise ValueError("theme: list, current, init, set NAME [--no-reload], or generate IMAGE")


def apply_appearance(source, light):
    values = dict([
        ("color-scheme", "prefer-light" if light else "prefer-dark"),
        ("gtk-theme", "Adwaita" if light else "Adwaita-dark"),
        ("icon-theme", (source / "icons.theme").read_text().strip() if (source / "icons.theme").exists() else "Yaru-magenta"),
    ])
    values.update(read_json(CONFIG / "desktop/interface.json"))
    if state().get("font"):
        values["monospace-font-name"] = selected_font() + " 11"
    for key, value in values.items():
        run(["gsettings", "set", "org.gnome.desktop.interface", key, value], check=False)


def wallpaper(args):
    action = args[0] if args else "current"
    value = state()
    if action == "current":
        print(value.get("wallpaper", settings().get("defaultWallpaper", "")))
        return
    if action == "grid":
        os.execvp("background-grid", ["background-grid"])
    choices = sorted(p for d in settings().get("wallpaperDirectories", [str(DATA / "wallpapers")])
                     for p in expand(d).rglob("*") if p.is_file() and p.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".avif"})
    if action == "next":
        if not choices:
            raise ValueError("No wallpapers found")
        current = expand(value.get("wallpaper", ""))
        image = choices[(choices.index(current) + 1) % len(choices)] if current in choices else choices[0]
    elif action == "restore":
        image = expand(value.get("wallpaper", settings().get("defaultWallpaper", "")))
    elif action == "set" and len(args) >= 2:
        image = expand(args[1]).resolve()
    else:
        raise ValueError("wallpaper: current, grid, next, restore, or set IMAGE [OUTPUT]")
    if not image.is_file():
        raise ValueError(f"Wallpaper is missing: {image}")
    output = args[2] if action == "set" and len(args) > 2 else None
    if run(["awww", "query"], check=False, capture=True).returncode:
        managed = run(["systemctl", "--user", "start", "desktop-awww.service"], check=False, capture=True)
        if managed.returncode:
            background(["awww-daemon", "--no-cache"])
        for _ in range(80):
            if not run(["awww", "query"], check=False, capture=True).returncode:
                break
            time.sleep(.05)
        else:
            raise ValueError("The wallpaper daemon did not become ready")
    transition = read_json(CONFIG / "background-grid/transition.json", {"type": "any", "duration": 2, "fps": 60, "step": 80})
    command = ["awww", "img", "--resize", "crop", "--transition-type", "none" if action == "restore" else transition['type'],
               "--transition-duration", str(transition['duration']), "--transition-fps", str(transition['fps']), "--transition-step", str(transition['step'])]
    if output:
        command += ["--outputs", output]
    run([*command, image])
    with state_lock():
        value = state(); value["wallpaper"] = str(image)
        value.setdefault("wallpapers", {})[output or "*"] = str(image)
        save_state(value)
        atomic_link(STATE / "background", image.resolve())
    if action == "restore":
        for monitor, saved in value.get("wallpapers", {}).items():
            if monitor != "*" and expand(saved).is_file():
                run(["awww", "img", "--outputs", monitor, "--transition-type", "none", saved], check=False)


def cwd():
    try:
        window = json.loads(run(["hyprctl", "-j", "activewindow"], capture=True).stdout)
        pid = str(window.get("pid", ""))
        children = run(["pgrep", "-P", pid], check=False, capture=True).stdout.split()
        for child in reversed(children):
            path = Path(f"/proc/{child}/cwd").resolve()
            if path.is_dir():
                return str(path)
    except (subprocess.CalledProcessError, OSError, ValueError):
        pass
    return str(HOME)


def terminal(args):
    args = list(args)
    floating = "--float" in args
    shell = "--shell" in args
    args = [x for x in args if x not in ("--float", "--shell")]
    app_id = "TUI.float" if floating else "org.desktop.terminal"
    if args and args[0] == "--app-id":
        app_id, args = args[1], args[2:]
    command = ["xdg-terminal-exec", "--dir=" + cwd(), "--app-id=" + app_id]
    if args:
        command += ["-e", "bash", "-lc", args[0]] if shell else ["-e", *args]
    os.execvp(command[0], command)


def launch(args):
    if not args:
        raise ValueError("launch: browser, webapp URL, tui COMMAND, editor, focus CLASS COMMAND, audio, bluetooth, wifi")
    what, rest = args[0], args[1:]
    if what == "focus":
        if not rest:
            raise ValueError("focus requires a window pattern and optional launch command")
        windows = json.loads(run(["hyprctl", "-j", "clients"], capture=True).stdout)
        pattern = re.compile(rest[0], re.IGNORECASE)
        found = next((w for w in windows if pattern.search(w.get("class", "")) or pattern.search(w.get("title", ""))), None)
        if found:
            selector = json.dumps("address:" + found["address"])
            run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.focus({{ window = {selector} }}))'])
        else:
            command = ["uwsm-app", "--", rest[0]] if len(rest) == 1 else shlex.split(rest[1]) if len(rest) == 2 else rest[1:]
            os.execvp(command[0], command)
    elif what in ("audio", "bluetooth", "wifi"):
        terminal(["--app-id", "org.desktop." + {"audio": "wiremix", "bluetooth": "bluetui", "wifi": "impala"}[what], {"audio": "wiremix", "bluetooth": "bluetui", "wifi": "impala"}[what]])
    elif what == "tui":
        if not rest:
            raise ValueError("tui requires a command")
        terminal(["--app-id", "org.desktop." + Path(rest[0]).name, *rest])
    elif what == "editor":
        terminal([settings().get("editor", "nvim"), *rest])
    elif what in ("browser", "webapp"):
        command = [settings().get("browser", "chromium")]
        if what == "webapp":
            if len(rest) != 1 or not re.match(r"https?://", rest[0]):
                raise ValueError("webapp requires one HTTP(S) URL")
            command += ["--app=" + rest[0]]
        else:
            command += ["--incognito" if x == "--private" else x for x in rest]
        os.execvp(command[0], command)
    else:
        raise ValueError(f"Unknown launch action: {what}")


def bar_stylesheet(style, scale):
    if scale == 1.0:
        return "@import " + json.dumps(str(style)) + ";\n"

    def inline(path, seen=()):
        path = path.resolve()
        if path in seen:
            raise ValueError(f"Circular Waybar stylesheet import: {path}")
        return re.sub(r'''@import\s+["']([^"']+)["']\s*;''',
                      lambda match: inline(path.parent / match[1], (*seen, path)), path.read_text())

    # Imported light/base rules must be scaled together; their relative paths
    # would otherwise resolve against the generated runtime directory.
    return re.sub(r"(?<![\w.])([0-9]+(?:\.[0-9]+)?)px", lambda m: f"{float(m[1]) * scale:g}px", inline(style))


def run_application(args):
    if not args:
        raise ValueError("run requires an application")
    app, rest = args[0], args[1:]
    if rest[:1] == ["--"]:
        rest = rest[1:]
    _, _, light = prepare_theme()
    raw = executable(app)
    if app == "walker":
        if not any(x in ("-t", "--theme") or x.startswith("--theme=") for x in rest):
            rest = ["--theme", "desktop-rounded", *rest]
    elif app == "mako":
        source = (CONFIG / "mako/config").read_text().replace("~/.local/state/desktop", str(STATE))
        target = RUNTIME / "mako.conf"; atomic_write(target, source)
        rest = ["--config", str(target), *rest]
    elif app == "swayosd-server":
        css = (STATE / "theme/swayosd.css").read_text() + (CONFIG / "swayosd/style.css").read_text()
        css += "\nlabel { font-family: " + json.dumps(selected_font()) + "; }\n"
        target = RUNTIME / "swayosd.css"; atomic_write(target, css)
        rest = ["--style", str(target), *rest]
    elif app == "btop":
        rest = ["--themes-dir", str(STATE / "theme"), *rest]
    elif app == "waybar":
        def take(short, long, default):
            for i, arg in enumerate(rest):
                if arg in (short, long) and i + 1 < len(rest):
                    return Path(rest[i + 1]).expanduser()
            return default
        source = take("-c", "--config", CONFIG / "waybar/config.jsonc")
        style = take("-s", "--style", CONFIG / "waybar/style.css")
        if source.name in ("config.jsonc", "config-minimal.jsonc"):
            text = source.read_text()
            bar = read_json(CONFIG / "desktop/bar-outputs.json")
            if bar:
                config = json.loads(text)
                config["output"] = bar.get("minimal" if "minimal" in source.name else "full", ["*"])
                scale = float(bar.get("scale", 1.0))
                if not 0.75 <= scale <= 2.0:
                    raise ValueError("Waybar scale must be between 0.75 and 2")
                for key in ("height", "margin-top", "margin-bottom", "margin-left", "margin-right"):
                    if key in config:
                        config[key] = round(config[key] * scale)
                # Tray icons use pixels too; keep them proportional to labels.
                if "tray" in config:
                    for key in ("icon-size", "spacing"):
                        if key in config["tray"]:
                            config["tray"][key] = round(config["tray"][key] * scale)
                text = json.dumps(config)
            else:
                scale = 1.0
            if light:
                mapping = {"#f4f4f5": "#4c4f69", "#cdd6f4": "#4c4f69", "#60a5fa": "#1e66f5", "#89b4fa": "#1e66f5", "#4ade80": "#40a02b", "#eab308": "#df8e1d", "#f87171": "#d20f39"}
                text = re.sub(r"#[0-9a-fA-F]{6}", lambda m: mapping.get(m[0].lower(), m[0]), text)
                alternate = style.with_name(style.stem + "-light.css")
                if alternate.exists():
                    style = alternate
            text = re.sub(r'("NordVPN"\s*:\s*")[^"]*(")', lambda m: m[1] + ("nordvpn-tray-black" if light else "nordvpn-tray-white") + m[2], text)
            target = RUNTIME / ("waybar-" + source.name); atomic_write(target, text)
            filtered = []
            skip = False
            for arg in rest:
                if skip:
                    skip = False; continue
                if arg in ("-c", "--config", "-s", "--style"):
                    skip = True; continue
                filtered.append(arg)
            css = bar_stylesheet(style, scale) + "\n* { font-family: " + json.dumps(selected_font()) + ", 'Symbols Nerd Font'; }\n"
            if state().get("desktopTheme"):
                c = colors(theme_path(selected_theme()))
                css += "\n".join(f"@define-color {key} {value};" for key, value in {
                    "text": c["foreground"], "subtext": c["color7"], "blue": c["accent"],
                    "mauve": c["color5"], "green": c["color2"], "red": c["color1"],
                    "yellow": c["color3"], "base": c["background"],
                }.items()) + "\n"
            runtime_style = RUNTIME / ("waybar-" + style.name)
            atomic_write(runtime_style, css)
            rest = ["--config", str(target), "--style", str(runtime_style), *filtered]
    os.execv(raw, [raw, *rest])


def launcher(args):
    if any(args[i] in ("-m", "--modules") and args[i + 1] == "clipboard" for i in range(len(args) - 1)):
        return clipboard(args)
    run(["systemctl", "--user", "start", "desktop-elephant.service", "desktop-walker.service"], check=False)
    run_application(["walker", "--width", "644", "--maxheight", "300", "--minheight", "300", *args])


def clipboard(args):
    width, height = 1000, 520
    try:
        monitors = json.loads(run(["hyprctl", "-j", "monitors"], capture=True, check=False).stdout)
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
    run(["systemctl", "--user", "start", "desktop-elephant.service", "desktop-walker.service"], check=False)
    modules = [] if any(x in args for x in ("-m", "--modules")) else ["-m", "clipboard"]
    run_application(["walker", "--theme", "desktop-clipboard-compact" if compact else "desktop-clipboard",
                     "--width", str(width), "--height", str(height),
                     *modules, *args])


def toggle(args):
    if not args:
        raise ValueError("toggle requires a feature")
    feature = args[0]
    marker = STATE / "toggles" / feature
    if feature == "enabled":
        return 0 if len(args) > 1 and (STATE / "toggles" / args[1]).exists() else 1
    if feature == "idle":
        # Automatic locking is opt-in for this login. Sleep protection always runs.
        with state_lock("idle"):
            marker = RUNTIME / "idle-enabled"
            enabled = not marker.exists()
            if args[1:2] in (["on"], ["off"]):
                enabled = args[1] == "on"
            if enabled:
                atomic_write(marker, "")
            else:
                marker.unlink(missing_ok=True)
            run(["systemctl", "--user", "restart", "desktop-hypridle.service"])
        return
    services = {"bar": ["desktop-waybar", "desktop-waybar-minimal", "desktop-workspaces"], "nightlight": ["desktop-hyprsunset"]}
    if feature in services:
        services[feature] = selected_units(services[feature])
        service = services[feature][0] + ".service"
        active = run(["systemctl", "--user", "is-active", "--quiet", service], check=False).returncode == 0
        if feature == "bar":
            with state_lock():
                disabled = STATE / "toggles/waybar-off"
                if active:
                    atomic_write(disabled, "")
                else:
                    disabled.unlink(missing_ok=True)
        run(["systemctl", "--user", "stop" if active else "start", *[x + ".service" for x in services[feature]]])
        return
    if feature == "notifications":
        run(["makoctl", "mode", "-t", "do-not-disturb"])
        return
    if feature == "screensaver":
        with state_lock():
            marker = STATE / "toggles/screensaver-off"
            if marker.exists():
                marker.unlink()
                enabled = True
            else:
                atomic_write(marker, "")
                enabled = False
        notify("Screensaver", "Automatic screensaver enabled" if enabled else "Automatic screensaver disabled")
        return
    if feature == "layout":
        workspace = json.loads(run(["hyprctl", "-j", "activeworkspace"], capture=True).stdout)
        layout = "scrolling" if workspace.get("tiledLayout") == "dwindle" else "dwindle"
        run(["hyprctl", "eval", f'hl.workspace_rule({{ workspace = {json.dumps(str(workspace["id"]))}, layout = {json.dumps(layout)} }})'])
        return
    with state_lock():
        enabled = not marker.exists()
        if args[1:2] in (["on"], ["off"]):
            enabled = args[1] == "off"  # marker records the disabled state
        if enabled:
            atomic_write(marker, "")
        else:
            marker.unlink(missing_ok=True)
        values = {"gaps": "general {\n gaps_in = 0\n gaps_out = 0\n}\n", "square": "layout {\n single_window_aspect_ratio = 1 1\n}\n"}
        if feature == "touchpad":
            devices = json.loads(run(["hyprctl", "-j", "devices"], capture=True).stdout)
            values[feature] = "".join("device {\n name = " + m["name"] + "\n enabled = false\n}\n" for m in devices.get("mice", []) if "touchpad" in m["name"])
        if feature not in values:
            marker.unlink(missing_ok=True)
            raise ValueError(f"Unknown toggle: {feature}")
        path = STATE / "toggles/hypr" / (feature + ".conf")
        if enabled:
            atomic_write(path, values[feature])
        else:
            path.unlink(missing_ok=True)
    run(["hyprctl", "reload"])


def display(args):
    monitors = json.loads(run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
    external = [m for m in monitors if not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled")]
    if args == ["external"]:
        return 0 if external else 1
    if (CONFIG / "desktop/ring.json").exists():
        command = "rescan" if args[-1:] in (["on"], ["off"]) else "menu"
        return run(["desktop-ring", command], check=False).returncode
    internal = next((m for m in monitors if m["name"].startswith(("eDP", "LVDS"))), None)
    if not internal:
        raise ValueError("No internal display is present")
    action = args[1] if len(args) > 1 else "toggle"
    mirror = args[:1] == ["mirror"]
    file = STATE / "toggles/hypr/internal-display.conf"
    disabled = not file.exists() if action == "toggle" else action == "off"
    if disabled and not external:
        raise ValueError("Refusing to disable the only available display")
    if mirror and action == "toggle" and file.exists() and "mirror" in file.read_text():
        file.unlink()
    elif mirror and external and action != "off":
        atomic_write(file, f"monitor = {internal['name']}, preferred, auto, auto, mirror, {external[0]['name']}\n")
    elif disabled:
        atomic_write(file, f"monitor = {internal['name']}, disable\n")
    else:
        file.unlink(missing_ok=True)
    run(["hyprctl", "reload"])


def brightness(args):
    if len(args) < 2:
        raise ValueError("brightness requires display|keyboard and an adjustment")
    kind, value = args[:2]
    if kind == "display" and value in ("on", "off"):
        run(["hyprctl", "dispatch", "dpms", value]); return
    directory = Path("/sys/class/leds" if kind == "keyboard" else "/sys/class/backlight")
    devices = sorted(p.name for p in directory.glob("*") if kind != "keyboard" or "kbd_backlight" in p.name)
    if not devices:
        raise ValueError(f"No {kind} brightness device is available")
    device = devices[0]
    if kind == "keyboard":
        if value in ("off", "restore"):
            run(["brightnessctl", "-d", device, "-s" if value == "off" else "-r", *(["set", "0"] if value == "off" else [])]); return
        current = int(run(["brightnessctl", "-d", device, "get"], capture=True).stdout)
        maximum = int(run(["brightnessctl", "-d", device, "max"], capture=True).stdout)
        step = max(1, maximum // 10)
        target = (current + step) % (maximum + 1) if value == "cycle" else min(maximum, max(0, current + (step if value == "up" else -step)))
        value = str(target)
    run(["brightnessctl", "-d", device, "set", value])
    percent = round(100 * int(run(["brightnessctl", "-d", device, "get"], capture=True).stdout) / int(run(["brightnessctl", "-d", device, "max"], capture=True).stdout))
    run(["swayosd-client", "--custom-icon", "keyboard-brightness-symbolic" if kind == "keyboard" else "display-brightness-symbolic", "--custom-progress", str(percent / 100)], check=False)


def osd(args):
    monitor = json.loads(run(["hyprctl", "-j", "activeworkspace"], capture=True).stdout).get("monitor")
    command = ["swayosd-client", *(["--monitor", monitor] if monitor else []), *args]
    return run(command, check=False).returncode


def wake():
    run(["hyprctl", "dispatch", "dpms", "on"], check=False)
    with contextlib.suppress(ValueError, subprocess.CalledProcessError):
        brightness(["keyboard", "restore"])


def lock(args):
    if run(["pgrep", "-u", str(os.getuid()), "-x", "hyprlock"], check=False, capture=True).returncode == 0:
        return
    unit = "desktop-lock-only.service" if "--only" in args or os.environ.get("DESKTOP_LOCK_ONLY") == "true" else "desktop-lock.service"
    result = run(["systemctl", "--user", "start", unit], check=False, capture=True)
    if result.returncode:
        background([sys.executable, __file__, "lock-worker", *(args or [])])


def idle_worker(args):
    config = "hypridle.conf" if (RUNTIME / "idle-enabled").exists() else "hypridle-sleep.conf"
    binary = executable("hypridle")
    os.execv(binary, [binary, "--config", str(CONFIG / "hypr" / config)])


def lock_worker(args):
    with state_lock("screen-lock", wait=False) as acquired:
        if not acquired:
            return
        prepare_theme()
        if shutil.which("1password"):
            background(["1password", "--lock"])
        run(["hyprctl", "switchxkblayout", "all", "0"], check=False)
        source = lock_config()
        target = RUNTIME / "hyprlock.conf"; atomic_write(target, source)
        child = subprocess.Popen(["hyprlock", "--config", str(target)])
        if "--only" not in args:
            time.sleep(3)
            if child.poll() is None:
                run(["hyprctl", "dispatch", "dpms", "off"], check=False)
                with contextlib.suppress(ValueError, subprocess.CalledProcessError):
                    brightness(["keyboard", "off"])
        result = child.wait()
        wake()
        if result:
            raise subprocess.CalledProcessError(result, child.args)


def copy_data(data, mime):
    process = subprocess.Popen(["wl-copy", "--foreground", "--type", mime], stdin=subprocess.PIPE,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    process.stdin.write(data); process.stdin.close()


def copy_text(text):
    copy_data(text.encode(), "text/plain;charset=utf-8")


def screenshot_geometry(mode):
    if mode == "region":
        selected = run(["slurp"], check=False, capture=True)
        return selected.stdout.strip() if not selected.returncode else None
    monitors = json.loads(run(["hyprctl", "-j", "monitors"], capture=True).stdout)
    focused = next((m for m in monitors if m.get("focused")), monitors[0] if monitors else None)
    if not focused:
        raise ValueError("No output is available for capture")
    def rect(m):
        w, h = int(m["width"] / m["scale"]), int(m["height"] / m["scale"])
        if m.get("transform", 0) % 2:
            w, h = h, w
        return f'{m["x"]},{m["y"]} {w}x{h}'
    if mode == "fullscreen":
        return rect(focused)
    clients = json.loads(run(["hyprctl", "-j", "clients"], capture=True).stdout)
    rectangles = [f'{w["at"][0]},{w["at"][1]} {w["size"][0]}x{w["size"][1]}' for w in clients
                  if w.get("workspace", {}).get("id") == focused.get("activeWorkspace", {}).get("id")
                  and not w.get("hidden")]
    rectangles.append(rect(focused))
    selected = run(["slurp", *(["-r"] if mode == "windows" else [])], input="\n".join(rectangles), check=False, capture=True)
    if selected.returncode or not selected.stdout.strip():
        return None
    geometry = selected.stdout.strip()
    click = re.fullmatch(r"(-?\d+),(-?\d+) (\d+)x(\d+)", geometry)
    if mode == "smart" and click and int(click[3]) * int(click[4]) < 20:
        x, y = int(click[1]), int(click[2])
        for rectangle in rectangles:
            r = re.fullmatch(r"(-?\d+),(-?\d+) (\d+)x(\d+)", rectangle)
            if r and int(r[1]) <= x < int(r[1]) + int(r[3]) and int(r[2]) <= y < int(r[2]) + int(r[4]):
                return rectangle
    return geometry


@contextlib.contextmanager
def frozen_display(enabled):
    if not enabled:
        yield
        return
    freezer = background(["hyprpicker", "-r", "-z"])
    try:
        # Match the reference capture's render allowance before area selection.
        time.sleep(.1)
        yield
    finally:
        if freezer.poll() is None:
            freezer.terminate()
            try:
                freezer.wait(timeout=1)
            except subprocess.TimeoutExpired:
                freezer.kill()
                freezer.wait()


def edit_screenshot(path, editor="satty"):
    if editor == "satty":
        run(["satty", "--filename", path, "--output-filename", path, "--actions-on-enter", "save-to-clipboard", "--save-after-copy", "--copy-command", "wl-copy"])
    else:
        run([*shlex.split(editor), path])


def capture(args):
    action = args[0] if args else "screenshot"
    if action == "record":
        pidfile = STATE / "recording.json"
        current = read_json(pidfile)
        if current:
            pid = current.get("pid")
            try:
                command = Path(f"/proc/{pid}/cmdline").read_bytes()
            except FileNotFoundError:
                command = b""
            if b"gpu-screen-recorder" in command:
                os.kill(pid, signal.SIGINT)
            pidfile.unlink(missing_ok=True)
            return
        directory = HOME / settings().get("recordings", "Videos"); directory.mkdir(parents=True, exist_ok=True)
        output = directory / time.strftime("%Y-%m-%d_%H-%M-%S.mp4")
        mode = args[1] if len(args) > 1 else "screen"
        command = ["gpu-screen-recorder", "-w", "portal" if mode == "screen" else mode, "-f", "60", "-fallback-cpu-encoding", "yes"]
        if mode == "region":
            geometry = screenshot_geometry("smart")
            if not geometry:
                return
            rectangle = re.fullmatch(r"(-?\d+),(-?\d+) (\d+)x(\d+)", geometry)
            if not rectangle:
                raise ValueError("Invalid recording region")
            command += ["-region", f"{rectangle[3]}x{rectangle[4]}+{rectangle[1]}+{rectangle[2]}"]
        if "--silent" not in args:
            command += ["-a", "default_output"]
        if "--microphone" in args:
            command += ["-a", "default_input"]
        child = background([*command, "-o", output])
        time.sleep(.25)
        if child.poll() is not None:
            raise ValueError("Screen recorder exited before recording started; check the capture backend and driver")
        atomic_write(pidfile, json.dumps({"pid": child.pid, "output": str(output)}))
        notify("Recording started", str(output)); return
    if action == "edit-latest":
        directory = HOME / settings().get("screenshots", "Pictures")
        files = sorted([*directory.glob("screenshot-*.png"), *directory.glob("Screenshot_*.png")], key=lambda path: path.stat().st_mtime_ns)
        if not files:
            raise ValueError("No saved screenshot is available")
        edit_screenshot(files[-1])
        return
    if action == "notify" and len(args) >= 2:
        output = expand(args[1])
        response = run(["notify-send", "Screenshot saved to clipboard and file", "Edit with Super + Alt + , (or click this)", "-t", "10000", "-i", output, "-A", "default=edit"], capture=True, check=False)
        if response.stdout.strip() == "default" and output.is_file():
            edit_screenshot(output, args[2] if len(args) > 2 else "satty")
        return
    editor = next((value.split("=", 1)[1] for value in args if value.startswith("--editor=")), "satty")
    positional = [value for value in args[1:] if not value.startswith("--")]
    mode = positional[0] if positional else ("region" if action == "ocr" else "smart")
    if mode not in ("smart", "region", "windows", "fullscreen"):
        raise ValueError("Screenshot mode: smart, region, windows, or fullscreen")
    processing = positional[1] if len(positional) > 1 else "slurp"
    if processing not in ("slurp", "copy", "save"):
        raise ValueError("Screenshot processing: slurp, copy, or save")
    with frozen_display(mode != "fullscreen"):
        geometry = screenshot_geometry(mode)
        if not geometry:
            return
        image = subprocess.run(["grim", "-g", geometry, "-"], check=True, capture_output=True).stdout
    if action == "ocr":
        result = subprocess.run(["tesseract", "stdin", "stdout", "--oem", "1", "--psm", "6", "-l", "eng", "--dpi", "300"], input=image, check=True, capture_output=True)
        copy_text(result.stdout.decode()); notify("Text copied from selection")
    else:
        if processing == "copy":
            copy_data(image, "image/png")
            return
        directory = HOME / settings().get("screenshots", "Pictures"); directory.mkdir(parents=True, exist_ok=True)
        output = directory / time.strftime("screenshot-%Y-%m-%d_%H-%M-%S.png")
        output.write_bytes(image)
        if "--edit" in args:
            edit_screenshot(output, editor)
        elif processing == "slurp":
            copy_data(image, "image/png")
            background(["desktopctl", "capture", "notify", output, editor])
        if processing == "save":
            print(output)


def reminder(args):
    action = args[0] if args else "show"
    if action == "show":
        now = time.time()
        pending = []
        for path in sorted((RUNTIME / "reminders").glob("*.json")):
            item = read_json(path)
            if item.get("due", 0) <= now:
                path.unlink(missing_ok=True)
                continue
            remaining = max(1, int((item["due"] - now + 59) // 60))
            pending.append(f'{item["message"]} — in {remaining} min ({time.strftime("%H:%M", time.localtime(item["due"]))})')
        notify("Upcoming reminders", "\n".join(pending) or "No outstanding reminders")
    elif action == "clear":
        units = run(["systemctl", "--user", "list-units", "--all", "--plain", "--no-legend", "desktop-reminder-*.timer"], capture=True).stdout
        names = [line.split()[0] for line in units.splitlines() if line.split()]
        if names:
            run(["systemctl", "--user", "stop", *names])
        for path in (RUNTIME / "reminders").glob("*.json"):
            path.unlink(missing_ok=True)
        notify("Reminders", "All reminders cleared")
    else:
        minutes = float(action)
        if not 0 < minutes <= 525600:
            raise ValueError("Reminder delay must be positive and at most one year")
        message = " ".join(args[1:]) or "Reminder"
        unit = "desktop-reminder-" + uuid.uuid4().hex
        run(["systemd-run", "--user", "--quiet", "--collect", "--unit=" + unit,
             f"--on-active={minutes * 60}s", "notify-send", "Reminder", message])
        due = time.time() + minutes * 60
        atomic_write(RUNTIME / "reminders" / (unit + ".json"), json.dumps({"message": message, "due": due}))
        notify("Reminder set", f'{message} — at {time.strftime("%H:%M", time.localtime(due))}')


def share(args):
    if not args or args[0] not in ("clipboard", "file", "folder"):
        raise ValueError("share: clipboard, file [PATH], or folder [PATH]")
    kind = args[0]
    if kind == "clipboard":
        types = run(["wl-paste", "--list-types"], capture=True).stdout.splitlines()
        mime = next((m for m in ("text/plain;charset=utf-8", "text/plain", "image/png", "image/jpeg") if m in types), None)
        if mime is None:
            raise ValueError("The clipboard has no shareable text or image; choose File instead")
        folder = RUNTIME / "shares"
        folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        suffix = ".png" if mime == "image/png" else ".jpg" if mime == "image/jpeg" else ".txt"
        descriptor, filename = tempfile.mkstemp(prefix="clipboard-", suffix=suffix, dir=folder)
        path = Path(filename)
        try:
            with os.fdopen(descriptor, "wb") as stream:
                subprocess.run(["wl-paste", "--type", mime], stdout=stream, check=True)
        except BaseException:
            path.unlink(missing_ok=True)
            raise
        # LocalSend can hand the request to its existing GUI; retain this private
        # file until logout rather than unlinking before the recipient is chosen.
    elif len(args) == 2:
        path = expand(args[1]).resolve()
    else:
        command = ["zenity", "--file-selection", "--title=Share " + kind]
        if kind == "folder":
            command.append("--directory")
        result = run(command, capture=True, check=False)
        if result.returncode or not result.stdout.strip():
            return
        path = expand(result.stdout.strip()).resolve()
    if not (path.is_dir() if kind == "folder" else path.is_file()):
        raise ValueError("Choose an existing " + kind)
    run(["localsend", "--headless", "send", str(path)])


def edit_config(args):
    choices = {
        "Hyprland": "hypr/.config/hypr/looknfeel.lua", "Keybindings": "hypr/.config/hypr/bindings.lua",
        "Hyprland configuration": "hypr/.config/hypr/hyprland.lua",
        "Input": "hypr/.config/hypr/input.lua", "Monitors": "hypr/.config/hypr/monitors.lua",
        "Hypridle": "hypr/.config/hypr/hypridle.conf", "Hyprlock": "hypr/.config/hypr/hyprlock.conf",
        "Night light": "hypr/.config/hypr/hyprsunset.conf", "Walker": "walker/.config/walker/config.toml",
        "Waybar": "waybar/.config/waybar/config.jsonc", "Desktop menu": "desktop-settings/.config/desktop/menus.json",
        "SwayOSD": "appearance/.config/swayosd/config.toml", "XCompose": "desktop-settings/.XCompose",
        "Application launchers": "appearance/.config/desktop/settings.json",
        "Default terminal": "desktop-settings/.config/xdg-terminals.list",
        "File and URL handlers": "desktop-settings/.config/mimeapps.list",
    }
    if len(args) != 1 or args[0] not in choices:
        raise ValueError("config: " + ", ".join(choices))
    root = expand(os.environ.get("DOTFILES", HOME / "nixos-config/dotfiles")).resolve()
    if args[0] == "Monitors":
        host = socket.gethostname()
        path = root.parent / "hosts" / (host if host in ("lambda", "desktop") else "desktop") / "monitors.lua"
    else:
        path = root / choices[args[0]]
    if not path.is_file() or str(root).startswith("/nix/store/"):
        raise ValueError("Open an editable nixos-config checkout and set DOTFILES to its dotfiles directory")
    launch(["editor", str(path)])


def lock_config():
    source = (CONFIG / "hypr/hyprlock.conf").read_text().replace("~/.local/state/desktop", str(STATE))
    source = re.sub(r"(?m)^(\s*font_family\s*=).*$", lambda m: m[1] + " " + selected_font(), source)
    image = state().get("lockBackground")
    if image and expand(image).is_file():
        atomic_link(STATE / "lock-background", expand(image).resolve())
        source = source.replace(str(STATE / "background"), str(STATE / "lock-background"))
    return source


def appearance(args):
    if args[:1] == ["terminal-colors"] and args[1:] in (["personal"], ["desktop"]):
        with state_lock():
            value = state(); value["terminalTheme"] = args[1] == "desktop"; save_state(value)
            prepare_theme()
        run(["pkill", "-USR2", "-u", str(os.getuid()), "-x", "ghostty"], check=False)
        restart("theme")
    elif args == ["lock-background", "reset"]:
        with state_lock():
            value = state(); value.pop("lockBackground", None); save_state(value)
    elif args[:1] == ["lock-background"]:
        if len(args) > 1:
            image = expand(args[1]).resolve()
        else:
            result = run(["zenity", "--file-selection", "--title=Choose lock screen background", "--file-filter=Images | *.png *.jpg *.jpeg *.webp"], capture=True, check=False)
            if result.returncode or not result.stdout.strip():
                return
            image = expand(result.stdout.strip()).resolve()
        if not image.is_file() or image.suffix.lower() not in (".png", ".jpg", ".jpeg", ".webp"):
            raise ValueError("Choose an existing PNG, JPEG or WebP image")
        with state_lock():
            value = state(); value["lockBackground"] = str(image); save_state(value)
    else:
        raise ValueError("appearance: terminal-colors personal|desktop, or lock-background [IMAGE|reset]")


def image_art(args, *, width=80, height=26):
    if args:
        image = expand(args[0]).resolve()
    else:
        result = run(["zenity", "--file-selection", "--title=Choose artwork image",
                      "--file-filter=Images | *.svg *.png *.jpg *.jpeg *.webp"], capture=True, check=False)
        if result.returncode or not result.stdout.strip():
            return None
        image = expand(result.stdout.strip()).resolve()
    if not image.is_file() or image.suffix.lower() not in (".svg", ".png", ".jpg", ".jpeg", ".webp"):
        raise ValueError("Choose an existing SVG, PNG, JPEG or WebP image")
    # Detect actual source transparency before padding, so opaque photos use
    # luminance while transparent logos retain their silhouette, of either color.
    limits = run(["magick", "-background", "none", str(image), "-alpha", "extract",
                  "-format", "%[fx:minima] %[fx:maxima]", "info:"], capture=True).stdout.split()
    transparent = len(limits) == 2 and float(limits[0]) < .999 and float(limits[1]) > float(limits[0])
    command = ["magick", "-background", "none", str(image), "-auto-orient"]
    command += ["-alpha", "extract", "-alpha", "off"] if transparent else ["-background", "white", "-alpha", "remove", "-alpha", "off", "-colorspace", "Gray", "-negate"]
    command += ["-bordercolor", "black", "-border", "1", "-trim", "+repage", "-resize", f"{width * 2}x{height * 4}",
                "-gravity", "center", "-background", "black", "-extent", f"{width * 2}x{height * 4}", "-depth", "8", "gray:-"]
    pixels = subprocess.run(command, check=True, capture_output=True).stdout
    stride = width * 2
    if len(pixels) != stride * height * 4:
        raise ValueError("Image conversion returned unexpected dimensions")
    dots = ((0, 0, 1), (0, 1, 2), (0, 2, 4), (1, 0, 8), (1, 1, 16), (1, 2, 32), (0, 3, 64), (1, 3, 128))
    lines = []
    for y in range(0, height * 4, 4):
        line = ""
        for x in range(0, stride, 2):
            code = sum(bit for dx, dy, bit in dots if pixels[(y + dy) * stride + x + dx] >= 128)
            line += chr(0x2800 + code) if code else " "
        lines.append(line.rstrip())
    art = "\n".join(lines).strip("\n")
    if not art.strip():
        raise ValueError("No artwork pixels found; choose an image with visible contrast")
    return art + "\n"


def about(args):
    action = args[0] if args else "show"
    target = STATE / "about.txt"
    if action in ("edit", "image", "reset"):
        if action == "reset":
            with state_lock():
                value = state(); value.pop("aboutArt", None); save_state(value)
            notify("About artwork", "Restored the NixOS logo")
            return
        if action == "image":
            art = image_art(args[1:], width=54)
            if art is None:
                return
            atomic_write(target, art)
        else:
            if target.is_symlink():
                raise ValueError("About artwork must be a writable state file, not a link")
            if not target.exists():
                atomic_write(target, "NixOS\n")
        with state_lock():
            value = state(); value["aboutArt"] = True; save_state(value)
        if action == "edit":
            launch(["editor", str(target)])
        else:
            notify("About artwork", "Image converted; open About to preview")
    elif action == "show":
        command = "fastfetch"
        if state().get("aboutArt") and target.is_file():
            command += " --logo-type file-raw --logo " + shlex.quote(str(target))
        terminal(["--shell", command + "; read -r -n 1 -s"])
    else:
        raise ValueError("about: show, edit, image [PATH], or reset")


def screensaver(args):
    action = args[0] if args else "start"
    name = state().get("screensaver", "naruto")
    if action == "current":
        print(name)
    elif action == "set" and len(args) == 2:
        if args[1] not in ("naruto", "tomoe-sharingan", "matrix", "custom"):
            raise ValueError("Unknown screensaver")
        if args[1] == "custom" and not (STATE / "screensaver.txt").is_file():
            raise ValueError("Use screensaver edit to create your artwork first")
        with state_lock():
            value = state(); value["screensaver"] = args[1]; save_state(value)
    elif action == "edit":
        target = STATE / "screensaver.txt"
        if target.is_symlink():
            raise ValueError("Custom screensaver artwork must be a writable state file, not a link")
        if not target.exists():
            atomic_write(target, (DATA / "screensavers/naruto.txt").read_text())
        screensaver(["set", "custom"])
        launch(["editor", str(target)])
    elif action == "image":
        art = image_art(args[1:])
        if art is not None:
            atomic_write(STATE / "screensaver.txt", art)
            screensaver(["set", "custom"])
            notify("Screensaver", "Image converted; choose Start to preview")
    elif action == "reset":
        screensaver(["set", "naruto"])
        notify("Screensaver", "Restored the Naruto artwork")
    elif action in ("start", "idle"):
        if action == "idle" and (STATE / "toggles/screensaver-off").exists():
            return
        if run(["pgrep", "-u", str(os.getuid()), "-x", "hyprlock"], check=False, capture=True).returncode == 0:
            return
        clients = json.loads(run(["hyprctl", "-j", "clients"], capture=True).stdout)
        if any(window.get("class") == "org.desktop.screensaver" for window in clients):
            return
        command = [sys.executable, __file__, "screensaver-worker"]
        os.execvp("ghostty", ["ghostty", "--class=org.desktop.screensaver", "--fullscreen=true", "--background-opacity=1",
                             "--font-size=18", "--window-padding-x=0", "--window-padding-y=0", "-e", *command])
    else:
        raise ValueError("screensaver: start, current, set NAME, edit, image [PATH], or reset")


def screensaver_worker(args):
    # Own only this child: leaving the saver never kills unrelated TTE processes.
    import select
    import termios
    import tty
    name = state().get("screensaver", "naruto")
    path = STATE / "screensaver.txt" if name == "custom" else DATA / "screensavers" / (name + ".txt")
    if name != "matrix" and not path.is_file():
        raise ValueError(f"Missing screensaver artwork: {path}")
    previous = termios.tcgetattr(sys.stdin)
    child = None
    try:
        tty.setcbreak(sys.stdin.fileno())
        while True:
            command = ["cmatrix", "-abs", "-u", "4"] if name == "matrix" else ["tte", "-i", str(path), "--frame-rate", "30", "--canvas-width", "0", "--canvas-height", "0",
                       "--anchor-canvas", "c", "--anchor-text", "c", "--random-effect", "--exclude-effects", "dev_worm", "--no-eol"]
            child = subprocess.Popen(command, stdin=None if name == "matrix" else subprocess.DEVNULL)
            while child.poll() is None:
                if select.select([sys.stdin], [], [], .2)[0]:
                    return
                try:
                    active = json.loads(run(["hyprctl", "-j", "activewindow"], capture=True, check=False).stdout)
                    if active.get("class") != "org.desktop.screensaver":
                        return
                except (OSError, ValueError):
                    return
            if child.returncode:
                raise ValueError("Screensaver animation exited unsuccessfully")
            if name == "matrix":
                return
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.kill(); child.wait()
        termios.tcsetattr(sys.stdin, termios.TCSADRAIN, previous)
        print("\033[?25h", end="", flush=True)


def menu(args):
    group = args[0] if args else "main"
    if group == "background":
        wallpaper(["grid"]); return
    if group == "reminder-set":
        delay = run([executable("walker"), "--dmenu", "--inputonly", "--placeholder", "Minutes"], input="", check=False, capture=True)
        if not delay.returncode and delay.stdout.strip():
            message = run([executable("walker"), "--dmenu", "--inputonly", "--placeholder", "Reminder"], input="", check=False, capture=True)
            if not message.returncode:
                reminder([delay.stdout.strip(), message.stdout.strip()])
        return
    prepare_theme()
    menus = read_json(CONFIG / "desktop/menus.json")
    stack = []
    while True:
        choices = dict(menus.get(group, {}).get("items", {}))
        if group == "theme":
            names = sorted({p.name for d in theme_directories() if d.is_dir() for p in d.iterdir() if p.is_dir()})
            choices = {("●  " if name == selected_theme() else "○  ") + name.replace("-", " ").title(): ["theme", "set", name] for name in names}
            choices["󰸌  Generate from wallpaper…"] = ["theme", "generate"]
        elif group == "font":
            families = run(["fc-list", ":spacing=mono", "--format", "%{family[0]}\n"], capture=True).stdout.splitlines()
            choices = {("●  " if family == selected_font() else "○  ") + family: ["font", "set", family] for family in sorted(set(families))}
        elif group == "config":
            choices = {name: ["config", name] for name in ["Hyprland configuration", "Hyprland", "Keybindings", "Input", "Monitors", "Hypridle", "Hyprlock", "Night light", "SwayOSD", "Walker", "Waybar", "XCompose", "Desktop menu"]}
        elif group in ("hardware", "toggle"):
            devices = json.loads(run(["hyprctl", "-j", "devices"], capture=True).stdout)
            if not any("touchpad" in m.get("name", "").lower() for m in devices.get("mice", [])):
                choices = {label: command for label, command in choices.items() if command != ["toggle", "touchpad"]}
        elif group == "power-profile":
            result = run(["powerprofilesctl", "list"], capture=True, check=False)
            choices = {name.replace("-", " ").title(): ["exec", "powerprofilesctl", "set", name] for name in ("power-saver", "balanced", "performance") if re.search(r"(?m)^\s*\*?\s*" + name + r":", result.stdout)}
        if not choices:
            raise ValueError(f"No choices available for menu: {group}")
        if stack:
            choices["󰁍  Back"] = ["back"]
        result = run([executable("walker"), "--theme", "desktop-menu", "--dmenu", "--width", "350", "--minheight", "1", "--maxheight", "630", "--placeholder", menus.get(group, {}).get("title", group.replace("-", " ").title() + "…")],
                     input="\n".join(choices) + "\n", check=False, capture=True)
        selected = result.stdout.strip()
        if result.returncode or selected not in choices:
            if not stack:
                return
            group = stack.pop()
            continue
        command, *rest = choices[selected]
        if command == "back":
            group = stack.pop()
        elif command == "menu" and rest[0] not in ("background", "reminder-set"):
            stack.append(group); group = rest[0]
        else:
            return dispatch(command, rest)


def transcode(args):
    backend = executable("desktop-transcode")
    if len(args) > 1 and all(expand(value).is_file() for value in args):
        for value in args:
            run([backend, value])
    else:
        run([backend, *args])


def keybindings():
    source = CONFIG / "hypr"
    lines = []
    for path in source.rglob("*.lua"):
        text = path.read_text()
        lines.extend(f"{key}  {description}" for key, description in re.findall(r'hl\.bind\(\s*"([^"]+)".*?description\s*=\s*"([^"]+)"', text, re.S))
    prepare_theme()
    run([executable("walker"), "--theme", "desktop-rounded", "--dmenu", "--placeholder", "Keybindings"], input="\n".join(sorted(set(lines))), check=False)


def seed_preferences():
    for kind, destination in [("config", CONFIG), ("state", STATE)]:
        defaults = DATA / "desktop-defaults" / kind
        if not defaults.is_dir():
            continue
        for source in defaults.rglob("*"):
            if not source.is_file():
                continue
            target = destination / source.relative_to(defaults)
            if target.exists() or target.is_symlink():
                continue
            # State toggles can be deliberately removed; seed them only once.
            if kind == "state" and (STATE / "initial-preferences-seeded").exists():
                continue
            atomic_write(target, source.read_bytes())
    atomic_write(STATE / "initial-preferences-seeded", "")


def seed_application_preferences():
    marker = STATE / "application-preferences-seeded"
    if marker.exists() or not all(shutil.which(name) for name in ("dconf", "gsettings")):
        return
    complete = True
    for schema, values in read_json(CONFIG / "desktop/applications.json").items():
        for key, value in values.items():
            path = "/" + schema.replace(".", "/") + "/" + key
            existing = run(["dconf", "read", path], check=False, capture=True)
            if existing.returncode:
                complete = False
                continue
            if existing.stdout.strip():
                continue
            result = run(["gsettings", "set", schema, key, json.dumps(value)], check=False)
            complete = complete and not result.returncode
    if complete:
        atomic_write(marker, "")


def integrate():
    seed_preferences()
    seed_application_preferences()
    defaults = CONFIG / "breakd/default.toml"
    preferences = CONFIG / "breakd/config.toml"
    if defaults.is_file() and not preferences.exists():
        atomic_write(preferences, defaults.read_text())
    manifest = read_json(CONFIG / "desktop/native-host.json")
    if manifest:
        manifest["path"] = str(HOME / ".local/bin/chatgpt-dictate-audio-duck-host")
        for browser in ("chromium", "google-chrome"):
            target = CONFIG / browser / "NativeMessagingHosts" / (manifest["name"] + ".json")
            if not target.is_symlink():
                atomic_write(target, json.dumps(manifest, indent=2) + "\n")


def session(args):
    desktop_environment = (
        "WAYLAND_DISPLAY", "DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP",
        "XDG_SESSION_TYPE", "XCURSOR_THEME", "XCURSOR_PATH", "XCURSOR_SIZE", "HYPRCURSOR_THEME", "HYPRCURSOR_SIZE", "GDK_BACKEND", "QT_QPA_PLATFORM",
        "QT_STYLE_OVERRIDE", "MOZ_ENABLE_WAYLAND", "ELECTRON_OZONE_PLATFORM_HINT", "OZONE_PLATFORM", "XCOMPOSEFILE",
    )
    names = [name for name in desktop_environment if name in os.environ]
    if names:
        run(["systemctl", "--user", "import-environment", *names], check=False)
        run(["dbus-update-activation-environment", "--systemd", *names], check=False)
    if shutil.which("uwsm"):
        run(["uwsm", "finalize", *names], check=False)
    prepare_theme()
    integrate()
    flags = STATE / "toggles/hypr/internal-display.conf"
    if flags.is_file() and shutil.which("hyprctl"):
        monitors = json.loads(run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
        if not any(not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled") for m in monitors):
            flags.unlink(); run(["hyprctl", "reload"], check=False)
    run(["systemctl", "--user", "start", "desktop-session.target"], check=False)
    recover_graphics_services()
    run(["hyprctl", "reload"], check=False)


def recover_graphics_services():
    # A compositor restart replaces its socket. Services that exhausted their
    # retries against the old socket need another start after environment import.
    result = run(["systemctl", "--user", "--failed", "--plain", "--no-legend", "--no-pager"],
                 check=False, capture=True)
    graphics = {"desktop-awww.service", "desktop-wallpaper.service", "desktop-polkit.service",
                "xdg-desktop-portal.service", "xdg-desktop-portal-hyprland.service"}
    failed = sorted({words[0] for line in (result.stdout or "").splitlines()
                     if (words := line.lstrip("● ").split()) and words[0] in graphics})
    if failed:
        run(["systemctl", "--user", "reset-failed", *failed], check=False)
        run(["systemctl", "--user", "start", *failed], check=False)


def monitor_watch(args):
    signature = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")
    if not signature:
        raise ValueError("Monitor watching requires a Hyprland session")
    runtime = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}"))
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(str(runtime / "hypr" / signature / ".socket2.sock"))
        with connection.makefile() as events:
            for event in events:
                if not event.startswith(("monitorremoved", "monitoradded")):
                    continue
                marker = STATE / "toggles/hypr/internal-display.conf"
                if not marker.exists():
                    continue
                monitors = json.loads(run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
                external = [m for m in monitors if not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled")]
                if not external:
                    marker.unlink(missing_ok=True)
                    run(["hyprctl", "reload"], check=False)
                    wake()


def dispatch(command, args):
    functions = {"theme": theme, "wallpaper": wallpaper, "terminal": terminal, "launch": launch, "run": run_application,
                 "launcher": launcher, "toggle": toggle, "display": display, "brightness": brightness, "osd": osd,
                 "lock": lock, "lock-worker": lock_worker, "idle-worker": idle_worker, "capture": capture, "reminder": reminder, "menu": menu,
                 "transcode": transcode, "session": session, "monitor-watch": monitor_watch,
                 "font": font, "appearance": appearance, "config": edit_config, "clipboard": clipboard,
                 "share": share, "about": about,
                 "screensaver": screensaver, "screensaver-worker": screensaver_worker}
    if command in functions:
        return functions[command](args)
    if command == "prepare":
        source, _, light = prepare_theme()
        apply_appearance(source, light)
        integrate()
    elif command == "seed-preferences":
        seed_preferences()
    elif command == "cwd":
        print(cwd())
    elif command == "path":
        if len(args) != 1:
            raise ValueError("path requires a tool name")
        print(executable(args[0]))
    elif command == "restart":
        return restart(args[0] if args else "theme")
    elif command == "wake":
        wake()
    elif command == "keybindings":
        keybindings()
    elif command == "integrate":
        integrate()
    elif command == "weather":
        os.execv(str(CONFIG / "waybar/weather.sh"), [str(CONFIG / "waybar/weather.sh")])
    elif command == "battery":
        batteries = list(Path("/sys/class/power_supply").glob("BAT*/capacity"))
        print(" · ".join(p.read_text().strip() + "%" for p in batteries) or "No battery")
        if "--notify" in args:
            low = [p for p in batteries if int(p.read_text()) <= 10 and (p.parent / "status").read_text().strip() == "Discharging"]
            marker = RUNTIME / "battery-notified"
            if low and not marker.exists():
                run(["notify-send", "-u", "critical", "Time to recharge", f"Battery is down to {low[0].read_text().strip()}%"], check=False)
                atomic_write(marker, "")
            elif not low:
                marker.unlink(missing_ok=True)
    elif command == "audio":
        if args == ["mic-mute"]:
            run(["wpctl", "set-mute", "@DEFAULT_SOURCE@", "toggle"])
        elif args == ["switch"]:
            sinks = run(["pactl", "list", "short", "sinks"], capture=True).stdout.splitlines()
            current = run(["pactl", "get-default-sink"], capture=True).stdout.strip()
            names = [line.split()[1] for line in sinks]
            if names:
                run(["pactl", "set-default-sink", names[(names.index(current) + 1) % len(names)] if current in names else names[0]])
        else:
            raise ValueError("audio: mic-mute or switch")
    elif command == "dictation-status":
        result = run(["dictate", "live", "status"], check=False, capture=True)
        print(json.dumps({"text": "󰍬" if "recording" in result.stdout.lower() else "", "tooltip": result.stdout.strip(), "class": "dictation"}))
    elif command == "iphone-folder":
        run(["nautilus", HOME / "From iPhone"])
    elif command == "power":
        if args == ["logout"]:
            run(["uwsm", "stop"])
        elif args == ["suspend"]:
            # Share the Rust backend's capability and lock-inhibitor checks.
            result = run([executable("desktop-ring"), "action", '{"kind":"suspend"}'],
                         check=False, capture=True)
            if result.returncode:
                raise ValueError(result.stderr.strip() or "Sleep request failed")
        elif args[:1] in (["reboot"], ["poweroff"]):
            run(["systemctl", args[0]])
        else:
            raise ValueError("power: suspend, reboot, poweroff, or logout")
    elif command == "guide":
        root = expand(os.environ.get("DOTFILES", HOME / "nixos-config/dotfiles")).resolve().parent
        launch(["editor", str(root / "docs/DESKTOP-MENUS.md")])
    elif command == "exec" and args:
        os.execvp(args[0], args)
    else:
        raise ValueError(f"Unknown command: {command}")
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", help="theme, font, appearance, wallpaper, clipboard, launcher, menu, launch, terminal, capture, reminder, toggle, lock, screensaver, config, session, prepare, or path")
    parser.add_argument("args", nargs=argparse.REMAINDER)
    parsed = parser.parse_args(argv)
    try:
        return dispatch(parsed.command, parsed.args) or 0
    except (ValueError, OSError, subprocess.CalledProcessError, tomllib.TOMLDecodeError) as error:
        print(f"desktopctl: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
