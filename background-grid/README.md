# Wallpaper grid

The picker and wallpaper commands are written in Rust with GTK4. The source,
styling, library settings, and launchers live in this Stow package. Compiled
executables, Cargo's lockfile, and build output stay in the local cache.

Install on an Omarchy PC after syncing this dotfiles checkout:

```sh
sudo pacman -S --needed stow awww gtk4 rust base-devel
cd ~/dotfiles
stow --no-folding -n -R -v background-grid
stow --no-folding -R background-grid
background-grid-build
background-grid-setup
```

On an already configured PC, restow the package and run `background-grid-build`
after pulling source changes; the setup helper does not need to run again.
The picker uses GTK's image loaders. The picker, wallpaper commands, and setup
helper all run in Rust; Python, PyGObject, and Pillow are not required.

`background-grid-build --check` checks Rust formatting, runs Clippy and the
automated catalog, thumbnail, and command tests, then installs a release build.
The command tests use a temporary home and fake renderer commands; they do not
change the running desktop. The installed executable lives at
`${XDG_CACHE_HOME:-$HOME/.cache}/background-grid/bin/background-grid-native`.

Open with `background-grid` or Ctrl+Super+Space. `--set IMAGE`, `--next`, and
`--restore` share the same wallpaper setter; `--list` lists library images
without opening a window. `--help` describes these modes.

Setup adds one source line to that PC's existing Hyprland configuration and
removes the old Waypaper/awww startup entries. It backs up changed configuration
first, checks Hyprland for errors, and restores the wallpaper. It does not replace
the PC's monitor configuration. Use `background-grid-setup --dry-run` to preview,
or `--no-reload` when preparing a PC outside a running Hyprland session.

If Stow reports a conflicting local file, back it up and move it aside before
retrying. Do not use `--adopt`, which would overwrite the package's source files.

Hyprspace's companion integration can already own `~/.local/bin/omarchy-launch-walker`.
If that wrapper still forwards the Background menu to `background-grid`, keep it
and add `--ignore=omarchy-launch-walker` to both Stow commands above.

The wallpapers already live in this checkout, under
`omarchy/.config/omarchy/themes/tokyoled/backgrounds/`. The package reads that
library through a relative path in `.config/background-grid/folders.json`, so it
works with another username or checkout location. It does not require stowing
the legacy `omarchy` package or modifying Omarchy's own source files.

See [.config/background-grid/README.md](.config/background-grid/README.md) for
folder organization, keyboard controls, and implementation details.
