-- Change the default Omarchy look'n'feel

-- https://wiki.hyprland.org/Configuring/Variables/#general
hl.config({
    general = {
        -- No gaps between windows or borders
        -- gaps_in = 0,
        -- gaps_out = 0,
        -- border_size = 0,
        -- Use master layout instead of dwindle
        -- layout = "master",
    },
})

-- https://wiki.hyprland.org/Configuring/Variables/#decoration
hl.config({
    decoration = {
        -- Use round window corners
        active_opacity = 0.85,
        inactive_opacity = 0.80,
        rounding = 10,
        blur = {
            enabled = true,
            size = 1,
            passes = 3,
            new_optimizations = true,
            ignore_opacity = true,
        },
    },
})

-- https://wiki.hypr.land/Configuring/Variables/#layout
hl.config({
    layout = {
        -- Avoid overly wide single-window layouts on wide screens
        -- single_window_aspect_ratio = { 1, 1 },
    },
})

hl.config({
    animations = {
        enabled = true,
    },
})
hl.curve("mybezier", { type = "bezier", points = { { 0.05, 0.9 }, { 0.1, 1.05 } } })
hl.animation({ leaf = "workspaces", enabled = true, speed = 6, bezier = "mybezier", style = "slidefade" })
