"""Test against Git checkouts and fake Slurm commands, without submitting jobs."""

import ast
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / ".local/bin/cluster"
CLI = runpy.run_path(str(SCRIPT))


class JobTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="cluster-jobs-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = self.root / "local repo"
        self.checkout = self.root / "remote checkout's name"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=str(self.bin) + ":/usr/bin:/bin",
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
        self.tool("ssh", '#!/bin/bash\nexec /bin/bash -c "${!#}"\n')
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

    def run_cli(self, *args, check=True, cwd=None):
        result = subprocess.run([sys.executable, str(SCRIPT)] + list(args),
                                cwd=str(cwd or self.local), env=self.env,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                timeout=15)
        if check:
            self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def test_python_36_syntax(self):
        ast.parse(SCRIPT.read_text(), feature_version=(3, 6))
        ast.parse(CLI["REMOTE"], feature_version=(3, 6))

    def test_link_saves_remote_root_independently_of_local_name(self):
        self.assertEqual(self.git("config", "--get", "cluster.test-host.path"), str(self.checkout))
        self.assertEqual(self.git("config", "--get", "cluster.host"), "test-host")
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
        self.assertEqual(self.git("config", "--get", "cluster.host"), "test-host")

    def test_dry_run_preserves_subdirectory_and_never_calls_sbatch(self):
        result = self.run_cli("submit", "--dry-run", "--", "--time=00:05:00", "train.sbatch", cwd=self.local / "scripts")
        self.assertIn(str(self.checkout / "scripts").replace("'", "'\"'\"'"), result.stdout)
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
        info = {"StdOut": "/logs/%x-%A_%04a.out", "JobName": "train", "ArrayJobId": "123", "ArrayTaskId": "2"}
        self.assertEqual(CLI["log_path"](info, "123_2"), "/logs/train-123_0002.out")
        info["ArrayTaskId"] = "0-10"
        with self.assertRaises(CLI["Error"]):
            CLI["log_path"](info, "123")

    def test_jobs_can_run_without_repository(self):
        self.tool("squeue", "#!/bin/bash\nprintf 'JOBID NAME STATE\\n'\n")
        result = self.run_cli("--host", "test-host", "jobs", cwd=self.root)
        self.assertIn("JOBID NAME STATE", result.stdout)

    def test_local_submission_needs_no_ssh_or_pairing(self):
        self.tool("ssh", "#!/bin/bash\nexit 99\n")
        result = self.run_cli("--local", "submit", "--dry-run", "scripts/train.sbatch")
        self.assertIn("no job submitted", result.stdout)
        self.assertFalse(self.marker.exists())


if __name__ == "__main__":
    unittest.main()
