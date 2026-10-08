# File picker

Super+Alt+Z opens a fuzzy search for files of any type. Type part of a filename or
path, select it, and press Enter. Switch to ChatGPT and press Ctrl+V in the
composer to attach the file. Super+Z opens the PDF picker and Enter opens the
selected PDF in Zathura. Both pickers share the same styling.

| Key | Action |
| --- | --- |
| Enter | Copy the selected file(s) as attachments |
| Tab / Shift+Tab | Select / deselect multiple files |
| Ctrl+D | Open a small card to drag into ChatGPT or another app |
| Ctrl+Y | Copy one absolute path for a file upload dialog |
| Ctrl+R | Rescan the original directory and restore the initial scope |
| Ctrl+A | Include dependency, build, and cache directories |
| Ctrl+S | Search readable files across the whole PC, including mounted drives |
| Esc | Cancel |

The drag card stays visible across workspaces. Drag its filename/card onto the
destination; it closes after a successful drop. A cancelled drop leaves it
available to retry. It also has copy buttons.

The PDF picker, file picker, and drag card use a matching light/dark palette,
readable solid backgrounds, and keyboard hints. The pickers share native fzf
styling; they do not generate thumbnails, read file contents, or transform the
incoming path list. The drag card reads size metadata only for its selected
file. The default palette follows
the desktop appearance; `FILE_PICKER_APPEARANCE=light` or `dark` overrides it for
an individual launch. Its CSS is in `.config/file-picker/style.css`.

## Setup on another PC

From `~/dotfiles` on an Omarchy PC with the Lua Hyprland configuration:

```sh
sudo pacman -S --needed python fd fzf zathura wl-clipboard ghostty gtk4 python-gobject libnotify stow
stow -n -v file-picker hypr
stow -R file-picker hypr
hyprctl reload
hyprctl configerrors
```

Resolve Stow conflicts before applying. The Hyprland package supplies the
bindings and floating window rules. No daemon, database, generated file list,
or machine-specific path is required.

## Search and clipboard behavior

The picker streams a fresh `fd` scan into `fzf`, so newly created files are
searchable immediately. It includes hidden files and ignores `.gitignore`.
The default view searches home while excluding common dependency/cache trees;
Ctrl+A includes those too. Ctrl+S scans `/` with no dependency exclusions.
Unreadable directories are skipped; `/proc`, `/sys`, `/dev`, and `/run` are
always excluded. Symlinked files are included, but symlinked directories are
not traversed. Search a symlinked directory explicitly with `--root` if needed.
The PDF picker keeps its original `fd` behavior: PDFs in home, excluding hidden
and ignored files. Ctrl+R refreshes either picker.

File copying advertises `text/uri-list`, with correctly escaped file URLs.
Chromium exposes these as real `File` objects on paste. A separate foreground
`wl-copy` process retains ownership after the terminal closes, until the next
clipboard copy. Copying a file does not read its contents into the picker.
Filenames with spaces, Unicode, shell characters, and newlines are preserved
using NUL-delimited selection and URI encoding.

The destination website still decides which file types, sizes, and paste
operations it accepts. If paste is unsupported, use Ctrl+D to drag. For an
upload dialog, use Ctrl+Y, then Ctrl+L and Ctrl+V in its location field. Path
copying supports one file at a time.

The tool can also be called directly:

```sh
file-picker --root ~/Downloads
pdf-picker --root ~/Downloads
file-picker --all
file-picker --copy ~/Downloads/paper.pdf
file-picker --drag ~/Downloads/paper.pdf
file-picker --path ~/Downloads/paper.pdf
```
