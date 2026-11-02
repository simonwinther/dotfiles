# Desktop search

A Rust backend for Morphing Notch Bar and the standalone clipboard picker. It
replaces the Go provider service. Both bar styles use the native menu renderer;
Walker is no longer needed for launcher modes or fallback dialogs.

The backend provides applications, files, PATH commands, calculator results,
symbols, Unicode, web search, repositories, notifications, Bluetooth and tasks.
The calculator uses fend for arithmetic and unit conversions. Its expression
syntax differs from Qalculate. Files are searched with fd on demand; application
metadata is cached. Nucleo provides Unicode-aware fuzzy scoring with reused
matcher scratch space and parsed patterns. Device catalogs and authentication reuse the Rust bar.
Configured desktop menus use their existing argument lists.

Clipboard collection uses Wayland data control directly, preferring the ext
protocol and falling back to wlr. Selection payloads stream to private files;
the database stores bounded previews and a full-text search index. Collection
ignores password-manager hints, supports pause/resume and retains pins across
restarts. The history limit includes pins, but pins are never evicted even when
they exceed the limit. Copy operations read the original payload rather than
the preview. wl-clipboard remains the clipboard ownership helper.

`desktop-search.service` owns the backend. Native clients use
`$XDG_RUNTIME_DIR/desktop-search/search.sock`; a compatibility socket at the old
provider path supports older clients during a transition. Both JSON and Protobuf
requests are supported. Query workers coalesce pending requests per connection
and cancel obsolete work. Helper output, payload sizes and operation times have
explicit limits.

Configuration is in `desktop-search/config.toml` below the XDG configuration
directory. `DESKTOP_SEARCH_CONFIG_FILE` can select a different configuration
without changing other applications' XDG paths. Editor saves create a new history
entry and retain the original. Private history, original payloads and task state live in
`$XDG_STATE_HOME/desktop-search`, defaulting to `~/.local/state/desktop-search`.
Directories use mode 0700 and stored files use mode 0600. This directory belongs
in a private backup; it must never be added to configuration source or the Nix
store. The SQLite cache is bounded independently of history size.

The `import-elephant` CLI operation requires the old provider service to remain
available. It imports clipboard text, images, pins, saved calculator results and
the local task CSV. It preserves original payloads, leaves the old state in
place, and saves a private migration backup and counts. It temporarily selects
the combined clipboard view and restores the previous filter afterward. Start
the new collector before import, verify the transfer, then retire the old
service. Import is never performed automatically on startup.

Nix builds the executable and supplies runtime dependencies. On other systems,
the portable launcher uses `desktop-search-build` and the shared build helper;
source snapshots and binaries stay in the XDG cache. The bar connects directly to the native search socket for additional providers.

Rust tests cover storage, ordering, pins, limits, scheduling, wire formats and
helper cancellation. `tests/test_desktop_search.py` exercises private sockets,
both encodings, provider actions, import and restart persistence. The bar's
private Wayland harness also accepts a search binary for clipboard capture,
burst retention, native copy and provider-menu acceptance.
