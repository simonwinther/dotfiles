# Cluster tools

Use `cluster --help` to see the setup, link, sync, fetch, submit, jobs, and logs commands.
Each command also has its own help, such as `cluster setup --help`.

Remote operations require an explicit host every time: `cluster sync -h hendrix`
or `cluster -h hendrix sync`. `--host` is equivalent to `-h`, and can also go
before or after the subcommand. `--help` displays help. No host is chosen from
defaults or saved repository metadata; `--local` explicitly selects this machine.

The CLI, remote Git/Slurm operations, and setup workflow run in Rust. The
`cluster` and `cluster-setup` commands are small shell launchers. Neither
Python nor Rust needs to be installed on the SSH host to run cluster commands.

## Build on your PC

Run `stow cluster` and `cluster-build` from this checkout on each PC. Building
requires Linux, Rust/Cargo 1.88+, a C linker with static libc libraries, and GNU
binutils (`readelf`). The default build uses the PC's architecture and checks
that the executable has no dynamic loader or shared-library dependencies.
Source lives in `.local/share/cluster`; executables, Cargo's lockfile, and build
artifacts stay in `${XDG_CACHE_HOME:-~/.cache}/cluster` outside the repository.
Run `cluster-build` again after source changes.

SSH operations cache the executable by its SHA-256 hash in
`${XDG_CACHE_HOME:-~/.cache}/cluster/agents` on the remote host. The first
lookup uploads it atomically; later commands reuse it. Git paths, job
arguments, and results use JSON. Existing Git pairings and saved job log
paths from the Python version remain compatible.

The PC and SSH host must have compatible Linux kernels and CPU architectures.
Linux x86_64 and ARM64 builds are supported; this rewrite was tested on
x86_64. For a host with a different CPU, set `CLUSTER_REMOTE_BINARY` to a static
`cluster-native` executable built for that host. `CLUSTER_BUILD_TARGET` selects
a Rust target when the necessary target libraries and linker are installed;
cross builds produce a separate executable in the cache's `bin` directory and
keep the PC's active executable. Run tests on the target architecture.

`cluster setup --check HOST` uses a read-only shell probe over SSH and does
not upload an executable, create a remote cache, or download tools.

## Submit from your PC

For submissions from committed code, pair a local Git checkout with its
existing cluster checkout once. The paths can differ; for example, from local
`~/dev/bratz2026-dev`:

```sh
cluster link hendrix dev/bratz2026
```

The remote path is relative to the remote home directory, or you can provide
an absolute path. Quote a `~/...` path so your local shell does not expand it.
The helper verifies that the checkouts have the same Git
origin and saves the pairing in the local repository's `.git/config`.
`cluster link --check hendrix dev/bratz2026` only validates it. Each checkout
can have a separate pairing for each SSH alias. Pass `-h HOST` to select a
pairing for submission; linking does not select a default host.

If you submit without `--sync` and without a saved link, the command exits
with a `cluster link` instruction before connecting or submitting anything. A missing checkout,
different Git origin, or mismatched commit also stops submission with an
explanation.

By default, code sync uses GitHub: commit and push locally, then pull in
the cluster checkout. Submission requires matching commits and no uncommitted
tracked changes on either side. Untracked files and local environment
variables are not copied. The helper preserves the current repository
subdirectory, so scripts see the same relative paths on both machines.

From the directory where you would normally run `sbatch`:

