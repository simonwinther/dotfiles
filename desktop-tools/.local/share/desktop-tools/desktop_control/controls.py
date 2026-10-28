"""Controls component with injected dependencies."""
from __future__ import annotations
import contextlib
import json
from pathlib import Path
import subprocess
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .services import Services


@dataclass
class Controls:
    ctx: Context
    store: StateStore
    services: Services

    def toggle(self, args: list[str]) -> int | None:
        if not args:
            raise ValueError("toggle requires a feature")
        feature = args[0]
        marker = self.ctx.paths.state / "toggles" / feature
        if feature == "enabled":
            return 0 if len(args) > 1 and (self.ctx.paths.state / "toggles" / args[1]).exists() else 1
        if feature == "idle":
            # Automatic locking is opt-in for this login. Sleep protection always runs.
            with self.store.state_lock("idle"):
                marker = self.ctx.paths.runtime / "idle-enabled"
                enabled = not marker.exists()
                if args[1:2] in (["on"], ["off"]):
                    enabled = args[1] == "on"
                if enabled:
                    storage.atomic_write(marker, "")
                else:
                    marker.unlink(missing_ok=True)
                self.ctx.processes.run(["systemctl", "--user", "restart", "desktop-hypridle.service"])
            return
        services = {"bar": ["desktop-waybar", "desktop-waybar-minimal", "desktop-workspaces"], "nightlight": ["desktop-hyprsunset"]}
        if feature in services:
            services[feature] = self.services.selected_units(services[feature])
            service = services[feature][0] + ".service"
            active = self.ctx.processes.run(["systemctl", "--user", "is-active", "--quiet", service], check=False).returncode == 0
            if feature == "bar":
                with self.store.state_lock():
                    disabled = self.ctx.paths.state / "toggles/waybar-off"
                    if active:
                        storage.atomic_write(disabled, "")
                    else:
                        disabled.unlink(missing_ok=True)
            self.ctx.processes.run(["systemctl", "--user", "stop" if active else "start", *[x + ".service" for x in services[feature]]])
            return
        if feature == "notifications":
            self.ctx.processes.run(["makoctl", "mode", "-t", "do-not-disturb"])
            return
        if feature == "screensaver":
            with self.store.state_lock():
                marker = self.ctx.paths.state / "toggles/screensaver-off"
                if marker.exists():
                    marker.unlink()
                    enabled = True
                else:
                    storage.atomic_write(marker, "")
                    enabled = False
            self.ctx.notify("Screensaver", "Automatic screensaver enabled" if enabled else "Automatic screensaver disabled")
            return
        if feature == "layout":
            workspace = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activeworkspace"], capture=True).stdout)
            layout = "scrolling" if workspace.get("tiledLayout") == "dwindle" else "dwindle"
            self.ctx.processes.run(["hyprctl", "eval", f'hl.workspace_rule({{ workspace = {json.dumps(str(workspace["id"]))}, layout = {json.dumps(layout)} }})'])
            return
        with self.store.state_lock():
            enabled = not marker.exists()
            if args[1:2] in (["on"], ["off"]):
                enabled = args[1] == "off"  # marker records the disabled state
            if enabled:
                storage.atomic_write(marker, "")
            else:
                marker.unlink(missing_ok=True)
            values = {"gaps": "general {\n gaps_in = 0\n gaps_out = 0\n}\n", "square": "layout {\n single_window_aspect_ratio = 1 1\n}\n"}
            if feature == "touchpad":
                devices = json.loads(self.ctx.processes.run(["hyprctl", "-j", "devices"], capture=True).stdout)
                values[feature] = "".join("device {\n name = " + m["name"] + "\n enabled = false\n}\n" for m in devices.get("mice", []) if "touchpad" in m["name"])
            if feature not in values:
                marker.unlink(missing_ok=True)
                raise ValueError(f"Unknown toggle: {feature}")
            path = self.ctx.paths.state / "toggles/hypr" / (feature + ".conf")
            if enabled:
                storage.atomic_write(path, values[feature])
            else:
                path.unlink(missing_ok=True)
        self.ctx.processes.run(["hyprctl", "reload"])


    def display(self, args: list[str]) -> int | None:
        monitors = json.loads(self.ctx.processes.run(["hyprctl", "-j", "monitors", "all"], capture=True).stdout)
        external = [m for m in monitors if not m["name"].startswith(("eDP", "LVDS")) and not m.get("disabled")]
        if args == ["external"]:
            return 0 if external else 1
        if (self.ctx.paths.config / "desktop/ring.json").exists():
            command = "rescan" if args[-1:] in (["on"], ["off"]) else "menu"
            return self.ctx.processes.run(["desktop-ring", command], check=False).returncode
        internal = next((m for m in monitors if m["name"].startswith(("eDP", "LVDS"))), None)
        if not internal:
            raise ValueError("No internal display is present")
        action = args[1] if len(args) > 1 else "toggle"
        mirror = args[:1] == ["mirror"]
        file = self.ctx.paths.state / "toggles/hypr/internal-display.conf"
        disabled = not file.exists() if action == "toggle" else action == "off"
        if disabled and not external:
            raise ValueError("Refusing to disable the only available display")
        if mirror and action == "toggle" and file.exists() and "mirror" in file.read_text():
            file.unlink()
        elif mirror and external and action != "off":
            storage.atomic_write(file, f"monitor = {internal['name']}, preferred, auto, auto, mirror, {external[0]['name']}\n")
        elif disabled:
            storage.atomic_write(file, f"monitor = {internal['name']}, disable\n")
        else:
            file.unlink(missing_ok=True)
        self.ctx.processes.run(["hyprctl", "reload"])


    def brightness(self, args: list[str]) -> int | None:
        if len(args) < 2:
            raise ValueError("brightness requires display|keyboard and an adjustment")
        kind, value = args[:2]
        if kind == "display" and value in ("on", "off"):
            self.ctx.processes.run(["hyprctl", "dispatch", "dpms", value]); return
        directory = Path("/sys/class/leds" if kind == "keyboard" else "/sys/class/backlight")
        devices = sorted(p.name for p in directory.glob("*") if kind != "keyboard" or "kbd_backlight" in p.name)
        if not devices:
            raise ValueError(f"No {kind} brightness device is available")
        device = devices[0]
        if kind == "keyboard":
            if value in ("off", "restore"):
                self.ctx.processes.run(["brightnessctl", "-d", device, "-s" if value == "off" else "-r", *(["set", "0"] if value == "off" else [])]); return
            current = int(self.ctx.processes.run(["brightnessctl", "-d", device, "get"], capture=True).stdout)
            maximum = int(self.ctx.processes.run(["brightnessctl", "-d", device, "max"], capture=True).stdout)
            step = max(1, maximum // 10)
            target = (current + step) % (maximum + 1) if value == "cycle" else min(maximum, max(0, current + (step if value == "up" else -step)))
            value = str(target)
        self.ctx.processes.run(["brightnessctl", "-d", device, "set", value])
        percent = round(100 * int(self.ctx.processes.run(["brightnessctl", "-d", device, "get"], capture=True).stdout) / int(self.ctx.processes.run(["brightnessctl", "-d", device, "max"], capture=True).stdout))
        self.ctx.processes.run(["swayosd-client", "--custom-icon", "keyboard-brightness-symbolic" if kind == "keyboard" else "display-brightness-symbolic", "--custom-progress", str(percent / 100)], check=False)


    def osd(self, args: list[str]) -> int | None:
        monitor = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activeworkspace"], capture=True).stdout).get("monitor")
        command = ["swayosd-client", *(["--monitor", monitor] if monitor else []), *args]
        return self.ctx.processes.run(command, check=False).returncode


    def wake(self) -> None:
        self.ctx.processes.run(["hyprctl", "dispatch", "dpms", "on"], check=False)
        with contextlib.suppress(ValueError, subprocess.CalledProcessError):
            self.brightness(["keyboard", "restore"])


    def battery(self, args: list[str]) -> None:
        batteries = list(Path("/sys/class/power_supply").glob("BAT*/capacity"))
        print(" · ".join(p.read_text().strip() + "%" for p in batteries) or "No battery")
        if "--notify" in args:
            low = [p for p in batteries if int(p.read_text()) <= 10 and (p.parent / "status").read_text().strip() == "Discharging"]
            marker = self.ctx.paths.runtime / "battery-notified"
            if low and not marker.exists():
                self.ctx.processes.run(["notify-send", "-u", "critical", "Time to recharge", f"Battery is down to {low[0].read_text().strip()}%"], check=False)
                storage.atomic_write(marker, "")
            elif not low:
                marker.unlink(missing_ok=True)


    def audio(self, args: list[str]) -> None:
        if args == ["mic-mute"]:
            self.ctx.processes.run(["wpctl", "set-mute", "@DEFAULT_SOURCE@", "toggle"])
        elif args == ["switch"]:
            sinks = self.ctx.processes.run(["pactl", "list", "short", "sinks"], capture=True).stdout.splitlines()
            current = self.ctx.processes.run(["pactl", "get-default-sink"], capture=True).stdout.strip()
            names = [line.split()[1] for line in sinks]
            if names:
                self.ctx.processes.run(["pactl", "set-default-sink", names[(names.index(current) + 1) % len(names)] if current in names else names[0]])
        else:
            raise ValueError("audio: mic-mute or switch")


    def power(self, args: list[str]) -> None:
        if args == ["logout"]:
            self.ctx.processes.run(["uwsm", "stop"])
        elif args == ["suspend"]:
            # Share the Rust backend's capability and lock-inhibitor checks.
            result = self.ctx.processes.run([self.ctx.executable("desktop-ring"), "action", '{"kind":"suspend"}'],
                         check=False, capture=True)
            if result.returncode:
                raise ValueError(result.stderr.strip() or "Sleep request failed")
        elif args[:1] in (["reboot"], ["poweroff"]):
            self.ctx.processes.run(["systemctl", args[0]])
        else:
            raise ValueError("power: suspend, reboot, poweroff, or logout")
