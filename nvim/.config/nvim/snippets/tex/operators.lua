local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t, i, fmt = C.P.s, C.P.t, C.P.i, C.P.fmt
local cond = C.cond
local snippets = C.snippets

local math_auto = {
  { "xx", "\\times " },
  { "neq", "\\neq " },
  { "nbla", "\\nabla " },
  { "mu", "\\mu " },
  { "pi", "\\pi " },
  { "in", "\\in " },
  { "Pi", "\\Pi " },
  { "...", "\\dots " },
  { "qq", "\\quad " },
  { "geq", "\\geq " },
  { "leq", "\\leq " },
  { "Delta", "\\Delta " },
  { "delta", "\\delta " },
  { "fa", "\\forall " },
  { "te", "\\exists " },
  { "pp", "\\prime" },
  { "iff", "\\iff " },
  { "imp", "\\implies " },
  { "ll", "\\ell " },
  { "tf", "\\therefore" },
  { "bc", "\\because " },
  { "approx", "\\approx " },
  { ":=", "\\coloneqq " },
  { "~", "\\sim " },
  { "sim", "\\sim " },
  { "c.", "\\cdot " },
  { "cdot", "\\cdot " },
  { ">=", "\\geq " },
  { "<=", "\\leq " },
  { "->", "\\to " },
  { "<-", "\\leftarrow " },
  { "=>", "\\Rightarrow " },
  { "<=>", "\\Leftrightarrow " },
  { "ln", "\\ln " },
  { "log", "\\log " },
}

for _, pair in ipairs(math_auto) do
  table.insert(
    snippets,
    s({ trig = pair[1], snippetType = "autosnippet" }, { t(pair[2]) }, { condition = cond.in_mathzone })
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
