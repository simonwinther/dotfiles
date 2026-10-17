"""Exercise setup against scratch homes; never modify the real home directory."""

import os
from pathlib import Path
import shutil
import shlex
import subprocess
import sys
import tarfile
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / ".local/bin/cluster-setup"
CLI = SCRIPT.with_name("cluster")


class SetupTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cluster-setup-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "target with ' quotes"
        self.source = self.root / "source"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, CLUSTER_SETUP_HOME=str(self.home))
        self.env.pop("XDG_CONFIG_HOME", None)
        self.env["PATH"] = f"{self.bin}:/usr/bin:/bin"
        for command, version in {
            "nvim": "NVIM v0.12.5",
            "tmux": "tmux 3.7c",
            "tree-sitter": "tree-sitter 0.26.9",
            "rg": "ripgrep 15.2.0",
            "fd": "fd 10.5.0",
            "fzf": "0.74.3",
        }.items():
            self.tool(command, f"printf '%s\\n' '{version}'\n")
        for relative, content in {
            "nvim/.config/nvim/init.lua": "-- test config\n",
            "tmux/.tmux.conf": "set -g mouse on\n",
            "tmux/.tmux.conf.custom": "# other config\n",
        }.items():
            path = self.source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        destination = self.source / "cluster/.local/bin/cluster-setup"
        destination.parent.mkdir(parents=True)
        shutil.copy2(SCRIPT, destination)

    def tool(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/bash\n" + body)
        path.chmod(0o755)

    def run_setup(self, *args, check=True, stdin=None):
        result = subprocess.run(
            ["bash", str(SCRIPT), *args],
            env=self.env,
            text=True,
            input=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=30,
        )
        if check:
            self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def run_cli(self, *args, check=True):
        result = subprocess.run(
            [sys.executable, str(CLI), *args], env=self.env, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30,
        )
        if check:
            self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def install(self):
        return self.run_setup(
            "--local", "--skip-plugins", "--source", str(self.source)
        )

    @property
    def state(self):
        return self.home / ".local/share/cluster-setup"

    def test_check_creates_nothing(self):
        result = self.run_setup("--local", "--check")
        self.assertIn("Check complete. No files changed.", result.stdout)
        self.assertNotIn("missing", result.stdout)
        self.assertFalse(self.home.exists())

    def test_cli_setup_local_check_creates_nothing(self):
        result = self.run_cli("setup", "--local", "--check")
        self.assertIn("Check complete. No files changed.", result.stdout)
        self.assertFalse(self.home.exists())

    def test_cli_setup_remote_check_with_global_host(self):
        self.tool("ssh", 'exec /bin/bash -c "${!#}"\n')
        result = self.run_cli("--host", "test-host", "setup", "--check")
        self.assertIn("Check complete. No files changed.", result.stdout)
        self.assertFalse(self.home.exists())

    def test_cli_setup_accepts_global_local_flag(self):
        result = self.run_cli("--local", "setup", "--check")
        self.assertIn("Check complete. No files changed.", result.stdout)
        self.assertFalse(self.home.exists())

    def test_cli_setup_remote_install_and_installed_local_command(self):
        self.tool("ssh", 'exec /bin/bash -c "${!#}"\n')
        self.run_cli("setup", "--skip-plugins", "-p", "2222", "test-host")
        command = self.home / ".local/bin/cluster"
        self.assertTrue(command.is_symlink())
        result = subprocess.run(
            [str(command), "setup", "--local", "--skip-plugins"],
            env=self.env, text=True, capture_output=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_cli_setup_invalid_targets_fail_before_changes(self):
        for args in (("setup",), ("setup", "--local", "test-host"),
                     ("setup", "--local", "-p", "22"),
                     ("--host", "one", "setup", "two")):
            with self.subTest(args=args):
                result = self.run_cli(*args, check=False)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertNotIn("Traceback", result.stdout)
        self.assertFalse(self.home.exists())

    def test_old_versions_are_reported(self):
        self.tool("nvim", "echo 'NVIM v0.11.5'\n")
        self.tool("tmux", "echo 'tmux 3.1c'\n")
        result = self.run_setup("--local", "--check")
        self.assertIn("need >= 0.12.0", result.stdout)
        self.assertIn("need >= 3.2", result.stdout)
        self.assertFalse(self.home.exists())

    def test_check_reports_incompatible_existing_tmux_server(self):
        self.tool("tmux", 'if [[ $1 == -V ]]; then echo "tmux 3.8"; else echo "server version is too old for client" >&2; exit 1; fi\n')
        result = self.run_setup("--local", "--check")
        self.assertIn("incompatible older server on the default socket", result.stdout)
        self.assertFalse(self.home.exists())

    def test_install_preserves_old_server_and_prints_separate_server_command(self):
        self.tool("tmux", 'if [[ $1 == -V ]]; then echo "tmux 3.8"; elif [[ $* == "-L default list-sessions" ]]; then echo "server version is too old for client" >&2; exit 1; else exit 99; fi\n')
        result = self.install()
        self.assertIn("tmux -L cluster new-session -A -s main", result.stdout)
        self.assertIn("existing sessions kept running", result.stdout)

    def test_installs_with_stow_and_backs_up_configs(self):
        for relative in (".config/nvim/init.lua", ".tmux.conf", ".config/tmux/tmux.conf"):
            path = self.home / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("original " + relative)
        for rc in (".bashrc", ".profile", ".zshrc", ".bash_profile"):
            (self.home / rc).write_text("# original startup\n")
        self.install()
        self.assertTrue((self.home / ".config/nvim").is_symlink())
        self.assertTrue((self.home / ".tmux.conf").is_symlink())
        self.assertTrue((self.home / ".local/bin/cluster-setup").is_symlink())
        self.assertFalse((self.home / ".config/tmux/tmux.conf").exists())
        backup = next((self.state / "backups").iterdir())
        self.assertEqual((backup / ".tmux.conf").read_text(), "original .tmux.conf")
        self.assertEqual((backup / ".config/nvim/init.lua").read_text(), "original .config/nvim/init.lua")
        self.assertEqual((backup / ".bash_profile").read_text(), "# original startup\n")
        for rc in (".bashrc", ".profile", ".zshrc", ".bash_profile"):
            self.assertEqual((self.home / rc).read_text().count("# cluster-setup"), 1)

    def test_repeat_sync_replaces_removed_files_and_preserves_startup(self):
        old = self.source / "nvim/.config/nvim/old.lua"
        old.write_text("-- obsolete\n")
        self.install()
        old.unlink()
        (self.source / "nvim/.config/nvim/init.lua").write_text("-- updated\n")
        self.install()
        self.assertFalse((self.home / ".config/nvim/old.lua").exists())
        self.assertEqual((self.home / ".config/nvim/init.lua").read_text(), "-- updated\n")
        for rc in (".bashrc", ".profile", ".zshrc"):
            self.assertEqual((self.home / rc).read_text().count("# cluster-setup"), 1)
        self.assertEqual(len(list((self.state / "backups").glob("*/.config/nvim"))), 0)

    def test_startup_puts_launchers_before_old_system_tools(self):
        self.install()
        result = subprocess.run(
            ["bash", "-c", 'PATH="/usr/bin:$HOME/.local/bin"; . "$1"; printf "%s" "$PATH"',
             "bash", str(self.home / ".bashrc")],
            env=self.env, text=True, capture_output=True, timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(result.stdout.startswith(os.environ["HOME"] + "/.local/bin:"))

    def test_installed_command_can_run_local_again(self):
        self.install()
        result = subprocess.run(
            [str(self.home / ".local/bin/cluster-setup"), "--local", "--skip-plugins"],
            env=self.env, text=True, capture_output=True, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_remote_check_accepts_stdin_script(self):
        self.tool("ssh", 'exec /bin/bash -c "${!#}"\n')
        result = self.run_setup("--check", "test-host")
        self.assertIn("Check complete. No files changed.", result.stdout)
        self.assertFalse(self.home.exists())

    def test_remote_bundle_contains_only_selected_files(self):
        archive = self.root / "payload.tar"
        self.tool("ssh", f"cat > {shlex.quote(str(archive))}\n")
        self.run_setup("--skip-plugins", "test-host")
        with tarfile.open(archive) as payload:
            names = payload.getnames()
        self.assertIn("nvim/.config/nvim/init.lua", names)
        self.assertIn("tmux/.tmux.conf", names)
        self.assertIn("cluster/.local/bin/cluster-setup", names)
        self.assertIn("cluster/.local/bin/cluster", names)
        self.assertTrue(all(
            name.startswith(("nvim/", "tmux/")) or name in (
                "cluster/.local/bin/cluster-setup", "cluster/.local/bin/cluster", "terminal.info"
            ) for name in names
        ))
        self.assertFalse(any(name.endswith("AGENTS.md") for name in names))
        self.assertFalse(self.home.exists())

    def test_remote_install_transport_and_cleanup(self):
        self.tool("ssh", 'exec /bin/bash -c "${!#}"\n')
        self.run_setup("--skip-plugins", "-p", "2222", "test-host")
        self.assertTrue((self.home / ".tmux.conf").is_symlink())
        self.assertTrue((self.home / ".config/nvim").is_symlink())
        self.assertTrue((self.home / ".local/bin/cluster").is_symlink())
        self.assertEqual(list(self.state.glob("incoming.*")), [])

    def test_failed_stow_restores_original_configs(self):
        original = self.home / ".config/nvim/init.lua"
        original.parent.mkdir(parents=True)
        original.write_text("-- original\n")
        self.tool("stow", '[[ $1 == --version ]] && exit 0\nexit 1\n')
        result = self.run_setup(
            "--local", "--skip-plugins", "--source", str(self.source), check=False
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("original configurations restored", result.stdout)
        self.assertEqual(original.read_text(), "-- original\n")
        self.assertFalse(original.parent.is_symlink())

    def test_failed_resync_restores_previous_snapshot(self):
        self.install()
        (self.source / "nvim/.config/nvim/init.lua").write_text("-- changed\n")
        self.tool("stow", '[[ $1 == --version ]] && exit 0\nexit 1\n')
        result = self.run_setup(
            "--local", "--skip-plugins", "--source", str(self.source), check=False
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.home / ".config/nvim/init.lua").read_text(), "-- test config\n")
        self.assertTrue((self.home / ".config/nvim").is_symlink())

    def test_linked_parent_is_rejected_before_changes(self):
        # Stow creates a folded directory symlink for this fixture.
        package_root = self.root / "parent-packages"
        config = package_root / "parent/.config/sentinel"
        config.parent.mkdir(parents=True)
        config.write_text("keep me\n")
        self.home.mkdir()
        subprocess.run(
            ["stow", "-d", str(package_root), "-t", str(self.home), "parent"],
            check=True, capture_output=True,
        )
        self.assertTrue((self.home / ".config").is_symlink())
        result = self.run_setup(
            "--local", "--skip-plugins", "--source", str(self.source), check=False
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("real directory", result.stdout)
        self.assertEqual(config.read_text(), "keep me\n")
        self.assertFalse(self.state.exists())

    def test_invalid_arguments(self):
        for args in ([], ["--local", "test-host"], ["--unknown"], ["-p"], ["-p", "oops", "host"]):
            with self.subTest(args=args):
                self.assertNotEqual(self.run_setup(*args, check=False).returncode, 0)
        self.assertFalse(self.home.exists())


if __name__ == "__main__":
    unittest.main()
