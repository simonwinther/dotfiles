hl.plugin.load(os.getenv("HOME") .. "/.local/share/hyprspace/hyprspace.so")

hl.bind("SUPER + A", function()
    return hl.plugin.hyprspace.overview()
end)
hl.unbind("SUPER + L")
hl.bind("SUPER + L", function()
    return hl.plugin.hyprspace.layoutcycle()
end)
hl.unbind("ALT + TAB")
hl.unbind("ALT + SHIFT + TAB")
hl.bind("ALT + TAB", function()
    return hl.plugin.hyprspace.switch()
end)
hl.bind("ALT + SHIFT + TAB", function()
    return hl.plugin.hyprspace.switch("prev")
end)

-- Plugins are loaded after the first config pass, then the config is reloaded.
if hl.get_config("plugin.hyprspace.follow_mouse") ~= nil then
    hl.config({
        plugin = {
            hyprspace = {
                follow_mouse = true,
                warp_cursor = true,
                overview = {
                    bg_dim = 0.80,
                    bg_color = "rgba(11111bff)",
                    tile_bg_color = "rgba(0d0d14d9)",
                    tile_border_color = "rgba(ffffff1a)",
                    padding = 56,
                    gap = 28,
                    rounding = 14,
                    border_size = 3,
                    active_border = "rgba(89b4faff)",
                    hover_border = "rgba(89b4fa80)",
                    workspace_labels = true,
                    label_color = "rgba(cdd6f4ff)",
                    title_bg_color = "rgba(1e1e2ee6)",
                    include_special = true,
                    all_monitors = true,
                    font = "Sans 12",
                    fullscreen_border = "rgba(89b4faff)",
                },
                switcher = {
                    icon_size = 96,
                    padding = 24,
                    gap = 12,
                    rounding = 20,
                    bg_color = "rgba(1e1e2ef0)",
                    highlight_color = "rgba(89b4fa40)",
                    text_color = "rgba(cdd6f4ff)",
                    show_title = true,
                    current_workspace_only = true,
                    font = "Sans 13",
                },
            },
        },
    })
end
