local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t, i, c, d, sn, fmt = C.P.s, C.P.t, C.P.i, C.P.c, C.P.d, C.P.sn, C.P.fmt
local cond = C.cond
local snippets = C.snippets

-- SUPERSCRIPT
table.insert(
  snippets,
  s(
    { trig = "^", snippetType = "autosnippet", wordTrig = false },
    fmt("^{{{}}}", { i(1) }),
    { condition = cond.in_mathzone }
  )
)

-- SUBSCRIPT
table.insert(
  snippets,
  s(
    { trig = "_", snippetType = "autosnippet", wordTrig = false },
    fmt("_{{{}}}", { i(1) }),
    { condition = cond.in_mathzone }
  )
)

-- FRACTION
table.insert(
  snippets,
  s(
    { trig = "/", snippetType = "autosnippet" },
    fmt("\\frac{<>}{<>}", { i(1), i(2) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

-- SUMMATION
table.insert(
  snippets,
  s(
    { trig = "sum", snippetType = "autosnippet" },
    c(1, {
      -- Full Limits
      fmt(
        [[
        \sum_{<>}^{<>}
        ]],
        { i(1, "i=0"), i(2, "\\infty") },
        { delimiters = "<>" }
      ),
      -- Subscript only
      fmt(
        [[
        \sum_{<>}
        ]],
        { i(1, "i") },
        { delimiters = "<>" }
      ),
      -- Superscript only
      fmt(
        [[
        \sum^{<>}
        ]],
        { i(1, "n") },
        { delimiters = "<>" }
      ),
      -- Bare
      fmt(
        [[
        \sum <>
        ]],
        { i(1, "x") },
        { delimiters = "<>" }
      ),
    }),
    { condition = cond.in_mathzone }
  )
)

-- PRODUCT
table.insert(
  snippets,
  s(
    { trig = "prod", snippetType = "autosnippet" },
    c(1, {
      -- Full Limits
      fmt(
        [[
        \prod_{<>}^{<>}
        ]],
        { i(1, "i=1"), i(2, "n") },
        { delimiters = "<>" }
      ),
      -- Subscript only
      fmt(
        [[
        \prod_{<>}
        ]],
        { i(1, "i") },
        { delimiters = "<>" }
      ),
      -- Bare
      fmt(
        [[
        \prod <>
        ]],
        { i(1, "x") },
        { delimiters = "<>" }
      ),
    }),
    { condition = cond.in_mathzone }
  )
)

-- INTEGRAL
table.insert(
  snippets,
  s(
    { trig = "int", snippetType = "autosnippet" },
    c(1, {
      -- Full Limits
      fmt(
        [[
        \int_{<>}^{<>}
        ]],
        { i(1, "a"), i(2, "b") },
        { delimiters = "<>" }
      ),
      -- Subscript only (e.g., over a domain D)
      fmt(
        [[
        \int_{<>}
        ]],
        { i(1, "\\Omega") },
        { delimiters = "<>" }
      ),
      -- Bare
      fmt(
        [[
        \int <>
        ]],
        { i(1, "f(x)") },
        { delimiters = "<>" }
      ),
    }),
    { condition = cond.in_mathzone }
  )
)

-- MULTIPLE \text{...} (e.g. 3tt)
table.insert(
  snippets,
  s(
    { trig = "(%d)tt", regTrig = true, snippetType = "autosnippet" },
    d(1, function(_, parent)
      local n = tonumber(parent.captures[1])
      local nodes = {}

      for j = 1, n do
        table.insert(nodes, t("\\text{"))
        table.insert(nodes, i(j))
        table.insert(nodes, t("}"))

        if j < n then
          table.insert(nodes, t(", "))
        end
      end

      return sn(nil, nodes)
    end),
    { condition = cond.in_mathzone }
  )
)
