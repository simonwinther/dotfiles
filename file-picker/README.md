# File picker

Super+Alt+Z opens a fuzzy search for files of any type. Type part of a filename or
path, select it, and press Enter. Files appear newest first in a prepared list.
Switch to ChatGPT and press Ctrl+V in the
composer to attach the file. Super+Z opens the PDF picker and Enter opens the
selected PDF in Zathura. Both pickers share the same styling. The picker and
native drag card are written in Rust; no Python runtime is used.

Switching between the two picker shortcuts hides the previous picker.

| Key | Action |
| --- | --- |
| Enter | Copy the selected file(s) as attachments |
| Tab / Shift+Tab | Select / deselect multiple files |
| Ctrl+D / Ctrl+U | Scroll half a page down / up, as in Neovim (both pickers) |
| Ctrl+G | Open a small card to drag into ChatGPT or another app |
| Ctrl+Y | Copy one absolute path for a file upload dialog |
| Ctrl+R | Rescan the original directory and restore the initial scope |
| Ctrl+T | Toggle newest-first / name order, preserving the current scope |
| Ctrl+A | Include dependency, build, and cache directories |
| Ctrl+S | Search readable files across the whole PC, including mounted drives |
| Esc | Cancel |

The drag card stays visible across workspaces. Drag its filename/card onto the
destination; it closes after a successful drop. A cancelled drop leaves it
available to retry. It also has copy buttons.

The PDF picker, file picker, and drag card use a matching light/dark palette,
readable solid backgrounds, and keyboard hints. The pickers share native fzf
styling; they do not generate thumbnails or read file contents. Rust reads
filesystem timestamps and sorts the list in the background. The drag card
reads size metadata for its selected file. The default palette follows
the desktop appearance; `FILE_PICKER_APPEARANCE=light` or `dark` overrides it for
an individual launch. Its CSS is in `.config/file-picker/style.css`.

## Setup on another PC

From `~/dotfiles` on an Desktop PC with the Lua Hyprland configuration:

```sh
sudo pacman -S --needed rust pkgconf fd fzf zathura wl-clipboard ghostty gtk4 libnotify stow
stow -n -v file-picker hypr
stow -R file-picker hypr
file-picker-build
hyprctl reload
hyprctl configerrors
file-picker --warm
file-picker --warm --pdf
```

Resolve Stow conflicts before applying. The Hyprland package supplies the
bindings and floating window rules. Rust source lives in
`.local/share/file-picker`. `file-picker-build` builds the picker and drag card
into `${XDG_CACHE_HOME:-~/.cache}/file-picker/bin`; Cargo's lockfile and build
artifacts also stay in that cache directory, outside the dotfiles repository.
Rebuild after changing the Rust source, or if the cache was cleared.
The launcher exports a stable executable path, so prepared pickers can refresh
and launch helpers even after a rebuild replaces their running executable.

Super+Alt+Z and Super+Z bring their prepared windows forward directly
from Hyprland; they use `file-picker --window` (plus `--pdf` for PDFs) to
start them if needed. Hyprland prepares both pickers at login, on the
hidden `special:file-picker` and `special:pdf-picker` workspaces. Each shortcut
brings its existing window forward and refreshes the file list. Esc, copying
and dragging hide the picker
while preserving its last drawn screen; refresh keeps the existing results
visible until the replacement list is ready. The first open waits for the
finder interface to initialize before showing the terminal. These avoid a
blank terminal flash between runs. Esc, copying and dragging also
prepare a fresh search for next time; they do not start a new terminal on
each launch. Closing it with Super+W stops it; the next shortcut starts it again.
Escape hides the focused picker directly in Hyprland, then cancels its search
in the background. In other applications Escape passes through normally.
Selecting a PDF opens it in Zathura while preparing the PDF picker for next time. Both picker
windows appear without a window animation. No persistent file index is needed.

## Search and clipboard behavior

The picker scans with `fd`, then Rust sorts regular files by the newer of their
modification and metadata-change times. This includes newly downloaded or
copied files whose modification dates were preserved. Lists are prepared while
hidden; refresh keeps the previous list visible until its replacement is ready.
Ctrl+T toggles name order. Typing ranks fuzzy matches by relevance, using list
order to break ties. `--sort none` retains direct streaming for CLI use. The default
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
