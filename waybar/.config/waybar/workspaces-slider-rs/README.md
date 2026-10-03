# workspaces-slider

Animated Hyprland workspace pill, drawn directly onto a `wlr-layer-shell`
surface. Replaces the earlier Python/GTK version, which cost ~36 MB PSS against
this one's ~2.8 MB.

No GTK, no Pango, no fontconfig: the surface is raw Wayland, the drawing is
`tiny-skia`, and the ten digits are rasterized once at startup with `fontdue`.
Hyprland is spoken to over its own sockets rather than by shelling out to
`hyprctl`, so a state read is a sub-millisecond round trip with no process
spawn. The animation runs off Wayland frame callbacks, so an idle pill causes no
wakeups at all.

## Build

The binary is deliberately not committed. Build it out of tree so `target/`
never lands in the dotfiles repo:

```sh
# Run from the dotfiles checkout.
CARGO_TARGET_DIR="$HOME/.cache/workspaces-slider-rs-target" cargo build --release --locked --manifest-path waybar/.config/waybar/workspaces-slider-rs/Cargo.toml
install -Dm755 "$HOME/.cache/workspaces-slider-rs-target/release/workspaces-slider" "$HOME/.local/bin/workspaces-slider"
```

Rebuild on each PC after pulling changes to this package. Cargo, a C compiler,
pkg-config, Wayland development files, and JetBrainsMono Nerd Font are required.

Started from `hypr/.config/hypr/autostart.lua`:

```lua
hl.on("hyprland.start", function()
    hl.exec_cmd("uwsm app -- ~/.local/bin/workspaces-slider")
end)
```

Restart after rebuilding:

```sh
pkill -x workspaces-slider
uwsm app -- ~/.local/bin/workspaces-slider &
```

## Appearance

The original dark palette is preserved. When Omarchy's current theme has a
`light.mode` marker, the pill uses Catppuccin Latte instead. Omarchy's Hyprland
reload updates the running pill without restarting it or changing its animation.
The usual workspace refresh also checks the appearance.

## Checking rendering

`--dump <path> [scale]` renders one frame to a PNG and exits, which is how the
output was compared against the original Cairo version. Pass the scale
explicitly — a fractional-scaled monitor negotiates buffer scale 2, and at least
one bug (a gradient stretched by the scale factor twice over) was invisible at
scale 1:

```sh
workspaces-slider --dump /tmp/frame.png 2
```

## Notes

- Renders at integer buffer scale; on a fractionally scaled output the
  compositor downsamples, same as the GTK version did. Implementing
  `wp_fractional_scale_v1` + `wp_viewporter` would make it sharper.
- Startup briefly peaks around 50 MB while `fontdue` parses the Nerd Font's
  12k glyphs, then `malloc_trim` hands it back; steady state is ~5 MB RSS.
  Caching the ten bitmaps would remove the spike.
