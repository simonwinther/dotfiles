# Cluster tools

Use `cluster --help` to see the setup, link, submit, jobs, and logs commands.
Each command also has its own help, such as `cluster setup --help`.

## Submit from your PC

Pair a local Git checkout with its existing cluster checkout once. The paths
can differ; for example, from local `~/dev/bratz2026-dev`:

```sh
cluster link hendrix dev/bratz2026
```

The remote path is relative to the remote home directory, or you can provide
an absolute path. Quote a `~/...` path so your local shell does not expand it.
The helper verifies that the checkouts have the same Git
origin and saves the pairing in the local repository's `.git/config`.
`cluster link --check hendrix dev/bratz2026` only validates it. Each checkout
can have a separate pairing for each SSH alias. The last saved pairing becomes
that repository's default; `--host` selects a different one.

If you submit without a saved link, the command exits with a `cluster link`
instruction before connecting or submitting anything. A missing checkout,
different Git origin, or mismatched commit also stops submission with an
explanation.

GitHub remains the code-sync workflow: commit and push locally, then pull in
the cluster checkout. Submission requires matching commits and no uncommitted
tracked changes on either side. Untracked files and local environment
variables are not copied. The helper preserves the current repository
subdirectory, so scripts see the same relative paths on both machines.

From the directory where you would normally run `sbatch`:

```sh
cluster submit --dry-run -- slurm/train.sbatch
cluster submit --follow -- slurm/train.sbatch
cluster submit --env RUN_NAME=experiment-1 -- --gres=gpu:a100:1 slurm/train.sbatch
cluster jobs
cluster logs 12345 --follow
cluster logs 12345 --stderr
```

Put helper options before `--`; everything after it is passed to `sbatch`.
The dry run validates both checkouts and prints the command without submitting
a job. `--follow` streams stdout, including waiting for a queued job's log to
appear. Ctrl+C stops following and leaves the job running. Output and error
paths come from Slurm, preserving your script's `#SBATCH --output` and
`--error` directives. Log paths are cached locally for jobs submitted through
the helper, so they remain readable after Slurm removes the job record.
For arrays, select a task: `cluster logs 12345_2 --follow`. Node-specific
output filename patterns are not supported.

Use `cluster --host mostafa ...` to select another paired cluster. When already
on a cluster, `cluster --local submit -- slurm/train.sbatch` uses the current
checkout directly. It requires Python 3.6+ and the usual Git and Slurm tools.
Resources, accounts, environments, project launchers, and dependencies remain
in your existing batch scripts. The helper does not run project-specific
submission wrappers; invoke those through their documented workflow.

Hendrix's existing live monitor can run directly in your local terminal:

```sh
ssh -t hendrix mytop
ssh hendrix 'mytop --once'
```

## Install terminal configurations

From the PC with this dotfiles checkout:

```sh
cluster setup --check my-cluster
cluster setup my-cluster
```

Use any SSH alias or `user@hostname`. SSH config settings, jump hosts, and
authentication work as usual. For a custom SSH port, use
`cluster setup -p 2222 user@hostname`. Run the same setup command again to send
your current configs. It transfers only tracked Neovim and tmux files and the
cluster commands, including local edits to those tracked configs.

On another PC, install the command from this checkout with `stow cluster`.
You can also run `./cluster/.local/bin/cluster setup my-cluster` directly. If
the checkout is already on a cluster, run `cluster setup --local`, or
`./cluster/.local/bin/cluster setup --local` before stowing it. The old
`cluster-setup` entry point remains available for compatibility.

The check reports Neovim, tmux, Git, ripgrep, fd, fzf, tree-sitter CLI, curl,
unzip, Python, Stow, and a C compiler. **`--check` changes no files and downloads
nothing.** Setup reuses suitable tools and installs missing or outdated ones
in `~/.local/share/cluster-setup/env` using
[micromamba](https://mamba.readthedocs.io/en/latest/installation/micromamba-installation.html)
and conda-forge. GNU Stow is installed from its source release if missing.
No sudo or system package changes are needed. Linux x86_64 and ARM64 are
supported; bootstrap downloads require curl or wget, tar, gzip, and bzip2,
plus outbound access to GitHub, conda-forge, micro.mamba.pm, and ftp.gnu.org.

The minimum Neovim version is 0.12 because the Treesitter revision in your
current lockfile requires it. tmux must be at least 3.2. Dependencies for
language servers and individual language toolchains remain project specific.

Setup copies the three packages to a private state directory and applies
them with GNU Stow. Existing configs, conflicting launchers, and modified
shell startup files are backed up under
`~/.local/share/cluster-setup/backups/`. Only `~/.local/bin` is added to Bash
and Zsh startup files; the tools environment is never activated in your
shell. Reconnect after the first setup. Existing tmux servers keep their
loaded settings; reload with `tmux source-file ~/.tmux.conf` when ready,
or use `tmux -L cluster new` to start a separate server with the new config.

Setup also detects an incompatible older server on the default socket and
prints the command for a separate server. If its old sessions are no longer
needed, the matching system binary can stop it with
`/usr/bin/tmux -L default kill-server`. This closes all its sessions and their
programs. The explicit socket also makes the command safe to run from inside
the separate `cluster` server.

Some SSH aliases distribute logins across several gateway nodes. tmux servers
belong to one node, so reconnect to that same node to reattach to a session;
the configuration and tools can still live in the shared home directory.

Your terminal's terminfo entry and `tmux-256color` are installed when `tic`
is available on the target. Fonts stay on the PC running your terminal.
tmux plugins are cloned during setup; `--skip-plugins` skips this step.
Neovim downloads plugins on its first interactive launch using your
`lazy-lock.json`. Credentials and desktop packages are not transferred.
Log in to Copilot separately if you use it on that machine.

For a disposable machine, keep the home directory on persistent storage if
you want tools and downloaded plugins to survive its replacement, or rerun
setup when recreating it. A remote home directory shared by login and
compute nodes only needs setup once;
those nodes must have compatible operating systems and CPU architectures.
Edits to the installed snapshot are replaced by the next sync; edit the
original dotfiles checkout on your PC instead.

Run the scratch-home and fake-scheduler checks with
`python -m unittest discover -s cluster/tests -v`. They require GNU Stow and
do not connect to an SSH host or submit real jobs.
