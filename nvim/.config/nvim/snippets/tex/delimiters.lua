local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, i, fmt = C.P.s, C.P.i, C.P.fmt
local cond = C.cond
local snippets = C.snippets

-- SET
table.insert(
  snippets,
  s(
    { trig = "set", snippetType = "autosnippet" },
    fmt(
      [[
      \left\{ <> \right\}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.in_mathzone }
  )
)

-- PARENTHESES: \left( ... \right)
table.insert(
  snippets,
  s(
    { trig = "prn", snippetType = "autosnippet" },
    fmt(
      [[
      \left( <> \right)
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.in_mathzone }
  )
)

-- BRACKETS: \left[ ... \right]
table.insert(
  snippets,
  s(
    { trig = "brk", snippetType = "autosnippet" },
    fmt(
      [[
      \left[ <> \right]
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.in_mathzone }
  )
)

-- @s SET / BRACES
table.insert(
  snippets,
  s(
    { trig = "@s", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left\\{ <> \\right\\}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @p PARENTHESES
table.insert(
  snippets,
  s(
    { trig = "@p", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left( <> \\right)", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- @b BRACKETS
table.insert(
  snippets,
  s(
    { trig = "@b", snippetType = "autosnippet", wordTrig = false },
    fmt("\\left[ <> \\right]", { i(1) }, { delimiters = "<>" }),
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
