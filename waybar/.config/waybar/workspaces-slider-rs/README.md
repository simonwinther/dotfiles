# workspaces-slider

Animated Hyprland workspace pill, drawn directly onto a `wlr-layer-shell`
surface. Replaces the earlier Python/GTK version, which cost ~36 MB PSS against
this one's ~2.8 MB.

Every output gets its own pill, each showing that monitor's own active
workspace and occupancy, and clicks and scrolls act on the monitor they landed
on. Monitors are picked up and dropped as they are plugged in and out. Set
`WAYBAR_OUTPUT_NAME` to pin the pill to a single output instead.

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
CARGO_TARGET_DIR=~/.cache/workspaces-slider-rs-target cargo build --release
cp ~/.cache/workspaces-slider-rs-target/release/workspaces-slider ~/.local/bin/
```

Started from `hypr/.config/hypr/autostart.conf`:

```
exec-once = uwsm app -- ~/.local/bin/workspaces-slider
```

Restart after rebuilding:

```sh
pkill -f '\.local/bin/workspaces-slider'
uwsm app -- ~/.local/bin/workspaces-slider &
```

Matching on the path, not the name: Linux truncates process names to 15
characters and `workspaces-slider` is 17, so `pkill -x` never matches it.

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
  Caching the ten bitmaps would remove the spike. Outputs sharing a buffer
  scale share one `Renderer`, so the parse happens once per distinct scale
  rather than once per monitor.
