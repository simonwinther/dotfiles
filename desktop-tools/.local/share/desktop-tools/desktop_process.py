"""Owned Linux process groups, operation deadlines, and private bounded diagnostics.

Detached jobs use this file as a supervisor so stderr is still drained and bounded
after desktopctl exits. No command arguments, stdin, or captured payloads are logged.
"""
from __future__ import annotations

import contextlib
import fcntl
import json
import math
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import threading
import time

TIMEOUTS = {
    "control": 5, "service": 45, "capture": 15, "clipboard": 10,
    "image": 60, "ocr": 120, "selection": 900, "notification": 15,
    "transfer": 1800, "media": 21600,
    # Their lifetime belongs to the user or desktop session, never a timer.
    "interactive": None, "daemon": None, "recording": None,
    "clipboard-owner": None, "lock": None, "screensaver": None,
}
PROGRAM_OPERATIONS = {
    "systemctl": "service", "systemd-run": "service", "uwsm": "service",
    "grim": "capture", "wl-paste": "clipboard", "wl-copy": "clipboard",
    "matugen": "image", "magick": "image", "tesseract": "ocr",
    "zenity": "selection", "slurp": "selection", "walker": "selection",
    "satty": "interactive", "nautilus": "interactive",
    "localsend": "transfer", "desktop-transcode": "media", "awww": "image",
}
DEFAULT = object()
LOG_BYTES = 64 * 1024
LOG_FILES = 3
TAIL_BYTES = 8 * 1024
POLL_INTERVAL = .05
STOP_GRACE = 1


class Cancelled(BaseException):
    def __init__(self, signum=signal.SIGINT):
        self.signum = signum
        super().__init__(f"cancelled by {signal.Signals(signum).name}")


