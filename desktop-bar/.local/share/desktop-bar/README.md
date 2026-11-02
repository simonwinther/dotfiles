# Morphing Notch Bar

A native Rust Wayland bar and menu surface, shipped as `desktop-bar`. One process
renders the bar on each selected output. Clean uses rounded groups; Motion uses
a compact notch attached to the top edge. Both styles open applications, desktop
controls, notifications, keybindings, clipboard history and the desktop menu tree
in this renderer. Walker is no longer required or started. On NixOS, menu shortcuts call the
packaged Rust executable directly.

The tenth workspace is labeled `0` while retaining workspace ID 10. Motion's
compact controls use full-height targets and assign the space between icons to
the nearest control without enlarging the notch.

Nucleo supplies fuzzy matching in process. The warmed application catalog includes
desktop controls: `bluec` finds Bluetooth controls. The Go menu also searches those
controls while preserving its normal top-level rows when the search is empty.
Large catalogs and provider queries run on a worker with one pending request;
new typing supersedes stale work. Idle matching workers sleep.

Launcher prefixes are `/` for modes, `.` for files, `:` for symbols, `=` for the
calculator, `@` for web search, `>` for commands, `$` for clipboard and `gh ` for
repositories. Named modes also include Unicode and tasks. Ctrl+O opens item and
provider actions, including running commands in a terminal, calculator history
and task creation. Ctrl+Y activates a result while keeping the picker open.

Ctrl+J/K moves down/up, Ctrl+D/U moves half a page, and Enter opens the selected
result. Dashboard and clipboard navigation also support plain hjkl. In the
clipboard grid Ctrl+H/J/K/L also navigates, and `/` or `i` enters search. The
dashboard retains `/` or Ctrl+L for search. `?` opens help for the current page. Escape returns through the
menu stack before closing the root. Super+Ctrl+D toggles the dashboard, and the
existing hover interaction remains available.

Command-line selection dialogs use `desktop-bar --dmenu` over a private local
request/response socket. Input rows and responses are bounded and remain literal;
cancellation returns status 130. If the bar is stopped or hidden, the controller
opens a temporary native menu process that exits when dismissed, without changing
the hidden-bar preference. Applications selected by menus still launch in their
independent application scopes.

The default is Clean. Retired selections fall back to Clean. Hidden bars stay
hidden, and changing styles reloads the running process with SIGUSR2.
Theme and font changes use the same reload path.

Clean always shows workspaces 1–10 and keeps Wi-Fi to an icon, with connection
details in its tooltip. Its native menus open in the center of the focused output,
including above fullscreen applications. CPU opens Mission Center. Clean volume opens the configured audio application;
Motion volume opens output and microphone sliders. Right clicking volume toggles
mute. Microphone clicks toggle its mute state. Scrolling changes output volume by
10% or microphone volume by 5%. Clean clock clicks toggle the date format; Motion
clock opens a month calendar with previous/next navigation. Controls also exposes
audio, calendar, battery and system panels. Ctrl+Space and Super+Space open the
selected style’s launcher. Tab cycles panels, arrows select results and Enter applies the selection. Escape
returns to the parent, and outside clicks dismiss the complete stack. Tray items support activation, secondary activation,
scrolling, context menus, submenus, separators, disabled entries, and toggles.
Tray icons load from SNI pixmaps or PNG/SVG icon theme files. Symbolic icons
follow the foreground color without recoloring application icons.

Hovering Motion opens a panel after 70 ms. Volume and microphone open Sound,
battery opens Battery, Wi-Fi opens connection status, and the clock opens Calendar. The rest of the notch opens
the full controls dashboard. Its compact body keeps numbered workspaces, clock, quick sound, Wi-Fi, battery
and the application tray visible. A pending
hover follows the current target without scheduling a timer for every pointer
movement. Moving into an expanded panel keeps it open; leaving gives a 130 ms
grace period before it springs closed. When a click inside resizes the panel, the
area it covered at that click keeps receiving the pointer until the pointer leaves
it, so a resting cursor neither closes the panel nor clicks outside it.
Hover does not acquire keyboard focus. Clicking or using a launcher shortcut lets
the panel receive keyboard input. Escape and outside clicks dismiss it immediately
and prevent the same pointer position from reopening it.

Wi-Fi uses the existing iwd system service for connection changes. Signal strength
comes from the kernel wireless statistics during the existing metrics update.
The connection panel opens the configured network settings application.
Mouse wheels use their discrete or high resolution steps; continuous touchpad
deltas accumulate before advancing a row or card. Stop frames do not move the
selection, and pointer motion cannot immediately undo a scroll selection.
Partial steps survive pauses until the gesture ends. Scrolling over Sound's
output or microphone slider adjusts that channel, including after hover opens it.

Alt+V and the Clipboard tab open history inside either style. The Rust
`desktop-search` daemon collects history; the bar shares the clipboard picker's
Rust history and guarded paste backend without loading GTK. Search filters the
four-column grid. In navigation mode, arrows or hjkl move, Space selects in order,
and p previews immediately. Ctrl+Tab changes All, Images and Text. Enter pastes,
Ctrl+Enter copies, and Ctrl+Shift+Enter copies image file links. Ctrl+P pins or
unpins, Delete removes an unpinned item, and Ctrl+R refreshes. Batch paste checks
the destination before every item and stops if focus changes. Closing releases
decoded images from memory.

