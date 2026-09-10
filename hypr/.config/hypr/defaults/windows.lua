-- See https://wiki.hypr.land/Configuring/Basics/Window-Rules/ for more
-- Hyprland 0.53+ syntax
hl.window_rule({ match = { class = ".*" }, suppress_event = "maximize" })

-- Tag all windows for default opacity (apps can override with -default-opacity tag)
hl.window_rule({ match = { class = ".*" }, tag = "+default-opacity" })

-- Fix some dragging issues with XWayland
hl.window_rule({
    match = { class = "^$", title = "^$", xwayland = true, float = true, fullscreen = false, pin = false },
    no_focus = true,
})

-- App-specific tweaks (may remove default-opacity tag)
-- App-specific tweaks
hl.window_rule({ match = { class = "^(1[p|P]assword)$" }, no_screen_share = true })
hl.window_rule({ match = { class = "^(1[p|P]assword)$" }, tag = "+floating-window" })
hl.window_rule({ match = { class = "^(Bitwarden)$" }, no_screen_share = true })
hl.window_rule({ match = { class = "^(Bitwarden)$" }, tag = "+floating-window" })

-- Bitwarden Chrome Extension
hl.window_rule({ match = { class = "chrome-nngceckbapebfimnlniiiahkandclblb-Default" }, no_screen_share = true })
hl.window_rule({ match = { class = "chrome-nngceckbapebfimnlniiiahkandclblb-Default" }, tag = "+floating-window" })
-- Browser types
hl.window_rule({
    match = { class = "((google-)?[cC]hrom(e|ium)|[bB]rave-browser|[mM]icrosoft-edge|Vivaldi-stable|helium)" },
    tag = "+chromium-based-browser",
})
hl.window_rule({ match = { class = "([fF]irefox|zen|librewolf)" }, tag = "+firefox-based-browser" })
hl.window_rule({ match = { tag = "chromium-based-browser" }, tag = "-default-opacity" })
hl.window_rule({ match = { tag = "firefox-based-browser" }, tag = "-default-opacity" })

-- Video apps: remove chromium browser tag so they don't get opacity applied
hl.window_rule({
    match = { class = "(chrome-youtube.com__-Default|chrome-app.zoom.us__wc_home-Default)" },
    tag = "-chromium-based-browser",
})
hl.window_rule({
    match = { class = "(chrome-youtube.com__-Default|chrome-app.zoom.us__wc_home-Default)" },
    tag = "-default-opacity",
})

-- Force chromium-based browsers into a tile to deal with --app bug
hl.window_rule({ match = { tag = "chromium-based-browser" }, tile = true })

-- Only a subtle opacity change, but not for video sites
hl.window_rule({ match = { tag = "chromium-based-browser" }, opacity = "1.0 0.985" })
hl.window_rule({ match = { tag = "firefox-based-browser" }, opacity = "1.0 0.985" })

