# Backgrounds

Open with **Ctrl + Super + Space** or **Desktop → Style → Background**.

The picker is a native Rust/GTK4 application. NixOS supplies the executable
through Nix and links configuration with Home Manager. On a Stow-managed desktop,
rebuild after syncing source changes with `background-grid-build`;
`background-grid-build --check` also runs the automated checks. Rust source and
the committed `Cargo.lock` live in `.local/share/background-grid/` inside this
Stow package. Builds use `--locked`, and compiled files stay outside dotfiles.

- Pictures directly in the main library folder appear at the top.
- Subfolders become named categories beneath them, collapsed by default on each launch.
- Nested folders become nested categories. Empty folders are shown with a count of zero.
- Click a category or press Enter/Space to expand or collapse it.
- Previews use four columns. The window fits the visible folders and scrolls for larger collections.
- H/J/K/L and arrow keys navigate. Right/L opens a category; Left/H closes it.
- Enter, Space, or a click applies the highlighted image; q or Escape closes the picker.
- Hold Ctrl while clicking, pressing Enter, or pressing Space to apply without
  closing the picker. Expanded categories and keyboard focus stay in place.
- A blue border marks the current wallpaper, and an outline marks keyboard focus.

Your folders are `space`, `digital-art`, and `jokes`. Add or move images with the
file manager, then reopen the picker to refresh the collection. Renaming a folder
renames its category. If moving the currently selected image yourself, select it
again afterwards to update the saved wallpaper path.

The main library is configured by `folders.json`. By default it points at
`~/.local/share/wallpapers/tokyoled`, supplied by the separate `wallpapers`
package through Home Manager on NixOS or Stow on other distributions. Paths
starting with `~/` expand to the current user's home; relative paths are resolved
from the source directory of `folders.json` inside the dotfiles checkout.

Only visible previews are loaded. Closed categories do not decode their images
until opened. The cache lives in `${XDG_CACHE_HOME:-$HOME/.cache}/background-grid`,
outside dotfiles; cache keys include source path, modification time, and size.
Only previews are cropped to 16:9. Originals are never resized or rewritten.

Existing thumbnails from the Python picker are reused. Image decoding runs on
two background workers; GTK widgets remain on the main thread. Relative library
paths are resolved from the Stow source of `folders.json`, including when Stow
links each configuration file separately.

The shared setter uses awww with the animation settings in `transition.json`
(default: `any`, two seconds, 60 fps). It maintains the `background` symlink and
`state.json` under `${XDG_STATE_HOME:-$HOME/.local/state}/desktop`, sharing the
selection with `desktopctl`. Normal selection hides the grid so the transition
is visible; Ctrl-selection leaves it open for continued browsing. On NixOS, the
`desktop-awww` and `desktop-wallpaper` user services start the renderer and restore
the saved wallpaper.

The shortcut and window rules live in the shared `hypr` package's
`.config/hypr/background-grid.lua`. See the [package README](../../README.md) for
setup and link ownership.
