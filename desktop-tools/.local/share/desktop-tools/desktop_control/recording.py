"""Recording component with injected dependencies."""
from __future__ import annotations
import contextlib
import hashlib
import json
import os
import re
import select
import signal
import subprocess
import tempfile
import uuid
from dataclasses import dataclass
from typing import Any, Iterator, TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore
    from .capture import Screenshots


RECORDING_STOP_TIMEOUT = 30



@dataclass
class Recording:
    ctx: Context
    store: StateStore
    capture: Screenshots
    stop_timeout: float = RECORDING_STOP_TIMEOUT

    def recording_identity(self, pid: int) -> dict[str, Any]:
        """Identify one Linux process incarnation, including its actual executable."""
        proc = (self.ctx.paths.proc / f'{pid}')
        # comm may contain spaces and closing parentheses; starttime is field 22.
        fields = (proc / "stat").read_text().rsplit(") ", 1)[1].split()
        binary = (proc / "exe").stat()
        return {"boot_id": self.ctx.paths.proc / 'sys/kernel/random/boot_id'.read_text().strip(),
                "start_time": int(fields[19]), "uid": proc.stat().st_uid,
                "executable": [binary.st_dev, binary.st_ino],
                "command_sha256": hashlib.sha256((proc / "cmdline").read_bytes()).hexdigest()}


    @contextlib.contextmanager
    def recording_process(self, current: dict[str, Any]) -> Iterator[int | None]:
        """Pin the PID before inspecting /proc; never signal a recycled numeric PID."""
        pid = current.get("pid")
        if type(pid) is not int or pid <= 1:
            raise ValueError("Invalid recorder PID; recording state retained for inspection")
        try:
            descriptor = os.pidfd_open(pid)
        except ProcessLookupError:
            yield None
            return
        try:
            poller = select.poll()
            poller.register(descriptor, select.POLLIN)
            if poller.poll(0):
                yield None
                return
            try:
                identity = self.recording_identity(pid)
            except (FileNotFoundError, ProcessLookupError):
                if not poller.poll(0):
                    raise
                identity = None
            # If /proc changed while reading it, the pinned process has exited.
            if poller.poll(0):
                yield None
                return
            if identity != current.get("identity") or identity["uid"] != self.ctx.uid:
                raise ValueError("Refusing to signal an unverified recorder; recording state retained for inspection")
            yield descriptor
        finally:
            os.close(descriptor)


    def finish_recording(self, current: dict[str, Any], status: str, error: str | None=None) -> None:
        result = {**current, "status": status, "finished_at": self.ctx.clock.time()}
        if error:
            result["error"] = error
        # Archive before clearing the active marker, including for stale/startup failures.
        storage.atomic_write(self.ctx.paths.state / "recordings" / f"{uuid.uuid4().hex}.json", json.dumps(result, indent=2) + "\n")
        (self.ctx.paths.state / "recording.json").unlink(missing_ok=True)


    def stop_recording(self, current: dict[str, Any]) -> None:
        log = current.get("log", "unavailable (legacy recording state)")
        with self.recording_process(current) as descriptor:
            if descriptor is None:
                if current.get("status") == "stopping":
                    self.finish_recording(current, "stopped")
                    self.ctx.notify("Recording stopped", current.get("output", ""))
                    return
                self.finish_recording(current, "failed", "Recorder exited before stop was requested or completed")
                raise ValueError(f"Recorder is no longer running; session details retained under {self.ctx.paths.state / 'recordings'}; log: {log}")
            if current.get("status") != "stopping":
                current.update(status="stopping", stop_requested_at=self.ctx.clock.time())
                storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
                try:
                    signal.pidfd_send_signal(descriptor, signal.SIGINT)
                except ProcessLookupError:
                    pass
                except OSError as error:
                    current.update(status="recording", error=f"Could not interrupt recorder: {error}")
                    storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
                    raise
            poller = select.poll()
            poller.register(descriptor, select.POLLIN)
            if not poller.poll(int(self.stop_timeout * 1000)):
                current["error"] = "Timed out waiting for recorder finalization"
                storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
                raise ValueError(f"Recorder is still finalizing; recording state retained; retry to wait again; log: {log}")
            self.finish_recording(current, "stopped")
        self.ctx.notify("Recording stopped", current.get("output", ""))


    def start_recording(self, args: list[str]) -> None:
        directory = self.ctx.paths.home / self.store.settings().get("recordings", "Videos")
        directory.mkdir(parents=True, exist_ok=True)
        # Rapid stop/start cycles must not reuse an output from the same second.
        output = directory / f"{self.ctx.clock.strftime('%Y-%m-%d_%H-%M-%S')}-{uuid.uuid4().hex}.mp4"
        mode = args[1] if len(args) > 1 else "screen"
        command = [self.ctx.executable("gpu-screen-recorder"), "-w", "portal" if mode == "screen" else mode,
                   "-f", "60", "-fallback-cpu-encoding", "yes"]
        if mode == "region":
            geometry = self.capture.screenshot_geometry("smart")
            if not geometry:
                return
            rectangle = re.fullmatch(r"(-?\d+),(-?\d+) (\d+)x(\d+)", geometry)
            if not rectangle:
                raise ValueError("Invalid recording region")
            command += ["-region", f"{rectangle[3]}x{rectangle[4]}+{rectangle[1]}+{rectangle[2]}"]
        if "--silent" not in args:
            command += ["-a", "default_output"]
        if "--microphone" in args:
            command += ["-a", "default_input"]
        command += ["-o", str(output)]
        logs = self.ctx.paths.state / "recordings"
        logs.mkdir(parents=True, exist_ok=True, mode=0o700)
        descriptor, name = tempfile.mkstemp(prefix="recorder-", suffix=".log", dir=logs)
        current = {"output": str(output), "log": name, "command": command,
                   "status": "recording", "started_at": self.ctx.clock.time()}
        child = None
        try:
            with os.fdopen(descriptor, "wb") as stream:
                child = self.ctx.processes.spawn(command, stdin=subprocess.DEVNULL, stdout=stream,
                                         stderr=subprocess.STDOUT, start_new_session=True)
            current["pid"] = child.pid
            current["identity"] = self.recording_identity(child.pid)
            storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
            self.ctx.clock.sleep(.25)
            if child.poll() is not None:
                raise ValueError(f"Screen recorder exited during startup (status {child.returncode})")
            # A launcher may exec its native binary during the startup allowance.
            current["identity"] = self.recording_identity(child.pid)
            storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
        except (OSError, ValueError) as error:
            if child is not None and child.poll() is None:
                child.send_signal(signal.SIGINT)
                try:
                    child.wait(timeout=self.stop_timeout)
                except subprocess.TimeoutExpired:
                    current.update(status="stopping", error=str(error))
                    storage.atomic_write(self.ctx.paths.state / "recording.json", json.dumps(current) + "\n")
                    raise ValueError(f"Recorder startup failed and PID {child.pid} is still finalizing; state retained; log: {name}") from error
            if child is not None:
                current["returncode"] = child.returncode
            self.finish_recording(current, "failed", str(error))
            raise ValueError(f"Screen recorder failed to start: {error}; log: {name}") from error
        self.ctx.notify("Recording started", str(output))


    def recording(self, args: list[str]) -> int | None:
        if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
            raise ValueError("Recording control requires Linux pidfd support and Python 3.9 or newer")
        with self.store.state_lock("recording"):
            pidfile = self.ctx.paths.state / "recording.json"
            if pidfile.exists():
                current = storage.read_json(pidfile)
                if not isinstance(current, dict) or not current:
                    raise ValueError("Invalid recording state retained for inspection")
                return self.stop_recording(current)
            # Fail before spawning if the running kernel does not support pidfds.
            descriptor = os.pidfd_open(os.getpid())
            os.close(descriptor)
            return self.start_recording(args)
