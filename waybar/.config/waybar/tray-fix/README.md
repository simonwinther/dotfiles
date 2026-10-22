# Electron tray compatibility

The patch fetches Electron tray properties using the registered bus name while retaining the usual proxy for signals. It is packaged against Waybar 0.15.0 in `pkgs/default.nix`. The portable Waybar wrapper resolves the independent native binary; it never replaces a system package. Rebuild the package after a compatibility review when updating Waybar.

Upstream tracking: https://github.com/Alexays/Waybar/issues/5240 and https://github.com/electron/electron/issues/52674.
