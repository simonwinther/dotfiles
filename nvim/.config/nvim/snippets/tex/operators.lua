local C = ...
if type(C) ~= "table" or not C.snippets then
  return {}
end

local s, t = C.P.s, C.P.t
local cond = C.cond
local snippets = C.snippets

local math_auto = {
  { "xx", "\\times " },
  { "**", "\\cdot " },
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
}

for _, pair in ipairs(math_auto) do
  table.insert(
    snippets,
    s({ trig = pair[1], snippetType = "autosnippet" }, { t(pair[2]) }, { condition = cond.in_mathzone })
  )
end
