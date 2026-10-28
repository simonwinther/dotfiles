# Desktop transcoding

`desktopctl transcode [--path path] [input] [format] [resolution]` keeps the
Walker file, format, and resolution pickers. Pictures support JPG/PNG at high,
medium, or low resolution; videos support MP4/GIF at 4k, 1080p, or 720p.
FFmpeg and ImageMagick retain the existing encoding settings.

The Rust supervisor encodes into a private `.desktop-transcode-*` directory
beside the destination. Only a successful encoder exit with a nonempty regular
output can be published. After flushing the file, Linux `RENAME_NOREPLACE`
atomically moves it to `<input stem>-<resolution>.<format>`; a competing file,
directory, or symlink wins without being overwritten. Filesystems that cannot
perform this operation fail safely. Clipboard copying and completion notices
run after publication.

Ctrl+C, SIGTERM, and SIGHUP stop the encoder process group, including delegates,
then remove the temporary directory. Processes ignoring TERM are killed after
500 ms. Failed jobs remove their intermediate files too. A file already
published is complete and remains if clipboard copying fails or cancellation
arrives afterward. SIGKILL and power loss cannot run cleanup; any resulting
hidden temporary directory can be removed once its job is no longer running.

On NixOS, `desktop-tools` supplies the packaged supervisor. On another
distribution, run `desktop-transcode-build` from the Stow package (use `--check`
for formatting, Clippy, and job tests). Source and Cargo.lock live in
`.local/share/desktop-transcode`; build output stays in XDG cache. Runtime
dependencies are Bash, coreutils, `file`, FFmpeg, ImageMagick, `wl-copy`,
`notify-send`, and `desktopctl`/`fd` for interactive selection.
