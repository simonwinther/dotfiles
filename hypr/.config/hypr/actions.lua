local M = {}

function M.close_all_windows()
    for _, window in ipairs(hl.get_windows()) do
        hl.dispatch(hl.dsp.window.close({ window = window }))
    end
    hl.dispatch(hl.dsp.focus({ workspace = "1" }))
end

function M.pop_window()
    local window = hl.get_active_window()
    if not window then
        return
    end
    if window.pinned then
        hl.dispatch(hl.dsp.window.pin({ window = window }))
        hl.dispatch(hl.dsp.window.float({ window = window }))
        hl.dispatch(hl.dsp.window.tag({ window = window, tag = "-pop" }))
    else
        hl.dispatch(hl.dsp.window.float({ window = window }))
        hl.dispatch(hl.dsp.window.resize({ window = window, x = 1300, y = 900 }))
        hl.dispatch(hl.dsp.window.center({ window = window }))
        hl.dispatch(hl.dsp.window.pin({ window = window }))
        hl.dispatch(hl.dsp.window.bring_to_top({ window = window }))
        hl.dispatch(hl.dsp.window.tag({ window = window, tag = "+pop" }))
    end
end

-- Window names: labels on the desktop (window-labels) and badges in the
-- hyprspace overview. The running window-labels instance is the state.
function M.window_names_shown()
    local status = io.popen("window-labels status 2>/dev/null")
    if not status then
        return false
    end
    local shown = status:read("*l") == "on"
    status:close()
    return shown
end

function M.toggle_window_names()
    local show = not M.window_names_shown()
    if hl.get_config("plugin.hyprspace.overview.window_titles") ~= nil then
        hl.config({ plugin = { hyprspace = { overview = { window_titles = show } } } })
    end
    hl.exec_cmd(show and "uwsm-app -- window-labels start" or "window-labels stop")
end

function M.toggle_blur()
    local enabled = not hl.get_config("decoration.blur.enabled")
    hl.config({ decoration = { blur = { enabled = enabled } } })
    hl.exec_cmd('notify-send "Blur ' .. (enabled and "enabled" or "disabled") .. '"')
end

function M.toggle_transparency()
    local mode = require("windows").toggle_transparency()
    hl.exec_cmd(
        'notify-send -t 1500 -h string:x-canonical-private-synchronous:global-transparency "Transparency '
            .. mode
            .. ' globally"'
    )
end

function M.cycle_monitor_scale(_direction)
    hl.exec_cmd("desktop-ring menu")
end

return M