@contextlib.contextmanager
def cancellation_signals():
    def cancel(signum, _frame):
        raise Cancelled(signum)
    previous = {sig: signal.signal(sig, cancel)
                for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    try:
        yield
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def policy(argv, operation=None, timeout=DEFAULT):
    if not argv:
        raise ValueError("A process command is required")
    operation = operation or PROGRAM_OPERATIONS.get(Path(str(argv[0])).name, "control")
    if operation not in TIMEOUTS:
        raise ValueError(f"Unknown process operation: {operation}")
    seconds = TIMEOUTS[operation] if timeout is DEFAULT else timeout
    if seconds is None and TIMEOUTS[operation] is not None:
        raise ValueError(f"{operation} requires a finite deadline")
    if seconds is not None and (not math.isfinite(seconds) or seconds <= 0):
        raise ValueError("Process deadlines must be positive and finite")
    return operation, seconds


class DiagnosticLog:
    def __init__(self, directory, argv, operation):
        self.directory = Path(directory)
        self.path = self.directory / "processes.jsonl"
        self.program = Path(argv[0]).name
        self.operation = operation

    def record(self, event, **fields):
        # Logging failures must not prevent locking or other desktop actions.
        try:
            self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
            if self.directory.is_symlink():
                return
            record = {"time": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
                      "program": self.program, "operation": self.operation,
                      "owner": os.getpid(), "event": event, **fields}
            raw = (json.dumps(record, ensure_ascii=True) + "\n").encode()
            # stderr is passed in chunks of at most 4096 bytes, including escapes.
            if len(raw) > LOG_BYTES:
                return
            flags = os.O_WRONLY | os.O_CREAT | os.O_CLOEXEC | os.O_NOFOLLOW
            with os.fdopen(os.open(self.directory / "lock", flags, 0o600), "wb") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                with os.fdopen(os.open(self.path, flags | os.O_APPEND, 0o600), "ab") as stream:
                    os.fchmod(stream.fileno(), 0o600)
                    if os.fstat(stream.fileno()).st_size + len(raw) > LOG_BYTES:
                        for index in range(LOG_FILES - 1, 0, -1):
                            source = self.path if index == 1 else self.path.with_suffix(f".jsonl.{index - 1}")
                            with contextlib.suppress(FileNotFoundError):
                                os.replace(source, self.path.with_suffix(f".jsonl.{index}"))
                        # The old descriptor now refers to .1; open the new log.
                        with os.fdopen(os.open(self.path, flags | os.O_APPEND, 0o600), "ab") as fresh:
                            fresh.write(raw)
                    else:
                        stream.write(raw)
        except OSError:
            pass


class ProcessError(subprocess.CalledProcessError):
    def __init__(self, child, stdout=None):
        super().__init__(child.returncode, child.args, output=stdout, stderr=child.stderr)
        self.log = child.log

    def __str__(self):
        return (f"{self.log.program} ({self.log.operation}) returned non-zero exit status "
                f"{self.returncode}; diagnostics: {self.log.path}")


class ProcessTimeout(subprocess.TimeoutExpired):
    def __init__(self, child, timeout, stdout=None):
        super().__init__(child.args, timeout, output=stdout, stderr=child.stderr)
        self.log = child.log

    def __str__(self):
        return (f"{self.log.program} ({self.log.operation}) exceeded {self.timeout:g}s; "
                f"diagnostics: {self.log.path}")


class ManagedProcess:
    """A child and its descendants; exiting the context always reaps the child."""
    def __init__(self, argv, *, log_directory, operation=None, timeout=DEFAULT,
                 text=True, capture=False, echo_stderr=True, diagnostic_stdout=False, **kwargs):
        self.args = [str(arg) for arg in argv]
        self.operation, self.timeout = policy(self.args, operation, timeout)
        self.deadline = None if self.timeout is None else time.monotonic() + self.timeout
        self.log = DiagnosticLog(log_directory, self.args, self.operation)
        self.text = text
        self._tail = b""
        self._stop_reader = threading.Event()
        self._closed = False
        read_fd, write_fd = os.pipe()
        try:
            if capture:
                kwargs["stdout"] = subprocess.PIPE
            if diagnostic_stdout:
                kwargs["stdout"] = write_fd
            self.process = subprocess.Popen(self.args, start_new_session=True, text=text,
                                            stderr=write_fd, **kwargs)
        except BaseException:
            os.close(read_fd)
            self.log.record("launch-failed")
            raise
        finally:
            os.close(write_fd)
        self.pid = self.process.pid
        self._reader = threading.Thread(target=self._drain, args=(read_fd, echo_stderr and not capture),
                                        name="desktop-stderr", daemon=True)
        self._reader.start()

    def _drain(self, fd, echo):
        try:
            while not self._stop_reader.is_set():
                if not select.select([fd], [], [], POLL_INTERVAL)[0]:
                    continue
                data = os.read(fd, 4096)
                if not data:
                    return
                self._tail = (self._tail + data)[-TAIL_BYTES:]
                self.log.record("stderr", pid=self.pid, text=data.decode(errors="replace"))
                if echo:
                    with contextlib.suppress(OSError, ValueError):
                        sys.stderr.write(data.decode(errors="replace"))
                        sys.stderr.flush()
        finally:
            os.close(fd)

    @property
    def stderr(self):
        return self._tail.decode(errors="replace") if self.text else self._tail

    @property
    def returncode(self):
        return self.process.returncode

    def poll(self):
        return self.process.poll()

    def remaining(self):
        if self.deadline is None:
            return None
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise ProcessTimeout(self, self.timeout)
        return remaining

    def wait(self, timeout=None):
        remaining = self.remaining()
        duration = remaining if timeout is None else timeout if remaining is None else min(timeout, remaining)
        return self.process.wait(timeout=duration)

    def send_signal(self, signum):
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.pid, signum)

    def terminate(self):
        self.send_signal(signal.SIGTERM)

    def kill(self):
        self.send_signal(signal.SIGKILL)

    def close(self, grace=STOP_GRACE):
        if self._closed:
            return
        self._closed = True
        self.terminate()
        try:
            self.process.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            pass
        finally:
            # Also remove descendants left behind after the direct child exited.
            self.kill()
            self.process.wait(timeout=STOP_GRACE)
            self._reader.join(timeout=STOP_GRACE)
            self._stop_reader.set()
            self._reader.join(timeout=STOP_GRACE)
            for stream in (self.process.stdin, self.process.stdout):
                if stream is not None:
                    stream.close()

    def __enter__(self):
        return self

    def __exit__(self, kind, error, _traceback):
        # Finish cleanup even if a second cancellation signal arrives.
        with cancellation_ignored():
            self.close()
        if error is not None:
            event = "cancelled" if isinstance(error, (Cancelled, KeyboardInterrupt)) else "timeout" if isinstance(error, subprocess.TimeoutExpired) else "failed"
            self.log.record(event,
                            pid=self.pid)
        elif self.returncode:
            self.log.record("exited", pid=self.pid, status=self.returncode)


