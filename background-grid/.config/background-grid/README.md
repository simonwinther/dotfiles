# Backgrounds

Open with **Ctrl + Super + Space** or **Omarchy → Style → Background**.

- Pictures directly in the main library folder appear at the top.
- Subfolders become named categories beneath them, collapsed by default on each launch.
- Nested folders become nested categories. Empty folders are shown with a count of zero.
- Click a category or press Enter/Space to expand or collapse it.
- H/J/K/L and arrow keys navigate. Right/L opens a category; Left/H closes it.
- Enter, Space, or a click applies the highlighted image; Escape closes the picker.
- Hold Shift while clicking, pressing Enter, or pressing Space to apply without
  closing the picker. Expanded categories and keyboard focus stay in place.
- A blue border marks the current wallpaper, and an outline marks keyboard focus.

Your folders are `space`, `digital-art`, and `jokes`. Add or move images with the
file manager, then reopen the picker to refresh the collection. Renaming a folder
renames its category. If moving the currently selected image yourself, select it
again afterwards to update the saved wallpaper path.

The main library is configured by `folders.json`, with paths relative to this
directory inside the dotfiles checkout. By default it points at
`omarchy/.config/omarchy/themes/tokyoled/backgrounds/` in the same checkout.
This persistent library takes precedence over Omarchy's temporary theme copies,
avoiding duplicate images and keeping the same collection on both PCs.

Only visible previews are loaded. Closed categories do not decode their images
until opened. The cache lives in `~/.cache/background-grid`, outside dotfiles;
cache keys include source path, modification time, and size. Only previews are
cropped to 16:9. Originals are never resized or rewritten.

The shared setter uses awww for the original two-second expanding-circle
transition at 60 fps (a random origin on each selection) and maintains
Omarchy's `current/background` symlink. Normal selection hides the grid so the
transition is visible; Shift-selection leaves it open for continued browsing.
Local command wrappers route wallpaper changes through
that setter and forward unrelated Omarchy commands upstream. A post-boot hook
restores the image and stops the default swaybg once awww is ready, leaving one
wallpaper renderer. No Omarchy-managed source files are modified. Animation
settings live in `transition.json` alongside this file and travel with dotfiles.

Configuration backups from setup are under `~/.local/state/wallpaper-grid/`.
The initial image reorganization also saved originals and a `moves.json` checksum
manifest there, in a `dotfiles-*/images/` directory.

To disable this package, remove the `source = ~/.config/background-grid/hyprland.conf`
line from your Hyprland configuration, run `stow -D background-grid` from your
dotfiles checkout, then log out and back in. This leaves the wallpaper library intact.
