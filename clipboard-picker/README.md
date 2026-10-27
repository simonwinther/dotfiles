# Clipboard picker

An independent Rust/GTK4 frontend for the existing Elephant clipboard history.
It does not run another clipboard watcher or rewrite saved history. Home Manager
installs it on NixOS; the portable wrapper also works in a Stow-managed home with
`clipboard-picker-native` installed. It requires Hyprland, Elephant and wl-clipboard.

Open with Alt+V. The frameless grid follows the desktop's current Walker palette
and selected monospace font. Four columns show images and text; numbered corner
badges show selection order. Space toggles the focused card; arrow keys and
Vim's h/j/k/l move between cards. Ctrl+J / Ctrl+K also move down / up and work
while search has focus. Ctrl+D / Ctrl+U move half a page down / up, matching the
other desktop pickers. Tab cycles all → images → text → all; Shift+Tab cycles
backward. Search and category changes retain existing selections. Ctrl+A
selects visible cards when the grid has focus; in search it selects the text.
Ctrl+Shift+A clears the selection.

- Enter or Paste: paste checked entries in click order into the original window.
  With nothing checked, paste the previewed entry.
- Ctrl+Enter or Copy only: copy a single entry, join selected text with newlines,
  or copy multiple selected images as a file list.
- Ctrl+Shift+Enter or Copy files: copy selected images as individual files.
- The picker opens with search focused. Escape enters grid navigation without
  clearing the search; another Escape closes it. Down or Ctrl+J also enters the
  grid. `/`, `i`, or Ctrl+L focuses search again. Typing h/j/k/l in search enters
  those letters normally.
- `p` in the grid or Ctrl+Space anywhere toggles a full preview of the focused
  entry. Escape returns from preview to the grid. Movement also works in preview.
- Ctrl+I / Ctrl+T: filter images / text. Ctrl+R refreshes history.
- The three-dot menu (Ctrl+comma) holds copy, file-copy, selection and history
  actions, plus the paste interval. The main view shows a compact paste action.
- Ctrl+P pins/unpins; Delete removes the focused entry outside search.
  Pinned entries cannot be
  removed until unpinned, matching the history provider's rules.

Batch paste uses Ctrl+V, or Ctrl+Shift+V for recognized terminals. It never presses
Enter or sends a message. The original window must remain focused; changing
windows stops the remaining batch. Clipboard ownership is held for the interval
set in the menu (default 800 ms per entry). Increase it for slow image imports.
The last item remains copied after the batch. A receiving app must accept the
content: text-only fields cannot accept images, and apps that replace their
current attachment on every paste will still do so. A window address cannot
detect focus changes between controls inside the same window.

Copy files uses `text/uri-list` for file managers and apps that accept file-list
pastes. Other apps should use sequential Paste. Images are exported with private
permissions into `$XDG_CACHE_HOME/clipboard-picker/exports` (default
`~/.cache/clipboard-picker/exports`) so history eviction cannot invalidate a copied
file list. Exports remain until that cache is cleared; avoid clearing it before
the destination has read the files.

The opt-in compositor smoke test is `tests/clipboard-picker-ui.py` in the NixOS
configuration repository. It displays synthetic history and checks category
cycling, Vim movement, half-page movement, search, preview, selection order and
paste shortcuts through real keyboard input and mocked commands. It captures
the real GTK window. It does not exercise a real destination app or
change the real clipboard/history. Run it in the `rust-desktop` development shell
with the built executable and an output screenshot path as its two arguments.

Build with `cargo build --release` in `.local/share/clipboard-picker`, using GTK4
and gtk4-layer-shell development libraries. `--check` checks the history connection
and prints counts only. `--target ADDRESS` supplies the original Hyprland window;
`--delay-ms 800` sets the initial paste interval. Tests cover history framing,
unmodified text, bounded responses, file URI escaping, batch ordering, failed
clipboard writes and destination focus changes.
