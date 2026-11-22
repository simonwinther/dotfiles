local ls = require("luasnip")
local s = ls.snippet
local t = ls.text_node
local i = ls.insert_node

local dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h")
local conditions = loadfile(dir .. "/tex/conditions.lua")("_load")

local snippets = {
  s({ trig = "align", snippetType = "autosnippet" }, {
    t({ [[\begin{aligned}]], "  " }), i(1), t({ "", [[\end{aligned}]] }),
  }, { condition = conditions.in_display_math }),
  s({ trig = ",m", snippetType = "autosnippet", wordTrig = false }, { t("$"), i(1), t("$") }),
  s({ trig = ",M", snippetType = "autosnippet", wordTrig = false }, { t({ "$$", "" }), i(1), t({ "", "$$" }) }),
  s("daylog", {
    t({
      "# Day 1 — 2026-01-28",
      "",
      "## Focus (start)",
      "",
    }),
    i(1),
    t({
      "",
      "",
      "## Work done",
      "",
      "",
      "## Notes / observations",
      "",
      "",
      "## Next (for tomorrow)",
      "",
    }),
  }),
}

return vim.list_extend(snippets, loadfile(dir .. "/tex.lua")("markdown_math"))
