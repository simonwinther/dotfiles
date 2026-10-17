# Cluster setup

From the PC with this dotfiles checkout:

```sh
cluster-setup --check my-cluster
cluster-setup my-cluster
```

Use any SSH alias or `user@hostname`. SSH config settings, jump hosts, and
authentication work as usual. For a custom SSH port, use
`cluster-setup -p 2222 user@hostname`. Run the same setup command again to send
your current configs. It transfers only tracked Neovim and tmux files and the
setup command, including local edits to those tracked configs.

On another PC, install the command from this checkout with `stow cluster`.
You can also run `./cluster/.local/bin/cluster-setup` directly. If the checkout
is already on a cluster, run `./cluster/.local/bin/cluster-setup --local`.

The check reports Neovim, tmux, Git, ripgrep, fd, fzf, tree-sitter CLI, curl,
unzip, Stow, and a C compiler. **`--check` changes no files and downloads
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

Run the scratch-home checks with
`python -m unittest discover -s cluster/tests -v`. They require GNU Stow and
do not connect to an SSH host.
