local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, i, t, fmt = C.P.s, C.P.i, C.P.t, C.P.fmt
local cond = C.cond
local snippets = C.snippets

-- EQUATION
table.insert(
  snippets,
  s(
    "eq",
    fmt(
      [[
      \begin{equation}
          <>
      \end{equation}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)

-- EQUATION*
table.insert(
  snippets,
  s(
    { trig = "eqq", wordTrig = false },
    fmt(
      [[
      \begin{equation*}
          <>
      \end{equation*}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)

-- ALIGN (Numbered)
table.insert(
  snippets,
  s(
    "al",
    fmt(
      [[
      \begin{align}
          <>
      \end{align}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)

-- ALIGN*
table.insert(
  snippets,
  s(
    "all",
    fmt(
      [[
      \begin{align*}
          <>
      \end{align*}
      ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)

table.insert(snippets, s({ trig = "nn" }, { t("\\nonumber") }, { condition = cond.in_mathzone }))
