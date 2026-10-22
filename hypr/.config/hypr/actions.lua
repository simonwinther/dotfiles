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
