# iPhone file transfers

`receive-iphone-files` and `send-to-iphone` share one Rust program. Receiving
still sorts Taildrop downloads into `~/From iPhone/Pictures`, `Videos`,
`Documents`, `Audio`, and `Files`. Sending retains the file chooser, progress
dialog, desktop entry, and Files context-menu action. The runtime helpers do not
need Python.

From this dotfiles checkout:

```sh
stow --no-folding -n -R iphone-transfer
stow --no-folding -R iphone-transfer
iphone-transfer-build --check
systemctl --user daemon-reload
systemctl --user enable --now taildrop-receive.service
```

Resolve Stow conflicts first. If this PC has the previous unmanaged Python
helpers, back those helpers and their matching desktop/service files up before
restowing. Preserve the phone address in `~/.config/taildrop/iphone.json`. The
receiving folders contain your data and remain in place during installation.
After replacing an already running helper, restart `taildrop-receive.service`.

Build dependencies are Rust/Cargo and a C linker. Runtime dependencies are
Tailscale, `file`, Zenity, `notify-send`, and Nautilus for the existing folder and
context-menu actions. Tailscale must permit your user to access its local file
inbox. Its daemon continues to provide Taildrop transport and authentication.

The source lives in `.local/share/iphone-transfer`. The build script copies it
to `${XDG_CACHE_HOME:-$HOME/.cache}/iphone-transfer/build`, runs checks when
requested, and atomically installs the optimized binary under that cache's
`bin` directory. Cargo.lock and compiler output stay outside the Stow package.
Run `iphone-transfer-build` after source changes.

## Use

- Open **Files from iPhone** to browse received files.
- Open **Send to iPhone**, or select files in Nautilus and use
  **Scripts → Send to iPhone**.
- `send-to-iphone FILE ...` sends specific files without a chooser.
- `send-to-iphone --check` checks the configured peer and reports whether it is
  online, without sending anything.
- `receive-iphone-files --list` lists pending Taildrop files without downloading
  or acknowledging them.
- `receive-iphone-files --once` receives one pending batch and exits. The service
  normally runs continuously. Stop it before using another receiver on the same
  folder; a lock protects active partial downloads.

Both commands support `--help`. Use `send-to-iphone -- FILE ...` for filenames
that look like command options. Passing filenames directly also supports raw
Unix filename bytes; the graphical chooser uses a separator that preserves
embedded newlines.

Phone settings retain this format, using your iPhone's Tailscale address:

```json
{"name":"iPhone","target":"100.x.y.z"}
```

Settings honor `XDG_CONFIG_HOME`, with `~/.config` as the default. The receiver
defaults to `/run/tailscale/tailscaled.sock` and `~/From iPhone`; `--socket PATH`
and `--directory PATH` provide explicit alternatives, including isolated tests.

## Receiving and recovery

The receiver uses Tailscale's
[local file API](https://github.com/tailscale/tailscale/blob/4aef0237657e052c14f88562490415085fbfa820/feature/taildrop/localapi.go).
It downloads into a private `.incoming` file, checks the byte count, syncs the
completed file, and publishes it with a hard link before acknowledging the
remote copy. It streams file bodies, including large videos. Extension-based
MIME lookup and `file` content detection retain the existing sorting behavior.

Existing names gain ` (1)`, ` (2)`, and so on, without replacing existing files
or following a destination filename's symlink. Incomplete downloads are cleaned
up and remain in Tailscale's inbox. If acknowledgement fails, both the completed
local file and inbox copy remain; a later retry can create a numbered duplicate.
The service retries errors after five seconds, and processes other valid files
in a batch even if one fails. Startup removes only its stale regular partial
files after acquiring the receiver lock.

Check service status with `systemctl --user status taildrop-receive.service`,
and logs with `journalctl --user -u taildrop-receive.service`.

`iphone-transfer-build --check` runs formatting, Clippy, and transfer tests.
The tests use private Unix sockets, folders, phone settings, and fake desktop /
Tailscale commands. They cover exact bytes, streaming and chunked downloads,
size mismatches, safe filenames, collisions, acknowledgement failures, locking,
sorting, sender arguments, chooser cancellation, progress errors, and peer checks.
