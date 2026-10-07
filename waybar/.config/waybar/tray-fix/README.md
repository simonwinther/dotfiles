# Electron tray compatibility

Slack 4.51.191 (Electron 43.4.0) only answers tray property requests addressed
to its well-known D-Bus name. GDBusProxy uses the unique connection name instead,
so Waybar 0.15.0 rejects the item and displays an empty tray pill.
The failure is reproducible with a raw `Properties.GetAll` call: the app name
returns its icon and ID, while the connection name returns an error.

Tracked upstream: https://github.com/Alexays/Waybar/issues/5240 and
https://github.com/electron/electron/issues/52674.

The patch fetches initial and updated properties using the registered name,
while retaining the usual proxy for signals and other operations. It is built
against Waybar 0.15.0 and never replaces the system package.

After `stow -R waybar`, run `waybar-build-tray-fix`, then
`omarchy restart waybar`. Building needs git, a C++ compiler, ninja, pkg-config,
the installed Waybar library headers, and meson or uv. Missing gdbus-codegen
tools are extracted from Arch's GLib development package into the cache.

The Waybar launcher selects the compatibility build only while the installed
Waybar package matches the recorded version. Package upgrades automatically
return to the system binary. Build output stays in
`${XDG_CACHE_HOME:-$HOME/.cache}/waybar-electron-fix`.
