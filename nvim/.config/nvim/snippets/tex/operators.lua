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
  { "dqq", "\\qquad" },
  { "ge", "\\ge" },
  { "le", "\\le" },
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
      { condition = cond.in_plain_mathzone }
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

table.insert(
  snippets,
  s(
    { trig = "%f[%a]([A-Za-z])hat", regTrig = true, wordTrig = false, name = "variable hat", snippetType = "autosnippet" },
    { f(function(_, snip) return "\\hat{" .. snip.captures[1] .. "}" end) },
    { condition = cond.in_plain_mathzone }
  )
)

local accents = {
  { trig = [[\?hat]], symbol = "\\hat", regTrig = true, name = "hat" },
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
      { trig = acc.trig, regTrig = acc.regTrig, name = acc.name, snippetType = "autosnippet" },
      fmt(acc.symbol .. "{<>}", { i(1) }, { delimiters = "<>" }),
      { condition = cond.in_mathzone }
    )
  )
end

local rep = require("luasnip.extras").rep
table.insert(snippets, s(
  { trig = ",rm", name = "Stacked row operations", snippetType = "autosnippet", wordTrig = false },
  fmt([[\xrightarrow{\substack{
<>
}}]], { i(1) }, { delimiters = "<>" }),
  { condition = cond.in_mathzone }
))

local function row_arrow_context()
  local first = vim.fn.search([[\\substack\s*{]], "bnW")
  local stacked = false
  if first > 0 then
    local row, col = unpack(vim.fn.searchpairpos("{", "", "}", "bnW", "", first))
    stacked = row > 0 and vim.fn.getline(row):sub(1, col):match([[\substack%s*{$]]) ~= nil
  end
  return { env_override = { ROW_PREFIX = stacked and "" or [[\xrightarrow{]], ROW_SUFFIX = stacked and "" or "}" } }
end

local row_operations = {
  {
    trigger = ",rr",
    name = "Row replacement",
    label = [[R_{<>} \leftarrow R_{<>} <>R_{<>}]],
    fields = { i(1, "2"), rep(1), i(2, "- 3"), i(3, "1") },
  },
  {
    trigger = ",rs",
    name = "Row swap",
    label = [[R_{<>} \leftrightarrow R_{<>}]],
    fields = { i(1, "1"), i(2, "2") },
  },
  {
    trigger = ",rk",
    name = "Row scaling",
    label = [[R_{<>} \leftarrow <>R_{<>}]],
    fields = { i(1, "2"), i(2, "3"), rep(1) },
  },
}

for _, op in ipairs(row_operations) do
  local nodes = fmt(op.label, op.fields, { delimiters = "<>" })
  table.insert(nodes, 1, f(function(_, snip) return snip.env.ROW_PREFIX end))
  table.insert(nodes, f(function(_, snip) return snip.env.ROW_SUFFIX end))
  table.insert(snippets, s(
    { trig = op.trigger, name = op.name, snippetType = "autosnippet", wordTrig = false,
      resolveExpandParams = row_arrow_context },
    nodes,
    { condition = cond.in_mathzone }
  ))
end
