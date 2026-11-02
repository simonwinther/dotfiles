"""Shared bar selection and native renderer settings for both command frontends."""

import math
import socket
import json

DEFAULT_BAR = "native-clean"
NATIVE_BARS = ("native-clean", "native-motion")


def names():
    return list(NATIVE_BARS)


def selected(preferences):
    name = preferences.get("bar", DEFAULT_BAR)
    return name if name in NATIVE_BARS else DEFAULT_BAR


def open_panel(runtime, panel, preferences, *, force=False):
    """Both styles share the Morphing Notch Bar menu surface."""
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as client:
            client.sendto(panel.encode(), str(runtime / "bar.sock"))
        return True
    except OSError:
        return False


def launcher_panel(arguments):
    providers = []
    query = ""
    options = iter(arguments)
    for option in options:
        if option in ("-m", "--modules"):
            value = next(options, None)
            if value is None:
                raise ValueError("Launcher modules require a provider")
            providers = value.split(",")
        elif option.startswith("--modules="):
            providers = option.split("=", 1)[1].split(",")
        elif option in ("-s", "--search", "--query"):
            query = next(options, None)
            if query is None:
                raise ValueError("Launcher search requires a query")
        elif option in ("-p", "--placeholder"):
            if next(options, None) is None:
                raise ValueError("Launcher placeholder requires text")
        else:
            raise ValueError(f"Unknown launcher option: {option}")
    available = {"apps", "desktopapplications", "websearch", "files", "calc", "runner",
                 "symbols", "unicode", "todo", "providerlist", "clipboard", "bluetooth", "menus"}
    if any(provider not in available and not (provider.startswith("menus:") and provider[6:])
           for provider in providers):
        raise ValueError("Unknown launcher provider")
    if len(providers) > 32 or len(query) > 4096:
        raise ValueError("Launcher request exceeds its limits")
    if not providers and not query:
        return "apps"
    if providers == ["clipboard"] and not query:
        return "clipboard"
    if providers == ["menus:notifications"] and not query:
        return "notifications"
    panel = "launcher/" + json.dumps({"providers": providers, "query": query}, ensure_ascii=False, separators=(",", ":"))
    if len(panel.encode()) > 16 * 1024:
        raise ValueError("Launcher request exceeds its limits")
    return panel


def visibility_marker(state):
    marker = state / "toggles/bar-off"
    legacy = state / "toggles/waybar-off"
    if legacy.exists():
        if marker.exists():
            legacy.unlink()
        else:
            legacy.replace(marker)
    return marker


def theme_data(preferences, defaults, palette, light, outputs):
    scale = outputs.get("scale", 1.15)
    if (
        type(scale) not in (int, float)
        or not math.isfinite(scale)
        or not 0.5 <= scale <= 3
    ):
        scale = 1.15

    def patterns(key, fallback):
        value = outputs.get(key, fallback)
        return (
            value
            if isinstance(value, list) and all(isinstance(p, str) and p for p in value)
            else fallback
        )

    return {
        "style": selected(preferences),
        "font": preferences.get(
            "font", defaults.get("defaultFont", "JetBrainsMono Nerd Font")
        ),
        "palette": palette,
        "light": light,
        "scale": scale,
        "outputs": {
            "full": patterns("full", ["*"]),
            "minimal": patterns("minimal", []),
        },
    }
