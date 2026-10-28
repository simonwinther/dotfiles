"""Sharing component with injected dependencies."""
from __future__ import annotations
import os
from pathlib import Path
import tempfile
from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .context import Context


@dataclass
class Sharing:
    ctx: Context

    def share(self, args: list[str]) -> None:
        if not args or args[0] not in ("clipboard", "file", "folder"):
            raise ValueError("share: clipboard, file [PATH], or folder [PATH]")
        kind = args[0]
        if kind == "clipboard":
            types = self.ctx.processes.run(["wl-paste", "--list-types"], capture=True).stdout.splitlines()
            mime = next((m for m in ("text/plain;charset=utf-8", "text/plain", "image/png", "image/jpeg") if m in types), None)
            if mime is None:
                raise ValueError("The clipboard has no shareable text or image; choose File instead")
            folder = self.ctx.paths.runtime / "shares"
            folder.mkdir(parents=True, exist_ok=True, mode=0o700)
            suffix = ".png" if mime == "image/png" else ".jpg" if mime == "image/jpeg" else ".txt"
            descriptor, filename = tempfile.mkstemp(prefix="clipboard-", suffix=suffix, dir=folder)
            path = Path(filename)
            try:
                with os.fdopen(descriptor, "wb") as stream:
                    self.ctx.processes.run(["wl-paste", "--type", mime], stdout=stream, text=False)
            except BaseException:
                path.unlink(missing_ok=True)
                raise
            # LocalSend can hand the request to its existing GUI; retain this private
            # file until logout rather than unlinking before the recipient is chosen.
        elif len(args) == 2:
            path = self.ctx.expand(args[1]).resolve()
        else:
            command = ["zenity", "--file-selection", "--title=Share " + kind]
            if kind == "folder":
                command.append("--directory")
            result = self.ctx.processes.run(command, capture=True, check=False)
            if result.returncode or not result.stdout.strip():
                return
            path = self.ctx.expand(result.stdout.strip()).resolve()
        if not (path.is_dir() if kind == "folder" else path.is_file()):
            raise ValueError("Choose an existing " + kind)
        self.ctx.processes.run(["localsend", "--headless", "send", str(path)])
