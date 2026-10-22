# Rust dictation

Native Rust replacement for the Python tool, including live transcription,
batch recording, the transcript overlay, personal vocabulary, local cleanup,
volume ducking, recovery, retries, and accuracy benchmarks.

Super+D still runs `dictate live toggle`. Super+Ctrl+Shift+X cancels.
Existing `config.env`, `secrets.env`, and `dictionary.toml` files work unchanged.
Credentials stay in `~/.config/dictate/secrets.env`; they are never included in
session or recovery metadata. Configuration files override environment variables,
and secrets override regular configuration. XDG directory overrides are supported.

## Install or sync to the other PC

Build dependencies: Rust/Cargo, a C compiler, pkg-config, GTK 4, and
gtk4-layer-shell. Recording needs PipeWire's `pw-record` and GNU `stdbuf`;
clipboard delivery needs `wl-copy`. Batch speed adjustment and MP3 conversion
use ffmpeg. Hyprland supplies monitor tracking and automatic paste; `wpctl`
or `pactl` supplies volume control. All are already available on this PC.

From the dotfiles checkout:

```sh
stow -n -v dictate
stow -R dictate
dictate-build --check
dictate check
```

If the dry run reports an existing uv-installed `~/.local/bin/dictate`, move
that launcher into `~/.cache/dictate/previous/` before restowing. Keep the old
Python checkout and uv environment if you want a rollback. Use Stow for links.

Source lives in this package under `.local/share/dictate`. Release executables,
Cargo's dependency lockfile, intermediate files, and checks stay under
`${XDG_CACHE_HOME:-~/.cache}/dictate`. No binaries or build output belong in Git.
`dictate-build` rebuilds and atomically replaces the cached executables. The
launcher builds them on first use if they are missing. `--check` additionally
runs formatting, Clippy with warnings rejected, and the offline test suite.
Rebuild after pulling source changes on either PC.

## Commands

```text
dictate live  {start,stop,cancel,toggle,status} [--language xx] [--model NAME]
dictate batch {start,stop,cancel,toggle,once,status,retry,discard} [--language xx] [--model NAME]
dictate {cancel,status,check,models}
dictate dictionary {init,path,check}
dictate bench AUDIO [SPEEDUP_FACTOR ...] [--language xx]
dictate eval init [DIRECTORY]
dictate eval check SUITE.toml
dictate eval run SUITE.toml [--language xx] [--model NAME] [--output REPORT.json]
dictate eval score SUITE.toml TRANSCRIPTS.json [--output REPORT.json]
```

`bench` and `eval run` make paid API requests. `check`, `eval check`,
`eval score`, and the automated tests are offline. Evaluation suites and saved
reports use the same format as the Python version, including word error rate,
technical-term recall, and number recall.

## Data science vocabulary

The personal dictionary and starter dictionary contain 65 focused hints for
machine learning, statistics, and Python tools, including linear classifiers,
cross-validation, regularization, PyTorch, JAX, and Weights & Biases. A short
`OPENAI_PROMPT` in the personal configuration describes everyday English/Danish
dictation that often discusses these subjects. Both live and batch requests
receive the hints; explicit `LIVE_PROMPT` overrides still take precedence.
This follows OpenAI's [transcription context guidance](https://developers.openai.com/api/docs/guides/realtime-transcription#add-transcription-context).

Hints help recognition without forcing the terms into ordinary speech. The 42
local corrections cover specific spelling and grammar mistakes; ordinary
"weights and biases" remains unchanged. There is no extra cleanup API request.
Edit `~/.config/dictate/dictionary.toml` and run `dictate dictionary check`;
changes apply to the next recording without rebuilding. Existing punctuation
commands remain available. For literal words such as "training period", set
`PUNCTUATION_MODE=explicit` and say "punctuation comma" for a command.

## Latency and desktop behavior

The CLI and recording worker do not load GTK. Microphone bytes flow directly
from PipeWire into a bounded channel and then the WebSocket, while a private
audio journal supports recovery. Transcript previews and waveform levels reach
the overlay over a pipe; there is no polling of growing audio or preview files.
Personal corrections and punctuation use small phrase tries, keeping Unicode
regex compilation out of recording startup.

