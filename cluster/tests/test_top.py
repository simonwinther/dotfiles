"""Exercise the real CLI and TUI through an isolated fake SSH endpoint."""
import fcntl
import codecs
import json
import os
import shlex
import shutil
import sys
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import unittest


BINARY = os.environ["CLUSTER_TEST_BINARY"]


class Terminal:
    def __init__(self, env, *args):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))

        def session():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [BINARY, *args], stdin=slave, stdout=slave, stderr=slave,
            env=env, preexec_fn=session, close_fds=True,
        )
        os.close(slave)
        self.output = b""
        self.rows = [[" "] * 120 for _ in range(36)]
        self.x = self.y = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.pending = ""

    def render(self, chunk):
        self.pending += self.decoder.decode(chunk)
        while self.pending:
            if self.pending.startswith("\x1b["):
                match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.pending)
                if not match:
                    return
                params, _, command = match.groups()
                self.pending = self.pending[match.end():]
                if params.startswith("?"):
                    continue
                values = [int(x or 0) for x in params.split(";")]
                n = values[0] or 1
                if command in "Hf":
                    self.y = max(0, min(35, n - 1))
                    self.x = max(0, min(119, (values[1] or 1) - 1 if len(values) > 1 else 0))
                elif command == "A": self.y = max(0, self.y - n)
                elif command == "B": self.y = min(35, self.y + n)
                elif command == "C": self.x = min(119, self.x + n)
                elif command == "D": self.x = max(0, self.x - n)
                elif command == "G": self.x = min(119, n - 1)
                elif command == "d": self.y = min(35, n - 1)
                elif command == "J" and values[0] == 2:
                    self.rows = [[" "] * 120 for _ in range(36)]
                elif command == "K":
                    if values[0] == 2: self.rows[self.y] = [" "] * 120
                    else: self.rows[self.y][self.x:] = [" "] * (120 - self.x)
                continue
            if self.pending == "\x1b":
                return
            char, self.pending = self.pending[0], self.pending[1:]
            if char == "\r": self.x = 0
            elif char == "\n": self.y = min(35, self.y + 1)
            elif char == "\b": self.x = max(0, self.x - 1)
            elif ord(char) >= 32:
                if self.x >= 120:
                    self.x = 0
                    self.y = min(35, self.y + 1)
                self.rows[self.y][self.x] = char
                self.x += 1

    def screen(self):
        return "\n".join("".join(row) for row in self.rows)

    def send(self, text):
        os.write(self.master, text.encode())

    def wait_for(self, token, timeout=8):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if token in self.screen():
                return
            if select.select([self.master], [], [], 0.1)[0]:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError:
                    break
                self.output += chunk
                self.render(chunk)
        raise AssertionError(f"Missing {token!r}:\n{self.screen()}")

    def close(self):
        if self.process.poll() is None:
            self.send("\x03")
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=3)
        os.close(self.master)


class RemoteTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mytop-integration-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.sshbin = self.root / "ssh-bin"
        self.remote = self.root / "remote-tools"
        self.sshbin.mkdir()
        self.remote.mkdir()
        self.env = dict(os.environ, HOME=str(self.root / "home"),
                        XDG_CONFIG_HOME=str(self.root / "config"),
                        PATH=str(self.sshbin), USER="local-user", TERM="xterm-256color",
                        CLUSTER_TEST_BINARY=BINARY, CLUSTER_TOP_TEST_ROOT=str(self.root),
                        CLUSTER_TEST_SYSTEM_PATH=os.environ.get("PATH", os.defpath))
        self.script(self.sshbin / "mytop", '#!/bin/sh\nexec "$CLUSTER_TEST_BINARY" "$@"\n')
        self.script(self.sshbin / "ssh", f"#!{sys.executable}\n" + r'''import os, subprocess, sys, time
host, command = sys.argv[-2:]
with open(os.environ["CLUSTER_TOP_TEST_ROOT"] + "/ssh-calls", "a") as log:
    log.write(host + "\n")
if host == "offline":
    print("Connection refused", file=sys.stderr)
    sys.exit(255)
if host == "old" and 'snapshot' in command:
    time.sleep(0.5)
env = dict(os.environ, USER=host + "-user", REMOTE_HOST=host,
           PATH=os.environ["CLUSTER_TOP_TEST_ROOT"] + "/remote-tools:" + os.environ["PATH"] + ":" + os.environ["CLUSTER_TEST_SYSTEM_PATH"])
sys.exit(subprocess.run(["/bin/sh", "-c", command], env=env).returncode)
''')
        self.script(self.remote / "scontrol", f"#!{sys.executable}\n" + r'''import os, sys
host = os.environ["REMOTE_HOST"]
if sys.argv[1:3] == ["show", "nodes"]:
    print(f"NodeName=node-{host} State=IDLE Partitions=gpu Gres=gpu:a100:4 CfgTRES=gres/gpu=4 AllocTRES= CPUAlloc=0 CPUTot=8 RealMemory=32000 AllocMem=0")
elif sys.argv[1:3] == ["show", "job"]:
    path = os.environ["CLUSTER_TOP_TEST_ROOT"] + "/remote 'quoted' $(literal)/job-123.out"
    print(f"JobId=123 WorkDir=/remote StdOut={path} StdErr={path}")
else:
    sys.exit(2)
''')
        self.script(self.remote / "squeue", f"#!{sys.executable}\n" + r'''import os
host = os.environ["REMOTE_HOST"]
print("\x1f".join(["123", host + "-user", "RUNNING", "gpu", "100", "2026-10-18T10:00:00", "00:10", "01:00:00", "gpu:1", "node-" + host, "job-" + host, "node-" + host]))
''')
        self.script(self.remote / "sacct", f"#!{sys.executable}\n" + r'''import os, sys
host = os.environ["REMOTE_HOST"]
if host == "noaccounting":
    print("Slurm accounting storage is disabled", file=sys.stderr)
    sys.exit(1)
print("\x1f".join(["122", host + "-user", "finished", "gpu", "COMPLETED", "2026-10-18T10:00:00", "00:10", "01:00:00", "gres/gpu=1", "node-" + host, "2026-10-18T10:00:10", "0:0", "/remote", "2026-10-18T10:00:00", "2026-10-18T10:00:00", "8", "1000M", "", "", "gres/gpu=1", ""]))
''')
        self.log = self.root / "remote 'quoted' $(literal)/job-123.out"
        self.log.parent.mkdir()
        self.log.write_text("REMOTE_LOG_MARKER\nsecond line\n")

    @staticmethod
    def script(path, text):
        path.write_text(text)
        path.chmod(0o755)

    def run_cli(self, *args, ok=True):
        command = args if args and args[0] in ("hosts", "agent") else ("top", *args)
        result = subprocess.run([BINARY, *command], env=self.env, text=True,
                                capture_output=True, timeout=12)
        if ok:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0)
        return result

    def terminal(self, *args):
        terminal = Terminal(self.env, "top", *args)
        self.addCleanup(terminal.close)
        return terminal

    def test_saved_hosts_add_remove_duplicate_and_no_connection(self):
        self.run_cli("hosts", "add", "alpha")
        self.run_cli("hosts", "add", "beta")
        self.run_cli("hosts", "add", "alpha", ok=False)
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["alpha", "beta"])
        self.run_cli("hosts", "remove", "alpha")
        self.run_cli("hosts", "remove", "missing", ok=False)
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["beta"])
        path = self.root / "config/cluster/hosts.json"
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertFalse((self.root / "ssh-calls").exists())

    def test_legacy_host_list_migrates_once_without_losing_entries(self):
        legacy = self.root / "config/mytop/clusters.json"
        legacy.parent.mkdir(parents=True)
        legacy.write_text(json.dumps({"hosts": ["alpha", "beta"]}))
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["alpha", "beta"])
        self.assertTrue((self.root / "config/cluster/hosts.json").is_file())
        self.run_cli("hosts", "remove", "alpha")
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["beta"])
        self.assertEqual(json.loads(legacy.read_text())["hosts"], ["alpha", "beta"])

    def test_installer_preserves_previous_binary_and_registers_host(self):
        import hashlib
        destination = self.root / "home/.local/bin/cluster"
        destination.parent.mkdir(parents=True)
        old = b"previous binary fixture\n"
        destination.write_bytes(old)
        result = self.run_cli("hosts", "install", "alpha")
        self.assertIn("Installed cluster and mytop", result.stdout)
        self.assertEqual(hashlib.sha256(destination.read_bytes()).digest(), hashlib.sha256(Path(BINARY).read_bytes()).digest())
        self.assertEqual(destination.stat().st_mode & 0o777, 0o755)
        alias = destination.with_name("mytop")
        result = subprocess.run([str(alias), "--once"], env=dict(self.env, PATH=str(self.remote) + ":/usr/bin:/bin", REMOTE_HOST="local"), text=True, capture_output=True, timeout=12)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("node-local", result.stdout)
        backups = list((self.root / "home/.local/share/cluster/backups").iterdir())
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_bytes(), old)
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["alpha"])

    def test_remote_snapshot_identity_history_and_failure(self):
        for host in ["alpha", "beta"]:
            result = self.run_cli("--host", host, "--once")
            self.assertIn(f"node-{host}", result.stdout)
            self.assertIn(f"{host}-user", result.stdout)
            self.assertNotIn("local-user", result.stdout)
        self.assertIn("chosen", self.run_cli("--host", "alpha", "--user", "chosen", "--once").stdout)
        history = self.run_cli("--host", "alpha", "--once", "--tab", "4")
        self.assertIn("finished", history.stdout)
        self.assertIn("accounting storage is disabled", self.run_cli("--host", "noaccounting", "--once", "--tab", "4", ok=False).stderr)
        self.assertIn("Connection refused", self.run_cli("--host", "offline", "--once", ok=False).stderr)
        self.run_cli("--host=-oBad", "--once", ok=False)
        self.assertNotIn("-oBad", (self.root / "ssh-calls").read_text())

    def test_picker_switch_while_old_request_is_in_flight(self):
        self.run_cli("hosts", "add", "old")
        self.run_cli("hosts", "add", "beta")
        terminal = self.terminal()
        terminal.wait_for("Clusters")
        terminal.send("\r")
        terminal.wait_for("[old]")
        terminal.send("c")
        terminal.wait_for("current")
        terminal.output = b""
        terminal.send("j\r")
        terminal.wait_for("node-beta")
        terminal.wait_for("beta-user")
        self.assertNotIn(b"node-old", terminal.output)
        self.assertIsNone(terminal.process.poll())

    def test_remote_log_and_reload_use_remote_paths(self):
        terminal = self.terminal("--host", "alpha", "--tab", "2")
        terminal.wait_for("job-alpha")
        terminal.send("\r")
        terminal.wait_for("REMOTE_LOG_MARKER")
        self.log.write_text("UPDATED_REMOTE_LOG\n")
        terminal.output = b""
        terminal.send("r")
        terminal.wait_for("UPDATED_REMOTE_LOG")
        terminal.send("c")
        terminal.wait_for("Clusters")

    def test_picker_adds_and_removes_saved_hosts_without_connecting(self):
        terminal = self.terminal()
        terminal.wait_for("No saved clusters")
        terminal.send("a")
        terminal.wait_for("SSH alias:")
        terminal.send("NewQ\r")
        terminal.wait_for("NewQ")
        end = time.monotonic() + 4
        while time.monotonic() < end:
            if self.run_cli("hosts", "list").stdout.splitlines() == ["NewQ"]:
                break
            time.sleep(0.05)
        self.assertEqual(self.run_cli("hosts", "list").stdout.splitlines(), ["NewQ"])
        terminal.send("d")
        end = time.monotonic() + 4
        while time.monotonic() < end:
            if not self.run_cli("hosts", "list").stdout:
                break
            time.sleep(0.05)
        self.assertEqual(self.run_cli("hosts", "list").stdout, "")
        self.assertFalse((self.root / "ssh-calls").exists())
        self.assertIsNone(terminal.process.poll())

    def test_failed_connection_does_not_prevent_switching(self):
        self.run_cli("hosts", "add", "offline")
        self.run_cli("hosts", "add", "beta")
        terminal = self.terminal("--host", "offline")
        terminal.wait_for("Connection refused")
        terminal.send("c")
        terminal.wait_for("Clusters")
        terminal.output = b""
        terminal.send("j\r")
        terminal.wait_for("node-beta")
        terminal.wait_for("beta-user")
        self.assertIsNone(terminal.process.poll())


if __name__ == "__main__":
    unittest.main()