@contextlib.contextmanager
def cancellation_ignored():
    # Only the main thread may install signal handlers. Worker-thread callers
    # can still use cancellation tokens and always get the same group cleanup.
    previous = {}
    if threading.current_thread() is threading.main_thread():
        previous = {sig: signal.signal(sig, signal.SIG_IGN)
                    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    try:
        yield
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def run(argv, *, log_directory, check=True, capture=False, input=None, text=True,
        operation=None, timeout=DEFAULT, cancel=None, **kwargs):
    if input is not None:
        kwargs["stdin"] = subprocess.PIPE
    child = ManagedProcess(argv, log_directory=log_directory, operation=operation,
                           timeout=timeout, text=text, capture=capture, **kwargs)
    output = None
    try:
        with child:
            first = True
            while True:
                if cancel is not None and cancel.is_set():
                    raise Cancelled()
                remaining = child.remaining()
                interval = POLL_INTERVAL if remaining is None else min(POLL_INTERVAL, remaining)
                try:
                    output, _ = child.process.communicate(input=input if first else None, timeout=interval)
                    break
                except subprocess.TimeoutExpired as error:
                    output = error.output
                    first = False
    except ProcessTimeout as error:
        error.output = output
        error.stderr = child.stderr
        raise
    if check and child.returncode:
        raise ProcessError(child, output)
    return subprocess.CompletedProcess(child.args, child.returncode, output,
                                       child.stderr if capture else None)


class BackgroundProcess:
    """A detached supervised child. pid is the actual job, not its supervisor."""
    def __init__(self, supervisor, pid, argv, log_directory, operation):
        self.supervisor = supervisor
        self.pid = pid
        self.args = argv
        self.log = DiagnosticLog(log_directory, argv, operation)

    @property
    def returncode(self):
        return self.supervisor.returncode

    def poll(self):
        return self.supervisor.poll()

    def wait(self, timeout=None):
        return self.supervisor.wait(timeout=timeout)

    def send_signal(self, signum):
        if self.poll() is not None:
            return
        with contextlib.suppress(ProcessLookupError):
            os.killpg(self.pid, signum)

    def terminate(self):
        self.send_signal(signal.SIGTERM)

    def kill(self):
        self.send_signal(signal.SIGKILL)

    def close(self, grace=STOP_GRACE):
        self.terminate()
        try:
            self.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            self.kill()
            self.wait(timeout=STOP_GRACE * 3)

    def write_input(self, data, timeout=TIMEOUTS["clipboard"]):
        stream = self.supervisor.stdin
        deadline = time.monotonic() + timeout
        try:
            os.set_blocking(stream.fileno(), False)
            view = memoryview(data)
            while view:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ValueError(f"{self.log.program} input exceeded {timeout:g}s; diagnostics: {self.log.path}")
                if select.select([], [stream], [], min(POLL_INTERVAL, remaining))[1]:
                    try:
                        view = view[os.write(stream.fileno(), view[:16384]):]
                    except BlockingIOError:
                        pass
        except BaseException:
            with cancellation_ignored():
                self.close()
            raise
        finally:
            stream.close()


def background(argv, *, log_directory, operation="daemon", timeout=DEFAULT,
               input=None, input_timeout=TIMEOUTS["clipboard"], diagnostic_stdout=False, **kwargs):
    argv = [str(arg) for arg in argv]
    operation, seconds = policy(argv, operation, timeout)
    read_fd, write_fd = os.pipe()
    supervisor = None
    try:
        command = [sys.executable, str(Path(__file__).resolve()), "--worker", str(write_fd),
                   str(log_directory), operation, json.dumps(seconds), json.dumps(diagnostic_stdout), *argv]
        supervisor = subprocess.Popen(command, start_new_session=True, pass_fds=(write_fd,),
                                      stdin=subprocess.PIPE if input is not None else subprocess.DEVNULL,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)
        os.close(write_fd)
        write_fd = None
        if not select.select([read_fd], [], [], TIMEOUTS["control"])[0]:
            raise ValueError("Background process startup exceeded 5s")
        message = json.loads(os.read(read_fd, 4096))
        if "pid" not in message:
            raise ValueError(f"{Path(argv[0]).name} failed to start; diagnostics: {log_directory}/processes.jsonl")
        job = BackgroundProcess(supervisor, message["pid"], argv, log_directory, operation)
        if input is not None:
            job.write_input(input, timeout=input_timeout)
        return job
    except BaseException:
        if supervisor is not None:
            with cancellation_ignored():
                supervisor.terminate()
                try:
                    supervisor.wait(timeout=STOP_GRACE * 3)
                except subprocess.TimeoutExpired:
                    supervisor.kill()
                    supervisor.wait(timeout=STOP_GRACE)
        raise
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)


def _worker(arguments):
    descriptor, directory, operation, seconds, diagnostic_stdout, *argv = arguments
    descriptor = int(descriptor)
    try:
        with cancellation_signals():
            with ManagedProcess(argv, log_directory=directory, operation=operation,
                                timeout=json.loads(seconds), text=False, echo_stderr=False,
                                stdin=sys.stdin.buffer, stdout=subprocess.DEVNULL,
                                diagnostic_stdout=json.loads(diagnostic_stdout)) as child:
                os.write(descriptor, json.dumps({"pid": child.pid}).encode())
                os.close(descriptor)
                descriptor = None
                while True:
                    try:
                        status = child.wait(timeout=POLL_INTERVAL)
                        return status if status >= 0 else 128 - status
                    except subprocess.TimeoutExpired:
                        child.remaining()
    except Cancelled as error:
        return 128 + error.signum
    except (OSError, ValueError, subprocess.SubprocessError):
        if descriptor is not None:
            with contextlib.suppress(OSError):
                os.write(descriptor, b'{"error":"launch-failed"}')
        return 1
    finally:
        if descriptor is not None:
            os.close(descriptor)


if __name__ == "__main__":
    if len(sys.argv) < 8 or sys.argv[1] != "--worker":
        raise SystemExit("desktop_process is an internal desktopctl supervisor")
    raise SystemExit(_worker(sys.argv[2:]))
