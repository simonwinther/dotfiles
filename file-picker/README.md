# File picker

Super+Alt+Z opens a fuzzy search for files of any type. Type part of a filename or
path, select it, and press Enter. Cached files appear newest first immediately, while a background refresh checks for changes.
Switch to ChatGPT and press Ctrl+V in the
composer to attach the file. Super+Z opens the PDF picker and Enter opens the
selected PDF in Zathura. Both pickers use [Ratatui](https://ratatui.rs/), with
background fuzzy matching through Nucleo. The native drag card remains GTK.

The picker opens in a floating terminal on the current workspace and closes
after an action or Escape. It creates no dedicated workspace and starts no
background window at login. Switching shortcuts closes the previous picker.
Pressing the same shortcut again focuses its existing window. One shared Ghostty
host remains running without windows between uses, avoiding a full GTK startup
for every shortcut press.

| Key | Action |
| --- | --- |
| Enter | Copy attachments; open the selected PDF in PDF mode |
| Ctrl+O | Open the selected file(s) in their default applications |
| Tab / Shift+Tab | Select / deselect multiple files |
| Alt+S (M-s), then a label | Jump to a visible row without changing the search |
| Ctrl+D / Ctrl+U | Scroll half a page down / up, as in Neovim (both pickers) |
| Ctrl+G | Open a small card to drag into ChatGPT or another app |
| Ctrl+Y | Copy one absolute path for a file upload dialog |
| Ctrl+R | Refresh the index and restore the original search scope |
| Ctrl+T | Toggle newest-first / name order, preserving the current scope |
| Ctrl+A | Include dependency, build, and cache directories |
| Ctrl+S | Search readable files across the whole PC, including mounted drives |
| Esc / Ctrl+C | Cancel |

Alt+S labels only the rows currently visible, from top to bottom. Labels use
the Danish alphabet: A–Z, Æ, Ø, Å. Lowercase keys work too. When more than 29
rows are visible, every label has two letters (AA, AB, AC, …); larger views use
longer labels. Type the label to move the highlight, then use Enter, Tab, or
another normal action. A partial label hides the other choices. Press Alt+S
again to cancel jump mode; Backspace removes a partial label. Esc or Ctrl+C closes
the picker, including while labels are shown.
Resizing or changing the terminal zoom cancels jump mode so the next Alt+S
uses the new viewport.

The drag card stays visible across workspaces. Drag its filename/card onto the
destination; it closes after a successful drop. A cancelled drop leaves it
available to retry. It also has copy buttons.

The PDF picker, file picker, and drag card use a matching light/dark palette,
solid backgrounds, and keyboard hints. Ratatui draws the search, results,
selection markers, and jump labels directly; no fzf or nested PTY is used.
Scanning and fuzzy matching run in the background so typing and Escape remain
responsive during large scans. Closing the terminal stops its UI and index feed.
A short index refresh can finish independently after a quick selection or Escape;
it has no window. File contents and thumbnails are never loaded by the picker.
`FILE_PICKER_APPEARANCE=light` or `dark` overrides desktop appearance for an
individual launch. The drag card's CSS is in `.config/file-picker/style.css`.

## Setup on another PC

On NixOS, the parent `nixos-config` repository builds this source through
`pkgs/file-picker.nix`. Home Manager owns the links and starts the single
`desktop-file-picker` user service with `--prepare-terminal`. Both shortcuts use
the packaged executable through PATH or `desktopctl path file-picker-native`.
The Nix wrapper supplies SQLite, Ghostty, GLib's D-Bus tools, the file scanner,
clipboard tools, viewers and desktop helpers. Do not run Stow or the portable
build helper over Home Manager's targets.

On another distribution, install Rust, pkg-config, fd, SQLite development files,
GTK4, GLib tools (`gdbus` and `gsettings`), Ghostty 1.3 or newer, Zathura,
wl-clipboard, libnotify, xdg-utils, xdg-user-dirs and GNU Stow. For example,
from `~/dotfiles` on Arch with the Lua Hyprland configuration:

```sh
sudo pacman -S --needed rust pkgconf fd sqlite zathura wl-clipboard ghostty gtk4 glib2 libnotify xdg-utils xdg-user-dirs stow
stow -n -v file-picker hypr
stow -R file-picker hypr
file-picker-build
file-picker --prepare-terminal
hyprctl reload
hyprctl configerrors
```

Resolve Stow conflicts before applying. The Hyprland package supplies the
bindings and floating window rules. Rust source lives in
`.local/share/file-picker`. `file-picker-build` builds the picker and drag card
into `${XDG_CACHE_HOME:-~/.cache}/file-picker/bin`; build artifacts stay in that
cache directory. The reviewed `Cargo.lock` lives beside `Cargo.toml`, and builds
use `--locked`. `file-picker-build --check` runs formatting, Clippy, and tests
with the drag feature before installing both release executables.
Rebuild after changing the Rust source, or if the cache was cleared.
The launcher exports a stable executable path so scans and drag helpers work
even if a rebuild replaces the running executable.

Super+Alt+Z and Super+Z use `file-picker --window` (plus `--pdf` for PDFs).
The launcher serializes rapid shortcut presses to prevent duplicate windows.
At login, the desktop user service calls `--prepare-terminal` to start one
Ghostty host with `initial-window=false`. The portable unit can be rendered
with the parent repository's `scripts/render-user-units.py` staging helper.
The shortcuts call Ghostty's D-Bus `new-window-command` action directly,
reusing that process without loading another GTK command-line instance.
`ghostty +new-window` remains a compatibility fallback.
The shared class is `file.picker`; the window title identifies file/PDF/recent
mode. No window or workspace exists between uses. Environment overrides and
root/command arguments are forwarded for each individual request.
Hyprland floats and centers the picker without switching workspaces. The window,
index feed, and search state end after an action, Escape, Ctrl+C, or Super+W. Each
new launch starts a fresh query using the saved index; Ctrl+R refreshes while open. Escape is
handled inside the picker and has no global Hyprland binding.

## Index maintenance

Refreshes happen automatically. These commands can also manage the cache:

```sh
file-picker --index-status
file-picker --index-update
pdf-picker --index-update
file-picker --root ~/Downloads --index-update
file-picker --index-remove ~/Downloads/old-name.pdf
file-picker --root ~/Downloads --index-clear
file-picker --cached-list
```

`--index-update` waits for a full refresh and reports added, updated and removed
counts. `--index-remove` removes exact cached paths from every scope, including
paths that no longer exist. It does not delete or modify actual files. A file
still present on disk is rediscovered on a later refresh. `--index-clear` clears
only the selected root/mode's cache and leaves actual files intact. Add `--pdf`
or `--all` to target that scope. `--cached-list` emits NUL-delimited cached paths
without scanning. Open pickers pick up external cache updates and removals automatically.

## Search and clipboard behavior

The picker loads filenames and timestamps from SQLite, without walking the
filesystem or checking every file on launch. The first page is displayed before
all cached entries have been decoded; the full search list continues loading
in the background. Typed searches wait for the complete dataset before accepting
a selection. A background refresh preserves an already complete result list
until its replacement is ready. The picker then refreshes with `fd` in the
background, and repeats every 30 seconds while the picker remains open. Added
files are inserted, changed timestamps are updated, and missing/renamed files
are pruned. A failed or interrupted refresh preserves the previous complete
snapshot. Readers can continue searching while the index is being updated.
Refreshes for the same scope are coalesced rather than running duplicate scans.

The index is `${XDG_CACHE_HOME:-~/.cache}/file-picker/index.sqlite3` (override with
`FILE_PICKER_INDEX`). It contains paths and timestamps, with raw path bytes
preserving non-UTF-8 filenames. Each root, resolved directory target, PDF mode,
and all-files mode has a separate scope. Normal and recent-file modes share
one complete scope; recent mode selects the newest 30,000 rows from it. A new
scope needs one initial scan; later opens use its saved rows. The picker's own
database and lock files are excluded from the index.

Cached results are replaced while preserving the query, focused file (if still
present), and surviving selections. Files are checked again before an action;
a missing cached file triggers a refresh instead of opening/copying it. If the
cache is unavailable, the picker falls back to a live scan.

Newest-first order uses the newer of modification and metadata-change times,
including downloaded/copied files whose modification dates were preserved.
Ctrl+T switches to name order using the cached list. Typing uses Nucleo fuzzy
matching in memory. `--list` remains a live scan for scripts, and
`--list --sort none` streams directly from `fd`. The default
file picker skips hidden files and directories, keeping app history, logs,
configuration and internal databases out of the list. It ignores `.gitignore`
so ordinary project files remain searchable.
The default view also skips common dependency/cache trees, `Android/Sdk`,
the installed packages in `~/R`, and `~/VirtualBox VMs`. R scripts in projects
remain searchable.
Ctrl+A includes hidden files and all these excluded trees when needed.
Ctrl+S scans `/` with no dependency exclusions.
Broad all-files or whole-PC scans can take longer to refresh and sort.
Unreadable directories are skipped; `/proc`, `/sys`, `/dev`, and `/run` are
always excluded. Symlinked files are included, but symlinked directories are
not traversed. Search a symlinked directory explicitly with `--root` if needed.
The PDF picker keeps its normal `fd` scope: PDFs in home,
excluding hidden and ignored files. Ctrl+R refreshes either picker.

File copying advertises `text/uri-list`, with correctly escaped file URLs.
Chromium exposes these as real `File` objects on paste. A separate foreground
`wl-copy` process retains ownership after the terminal closes, until the next
clipboard copy. Copying a file does not read its contents into the picker.
Filenames with spaces, Unicode, shell characters, and newlines are preserved
using NUL-delimited selection and URI encoding.

The destination website still decides which file types, sizes, and paste
operations it accepts. If paste is unsupported, use Ctrl+G to drag. For an
upload dialog, use Ctrl+Y, then Ctrl+L and Ctrl+V in its location field. Path
copying supports one file at a time.

The optional `file-picker --window --recent` mode opens the newest 30,000 files
at most, with an action menu after selection. Name sorting still uses that
recent set. Press `o` to select Open (Zathura for PDFs, the default app for other
files), `c` to select Copy path, or `a` to select Copy file as attachment; then
press Enter to confirm. Escape cancels. No file is automatically pasted. This
mode has no global shortcut and starts its window only when requested.

The tool can also be called directly:

```sh
file-picker --root ~/Downloads
file-picker --window
file-picker --window --recent
file-picker --list --sort name
pdf-picker --root ~/Downloads
file-picker --all
file-picker --copy ~/Downloads/paper.pdf
file-picker --drag ~/Downloads/paper.pdf
file-picker --path ~/Downloads/paper.pdf
file-picker --latest-download
```