-- Hide the screen-sharing notification bar (the "Hide" button on it is broken on Wayland)
hl.window_rule({ match = { title = ".*is sharing.*" }, workspace = "special silent" })
-- Remove 1px border around hyprshot screenshots
hl.layer_rule({ match = { namespace = "selection" }, no_anim = true })
-- Disable mouse focus (see https://github.com/basecamp/omarchy/pull/5183#issuecomment-4189299971)
hl.window_rule({ match = { class = "^(jetbrains-.*)$" }, name = "jetbrains-focus", no_follow_mouse = true })
-- Float LocalSend and fzf file picker
hl.window_rule({ match = { class = "(Share|localsend)" }, float = true })
hl.window_rule({ match = { class = "(Share|localsend)" }, center = true })
hl.window_rule({ match = { class = "localsend" }, size = { 1100, 700 } })
-- Picture-in-picture overlays
hl.window_rule({ match = { title = "(Picture.?in.?[Pp]icture)" }, tag = "+pip" })
hl.window_rule({ match = { tag = "pip" }, tag = "-default-opacity" })
hl.window_rule({ match = { tag = "pip" }, float = true })
hl.window_rule({ match = { tag = "pip" }, pin = true })
hl.window_rule({ match = { tag = "pip" }, size = { 600, 338 } })
hl.window_rule({ match = { tag = "pip" }, keep_aspect_ratio = true })
hl.window_rule({ match = { tag = "pip" }, border_size = 0 })
hl.window_rule({ match = { tag = "pip" }, opacity = "1 1" })
hl.window_rule({ match = { tag = "pip" }, move = { "(monitor_w-window_w-40)", "(monitor_h*0.04)" } })
hl.window_rule({ match = { class = "qemu" }, tag = "-default-opacity" })
hl.window_rule({ match = { class = "qemu" }, opacity = "1 1" })
hl.window_rule({ match = { class = "com.libretro.RetroArch" }, fullscreen = true })
hl.window_rule({ match = { class = "com.libretro.RetroArch" }, tag = "-default-opacity" })
hl.window_rule({ match = { class = "com.libretro.RetroArch" }, opacity = "1 1" })
hl.window_rule({ match = { class = "com.libretro.RetroArch" }, idle_inhibit = "fullscreen" })
-- Float Steam
hl.window_rule({ match = { class = "steam" }, float = true })
hl.window_rule({ match = { class = "steam", title = "Steam" }, center = true })
hl.window_rule({ match = { class = "steam.*" }, tag = "-default-opacity" })
hl.window_rule({ match = { class = "steam.*" }, opacity = "1 1" })
hl.window_rule({ match = { class = "steam", title = "Steam" }, size = { 1100, 700 } })
hl.window_rule({ match = { class = "steam", title = "Friends List" }, size = { 460, 800 } })
hl.window_rule({ match = { class = "steam" }, idle_inhibit = "fullscreen" })
hl.window_rule({ match = { class = "GeForceNOW" }, name = "geforce", idle_inhibit = "fullscreen" })
hl.window_rule({
    match = { class = "com.moonlight_stream.Moonlight" },
    name = "moonlight",
    fullscreen = true,
    idle_inhibit = "fullscreen",
})
-- Floating windows
hl.window_rule({ match = { tag = "floating-window" }, float = true })
hl.window_rule({ match = { tag = "floating-window" }, center = true })
hl.window_rule({ match = { tag = "floating-window" }, size = { 875, 600 } })

hl.window_rule({
    match = {
        class = "(org.omarchy.bluetui|org.omarchy.impala|org.omarchy.wiremix|org.omarchy.btop|org.omarchy.terminal|org.omarchy.bash|org.codeberg.dnkl.foot|org.gnome.NautilusPreviewer|org.gnome.Evince|com.gabm.satty|Omarchy|About|TUI.float|imv|mpv)",
    },
    tag = "+floating-window",
})
hl.window_rule({
    match = {
        class = "(xdg-desktop-portal-gtk|sublime_text|DesktopEditors|org.gnome.Nautilus)",
        title = "^(Open.*Files?|Open [F|f]older.*|Save.*Files?|Save.*As|Save|All Files|.*wants to [open|save].*|[C|c]hoose.*)",
    },
    tag = "+floating-window",
})
hl.window_rule({ match = { class = "org.gnome.Calculator" }, float = true })

-- Fullscreen screensaver
hl.window_rule({ match = { class = "org.omarchy.screensaver" }, fullscreen = true })
hl.window_rule({ match = { class = "org.omarchy.screensaver" }, float = true })
hl.window_rule({ match = { class = "org.omarchy.screensaver" }, animation = "slide" })

-- No transparency on media windows
hl.window_rule({
    match = {
        class = "^(zoom|vlc|mpv|org.kde.kdenlive|com.obsproject.Studio|com.github.PintaProject.Pinta|imv|org.gnome.NautilusPreviewer)$",
    },
    tag = "-default-opacity",
})
hl.window_rule({
    match = {
        class = "^(zoom|vlc|mpv|org.kde.kdenlive|com.obsproject.Studio|com.github.PintaProject.Pinta|imv|org.gnome.NautilusPreviewer)$",
    },
    opacity = "1 1",
})

-- Popped window rounding
hl.window_rule({ match = { tag = "pop" }, rounding = 8 })

-- Prevent idle while open
hl.window_rule({ match = { tag = "noidle" }, idle_inhibit = "always" })
-- Prevent Telegram from stealing focus on new messages
hl.window_rule({ match = { class = "org.telegram.desktop" }, focus_on_activate = false })
-- Float Typora print dialog
hl.window_rule({ match = { class = "^Typora$", title = "^Print$" }, float = true, center = true })
-- Define terminal tag to style them uniformly
hl.window_rule({ match = { class = "(Alacritty|kitty|com.mitchellh.ghostty|foot)" }, tag = "+terminal" })
hl.window_rule({ match = { tag = "terminal" }, tag = "-default-opacity" })
hl.window_rule({ match = { tag = "terminal" }, opacity = "0.985 0.96" })
-- Application-specific animation
hl.layer_rule({ match = { namespace = "walker" }, no_anim = true })
-- Webcam overlay for screen recording
hl.window_rule({ match = { title = "WebcamOverlay" }, float = true })
hl.window_rule({ match = { title = "WebcamOverlay" }, pin = true })
hl.window_rule({ match = { title = "WebcamOverlay" }, no_initial_focus = true })
hl.window_rule({ match = { title = "WebcamOverlay" }, no_dim = true })
hl.window_rule({ match = { title = "WebcamOverlay" }, move = { "(monitor_w-window_w-40)", "(monitor_h-window_h-40)" } })

-- Apply default opacity after apps have had a chance to opt out
hl.window_rule({ match = { tag = "default-opacity" }, opacity = "0.985 0.96" })
