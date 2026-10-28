"""Session component with injected dependencies."""
from __future__ import annotations
import contextlib
import json
from pathlib import Path
import socket
import subprocess
import desktop_process as processes
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .theme_render import ThemeRenderer
    from .appearance import Appearance
    from .controls import Controls
    from .preferences import Preferences


@dataclass
class Session:
    ctx: Context
    store: StateStore
    renderer: ThemeRenderer
    appearance: Appearance
    controls: Controls
    preferences: Preferences

    def lock(self, args: list[str]) -> int | None:
        if self.ctx.processes.run(["pgrep", "-u", str(self.ctx.uid), "-x", "hyprlock"], check=False, capture=True).returncode == 0:
            return
        unit = "desktop-lock-only.service" if "--only" in args or self.ctx.env.get("DESKTOP_LOCK_ONLY") == "true" else "desktop-lock.service"
        result = self.ctx.processes.run(["systemctl", "--user", "start", unit], check=False, capture=True)
        if result.returncode:
            self.ctx.processes.background([self.ctx.python, str(self.ctx.entrypoint), "lock-worker", *(args or [])], operation="lock")


    def idle_worker(self, args: list[str]) -> int | None:
        config = "hypridle.conf" if (self.ctx.paths.runtime / "idle-enabled").exists() else "hypridle-sleep.conf"
        binary = self.ctx.executable("hypridle")
        self.ctx.processes.execv(binary, [binary, "--config", str(self.ctx.paths.config / "hypr" / config)])


    def lock_worker(self, args: list[str]) -> int | None:
        with self.store.state_lock("screen-lock", wait=False) as acquired:
            if not acquired:
                return
            if self.ctx.processes.which("1password"):
                self.ctx.processes.background(["1password", "--lock"], operation="control")
            self.ctx.processes.run(["hyprctl", "switchxkblayout", "all", "0"], check=False)
            source = self.appearance.lock_config()
            target = self.ctx.paths.runtime / "hyprlock.conf"; storage.atomic_write(target, source)
            with self.ctx.processes.managed(["hyprlock", "--config", str(target)], operation="lock") as child:
                try:
                    if "--only" not in args:
                        self.ctx.clock.sleep(3)
                        if child.poll() is None:
                            self.ctx.processes.run(["hyprctl", "dispatch", "dpms", "off"], check=False)
                            with contextlib.suppress(ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
                                self.controls.brightness(["keyboard", "off"])
                    result = child.wait()
                finally:
                    self.controls.wake()
            if result:
                raise processes.ProcessError(child)


    def session(self, args: list[str]) -> int | None:
        desktop_environment = (
            "WAYLAND_DISPLAY", "DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP",
            "XDG_SESSION_TYPE", "XCURSOR_THEME", "XCURSOR_PATH", "XCURSOR_SIZE", "HYPRCURSOR_THEME", "HYPRCURSOR_SIZE", "GDK_BACKEND", "QT_QPA_PLATFORM",
            "QT_STYLE_OVERRIDE", "MOZ_ENABLE_WAYLAND", "ELECTRON_OZONE_PLATFORM_HINT", "OZONE_PLATFORM", "XCOMPOSEFILE",
        )
        names = [name for name in desktop_environment if name in self.ctx.env]
        if names:
            self.ctx.processes.run(["systemctl", "--user", "import-environment", *names], check=False)
            self.ctx.processes.run(["dbus-update-activation-environment", "--systemd", *names], check=False)
        if self.ctx.processes.which("uwsm"):
            self.ctx.processes.run(["uwsm", "finalize", *names], check=False)
        self.renderer.prepare_theme()
        self.preferences.integrate()
        flags = self.ctx.paths.state / "toggles/hypr/internal-display.conf"
        if flags.is_file() and self.ctx.processes.which("hyprctl"):
            monitors = json.loads(self.ctx.processes.run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
            if not any(not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled") for m in monitors):
                flags.unlink(); self.ctx.processes.run(["hyprctl", "reload"], check=False)
        self.ctx.processes.run(["systemctl", "--user", "start", "desktop-session.target"], check=False)
        self.recover_graphics_services()
        self.ctx.processes.run(["hyprctl", "reload"], check=False)


    def recover_graphics_services(self) -> None:
        # A compositor restart replaces its socket. Services that exhausted their
        # retries against the old socket need another start after environment import.
        result = self.ctx.processes.run(["systemctl", "--user", "--failed", "--plain", "--no-legend", "--no-pager"],
                     check=False, capture=True)
        graphics = {"desktop-awww.service", "desktop-wallpaper.service", "desktop-polkit.service",
                    "xdg-desktop-portal.service", "xdg-desktop-portal-hyprland.service"}
        failed = sorted({words[0] for line in (result.stdout or "").splitlines()
                         if (words := line.lstrip("● ").split()) and words[0] in graphics})
        if failed:
            self.ctx.processes.run(["systemctl", "--user", "reset-failed", *failed], check=False)
            self.ctx.processes.run(["systemctl", "--user", "start", *failed], check=False)


    def monitor_watch(self, args: list[str]) -> int | None:
        signature = self.ctx.env.get("HYPRLAND_INSTANCE_SIGNATURE")
        if not signature:
            raise ValueError("Monitor watching requires a Hyprland session")
        runtime = Path(self.ctx.env.get("XDG_RUNTIME_DIR", f"/run/user/{self.ctx.uid}"))
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.connect(str(runtime / "hypr" / signature / ".socket2.sock"))
            with connection.makefile() as events:
                for event in events:
                    if not event.startswith(("monitorremoved", "monitoradded")):
                        continue
                    marker = self.ctx.paths.state / "toggles/hypr/internal-display.conf"
                    if not marker.exists():
                        continue
                    monitors = json.loads(self.ctx.processes.run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
                    external = [m for m in monitors if not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled")]
                    if not external:
                        marker.unlink(missing_ok=True)
                        self.ctx.processes.run(["hyprctl", "reload"], check=False)
                        self.controls.wake()