Live upload reads the existing audio journal independently of microphone
capture, so a full network queue cannot block stop/cancel or recording limits.
Queued samples retain their original order without unbounded memory buffering.
WebSocket audio, commit, and keepalive writes have deadlines; the final
transcript deadline includes sending the commit. Failed uploads keep the
recording available for retry.

Long transcripts reuse the overlay's cached text layout while the visible tail
is unchanged. Live preview processing examines at most the last 8,000 characters;
the complete final transcript is still saved, copied, and pasted. Incoming
transcript storage has an aggregate size and item-count limit, and stale draft
events cannot overwrite final text.

The recording overlay skips repaints when its waveform, transcript, timer, and
displayed cost are unchanged. A regression scenario requires 10 repaints for
625 input polls during ten quiet seconds instead of repainting every poll.
The waveform and finishing animation keep their smooth cadence. Desktop event
bursts share one monitor refresh, independent monitor queries run concurrently,
and sessions with the overlay disabled do no monitor tracking.

Stop and cancel interrupt volume fade-in. The personal configuration uses a
150 ms fade-in, a 200 ms fade-out, and eight steps, reducing the time before
another recording can start. The original volume and shared browser-dictation
coordination are preserved. A recorder that closes its audio pipe without
exiting has a bounded shutdown; failure retains audio for retry without
delivering an incomplete recording.

PipeWire 1.6.x reports exit status 1 even after a normal requested recording
stop. A quiet, requested stop is accepted; unexpected exit or recorder error
output still preserves audio for recovery. Failure messages keep the recorder's
actual cause, exit status, and bounded diagnostic output instead of replacing
them with a generic microphone error. Diagnostics add no per-audio-frame work.

Measured on 17/10/2026, cancellation-to-ready median fell from 1,486 ms to
195 ms with the interruptible fade and shorter personal fade settings. This
used three trials after a warmup, synthetic PCM, a loopback WebSocket, and a
simulated audio sink. Every trial restored the original volume. These numbers
measure local session cleanup, excluding network and model response time.

Measured on 15/10/2026 with the installed launchers and the existing personal
dictionary (19 terms, 36 corrections):

| Local startup measurement | Python | Rust |
| --- | ---: | ---: |
| First audio reaches the WebSocket server | 234.4 ms | 17.1 ms |
| Start command returns | 270.6 ms | 13.4 ms |

These are medians of ten recordings after one warmup, using synthetic PCM and
a loopback WebSocket server. Overlay, ducking, and paste were disabled for both
versions. The first-audio improvement is 13.7×; network and model transcription
latency are outside this measurement. The benchmark never contacted OpenAI.

The separate overlay animates at up to 60 Hz through native Wayland layer-shell. Its
rounded cyan frame and black background match the previous overlay. It follows
the active application's monitor, stays above other windows, accepts no keyboard
focus, and lets pointer clicks through. It needs no additional Hyprland rules.

The current live model and `LIVE_DELAY=low` are preserved. Lowering model delay
changes the accuracy/latency tradeoff; the port does not change that setting.
Final delivery has no artificial overlay dwell or paste delay by default.
`PASTE_DELAY` still allows an explicit delay if an application needs one.
Terminals receive Ctrl+Shift+V; other applications receive Ctrl+V. Both Lua and
legacy Hyprland configuration providers are supported.

## Recovery

Failed requests preserve audio under `$XDG_STATE_HOME/dictate/recovery`
(`~/.local/state/dictate/recovery` by default). `dictate batch retry` can recover
live or batch recordings and understands recovery items from the Python tool.
If final text was already obtained, retry uses the cached text without uploading
again. Changing the saved model or language explicitly requests a new transcript.
Retry retains request settings and uses current credentials, endpoints, and
desktop delivery settings. A new recording keeps any older recovery item until
another failure replaces it or an explicit discard removes it.

Cancelling before delivery prevents clipboard changes and paste. Timeouts never
deliver provisional text. A crashed worker's audio is recovered on the next
status or start command; batch WAV headers are repaired when necessary. Recovery
archiving failures retain the original recording and metadata for another attempt.
Session files are private and stale PIDs are checked against Linux process start
times and boot IDs. Volume state coordinates with the browser's dictation helper
and records the original audio device identity for restoration.

The latest transcript and log still default to `/tmp/dictate-last.txt` and
`/tmp/dictate-last.log`. `KEEP_AUDIO=1` retains successful recordings and local
preprocessing variants. Both recording limits and all personal corrections carry
over from the existing configuration.
