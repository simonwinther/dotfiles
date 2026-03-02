local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t, i, fmt = C.P.s, C.P.t, C.P.i, C.P.fmt
local cond = C.cond
local snippets = C.snippets

-- ==========================================
-- Auto Pairs: Left and Right Together
-- ==========================================
-- @s SET / BRACES
table.insert(
  snippets,
  s(
    { trig = "@s", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\{ <> \\right\\}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @pa PARENTHESES
table.insert(
  snippets,
  s(
    { trig = "@p", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left( <> \\right)", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @br BRACKETS
table.insert(
  snippets,
  s(
    { trig = "@b", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left[ <> \\right]", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @c CURLY BRACES
table.insert(
  snippets,
  s(
    { trig = "@c", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\{ <> \\right\\}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @a ANGLE BRACKETS
table.insert(
  snippets,
  s(
    { trig = "@a", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\langle <> \\right\\rangle", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- ==========================================
-- Half Pairs Left and Right Standalone
-- ==========================================
-- Parentheses
table.insert(
  snippets,
  s({ trig = "@lp", snippetType = "autosnippet", wordTrig = false }, { t("\\left(") }, { condition = cond.in_mathzone })
)
table.insert(
  snippets,
  s(
    { trig = "@rp", snippetType = "autosnippet", wordTrig = false },
    { t("\\right)") },
    { condition = cond.in_mathzone }
  )
)

-- Brackets
table.insert(
  snippets,
  s({ trig = "@lb", snippetType = "autosnippet", wordTrig = false }, { t("\\left[") }, { condition = cond.in_mathzone })
)
table.insert(
  snippets,
  s(
    { trig = "@rb", snippetType = "autosnippet", wordTrig = false },
    { t("\\right]") },
    { condition = cond.in_mathzone }
  )
)

-- Curly Braces
table.insert(
  snippets,
  s(
    { trig = "@lc", snippetType = "autosnippet", wordTrig = false },
    { t("\\left\\{") },
    { condition = cond.in_mathzone }
  )
)

table.insert(
  snippets,
  s(
    { trig = "@rc", snippetType = "autosnippet", wordTrig = false },
    { t("\\right\\}") },
    { condition = cond.in_mathzone }
  )
)

-- ==========================================
-- Generic Left and Right
-- ==========================================

-- @l Left
table.insert(
  snippets,
  s(
    { trig = "@le", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left<>", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @r Right
table.insert(
  snippets,
  s(
    { trig = "@ri", snippetType = "autosnippet", wordTrig = false },
    fmt("\\right<>", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)
