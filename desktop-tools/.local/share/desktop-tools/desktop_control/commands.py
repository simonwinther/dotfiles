"""Typed command boundary shared by the CLI and menu selections."""
from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from enum import Enum


class CommandName(str, Enum):
    THEME = "theme"
    FONT = "font"
    APPEARANCE = "appearance"
    WALLPAPER = "wallpaper"
    TERMINAL = "terminal"
    LAUNCH = "launch"
    RUN = "run"
    LAUNCHER = "launcher"
    CLIPBOARD = "clipboard"
    CLIPBOARD_PASTE = "clipboard-paste"
    TOGGLE = "toggle"
    DISPLAY = "display"
    BRIGHTNESS = "brightness"
    OSD = "osd"
    LOCK = "lock"
    LOCK_WORKER = "lock-worker"
    IDLE_WORKER = "idle-worker"
    CAPTURE = "capture"
    REMINDER = "reminder"
    MENU = "menu"
    TRANSCODE = "transcode"
    SESSION = "session"
    MONITOR_WATCH = "monitor-watch"
    CONFIG = "config"
    SHARE = "share"
    ABOUT = "about"
    SCREENSAVER = "screensaver"
    SCREENSAVER_WORKER = "screensaver-worker"
    PREPARE = "prepare"
    SEED_PREFERENCES = "seed-preferences"
    CWD = "cwd"
    PATH = "path"
    RESTART = "restart"
    WAKE = "wake"
    KEYBINDINGS = "keybindings"
    INTEGRATE = "integrate"
    WEATHER = "weather"
    BATTERY = "battery"
    AUDIO = "audio"
    DICTATION_STATUS = "dictation-status"
    IPHONE_FOLDER = "iphone-folder"
    POWER = "power"
    GUIDE = "guide"
    EXEC = "exec"


@dataclass(frozen=True)
class Command:
    name: CommandName
    args: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if not isinstance(self.name, CommandName):
            raise ValueError("Command name must be a CommandName")
        if not isinstance(self.args, tuple) or any(not isinstance(arg, str) for arg in self.args):
            raise ValueError("Command arguments must be a tuple of strings")

    @classmethod
    def parse(cls, name: str, args: Sequence[str] = ()) -> Command:
        try:
            kind = CommandName(name)
        except ValueError:
            raise ValueError(f"Unknown command: {name}") from None
        if isinstance(args, str):
            raise ValueError("Command arguments must be a sequence of strings")
        return cls(kind, tuple(args))


Handler = Callable[[list[str]], int | None]


class Dispatcher:
    def __init__(self, handlers: Mapping[CommandName, Handler]) -> None:
        missing = set(CommandName) - handlers.keys()
        if missing:
            raise ValueError("Missing command handlers: " + ", ".join(sorted(name.value for name in missing)))
        self.handlers = dict(handlers)

    def dispatch(self, command: Command) -> int:
        return self.handlers[command.name](list(command.args)) or 0