```sh
cluster -h hendrix submit --dry-run -- slurm/train.sbatch
cluster -h hendrix submit --follow -- slurm/train.sbatch
cluster -h hendrix submit --env RUN_NAME=experiment-1 -- --gres=gpu:a100:1 slurm/train.sbatch
cluster jobs -h hendrix
cluster logs -h hendrix 12345 --follow
cluster logs -h hendrix 12345 --stderr
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

Submission runs `sbatch` once. If SSH disconnects or its response is unclear,
check `cluster jobs -h HOST` before retrying: the scheduler may have accepted the job.

Use `cluster --host mostafa ...` to select another paired cluster. When already
on a cluster, `cluster --local submit -- slurm/train.sbatch` uses the current
checkout directly. It requires the usual Git and Slurm tools.
Resources, accounts, environments, project launchers, and dependencies remain
in your existing batch scripts. The helper does not run project-specific
submission wrappers; invoke those through their documented workflow.

### Test uncommitted code

To sync the current folder's contents without submitting a job, provide both
a host and a destination:

```sh
cluster sync -h hendrix --to dev/WMF
cluster sync -h hendrix --to dev/WMF --dry-run
```

From `thesis/models/WMF`, this copies that folder's contents directly into
`~/dev/WMF` on Hendrix, such as `~/dev/WMF/scripts/train_wfm.py`. It does not
copy sibling projects. Relative destinations start at the target user's home;
absolute paths work too. Quote `--to '~/dev/WMF'` if using a tilde, so your local
shell does not expand it. Both host and destination are required every time.

This uses `rsync`, which must be installed on both machines. Sync mirrors the
selected source: it adds and updates files and removes obsolete non-ignored
destination files, including files deleted locally. Deletions run after file
transfers, with a separate deletion scan so source I/O errors can prevent cleanup.
Checksums detect changed contents even if sizes and timestamps match.
Remote files covered by your local Git ignore rules are preserved, as is `.git`
metadata. This keeps remote environments, datasets, and ignored outputs intact.
Already-tracked files follow the source even if an ignore pattern matches them.
Deletion is limited to paths present during destination inspection, preserving
new job outputs created while syncing. Unignored files created after inspection
are considered on the next sync. A sync is not transactional: an interrupted
transfer can leave some files updated, so rerun it before starting a job.

No paired checkout or commit is needed. `--dry-run` connects to inspect the
destination and lists planned transfers and deletions without modifying project
files or creating the destination. A missing destination is shown as a planned
directory creation. Use the preview to review the mirror before applying it.
Filesystem roots, home directories, and `.git` destinations are rejected.
Real transfers show filenames and live overall progress, including percentage,
transfer speed, elapsed/remaining time, and rsync's remaining-file counter
(`to-chk`). Already up-to-date files are skipped by rsync.

To download run outputs from Hendrix, choose a remote source directory and a
local destination explicitly:

```sh
cluster fetch -h hendrix --from '~/dev/WMF/runs' --to ./runs --dry-run
cluster fetch -h hendrix --from '~/dev/WMF/runs' --to ./runs
```

This copies the contents of the remote `runs` directory into local `./runs`,
including logs, TensorBoard events, and checkpoints. Git ignore rules do not
exclude downloads; `.git` metadata is excluded. Existing downloads are updated,
local-only files are kept, and files on Hendrix are left in place. No Git
checkout, pairing, or job submission is needed. Host, source, and destination
are required, and `--dry-run` previews without creating or changing project
files. Remote relative paths start at the remote home; local relative paths
start at the current directory. Fetching a running job gives you the files
available at that moment; fetch again after it finishes for its final outputs.

Use `submit --sync` to upload a fresh repository snapshot and submit a job in
one step, without committing, pushing, or pairing a remote checkout:

```sh
cluster --host hendrix submit --sync --dry-run -- scripts/slurm/wmf_smoke.sbatch /cluster/path/to/BraTS/train
cluster --host hendrix submit --sync --follow -- scripts/slurm/wmf_smoke.sbatch /cluster/path/to/BraTS/train
```

Run from the same local directory where you would normally run `sbatch`.
The helper preserves that directory relative to the repository root. The
dry run lists the file count and command without connecting, uploading, or
submitting a job. `--sync` also works with `--local`.

Git selects tracked files and non-ignored untracked files using
`git ls-files --cached --others --exclude-standard`. This includes local edits,
staged changes, and new scripts. It respects nested `.gitignore` files,
negated patterns, `.git/info/exclude`, and global Git excludes. As with Git,
already-tracked files are included even when an ignore rule matches them.
Deleted files are omitted. Git metadata is never copied, symlinks are preserved
without following their targets, and submodules are currently unsupported.

Each `submit --sync` uploads a fresh snapshot of the whole repository over SSH using `tar`, into
`${XDG_DATA_HOME:-$HOME/.local/share}/cluster/runs/run-*/source` on the target.
Later submissions do not modify earlier snapshots or any paired checkout. A
`manifest.json` records the base commit and working directory, and
`changes.patch` records tracked changes; new files are in the source snapshot.
Your local files, index, and commit history stay unchanged. Snapshots persist
alongside their job output; remove old run directories when you no longer
need them.

The Python environment and datasets must already exist on the cluster. Set up
the environment in your batch script and pass cluster paths for external data.
Ignored files are not uploaded automatically.

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
cluster launchers, Rust sources, and a self-contained executable, including
local edits to those tracked configs.

On another PC, install the command from this checkout with `stow cluster`.
After building, you can also run
`./cluster/.local/bin/cluster setup my-cluster` directly. If the checkout is
already on a cluster, run `cluster setup --local`, or
`./cluster/.local/bin/cluster setup --local` before stowing it. The old
`cluster-setup` entry point remains available for compatibility.

The check reports Neovim, tmux, Git, ripgrep, fd, fzf, tree-sitter CLI, curl,
unzip, Python, Stow, and a C compiler. Python remains available for editor and
project tooling; the cluster commands themselves use Rust. **`--check` changes
no files and downloads nothing.** Setup reuses suitable tools and installs missing or outdated ones
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

Run `cluster-build --check` for Rust formatting, Clippy, unit tests, the static
release build, and the scratch-home/fake-scheduler integration tests. The
integration tests require Python 3 and GNU Stow; all network, scheduler, and
download operations in them use fixtures. They do not connect to an SSH host
or submit real jobs. After building, you can also run
`python -m unittest discover -s cluster/tests -v` directly.
