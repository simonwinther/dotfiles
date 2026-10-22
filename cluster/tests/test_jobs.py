"""Test against Git checkouts and fake Slurm commands, without submitting jobs."""

import json
import hashlib
import os
import shlex
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / ".local/bin/cluster"
BINARY = Path(os.environ.get("CLUSTER_TEST_BINARY", str(Path.home() / ".cache/cluster/bin/cluster-native")))


class JobTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cluster-jobs-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = self.root / "local repo"
        self.checkout = self.root / "remote checkout's name"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = dict(os.environ, PATH=str(self.bin) + ":/usr/bin:/bin",
                        HOME=str(self.home), XDG_CACHE_HOME=str(self.root / "cache"),
                        XDG_DATA_HOME=str(self.home / ".local/share"),
                        XDG_STATE_HOME=str(self.root / "state"))
        self.local.mkdir()
        self.git("init", "--quiet")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        (self.local / "scripts").mkdir()
        (self.local / "scripts/train.sbatch").write_text("#!/bin/bash\ntrue\n")
        self.git("add", "scripts/train.sbatch")
        self.git("-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "Initial")
        self.git("remote", "add", "origin", "git@github.com:example/project.git")
        subprocess.run(["git", "clone", "--quiet", str(self.local), str(self.checkout)], check=True)
        self.git("remote", "set-url", "origin", "https://github.com/example/project.git", cwd=self.checkout)
        self.marker = self.root / "submitted.json"
        self.stdout = self.root / "job output.log"
        self.stderr = self.root / "job errors.log"
        self.stdout.write_text("training output\n")
        self.stderr.write_text("training error\n")
        self.tool("ssh", '#!/bin/bash\n[[ ${1:-} != -- ]] || shift\nshift\nexec /bin/bash -c "$*"\n')
        self.tool("sbatch", "#!/usr/bin/env python3\nimport json,os,pathlib,sys\n"
                  + "pathlib.Path(" + repr(str(self.marker)) + ").write_text(json.dumps({"
                  + "'args':sys.argv[1:], 'cwd':os.getcwd(), 'run':os.environ.get('RUN_NAME')}))\n"
                  + "print('456')\n")
        record = "JobId=456 JobName=training run UserId=test(123) WorkDir={} StdOut={} StdErr={}".format(
            self.checkout, self.stdout, self.stderr)
        self.tool("scontrol", "#!/usr/bin/env python3\nprint(" + repr(record) + ")\n")
        self.run_cli("link", "test-host", str(self.checkout))

    def git(self, *args, cwd=None):
        result = subprocess.run(["git"] + list(args), cwd=str(cwd or self.local),
                                text=True, capture_output=True, check=True)
        return result.stdout.strip()

    def tool(self, name, source):
        path = self.bin / name
        path.write_text(source)
        path.chmod(0o755)

    def run_cli(self, *args, check=True, cwd=None, explicit_host=True):
        if explicit_host and args and args[0] not in ("link", "agent", "--local", "--host", "-h", "-H"):
            args = ("--host", "test-host", *args)
        result = subprocess.run([str(BINARY)] + list(args),
                                cwd=str(cwd or self.local), env=self.env,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=15)
        if check:
            self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def test_remote_operations_do_not_require_python(self):
        self.tool("python3", "#!/bin/bash\necho 'Python must not run' >&2\nexit 99\n")
        result = self.run_cli("submit", "--dry-run", "scripts/train.sbatch")
        self.assertIn("no job submitted", result.stdout)
        self.assertNotIn("Python must not run", result.stdout)
        agents = list((self.root / "cache/cluster/agents").iterdir())
        self.assertEqual(len(agents), 1)
        self.assertEqual(agents[0].read_bytes()[:4], b"\x7fELF")

    def test_link_saves_remote_root_independently_of_local_name(self):
        self.assertEqual(self.git("config", "--get", "cluster.test-host.path"), str(self.checkout))
        self.assertNotIn("cluster.host=", self.git("config", "--local", "--list"))
        self.assertFalse(self.marker.exists())

    def test_missing_link_exits_cleanly_before_ssh_or_submission(self):
        self.git("config", "--unset", "cluster.test-host.path")
        self.tool("ssh", "#!/bin/bash\necho 'SSH MUST NOT RUN'\nexit 99\n")
        for dirty in (False, True):
            if dirty:
                (self.local / "scripts/train.sbatch").write_text("#!/bin/bash\necho changed\n")
            with self.subTest(dirty=dirty):
                result = self.run_cli("submit", "scripts/train.sbatch", check=False)
                self.assertEqual(result.returncode, 1, result.stdout)
                self.assertIn("This checkout is not linked to test-host", result.stdout)
                self.assertIn("cluster link test-host REMOTE_CHECKOUT_PATH", result.stdout)
                self.assertNotIn("Traceback", result.stdout)
                self.assertNotIn("SSH MUST NOT RUN", result.stdout)
                self.assertFalse(self.marker.exists())

    def test_missing_link_for_another_host_does_not_use_default_pairing(self):
        result = self.run_cli("--host", "another-host", "submit", "scripts/train.sbatch", check=False)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("cluster link another-host REMOTE_CHECKOUT_PATH", result.stdout)
        self.assertNotIn("Traceback", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_submit_outside_repository_exits_without_traceback(self):
        result = self.run_cli("submit", "scripts/train.sbatch", cwd=self.root, check=False)
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertNotIn("Traceback", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_link_check_does_not_save_another_pairing(self):
        self.run_cli("link", "another-host", str(self.checkout), "--check")
        result = subprocess.run(["git", "config", "--get", "cluster.another-host.path"],
                                cwd=self.local, capture_output=True)
        self.assertNotEqual(result.returncode, 0)

    def test_link_rejects_other_origin(self):
        self.git("remote", "set-url", "origin", "https://github.com/example/other.git", cwd=self.checkout)
        result = self.run_cli("link", "other-host", str(self.checkout), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("different Git origins", result.stdout)
        self.assertEqual(self.git("config", "--get", "cluster.test-host.path"), str(self.checkout))

    def test_remote_commands_require_explicit_host_even_with_saved_metadata(self):
        self.git("config", "cluster.host", "old-default-host")
        self.tool("ssh", "#!/bin/bash\necho 'SSH MUST NOT RUN'\nexit 99\n")
        for args in (("sync",), ("sync", "--dry-run"), ("fetch",),
                     ("submit", "scripts/train.sbatch"),
                     ("submit", "--sync", "scripts/train.sbatch"),
                     ("jobs",), ("logs", "456")):
            with self.subTest(args=args):
                result = self.run_cli(*args, check=False, explicit_host=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn("Missing SSH host. Add -h HOST", result.stdout)
                self.assertNotIn("SSH MUST NOT RUN", result.stdout)
                self.assertFalse(self.marker.exists())
        self.assertFalse((self.home / ".local/share/cluster/runs").exists())

    def test_host_flags_work_before_and_after_subcommand_and_help_stays_available(self):
        for args in (("-h", "test-host", "sync", "--to", "dev/project", "--dry-run"),
                     ("sync", "-h", "test-host", "--to", "dev/project", "--dry-run"),
                     ("sync", "--host", "test-host", "--to", "dev/project", "--dry-run"),
                     ("-H", "test-host", "sync", "--to", "dev/project", "--dry-run")):
            with self.subTest(args=args):
                result = self.run_cli(*args, explicit_host=False)
                self.assertIn("Would sync to test-host:", result.stdout)
        for args in (("--help",), ("sync", "--help"), ("submit", "--help")):
            with self.subTest(args=args):
                result = self.run_cli(*args, explicit_host=False)
                self.assertIn("--host", result.stdout)
                self.assertIn("--help", result.stdout)

    def test_dry_run_preserves_subdirectory_and_never_calls_sbatch(self):
        result = self.run_cli("submit", "--dry-run", "--", "--time=00:05:00", "train.sbatch", cwd=self.local / "scripts")
        command_line = next(line for line in result.stdout.splitlines() if line.startswith("test-host: cd "))
        self.assertEqual(shlex.split(command_line)[2], str(self.checkout / "scripts"))
        self.assertIn("--time=00:05:00", result.stdout)
        self.assertIn("no job submitted", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_submit_forwards_arguments_environment_and_working_directory(self):
        result = self.run_cli("submit", "--env", "RUN_NAME=run with spaces", "--", "--time=00:05:00", "train.sbatch", "a b", cwd=self.local / "scripts")
        record = json.loads(self.marker.read_text())
        self.assertEqual(record["args"], ["--parsable", "--time=00:05:00", "train.sbatch", "a b"])
        self.assertEqual(record["cwd"], str(self.checkout / "scripts"))
        self.assertEqual(record["run"], "run with spaces")
        self.assertIn("Submitted job 456", result.stdout)
        self.assertIn("cluster --host test-host logs 456 --follow", result.stdout)

    def test_origin_credentials_are_not_sent_to_the_cluster(self):
        self.git("remote", "set-url", "origin", "https://fake-sensitive-token@github.com/example/project.git")
        self.tool("ssh", '#!/bin/bash\n[[ ${!#} != *fake-sensitive-token* ]] || exit 99\nexec /bin/bash -c "${!#}"\n')
        result = self.run_cli("submit", "scripts/train.sbatch")
        self.assertIn("Submitted job 456", result.stdout)

    def test_local_changes_prevent_submission(self):
        (self.local / "scripts/train.sbatch").write_text("#!/bin/bash\necho changed\n")
        result = self.run_cli("submit", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Commit your local tracked changes", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_remote_changes_prevent_submission(self):
        (self.checkout / "scripts/train.sbatch").write_text("#!/bin/bash\necho changed\n")
        result = self.run_cli("submit", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cluster checkout has uncommitted", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_different_commits_prevent_submission(self):
        self.git("-c", "commit.gpgsign=false", "commit", "--quiet", "--allow-empty", "-m", "New commit")
        result = self.run_cli("submit", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("commits differ", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_sync_respects_git_ignores_and_preserves_local_changes(self):
        (self.local / ".gitignore").write_text(".venv/\nignored/\n*.tmp\n!keep.tmp\n")
        (self.local / "scripts/.gitignore").write_text("*.ignored\n")
        (self.local / "deleted.txt").write_text("delete this\n")
        (self.local / "tracked.tmp").write_text("tracked despite ignore\n")
        self.git("add", ".gitignore", "scripts/.gitignore", "deleted.txt")
        self.git("add", "-f", "tracked.tmp")
        self.git("-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "Fixture")
        base = self.git("rev-parse", "HEAD")
        (self.local / "deleted.txt").unlink()
        script = self.local / "scripts/train.sbatch"
        script.write_text("#!/bin/bash\necho staged\n")
        self.git("add", "scripts/train.sbatch")
        script.write_text("#!/bin/bash\necho unstaged\n")
        script.chmod(0o755)
        new_script = self.local / "scripts/smoke's run.sbatch"
        new_script.write_text("#!/bin/bash\necho new\n")
        for name in ("keep.tmp", "-new\nfile.py", "tracked.tmp"):
            (self.local / name).write_text("local content\n")
        for name in (".venv/secret", "ignored/secret", "private/secret",
                     "scripts/cache.ignored", "skip.tmp", "skip.global"):
            path = self.local / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("must not upload\n")
        (self.local / ".git/info/exclude").write_text("private/\n")
        global_ignore = self.root / "global ignore"
        global_ignore.write_text("*.global\n")
        self.git("config", "core.excludesFile", str(global_ignore))
        (self.local / "script-link").symlink_to("scripts/train.sbatch")
        os.link(script, self.local / "hard-link")
        before = [self.git(*args) for args in (
            ("status", "--porcelain"), ("diff", "--binary"),
            ("diff", "--cached", "--binary"), ("rev-parse", "HEAD"))]

        self.run_cli("submit", "--sync", "--env", "RUN_NAME=snapshot test", "--",
                     "--time=00:05:00", new_script.name, "data with spaces",
                     cwd=self.local / "scripts")
        record = json.loads(self.marker.read_text())
        source = Path(record["cwd"]).parent
        self.assertEqual(record["args"], ["--parsable", "--time=00:05:00",
                                        new_script.name, "data with spaces"])
        self.assertEqual(record["run"], "snapshot test")
        self.assertEqual((source / "scripts/train.sbatch").read_text(), script.read_text())
        self.assertEqual((source / "scripts/train.sbatch").stat().st_mode & 0o777, 0o755)
        for name in ("keep.tmp", "-new\nfile.py", "tracked.tmp", "hard-link"):
            self.assertTrue((source / name).is_file(), name)
        self.assertEqual(os.readlink(source / "script-link"), "scripts/train.sbatch")
        for name in (".git", ".venv", "ignored", "private", "scripts/cache.ignored",
                     "skip.tmp", "skip.global", "deleted.txt"):
            self.assertFalse((source / name).exists(), name)
        manifest = json.loads((source.parent / "manifest.json").read_text())
        self.assertEqual(manifest["base_commit"], base)
        self.assertEqual(manifest["subdir"], "scripts")
        self.assertIn("echo unstaged", (source.parent / "changes.patch").read_text())
        after = [self.git(*args) for args in (
            ("status", "--porcelain"), ("diff", "--binary"),
            ("diff", "--cached", "--binary"), ("rev-parse", "HEAD"))]
        self.assertEqual(before, after)
        self.assertEqual((self.checkout / "scripts/train.sbatch").read_text(), "#!/bin/bash\ntrue\n")

    def test_standalone_sync_copies_only_current_directory_to_chosen_destination(self):
        self.git("config", "--unset", "cluster.test-host.path")
        self.git("remote", "remove", "origin")
        (self.local / ".gitignore").write_text("*.ignored\n")
        (self.local / "scripts/secret.ignored").write_text("must not upload\n")
        (self.local / "outside.txt").write_text("outside source directory\n")
        (self.local / "scripts/train.sbatch").write_text("local change\n")
        (self.local / "scripts/new.sbatch").write_text("new script\n")
        before = self.git("status", "--porcelain")
        self.tool("sbatch", "#!/bin/bash\necho 'SBATCH MUST NOT RUN' >&2\nexit 99\n")
        destination = self.root / "chosen output's name"
        result = self.run_cli("sync", "--to", str(destination), cwd=self.local / "scripts")
        self.assertIn("new.sbatch", result.stdout)
        self.assertIn("100%", result.stdout)
        self.assertIn("xfr#", result.stdout)
        self.assertIn("to-chk=0/", result.stdout)
        line = next(line for line in result.stdout.splitlines() if line.startswith("Synced to "))
        workdir = Path(line.removeprefix("Synced to test-host:"))
        self.assertEqual(workdir, destination)
        self.assertEqual((workdir / "train.sbatch").read_text(), "local change\n")
        self.assertEqual((workdir / "new.sbatch").read_text(), "new script\n")
        self.assertFalse((workdir / "secret.ignored").exists())
        self.assertFalse((workdir / "outside.txt").exists())
        self.assertFalse((workdir / "scripts").exists())
        self.assertFalse((workdir / ".git").exists())
        self.assertFalse(self.marker.exists())
        self.assertEqual(before, self.git("status", "--porcelain"))
        (destination / "remote-only.txt").write_text("obsolete code\n")
        (self.local / "scripts/train.sbatch").write_text("updated content\n")
        (self.local / "scripts/new.sbatch").unlink()
        self.run_cli("sync", "--to", str(destination), cwd=self.local / "scripts")
        self.assertEqual((destination / "train.sbatch").read_text(), "updated content\n")
        self.assertFalse((destination / "remote-only.txt").exists())
        self.assertFalse((destination / "new.sbatch").exists())

    def test_standalone_sync_dry_run_previews_deletions_without_changing_files(self):
        self.git("config", "--unset", "cluster.test-host.path")
        destination = self.home / "dev/WMF"
        (destination / "scripts").mkdir(parents=True)
        (destination / "obsolete.txt").write_text("must stay during preview\n")
        (destination / "scripts/train.sbatch").write_text("old content\n")
        before = {str(p.relative_to(destination)): p.read_bytes()
                  for p in destination.rglob('*') if p.is_file()}
        for args in (
            ("sync", "-h", "test-host", "--to", "~/dev/WMF", "--dry-run"),
            ("sync", "--dry-run", "-h", "test-host", "--to", "dev/WMF"),
        ):
            with self.subTest(args=args):
                result = self.run_cli(*args, explicit_host=False)
                self.assertIn("Previewing mirror of 1 files", result.stdout)
                self.assertIn("*deleting", result.stdout)
                self.assertIn("obsolete.txt", result.stdout)
                self.assertIn(f"Would sync to test-host:{destination}", result.stdout)
                after = {str(p.relative_to(destination)): p.read_bytes()
                         for p in destination.rglob('*') if p.is_file()}
                self.assertEqual(before, after)
        local_target = self.root / "dry-run must not create this"
        result = self.run_cli("--local", "sync", "--to", str(local_target), "--dry-run")
        self.assertIn("would upload", result.stdout)
        self.assertFalse(local_target.exists())
        self.assertFalse(self.marker.exists())
        self.assertFalse((self.home / ".local/share/cluster/runs").exists())

    def test_mirror_preserves_git_ignored_remote_files_but_deletes_removed_tracked_files(self):
        (self.local / '.gitignore').write_text('.venv/\ndata/\n*.tmp\n!keep.tmp\n')
        (self.local / 'scripts/.gitignore').write_text('cache/\n')
        (self.local / 'scripts/data').mkdir()
        (self.local / 'scripts/data/tracked.py').write_text('remove this later\n')
        self.git('add', '-f', 'scripts/data/tracked.py')
        global_ignore = self.root / 'global-ignore'
        global_ignore.write_text('*.secret\n')
        self.git('config', 'core.excludesFile', str(global_ignore))
        destination = self.root / 'mirror'
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        for name in ('.venv/keep.py', 'data/keep.bin', 'cache/keep.bin', 'remote.tmp',
                     'remote.secret', '.git/config', 'nested/.git/config'):
            file = destination / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('preserve ignored data\n')
        (destination / 'keep.tmp').write_text('negated ignore: delete this\n')
        (destination / 'obsolete/old.py').parent.mkdir()
        (destination / 'obsolete/old.py').write_text('delete\n')
        (self.local / 'scripts/data/tracked.py').unlink()
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        for name in ('.venv/keep.py', 'data/keep.bin', 'cache/keep.bin', 'remote.tmp',
                     'remote.secret', '.git/config', 'nested/.git/config'):
            self.assertEqual((destination / name).read_text(), 'preserve ignored data\n', name)
        for name in ('data/tracked.py', 'keep.tmp', 'obsolete'):
            self.assertFalse((destination / name).exists(), name)

    def test_mirror_handles_literal_filename_patterns_and_checks_file_contents(self):
        destination = self.root / 'literal mirror'
        names = ['star*.py', 'question?.py', '[literal].py', 'slash\\name.py', 'line\nname.py']
        for name in names:
            (self.local / 'scripts' / name).write_text('new bytes\n')
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        for name in names:
            self.assertEqual((destination / name).read_text(), 'new bytes\n', name)
        file = destination / names[0]
        before = file.stat()
        file.write_text('old bytes\n')
        os.utime(file, ns=(before.st_atime_ns, before.st_mtime_ns))
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        self.assertEqual(file.read_text(), 'new bytes\n')
        (self.local / 'scripts' / names[-1]).unlink()
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        self.assertFalse((destination / names[-1]).exists())

    def test_mirror_can_delete_the_last_source_file_and_empty_remote_directories(self):
        destination = self.root / 'empty mirror'
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        (self.local / 'scripts/train.sbatch').unlink()
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        self.assertEqual(list(destination.iterdir()), [])

    def test_mirror_refuses_home_root_and_git_metadata_destinations(self):
        for target in ('~', '/', '~/project/..', '~/project/.git'):
            with self.subTest(target=target):
                result = self.run_cli('sync', '--to', target, check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertFalse((self.home / 'project').exists())
        alias = self.root / 'metadata-alias'
        alias.symlink_to(self.checkout / '.git', target_is_directory=True)
        result = self.run_cli('sync', '--to', str(alias), check=False)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('Git metadata', result.stdout)

    def test_mirror_reconciles_a_large_mixed_tree_without_changing_git(self):
        source = self.local / 'scripts'
        destination = self.root / 'mixed mirror'
        (source / '.gitignore').write_text('cache/\n*.output\n')
        for index in range(240):
            file = source / f'group {index % 12}' / f'file-{index}.bin'
            file.parent.mkdir(exist_ok=True)
            file.write_bytes(bytes(range(256)) + str(index).encode())
        executable = source / 'run café.sh'
        executable.write_text('#!/bin/sh\ntrue\n')
        executable.chmod(0o755)
        (source / 'current').symlink_to('group 0/file-0.bin')
        (source / 'cache').mkdir()
        (source / 'cache/local-only').write_text('never upload\n')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        (destination / 'cache').mkdir()
        (destination / 'cache/results').write_text('preserve results\n')
        (destination / 'job.output').write_text('preserve log\n')
        (destination / 'obsolete').mkdir()
        (destination / 'obsolete/remote-only').write_text('remove\n')
        for index in range(240):
            file = source / f'group {index % 12}' / f'file-{index}.bin'
            if index % 4 == 0:
                file.unlink()
            elif index % 4 == 1:
                file.write_bytes(b'changed\x00' + str(index).encode())
            elif index % 4 == 2:
                file.rename(file.with_suffix('.renamed'))
        before = [self.git(*args) for args in (
            ('status', '--porcelain'), ('diff', '--binary'),
            ('diff', '--cached', '--binary'), ('rev-parse', 'HEAD'))]
        self.run_cli('sync', '--to', str(destination), cwd=source)
        names = self.git('ls-files', '--cached', '--others', '--exclude-standard',
                         '-z', '--', 'scripts').split('\0')
        expected = {}
        for name in names:
            if not name:
                continue
            file = self.local / name
            relative = file.relative_to(source)
            if file.is_symlink():
                expected[relative] = ('link', os.readlink(file))
            elif file.is_file():
                expected[relative] = ('file', file.read_bytes(), file.stat().st_mode & 0o777)
        actual = {}
        for file in destination.rglob('*'):
            relative = file.relative_to(destination)
            if relative.parts[0] == 'cache' or relative == Path('job.output'):
                continue
            if file.is_symlink():
                actual[relative] = ('link', os.readlink(file))
            elif file.is_file():
                actual[relative] = ('file', file.read_bytes(), file.stat().st_mode & 0o777)
        self.assertEqual(actual, expected)
        self.assertFalse((destination / 'obsolete').exists())
        self.assertFalse((destination / 'cache/local-only').exists())
        self.assertEqual((destination / 'cache/results').read_text(), 'preserve results\n')
        self.assertEqual((destination / 'job.output').read_text(), 'preserve log\n')
        self.assertEqual(before, [self.git(*args) for args in (
            ('status', '--porcelain'), ('diff', '--binary'),
            ('diff', '--cached', '--binary'), ('rev-parse', 'HEAD'))])
        result = self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertIn('xfr#0', result.stdout)

    def test_mirror_handles_unstaged_directory_file_and_symlink_replacements(self):
        source = self.local / 'scripts'
        destination = self.root / 'replacement mirror'
        item = source / 'item'
        item.mkdir()
        (item / 'tracked.py').write_text('old code\n')
        self.git('add', 'scripts/item/tracked.py')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        shutil.rmtree(item)
        item.write_text('now a file\n')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertEqual((destination / 'item').read_text(), 'now a file\n')
        item.unlink()
        item.symlink_to('train.sbatch')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertTrue((destination / 'item').is_symlink())
        self.assertEqual(os.readlink(destination / 'item'), 'train.sbatch')
        item.unlink()
        item.mkdir()
        (item / 'new.py').write_text('new code\n')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertFalse((destination / 'item').is_symlink())
        self.assertEqual((destination / 'item/new.py').read_text(), 'new code\n')
        self.assertFalse((destination / 'item/tracked.py').exists())

    def test_mirror_does_not_follow_source_or_destination_symlinks(self):
        source = self.local / 'scripts'
        destination = self.root / 'symlink mirror'
        outside = self.root / 'outside'
        outside.mkdir()
        (outside / 'keep').write_text('outside data\n')
        (source / 'external').symlink_to(outside, target_is_directory=True)
        self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertTrue((destination / 'external').is_symlink())
        self.assertEqual(os.readlink(destination / 'external'), str(outside))
        (destination / 'obsolete-link').symlink_to(outside, target_is_directory=True)
        (destination / 'replacement').symlink_to(outside, target_is_directory=True)
        (source / 'replacement').mkdir()
        (source / 'replacement/new').write_text('inside data\n')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        self.assertFalse((destination / 'obsolete-link').is_symlink())
        self.assertFalse((destination / 'replacement').is_symlink())
        self.assertEqual((destination / 'replacement/new').read_text(), 'inside data\n')
        self.assertEqual({p.name: p.read_text() for p in outside.iterdir()},
                         {'keep': 'outside data\n'})

    def test_mirror_transfer_connection_failure_reports_failure_without_deleting(self):
        destination = self.root / 'failed mirror'
        self.run_cli('sync', '--to', str(destination))
        (destination / 'remote-only').write_text('keep if transfer fails\n')
        before = {p.relative_to(destination): p.read_bytes()
                  for p in destination.rglob('*') if p.is_file()}
        self.tool('ssh', '#!/bin/bash\n[[ ${1:-} != -- ]] || shift\nshift\n'
                  'if [[ ${1:-} == rsync ]]; then echo "simulated connection failure" >&2; exit 255; fi\n'
                  'exec /bin/bash -c "$*"\n')
        result = self.run_cli('sync', '--to', str(destination), check=False)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('simulated connection failure', result.stdout)
        self.assertNotIn('Synced to ', result.stdout)
        self.assertEqual(before, {p.relative_to(destination): p.read_bytes()
                                 for p in destination.rglob('*') if p.is_file()})

    def test_mirror_ignore_check_failure_stops_before_transfer_or_deletion(self):
        destination = self.root / 'ignore failure mirror'
        destination.mkdir()
        (destination / 'remote-only').write_text('keep on error\n')
        self.tool('git', '#!/bin/bash\n'
                  'if [[ ${1:-} == check-ignore || ${2:-} == check-ignore ]]; then echo "simulated Git error" >&2; exit 128; fi\n'
                  'exec /usr/bin/git "$@"\n')
        self.tool('rsync', '#!/bin/bash\necho "RSYNC MUST NOT RUN"; exit 99\n')
        result = self.run_cli('sync', '--to', str(destination), check=False)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('simulated Git error', result.stdout)
        self.assertNotIn('RSYNC MUST NOT RUN', result.stdout)
        self.assertEqual((destination / 'remote-only').read_text(), 'keep on error\n')

    def test_mirror_current_directory_ignores_unrelated_sibling_submodule(self):
        (self.local / 'unrelated-submodule').mkdir()
        self.git('update-index', '--add', '--cacheinfo',
                 f'160000,{self.git("rev-parse", "HEAD")},unrelated-submodule')
        destination = self.root / 'scoped mirror'
        self.run_cli('sync', '--to', str(destination), cwd=self.local / 'scripts')
        self.assertEqual((destination / 'train.sbatch').read_text(), '#!/bin/bash\ntrue\n')
        self.assertFalse((destination / 'unrelated-submodule').exists())

    def test_mirror_source_directory_names_are_literal_git_paths(self):
        for index, name in enumerate((':(literal)project', 'project[1]')):
            with self.subTest(name=name):
                source = self.local / name
                source.mkdir()
                (source / 'code.py').write_text('local code\n')
                destination = self.root / f'literal scope mirror {index}'
                destination.mkdir()
                (destination / 'code.py').write_text('old remote code\n')
                (destination / 'obsolete.py').write_text('remove\n')
                self.run_cli('sync', '--to', str(destination), cwd=source)
                self.assertEqual((destination / 'code.py').read_text(), 'local code\n')
                self.assertFalse((destination / 'obsolete.py').exists())

    def test_mirror_source_read_error_reports_failure_and_skips_deletions(self):
        destination = self.root / 'unreadable source mirror'
        self.run_cli('sync', '--to', str(destination))
        (destination / 'remote-only').write_text('keep if source cannot be read\n')
        script = self.local / 'scripts/train.sbatch'
        script.write_text('unreadable new contents\n')
        script.chmod(0)
        self.addCleanup(script.chmod, 0o644)
        result = self.run_cli('sync', '--to', str(destination), check=False)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertNotIn('Synced to ', result.stdout)
        self.assertEqual((destination / 'remote-only').read_text(),
                         'keep if source cannot be read\n')
        self.assertEqual((destination / 'scripts/train.sbatch').read_text(),
                         '#!/bin/bash\ntrue\n')

    def test_mirror_preserves_outputs_created_after_destination_inspection(self):
        source = self.local / 'scripts'
        (source / '.gitignore').write_text('*.output\n')
        destination = self.root / 'concurrent output mirror'
        self.run_cli('sync', '--to', str(destination), cwd=source)
        (destination / 'obsolete').mkdir()
        (destination / 'obsolete/old.py').write_text('delete old code\n')
        outputs = ['late.output', 'obsolete/late.output', 'new folder/late.output']
        writes = []
        for name in outputs:
            file = destination / name
            writes.append(f'mkdir -p -- {shlex.quote(str(file.parent))}\n'
                          f'printf "new job output\\n" > {shlex.quote(str(file))}\n')
        self.tool('rsync', '#!/bin/bash\nset -eu\n' + ''.join(writes)
                  + 'exec /usr/bin/rsync "$@"\n')
        self.run_cli('sync', '--to', str(destination), cwd=source)
        for name in outputs:
            self.assertTrue((destination / name).is_file(), f'Lost concurrent output: {name}')
            self.assertEqual((destination / name).read_text(), 'new job output\n')
        self.assertFalse((destination / 'obsolete/old.py').exists())

    def test_standalone_sync_requires_destination_before_connecting(self):
        self.tool("ssh", "#!/bin/bash\necho 'SSH MUST NOT RUN'\nexit 99\n")
        result = self.run_cli("sync", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing sync destination. Add --to PATH", result.stdout)
        self.assertNotIn("SSH MUST NOT RUN", result.stdout)

    def test_fetch_downloads_ignored_outputs_and_keeps_local_only_and_remote_files(self):
        source = self.home / "dev/WMF/runs/smoke's run"
        (source / 'checkpoints').mkdir(parents=True)
        (source / 'checkpoints/model.pt').write_bytes(b'model\x00weights')
        (source / 'events.out.tfevents.test').write_bytes(b'tensorboard\x00events')
        (source / 'train.log').write_text('first log\n')
        (source / 'latest').symlink_to('checkpoints/model.pt')
        (source / '.git').mkdir()
        (source / '.git/config').write_text('never import metadata\n')
        (self.local / '.gitignore').write_text('/runs/\n*.pt\n*.log\n')
        destination = self.local / 'runs/downloaded smoke'
        destination.mkdir(parents=True)
        (destination / 'local-notes.txt').write_text('keep local notes\n')
        before_git = [self.git(*args) for args in (
            ('status', '--porcelain'), ('diff', '--binary'),
            ('diff', '--cached', '--binary'), ('rev-parse', 'HEAD'))]
        before_remote = {p.relative_to(source): p.read_bytes()
                         for p in source.rglob('*') if p.is_file() and not p.is_symlink()}
        result = self.run_cli('fetch', '-h', 'test-host', '--from',
                              "~/dev/WMF/runs/smoke's run", '--to', 'runs/downloaded smoke')
        self.assertIn(f'Fetched to {destination}', result.stdout)
        self.assertIn('xfr#', result.stdout)
        self.assertEqual((destination / 'checkpoints/model.pt').read_bytes(), b'model\x00weights')
        self.assertEqual((destination / 'events.out.tfevents.test').read_bytes(), b'tensorboard\x00events')
        self.assertEqual((destination / 'train.log').read_text(), 'first log\n')
        self.assertEqual(os.readlink(destination / 'latest'), 'checkpoints/model.pt')
        self.assertFalse((destination / '.git').exists())
        self.assertEqual(before_remote, {p.relative_to(source): p.read_bytes()
                                        for p in source.rglob('*') if p.is_file() and not p.is_symlink()})
        log = source / 'train.log'
        previous = log.stat()
        log.write_text('later log\n')
        os.utime(log, ns=(previous.st_atime_ns, previous.st_mtime_ns))
        (source / 'checkpoints/model.pt').unlink()
        self.run_cli('fetch', '--from', str(source), '--to', str(destination))
        self.assertEqual((destination / 'train.log').read_text(), 'later log\n')
        self.assertEqual((destination / 'checkpoints/model.pt').read_bytes(), b'model\x00weights')
        self.assertEqual((destination / 'local-notes.txt').read_text(), 'keep local notes\n')
        self.assertEqual(before_git, [self.git(*args) for args in (
            ('status', '--porcelain'), ('diff', '--binary'),
            ('diff', '--cached', '--binary'), ('rev-parse', 'HEAD'))])
        self.assertFalse(self.marker.exists())

    def test_fetch_dry_run_does_not_create_or_change_local_project_files(self):
        source = self.home / 'remote runs'
        source.mkdir()
        (source / 'train.log').write_text('remote output\n')
        destination = self.root / 'missing/parents/local runs'
        result = self.run_cli('fetch', '--from', 'remote runs', '--to', str(destination),
                              '--dry-run', cwd=self.root)
        self.assertIn('train.log', result.stdout)
        self.assertIn(f'Would fetch to {destination}', result.stdout)
        self.assertFalse((self.root / 'missing').exists())
        destination.mkdir(parents=True)
        (destination / 'train.log').write_text('old local output\n')
        (destination / 'local-only').write_text('keep\n')
        before = {p.name: p.read_bytes() for p in destination.iterdir()}
        result = self.run_cli('fetch', '--from', str(source), '--to', str(destination), '--dry-run')
        self.assertIn('train.log', result.stdout)
        self.assertNotIn('*deleting', result.stdout)
        self.assertEqual(before, {p.name: p.read_bytes() for p in destination.iterdir()})

    def test_fetch_requires_explicit_source_and_destination_before_connecting(self):
        self.tool('ssh', '#!/bin/bash\necho "SSH MUST NOT RUN"; exit 99\n')
        for args, expected in (
            (('fetch', '--to', 'runs'), 'Missing fetch source'),
            (('fetch', '--from', '~/remote/runs'), 'Missing fetch destination'),
        ):
            with self.subTest(args=args):
                result = self.run_cli(*args, check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn(expected, result.stdout)
                self.assertNotIn('SSH MUST NOT RUN', result.stdout)

    def test_fetch_missing_or_non_directory_source_does_not_create_local_destination(self):
        for source in ('~/missing/runs', str(self.checkout / 'scripts/train.sbatch')):
            with self.subTest(source=source):
                destination = self.root / 'no downloads'
                result = self.run_cli('fetch', '--from', source, '--to', str(destination), check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn('Fetch source', result.stdout)
                self.assertFalse(destination.exists())
                self.assertNotIn('Fetched to ', result.stdout)
        self.assertFalse((self.home / 'missing').exists())

    def test_fetch_local_mode_works_outside_git_and_rejects_overlapping_directories(self):
        self.tool('ssh', '#!/bin/bash\necho "SSH MUST NOT RUN"; exit 99\n')
        source = self.root / 'local run'
        source.mkdir()
        (source / 'model.pt').write_bytes(b'checkpoint')
        destination = self.root / 'downloaded run'
        self.run_cli('--local', 'fetch', '--from', str(source), '--to', str(destination), cwd=self.root)
        self.assertEqual((destination / 'model.pt').read_bytes(), b'checkpoint')
        for target in (source, source / 'nested', self.root):
            with self.subTest(target=target):
                result = self.run_cli('--local', 'fetch', '--from', str(source),
                                      '--to', str(target), check=False, cwd=self.root)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn('cannot contain one another', result.stdout)
        self.assertFalse((source / 'nested').exists())

    def test_fetch_rejects_local_file_and_git_metadata_destinations(self):
        destination = self.root / 'existing file'
        destination.write_text('keep\n')
        self.tool('ssh', '#!/bin/bash\necho "SSH MUST NOT RUN"; exit 99\n')
        for target in (destination, self.local / '.git/downloads'):
            with self.subTest(target=target):
                result = self.run_cli('fetch', '--from', '~/remote/runs',
                                      '--to', str(target), check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertNotIn('SSH MUST NOT RUN', result.stdout)
        self.assertEqual(destination.read_text(), 'keep\n')
        self.assertFalse((self.local / '.git/downloads').exists())

    def test_fetch_connection_failure_does_not_claim_success_or_delete_local_files(self):
        source = self.home / 'remote run'
        source.mkdir()
        (source / 'model.pt').write_bytes(b'checkpoint')
        destination = self.root / 'downloads'
        destination.mkdir()
        (destination / 'notes').write_text('keep\n')
        self.tool('ssh', '#!/bin/bash\n[[ ${1:-} != -- ]] || shift\nshift\n'
                  'if [[ ${1:-} == rsync ]]; then echo "simulated connection failure" >&2; exit 255; fi\n'
                  'exec /bin/bash -c "$*"\n')
        result = self.run_cli('fetch', '--from', str(source), '--to', str(destination), check=False)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('simulated connection failure', result.stdout)
        self.assertNotIn('Fetched to ', result.stdout)
        self.assertEqual(list(destination.iterdir()), [destination / 'notes'])
        self.assertEqual((destination / 'notes').read_text(), 'keep\n')

    def test_sync_relative_and_tilde_destinations_are_relative_to_target_home(self):
        for destination in ("dev/project", "~/other project"):
            with self.subTest(destination=destination):
                result = self.run_cli("sync", "--to", destination)
                target = self.home / destination.removeprefix("~/")
                self.assertIn(f"Synced to test-host:{target}", result.stdout)
                self.assertTrue((target / "scripts/train.sbatch").is_file())

    def test_sync_needs_no_pairing_and_keeps_each_submission_separate(self):
        self.git("config", "--unset", "cluster.test-host.path")
        self.git("remote", "remove", "origin")
        script = self.local / "scripts/train.sbatch"
        script.write_text("first snapshot\n")
        self.run_cli("submit", "--sync", "--", "scripts/train.sbatch")
        first = Path(json.loads(self.marker.read_text())["cwd"])
        script.write_text("second snapshot\n")
        self.run_cli("submit", "--sync", "--", "scripts/train.sbatch")
        second = Path(json.loads(self.marker.read_text())["cwd"])
        self.assertNotEqual(first, second)
        self.assertEqual((first / "scripts/train.sbatch").read_text(), "first snapshot\n")
        self.assertEqual((second / "scripts/train.sbatch").read_text(), "second snapshot\n")

    def test_sync_dry_run_does_not_connect_upload_or_submit(self):
        (self.local / "scripts/train.sbatch").write_text("local change\n")
        self.git("config", "--unset", "cluster.test-host.path")
        self.tool("ssh", "#!/bin/bash\necho 'SSH MUST NOT RUN'\nexit 99\n")
        result = self.run_cli("submit", "--sync", "--dry-run", "--", "train.sbatch",
                              cwd=self.local / "scripts")
        self.assertIn("Would sync 1 files", result.stdout)
        self.assertIn("<new snapshot>/source/scripts", result.stdout)
        self.assertIn("no project files uploaded", result.stdout)
        self.assertIn("no job submitted", result.stdout)
        self.assertFalse(self.marker.exists())
        self.assertFalse((self.home / ".local/share/cluster/runs").exists())

    def test_sync_failed_upload_never_calls_sbatch(self):
        self.tool("ssh", '#!/bin/bash\n[[ ${!#} != "exec tar "* ]] || exit 99\nexec /bin/bash -c "${!#}"\n')
        result = self.run_cli("submit", "--sync", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Snapshot upload failed", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_sync_local_mode_uses_a_snapshot_without_ssh(self):
        self.tool("ssh", "#!/bin/bash\nexit 99\n")
        (self.local / "scripts/train.sbatch").write_text("local change\n")
        self.run_cli("--local", "submit", "--sync", "scripts/train.sbatch")
        source = Path(json.loads(self.marker.read_text())["cwd"])
        self.assertNotEqual(source, self.local)
        self.assertEqual((source / "scripts/train.sbatch").read_text(), "local change\n")

    def test_snapshot_protocol_rejects_paths_outside_run_directory(self):
        result = self.run_cli("agent", "submit", json.dumps({
            "path": str(self.checkout), "subdir": "", "snapshot": True,
            "origin": "github.com/example/project", "commit": self.git("rev-parse", "HEAD"),
            "arguments": ["scripts/train.sbatch"], "env": {}, "dry_run": False,
        }), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the cluster run directory", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_logs_follow_custom_paths_and_remain_available_after_accounting_purge(self):
        self.run_cli("submit", "scripts/train.sbatch")
        self.tool("scontrol", "#!/bin/bash\necho 'Invalid job id' >&2\nexit 1\n")
        result = self.run_cli("logs", "456")
        self.assertIn("training output", result.stdout)
        result = self.run_cli("logs", "456", "--stderr")
        self.assertIn("training error", result.stdout)

    def test_logs_query_jobs_submitted_elsewhere(self):
        result = self.run_cli("logs", "456")
        self.assertIn("training output", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_successful_submission_is_reported_when_log_lookup_fails(self):
        self.tool("scontrol", "#!/bin/bash\necho 'controller unavailable' >&2\nexit 1\n")
        result = self.run_cli("submit", "scripts/train.sbatch")
        self.assertTrue(self.marker.exists())
        self.assertIn("Submitted job 456", result.stdout)
        self.assertIn("was submitted, but log lookup failed", result.stdout)

    def test_follow_waits_for_log_creation_and_passes_the_actual_path(self):
        marker = self.root / "tail-arguments.json"
        self.tool("tail", "#!/usr/bin/env python3\nimport json,pathlib,sys\n"
                  + "pathlib.Path(" + repr(str(marker)) + ").write_text(json.dumps(sys.argv[1:]))\n")
        result = self.run_cli("submit", "--follow", "scripts/train.sbatch")
        self.assertEqual(json.loads(marker.read_text()), ["-n", "80", "-F", "--", str(self.stdout)])
        self.assertIn("the job keeps running", result.stdout)

    def test_quiet_and_bad_job_ids_fail_without_submission(self):
        self.assertNotEqual(self.run_cli("submit", "--", "--quiet", "scripts/train.sbatch", check=False).returncode, 0)
        self.assertNotEqual(self.run_cli("logs", "../456", check=False).returncode, 0)
        self.assertFalse(self.marker.exists())

    def test_array_patterns_and_width(self):
        path = self.root / "train-123_0002.out"
        path.write_text("array task output\n")
        record = f"JobId=123_2 ArrayJobId=123 ArrayTaskId=2 JobName=train StdOut={self.root}/%x-%A_%04a.out"
        self.tool("scontrol", "#!/usr/bin/env python3\nprint(" + repr(record) + ")\n")
        result = self.run_cli("logs", "123_2")
        self.assertIn("array task output", result.stdout)

    def test_jobs_can_run_without_repository(self):
        self.tool("squeue", "#!/bin/bash\nprintf 'JOBID NAME STATE\\n'\n")
        result = self.run_cli("--host", "test-host", "jobs", cwd=self.root)
        self.assertIn("JOBID NAME STATE", result.stdout)

    def test_local_submission_needs_no_ssh_or_pairing(self):
        self.tool("ssh", "#!/bin/bash\nexit 99\n")
        result = self.run_cli("--local", "submit", "--dry-run", "scripts/train.sbatch")
        self.assertIn("no job submitted", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_ambiguous_scheduler_output_is_not_retried(self):
        count = self.root / "submit-count"
        self.tool("sbatch", "#!/bin/bash\nprintf 'submitted\\n' >> " + shlex.quote(str(count)) + "\nprintf 'unexpected scheduler response\\n'\n")
        result = self.run_cli("submit", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("check the queue before retrying", result.stdout)
        self.assertEqual(count.read_text().splitlines(), ["submitted"])

    def test_lost_ssh_after_submission_does_not_submit_twice(self):
        count = self.root / "submit-count"
        self.tool("sbatch", "#!/bin/bash\nprintf 'submitted\\n' >> " + shlex.quote(str(count)) + "\nprintf '456\\n'\n")
        self.tool("ssh", '#!/bin/bash\n/bin/bash -c "${!#}"\nstatus=$?\n[[ ${!#} != *" agent submit "* ]] || exit 255\nexit "$status"\n')
        result = self.run_cli("submit", "scripts/train.sbatch", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no automatic retry", result.stdout)
        self.assertEqual(count.read_text().splitlines(), ["submitted"])

    def test_remote_native_cache_is_reused(self):
        agents = self.root / "cache/cluster/agents"
        before = {p.name: p.stat().st_mtime_ns for p in agents.iterdir()}
        self.run_cli("submit", "--dry-run", "scripts/train.sbatch")
        self.assertEqual(before, {p.name: p.stat().st_mtime_ns for p in agents.iterdir()})
        self.assertFalse(list(agents.glob("incoming.*")))

    def test_remote_protocol_rejects_working_directory_escape(self):
        result = self.run_cli("agent", "submit", json.dumps({
            "path": str(self.checkout), "subdir": str(self.root),
            "origin": "github.com/example/project", "commit": self.git("rev-parse", "HEAD"),
            "arguments": ["scripts/train.sbatch"], "env": {}, "dry_run": False,
        }), check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("escapes the paired checkout", result.stdout)
        self.assertFalse(self.marker.exists())

    def test_log_cache_from_python_version_is_still_readable(self):
        key = hashlib.sha256(b"test-host").hexdigest()[:16]
        path = self.root / "state/cluster/jobs" / (key + "-456.json")
        path.parent.mkdir(parents=True)
        path.write_text(json.dumps({"host": "test-host", "job": "456", "info": {
            "StdOut": str(self.stdout), "StdErr": str(self.stderr),
        }}) + "\n")
        self.tool("scontrol", "#!/bin/bash\necho 'Job accounting unavailable' >&2\nexit 1\n")
        result = self.run_cli("logs", "456")
        self.assertIn("training output", result.stdout)
        self.assertNotIn("Job accounting unavailable", result.stdout)


if __name__ == "__main__":
    unittest.main()
