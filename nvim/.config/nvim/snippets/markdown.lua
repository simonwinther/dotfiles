local ls = require("luasnip")
local s = ls.snippet
local t = ls.text_node
local i = ls.insert_node

return {
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
