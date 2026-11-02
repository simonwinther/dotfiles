-- Read data files written by the installed Desktop theme and toggle commands.
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

local function comma_values(value)
    local values = {}
    for part in (value .. ","):gmatch("(.-),") do
        values[#values + 1] = trim(part)
    end
    return values
end

local function directive(section, key, value)
    if section == "animations" and key == "bezier" then
        local parts = comma_values(value)
        hl.curve(parts[1], {
            type = "bezier",
            points = { { tonumber(parts[2]), tonumber(parts[3]) }, { tonumber(parts[4]), tonumber(parts[5]) } },
        })
    elseif section == "animations" and key == "animation" then
        local parts = comma_values(value)
        hl.animation({
            leaf = parts[1], enabled = parts[2] ~= "0" and scalar(parts[2]) ~= false,
            speed = tonumber(parts[3]), bezier = parts[4], style = parts[5],
        })
    elseif section == "gestures" and key == "gesture" then
        local parts = comma_values(value)
        local action = parts[3]
        if action == "dispatcher" then
            assert(parts[4] == "workspace", "Unsupported Desktop gesture dispatcher: " .. tostring(parts[4]))
            local dispatch = hl.dsp.focus({ workspace = parts[5] })
            action = function() return hl.dispatch(dispatch) end
        end
        hl.gesture({ fingers = tonumber(parts[1]), direction = parts[2], action = action })
    elseif section == nil and key == "layerrule" then
        local parts = comma_values(value)
        local rule = { match = { namespace = parts[2] } }
        if parts[1] == "ignorezero" then
            rule.ignore_alpha = 0
        else
            assert(parts[1] == "blur", "Unsupported Desktop layer rule: " .. parts[1])
            rule.blur = true
        end
        hl.layer_rule(rule)
    elseif section == nil and key == "windowrule" then
        local parts = comma_values(value)
        local effect, argument = parts[1]:match("^(%w+)%s+(.+)$")
        assert(effect == "opacity", "Unsupported Desktop window rule: " .. parts[1])
        local rule = { match = {}, opacity = argument }
        for i = 2, #parts do
            local property, match = parts[i]:match("^match:([%w_]+)%s+(.+)$")
            assert(property, "Unsupported Desktop window match: " .. parts[i])
            rule.match[property] = scalar(match)
        end
        hl.window_rule(rule)
    else
        return false
    end
    return true
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
    local variables = {}
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
            assert(key, "Unsupported Desktop data in " .. path .. ": " .. line)
            key, value = trim(key), trim(value)
            value = value:gsub("%$([%w_]+)", variables)
            if key:sub(1, 1) == "$" then
                variables[key:sub(2)] = value
            elseif directive(sections[#sections], key, value) then
                -- Curves, animations, gestures and rules have dedicated Lua APIs.
            elseif key == "monitor" then
                local parts = comma_values(value)
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
            'for file in "${XDG_STATE_HOME:-$HOME/.local/state}"/desktop/toggles/hypr/*.conf; do test ! -f "$file" || printf "%s\\n" "$file"; done'
        )
    )
    for path in files:lines() do
        if not path:match("/internal%-display.conf$") then M.apply(path) end
    end
    files:close()
end

return M
