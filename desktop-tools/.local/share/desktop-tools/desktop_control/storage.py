"""StateStore component with injected dependencies."""
from __future__ import annotations
import contextlib
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
from dataclasses import dataclass, field
from typing import Any, Iterator, TYPE_CHECKING
from .state_schema import STATE_VERSION, UnsupportedStateVersion, check_json_depth, decode_json, normalize_state

if TYPE_CHECKING:
    from .context import Paths


def read_json(path: Path, default=None):
    try:
        raw = path.read_bytes()
    except FileNotFoundError:
        return {} if default is None else default
    value = decode_json(raw, path)
    if not isinstance(value, dict):
        raise ValueError(f"{path}: expected a JSON object")
    return value


def atomic_write(path: Path, content: str | bytes, mode=0o600):
    raw = content.encode() if isinstance(content, str) else content
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.is_file() and not path.is_symlink() and path.read_bytes() == raw:
        return
    if path.is_symlink():
        raise ValueError(f"Refusing to overwrite a link: {path}")
    descriptor, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(name, mode)
        os.replace(name, path)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(name)


def atomic_link(path: Path, target: Path):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.exists() and not path.is_symlink():
        raise ValueError(f"Refusing to replace a file with a link: {path}")
    with tempfile.TemporaryDirectory(prefix=f".{path.name}.", dir=path.parent) as directory:
        link = Path(directory) / "link"
        link.symlink_to(target)
        os.replace(link, path)



@dataclass
class StateStore:
    paths: Paths
    _held_state_locks: Any = field(default_factory=threading.local, init=False, repr=False)

    @contextlib.contextmanager
    def state_lock(self, name: str='state', *, wait: bool=True) -> Iterator[bool]:
        # Reads can migrate/recover state, including while a command already holds
        # this lock. Reuse locks in the same thread; other threads/processes flock.
        path = self.paths.state / f"{name}.lock"
        held = getattr(self._held_state_locks, "paths", None)
        if held is None:
            held = self._held_state_locks.paths = set()
        if wait and path in held:
            yield True
            return
        self.paths.state.mkdir(parents=True, exist_ok=True, mode=0o700)
        with path.open("a") as stream:
            try:
                fcntl.flock(stream, fcntl.LOCK_EX | (0 if wait else fcntl.LOCK_NB))
            except BlockingIOError:
                yield False
                return
            held.add(path)
            try:
                yield True
            finally:
                held.remove(path)


    def settings(self) -> dict[str, Any]:
        return read_json(self.paths.config / "desktop/settings.json")


    def write_state(self, value: dict[str, Any]) -> None:
        atomic_write(self.paths.state / "state.json", json.dumps(value, indent=2, allow_nan=False) + "\n")


    def load_state(self) -> dict[str, Any]:
        """Load under state_lock; preserve invalid bytes before replacing state."""
        path = self.paths.state / "state.json"
        if path.is_symlink():
            raise ValueError(f"Refusing to read or replace linked desktop state: {path}")
        try:
            raw = path.read_bytes()
        except FileNotFoundError:
            return {"schemaVersion": STATE_VERSION}
        try:
            original = decode_json(raw, path)
        except ValueError as error:
            value, issues = {"schemaVersion": STATE_VERSION}, [str(error)]
        else:
            try:
                value, issues = normalize_state(original)
            except UnsupportedStateVersion as error:
                raise UnsupportedStateVersion(f"{path}: {error}") from None
            if not issues:
                if value != original:
                    self.write_state(value)
                return value
        backup = None
        try:
            descriptor, name = tempfile.mkstemp(prefix="state.json.invalid-", suffix=".json", dir=self.paths.state)
            backup = Path(name)
            with os.fdopen(descriptor, "wb") as stream:
                stream.write(raw)
                stream.flush()
                os.fsync(stream.fileno())
            self.write_state(value)
        except OSError as error:
            retained = f"; recovery copy: {backup}" if backup else ""
            raise OSError(f"{path}: state recovery failed: {error}{retained}") from error
        print(f"desktopctl: {path}: recovered invalid state ({'; '.join(dict.fromkeys(issues))}); "
              f"original saved to {backup}; valid choices retained, invalid choices use defaults", file=sys.stderr)
        return value


    def state(self) -> dict[str, Any]:
        with self.state_lock():
            return self.load_state()


    def save_state(self, value: dict[str, Any]) -> None:
        value, issues = normalize_state(value)
        if issues:
            raise ValueError("Refusing to save invalid desktop state: " + "; ".join(dict.fromkeys(issues)))
        # Validate serialization before touching existing state, including unknown
        # extension fields passed by callers rather than read from JSON.
        check_json_depth(value)
        content = json.dumps(value, indent=2, allow_nan=False) + "\n"
        with self.state_lock():
            self.load_state()  # Recover corruption and refuse newer schemas before writing.
            atomic_write(self.paths.state / "state.json", content)
