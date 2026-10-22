# Snippet and macro pickers

Super+Alt+Period opens `globalsnip`; Super+Alt+M opens `macro-picker`. Both use
the same Rust program with the existing fzf search layout, colors, and preview
pane. Enter copies and pastes the selected text into the previous application;
Escape closes the picker without changing the clipboard.

The `hypr` Stow package provides the existing shortcuts and command names. This
package provides their shared executable, build helper, and editable entries.
On an already configured PC:

```sh
sudo pacman -S --needed stow rust base-devel fzf wtype ghostty
cd ~/dotfiles
stow --no-folding -n -R -v text-picker
stow --no-folding -R text-picker
text-picker-build --check
```

The usual `hypr` desktop setup is also required on a new PC. Resolve Stow
conflicts before applying its changes; do not use `--adopt`.

Edit `.config/text-picker/snippets.json` or `macros.json` to change entries.
Each record has a `label` and `text`; JSON escapes such as `\n` and `\t` preserve
multiline text and tabs. Entry edits take effect the next time the picker opens
and do not require a rebuild. The migrated collection contains the same 14
snippets and four macros. `globalsnip --list` and `macro-picker --list` print
their labels without opening a window or changing the clipboard.

After Rust source changes, run `text-picker-build`. `--check` also checks
formatting, runs Clippy, and tests configuration, native Hyprland IPC, focus
handoff, fzf selection, and preview quoting. These checks do not touch the live
clipboard or send keyboard input. Cargo's lockfile, build output, and installed
binary stay under `${XDG_CACHE_HOME:-$HOME/.cache}/text-picker/`.

The program talks to Hyprland directly. It uses fzf for searching and wtype for
the final Ctrl+V, or Ctrl+Shift+V in Ghostty. A separate Rust process, launched
by Hyprland, owns the Wayland clipboard after the picker terminal exits. It
serves paste requests until another application replaces that selection.
If focus does not leave the picker, the copied text remains available for
manual pasting.

Previews use a private snapshot and numeric record IDs. Snippet text is never
inserted into a shell command, and the exact selected text is sent to the
clipboard helper over a private local socket.