Motion's transparent layer keeps a fixed envelope at the monitor's top edge.
The compact bar and expanded content draw into the same persistent surface, so
the compositor does not animate or position a separate popup. The input region
follows the painted shape; the surrounding transparent area accepts no clicks. Width and height use
exact damped spring solutions with preserved velocity when retargeted; the height
has a restrained overshoot. Status glyphs fade without painting a second body. Content clips to the moving notch and fades in as it opens.
Both views share one pending frame callback, paced by the compositor; settled
panels stop requesting them. Each frame clears and damages only the previous and
current notch footprint, and the compact bar retains only its top band in memory;
lower buffer rows stay transparent. Frames reach the compositor without channel
swizzling when it offers ABGR8888 buffers. Dismissal releases
keyboard and pointer focus immediately, including during a closing reversal.

Catalogs load away from rendering. Applications follow XDG desktop-file precedence
and visibility and launch through GIO. Wallpaper thumbnails are generated on
demand, capped at 320×180, and cached under `$XDG_CACHE_HOME/desktop/bar-thumbnails`.
The bar decodes an existing cached thumbnail directly and starts desktopctl only to
generate a missing one.
Only the current visible set stays in memory. Image requests supersede obsolete
selections and reuse decoded previews while navigating, so rapid browsing does
not build a queue of old conversions. Theme and wallpaper selections use the
existing desktop controller. Static groups from `desktop/menus.json`, dynamic settings and selection dialogs
open inside the same native surface.

Add actions to `$XDG_CONFIG_HOME/desktop/bar-actions.json` as a JSON array. Each
action has `name`, optional `detail` and `keywords`, and a `command` argument list
for desktopctl. For example, an action can use `["launch", "tui", "btop"]` or
`["panel", "volume"]`. Native panel names are apps, themes, wallpapers, clipboard,
styles, controls, projects, volume, calendar, battery, network and system. The
`desktopctl bar panel NAME` and `desktop-bar --panel NAME` interfaces open them
without restarting the bar.

Projects lists the browser-maintained ChatGPT projects through `project-picker --catalog`
and opens the selected one with `project-picker --open`. The first row shows the
workspace and sync state, and the page refreshes every second while it is open. With
the Motion style, the project picker shortcut opens this page; Clean keeps the
terminal picker.

Opening a project is tuned for speed, and the RAM use is deliberate:

1. The bar and picker start the browser directly instead of through desktopctl,
   saving about 0.2 s.
2. Chromium stays running without windows (`desktop-browser.service`, about 650 MB
   idle). A cold start takes 2-3 s before ChatGPT even begins loading; a running
   browser opens a window in about 0.6 s. The project-picker extension's
   `background` permission keeps Chromium alive after its last window closes;
   Chromium's "Continue running background apps" setting must stay on; on NixOS
   the `BackgroundModeEnabled` policy enforces it.
3. The extension keeps a native connection open, and `project-picker --open` asks
   it to show the project in the open ChatGPT app window, which skips loading
   ChatGPT again. Without such a window, a new app window opens.

Runtime settings come from `$XDG_RUNTIME_DIR/desktop/bar-theme.json`, published by
desktopctl. It contains the selected style, palette, light flag, desktop font, UI
scale, and full/minimal output lists. Fractional output scale is independent of UI
scale. Icon glyphs use Symbols Nerd Font Mono; fonts are memory mapped and glyph
coverage is cached on demand.

The portable build helper snapshots this crate including its weather library, shared
clipboard core and Pulse dependency into the user cache before compiling. Build output stays outside the
checkout. The Nix derivation uses the same source packages and lockfile.

Weather settings live in `~/.config/desktop/weather.json`; the API key stays in
`~/.owm-key`. `desktopctl weather` prints the current reading through the same
Rust application, and accepts `--refresh` to bypass a fresh cache.

Offline previews use the `--dump FILE SCALE clean|motion` arguments, with optional
`--light` or `--panel volume|calendar|battery`. The `--preview` option places a
nonexclusive bar below the desktop bar. The repository's private compositor smoke
test accepts the built binary and the Hyprspace headless display/input fixtures;
it exercises style reload in the same process, search and launch, gallery
selection, popup dismissal and reopening, fractional scale, minimal outputs,
and a minute of idle memory sampling without connecting to desktop sockets.
Its `--hyprspace` option exercises active-space metadata and workspace commands.

Rendering and protocol work run on calloop. Sources update on compositor, audio,
and tray events, minute boundaries, or coarse CPU/memory/disk/battery/weather
intervals. Settled views request no animation frames. Status samples that do not
change what the compact notch draws request no frame. Tray items keep their decoded
icon until its icon properties change, and tooltip signals are ignored because hover
text comes from the title. Live pages refresh every second only while one is shown,
and unchanged refreshes repaint nothing. The memory target is below
10 MB RSS at steady state; measure optimized builds, including the intended
output count and scales, before treating that target as accepted.
