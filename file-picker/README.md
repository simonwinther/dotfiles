# File and PDF picker

Super+Alt+Z opens the file picker, and Super+Z opens the PDF picker. Both use a
Ratatui interface backed by a local SQLite index. Cached filenames appear
immediately while a background scan refreshes the results. Files are sorted
newest first; search matches filenames and paths without reading file contents.

The picker opens a disposable floating terminal on the current workspace.
Selecting a file or pressing Escape closes the window. A shared terminal host
prepares fonts and GTK at login without creating hidden windows or reserving
workspaces. Opening another mode closes the previous picker.

| Key | Action |
| --- | --- |
| Enter | Copy selected files, or open the selected PDF |
| Tab / Shift+Tab | Select or deselect files |
| Ctrl+D / Ctrl+U | Scroll half a page |
| Ctrl+G | Open a native drag card |
| Ctrl+Y | Copy an absolute path |
| Ctrl+R | Refresh the current scope |
| Ctrl+T | Switch between newest-first and name order |
| Ctrl+A | Include dependency, build, and cache directories |
| Ctrl+S | Search readable files across the computer |
| Escape | Close the picker |

The drag card remains available across workspaces and closes after a successful
drop. Its palette and the picker palette follow the desktop appearance.

NixOS builds the picker, SQLite support, and drag helper declaratively. Home
Manager installs the configuration and starts the shared terminal preparation
service. Rebuild the host configuration after editing the source.

On other distributions, Stow owns the file-picker and Hyprland configuration.
The portable build launcher uses the shared Rust build helper and keeps its
source snapshot, compiler output, and executables in the local cache. Runtime
requirements include SQLite, fd, Ghostty, clipboard tools, and Zathura for PDFs;
the drag helper also requires GTK 4.

Index maintenance can refresh, inspect, remove cached paths, or clear a scope.
Removing an index entry never deletes the actual file. The command help lists
the maintenance options and cache overrides.

While cached results are idle, the index feed sleeps until a cache change,
refresh completion, or the existing 30-second refresh deadline. It also stops
promptly when its output pipe closes. Kernel notifications and process handles
replace repeated SQLite checks; when those facilities are unavailable, the
feed retains bounded polling. Large refresh errors are drained and capped so
a full diagnostics pipe cannot stall the picker or consume unbounded memory.

An unsupported or corrupt index is moved with its SQLite sidecars into a private
`index.sqlite3.quarantine-*` directory beside the cache, then replaced with an
empty current index. The next refresh rebuilds cached results from the filesystem.
Recovery reports the quarantine location on stderr. If the cache is busy or
cannot be recovered, the picker can still use a live scan.
Nonempty files shorter than a SQLite header are rejected before SQLite opens
them, preserving their original sidecars for inspection.
