"""Applications component with injected dependencies."""
from __future__ import annotations
import json
import bar_catalog
import bar_config
from pathlib import Path
import re
import shlex
import signal
import socket
import subprocess
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .catalog import ThemeCatalog
    from .theme_render import ThemeRenderer


@dataclass
class Applications:
    ctx: Context
    store: StateStore
    catalog: ThemeCatalog
    renderer: ThemeRenderer

    def cwd(self) -> str:
        try:
            window = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activewindow"], capture=True).stdout)
            pid = str(window.get("pid", ""))
            children = self.ctx.processes.run(["pgrep", "-P", pid], check=False, capture=True).stdout.split()
            for child in reversed(children):
                path = (self.ctx.paths.proc / f'{child}/cwd').resolve()
                if path.is_dir():
                    return str(path)
        except (subprocess.CalledProcessError, OSError, ValueError):
            pass
        return str(self.ctx.paths.home)


    def terminal(self, args: list[str]) -> int | None:
        args = list(args)
        floating = "--float" in args
        shell = "--shell" in args
        args = [x for x in args if x not in ("--float", "--shell")]
        app_id = "TUI.float" if floating else "org.desktop.terminal"
        if args and args[0] == "--app-id":
            app_id, args = args[1], args[2:]
        command = ["xdg-terminal-exec", "--dir=" + self.cwd(), "--app-id=" + app_id]
        if args:
            command += ["-e", "bash", "-lc", args[0]] if shell else ["-e", *args]
        command = bar_catalog.application_command(command)
        self.ctx.processes.execvp(command[0], command)


    def launch(self, args: list[str]) -> int | None:
        if not args:
            raise ValueError("launch: browser, webapp URL, tui COMMAND, editor, focus CLASS COMMAND, audio, bluetooth, wifi")
        what, rest = args[0], args[1:]
        if what == "focus":
            if not rest:
                raise ValueError("focus requires a window pattern and optional launch command")
            windows = json.loads(self.ctx.processes.run(["hyprctl", "-j", "clients"], capture=True).stdout)
            pattern = re.compile(rest[0], re.IGNORECASE)
            found = next((w for w in windows if pattern.search(w.get("class", "")) or pattern.search(w.get("title", ""))), None)
            if found:
                selector = json.dumps("address:" + found["address"])
                self.ctx.processes.run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.focus({{ window = {selector} }}))'])
            else:
                command = bar_catalog.application_command([rest[0]] if len(rest) == 1 else shlex.split(rest[1]) if len(rest) == 2 else rest[1:])
                self.ctx.processes.execvp(command[0], command)
        elif what in ("audio", "bluetooth", "wifi"):
            if what in ("audio", "bluetooth", "wifi") and not rest and bar_config.open_panel(self.ctx.paths.runtime, what, self.store.state()):
                return
            self.terminal(["--app-id", "org.desktop." + {"audio": "wiremix", "bluetooth": "bluetui", "wifi": "impala"}[what], {"audio": "wiremix", "bluetooth": "bluetui", "wifi": "impala"}[what]])
        elif what == "tui":
            if not rest:
                raise ValueError("tui requires a command")
            self.terminal(["--app-id", "org.desktop." + Path(rest[0]).name, *rest])
        elif what == "editor":
            self.terminal([self.store.settings().get("editor", "nvim"), *rest])
        elif what in ("browser", "webapp"):
            command = [self.store.settings().get("browser", "chromium")]
            if what == "webapp":
                if len(rest) != 1 or not re.match(r"https?://", rest[0]):
                    raise ValueError("webapp requires one HTTP(S) URL")
                command += ["--app=" + rest[0]]
            else:
                command += ["--incognito" if x == "--private" else x for x in rest]
            command = bar_catalog.application_command(command)
            self.ctx.processes.execvp(command[0], command)
        else:
            raise ValueError(f"Unknown launch action: {what}")


    def run_application(self, args: list[str]) -> int | None:
        if not args:
            raise ValueError("run requires an application")
        app, rest = args[0], args[1:]
        if rest[:1] == ["--"]:
            rest = rest[1:]
        self.renderer.prepare_theme()
        raw = self.ctx.executable(app)
        if app == "mako":
            source = (self.ctx.paths.config / "mako/config").read_text().replace("~/.local/state/desktop", str(self.ctx.paths.state))
            target = self.ctx.paths.runtime / "mako.conf"; storage.atomic_write(target, source)
            rest = ["--config", str(target), *rest]
        elif app == "swayosd-server":
            css = (self.ctx.paths.state / "theme/swayosd.css").read_text() + (self.ctx.paths.config / "swayosd/style.css").read_text()
            css += "\nlabel { font-family: " + json.dumps(self.catalog.selected_font()) + "; }\n"
            target = self.ctx.paths.runtime / "swayosd.css"; storage.atomic_write(target, css)
            rest = ["--style", str(target), *rest]
        elif app == "btop":
            rest = ["--themes-dir", str(self.ctx.paths.state / "theme"), *rest]
        self.ctx.processes.execv(raw, [raw, *rest])


    def edit_config(self, args: list[str]) -> int | None:
        choices = {
            "Hyprland": "hypr/.config/hypr/looknfeel.lua", "Keybindings": "hypr/.config/hypr/bindings.lua",
            "Hyprland configuration": "hypr/.config/hypr/hyprland.lua",
            "Input": "hypr/.config/hypr/input.lua", "Monitors": "hypr/.config/hypr/monitors.lua",
            "Hypridle": "hypr/.config/hypr/hypridle.conf", "Hyprlock": "hypr/.config/hypr/hyprlock.conf",
            "Night light": "hypr/.config/hypr/hyprsunset.conf", "Desktop search": "desktop-search/.config/desktop-search/config.toml",
            "Weather": "desktop-bar/.config/desktop/weather.json", "Desktop menu": "desktop-settings/.config/desktop/menus.json",
            "SwayOSD": "appearance/.config/swayosd/config.toml", "XCompose": "desktop-settings/.XCompose",
            "Application launchers": "appearance/.config/desktop/settings.json",
            "Default terminal": "desktop-settings/.config/xdg-terminals.list",
            "File and URL handlers": "desktop-settings/.config/mimeapps.list",
        }
        if len(args) != 1 or args[0] not in choices:
            raise ValueError("config: " + ", ".join(choices))
        root = self.ctx.expand(self.ctx.env.get("DOTFILES", self.ctx.paths.home / "nixos-config/dotfiles")).resolve()
        if args[0] == "Monitors":
            host = socket.gethostname()
            path = root.parent / "hosts" / (host if host in ("lambda", "desktop") else "desktop") / "monitors.lua"
        else:
            path = root / choices[args[0]]
        if not path.is_file() or str(root).startswith("/nix/store/"):
            raise ValueError("Open an editable nixos-config checkout and set DOTFILES to its dotfiles directory")
        self.launch(["editor", str(path)])


    def transcode(self, args: list[str]) -> int | None:
        backend = self.ctx.executable("desktop-transcode")
        if not (len(args) > 1 and all(self.ctx.expand(value).is_file() for value in args)):
            # Keep the public command's PID through the picker and Rust supervisor.
            self.ctx.processes.execv(backend, [backend, *args])
            return

        child = None
        cancelled = 0

        def cancel(signum, _frame):
            nonlocal cancelled
            cancelled = signum
            if child is not None and child.poll() is None:
                try:
                    child.send_signal(signum)
                except ProcessLookupError:
                    pass

        previous = {number: signal.signal(number, cancel)
                    for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
        try:
            for value in args:
                if cancelled:
                    raise SystemExit(128 + cancelled)
                command = [backend, str(self.ctx.expand(value))]
                child = self.ctx.processes.spawn(command)
                # A signal can arrive between the loop check and starting the child.
                if cancelled:
                    cancel(cancelled, None)
                status = child.wait()
                if cancelled:
                    raise SystemExit(128 + cancelled)
                if status:
                    raise subprocess.CalledProcessError(status, command)
        finally:
            for number, handler in previous.items():
                signal.signal(number, handler)


    def weather(self, args: list[str]) -> None:
        self.ctx.processes.execvp("desktop-bar", ["desktop-bar", "--weather", *args])


    def dictation_status(self, args: list[str]) -> None:
        result = self.ctx.processes.run(["dictate", "live", "status"], check=False, capture=True)
        print(json.dumps({"text": "󰍬" if "recording" in result.stdout.lower() else "", "tooltip": result.stdout.strip(), "class": "dictation"}))


    def iphone_folder(self, args: list[str]) -> None:
        self.ctx.processes.run(["nautilus", self.ctx.paths.home / "From iPhone"])


    def guide(self, args: list[str]) -> None:
        root = self.ctx.expand(self.ctx.env.get("DOTFILES", self.ctx.paths.home / "nixos-config/dotfiles")).resolve().parent
        self.launch(["editor", str(root / "docs/DESKTOP-MENUS.md")])


    def execute(self, args: list[str]) -> None:
        if not args:
            raise ValueError("Unknown command: exec")
        self.ctx.processes.execvp(args[0], args)
