local picker = {}

local function show(mode, present_only)
    local name = mode .. "-picker"
    local class = mode .. ".picker"
    local workspace = hl.get_active_workspace()
    -- Only one picker should be visible, even when switching shortcuts rapidly.
    for _, window in ipairs(hl.get_windows()) do
        local other = ({ ["file.picker"] = "file", ["pdf.picker"] = "pdf", ["recent.picker"] = "recent" })[window.class]
        if other and other ~= mode then
            hl.dispatch(hl.dsp.window.move({ window = window, workspace = "special:" .. other .. "-picker", follow = false }))
        end
    end
    local command = "file-picker --window" .. (mode == "pdf" and " --pdf" or mode == "recent" and " --recent" or "")
    local runtime = os.getenv("XDG_RUNTIME_DIR")
    local ready = runtime and io.open(runtime .. "/" .. name .. "-ready", "rb")
    if not ready and not present_only then
        hl.exec_cmd(command)
        return
    end
    if ready then
        ready:close()
    end
    -- Bring the prepared window forward directly, without spawning a launcher.
    for _, window in ipairs(hl.get_windows()) do
        if window.class == class then
            local monitor = hl.get_active_monitor()
            if monitor then
                hl.dispatch(hl.dsp.window.move({ window = window, monitor = monitor, follow = false }))
            end
            hl.dispatch(hl.dsp.window.move({ window = window, workspace = workspace, follow = false }))
            if monitor then
                hl.dispatch(hl.dsp.window.resize({
                    window = window,
                    x = math.floor(monitor.width / monitor.scale * 0.76 + 0.5),
                    y = math.floor(monitor.height / monitor.scale * 0.68 + 0.5),
                }))
            end
            hl.dispatch(hl.dsp.window.center({ window = window }))
            hl.dispatch(hl.dsp.focus({ window = window }))
            if not present_only then
                hl.dispatch(hl.dsp.send_shortcut({ mods = "CTRL", key = "R", window = window }))
            end
            return
        end
    end
    if not present_only then
        hl.exec_cmd(command)
    end
end

function picker.show(present_only)
    show("file", present_only)
end

function picker.show_pdf(present_only)
    show("pdf", present_only)
end

function picker.show_recent(present_only)
    show("recent", present_only)
end

function picker.dismiss()
    local window = hl.get_active_window()
    if not window or (window.class ~= "file.picker" and window.class ~= "pdf.picker" and window.class ~= "recent.picker") then
        return { ok = false }
    end
    local name = window.class == "recent.picker" and "recent-picker" or window.class == "pdf.picker" and "pdf-picker" or "file-picker"
    hl.dispatch(hl.dsp.window.move({ window = window, workspace = "special:" .. name, follow = false }))
    -- Cancel and reset the finder after hiding, without waiting for its cleanup.
    hl.dispatch(hl.dsp.send_shortcut({ mods = "", key = "Escape", window = window }))
    return { ok = true }
end

return picker
