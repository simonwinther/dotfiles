local dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h") .. "/tex/"

local P = loadfile(dir .. "preamble.lua")("_load")
local cond = loadfile(dir .. "conditions.lua")("_load")
local helpers = loadfile(dir .. "helpers.lua")("_load")

local snippets = {}
local math_snippets = {}
if ... == "markdown_math" then
  local make_snippet = P.s
  P.s = function(context, nodes, opts)
    local snippet = make_snippet(context, nodes, opts)
    if opts and (opts.condition == cond.in_mathzone or opts.condition == cond.in_plain_mathzone) then
      table.insert(math_snippets, snippet)
    end
    return snippet
  end
end
local ctx = { P = P, cond = cond, helpers = helpers, snippets = snippets }

loadfile(dir .. "templates.lua")(ctx)
loadfile(dir .. "sections.lua")(ctx)
loadfile(dir .. "lists.lua")(ctx)
loadfile(dir .. "environments.lua")(ctx)
loadfile(dir .. "delimiters.lua")(ctx)
loadfile(dir .. "operators.lua")(ctx)
loadfile(dir .. "letters.lua")(ctx)
loadfile(dir .. "fonts.lua")(ctx)
loadfile(dir .. "spacing.lua")(ctx)
loadfile(dir .. "scripts.lua")(ctx)

return ... == "markdown_math" and math_snippets or snippets
