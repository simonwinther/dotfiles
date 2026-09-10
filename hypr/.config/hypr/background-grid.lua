-- Wallpaper grid: override the earlier Omarchy/Waypaper shortcut.
hl.unbind("SUPER + CTRL + SPACE")
hl.bind("SUPER + CTRL + SPACE", hl.dsp.exec_cmd("~/.local/bin/background-grid"), { description = "Wallpaper grid" })

hl.window_rule({
    match = { class = "^(com\\.simon\\.BackgroundGrid)$" },
    name = "background-grid",
    float = true,
    center = true,
    opacity = "1.0 override 1.0 override",
    no_blur = true,
    no_shadow = true,
    border_color = "rgba(464e68ff)",
})

-- Omarchy's anonymous opacity rules run after named rules.
hl.window_rule({ match = { class = "^(com\\.simon\\.BackgroundGrid)$" }, tag = "-default-opacity" })
hl.window_rule({ match = { class = "^(com\\.simon\\.BackgroundGrid)$" }, opacity = "1.0 override 1.0 override" })
