"""Screenshots component with injected dependencies."""
from __future__ import annotations
import contextlib
import json
import os
from pathlib import Path
import re
import shlex
import tempfile
import uuid
import desktop_process as processes
from dataclasses import dataclass
from typing import Iterator, TYPE_CHECKING

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .clipboard import Clipboard


@dataclass
class Screenshots:
    ctx: Context
    store: StateStore
    clipboard: Clipboard

    def screenshot_geometry(self, mode: str) -> str | None:
        if mode == "region":
            selected = self.ctx.processes.run(["slurp"], check=False, capture=True)
            return selected.stdout.strip() if not selected.returncode else None
        monitors = json.loads(self.ctx.processes.run(["hyprctl", "-j", "monitors"], capture=True).stdout)
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
        clients = json.loads(self.ctx.processes.run(["hyprctl", "-j", "clients"], capture=True).stdout)
        rectangles = [f'{w["at"][0]},{w["at"][1]} {w["size"][0]}x{w["size"][1]}' for w in clients
                      if w.get("workspace", {}).get("id") == focused.get("activeWorkspace", {}).get("id")
                      and not w.get("hidden")]
        rectangles.append(rect(focused))
        selected = self.ctx.processes.run(["slurp", *(["-r"] if mode == "windows" else [])], input="\n".join(rectangles), check=False, capture=True)
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
    def frozen_display(self, enabled: bool) -> Iterator[None]:
        if not enabled:
            yield
            return
        freezer = self.ctx.processes.background(["hyprpicker", "-r", "-z"], operation="selection")
        try:
            # Match the reference capture's render allowance before area selection.
            self.ctx.clock.sleep(.1)
            yield
        finally:
            with processes.cancellation_ignored():
                freezer.close()


    def edit_screenshot(self, path: Path, editor: str='satty') -> None:
        if editor == "satty":
            self.ctx.processes.run(["satty", "--filename", path, "--output-filename", path, "--actions-on-enter", "save-to-clipboard", "--save-after-copy", "--copy-command", "wl-copy"])
        else:
            self.ctx.processes.run([*shlex.split(editor), path], operation="interactive")


    def save_screenshot(self, directory: Path, image: bytes) -> Path:
        directory.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(prefix=".screenshot-", suffix=".tmp", dir=directory)
        try:
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(image)
                stream.flush()
                os.fsync(stream.fileno())
            stem = self.ctx.clock.strftime("screenshot-%Y-%m-%d_%H-%M-%S")
            while True:
                output = directory / f"{stem}-{uuid.uuid4().hex}.png"
                try:
                    # Publish complete bytes atomically, refusing to replace any
                    # existing entry (including dangling symlinks) on collision.
                    os.link(temporary, output)
                except FileExistsError:
                    continue
                return output
        finally:
            with contextlib.suppress(FileNotFoundError):
                os.unlink(temporary)


    def capture(self, args: list[str]) -> int | None:
        action = args[0] if args else "screenshot"
        if action == "record":
            return self.recording.recording(args)
        if action == "edit-latest":
            directory = self.ctx.paths.home / self.store.settings().get("screenshots", "Pictures")
            files = sorted([*directory.glob("screenshot-*.png"), *directory.glob("Screenshot_*.png")], key=lambda path: path.stat().st_mtime_ns)
            if not files:
                raise ValueError("No saved screenshot is available")
            self.edit_screenshot(files[-1])
            return
        if action == "notify" and len(args) >= 2:
            output = self.ctx.expand(args[1])
            response = self.ctx.processes.run(["notify-send", "Screenshot saved to clipboard and file", "Edit with Super + Alt + , (or click this)", "-t", "10000", "-i", output, "-A", "default=edit"], capture=True, check=False, operation="notification")
            if response.stdout.strip() == "default" and output.is_file():
                self.edit_screenshot(output, args[2] if len(args) > 2 else "satty")
            return
        editor = next((value.split("=", 1)[1] for value in args if value.startswith("--editor=")), "satty")
        positional = [value for value in args[1:] if not value.startswith("--")]
        mode = positional[0] if positional else ("region" if action == "ocr" else "smart")
        if mode not in ("smart", "region", "windows", "fullscreen"):
            raise ValueError("Screenshot mode: smart, region, windows, or fullscreen")
        processing = positional[1] if len(positional) > 1 else "slurp"
        if processing not in ("slurp", "copy", "save"):
            raise ValueError("Screenshot processing: slurp, copy, or save")
        with self.frozen_display(mode != "fullscreen"):
            geometry = self.screenshot_geometry(mode)
            if not geometry:
                return
            image = self.ctx.processes.run(["grim", "-g", geometry, "-"], capture=True, text=False).stdout
        if action == "ocr":
            result = self.ctx.processes.run(["tesseract", "stdin", "stdout", "--oem", "1", "--psm", "6", "-l", "eng", "--dpi", "300"], input=image, capture=True, text=False)
            self.clipboard.copy_text(result.stdout.decode()); self.ctx.notify("Text copied from selection")
        else:
            if processing == "copy":
                self.clipboard.copy_data(image, "image/png")
                return
            directory = self.ctx.paths.home / self.store.settings().get("screenshots", "Pictures")
            output = self.save_screenshot(directory, image)
            if "--edit" in args:
                self.edit_screenshot(output, editor)
            elif processing == "slurp":
                self.clipboard.copy_data(image, "image/png")
                self.ctx.processes.background(["desktopctl", "capture", "notify", output, editor], operation="interactive")
            if processing == "save":
                print(output)
