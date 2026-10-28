"""Screensavers component with injected dependencies."""
from __future__ import annotations
import json
import select
import shlex
import subprocess
import sys
import desktop_process as processes
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .applications import Applications


@dataclass
class Screensavers:
    ctx: Context
    store: StateStore
    applications: Applications

    def image_art(self, args: list[str], *, width: int=80, height: int=26) -> str | None:
        if args:
            image = self.ctx.expand(args[0]).resolve()
        else:
            result = self.ctx.processes.run(["zenity", "--file-selection", "--title=Choose artwork image",
                          "--file-filter=Images | *.svg *.png *.jpg *.jpeg *.webp"], capture=True, check=False)
            if result.returncode or not result.stdout.strip():
                return None
            image = self.ctx.expand(result.stdout.strip()).resolve()
        if not image.is_file() or image.suffix.lower() not in (".svg", ".png", ".jpg", ".jpeg", ".webp"):
            raise ValueError("Choose an existing SVG, PNG, JPEG or WebP image")
        # Detect actual source transparency before padding, so opaque photos use
        # luminance while transparent logos retain their silhouette, of either color.
        limits = self.ctx.processes.run(["magick", "-background", "none", str(image), "-alpha", "extract",
                      "-format", "%[fx:minima] %[fx:maxima]", "info:"], capture=True).stdout.split()
        transparent = len(limits) == 2 and float(limits[0]) < .999 and float(limits[1]) > float(limits[0])
        command = ["magick", "-background", "none", str(image), "-auto-orient"]
        command += ["-alpha", "extract", "-alpha", "off"] if transparent else ["-background", "white", "-alpha", "remove", "-alpha", "off", "-colorspace", "Gray", "-negate"]
        command += ["-bordercolor", "black", "-border", "1", "-trim", "+repage", "-resize", f"{width * 2}x{height * 4}",
                    "-gravity", "center", "-background", "black", "-extent", f"{width * 2}x{height * 4}", "-depth", "8", "gray:-"]
        pixels = self.ctx.processes.run(command, capture=True, text=False).stdout
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


    def about(self, args: list[str]) -> int | None:
        action = args[0] if args else "show"
        target = self.ctx.paths.state / "about.txt"
        if action in ("edit", "image", "reset"):
            if action == "reset":
                with self.store.state_lock():
                    value = self.store.state(); value.pop("aboutArt", None); self.store.save_state(value)
                self.ctx.notify("About artwork", "Restored the NixOS logo")
                return
            if action == "image":
                art = self.image_art(args[1:], width=54)
                if art is None:
                    return
                storage.atomic_write(target, art)
            else:
                if target.is_symlink():
                    raise ValueError("About artwork must be a writable state file, not a link")
                if not target.exists():
                    storage.atomic_write(target, "NixOS\n")
            with self.store.state_lock():
                value = self.store.state(); value["aboutArt"] = True; self.store.save_state(value)
            if action == "edit":
                self.applications.launch(["editor", str(target)])
            else:
                self.ctx.notify("About artwork", "Image converted; open About to preview")
        elif action == "show":
            command = "fastfetch"
            if self.store.state().get("aboutArt") and target.is_file():
                command += " --logo-type file-raw --logo " + shlex.quote(str(target))
            self.applications.terminal(["--shell", command + "; read -r -n 1 -s"])
        else:
            raise ValueError("about: show, edit, image [PATH], or reset")


    def screensaver(self, args: list[str]) -> int | None:
        action = args[0] if args else "start"
        name = self.store.state().get("screensaver", "naruto")
        if action == "current":
            print(name)
        elif action == "set" and len(args) == 2:
            if args[1] not in ("naruto", "tomoe-sharingan", "matrix", "custom"):
                raise ValueError("Unknown screensaver")
            if args[1] == "custom" and not (self.ctx.paths.state / "screensaver.txt").is_file():
                raise ValueError("Use screensaver edit to create your artwork first")
            with self.store.state_lock():
                value = self.store.state(); value["screensaver"] = args[1]; self.store.save_state(value)
        elif action == "edit":
            target = self.ctx.paths.state / "screensaver.txt"
            if target.is_symlink():
                raise ValueError("Custom screensaver artwork must be a writable state file, not a link")
            if not target.exists():
                storage.atomic_write(target, (self.ctx.paths.data / "screensavers/naruto.txt").read_text())
            self.screensaver(["set", "custom"])
            self.applications.launch(["editor", str(target)])
        elif action == "image":
            art = self.image_art(args[1:])
            if art is not None:
                storage.atomic_write(self.ctx.paths.state / "screensaver.txt", art)
                self.screensaver(["set", "custom"])
                self.ctx.notify("Screensaver", "Image converted; choose Start to preview")
        elif action == "reset":
            self.screensaver(["set", "naruto"])
            self.ctx.notify("Screensaver", "Restored the Naruto artwork")
        elif action in ("start", "idle"):
            if action == "idle" and (self.ctx.paths.state / "toggles/screensaver-off").exists():
                return
            if self.ctx.processes.run(["pgrep", "-u", str(self.ctx.uid), "-x", "hyprlock"], check=False, capture=True).returncode == 0:
                return
            clients = json.loads(self.ctx.processes.run(["hyprctl", "-j", "clients"], capture=True).stdout)
            if any(window.get("class") == "org.desktop.screensaver" for window in clients):
                return
            command = [self.ctx.python, str(self.ctx.entrypoint), "screensaver-worker"]
            self.ctx.processes.execvp("ghostty", ["ghostty", "--class=org.desktop.screensaver", "--fullscreen=true", "--background-opacity=1",
                                 "--font-size=18", "--window-padding-x=0", "--window-padding-y=0", "-e", *command])
        else:
            raise ValueError("screensaver: start, current, set NAME, edit, image [PATH], or reset")


    def screensaver_worker(self, args: list[str]) -> int | None:
        # Own only this child: leaving the saver never kills unrelated TTE processes.
        import select
        import termios
        import tty
        name = self.store.state().get("screensaver", "naruto")
        path = self.ctx.paths.state / "screensaver.txt" if name == "custom" else self.ctx.paths.data / "screensavers" / (name + ".txt")
        if name != "matrix" and not path.is_file():
            raise ValueError(f"Missing screensaver artwork: {path}")
        previous = termios.tcgetattr(sys.stdin)
        child = None
        try:
            tty.setcbreak(sys.stdin.fileno())
            while True:
                command = ["cmatrix", "-abs", "-u", "4"] if name == "matrix" else ["tte", "-i", str(path), "--frame-rate", "30", "--canvas-width", "0", "--canvas-height", "0",
                           "--anchor-canvas", "c", "--anchor-text", "c", "--random-effect", "--exclude-effects", "dev_worm", "--no-eol"]
                if child is not None:
                    child.close()
                child = self.ctx.processes.managed(command, operation="screensaver", stdin=None if name == "matrix" else subprocess.DEVNULL)
                while child.poll() is None:
                    if select.select([sys.stdin], [], [], .2)[0]:
                        return
                    try:
                        active = json.loads(self.ctx.processes.run(["hyprctl", "-j", "activewindow"], capture=True, check=False).stdout)
                        if active.get("class") != "org.desktop.screensaver":
                            return
                    except (OSError, ValueError):
                        return
                if child.returncode:
                    raise ValueError("Screensaver animation exited unsuccessfully")
                if name == "matrix":
                    return
        finally:
            if child is not None:
                with processes.cancellation_ignored():
                    child.close()
            termios.tcsetattr(sys.stdin, termios.TCSADRAIN, previous)
            print("\033[?25h", end="", flush=True)
