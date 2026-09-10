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

function M.cycle_monitor_scale(direction)
    local monitor = hl.get_active_monitor()
    if not monitor then
        return
    end
    local scales = { 1, 1.25, 1.6, 2, 3, 4 }
    local closest = 1
    for i = 2, #scales do
        if math.abs(monitor.scale - scales[i]) < math.abs(monitor.scale - scales[closest]) then
            closest = i
        end
    end
    local scale = scales[(closest - 1 + direction) % #scales + 1]
    hl.monitor({
        output = monitor.name,
        mode = string.format("%dx%d@%s", monitor.width, monitor.height, monitor.refresh_rate),
        position = "auto",
        scale = scale,
    })
    hl.exec_cmd('notify-send -u low "󰍹    Display scaling set to ' .. scale .. 'x"')
end

return M
