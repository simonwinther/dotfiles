-- Configuration evaluation blocks the compositor: read a declared path
-- directly and leave only undeclared, portable lookups to desktopctl.
local function declared_plugin()
    local selected = os.getenv("DESKTOP_HYPRSPACE_BIN")
    if selected and selected ~= "" then return selected end
    local config = os.getenv("XDG_CONFIG_HOME") or (os.getenv("HOME") .. "/.config")
    local file = io.open(config .. "/desktop/paths.json", "r")
    if not file then return nil end
    local text = file:read("*a")
    file:close()
    selected = text:match('"hyprspace"%s*:%s*"([^"]*)"')
    if selected and selected ~= "" and not selected:find("\\", 1, true) then return selected end
end

local plugin = declared_plugin()
if plugin then
    -- Like desktopctl, reject a declared path that no longer exists.
    local file = io.open(plugin, "r")
    if file then file:close() else plugin = nil end
else
    local paths = io.popen("desktopctl path hyprspace 2>/dev/null")
    if paths then plugin = paths:read("*l"); paths:close() end
end
if not plugin or plugin == "" then plugin = os.getenv("HYPRSPACE_PLUGIN") end
if not plugin or plugin == "" then return end
hl.plugin.load(plugin)

hl.bind("SUPER + A", function()
    return hl.plugin.hyprspace.overview()
end)
-- Three-finger swipe up opens the workspace overview.
hl.gesture({
    fingers = 3,
    direction = "up",
    action = function()
        return hl.plugin.hyprspace.overview("on")
    end,
})

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
                -- Neutral greys with the desktop's own accent (the green of the active
                -- window border).
                overview = {
                    bg_dim = 0.80,
                    bg_color = "rgba(0a0a0bff)",
                    tile_bg_color = "rgba(111113d9)",
                    tile_border_color = "rgba(ffffff1a)",
                    padding = 56,
                    gap = 28,
                    rounding = 14,
                    border_size = 3,
                    active_border = "rgba(6bbf7aff)",
                    hover_border = "rgba(6bbf7a80)",
                    workspace_labels = true,
                    window_titles = require("actions").window_names_shown(),
                    label_color = "rgba(e4e4e4ff)",
                    title_bg_color = "rgba(141416e6)",
                    -- Include scratchpad and other special workspaces.
                    include_special = true,
                    exclude_workspaces = "",
                    all_monitors = true,
                    font = "Sans 12",
                    fullscreen_border = "rgba(6bbf7aff)",
                },
                switcher = {
                    icon_size = 96,
                    padding = 24,
                    gap = 12,
                    rounding = 20,
                    bg_color = "rgba(141416f0)",
                    highlight_color = "rgba(6bbf7a40)",
                    text_color = "rgba(e4e4e4ff)",
                    show_title = true,
                    current_workspace_only = true,
                    font = "Sans 13",
                },
            },
        },
    })
end
