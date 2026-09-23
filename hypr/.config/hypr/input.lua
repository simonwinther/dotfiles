-- Control your input devices
-- See https://wiki.hypr.land/Configuring/Variables/#input
hl.config({
    input = {
        -- Use multiple keyboard layouts and switch between them with Left Alt + Right Alt
        -- kb_layout = "us,dk,eu",
        kb_layout = "dk",
        kb_variant = "nodeadkeys",
        kb_options = "caps:swapescape",
        -- Change speed of keyboard repeat
        repeat_rate = 50,
        repeat_delay = 250,
        -- Start with numlock on by default
        numlock_by_default = true,
        -- Increase sensitivity for mouse/trackpad (default: 0)
        sensitivity = 0.5,
        touchpad = {
            -- Use natural (inverse) scrolling
            natural_scroll = true,
            -- Use two-finger clicks for right-click instead of lower-right corner
            -- clickfinger_behavior = true,
            -- Control the speed of your scrolling
            scroll_factor = 0.4,
            -- Enable the touchpad while typing
            -- disable_while_typing = false,
            -- Left-click-and-drag with three fingers
            -- drag_3fg = 1,
        },
    },
})

-- Scroll nicely in the terminal
hl.window_rule({ match = { class = "com.mitchellh.ghostty" }, scroll_touchpad = 0.2 })

-- Enable touchpad gestures for changing workspaces
-- See https://wiki.hyprland.org/Configuring/Gestures/
hl.gesture({ fingers = 3, direction = "horizontal", action = "workspace" })

-- Discover touchpads on this machine instead of hard-coding a device name.
-- Reload Hyprland after connecting a new external touchpad.
local touchpads = {}
local devices = io.popen("udevadm info --export-db")
if devices then
    local database = devices:read("*a")
    devices:close()
    for entry in (database .. "\n\n"):gmatch("(.-)\n\n") do
        local path = entry:match("^P: ([^\n]+)")
        if path and path:match("/event%d+$") and (entry .. "\n"):find("\nE: ID_INPUT_TOUCHPAD=1\n", 1, true) then
            local file = io.open("/sys" .. path .. "/device/name", "r")
            if file then
                local name = file:read("*l")
                file:close()
                if name then
                    -- Match Hyprland's device-name normalization.
                    table.insert(touchpads, (name:lower():gsub("[ ,\n]", "-")))
                end
            end
        end
    end
end

-- Consume three-finger tap middle-clicks so missed swipes cannot paste.
-- Keep one-/two-finger taps and middle buttons on other devices available.
if #touchpads > 0 then
    hl.bind("mouse:274", function() end, {
        description = "Block touchpad middle-click paste",
        ignore_mods = true,
        device = { inclusive = true, list = touchpads },
    })
end
