local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local helpers = C.helpers
local s, i, fmt, f = C.P.s, C.P.i, C.P.fmt, C.P.f

local cond = C.cond
local snippets = C.snippets

local math_auto = {
  { "xx", "\\times" },
  { "neq", "\\neq" },
  { "nbla", "\\nabla" },
  { "nabla", "\\nabla" },
  { "inf", "\\infty" },
  { "cup", "\\cup" },
  { "cap", "\\cap" },
  { "subset", "\\subset" },
  { "supset", "\\supset" },
  { "subseteq", "\\subseteq" },
  { "supseteq", "\\supseteq" },
  { "mu", "\\mu" },
  { "pi", "\\pi" },
  { "inn", "\\in" },
  { "Pi", "\\Pi" },
  { "qq", "\\quad" },
  { "geq", "\\geq" },
  { "leq", "\\leq" },
  { "Delta", "\\Delta" },
  { "delta", "\\delta" },
  { "fa", "\\forall" },
  { "te", "\\exists" },
  { "pp", "\\prim" },
  { "iff", "\\iff" },
  { "imp", "\\implies" },
  { "ll", "\\ell" },
  { "tf", "\\therefore" },
  { "bc", "\\because" },
  { "approx", "\\approx" },
  { ":=", "\\coloneqq" },
  { "~", "\\sim" },
  { "sim", "\\sim" },
  { "...", "\\dots" },
  { "c.", "\\cdot" },
  { "cdot", "\\cdot" },
  { ">=", "\\geq" },
  { "<=", "\\leq" },
  { "->", "\\to" },
  { "<-", "\\leftarrow" },
  { "=>", "\\Rightarrow" },
  { "<=>", "\\Leftrightarrow" },
  { "|", "\\mid" },
  { "ln", "\\ln" },
  { "nin", "\\notin" },
  { "log", "\\log" },
}

-- for _, pair in ipairs(math_auto) do
--   table.insert(
--     snippets,
--     s({ trig = pair[1], snippetType = "autosnippet" }, { t(pair[2]) }, { condition = cond.in_mathzone })
--   )
-- end

for _, pair in ipairs(math_auto) do
  table.insert(
    snippets,
    s(
      { trig = pair[1], snippetType = "autosnippet" },
      { f(helpers.smart_space(pair[2])) },
      { condition = cond.in_mathzone }
    )
  )
end

table.insert(
  snippets,
  s(
    { trig = "exp", snippetType = "autosnippet" },
    fmt("\\exp(<>)", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

table.insert(
  snippets,
  s(
    { trig = "cancel", snippetType = "autosnippet" },
    fmt("\\cancel{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

table.insert(
  snippets,
  s(
    { trig = "sqrt", snippetType = "autosnippet" },
    fmt("\\sqrt{<>}", { i(1) }, { delimiters = "<>" }),
    { condition = cond.in_mathzone }
  )
)

local accents = {
  { trig = ":h", symbol = "\\hat" },
  { trig = ":b", symbol = "\\bar" },
  { trig = ":v", symbol = "\\vec" },
  { trig = ":t", symbol = "\\tilde" },
  { trig = ":d", symbol = "\\dot" },
  { trig = ":dd", symbol = "\\ddot" },
}

for _, acc in ipairs(accents) do
  table.insert(
    snippets,
    s(
      { trig = acc.trig, snippetType = "autosnippet" },
      fmt(acc.symbol .. "{<>}", { i(1) }, { delimiters = "<>" }),
      { condition = cond.in_mathzone }
    )
  )
end
