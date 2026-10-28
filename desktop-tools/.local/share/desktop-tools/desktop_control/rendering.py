"""Rendering helpers use explicit inputs and never launch desktop processes."""
from __future__ import annotations
import json
import re
import tomllib
from xml.sax.saxutils import escape as xml_escape
from . import storage


def prepare_terminal_theme(target, c, follow_theme, family):
    quoted = json.dumps(family)
    ghostty = f"font-family = \"\"\nfont-family = {quoted}\n"
    if follow_theme:
        ghostty += "theme =\n" + "\n".join(f"{key} = {c[key]}" for key in ("background", "foreground")) + f"\ncursor-color = {c['cursor']}\n"
        ghostty += "\n".join(f"palette = {i}={c[f'color{i}']}" for i in range(16)) + "\n"
        ghostty += f"selection-background = {c.get('selection_background', c['accent'])}\nselection-foreground = {c.get('selection_foreground', c['background'])}\n"
    storage.atomic_write(target / "ghostty.conf", ghostty)
    kitty = "\n".join(f"{key} {c[key]}" for key in ("background", "foreground", "cursor")) + "\n"
    kitty += "\n".join(f"color{i} {c[f'color{i}']}" for i in range(16)) + f"\nfont_family {family}\n"
    storage.atomic_write(target / "kitty.conf", kitty)
    alacritty = f"[font]\nnormal = {{ family = {quoted}, style = 'Regular' }}\nbold = {{ family = {quoted}, style = 'Bold' }}\nitalic = {{ family = {quoted}, style = 'Italic' }}\n"
    alacritty += f"[colors.primary]\nbackground = '{c['background']}'\nforeground = '{c['foreground']}'\n[colors.cursor]\ncursor = '{c['cursor']}'\ntext = '{c['background']}'\n"
    names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"]
    for offset, section in [(0, "normal"), (8, "bright")]:
        alacritty += f"[colors.{section}]\n" + "\n".join(f"{name} = '{c[f'color{i + offset}']}'" for i, name in enumerate(names)) + "\n"
    storage.atomic_write(target / "alacritty.toml", alacritty)
    storage.atomic_write(target / "fontconfig.conf", '<?xml version="1.0"?>\n<fontconfig><match target="pattern"><test name="family"><string>monospace</string></test><edit name="family" mode="assign" binding="strong"><string>' + xml_escape(family) + '</string></edit></match></fontconfig>\n')


def colors(path):
    return _theme_colors((path / "colors.toml").read_text(), path.name)


def _theme_colors(content, name):
    palette = tomllib.loads(content)
    for key in ("accent", "foreground", "background", *[f"color{i}" for i in range(16)]):
        if not re.fullmatch(r"#[0-9a-fA-F]{6}", palette.get(key, "")):
            raise ValueError(f"Invalid or missing color {key} in {name}")
    palette.setdefault("cursor", palette["accent"])
    return palette


def hyprlock_palette(c):
    return "\n".join(f"${k} = rgba({v[1:]}ff)" for k, v in {
        "color": c["background"], "inner_color": c["background"], "outer_color": c["foreground"],
        "font_color": c["foreground"], "check_color": c["accent"],
    }.items()) + "\n"


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
