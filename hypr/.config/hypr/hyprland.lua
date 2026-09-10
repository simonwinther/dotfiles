-- Lua copies of the Omarchy defaults, followed by personal overrides.
require("defaults.autostart")
require("defaults.bindings.media")
require("defaults.bindings.clipboard")
require("defaults.bindings.tiling-v2")
require("defaults.bindings.utilities")
require("defaults.envs")
require("defaults.looknfeel")
require("defaults.input")
require("defaults.windows")

-- Omarchy still writes theme and toggle data in its own configuration format.
local config_data = require("config-data")
config_data.apply("~/.config/omarchy/current/theme/hyprland.conf")

require("monitors")
require("input")
require("bindings")
require("looknfeel")
require("autostart")
require("windows")

config_data.toggles()

-- Keep these overrides last so their unbinds take precedence.
require("hyprspace")
require("background-grid")
