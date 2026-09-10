# Wallpaper grid

Install on an Omarchy PC after syncing this dotfiles checkout:

```sh
sudo pacman -S --needed stow awww gtk4 python-gobject python-pillow
cd ~/dotfiles
stow --no-folding -n -v background-grid
stow --no-folding background-grid
background-grid-setup
```

Setup adds one source line to that PC's existing Hyprland configuration and
removes the old Waypaper/awww startup entries. It backs up changed configuration
first, checks Hyprland for errors, and restores the wallpaper. It does not replace
the PC's monitor configuration. Use `background-grid-setup --dry-run` to preview,
or `--no-reload` when preparing a PC outside a running Hyprland session.

If Stow reports a conflicting local file, back it up and move it aside before
retrying. Do not use `--adopt`, which would overwrite the package's source files.

The wallpapers already live in this checkout, under
`omarchy/.config/omarchy/themes/tokyoled/backgrounds/`. The package reads that
library through a relative path in `.config/background-grid/folders.json`, so it
works with another username or checkout location. It does not require stowing
the legacy `omarchy` package or modifying Omarchy's own source files.

See [.config/background-grid/README.md](.config/background-grid/README.md) for
folder organization, keyboard controls, and implementation details.
