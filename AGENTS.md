# Repository Guidelines

This repo is a GNU Stow-managed dotfiles collection. Each top-level directory is a Stow package that mirrors its target path under `$HOME`.

Examples:

* `nvim/.config/nvim`
* `zsh/.zshrc`
* `ghostty/.config/ghostty`
* `tmux/.tmux.conf`
* `usr-shell-scripts/.local/bin`

Run Stow from the repo root. Do not create manual symlinks with `ln`.

## Common commands

Preview changes before applying:

```bash
stow -n -v <package>
```

Apply or refresh a package:

```bash
stow -R <package>
```

Install common packages:

```bash
stow nvim zsh ghostty tmux
```

Format Neovim Lua:

```bash
stylua nvim/.config/nvim
```

Check shell scripts:

```bash
bash -n <script>
```

## Rules for agents

* Treat this as a dotfiles repo, not an app repo.
* Keep top-level package names lowercase and descriptive.
* Use English for code, comments, and docs.
* Do not manually symlink files.
* Do not edit destructive install scripts without explaining the risk.
* For shell scripts, prefer `#!/usr/bin/env bash` and `set -euo pipefail`.
* Before changing Stow-managed files, consider whether the change affects the real `$HOME` target after restow.
* After edits, suggest the exact validation command, usually `stow -n -v <package>` and then `stow -R <package>`.
