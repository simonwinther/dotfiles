# Wallpaper grid

The picker and wallpaper commands are written in Rust with GTK4. The source,
committed `Cargo.lock`, styling, library settings, and launchers live in this
Stow package. The portable build copies the source and lockfile into the local
cache and uses `--locked`. Compiled executables and build output stay there too.

On NixOS, Home Manager owns the links for `background-grid` and `wallpapers`,
and Nix supplies the `background-grid-native` executable. The `desktop-awww` and
`desktop-wallpaper` user services manage the renderer and restore the wallpaper.

On other distributions, GNU Stow owns the links. Provide Rust/Cargo, a native
build toolchain, GTK4 development libraries, and pkg-config, plus awww, UWSM, and
Hyprland. From the dotfiles checkout, preview and apply the links, then build:

```sh
stow --target="$HOME" --no-folding -n -R -v background-grid wallpapers
stow --target="$HOME" --no-folding -R background-grid wallpapers
background-grid-build
```

On an already configured Stow-managed desktop, restow these packages and run
`background-grid-build` after pulling source changes.
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

The shared `hypr` package declares the shortcut and window rules in
`.config/hypr/background-grid.lua`. `background-grid-setup` recognizes the shared
Lua configuration and exits without changes. Edit the shared configuration source
for binding or window-rule changes.

If Stow reports a conflicting local file, back it up and move it aside before
retrying. Do not use `--adopt`, which would overwrite the package's source files.

The wallpapers live in the separate `wallpapers` package, under
`wallpapers/.local/share/wallpapers/tokyoled/` in this checkout. Home Manager or
Stow exposes them at `~/.local/share/wallpapers/tokyoled`, the default library in
`.config/background-grid/folders.json`. The paths work with another username or
checkout location.

See [.config/background-grid/README.md](.config/background-grid/README.md) for
folder organization, keyboard controls, and implementation details.
