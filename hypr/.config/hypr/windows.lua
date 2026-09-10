-- Power behavior
hl.config({
    misc = {
        mouse_move_enables_dpms = true,
        key_press_enables_dpms = true,
    },
})

-- Window rules migrated from omarchy-supplement/hypr/hyprland-overrides.conf
hl.window_rule({
    match = { class = "^(Stretchly)$" },
    name = "stretchly-giant",
    float = true,
    monitor = "DP-2",
    move = { 0, 0 },
    size = { 1920, 1080 },
    pin = true,
})

hl.window_rule({
    match = { class = "^global\\.snip$" },
    name = "global-snip",
    float = true,
    size = { "(monitor_w*0.9)", "(monitor_h*0.6)" },
    center = true,
})

hl.window_rule({
    match = { class = "^macro\\.picker$" },
    name = "macro-picker",
    float = true,
    size = { "(monitor_w*0.9)", "(monitor_h*0.5)" },
    center = true,
})

hl.window_rule({
    match = { class = "^pdf\\.picker$" },
    name = "pdf-picker",
    float = true,
    size = { "(monitor_w*0.78)", "(monitor_h*0.58)" },
    center = true,
})

-- Keep the dictation overlay visible without stealing keyboard focus.
-- The tkinter overlay maps as class "Tk" / title "Dictate".
hl.window_rule({
    match = { class = "^(Tk)$", title = "^(Dictate)$" },
    name = "dictate-overlay",
    no_initial_focus = true,
    focus_on_activate = false,
})

hl.window_rule({
    match = { class = "(waypaper)" },
    name = "waypaper-floating",
    float = true,
    size = { 850, 400 },
    center = true,
    border_color = { colors = { "rgba(89b4faff)", "rgba(f5c2e7ff)" }, angle = 45 },
})

hl.window_rule({ match = { class = "(waypaper)" }, name = "waypaper-pinned", pin = true })

hl.window_rule({ match = { class = "(waypaper)" }, name = "waypaper-opacity", opacity = "0.7 0.6" })

-- Toggled globally with SUPER + ALT + T.
local opaque_rule = hl.window_rule({ match = { class = ".*" }, name = "global-opaque", opaque = true, enabled = false })

return { opaque_rule = opaque_rule }
