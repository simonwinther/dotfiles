local C = ...
if type(C) ~= "table" or not C.snippets then return {} end

local s, i, f, fmt = C.P.s, C.P.i, C.P.f, C.P.fmt
local cond = C.cond
local snippets = C.snippets

-- TEXT
table.insert(
  snippets,
  s({ trig = "tt", snippetType = "autosnippet" }, fmt("\\text{{{}}}", { i(1) }), { condition = cond.in_mathzone })
)

-- MATHCAL
table.insert(
  snippets,
  s({ trig = "cal", snippetType = "autosnippet" }, fmt("\\mathcal{{{}}}", { i(1) }), { condition = cond.in_mathzone })
)

-- MATHBB
table.insert(
  snippets,
  s({ trig = "bb", snippetType = "autosnippet" }, fmt("\\mathbb{{{}}}", { i(1) }), { condition = cond.in_mathzone })
)

-- BOLD TEXT
table.insert(
  snippets,
  s(
    { trig = "tbf", snippetType = "autosnippet" },
    fmt("\\textbf{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.not_in_mathzone }
  )
)

-- TEXT ITALIC
table.insert(
  snippets,
  s(
    { trig = "tit", snippetType = "autosnippet" },
    fmt("\\textit{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.not_in_mathzone }
  )
)

-- TEXTTT
table.insert(
  snippets,
  s(
    { trig = "ttt", snippetType = "autosnippet" },
    fmt("\\texttt{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.not_in_mathzone }
  )
)

-- MATHCAL SHORTCUT (e.g., cB -> \mathcal{B})
table.insert(
  snippets,
  s(
    { trig = "c([A-Z])", regTrig = true, snippetType = "autosnippet" },
    f(function(_, snip)
      return "\\mathcal{" .. snip.captures[1] .. "}"
    end),
    { condition = cond.in_mathzone }
  )
)

-- MATHBB SHORTCUT (e.g., bR -> \mathbb{R})
table.insert(
  snippets,
  s(
    { trig = "b([A-Z])", regTrig = true, snippetType = "autosnippet" },
    f(function(_, snip)
      return "\\mathbb{" .. snip.captures[1] .. "}"
    end),
    { condition = cond.in_mathzone }
  )
)
