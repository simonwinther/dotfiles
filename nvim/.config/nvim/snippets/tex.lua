local dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h") .. "/tex/"

local P = loadfile(dir .. "preamble.lua")("_load")
local cond = loadfile(dir .. "conditions.lua")("_load")
local helpers = loadfile(dir .. "helpers.lua")("_load")

local snippets = {}
local ctx = { P = P, cond = cond, helpers = helpers, snippets = snippets }

loadfile(dir .. "sections.lua")(ctx)
loadfile(dir .. "lists.lua")(ctx)
loadfile(dir .. "environments.lua")(ctx)
loadfile(dir .. "delimiters.lua")(ctx)
loadfile(dir .. "operators.lua")(ctx)
loadfile(dir .. "letters.lua")(ctx)
loadfile(dir .. "fonts.lua")(ctx)
loadfile(dir .. "spacing.lua")(ctx)
loadfile(dir .. "scripts.lua")(ctx)

return snippets
