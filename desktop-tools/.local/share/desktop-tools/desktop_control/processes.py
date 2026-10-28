"""The process boundary; components never invoke subprocess or exec directly."""
from __future__ import annotations

from collections.abc import Mapping, Sequence
import os
from pathlib import Path
import shutil
import subprocess
from typing import Any, NoReturn, Protocol

import desktop_process

Argv = Sequence[str | Path]


class Processes(Protocol):
    def run(self, argv: Argv, *, check: bool = True, capture: bool = False, **kwargs: Any) -> subprocess.CompletedProcess[str]: ...
    def run_bytes(self, argv: Argv, **kwargs: Any) -> subprocess.CompletedProcess[bytes]: ...
    def spawn(self, argv: Argv, **kwargs: Any) -> subprocess.Popen[bytes]: ...
    def background(self, argv: Argv, **kwargs: Any) -> subprocess.Popen[bytes]: ...
    def managed(self, argv: Argv, **kwargs: Any) -> desktop_process.ManagedProcess: ...
    def which(self, name: str) -> str | None: ...
    def execv(self, executable: str, argv: Argv) -> NoReturn: ...
    def execvp(self, executable: str, argv: Argv) -> NoReturn: ...
    def kill(self, pid: int, sig: int) -> None: ...


class SystemProcesses:
    def __init__(self, env: Mapping[str, str], log_directory: Path) -> None:
        self.env = env
        self.log_directory = log_directory

    def run(self, argv: Argv, *, check: bool = True, capture: bool = False, **kwargs: Any) -> subprocess.CompletedProcess[str]:
        return desktop_process.run(argv, check=check, capture=capture,
                                   log_directory=self.log_directory, env=self.env, **kwargs)

    def run_bytes(self, argv: Argv, **kwargs: Any) -> subprocess.CompletedProcess[bytes]:
        capture = kwargs.pop("capture_output", False)
        return desktop_process.run(argv, text=False, capture=capture,
                                   log_directory=self.log_directory, env=self.env, **kwargs)

    def spawn(self, argv: Argv, **kwargs: Any) -> subprocess.Popen[bytes]:
        return subprocess.Popen([str(x) for x in argv], env=self.env, **kwargs)

    def background(self, argv: Argv, **kwargs: Any) -> subprocess.Popen[bytes]:
        return desktop_process.background(argv, log_directory=self.log_directory, env=self.env, **kwargs)

    def managed(self, argv: Argv, **kwargs: Any) -> desktop_process.ManagedProcess:
        return desktop_process.ManagedProcess(argv, log_directory=self.log_directory, env=self.env, **kwargs)

    def which(self, name: str) -> str | None:
        return shutil.which(name, path=self.env.get("PATH", os.defpath))

    def execv(self, executable: str, argv: Argv) -> NoReturn:
        os.execve(executable, [str(x) for x in argv], self.env)

    def execvp(self, executable: str, argv: Argv) -> NoReturn:
        os.execvpe(executable, [str(x) for x in argv], self.env)

    def kill(self, pid: int, sig: int) -> None:
        os.kill(pid, sig)
