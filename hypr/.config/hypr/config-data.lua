-- Read data files written by the installed Omarchy theme and toggle commands.
-- Hyprland configuration, bindings, and rules live in the Lua modules.
local M = {}

local function trim(value)
    return value:match("^%s*(.-)%s*$")
end

local function expand(path)
    return (path:gsub("^~", os.getenv("HOME")))
end

local function scalar(value)
    if value == "true" or value == "yes" or value == "on" then
        return true
    end
    if value == "false" or value == "no" or value == "off" then
        return false
    end
    return tonumber(value) or value
end

local function config_value(value)
    local colors = {}
    for color in value:gmatch("rgba?%([^)]*%)") do
        colors[#colors + 1] = color
    end
    if #colors > 1 or value:match("deg$") then
        return { colors = colors, angle = tonumber(value:match("([%d.-]+)deg")) or 0 }
    end
    local x, y = value:match("^([%d.-]+)%s+([%d.-]+)$")
    if x and y then
        return { tonumber(x), tonumber(y) }
    end
    return scalar(value)
end

function M.environment(path)
    local file = io.open(expand(path))
    if not file then
        return
    end
    for line in file:lines() do
        local name, value = line:match("^%s*env%s*=%s*([^,]+),%s*(.*)$")
        if name then
            hl.env(trim(name), (trim(value):gsub("##", "#")))
        end
    end
    file:close()
end

function M.apply(path)
    local file = io.open(expand(path))
    if not file then
        return
    end
    local sections, config, device = {}, {}, nil
    for raw in file:lines() do
        local line = trim((raw:gsub("#.*$", "")))
        local section = line:match("^([%w_.]+)%s*{$")
        if section then
            sections[#sections + 1] = section
            if section == "device" then
                device = {}
            end
        elseif line == "}" then
            if sections[#sections] == "device" then
                hl.device(device)
                device = nil
            end
            sections[#sections] = nil
        elseif line ~= "" then
            local key, value = line:match("^([^=]+)=%s*(.*)$")
            assert(key, "Unsupported Omarchy data in " .. path .. ": " .. line)
            key, value = trim(key), trim(value)
            if key == "monitor" then
                local parts = {}
                for part in (value .. ","):gmatch("(.-),") do
                    parts[#parts + 1] = trim(part)
                end
                local monitor = { output = parts[1] }
                if parts[2] == "disable" then
                    monitor.disabled = true
                else
                    monitor.mode, monitor.position, monitor.scale = parts[2], parts[3], scalar(parts[4])
                    for i = 5, #parts, 2 do
                        monitor[parts[i]] = scalar(parts[i + 1])
                    end
                end
                hl.monitor(monitor)
            elseif device then
                device[key] = config_value(value)
            else
                local prefix = #sections > 0 and table.concat(sections, ".") .. "." or ""
                config[prefix .. key:gsub(":", ".")] = config_value(value)
            end
        end
    end
    file:close()
    hl.config(config)
end

function M.toggles()
    local files = assert(
        io.popen(
            'for file in "$HOME"/.local/state/omarchy/toggles/hypr/*.conf; do test ! -f "$file" || printf "%s\\n" "$file"; done'
        )
    )
    for path in files:lines() do
        M.apply(path)
    end
    files:close()
end

return M
