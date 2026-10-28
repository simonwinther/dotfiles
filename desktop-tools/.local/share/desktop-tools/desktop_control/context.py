"""Explicit paths, environment, tools, time, and worker entry point."""
from __future__ import annotations

from collections.abc import Mapping, MutableMapping
from dataclasses import dataclass, field
import os
from pathlib import Path
import re
import sys
import tempfile
import time as system_time
from typing import Protocol

from .processes import Processes, SystemProcesses
from .storage import read_json


@dataclass(frozen=True)
class Paths:
    home: Path
    config: Path
    data: Path
    state: Path
    runtime: Path
    proc: Path = Path("/proc")
    sys_class: Path = Path("/sys/class")

    @classmethod
    def from_environment(cls, env: Mapping[str, str], *, uid: int) -> Paths:
        home = Path(env.get("HOME") or Path.home())
        return cls(
            home=home,
            config=Path(env.get("XDG_CONFIG_HOME", home / ".config")),
            data=Path(env.get("XDG_DATA_HOME", home / ".local/share")),
            state=Path(env.get("XDG_STATE_HOME", home / ".local/state")) / "desktop",
            runtime=Path(env.get("XDG_RUNTIME_DIR", Path(tempfile.gettempdir()) / f"desktop-{uid}")) / "desktop",
        )


class Clock(Protocol):
    def time(self) -> float: ...
    def monotonic(self) -> float: ...
    def sleep(self, seconds: float) -> None: ...
    def strftime(self, format: str, t: system_time.struct_time | None = None) -> str: ...
    def localtime(self, seconds: float) -> system_time.struct_time: ...


class SystemClock:
    time = staticmethod(system_time.time)
    monotonic = staticmethod(system_time.monotonic)
    sleep = staticmethod(system_time.sleep)
    localtime = staticmethod(system_time.localtime)

    @staticmethod
    def strftime(format: str, t: system_time.struct_time | None = None) -> str:
        return system_time.strftime(format, t) if t is not None else system_time.strftime(format)


@dataclass
class Context:
    paths: Paths
    env: MutableMapping[str, str]
    processes: Processes
    clock: Clock
    entrypoint: Path
    uid: int
    python: str = field(default_factory=lambda: sys.executable)

    @classmethod
    def from_environment(cls, entrypoint: Path) -> Context:
        env = dict(os.environ)
        uid = os.getuid()
        paths = Paths.from_environment(env, uid=uid)
        return cls(paths, env, SystemProcesses(env, paths.state / "processes"), SystemClock(), entrypoint, uid)

    def expand(self, value: str | Path) -> Path:
        text = str(value)
        if text == "~" or text.startswith("~/"):
            text = str(self.paths.home) + text[1:]
        else:
            text = os.path.expanduser(text)
        text = re.sub(r"\$(\w+|\{[^}]*\})", lambda m: self.env.get(m[1].strip("{}"), m[0]), text)
        return Path(text)

    def executable(self, name: str) -> str:
        variable = "DESKTOP_" + re.sub(r"[^A-Z0-9]", "_", name.upper()) + "_BIN"
        selected = self.env.get(variable) or read_json(self.paths.config / "desktop/paths.json").get(name)
        if selected:
            if not Path(selected).exists():
                raise ValueError(f"Declared {name} path is missing: {selected}")
            return selected
        for directory in self.env.get("PATH", os.defpath).split(os.pathsep):
            candidate = Path(directory) / name
            if candidate.is_file() and os.access(candidate, os.X_OK):
                with candidate.open("rb") as stream:
                    first = stream.read(512)
                if b"desktopctl run " not in first:
                    return str(candidate)
        raise ValueError(f"{name} is unavailable; install it or declare desktop/paths.json")

    def notify(self, title: str, body: str = "") -> None:
        if self.processes.which("notify-send"):
            self.processes.run(["notify-send", title, body], check=False)

    def worker(self, name: str, args: list[str]) -> list[str]:
        return [self.python, str(self.entrypoint), name, *args]
