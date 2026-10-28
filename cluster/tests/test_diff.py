"""Compare real folders through rsync, including a read-only fake SSH server."""

import os
import shlex
import shutil
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest


BINARY = os.environ["CLUSTER_TEST_BINARY"]


class DiffTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cluster-diff-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = self.root / "local project"
        self.remote_home = self.root / "remote-home"
        self.remote = self.remote_home / "project 'quoted'"
        self.home = self.root / "home"
        self.bin = self.root / "bin"
        for path in (self.local, self.remote, self.home, self.bin):
            path.mkdir(parents=True)
        self.env = dict(os.environ, HOME=str(self.home),
                        XDG_CONFIG_HOME=str(self.home / ".config"),
                        XDG_CACHE_HOME=str(self.home / ".cache"),
                        PATH=str(self.bin) + ":" + os.environ.get("PATH", os.defpath),
                        REMOTE_HOME=str(self.remote_home),
                        SSH_LOG=str(self.root / "ssh-log"))
        ssh = self.bin / "ssh"
        ssh.write_text(f"#!{sys.executable}\n" + '''import os, sys
args = sys.argv[1:]
while args and args[0] == '-o':
    del args[:2]
if args and args[0] == '--':
    args.pop(0)
host = args.pop(0)
if host == 'offline':
    print('Connection refused', file=sys.stderr)
    sys.exit(255)
command = ' '.join(args)
assert command.startswith('rsync --server --sender '), command
with open(os.environ['SSH_LOG'], 'a') as log:
    log.write(command + '\\n')
os.chdir(os.environ['REMOTE_HOME'])
os.execl('/bin/sh', 'sh', '-c', command)
''')
        ssh.chmod(0o755)

    def diff(self, *args, host="test-host", directory="~/project 'quoted'", check=True, cwd=None):
        selection = ["--local"] if host == "local" else ["-h", host]
        result = subprocess.run([BINARY, *selection, "diff", "--with", directory, *args],
                                env=self.env, cwd=cwd or self.local, text=True,
                                capture_output=True, timeout=15)
        if check:
            self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def write(self, root, name, text):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    @staticmethod
    def tree(root):
        result = {}
        for path in sorted(root.rglob("*")):
            info = path.lstat()
            value = (os.readlink(path) if path.is_symlink() else
                     path.read_bytes() if path.is_file() else None)
            result[str(path.relative_to(root))] = (info.st_mode, info.st_mtime_ns, value)
        return result

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.local, env=self.env,
                              text=True, capture_output=True, check=True).stdout

    def test_same_contents_ignore_metadata_and_git_without_creating_caches(self):
        local = self.write(self.local, "same.py", "identical")
        remote = self.write(self.remote, "same.py", "identical")
        local.chmod(0o600)
        remote.chmod(0o755)
        os.utime(local, (1000, 1000))
        os.utime(remote, (2000, 2000))
        self.write(self.local, ".git/local-only", "private metadata")
        self.write(self.remote, ".git/remote-only", "other metadata")
        before = [self.tree(p) for p in (self.local, self.remote_home, self.home)]
        result = self.diff()
        self.assertIn("In sync", result.stdout)
        self.assertNotIn("Different —", result.stdout)
        self.assertEqual(before, [self.tree(p) for p in (self.local, self.remote_home, self.home)])
        self.assertEqual(len((self.root / "ssh-log").read_text().splitlines()), 1)

    def test_changed_contents_and_renames_are_grouped_without_changing_either_side(self):
        for root, contents in ((self.local, "local"), (self.remote, "other")):
            path = self.write(root, "same-size.py", contents)
            os.utime(path, (1000, 1000))
        self.write(self.local, "guided_diffusion/wmf.py", "local renamed code")
        self.write(self.remote, "guided_diffusion/wfm.py", "old remote code")
        before = [self.tree(p) for p in (self.local, self.remote_home)]
        result = self.diff()
        self.assertIn("1 local only · 1 remote only · 1 changed", result.stdout)
        self.assertIn("Only on this PC (1)\n  guided_diffusion/wmf.py", result.stdout)
        self.assertIn("Only on test-host (1)\n  guided_diffusion/wfm.py", result.stdout)
        self.assertIn("Different contents or type (1)\n  same-size.py", result.stdout)
        self.assertNotIn("ahead", result.stdout)
        self.assertNotIn("behind", result.stdout)
        self.assertEqual(before, [self.tree(p) for p in (self.local, self.remote_home)])

    def test_type_changes_links_and_control_characters_are_reported_safely(self):
        self.write(self.local, "replacement", "a file")
        self.write(self.remote, "replacement/child", "remote directory")
        (self.local / "link").symlink_to("local-target")
        (self.remote / "link").symlink_to("remote-target")
        self.write(self.remote, "line\nbreak.py", "remote-only")
        self.write(self.local, "literal\\#012.py", "local-only")
        result = self.diff()
        self.assertIn("2 changed", result.stdout)
        self.assertIn("line\\#012break.py", result.stdout)
        self.assertNotIn("line\nbreak.py", result.stdout)
        self.assertIn("replacement/", result.stdout)
        self.assertEqual(os.readlink(self.local / "link"), "local-target")
        self.assertTrue((self.local / "replacement").is_file())

    def test_code_only_honors_nested_global_and_tracked_ignore_rules(self):
        self.git("init", "--quiet")
        self.write(self.local, ".gitignore", "runs/\n__pycache__/\n*.tmp\n!keep.tmp\n")
        self.write(self.remote, ".gitignore", (self.local / ".gitignore").read_text())
        self.write(self.local, "nested/.gitignore", "*.hidden\n")
        self.write(self.remote, "nested/.gitignore", "*.hidden\n")
        self.write(self.local, ".git/info/exclude", "private/\n")
        global_ignore = self.write(self.root, "global-ignore", "*.global\n")
        self.git("config", "core.excludesFile", str(global_ignore))
        for name in ("runs/checkpoint.pt", "__pycache__/code.pyc", "skip.tmp",
                     "nested/cache.hidden", "private/secret", "cache.global", "line\nbreak.tmp"):
            self.write(self.local, name, "local ignored")
            self.write(self.remote, name, "other ignored")
        self.write(self.local, "tracked.tmp", "local tracked")
        self.git("add", "-f", "tracked.tmp")
        self.write(self.remote, "tracked.tmp", "remote tracked")
        self.write(self.local, "keep.tmp", "same")
        self.write(self.remote, "keep.tmp", "same")
        self.write(self.local, "wmf.py", "new name")
        self.write(self.remote, "wfm.py", "old name")
        before = [self.tree(p) for p in (self.local, self.remote_home, self.home)]
        result = self.diff("--code-only")
        self.assertIn("1 local only · 1 remote only · 1 changed", result.stdout)
        for name in ("checkpoint.pt", "code.pyc", "skip.tmp", "cache.hidden", "secret", "cache.global", "break.tmp"):
            self.assertNotIn(name, result.stdout)
        self.assertIn("tracked.tmp", result.stdout)
        self.assertIn("wmf.py", result.stdout)
        self.assertIn("wfm.py", result.stdout)
        self.assertEqual(before, [self.tree(p) for p in (self.local, self.remote_home, self.home)])
        self.assertIn("checkpoint.pt", self.diff().stdout)

    def test_local_mode_works_without_git_and_full_output_is_optional(self):
        for i in range(23):
            self.write(self.remote, f"file-{i:02d}.py", "remote only")
        result = self.diff(host="local", directory=str(self.remote))
        self.assertIn("23 remote only", result.stdout)
        self.assertIn("3 more; use --all", result.stdout)
        self.assertNotIn("file-22.py", result.stdout)
        result = self.diff("--all", host="local", directory=str(self.remote))
        self.assertIn("file-22.py", result.stdout)
        self.assertFalse((self.root / "ssh-log").exists())

    def test_missing_inputs_and_code_only_outside_git_fail_before_connecting(self):
        for args, message in [
            (["diff", "--with", "~/project"], "Missing SSH host"),
            (["diff", "-h", "test-host"], "Missing comparison directory"),
            (["diff", "-h", "test-host", "--with", "~/project", "--code-only"], "requires a local Git working tree"),
        ]:
            result = subprocess.run([BINARY, *args], env=self.env, cwd=self.local,
                                    text=True, capture_output=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)
        self.assertFalse((self.root / "ssh-log").exists())

    def test_missing_source_and_connection_failures_never_claim_in_sync(self):
        for options in ({"host": "offline"}, {"directory": "~/does-not-exist"}):
            result = self.diff(check=False, **options)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Comparison failed", result.stderr)
            self.assertNotIn("In sync", result.stdout)
        self.assertEqual(self.tree(self.local), {})

    def test_paths_with_shell_fragments_are_literal_and_tilde_is_remote(self):
        path = self.remote_home / "literal ' $(touch injected)"
        self.write(path, "code.py", "remote only")
        result = self.diff(directory="~/literal ' $(touch injected)")
        self.assertIn("code.py", result.stdout)
        self.assertFalse((self.remote_home / "injected").exists())


if __name__ == "__main__":
    unittest.main()
