# Browser dictation audio ducking

The ChatGPT dictation extension uses one Rust program for the native messaging
bridge and system volume commands. `dictation-volume-duck` and
`chatgpt-dictate-audio-duck-host` are small launchers for the same compiled binary.
The bridge has no Python dependency.

From this dotfiles checkout, install and build with:

```sh
stow --no-folding -n -R --ignore=extensions chrome
stow --no-folding -R --ignore=extensions chrome
dictation-volume-duck-build --check
```

`--ignore=extensions` preserves the existing shared extension directory link.
On a new PC, also run `stow -n -v chrome` and `stow chrome` to install that link.
Resolve any Stow conflicts before applying changes. The browser native messaging
manifests require an absolute host path; check their `path` on the other PC.

Building requires Rust/Cargo and a C linker. At runtime, install WirePlumber's
`wpctl`, or `pactl` as a fallback when `wpctl` is unavailable. The build script
keeps Cargo.lock, build files, and the optimized binary under
`${XDG_CACHE_HOME:-$HOME/.cache}/dictation-volume-duck`. Rebuild after changing
the source in `.local/share/dictation-volume-duck`.

## Commands and behavior

```sh
dictation-volume-duck start my-client 0.3
dictation-volume-duck stop my-client
dictation-volume-duck status
dictation-volume-duck reset
```

The first client lowers the original volume to 30% of its starting value. A
second start from the same client is harmless. Other clients join the session
without lowering volume again, and the last stop restores the original volume
and mute state. The extension continues to use its existing host name, tab IDs,
and Alt+D / Alt+C / Alt+S shortcuts.

The defaults remain `VOLUME_DUCK_FACTOR=0.3`, `VOLUME_DUCK_FADE_IN_MS=700`,
`VOLUME_DUCK_FADE_OUT_MS=900`, and `VOLUME_DUCK_FADE_STEPS=12`. A start command's
factor overrides the environment; the browser explicitly sends `0.3`. Zero fade
duration disables interpolation. Steps are limited to 100, durations to 60
seconds, and nonpositive factors fall back to 0.3. The browser operation has a
five second deadline; longer custom fades are for the command line.

State remains in `$XDG_RUNTIME_DIR/dictation-volume-duck/state.env`, protected by
the same `flock` lock as the native `dictate` program. Without XDG_RUNTIME_DIR,
both Rust programs use `/tmp/dictate-<uid>/dictation-volume-duck`. Shell quoting
from the old helper is decoded as data; the state file is never executed. Old
state without a sink ID is upgraded when it is next changed. New sessions retain
the original sink and, with PipeWire, its object serial so a changed default or
recycled node ID cannot redirect restoration.

Failed starts attempt an immediate rollback. Failed restoration keeps the saved
original volume for a subsequent stop, reset, or start to retry. `status` retains
the existing `idle` / `ducked clients=...` output; after a failure an idle state
can still have a pending restoration. Use `reset` to retry it once audio is
available.

The native host preserves the single framed JSON request/reply used by
[`sendNativeMessage`](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging).
It accepts at most 64 KiB per request and returns the existing `ok`, `status`,
`stdout`, `stderr`, or `error` fields. External audio commands have bounded
timeouts, and the private state file is replaced atomically.

`dictation-volume-duck-build --check` runs formatting, Clippy, and integration
tests with fake audio tools and private runtime directories. The tests cover
browser framing, concurrent clients, shared native dictation state, legacy state,
both backends, fades, mute restoration, failed commands, timeouts, and recovery.
