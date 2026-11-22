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

-- PROOF
table.insert(
  snippets,
  s(
    "pf",
    fmt(
      [[
        \begin{proof}
            <>
        \end{proof}
        ]],
      { i(1) },
      { delimiters = "<>" }
    ),
    { condition = cond.not_in_mathzone }
  )
)

table.insert(snippets, s({ trig = "nn" }, { t("\\nonumber") }, { condition = cond.in_mathzone }))

table.insert(
  snippets,
  s(
    { trig = "bmat", snippetType = "autosnippet" },
    { t({ [[\begin{bmatrix}]], "  " }), i(1), t({ "", [[\end{bmatrix}]] }) },
    { condition = cond.in_mathzone }
  )
)

table.insert(
  snippets,
  s(
    {
      trig = "array",
      snippetType = "autosnippet",
      resolveExpandParams = function(_, line)
        if not (line:match([[\begin{array$]]) or line:match([[\end{array$]])) then
          return {}
        end
      end,
    },
    { t([[\begin{array}{]]), i(1, "c"), t({ "}", "  " }), i(2), t({ "", [[\end{array}]] }) },
    { condition = cond.in_mathzone }
  )
)
